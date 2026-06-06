"""Schema-driven WHOOP frame interpreter. Generic field walk + per-type post-hooks.
Output is compatible with the legacy dashboard whoop_fields.parse_frame."""
import struct

from .framing import verify_frame
from .schema import load_schema

_DTYPE = {
    "u8": ("<B", 1), "u16": ("<H", 2), "u32": ("<L", 4),
    "i16": ("<h", 2), "f32": ("<f", 4),
}

# post-hook registry: name -> callable(fb, frame, length, schema)
_POST_HOOKS = {}


def _hook(name):
    def deco(fn):
        _POST_HOOKS[name] = fn
        return fn
    return deco


class FB:
    """Field builder: accumulates annotated fields and a flat parsed dict."""
    def __init__(self, frame):
        self.frame = frame
        self.fields = []
        self.parsed = {}

    def add(self, off, length, name, cat, value=None, note=None):
        raw = self.frame[off:off + length]
        self.fields.append({"off": off, "len": length, "name": name, "cat": cat,
                            "value": value, "raw": raw.hex(), "note": note})
        if value is not None and cat not in ("frame", "unknown"):
            self.parsed[name] = value
        return self

    def region(self, start, end, name, cat, note=None):
        if start < end <= len(self.frame):
            self.add(start, end - start, name, cat, value=f"[{end-start} bytes]", note=note)


def _read(frame, off, dtype):
    if dtype == "s24":  # signed 24-bit little-endian (no struct format for 3 bytes)
        if off + 3 > len(frame):
            return None
        v = frame[off] | (frame[off + 1] << 8) | (frame[off + 2] << 16)
        return v - 0x1000000 if v & 0x800000 else v
    spec = _DTYPE.get(dtype)
    if spec is None:
        # Unknown scalar dtype (e.g. "bytes"/"ascii" payload regions): no scalar value,
        # mirroring the Swift readDType() default-nil. The static-field walk skips these
        # (the bytes/ascii regions are surfaced by per-type post-hooks instead).
        return None
    fmt, n = spec
    if off + n > len(frame):
        return None
    return struct.unpack(fmt, frame[off:off + n])[0]


def parse_frame(frame: bytes) -> dict:
    out = {"ok": False, "raw": frame.hex(), "len_bytes": len(frame)}
    if len(frame) < 8 or frame[0] != 0xAA:
        out["type_name"] = "INVALID/FRAGMENT"
        return out

    schema = load_schema()
    check = verify_frame(frame)
    length = check.length
    out["crc_ok"] = check.crc32_ok

    t = frame[4]
    out["type_name"] = schema.type_name(t)
    out["seq"] = frame[5]

    fb = FB(frame)
    # envelope
    fb.add(0, 1, "SOF", "frame", "0xAA")
    fb.add(1, 2, "length", "frame", length)
    fb.add(3, 1, "crc8", "frame", f"0x{frame[3]:02X}")
    fb.add(4, 1, "packet_type", "frame", schema.type_name(t))
    fb.add(5, 1, "seq", "frame", frame[5])

    spec = schema.packet_for_type(t)
    if spec is None:
        fb.add(6, 1, "cmd", "cmd", frame[6] if len(frame) > 6 else None)
        fb.region(7, length, "payload", "unknown")
    else:
        # static fields from schema
        for fld in spec.get("fields", []):
            off, ln, dtype = fld["off"], fld["len"], fld.get("dtype")
            if dtype is None:
                continue
            val = _read(frame, off, dtype)
            if val is None:
                continue
            if "enum" in fld:
                val = schema.enum_name(fld["enum"], val)
            fb.add(off, ln, fld["name"], fld["cat"], val, fld.get("note"))
        # per-type post-hook for irregular fields
        hook = _POST_HOOKS.get(spec.get("post"))
        if hook is not None:
            hook(fb, frame, length, schema)

    # crc32 trailer field
    if length is not None and length + 4 <= len(frame):
        crc_val = struct.unpack("<L", frame[length:length + 4])[0]
        fb.add(length, 4, "crc32", "frame", f"0x{crc_val:08X}",
               "OK" if check.crc32_ok else "MISMATCH")

    cmd_byte = frame[6] if len(frame) > 6 else 0
    out["cmd_name"] = schema.enum_name("CommandNumber", cmd_byte) if t in (35, 36) else None
    out["ok"] = True
    out["fields"] = fb.fields
    out["parsed"] = fb.parsed
    return out


