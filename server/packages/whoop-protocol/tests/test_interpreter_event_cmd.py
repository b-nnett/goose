from whoop_protocol.interpreter import parse_frame
from tests.fixtures import frames


def test_event_decode():
    out = parse_frame(frames.EVENT_RAW_ON)
    assert out["type_name"] == "EVENT"
    p = out["parsed"]
    assert p["event"] == "RAW_DATA_COLLECTION_ON(46)"
    assert p["event_timestamp"] == 0x677ED619


def test_command_response_battery():
    out = parse_frame(frames.CMD_RESP_BATTERY)
    assert out["type_name"] == "COMMAND_RESPONSE"
    p = out["parsed"]
    assert p["resp_cmd"] == "GET_BATTERY_LEVEL(26)"
    assert p["battery_pct"] == 25.5


def test_command_response_version_info():
    # Live strap sends REPORT_VERSION_INFO (cmd 7); payload is response-status (2 bytes)
    # + version longs. "<BBBLLLLLLLL" needs 35 bytes — regression for a porting bug that
    # sliced the buffer to 31 and raised struct.error on real data.
    import struct
    from whoop_protocol.framing import frame_from_payload
    # payload: [status BB][pad B] then 8 LE u32 version components (harvard 1.2.3.4 / boylston 5.6.7.8)
    pay = bytes([0x0a, 0x01, 0x00]) + struct.pack("<LLLLLLLL", 1, 2, 3, 4, 5, 6, 7, 8)
    frame = frame_from_payload(pay, type_byte=36, seq=0, cmd=7)  # COMMAND_RESPONSE, REPORT_VERSION_INFO
    out = parse_frame(frame)
    assert out["type_name"] == "COMMAND_RESPONSE"
    assert out["parsed"]["fw_harvard"] == "1.2.3.4"
    assert out["parsed"]["fw_boylston"] == "5.6.7.8"
