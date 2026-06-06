"""Whoop 4.0 packet -> annotated fields. The single source of decode knowledge.

parse_frame(frame: bytes) -> dict with:
  ok, type_name, seq, cmd_name, length, crc_ok,
  fields: [ {off, len, name, cat, value, raw, note} ],  (off/len in WHOLE-FRAME bytes)
  parsed: { ... flat decoded values ... }

Categories (cat) drive the UI color legend:
  frame time hr rr accel gyro ppg battery event meta text cmd unknown
"""
from __future__ import annotations
import struct, sys, zlib
sys.path.insert(0, "whoomp/scripts")
from packet import PacketType, CommandNumber, EventNumber, MetadataType  # noqa: E402

CATEGORIES = ["frame", "cmd", "time", "hr", "rr", "accel", "gyro", "ppg",
              "battery", "event", "meta", "text", "unknown"]


def _name(enum, val):
    try:
        return f"{enum(val).name}({val})"
    except ValueError:
        return f"0x{val:02X}({val})"


def _u(b, o, n):
    return int.from_bytes(b[o:o+n], "little") if o+n <= len(b) else None


def _i16(b, off, n):
    """Read up to n signed int16 LE starting at byte off."""
    cnt = max(0, min(n, (len(b) - off) // 2))
    return list(struct.unpack(f"<{cnt}h", b[off:off + cnt*2])) if cnt else []


class FB:
    """Field builder: accumulates annotated fields over a frame."""
    def __init__(self, frame):
        self.frame = frame
        self.fields = []
        self.parsed = {}

    def add(self, off, length, name, cat, value=None, note=None):
        raw = self.frame[off:off+length]
        self.fields.append({
            "off": off, "len": length, "name": name, "cat": cat,
            "value": value, "raw": raw.hex(), "note": note,
        })
        if value is not None and cat not in ("frame", "unknown"):
            self.parsed[name] = value
        return self

    def region(self, start, end, name, cat, note=None):
        if start < end <= len(self.frame):
            self.add(start, end - start, name, cat, value=f"[{end-start} bytes]", note=note)


def parse_frame(frame: bytes) -> dict:
    out = {"ok": False, "raw": frame.hex(), "len_bytes": len(frame)}
    if len(frame) < 8 or frame[0] != 0xAA:
        out["type_name"] = "INVALID/FRAGMENT"
        return out
    length = struct.unpack("<H", frame[1:3])[0]
    inner = frame[4:length]
    if len(inner) < 3:
        out["type_name"] = "SHORT"
        return out
    t, seq, cmd = inner[0], inner[1], inner[2]
    try:
        tname = PacketType(t).name
    except ValueError:
        tname = f"type{t}"
    out["type_name"] = tname
    out["seq"] = seq
    # crc check
    crc_ok = None
    if length + 4 <= len(frame):
        calc = zlib.crc32(inner) & 0xFFFFFFFF
        want = struct.unpack("<L", frame[length:length+4])[0]
        crc_ok = (calc == want)
    out["crc_ok"] = crc_ok

    fb = FB(frame)
    # --- common frame envelope ---
    fb.add(0, 1, "SOF", "frame", "0xAA")
    fb.add(1, 2, "length", "frame", length)
    fb.add(3, 1, "crc8", "frame", f"0x{frame[3]:02X}")
    fb.add(4, 1, "packet_type", "frame", _name(PacketType, t))

    T = PacketType
    # REALTIME_DATA(40): ts starts at the cmd slot (frame[6]); whoomp hack
    if t == T.REALTIME_DATA.value:
        fb.add(5, 1, "seq", "frame", seq)
        ts = _u(frame, 6, 4); ss = _u(frame, 10, 2); hr = _u(frame, 12, 1); rrn = _u(frame, 13, 1)
        fb.add(6, 4, "timestamp", "time", ts)
        fb.add(10, 2, "subseconds", "time", ss)
        fb.add(12, 1, "heart_rate", "hr", hr, "bpm")
        fb.add(13, 1, "rr_count", "rr", rrn)
        rrs = []
        for i in range(rrn or 0):
            v = _u(frame, 14 + i*2, 2)
            if v is not None:
                fb.add(14 + i*2, 2, f"rr[{i}]", "rr", v, "ms (1/1000s? confirm)")
                rrs.append(v)
        fb.parsed["rr_intervals"] = rrs

    # REALTIME_RAW_DATA(43) / HISTORICAL_DATA(47): IMU + PPG payload
    elif t in (T.REALTIME_RAW_DATA.value, T.HISTORICAL_DATA.value):
        fb.add(5, 1, "seq", "frame", seq)
        fb.add(6, 1, "cmd", "cmd", cmd)
        # record header (parser.py: data[4:15]=<LHLB>, data=frame[7:])
        fb.add(7, 4, "record_hdr", "meta", _u(frame, 7, 4))
        fb.add(11, 4, "timestamp", "time", _u(frame, 11, 4), "device epoch")
        fb.add(15, 2, "subseconds", "time", _u(frame, 15, 2))
        fb.add(17, 4, "unknown_hdr", "meta", _u(frame, 17, 4))
        hr = _u(frame, 21, 1); rrn = _u(frame, 22, 1)
        # subtype-10 IMU packet (confirmed empirically against captured frames).
        # Offsets are data_hex+7 -> frame coords. Each axis = 100 signed int16 LE.
        # 1g ≈ ~3100-3900 LSB (empirical; app applies NO scale, raw counts to cloud).
        data_len = length - 7
        if data_len == 1921:  # OPTICAL/PPG companion packet (24-bit channels)
            fb.add(11, 4, "timestamp", "time", _u(frame, 11, 4), "device epoch")
            fb.region(40, length, "RAW OPTICAL / PPG", "ppg",
                      "~4 photodiode channels, 24-bit LE interleaved; one is a smooth pulsatile PPG "
                      "(autocorr 0.96). Green=HR, Red+IR→SpO2 (computed cloud-side). Not parsed by app.")
            fb.parsed["packet"] = "raw optical/PPG (24-bit ×~4ch)"
        elif data_len == 1917:  # IMU accel+gyro (APK-confirmed)
            fb.add(21, 1, "heart_rate", "hr", hr, "bpm")
            fb.add(22, 1, "rr_count", "rr", rrn)
            for i in range(min(rrn or 0, 4)):
                fb.add(23 + i*2, 2, f"rr[{i}]", "rr", _u(frame, 23 + i*2, 2), "ms")
            AX = [("accelX", 89, "accel"), ("accelY", 289, "accel"), ("accelZ", 489, "accel"),
                  ("gyroX", 692, "gyro"), ("gyroY", 892, "gyro"), ("gyroZ", 1092, "gyro")]
            for name, off, cat in AX:
                vals = _i16(frame, off, 100)
                mean = round(sum(vals)/len(vals), 1) if vals else None
                fb.add(off, 200, name, cat,
                       f"μ={mean} ({len(vals)}×i16)" if vals else None,
                       "100 samples/axis @52Hz · signed int16 LE · raw LSB (no scale in app)")
                if mean is not None:
                    fb.parsed[f"{name}_mean"] = mean
            fb.region(1292, length, "tail (optical? — not parsed by app)", "unknown")
        else:
            fb.region(21, length, "sensor payload (short/alt subtype)", "unknown")

    # EVENT(48)
    elif t == T.EVENT.value:
        fb.add(5, 1, "seq", "frame", seq)
        fb.add(6, 1, "event", "event", _name(EventNumber, cmd))
        ts = _u(frame, 8, 4)
        if ts is not None:
            fb.add(8, 4, "event_timestamp", "time", ts)
        try:
            ev = EventNumber(cmd)
        except ValueError:
            ev = None
        if ev in (EventNumber.BATTERY_LEVEL, EventNumber.EXTENDED_BATTERY_INFORMATION):
            # payload tail: SOC*10 and mV observed (offsets within payload vary)
            pay = frame[7:length]
            fb.region(7, length, f"{ev.name} payload", "battery", "SOC(×10) + mV inside")
            # best-effort soc/mv (refine): look for plausible mV ~3000-4300
            for o in range(0, len(pay)-1):
                v = int.from_bytes(pay[o:o+2], "little")
                if 3000 <= v <= 4300:
                    fb.parsed["battery_mV?"] = v; break

    # COMMAND_RESPONSE(36)
    elif t == T.COMMAND_RESPONSE.value:
        fb.add(5, 1, "seq", "frame", seq)
        fb.add(6, 1, "resp_cmd", "cmd", _name(CommandNumber, cmd))
        pay = frame[7:length]
        fb.region(7, length, "response payload", "cmd")
        try:
            cn = CommandNumber(cmd)
        except ValueError:
            cn = None
        if cn == CommandNumber.GET_BATTERY_LEVEL and len(pay) >= 4:
            fb.parsed["battery_pct"] = struct.unpack("<H", pay[2:4])[0] / 10
        elif cn == CommandNumber.GET_CLOCK and len(pay) >= 6:
            fb.parsed["clock"] = struct.unpack("<L", pay[2:6])[0]
        elif cn == CommandNumber.REPORT_VERSION_INFO and len(pay) >= 31:
            u = struct.unpack("<BBBLLLLLLLL", pay[:35] if len(pay) >= 35 else pay[:31].ljust(35, b"\0"))
            fb.parsed["fw_harvard"] = f"{u[3]}.{u[4]}.{u[5]}.{u[6]}"
            fb.parsed["fw_boylston"] = f"{u[7]}.{u[8]}.{u[9]}.{u[10]}"
        elif cn == CommandNumber.GET_EXTENDED_BATTERY_INFO and len(pay) >= 9:
            fb.parsed["battery_mV"] = struct.unpack("<H", pay[7:9])[0]
        elif cn == CommandNumber.GET_DATA_RANGE:
            # embedded real-unix timestamps mark the stored-history window
            uniq = []
            for o in range(3, len(pay) - 3):
                v = struct.unpack("<I", pay[o:o+4])[0]
                if 1_600_000_000 <= v <= 1_800_000_000 and v not in uniq:
                    uniq.append(v)
            if uniq:
                import datetime as _dt
                lo, hi = min(uniq), max(uniq)
                fb.parsed["history_oldest"] = _dt.datetime.fromtimestamp(lo, _dt.timezone.utc).strftime("%Y-%m-%d %H:%M UTC")
                fb.parsed["history_newest"] = _dt.datetime.fromtimestamp(hi, _dt.timezone.utc).strftime("%Y-%m-%d %H:%M UTC")

    # METADATA(49)
    elif t == T.METADATA.value:
        fb.add(5, 1, "seq", "frame", seq)
        fb.add(6, 1, "meta_type", "meta", _name(MetadataType, cmd))
        pay = frame[7:length]
        if len(pay) >= 14:
            unix, ss, unk0, trim = struct.unpack("<LHLL", pay[:14])
            fb.add(7, 4, "unix", "time", unix)
            fb.add(11, 2, "subsec", "time", ss)
            fb.add(13, 4, "unk0", "meta", unk0)
            fb.add(17, 4, "trim_cursor", "meta", trim, "ack with this to advance")

    # CONSOLE_LOGS(50)
    elif t == T.CONSOLE_LOGS.value:
        fb.add(5, 1, "seq", "frame", seq)
        try:
            txt = frame[11:length-1].decode("utf-8", "replace")
        except Exception:
            txt = ""
        fb.region(7, length, "console log text", "text", txt[:80])
        fb.parsed["log"] = txt

    else:
        fb.add(5, 1, "seq", "frame", seq)
        fb.add(6, 1, "cmd", "cmd", cmd)
        fb.region(7, length, "payload", "unknown")

    # crc32 trailer
    if length + 4 <= len(frame):
        fb.add(length, 4, "crc32", "frame", f"0x{struct.unpack('<L', frame[length:length+4])[0]:08X}",
               "OK" if crc_ok else "MISMATCH")

    out["ok"] = True
    out["fields"] = fb.fields
    out["parsed"] = fb.parsed
    out["cmd_name"] = _name(CommandNumber, cmd) if t in (T.COMMAND.value, T.COMMAND_RESPONSE.value) else None
    return out
