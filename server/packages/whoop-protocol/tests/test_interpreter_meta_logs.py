import struct
from whoop_protocol.interpreter import parse_frame
from whoop_protocol.framing import crc8, crc32


def _build(type_byte, seq, body):
    """Build a valid frame: [AA][len][crc8][type][seq][body][crc32]."""
    inner = bytes([type_byte, seq]) + body
    length = len(inner) + 4
    blen = struct.pack("<H", length)
    head = bytes([0xAA]) + blen + bytes([crc8(blen)])
    pkt = head + inner
    return pkt + struct.pack("<L", crc32(pkt[4:length]))


def test_metadata_history_end_trim_cursor():
    # meta_type=2 (HISTORY_END) at frame[6]; payload<LHLL> from frame[7]
    body = bytes([2]) + struct.pack("<LHLL", 1_700_000_000, 5, 0, 4242)
    out = parse_frame(_build(49, 7, body))
    assert out["type_name"] == "METADATA"
    assert out["parsed"]["trim_cursor"] == 4242
    assert out["parsed"]["unix"] == 1_700_000_000


def test_console_logs_text():
    body = bytes([0, 0, 0, 0, 0]) + b"hello strap\x00"
    out = parse_frame(_build(50, 1, body))
    assert out["type_name"] == "CONSOLE_LOGS"
    assert "hello strap" in out["parsed"]["log"]
