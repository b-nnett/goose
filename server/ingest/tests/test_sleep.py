"""
Tests for analysis.sleep — sleep/wake detection, staging (APPROXIMATE), and
the daily sleep summary.

These tests are PURE (no DB, no network). The "real-data" validation against the
live my-whoop streams is deferred to Phase 5 of the mega-plan; here we use
realistic synthetic 1 Hz streams that match the read API's query_stream shape.

Run offline:
    cd ~/Developer/home-server/stacks/whoop/ingest
    ~/Developer/home-server/venv/bin/python -m pytest tests/test_sleep.py -q
"""
from __future__ import annotations

import datetime as dt
import math

import pytest

from app.analysis.sleep import (
    GRAVITY_STILL_THRESHOLD_G,
    MERGE_MIN,
    MIN_SLEEP_MIN,
    STILL_FRACTION,
    SleepSession,
    StageSegment,
    daily_sleep_summary,
    detect_sleep,
    hypnogram_metrics,
    sleep_performance_score,
    _gravity_deltas,
    _merge_periods,
)
from app.analysis import sleep_features as sf

# ---------------------------------------------------------------------------
# Synthetic stream builders
# ---------------------------------------------------------------------------

T0 = 1_700_000_000.0  # arbitrary fixed epoch start (UTC)


def _still_night(
    minutes: int,
    *,
    start: float = T0,
    hz: float = 1.0,
    base_hr: float = 54.0,
    dip_block: tuple[int, int] | None = None,
    rr_ms: int = 1050,
    resp_raw: int = 4000,
    gravity=(0.0, 0.0, 1.0),
) -> dict[str, list[dict]]:
    """Build a calm, still night: constant gravity (deltas ~0), low HR with an
    optional deep-dip block (minute range), steady RR + resp.
    `dip_block` = (start_min, end_min) where HR drops ~6 bpm and HRV rises."""
    step = 1.0 / hz
    n = int(minutes * 60 * hz)
    hr, rr, resp, grav, skin = [], [], [], [], []
    gx, gy, gz = gravity
    for i in range(n):
        ts = start + i * step
        minute = (ts - start) / 60.0
        in_dip = dip_block is not None and dip_block[0] <= minute < dip_block[1]
        bpm = base_hr - (6.0 if in_dip else 0.0)
        hr.append({"ts": ts, "bpm": round(bpm)})
        # deep block: longer RR (lower HR) + low beat-to-beat variation (high RMSSD
        # comes from the alternation amplitude); make dip RR vary more for elevated HRV.
        # amp: alternation amplitude drives RMSSD — larger amp → higher RMSSD (elevated
        # HRV in dip block) which triggers the deep-stage classification.
        amp = 60 if in_dip else 12
        rr.append({"ts": ts, "rr_ms": rr_ms + (amp if i % 2 == 0 else -amp)})
        resp.append({"ts": ts, "raw": resp_raw})
        grav.append({"ts": ts, "x": gx, "y": gy, "z": gz})
        skin.append({"ts": ts, "raw": 930})
    return {"hr": hr, "rr": rr, "resp": resp, "gravity": grav, "skin_temp": skin}


def _active_stream(minutes: int, *, start: float = T0) -> dict[str, list[dict]]:
    """Jittering gravity (deltas >> threshold) + elevated, noisy HR throughout."""
    n = minutes * 60
    hr, rr, resp, grav, skin = [], [], [], [], []
    for i in range(n):
        ts = start + i
        # gravity flips sign each sample -> per-sample delta = 2.0 (>> 0.01)
        v = 1.0 if i % 2 == 0 else -1.0
        grav.append({"ts": ts, "x": v, "y": 0.0, "z": 0.0})
        hr.append({"ts": ts, "bpm": 95 + (i % 7)})
        rr.append({"ts": ts, "rr_ms": 600 + (i % 50)})
        resp.append({"ts": ts, "raw": 4000 + (i % 200)})
        skin.append({"ts": ts, "raw": 930})
    return {"hr": hr, "rr": rr, "resp": resp, "gravity": grav, "skin_temp": skin}


