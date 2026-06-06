"""
Tests for analysis.activity — per-record gravity L2 delta motion intensity.

PURE (no DB). Run offline:
    cd ~/Developer/home-server/stacks/whoop/ingest
    ~/Developer/home-server/venv/bin/python -m pytest tests/test_activity.py -q
"""
from __future__ import annotations

import math

from app.analysis.activity import activity_series, summarize_activity

T0 = 1_700_000_000.0


# ---------------------------------------------------------------------------
# Single triplet
# ---------------------------------------------------------------------------

def test_empty_returns_empty():
    assert activity_series([]) == []


def test_constant_gravity_zero_intensity():
    rows = [{"ts": T0 + i, "x": 0.0, "y": 0.0, "z": 1.0} for i in range(10)]
    series = activity_series(rows)
    assert len(series) == 10
    assert series[0]["intensity"] == 0.0          # first row has no predecessor
    assert all(p["intensity"] == 0.0 for p in series[1:])


def test_oscillating_gravity_high_intensity():
    rows = []
    for i in range(10):
        v = 1.0 if i % 2 == 0 else -1.0
        rows.append({"ts": T0 + i, "x": v, "y": 0.0, "z": 0.0})
    series = activity_series(rows)
    # |1 - (-1)| = 2 each step after the first
    assert series[0]["intensity"] == 0.0
    assert all(abs(p["intensity"] - 2.0) < 1e-9 for p in series[1:])


def test_known_l2_delta():
    rows = [
        {"ts": T0, "x": 0.0, "y": 0.0, "z": 0.0},
        {"ts": T0 + 1, "x": 3.0, "y": 4.0, "z": 0.0},  # sqrt(9+16)=5
    ]
    series = activity_series(rows)
    assert series[1]["intensity"] == 5.0


def test_missing_coord_is_infinite_delta():
    rows = [
        {"ts": T0, "x": 0.0, "y": 0.0, "z": 1.0},
        {"ts": T0 + 1, "x": None, "y": 0.0, "z": 1.0},
    ]
    series = activity_series(rows)
    assert math.isinf(series[1]["intensity"])


def test_rows_sorted_by_ts():
    rows = [
        {"ts": T0 + 2, "x": 2.0, "y": 0.0, "z": 0.0},
        {"ts": T0, "x": 0.0, "y": 0.0, "z": 0.0},
        {"ts": T0 + 1, "x": 1.0, "y": 0.0, "z": 0.0},
    ]
    series = activity_series(rows)
    assert [p["ts"] for p in series] == [T0, T0 + 1, T0 + 2]
    assert series[0]["intensity"] == 0.0
    assert abs(series[1]["intensity"] - 1.0) < 1e-9
    assert abs(series[2]["intensity"] - 1.0) < 1e-9


# ---------------------------------------------------------------------------
# Two triplets (forward-compatible; gravity2 not yet persisted — Task 4.2)
# ---------------------------------------------------------------------------

def test_both_triplets_average():
    # triplet1 delta = 2 (x flips 0->2), triplet2 delta = 4 (x2 flips 0->4)
    rows = [
        {"ts": T0, "x": 0.0, "y": 0.0, "z": 0.0, "x2": 0.0, "y2": 0.0, "z2": 0.0},
        {"ts": T0 + 1, "x": 2.0, "y": 0.0, "z": 0.0, "x2": 4.0, "y2": 0.0, "z2": 0.0},
    ]
    series = activity_series(rows)
    # average of 2 and 4 = 3
    assert abs(series[1]["intensity"] - 3.0) < 1e-9


def test_both_triplets_one_missing_is_infinite():
    rows = [
        {"ts": T0, "x": 0.0, "y": 0.0, "z": 0.0, "x2": 0.0, "y2": 0.0, "z2": 0.0},
        {"ts": T0 + 1, "x": 1.0, "y": 0.0, "z": 0.0, "x2": None, "y2": 0.0, "z2": 0.0},
    ]
    series = activity_series(rows)
    assert math.isinf(series[1]["intensity"])  # (finite + inf)/2 = inf


def test_single_triplet_path_when_no_second():
    rows = [
        {"ts": T0, "x": 0.0, "y": 0.0, "z": 0.0},
        {"ts": T0 + 1, "x": 3.0, "y": 4.0, "z": 0.0},
    ]
    series = activity_series(rows)
    assert series[1]["intensity"] == 5.0  # single triplet, not averaged


# ---------------------------------------------------------------------------
# summarize_activity
# ---------------------------------------------------------------------------

def test_summarize_basic():
    rows = []
    for i in range(5):
        v = 1.0 if i % 2 == 0 else -1.0
        rows.append({"ts": T0 + i, "x": v, "y": 0.0, "z": 0.0})
    summary = summarize_activity(activity_series(rows))
    # intensities: [0, 2, 2, 2, 2] -> mean 1.6, peak 2, n 5
    assert summary["n"] == 5
    assert abs(summary["peak"] - 2.0) < 1e-9
    assert abs(summary["mean"] - 1.6) < 1e-9


def test_summarize_excludes_infinite():
    rows = [
        {"ts": T0, "x": 0.0, "y": 0.0, "z": 0.0},
        {"ts": T0 + 1, "x": None, "y": 0.0, "z": 0.0},
        {"ts": T0 + 2, "x": 0.0, "y": 0.0, "z": 0.0},
    ]
    summary = summarize_activity(activity_series(rows))
    # row0=0 (finite); row1 has a None coord -> inf; row2's prev (row1) is None
    # -> inf. So only 1 finite point (row0).
    assert summary["n"] == 1
    assert summary["peak"] == 0.0


def test_summarize_empty():
    assert summarize_activity([]) == {"mean": 0.0, "peak": 0.0, "n": 0}