@_hook("realtime_data")
def _post_realtime_data(fb, frame, length, schema):
    rrn = _read(frame, 13, "u8") or 0
    rrs = []
    for i in range(rrn):
        v = _read(frame, 14 + i * 2, "u16")
        if v is not None:
            fb.add(14 + i * 2, 2, f"rr[{i}]", "rr", v, "ms")
            rrs.append(v)
    fb.parsed["rr_intervals"] = rrs


@_hook("event")
def _post_event(fb, frame, length, schema):
    ev_val = frame[6] if len(frame) > 6 else None
    ev_name = schema.enums.get("EventNumber", {}).get(str(ev_val))
    if ev_name == "BATTERY_LEVEL":
        # Fixed layout, empirically verified against captured frames (and matching the
        # Payload-slice offsets 1/5/10, once our 4-byte SOF/len/crc8
        # prefix + the u32 event_timestamp@8 are accounted for). The strap emits this event
        # autonomously every ~8 min, so decoding it gives a DENSE battery series — unlike the
        # rare GET_BATTERY_LEVEL command response.
        #   soc%      = u16 @ 17 / 10   (e.g. 0x00dc=220 -> 22.0%)
        #   mV        = u16 @ 21        (e.g. 0x0f24=3876)
        #   charging  = u8  @ 26 bit0
        fb.region(7, length, "BATTERY_LEVEL payload", "battery", "soc@17(/10) mv@21 charge@26")
        soc = _read(frame, 17, "u16")
        if soc is not None and 0 <= soc <= 1100:
            fb.parsed["battery_pct"] = soc / 10
        mv = _read(frame, 21, "u16")
        if mv is not None and 3000 <= mv <= 4300:
            fb.parsed["battery_mV"] = mv
        ch = _read(frame, 26, "u8")
        if ch is not None and ch in (0, 1):
            fb.parsed["battery_charging"] = ch & 1
    elif ev_name == "EXTENDED_BATTERY_INFORMATION":
        # The WHOOP app does NOT decode this event's body; keep the heuristic mV scan only.
        pay = frame[7:length]
        fb.region(7, length, f"{ev_name} payload", "battery", "mV (heuristic scan)")
        for o in range(len(pay) - 1):
            v = int.from_bytes(pay[o:o + 2], "little")
            if 3000 <= v <= 4300:
                fb.parsed["battery_mV?"] = v
                break


@_hook("command_response")
def _post_command_response(fb, frame, length, schema):
    pay = frame[7:length]
    fb.region(7, length, "response payload", "cmd")
    cmd = frame[6] if len(frame) > 6 else None
    name = schema.enums.get("CommandNumber", {}).get(str(cmd))
    if name == "GET_BATTERY_LEVEL" and len(pay) >= 4:
        fb.parsed["battery_pct"] = struct.unpack("<H", pay[2:4])[0] / 10
    elif name == "GET_CLOCK" and len(pay) >= 6:
        fb.parsed["clock"] = struct.unpack("<L", pay[2:6])[0]
    elif name == "GET_EXTENDED_BATTERY_INFO" and len(pay) >= 9:
        fb.parsed["battery_mV"] = struct.unpack("<H", pay[7:9])[0]
    elif name == "REPORT_VERSION_INFO" and len(pay) >= 31:
        # "<BBBLLLLLLLL" is 3 + 8*4 = 35 bytes; pad short payloads to 35 and unpack the
        # whole buffer (slicing to buf[:31] was a porting bug -> struct.error on live data).
        buf = pay[:35] if len(pay) >= 35 else pay[:31].ljust(35, b"\0")
        u = struct.unpack("<BBBLLLLLLLL", buf)
        fb.parsed["fw_harvard"] = f"{u[3]}.{u[4]}.{u[5]}.{u[6]}"
        fb.parsed["fw_boylston"] = f"{u[7]}.{u[8]}.{u[9]}.{u[10]}"
    elif name == "GET_DATA_RANGE":
        import datetime as _dt
        uniq = []
        for o in range(3, len(pay) - 3):
            v = struct.unpack("<I", pay[o:o + 4])[0]
            if 1_600_000_000 <= v <= 1_800_000_000 and v not in uniq:
                uniq.append(v)
        if uniq:
            fb.parsed["history_oldest"] = _dt.datetime.fromtimestamp(
                min(uniq), _dt.timezone.utc).strftime("%Y-%m-%d %H:%M UTC")
            fb.parsed["history_newest"] = _dt.datetime.fromtimestamp(
                max(uniq), _dt.timezone.utc).strftime("%Y-%m-%d %H:%M UTC")