def _mixed_night(start: float = T0) -> dict[str, list[dict]]:
    """A longer realistic night: 20 min settling-in (active), then ~7h asleep
    with two brief wake bursts, then 15 min waking (active)."""
    streams = {"hr": [], "rr": [], "resp": [], "gravity": [], "skin_temp": []}
    cursor = start

    def append_block(minutes, *, still, bpm, dip=False):
        nonlocal cursor
        n = minutes * 60
        for i in range(n):
            ts = cursor + i
            if still:
                grav = (0.0, 0.0, 1.0)
            else:
                v = 1.0 if i % 2 == 0 else -1.0
                grav = (v, 0.0, 0.0)
            streams["gravity"].append({"ts": ts, "x": grav[0], "y": grav[1], "z": grav[2]})
            streams["hr"].append({"ts": ts, "bpm": bpm})
            amp = 55 if dip else 14
            streams["rr"].append({"ts": ts, "rr_ms": 1050 + (amp if i % 2 == 0 else -amp)})
            streams["resp"].append({"ts": ts, "raw": 4000})
            streams["skin_temp"].append({"ts": ts, "raw": 930})
        cursor += n

    append_block(20, still=False, bpm=78)   # settling in (awake, fidgeting)
    append_block(90, still=True, bpm=56)     # light
    append_block(60, still=True, bpm=49, dip=True)  # deep
    append_block(4, still=False, bpm=70)     # wake burst
    append_block(120, still=True, bpm=55)    # light/rem
    append_block(3, still=False, bpm=68)     # wake burst
    append_block(100, still=True, bpm=52, dip=True)  # more deep
    append_block(15, still=False, bpm=80)    # waking up
    return streams


# ---------------------------------------------------------------------------
# _gravity_deltas / _merge_periods unit tests
# ---------------------------------------------------------------------------


class TestGravityDeltas:
    def test_first_delta_is_zero(self):
        rows = [{"ts": 0.0, "x": 0.0, "y": 0.0, "z": 1.0},
                {"ts": 1.0, "x": 0.0, "y": 0.0, "z": 1.0}]
        d = _gravity_deltas(rows)
        assert d[0] == 0.0
        assert d[1] == pytest.approx(0.0)

    def test_l2_norm(self):
        rows = [{"ts": 0.0, "x": 0.0, "y": 0.0, "z": 0.0},
                {"ts": 1.0, "x": 3.0, "y": 4.0, "z": 0.0}]
        d = _gravity_deltas(rows)
        assert d[1] == pytest.approx(5.0)


class TestMergePeriods:
    def test_short_period_merged_into_next(self):
        # 5-min active sandwiched then a long sleep -> short one disappears/merges
        periods = [
            {"stage": "active", "start": 0.0, "end": 5 * 60.0},
            {"stage": "sleep", "start": 5 * 60.0, "end": 200 * 60.0},
        ]
        merged = _merge_periods(periods, merge_min=MERGE_MIN)
        # the 5-min active < 15 min is merged into the long sleep neighbour
        assert len(merged) == 1
        assert merged[0]["stage"] == "sleep"

    def test_empty(self):
        assert _merge_periods([], merge_min=MERGE_MIN) == []


# ---------------------------------------------------------------------------
# detect_sleep
# ---------------------------------------------------------------------------


class TestDetectSleep:
    def test_good_night_one_session(self):
        streams = _still_night(420, dip_block=(120, 200))  # 7h, deep block 80 min
        sessions = detect_sleep(streams)
        assert len(sessions) == 1
        s = sessions[0]
        assert isinstance(s, SleepSession)
        dur_min = (s.end - s.start) / 60.0
        assert dur_min == pytest.approx(420, abs=5)
        assert s.efficiency > 0.8
        assert s.efficiency <= 1.0

    def test_good_night_has_stages(self):
        streams = _still_night(420, dip_block=(120, 200))
        sessions = detect_sleep(streams)
        s = sessions[0]
        assert len(s.stages) >= 1
        # all stage segments lie within session bounds, contiguous-ish
        for seg in s.stages:
            assert isinstance(seg, StageSegment)
            assert s.start <= seg.start <= seg.end <= s.end
        # stages should sum approximately to the in-bed span
        total = sum(seg.end - seg.start for seg in s.stages)
        assert total == pytest.approx(s.end - s.start, abs=120)

    def test_all_active_no_session(self):
        streams = _active_stream(420)
        assert detect_sleep(streams) == []

    def test_empty_streams(self):
        assert detect_sleep({}) == []
        assert detect_sleep({"hr": [], "rr": [], "gravity": []}) == []

    def test_gravity_only_degrades_gracefully(self):
        # No HR/RR/resp — gravity alone should still detect a still session.
        streams = _still_night(180)
        streams = {"gravity": streams["gravity"]}
        sessions = detect_sleep(streams)
        assert len(sessions) == 1
        assert (sessions[0].end - sessions[0].start) / 60.0 == pytest.approx(180, abs=5)

    def test_sessions_sorted(self):
        # two still blocks separated by a long active gap -> two sessions, sorted
        a = _still_night(120, start=T0)
        # 90-min active gap, then another still block
        gap_start = T0 + 120 * 60
        b_start = gap_start + 90 * 60
        b = _still_night(120, start=b_start)
        active = _active_stream(90, start=gap_start)
        merged = {k: a[k] + active.get(k, []) + b[k] for k in a}
        sessions = detect_sleep(merged)
        assert len(sessions) >= 2
        starts = [s.start for s in sessions]
        assert starts == sorted(starts)


