import pytest
from whoop_protocol.interpreter import parse_frame
from tests.fixtures import frames


@pytest.mark.skipif(frames.IMU_FRAME is None, reason="motion_capture.jsonl not present")
def test_imu_axes_means_present():
    out = parse_frame(frames.IMU_FRAME)
    p = out["parsed"]
    for axis in ("accelX_mean", "accelY_mean", "accelZ_mean",
                 "gyroX_mean", "gyroY_mean", "gyroZ_mean"):
        assert axis in p
    # still/at-rest accelZ is the large gravity axis (empirically ~3000-4000 LSB)
    assert abs(p["accelZ_mean"]) > 1000
    # field annotations exist for the hex view
    axis_fields = [f for f in out["fields"] if f["name"] == "accelX"]
    assert axis_fields and axis_fields[0]["len"] == 200


@pytest.mark.skipif(frames.OPTICAL_FRAME is None, reason="no optical fixture")
def test_optical_region_present():
    out = parse_frame(frames.OPTICAL_FRAME)
    assert out["parsed"]["packet"] == "raw optical/PPG (24-bit x~4ch)"
    ppg = [f for f in out["fields"] if f["cat"] == "ppg"]
    assert ppg
