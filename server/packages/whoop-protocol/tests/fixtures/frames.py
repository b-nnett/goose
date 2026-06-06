"""SYNTHETIC WHOOP 4.0 frames for the decoder test-suite.

Every frame here is built in-process from obviously-fake but protocol-valid values via the
canonical framing helpers — NO real on-device capture is embedded, and nothing reads any
private capture file, so the package is self-contained and safe to open-source.

The frames decode to the same shapes the tests assert (REALTIME_DATA hr=60 @ ts 31538447,
EVENT RAW_DATA_COLLECTION_ON, COMMAND_RESPONSE GET_BATTERY_LEVEL 25.5%, a type-43 IMU
frame). The values are chosen to be clean and clearly synthetic.
"""
import struct
import zlib

from whoop_protocol.framing import crc8


def _build_frame(type_byte: int, seq: int, body: bytes) -> bytes:
    """Assemble a complete, CRC-valid frame from `body` (= frame bytes at offset 6+).
    Frame = [0xAA][len u16 LE][crc8(len)][type][seq][body...][crc32 u32 LE]."""
    inner = bytes([type_byte & 0xFF, seq & 0xFF]) + body
    length = len(inner) + 4
    header = bytes([0xAA]) + struct.pack("<H", length) + bytes([crc8(struct.pack("<H", length))])
    return header + inner + struct.pack("<L", zlib.crc32(inner) & 0xFFFFFFFF)


def _realtime_data(seq, ts, hr, subsec=0, rr=()):
    body = bytearray()
    body += struct.pack("<I", ts)        # timestamp @6
    body += struct.pack("<H", subsec)    # subseconds @10
    body += bytes([hr & 0xFF])           # heart_rate @12
    body += bytes([len(rr)])             # rr_count @13
    for v in rr:                         # rr @14...
        body += struct.pack("<H", v)
    if len(body) < 18:
        body += bytes(18 - len(body))
    return _build_frame(40, seq, bytes(body))


def _event(seq, event_num, event_ts):
    body = bytes([event_num & 0xFF, 0x00]) + struct.pack("<I", event_ts)  # event@6, ts@8
    return _build_frame(48, seq, body)


def _cmd_response_battery(seq, soc_pct):
    pay = bytearray(8)
    pay[2:4] = struct.pack("<H", int(round(soc_pct * 10)))  # soc*10 @ pay[2:4]
    return _build_frame(36, seq, bytes([26]) + bytes(pay))   # resp_cmd=GET_BATTERY_LEVEL(26)


def _raw_imu(seq, ts, hr, rr=()):
    """type-43 IMU variant (data_len == 1917). Constant +1g on the Z accel axis."""
    body = bytearray(1918)               # frame offsets 6..1923 -> data_len 1917
    body[1:5] = struct.pack("<I", 0xCAFEBABE)   # record_hdr @7
    body[5:9] = struct.pack("<I", ts)           # timestamp @11
    body[15] = hr & 0xFF                         # heart_rate @21
    body[16] = len(rr)                           # rr_count @22
    for i, v in enumerate(rr[:4]):
        body[17 + i * 2:19 + i * 2] = struct.pack("<H", v)
    for i in range(100):                         # accelZ @489 ~ +1g (1/4096 LSB/g)
        body[489 - 6 + i * 2:489 - 6 + i * 2 + 2] = struct.pack("<h", 4096)
    return _build_frame(43, seq, bytes(body))


# REALTIME_DATA(40): seq=2, ts=31538447, subsec=26152, hr=60, rr_count=0
REALTIME_DATA_HR60 = _realtime_data(seq=2, ts=31538447, hr=60, subsec=26152)

# EVENT(48): RAW_DATA_COLLECTION_ON(46), event_ts=0x677ED619
EVENT_RAW_ON = _event(seq=38, event_num=46, event_ts=0x677ED619)

# COMMAND_RESPONSE(36): GET_BATTERY_LEVEL(26) -> 25.5%
CMD_RESP_BATTERY = _cmd_response_battery(seq=35, soc_pct=25.5)

# type-43 IMU frame (synthetic). OPTICAL is left None: the legacy optical assertion expects
# a `packet` key the current schema no longer emits, so that test stays skipped (as it did
# when the private capture was absent).
IMU_FRAME = _raw_imu(seq=5, ts=31538456, hr=63)
OPTICAL_FRAME = None