# ---------------------------------------------------------------------------
# Staging present + non-negative minutes
# ---------------------------------------------------------------------------


class TestStaging:
    def test_two_distinct_stages(self):
        streams = _still_night(420, dip_block=(120, 240))  # big deep block
        sessions = detect_sleep(streams)
        s = sessions[0]
        kinds = {seg.stage for seg in s.stages}
        assert len(kinds) >= 2, f"expected >=2 distinct stages, got {kinds}"

    def test_stage_minutes_non_negative_and_sum(self):
        streams = _still_night(420, dip_block=(120, 240))
        sessions = detect_sleep(streams)
        date = _end_date(sessions[0])
        summary = daily_sleep_summary(sessions, date)
        assert summary["deep_min"] >= 0
        assert summary["rem_min"] >= 0
        assert summary["light_min"] >= 0
        asleep = summary["total_sleep_min"]
        staged = summary["deep_min"] + summary["rem_min"] + summary["light_min"]
        assert staged == pytest.approx(asleep, abs=2)


# ---------------------------------------------------------------------------
# daily_sleep_summary
# ---------------------------------------------------------------------------


def _end_date(session: SleepSession) -> dt.date:
    return dt.datetime.fromtimestamp(session.end, dt.timezone.utc).date()


class TestDailySummary:
    def test_summary_fields_plausible(self):
        streams = _still_night(420, dip_block=(120, 200), base_hr=54.0)
        sessions = detect_sleep(streams)
        date = _end_date(sessions[0])
        summary = daily_sleep_summary(sessions, date)
        assert summary["date"] == date
        assert summary["total_sleep_min"] > 400
        assert 0.0 <= summary["efficiency"] <= 1.0
        assert summary["resting_hr"] is not None
        assert summary["resting_hr"] < 60  # low overnight HR
        assert summary["avg_hrv"] is not None and summary["avg_hrv"] > 0
        assert summary["disturbances"] >= 0

    def test_summary_reports_bed_and_wake_times(self):
        streams = _still_night(420, dip_block=(120, 200), start=T0)
        sessions = detect_sleep(streams)
        summary = daily_sleep_summary(sessions, _end_date(sessions[0]))
        # The night's in-bed span: earliest start, latest end (epoch seconds).
        assert summary["sleep_start"] == pytest.approx(sessions[0].start)
        assert summary["sleep_end"] == pytest.approx(sessions[-1].end)
        assert summary["sleep_start"] < summary["sleep_end"]

    def test_no_session_for_date(self):
        streams = _still_night(420, dip_block=(120, 200))
        sessions = detect_sleep(streams)
        other_day = _end_date(sessions[0]) + dt.timedelta(days=10)
        summary = daily_sleep_summary(sessions, other_day)
        assert summary["total_sleep_min"] == 0
        assert summary["efficiency"] == 0.0
        assert summary["resting_hr"] is None
        assert summary["avg_hrv"] is None
        assert summary["sleep_start"] is None
        assert summary["sleep_end"] is None

    def test_empty_sessions(self):
        summary = daily_sleep_summary([], dt.date(2024, 1, 1))
        assert summary["total_sleep_min"] == 0


# ---------------------------------------------------------------------------
# Smoke / robustness over realistic mixed night
# ---------------------------------------------------------------------------


