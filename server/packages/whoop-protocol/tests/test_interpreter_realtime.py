from whoop_protocol.interpreter import parse_frame
from tests.fixtures import frames


def test_realtime_data_hr_and_rr():
    out = parse_frame(frames.REALTIME_DATA_HR60)
    p = out["parsed"]
    assert p["heart_rate"] == 60
    assert p["timestamp"] == 31538447
    assert p["subseconds"] == 26152
    assert p["rr_count"] == 0
    assert p["rr_intervals"] == []
