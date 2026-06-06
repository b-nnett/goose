import pytest

import whoop_protocol as wp
from whoop_protocol.interpreter import extract_streams, parse_frame
from tests.fixtures import frames


def test_package_public_imports():
    assert callable(wp.parse_frame)
    assert callable(wp.extract_streams)
    assert callable(wp.load_schema)


def test_extract_streams_hr_row():
    parsed = [parse_frame(frames.REALTIME_DATA_HR60)]
    # clock_ref: device_clock 31538447 maps to wall epoch 1700000000
    streams = extract_streams(parsed, device_clock_ref=31538447, wall_clock_ref=1700000000)
    assert len(streams["hr"]) == 1
    row = streams["hr"][0]
    assert row["bpm"] == 60
    assert row["ts"] == 1700000000  # same instant as the device_clock_ref
    assert streams["rr"] == []


def test_extract_streams_event_row():
    parsed = [parse_frame(frames.EVENT_RAW_ON)]
    streams = extract_streams(parsed, device_clock_ref=0, wall_clock_ref=0)
    assert len(streams["events"]) == 1
    assert streams["events"][0]["kind"] == "RAW_DATA_COLLECTION_ON(46)"


def test_extract_streams_event_ts_is_real_unix_not_offset():
    # EVENT timestamps are real RTC unix seconds, so the device->wall clock offset must
    # NOT be applied (regression: applying it threw events ~50 years into the future).
    parsed = [parse_frame(frames.EVENT_RAW_ON)]
    streams = extract_streams(parsed, device_clock_ref=31538447, wall_clock_ref=1779570508)
    assert streams["events"][0]["ts"] == 0x677ED619  # the frame's raw event_timestamp, unchanged


@pytest.mark.skipif(frames.IMU_FRAME is None, reason="motion_capture.jsonl not present")
def test_extract_streams_excludes_type43_hr():
    # REALTIME_RAW_DATA (type 43) carries an HR byte, but type-40 is the canonical HR
    # stream; type-43 HR is intentionally NOT routed (avoids double-counting). The IMU
    # frame parses with a heart_rate in `parsed`, yet must produce zero hr/rr rows.
    imu = parse_frame(frames.IMU_FRAME)
    assert "heart_rate" in imu["parsed"]  # it IS decoded...
    streams = extract_streams([imu], device_clock_ref=0, wall_clock_ref=0)
    assert streams["hr"] == []             # ...but deliberately not routed to the stream
    assert streams["rr"] == []


def test_extract_streams_skips_crc_failed_frame():
    bad = bytearray(frames.REALTIME_DATA_HR60)
    bad[-1] ^= 0xFF  # corrupt the crc32 trailer
    parsed = [parse_frame(bytes(bad))]
    assert parsed[0]["crc_ok"] is False
    streams = extract_streams(parsed, device_clock_ref=31538447, wall_clock_ref=1700000000)
    assert streams["hr"] == []  # crc-failed frame contributes no rows