class TestStagingDistribution:
    """The staging classifier is APPROXIMATE, but it must be physiologically
    coherent: a long calm night with distinct HR/HRV regimes should surface
    deep AND rem (not one or zero stages), and a still night must not invent a
    pile of false 'wake' disturbances. These lock in the percentile-relative
    rewrite (the old absolute-band classifier produced rem=0 + many disturbances
    on real data)."""

    def _structured_night(self) -> dict[str, list[dict]]:
        """A still night with three regimes the stager should separate:
        - 120 min low-HR / high-HRV (deep)
        - 120 min elevated-HR / low-HRV (rem — still body, active autonomics)
        - 180 min mid-HR / mid-HRV (light)
        Gravity is constant throughout (no movement → no wake)."""
        streams = {"hr": [], "rr": [], "resp": [], "gravity": [], "skin_temp": []}
        cursor = T0

        def block(minutes, bpm, amp):
            nonlocal cursor
            for i in range(minutes * 60):
                ts = cursor + i
                streams["hr"].append({"ts": ts, "bpm": bpm})
                streams["rr"].append({"ts": ts, "rr_ms": 1050 + (amp if i % 2 == 0 else -amp)})
                streams["resp"].append({"ts": ts, "raw": 4000})
                streams["gravity"].append({"ts": ts, "x": 0.0, "y": 0.0, "z": 1.0})
                streams["skin_temp"].append({"ts": ts, "raw": 930})
            cursor += minutes * 60

        block(120, bpm=48, amp=60)   # deep: low HR, high HRV
        block(120, bpm=58, amp=10)   # rem: elevated HR, low HRV, still
        block(180, bpm=53, amp=20)   # light: in-between
        return streams

    def test_still_night_surfaces_deep_and_rem(self):
        sessions = detect_sleep(self._structured_night())
        assert len(sessions) == 1
        kinds = {seg.stage for seg in sessions[0].stages}
        # The headline bug was rem=0; the elevated-HR still block must read as REM.
        assert "rem" in kinds, f"expected REM to appear, got {kinds}"
        assert "deep" in kinds, f"expected deep to appear, got {kinds}"

    def test_calm_still_night_has_no_false_wake(self):
        # No movement anywhere → there should be no 'wake' disturbances at all.
        sessions = detect_sleep(self._structured_night())
        summary = daily_sleep_summary(sessions, _end_date(sessions[0]))
        assert summary["disturbances"] == 0, (
            f"still night should have 0 disturbances, got {summary['disturbances']}")


class TestSmokeRealistic:
    def test_mixed_night_coherent_structure(self):
        # NOTE: real my-whoop validation is deferred to Phase 5 (no network in tests).
        streams = _mixed_night()
        sessions = detect_sleep(streams)
        assert len(sessions) >= 1
        # sorted
        assert [s.start for s in sessions] == sorted(s.start for s in sessions)
        for s in sessions:
            assert 0.0 <= s.efficiency <= 1.0
            assert s.end > s.start
            for seg in s.stages:
                assert s.start <= seg.start <= seg.end <= s.end
                assert seg.stage in {"light", "deep", "rem", "wake"}
        # daily summary runs without error
        date = _end_date(sessions[-1])
        summary = daily_sleep_summary(sessions, date)
        assert summary["total_sleep_min"] >= 0
        assert 0.0 <= summary["efficiency"] <= 1.0
        # The fixture injects two active (jittering) wake bursts (4 min and 3 min).
        # _merge_periods absorbs them into the surrounding sleep run at the session
        # level (too short to split the session), but the staging pass independently
        # re-examines the raw gravity+HR signal at 5-min sub-window granularity and
        # can still label those bursts as "wake" segments. Confirm they surface.
        assert summary["disturbances"] > 0


# ===========================================================================
# Task 8 — multi-signal staging engine (sleep_features.py) unit tests
# ===========================================================================


