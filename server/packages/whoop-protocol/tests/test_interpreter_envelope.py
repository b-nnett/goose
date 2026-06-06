from whoop_protocol.interpreter import parse_frame
from tests.fixtures import frames


def test_parse_frame_envelope_fields():
    out = parse_frame(frames.REALTIME_DATA_HR60)
    assert out["ok"] is True
    assert out["type_name"] == "REALTIME_DATA"
    assert out["seq"] == 2
    assert out["crc_ok"] is True
    names = {f["name"] for f in out["fields"]}
    assert {"SOF", "length", "crc8", "packet_type", "seq"} <= names


def test_parse_frame_rejects_fragment():
    out = parse_frame(b"\x01\x02\x03")
    assert out["ok"] is False
    assert out["type_name"] == "INVALID/FRAGMENT"


def test_parse_frame_includes_crc32_trailer_field():
    out = parse_frame(frames.REALTIME_DATA_HR60)
    crc_fields = [f for f in out["fields"] if f["name"] == "crc32"]
    assert len(crc_fields) == 1
    assert crc_fields[0]["note"] == "OK"