def _i16_block(frame, off, count):
    end = off + count * 2
    if end > len(frame):
        count = max(0, (len(frame) - off) // 2)
        end = off + count * 2
    return list(struct.unpack(f"<{count}h", frame[off:end])) if count else []


@_hook("raw_data")
def _post_raw_data(fb, frame, length, schema):
    spec = schema.packet_for_type(frame[4])
    data_len = length - 7
    variant = spec.get("variants", {}).get(str(data_len))
    if variant is None:
        fb.region(21, length, "sensor payload (short/alt subtype)", "unknown")
        return
    if variant["kind"] == "imu":
        hr = _read(frame, variant["hr_off"], "u8")
        rrn = _read(frame, variant["rr_count_off"], "u8") or 0
        fb.add(variant["hr_off"], 1, "heart_rate", "hr", hr, "bpm")
        fb.add(variant["rr_count_off"], 1, "rr_count", "rr", rrn)
        rr_vals = []
        for i in range(min(rrn, 4)):
            off = variant["rr_first_off"] + i * 2
            v = _read(frame, off, "u16")
            fb.add(off, 2, f"rr[{i}]", "rr", v, "ms")
            if v is not None:
                rr_vals.append(v)
        fb.parsed["heart_rate"] = hr
        fb.parsed["rr_intervals"] = rr_vals
        samples = variant["samples"]
        for name, off, cat in variant["axes"]:
            vals = _i16_block(frame, off, samples)
            mean = round(sum(vals) / len(vals), 1) if vals else None
            fb.add(off, samples * 2, name, cat,
                   f"mean={mean} ({len(vals)}xi16)" if vals else None, variant["note"])
            if mean is not None:
                fb.parsed[f"{name}_mean"] = mean
        fb.region(variant["tail_from"], length, "tail (optical? - not parsed by app)", "unknown")
    elif variant["kind"] == "optical":
        fb.region(variant["config_from"], variant["ppg_off"],
                  "optical config header (UNKNOWN)", "unknown", variant["note"])
        off, stride, n = variant["ppg_off"], variant["ppg_stride"], variant["ppg_samples"]
        vals = []
        for i in range(n):
            v = _read(frame, off + i * stride, "s24")
            if v is None:
                break
            vals.append(v)
        if vals:
            mean = round(sum(vals) / len(vals), 1)
            fb.add(off, len(vals) * stride, "ppg_green_ac", "ppg",
                   f"mean={mean} ({len(vals)}xs24)", variant["note"])
            fb.parsed["ppg_sample_count"] = len(vals)
            fb.parsed["ppg_mean"] = mean


def _resolve_version(versions, version):
    """Pick the layout for a type-47 version byte, following a `ref` chain (V12 -> V24)."""
    entry = versions.get(str(version))
    if entry is None:
        return None
    seen = set()
    while "ref" in entry and entry["ref"] not in seen:
        seen.add(entry["ref"])
        base = dict(versions.get(entry["ref"], {}))
        base.update({k: v for k, v in entry.items() if k != "ref"})
        entry = base
    return entry


@_hook("historical_data")
def _post_historical_data(fb, frame, length, schema):
    """type-47 HISTORICAL_DATA: the 14-day biometric store. Version = seq byte (frame[5]).
    V24/V12 carry the full DSP record; V5/V7/V9 are HR/RR-only generic records."""
    spec = schema.packet_for_type(frame[4])
    version = frame[5]
    fb.parsed["hist_version"] = version
    entry = _resolve_version(spec.get("versions", {}), version)
    if entry is None:
        fb.region(7, length, f"HISTORICAL_DATA v{version} (unmapped layout)", "unknown")
        return
    for fld in entry.get("fields", []):
        dtype = fld.get("dtype")
        if dtype is None:
            continue
        val = _read(frame, fld["off"], dtype)
        if val is None:
            continue
        fb.add(fld["off"], fld["len"], fld["name"], fld["cat"], val, fld.get("note"))
    rr_first = entry.get("rr_first_off")
    rr_vals = []
    if rr_first is not None:
        rrn = fb.parsed.get("rr_count") or 0
        for i in range(min(rrn, 4)):
            o = rr_first + i * 2
            v = _read(frame, o, "u16")
            if v:
                fb.add(o, 2, f"rr[{i}]", "rr", v, "ms")
                rr_vals.append(v)
    fb.parsed["rr_intervals"] = rr_vals


@_hook("metadata")
def _post_metadata(fb, frame, length, schema):
    pay = frame[7:length]
    if len(pay) >= 14:
        unix, ss, unk0, trim = struct.unpack("<LHLL", pay[:14])
        fb.add(7, 4, "unix", "time", unix)
        fb.add(11, 2, "subsec", "time", ss)
        fb.add(13, 4, "unk0", "meta", unk0)
        fb.add(17, 4, "trim_cursor", "meta", trim, "ack with this to advance")


@_hook("console_logs")
def _post_console_logs(fb, frame, length, schema):
    try:
        txt = frame[11:length - 1].decode("utf-8", "replace")
    except Exception:
        txt = ""
    fb.region(7, length, "console log text", "text", txt[:80])
    fb.parsed["log"] = txt


def _to_wall(device_ts, device_clock_ref, wall_clock_ref):
    """Map a device-epoch timestamp to wall-clock unix seconds via a linear offset.

    Assumes the strap clock and wall clock tick at the same rate (pure offset, no
    skew/drift correction). Fine for v1 and short batches; drift correction is deferred.
    """
    if device_ts is None:
        return None
    return wall_clock_ref + (device_ts - device_clock_ref)


def extract_streams(parsed_results, device_clock_ref, wall_clock_ref):
    """Turn parsed frames into datastore rows. Returns {hr, rr, events, battery}, each a
    list of dicts with a wall-clock `ts` (unix seconds). Raw IMU/optical is NOT exploded
    here (archived separately, decoded later in sub-project B).

    HR/R-R are taken ONLY from REALTIME_DATA (type 40), the canonical HR stream.
    REALTIME_RAW_DATA (type 43) also carries an HR byte, but type-40 frames stream
    alongside type-43 during raw collection, so routing both would double-count HR for
    the same instants. The type-43 HR remains recoverable from the raw archive.

    CRC-failed frames are skipped (crc_ok is False) — a decoder fed untrusted BLE bytes
    should not emit datastore rows from a frame that failed its checksum.
    """
    out = {"hr": [], "rr": [], "events": [], "battery": []}
    for r in parsed_results:
        if not r.get("ok") or r.get("crc_ok") is False:
            continue
        p = r["parsed"]
        tname = r["type_name"]
        if tname == "REALTIME_DATA":
            ts = _to_wall(p.get("timestamp"), device_clock_ref, wall_clock_ref)
            if ts is not None:
                if "heart_rate" in p:
                    out["hr"].append({"ts": ts, "bpm": p["heart_rate"]})
                for rr in p.get("rr_intervals", []):
                    out["rr"].append({"ts": ts, "rr_ms": rr})
        elif tname == "EVENT":
            # EVENT timestamps are real RTC unix seconds (the device's wall clock),
            # NOT the monotonic device epoch that REALTIME_DATA uses. They are already
            # wall-clock, so they must NOT be offset by the device->wall correlation
            # (doing so threw event rows ~50 years into the future).
            ts = p.get("event_timestamp")
            payload = {k: v for k, v in p.items() if k not in ("event", "event_timestamp")}
            out["events"].append({"ts": ts, "kind": p.get("event"), "payload": payload})
            # BATTERY_LEVEL events (every ~8 min) carry SoC/mV/charging + a real RTC ts →
            # the DENSE battery series. _post_event decoded the fields above.
            if (p.get("event") or "").split("(")[0] == "BATTERY_LEVEL" \
                    and ("battery_pct" in p or "battery_mV" in p):
                out["battery"].append(_battery_row(ts, p))
        elif tname == "COMMAND_RESPONSE":
            # COMMAND_RESPONSE carries no device timestamp, so battery rows are stamped
            # with wall_clock_ref (the batch's correlation instant). A2 may prefer the
            # batch capture time if it differs from wall_clock_ref.
            if "battery_pct" in p or "battery_mV" in p:
                out["battery"].append(_battery_row(wall_clock_ref, p))
    return out


def _battery_row(ts, p):
    """A battery sample dict from a parsed frame's fields. charging is a real bool when the
    frame carried it (BATTERY_LEVEL events) else None (command responses don't report it)."""
    ch = p.get("battery_charging")
    return {"ts": ts, "soc": p.get("battery_pct"), "mv": p.get("battery_mV"),
            "charging": (bool(ch) if ch is not None else None)}


def extract_historical_streams(parsed_results, device_clock_ref, wall_clock_ref):
    """Historical offload streams. Two distinct sources:

    - type-47 HISTORICAL_DATA (the 14-day biometric store, V24/V12): carries a REAL unix
      timestamp plus the full DSP record (HR, R-R, SpO2 raw, skin-temp raw, resp raw,
      gravity vector). This is the canonical biometric history.
    - type-43 REALTIME_RAW_DATA: live-contamination frames replayed during an old-style
      offload; HR/R-R from the header, device-epoch timestamp (offset to wall clock).

    SpO2/skin-temp/resp are RAW ADC values — WHOOP computes the human-unit values in the
    cloud; no client-side conversion exists, so they are emitted raw and flagged.
    Returns {hr, rr, spo2, skin_temp, resp, gravity, events, battery}; each ts is unix sec.
    """
    out = {"hr": [], "rr": [], "spo2": [], "skin_temp": [], "resp": [],
           "gravity": [], "events": [], "battery": []}
    for r in parsed_results:
        if not r.get("ok") or r.get("crc_ok") is False:
            continue
        p = r["parsed"]
        tname = r["type_name"]
        if tname == "HISTORICAL_DATA":
            ts = p.get("unix")
            if ts is None:
                continue
            if p.get("heart_rate"):  # skip startup hr=0
                out["hr"].append({"ts": ts, "bpm": p["heart_rate"]})
            for rr in p.get("rr_intervals", []):
                out["rr"].append({"ts": ts, "rr_ms": rr})
            if "spo2_red" in p:
                out["spo2"].append({"ts": ts, "red": p.get("spo2_red"), "ir": p.get("spo2_ir"),
                                    "unit": "raw_adc"})
            if "skin_temp_raw" in p:
                out["skin_temp"].append({"ts": ts, "raw": p["skin_temp_raw"], "unit": "raw_adc"})
            if "resp_rate_raw" in p:
                out["resp"].append({"ts": ts, "raw": p["resp_rate_raw"], "unit": "raw_adc"})
            if "gravity_x" in p:
                out["gravity"].append({"ts": ts, "x": p.get("gravity_x"),
                                       "y": p.get("gravity_y"), "z": p.get("gravity_z"), "unit": "g"})
        elif tname == "REALTIME_RAW_DATA":
            ts = _to_wall(p.get("timestamp"), device_clock_ref, wall_clock_ref)
            if ts is not None:
                if "heart_rate" in p:
                    out["hr"].append({"ts": ts, "bpm": p["heart_rate"]})
                for rr in p.get("rr_intervals", []):
                    out["rr"].append({"ts": ts, "rr_ms": rr})
        elif tname == "EVENT":
            # EVENT timestamps are real RTC unix seconds (device wall clock), already
            # wall-clock — must NOT be offset by device->wall correlation.
            ts = p.get("event_timestamp")
            payload = {k: v for k, v in p.items() if k not in ("event", "event_timestamp")}
            out["events"].append({"ts": ts, "kind": p.get("event"), "payload": payload})
            if (p.get("event") or "").split("(")[0] == "BATTERY_LEVEL" \
                    and ("battery_pct" in p or "battery_mV" in p):
                out["battery"].append(_battery_row(ts, p))
        elif tname == "COMMAND_RESPONSE":
            if "battery_pct" in p or "battery_mV" in p:
                out["battery"].append(_battery_row(wall_clock_ref, p))
    return out