class TestColeKripke:
    """Cole–Kripke pinned against the cited te Lindert 30 s coefficients (§2.1)."""

    def test_weights_and_scale_pinned(self):
        # Guard the exact cited form: SI = 0.001 × (106,54,58,76,230,74,67)·A
        assert sf.CK_WEIGHTS == (106.0, 54.0, 58.0, 76.0, 230.0, 74.0, 67.0)
        assert sf.CK_SCALE == 0.001

    def test_all_zero_counts_is_sleep(self):
        # No activity anywhere → SI = 0 < 1 → every epoch sleep.
        flags = sf.cole_kripke([0.0] * 20)
        assert all(flags)

    def test_hand_computed_si_on_one_epoch(self):
        # Build counts so a single epoch's SI is hand-checkable.
        # Put A0=10 at index 5, everything else 0. For epoch i=5, the window
        # [i-4..i+2] sees A0 with weight 230 → SI = 0.001 × 230 × 10 = 2.3 ≥ 1 → WAKE.
        counts = [0.0] * 12
        counts[5] = 10.0
        rescaled = counts  # call cole_kripke directly in isolation; rescale_counts is NOT applied here
        flags = sf.cole_kripke(rescaled)
        # epoch 5: SI = 2.3 → wake (False)
        assert flags[5] is False
        # epoch 1 (A0=0, but A0 is at +4 here with weight 106): index1 sees
        # j=1-4+0..= -3..3 ; A at index5 not in window → SI=0 → sleep.
        assert flags[1] is True

    def test_below_threshold_is_sleep(self):
        # A0 = 4 (rescaled) with weight 230 → SI = 0.001×230×4 = 0.92 < 1 → SLEEP.
        counts = [0.0] * 12
        counts[5] = 4.0
        flags = sf.cole_kripke(counts)
        assert flags[5] is True

    def test_rescale_divides_by_100_and_clips_300(self):
        assert sf.rescale_counts([100.0]) == [1.0]
        assert sf.rescale_counts([5_000_000.0]) == [300.0]  # clip
        assert sf.rescale_counts([0.0]) == [0.0]


