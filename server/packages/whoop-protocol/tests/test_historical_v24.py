"""type-47 HISTORICAL_DATA V24 — the 14-day biometric store record.

The frame is SYNTHETIC, built by scripts/gen_synthetic_fixtures.historical_v24() and
embedded here so the test needs no gitignored capture. No real on-device data. Mirrors the
Swift test at Packages/WhoopProtocol/Tests/WhoopProtocolTests/HistoricalV24Tests.swift
(same frame bytes, same expected fields).
"""
import math

from whoop_protocol.interpreter import extract_historical_streams, parse_frame

# A synthetic V24 record: HR=63, one R-R interval, on-wrist, |gravity| ~ 1g, unix anchored
# to 1700000000 (2023-11-14T22:13:20Z — an obviously-fixed synthetic epoch).
V24_FRAME = bytes.fromhex(
    "aa5a008e2f18000000000000f153650000000000003f0152030000000000000000dc053075"
    "000000cdcc4c3dcdcccc3d5a657e3f00000040cdcc4c3dcdcccc3d5a657e3f504668428403"
    "200364006400b80bb80b000000000000c25c1a88"
)


def test_v24_decodes_as_historical_data():
    out = parse_frame(V24_FRAME)
    assert out["ok"]
    assert out["type_name"] == "HISTORICAL_DATA"
    assert out["crc_ok"] is True
    assert out["seq"] == 24  # version byte


def test_v24_biometric_fields():
    p = parse_frame(V24_FRAME)["parsed"]
    assert p["hist_version"] == 24
    assert p["unix"] == 1700000000  # synthetic anchor epoch, not device epoch
    assert p["heart_rate"] == 63
    assert p["rr_count"] == 1
    assert p["rr_intervals"] == [850]
    assert p["ppg_green"] == 1500
    assert p["ppg_red_ir"] == 30000
    assert p["skin_contact"] == 64
    assert p["spo2_red"] == 18000
    assert p["spo2_ir"] == 17000
    assert p["skin_temp_raw"] == 900
    assert p["resp_rate_raw"] == 3000
    assert p["signal_quality"] == 3000


def test_v24_gravity_is_f32_unit_vector():
    p = parse_frame(V24_FRAME)["parsed"]
    # gravity is a 3xf32 vector; magnitude must be ~1g (physical plausibility gate).
    mag = math.sqrt(p["gravity_x"] ** 2 + p["gravity_y"] ** 2 + p["gravity_z"] ** 2)
    assert 0.9 < mag < 1.1
    assert isinstance(p["gravity_x"], float)


def test_v24_gravity2_present_and_plausible():
    p = parse_frame(V24_FRAME)["parsed"]
    # 2nd accel/gravity triplet at frame offsets 56, 60, 64.
    assert "gravity2_x" in p
    assert "gravity2_y" in p
    assert "gravity2_z" in p
    assert isinstance(p["gravity2_x"], float)
    mag2 = math.sqrt(p["gravity2_x"] ** 2 + p["gravity2_y"] ** 2 + p["gravity2_z"] ** 2)
    assert 0.9 < mag2 < 1.1, f"|gravity2| = {mag2:.6f} is outside plausible range (0.9, 1.1)"


def test_v24_extract_historical_streams():
    out = parse_frame(V24_FRAME)
    st = extract_historical_streams([out], device_clock_ref=0, wall_clock_ref=0)
    assert st["hr"] == [{"ts": 1700000000, "bpm": 63}]
    assert st["rr"] == [{"ts": 1700000000, "rr_ms": 850}]
    assert st["spo2"][0] == {"ts": 1700000000, "red": 18000, "ir": 17000, "unit": "raw_adc"}
    assert st["skin_temp"][0] == {"ts": 1700000000, "raw": 900, "unit": "raw_adc"}
    assert st["resp"][0]["raw"] == 3000
    g = st["gravity"][0]
    assert g["ts"] == 1700000000 and g["unit"] == "g"


def test_unmapped_version_falls_back_gracefully():
    # Flip the version byte (frame[5]) to an unmapped value; must not raise, must mark unmapped.
    bad = bytearray(V24_FRAME)
    bad[5] = 99
    out = parse_frame(bytes(bad))
    assert out["ok"]  # frame still parses (crc will mismatch but parse is defensive)
    assert out["parsed"].get("hist_version") == 99