class TestEpochGrid:
    def test_grid_buckets_streams_to_30s(self):
        start = 0.0
        end = 90.0  # 3 epochs
        # gravity every 1 s; HR every 1 s; rr/resp every 1 s
        gt = [float(i) for i in range(90)]
        gd = [0.0] + [0.005] * 89  # all "still" (below 0.01)
        hr = [{"ts": float(i), "bpm": 50 + (i // 30)} for i in range(90)]
        rr = [{"ts": float(i), "rr_ms": 1000} for i in range(90)]
        resp = [{"ts": float(i), "raw": 4000} for i in range(90)]
        grid = sf.build_epoch_grid(start, end, gt, gd, hr, rr, resp)
        assert grid.n_epochs == 3
        assert len(grid.edges) == 4
        # epoch HR means: 50, 51, 52
        assert grid.hr == pytest.approx([50.0, 51.0, 52.0])
        # all still → move_frac ~0 (the very first delta is 0)
        assert all(mf < 0.5 for mf in grid.move_frac)
        # rr buckets each have 30 intervals
        assert all(len(b) == 30 for b in grid.rr)

    def test_move_fraction_high_when_jittering(self):
        start, end = 0.0, 60.0
        gt = [float(i) for i in range(60)]
        gd = [0.0] + [2.0] * 59  # huge deltas → moving
        grid = sf.build_epoch_grid(start, end, gt, gd, [], [], [])
        assert grid.move_frac[0] > 0.9
        assert grid.move_frac[1] > 0.9

    def test_empty_window(self):
        grid = sf.build_epoch_grid(10.0, 10.0, [], [], [], [], [])
        assert grid.n_epochs in (0, 1)


class TestRespRateRRV:
    def test_regular_breathing_low_rrv(self):
        import numpy as np
        t = np.arange(0, 300, 1.0)
        sig = np.sin(2 * np.pi * 0.25 * t) * 100 + 4000  # 15 br/min
        rate, rrv = sf.resp_rate_and_rrv(sig.tolist())
        assert rate == pytest.approx(15.0, abs=3.0)
        assert rrv < 0.5  # very regular

    def test_irregular_breathing_higher_rrv(self):
        import numpy as np
        rng = np.random.default_rng(0)
        # jittered breath intervals → higher RRV
        t = 0.0
        samples = []
        peaks_t = []
        while t < 300:
            interval = 4.0 + rng.uniform(-1.5, 1.5)
            peaks_t.append(t)
            t += interval
        x = np.zeros(300)
        for pt in peaks_t:
            i = int(round(pt))
            if 0 <= i < 300:
                x[i] = 100.0
        rate_irr, rrv_irr = sf.resp_rate_and_rrv(x.tolist())
        assert math.isfinite(rrv_irr)
        # regular reference
        treg = np.arange(0, 300, 1.0)
        sigreg = np.sin(2 * np.pi * 0.25 * treg) * 100 + 4000
        _, rrv_reg = sf.resp_rate_and_rrv(sigreg.tolist())
        assert rrv_irr > rrv_reg

    def test_too_few_samples_nan(self):
        r, v = sf.resp_rate_and_rrv([1.0, 2.0, 3.0])
        assert math.isnan(r) and math.isnan(v)


class TestDoGHRVariability:
    def test_flat_hr_yields_near_zero(self):
        out = sf.dog_hr_variability([60.0] * 40)
        import numpy as np
        assert np.allclose(out, 0.0, atol=1e-6)

    def test_step_change_produces_response(self):
        import numpy as np
        hr = [55.0] * 20 + [70.0] * 20
        out = sf.dog_hr_variability(hr)
        # the DoG should be non-trivial around the step
        assert np.std(out) > 0.0

    def test_empty(self):
        import numpy as np
        assert sf.dog_hr_variability([]).size == 0


def _mk_feat(**kw):
    """Build an EpochFeatures with sensible defaults; override what matters."""
    base = dict(
        index=0, mid_ts=0.0, count=0.0, move_frac=0.0, ck_sleep=True,
        hr=55.0, hr_var=1.0, rmssd=40.0, sdnn=40.0, hf=200.0, lfhf=1.0,
        resp_rate=14.0, rrv=0.5, clock=0.5,
    )
    base.update(kw)
    return sf.EpochFeatures(**base)


class TestClassifyEpochs:
    """The transparent staging classifier (THE MODEL SEAM) on synthetic epochs."""

    def test_deep_epoch(self):
        # A deep block: low HR + high HF/RMSSD + still + regular resp, plus filler
        # light epochs so the percentile bands are well-defined.
        feats = []
        for _ in range(20):  # light filler: mid HR, mid HRV
            feats.append(_mk_feat(hr=58.0, rmssd=35.0, hf=150.0, rrv=0.6))
        for _ in range(10):  # deep candidates
            feats.append(_mk_feat(hr=48.0, rmssd=80.0, hf=400.0, rrv=0.2, move_frac=0.0))
        labels = sf.classify_epochs(feats)
        assert "deep" in labels[20:], f"expected deep in deep block, got {set(labels[20:])}"

    def test_rem_epoch(self):
        feats = []
        for _ in range(20):  # filler
            feats.append(_mk_feat(hr=54.0, hr_var=1.0, rrv=0.4))
        for _ in range(10):  # REM: still body, elevated HR + HR-variability, irregular resp
            feats.append(_mk_feat(hr=64.0, hr_var=5.0, rmssd=30.0, hf=100.0,
                                   rrv=2.0, move_frac=0.0))
        labels = sf.classify_epochs(feats)
        assert "rem" in labels[20:], f"expected rem, got {set(labels[20:])}"

    def test_wake_epoch(self):
        feats = []
        for _ in range(20):
            feats.append(_mk_feat(hr=54.0, move_frac=0.0))
        for _ in range(5):  # moving + elevated HR → wake
            feats.append(_mk_feat(hr=80.0, hr_var=8.0, move_frac=0.9))
        labels = sf.classify_epochs(feats)
        assert "wake" in labels[20:], f"expected wake, got {set(labels[20:])}"

    def test_still_calm_is_light_not_wake(self):
        # A still epoch with calm HR must never be wake.
        feats = [_mk_feat(hr=55.0, move_frac=0.0) for _ in range(30)]
        labels = sf.classify_epochs(feats)
        assert "wake" not in labels

    def test_empty(self):
        assert sf.classify_epochs([]) == []


class TestSmoothing:
    def test_isolated_flip_removed(self):
        labels = ["light"] * 10
        labels[5] = "wake"  # a single isolated flip
        out = sf.smooth_labels(labels, window=5)
        assert out[5] == "light", f"isolated flip should be smoothed away: {out}"

    def test_real_block_preserved(self):
        labels = ["light"] * 5 + ["deep"] * 10 + ["light"] * 5
        out = sf.smooth_labels(labels, window=5)
        # the deep block (10 epochs) survives smoothing
        assert out.count("deep") >= 8

    def test_window_one_is_identity(self):
        labels = ["light", "wake", "deep"]
        assert sf.smooth_labels(labels, window=1) == labels


class TestPhysiology:
    def test_no_rem_in_first_15min_after_onset(self):
        # 30 epochs; onset at 0. First 15 min = 30 epochs → all early. Mark some REM.
        feats = [_mk_feat(index=i, clock=i / 60.0) for i in range(60)]
        labels = ["rem"] * 60
        out = sf.reimpose_physiology(labels, feats, onset_idx=0, final_wake_idx=59)
        # epochs within 15 min (30 epochs) of onset must NOT be rem
        early = out[:30]
        assert "rem" not in early, f"early REM should be relabeled: {early}"

    def test_deep_in_last_third_downgraded(self):
        feats = [_mk_feat(index=i, clock=i / 30.0) for i in range(30)]  # clock 0..~1
        labels = ["deep"] * 30
        out = sf.reimpose_physiology(labels, feats, onset_idx=0, final_wake_idx=29)
        # deep with clock > 1/3 should be light
        late_deep = [out[i] for i in range(30) if feats[i].clock > 1 / 3]
        assert all(s == "light" for s in late_deep)


class TestNoEarlyRemIntegration:
    """End-to-end integration test: no REM should appear in the first 15 minutes
    after sleep onset when running the full detect_sleep pipeline.

    TestPhysiology above validates reimpose_physiology in isolation; this class
    closes the gap by exercising the complete Stage 0–3 stack on a synthetic
    night that is deliberately constructed to tempt the classifier into early REM
    (still body + activated cardiac from the very first epoch).
    """

    def _early_rem_temptation_night(self) -> dict[str, list[dict]]:
        """Synthetic 5-hour night where the first 30 min has elevated HR and
        high HR-variability (REM-like cardiac signature) from sleep onset, then
        settles into a calm light/deep profile for the rest.

        The physiology re-imposition layer must suppress any REM calls in the
        opening 15 min regardless of what the classifier sees."""
        streams: dict[str, list[dict]] = {
            "hr": [], "rr": [], "resp": [], "gravity": [], "skin_temp": []
        }
        cursor = T0

        def block(minutes: int, bpm: int, amp: int) -> None:
            nonlocal cursor
            for i in range(minutes * 60):
                ts = cursor + i
                # constant gravity = perfectly still throughout (no motion → no wake gate)
                streams["gravity"].append({"ts": ts, "x": 0.0, "y": 0.0, "z": 1.0})
                streams["hr"].append({"ts": ts, "bpm": bpm})
                # alternating RR drives RMSSD; larger amp + elevated HR mimics REM cardiac
                streams["rr"].append({"ts": ts, "rr_ms": 900 + (amp if i % 2 == 0 else -amp)})
                streams["resp"].append({"ts": ts, "raw": 4000})
                streams["skin_temp"].append({"ts": ts, "raw": 930})
            cursor += minutes * 60

        block(30, bpm=68, amp=50)   # first 30 min: elevated HR + high HRV (REM-like)
        block(60, bpm=48, amp=65)   # deep block
        block(120, bpm=55, amp=15)  # light
        block(90, bpm=60, amp=10)   # rem-eligible
        return streams

    def test_no_rem_in_first_15_minutes_after_sleep_onset(self):
        """Full detect_sleep pipeline must not produce any REM segment that
        starts within 15 minutes of the detected sleep onset."""
        streams = self._early_rem_temptation_night()
        sessions = detect_sleep(streams)
        assert len(sessions) >= 1, "expected at least one sleep session"

        s = sessions[0]
        # Sleep onset = start of the first non-wake stage segment.
        non_wake = [seg for seg in s.stages if seg.stage != "wake"]
        if not non_wake:
            return  # no sleep at all → vacuously fine, but flag if unexpected
        onset_ts = non_wake[0].start
        no_rem_cutoff = onset_ts + 15 * 60  # 15 min after onset (in seconds)

        early_rem = [
            seg for seg in s.stages
            if seg.stage == "rem" and seg.start < no_rem_cutoff
        ]
        assert early_rem == [], (
            f"REM appeared within 15 min of sleep onset: "
            f"{[(seg.start - onset_ts, seg.end - onset_ts) for seg in early_rem]}"
        )


class TestHypnogramMetrics:
    """AASM metric formulas (§5) on a hand-built hypnogram."""

    def _session(self, segs):
        return SleepSession(
            start=segs[0].start, end=segs[-1].end, efficiency=0.0, stages=segs,
        )

    def test_tst_sol_waso_disturbances_efficiency(self):
        # in-bed 0..3600 s (60 min). Layout:
        #   0–600   wake  (10 min latency, pre-onset → not WASO)
        #   600–1800 light (20 min sleep)
        #   1800–1980 wake (3 min WASO, 1 disturbance)
        #   1980–3300 deep (22 min sleep)
        #   3300–3600 wake (5 min — AFTER final sleep epoch → not WASO)
        segs = [
            StageSegment(0, 600, "wake"),
            StageSegment(600, 1800, "light"),
            StageSegment(1800, 1980, "wake"),
            StageSegment(1980, 3300, "deep"),
            StageSegment(3300, 3600, "wake"),
        ]
        m = hypnogram_metrics(self._session(segs))
        assert m["tib_s"] == pytest.approx(3600)
        # TST = light(1200) + deep(1320) = 2520
        assert m["tst_s"] == pytest.approx(2520)
        # SOL = onset(600) − start(0) = 600
        assert m["sol_s"] == pytest.approx(600)
        # WASO = the 1800–1980 wake (180 s); the trailing wake after final sleep
        # (deep ends 3300) is NOT counted.
        assert m["waso_s"] == pytest.approx(180)
        assert m["disturbances"] == 1
        # SE = TST/TIB = 2520/3600 = 0.7
        assert m["efficiency"] == pytest.approx(0.7)

    def test_rem_latency(self):
        # onset at 600 (light), REM starts at 1800 → REM latency = 1200 s.
        segs = [
            StageSegment(0, 600, "wake"),
            StageSegment(600, 1800, "light"),
            StageSegment(1800, 2400, "rem"),
        ]
        m = hypnogram_metrics(self._session(segs))
        assert m["rem_latency_s"] == pytest.approx(1200)

    def test_no_rem_latency_is_nan(self):
        segs = [StageSegment(0, 600, "wake"), StageSegment(600, 3600, "light")]
        m = hypnogram_metrics(self._session(segs))
        assert math.isnan(m["rem_latency_s"])

    def test_stage_percentages_sum_to_100(self):
        segs = [
            StageSegment(0, 1200, "light"),
            StageSegment(1200, 1800, "deep"),
            StageSegment(1800, 2400, "rem"),
        ]
        m = hypnogram_metrics(self._session(segs))
        total_pct = m["light_pct"] + m["deep_pct"] + m["rem_pct"]
        assert total_pct == pytest.approx(100.0)

    def test_all_wake_no_sleep(self):
        segs = [StageSegment(0, 3600, "wake")]
        m = hypnogram_metrics(self._session(segs))
        assert m["tst_s"] == 0
        assert m["efficiency"] == 0.0
        assert m["sol_s"] == pytest.approx(3600)  # never fell asleep


# ---------------------------------------------------------------------------
# ALG-10 — sleep_performance_score (pure, no DB)
# ---------------------------------------------------------------------------


class TestSleepPerformanceScore:
    """ALG-10 Sleep Performance composite score (APPROXIMATE).

    Weights W_dur=0.45, W_eff=0.25, W_stg=0.20, W_con=0.10. Always clamped
    to [0.0, 100.0]. Pure function — no streams, no DB.
    """

    def test_perfect_sleep_saturates_to_100(self):
        # 8h TST, 100% efficiency, restorative ratio 40% (96+96)/480, 0 disturbances
        s = sleep_performance_score(480, 1.0, 96, 96, 0, 420)
        assert abs(s - 100.0) < 0.1

    def test_zero_sleep_returns_zero(self):
        s = sleep_performance_score(0, 0.0, 0, 0, 0, 420)
        assert s == 0.0

    def test_typical_night_in_plausible_range(self):
        s = sleep_performance_score(420, 0.85, 70, 70, 3, 420)
        assert 70.0 <= s <= 95.0

    def test_clamps_above_100(self):
        s = sleep_performance_score(1000, 1.0, 500, 500, 0, 420)
        assert s == 100.0

    def test_sleep_needed_fallback_420(self):
        # Omitting sleep_needed_min must behave identically to passing 420.0
        a = sleep_performance_score(420, 0.85, 70, 70, 3)
        b = sleep_performance_score(420, 0.85, 70, 70, 3, 420)
        assert a == b

    def test_result_is_clamped_non_negative(self):
        # disturbances dominate but score never drops below 0
        s = sleep_performance_score(0, 0.0, 0, 0, 100, 420)
        assert s >= 0.0

    def test_returns_float(self):
        s = sleep_performance_score(480, 1.0, 96, 96, 0, 420)
        assert isinstance(s, float)
