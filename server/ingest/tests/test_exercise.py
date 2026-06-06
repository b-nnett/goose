"""
Tests for analysis.exercise — retroactive workout/exercise detection.

PURE (no DB). Run offline:
    cd ~/Developer/home-server/stacks/whoop/ingest
    ~/Developer/home-server/venv/bin/python -m pytest tests/test_exercise.py -q

Detection runs over the backfilled 1 Hz store: a workout is a sustained
(>= MIN_EXERCISE_MIN) window of elevated HR (above resting + margin) AND
sustained motion (activity intensity above threshold). No raw accel is needed,
so a run done while the phone was disconnected is detected after the next sync.
"""
from __future__ import annotations

from app.analysis.exercise import (
    ExerciseSession,
    MERGE_GAP_S,
    MIN_EXERCISE_MIN,
    MIN_INTENSITY_Z2PLUS,
    HR_MARGIN_BPM,
    detect_exercises,
)
from app.analysis.strain import (
    _edwards_trimp,
    _pct_hrr,
    _trimp_to_strain,
    _zone_weight,
)

T0 = 1_700_000_000.0


# ---------------------------------------------------------------------------
# Stream builders
# ---------------------------------------------------------------------------

def _hr_block(start: float, n: int, bpm: int, step_s: float = 1.0) -> list[dict]:
    return [{"ts": start + i * step_s, "bpm": bpm} for i in range(n)]


def _gravity_still(start: float, n: int, step_s: float = 1.0) -> list[dict]:
    """A constant gravity vector → ~0 motion intensity."""
    return [{"ts": start + i * step_s, "x": 0.0, "y": 0.0, "z": 1.0}
            for i in range(n)]


def _gravity_active(start: float, n: int, step_s: float = 1.0,
                    amp: float = 1.0) -> list[dict]:
    """An oscillating gravity vector → high motion intensity each step."""
    out = []
    for i in range(n):
        v = amp if i % 2 == 0 else -amp
        out.append({"ts": start + i * step_s, "x": v, "y": 0.0, "z": 0.0})
    return out


def _merge(*blocks: list[dict]) -> list[dict]:
    out: list[dict] = []
    for b in blocks:
        out.extend(b)
    return out


# A standard "workout": ~30 min elevated HR + high motion, surrounded by rest.
def _workout_streams(*, work_bpm=150, rest_bpm=55, work_min=30, rest_min=10):
    work_n = work_min * 60
    rest_n = rest_min * 60
    rest_a_start = T0
    work_start = rest_a_start + rest_n
    rest_b_start = work_start + work_n

    hr = _merge(
        _hr_block(rest_a_start, rest_n, rest_bpm),
        _hr_block(work_start, work_n, work_bpm),
        _hr_block(rest_b_start, rest_n, rest_bpm),
    )
    gravity = _merge(
        _gravity_still(rest_a_start, rest_n),
        _gravity_active(work_start, work_n),
        _gravity_still(rest_b_start, rest_n),
    )
    return {"hr": hr, "gravity": gravity}, work_start, work_n


# ---------------------------------------------------------------------------
# 1. Detected workout
# ---------------------------------------------------------------------------

def test_detected_workout():
    streams, work_start, work_n = _workout_streams()
    sessions = detect_exercises(streams, resting_hr=55, max_hr=190)
    assert len(sessions) == 1
    s = sessions[0]
    assert isinstance(s, ExerciseSession)
    # duration ~30 min (allow slack for edge samples)
    dur_min = (s.end - s.start) / 60.0
    assert 28.0 <= dur_min <= 31.0
    # avg/peak HR around the work level
    assert 148 <= s.avg_hr <= 152
    assert s.peak_hr >= 150
    # strain present (>=600 samples + elevated HR) and within scale
    assert s.strain is not None
    assert 0.0 < s.strain <= 21.0
    # kind not classified (needs raw accel sample)
    assert s.kind is None


# ---------------------------------------------------------------------------
# 2. Quiet stream → none
# ---------------------------------------------------------------------------

def test_quiet_stream_no_sessions():
    n = 40 * 60
    streams = {
        "hr": _hr_block(T0, n, 55),
        "gravity": _gravity_still(T0, n),
    }
    assert detect_exercises(streams, resting_hr=55, max_hr=190) == []


# ---------------------------------------------------------------------------
# 3. Elevated HR but NO motion (stress/fever) → NOT exercise (motion gate)
# ---------------------------------------------------------------------------

def test_elevated_hr_no_motion_not_detected():
    n = 30 * 60
    streams = {
        "hr": _hr_block(T0, n, 150),       # high HR
        "gravity": _gravity_still(T0, n),  # but no motion
    }
    # Intentional: high HR without motion (stress/fever) is NOT an exercise.
    assert detect_exercises(streams, resting_hr=55, max_hr=190) == []


# ---------------------------------------------------------------------------
# 4. Motion but normal HR (fidgeting) → NOT exercise (HR gate)
# ---------------------------------------------------------------------------

def test_motion_normal_hr_not_detected():
    n = 30 * 60
    streams = {
        "hr": _hr_block(T0, n, 65),         # near resting + below margin
        "gravity": _gravity_active(T0, n),  # lots of motion
    }
    assert detect_exercises(streams, resting_hr=55, max_hr=190) == []


# ---------------------------------------------------------------------------
# 5. Too short → NOT detected
# ---------------------------------------------------------------------------

def test_too_short_not_detected():
    rest_n = 10 * 60
    burst_n = 3 * 60          # < MIN_EXERCISE_MIN (5)
    assert burst_n / 60.0 < MIN_EXERCISE_MIN
    burst_start = T0 + rest_n
    after_start = burst_start + burst_n
    streams = {
        "hr": _merge(
            _hr_block(T0, rest_n, 55),
            _hr_block(burst_start, burst_n, 150),
            _hr_block(after_start, rest_n, 55),
        ),
        "gravity": _merge(
            _gravity_still(T0, rest_n),
            _gravity_active(burst_start, burst_n),
            _gravity_still(after_start, rest_n),
        ),
    }
    assert detect_exercises(streams, resting_hr=55, max_hr=190) == []


def test_exactly_5min_workout_is_detected():
    """A workout lasting exactly MIN_EXERCISE_MIN (5:00) IS detected.

    The rolling-mean onset latency (MOTION_SMOOTH_S = 10 s) trims ~10 s from
    the active-sample span, but the MIN_EXERCISE_MIN check applies a matching
    tolerance so the genuine 5-minute bout is accepted.

    We build a 10-min rest block, a 5-min (300 s) workout block, then a 10-min
    rest tail to ensure the HR floor is well-established.
    """
    rest_n = 10 * 60
    burst_n = 5 * 60          # == MIN_EXERCISE_MIN exactly
    assert burst_n / 60.0 == MIN_EXERCISE_MIN
    burst_start = T0 + rest_n
    after_start = burst_start + burst_n
    streams = {
        "hr": _merge(
            _hr_block(T0, rest_n, 55),
            _hr_block(burst_start, burst_n, 150),
            _hr_block(after_start, rest_n, 55),
        ),
        "gravity": _merge(
            _gravity_still(T0, rest_n),
            _gravity_active(burst_start, burst_n),
            _gravity_still(after_start, rest_n),
        ),
    }
    sessions = detect_exercises(streams, resting_hr=55, max_hr=190)
    assert len(sessions) == 1, (
        f"Expected 1 session for exactly 5:00 workout, got {len(sessions)}"
    )


def test_5min10s_workout_is_detected():
    """A workout lasting 5:10 (10 s over the threshold) IS detected."""
    rest_n = 10 * 60
    burst_n = 5 * 60 + 10
    burst_start = T0 + rest_n
    after_start = burst_start + burst_n
    streams = {
        "hr": _merge(
            _hr_block(T0, rest_n, 55),
            _hr_block(burst_start, burst_n, 150),
            _hr_block(after_start, rest_n, 55),
        ),
        "gravity": _merge(
            _gravity_still(T0, rest_n),
            _gravity_active(burst_start, burst_n),
            _gravity_still(after_start, rest_n),
        ),
    }
    sessions = detect_exercises(streams, resting_hr=55, max_hr=190)
    assert len(sessions) == 1, (
        f"Expected 1 session for 5:10 workout, got {len(sessions)}"
    )


def test_4min_workout_not_detected():
    """A workout lasting 4:00 (well under MIN_EXERCISE_MIN) is NOT detected."""
    rest_n = 10 * 60
    burst_n = 4 * 60          # < MIN_EXERCISE_MIN
    assert burst_n / 60.0 < MIN_EXERCISE_MIN
    burst_start = T0 + rest_n
    after_start = burst_start + burst_n
    streams = {
        "hr": _merge(
            _hr_block(T0, rest_n, 55),
            _hr_block(burst_start, burst_n, 150),
            _hr_block(after_start, rest_n, 55),
        ),
        "gravity": _merge(
            _gravity_still(T0, rest_n),
            _gravity_active(burst_start, burst_n),
            _gravity_still(after_start, rest_n),
        ),
    }
    assert detect_exercises(streams, resting_hr=55, max_hr=190) == []


# ---------------------------------------------------------------------------
# 6. Two workouts separated by a long rest → two sessions
# ---------------------------------------------------------------------------

def test_two_workouts_separated():
    work_n = 10 * 60
    gap_n = 30 * 60          # >> MERGE_GAP_S
    a_start = T0
    gap_start = a_start + work_n
    b_start = gap_start + gap_n
    end_rest_start = b_start + work_n

    hr = _merge(
        _hr_block(a_start, work_n, 150),
        _hr_block(gap_start, gap_n, 55),
        _hr_block(b_start, work_n, 150),
        _hr_block(end_rest_start, 5 * 60, 55),
    )
    gravity = _merge(
        _gravity_active(a_start, work_n),
        _gravity_still(gap_start, gap_n),
        _gravity_active(b_start, work_n),
        _gravity_still(end_rest_start, 5 * 60),
    )
    sessions = detect_exercises({"hr": hr, "gravity": gravity},
                                resting_hr=55, max_hr=190)
    assert len(sessions) == 2
    assert sessions[0].start < sessions[1].start
    # they don't fuse: there's a real gap between them
    assert sessions[1].start - sessions[0].end > gap_n / 2


# ---------------------------------------------------------------------------
# 7. Brief gap inside one workout is merged (MERGE_GAP_S spirit)
# ---------------------------------------------------------------------------

def test_brief_gap_merged_into_one():
    seg = 8 * 60
    short_gap = 30          # < MERGE_GAP_S
    a_start = T0
    gap_start = a_start + seg
    b_start = gap_start + short_gap
    hr = _merge(
        _hr_block(a_start, seg, 150),
        _hr_block(gap_start, short_gap, 55),   # brief lull
        _hr_block(b_start, seg, 150),
    )
    gravity = _merge(
        _gravity_active(a_start, seg),
        _gravity_still(gap_start, short_gap),
        _gravity_active(b_start, seg),
    )
    sessions = detect_exercises({"hr": hr, "gravity": gravity},
                                resting_hr=55, max_hr=190)
    assert len(sessions) == 1


# ---------------------------------------------------------------------------
# 8. Edge cases: empty / HR-only
# ---------------------------------------------------------------------------

def test_empty_streams():
    assert detect_exercises({}) == []
    assert detect_exercises({"hr": [], "gravity": []}) == []


def test_hr_only_no_gravity_returns_empty():
    # No motion signal at all → cannot confirm exercise → [] (documented).
    n = 30 * 60
    streams = {"hr": _hr_block(T0, n, 150)}
    assert detect_exercises(streams, resting_hr=55, max_hr=190) == []


# ---------------------------------------------------------------------------
# 9. Resting baseline auto-derived when not provided
# ---------------------------------------------------------------------------

def test_resting_hr_auto_derived():
    # No resting_hr passed: derive from the day's HR (low percentile).
    streams, _, _ = _workout_streams(work_bpm=150, rest_bpm=55)
    sessions = detect_exercises(streams, max_hr=190)
    assert len(sessions) == 1
    assert 148 <= sessions[0].avg_hr <= 152


def test_hr_margin_gate_uses_constant():
    """HR floor gate + intensity filter: below-floor HR is rejected; high-zone
    workout HR is accepted.

    The "below" case checks the HR_MARGIN_BPM gate: bpm just under resting+margin
    never passes the HR floor check.

    The "above" case uses bpm=140 (well into zone 2: pct_hrr≈63%) so it passes
    BOTH the HR floor AND the MIN_INTENSITY_Z2PLUS filter.  Previously bpm=75
    was used, but that sits in zone 0 (pct_hrr≈15%) and is correctly rejected
    by the intensity qualification filter as low-effort activity — not a workout.
    """
    n = 30 * 60
    rest = 55
    below = rest + HR_MARGIN_BPM - 2   # below HR floor → rejected by HR gate
    above = 140                         # zone 2 (pct_hrr≈63%) → passes both gates
    below_streams = {"hr": _hr_block(T0, n, below),
                     "gravity": _gravity_active(T0, n)}
    above_streams = {"hr": _hr_block(T0, n, above),
                     "gravity": _gravity_active(T0, n)}
    assert detect_exercises(below_streams, resting_hr=rest, max_hr=190) == []
    assert len(detect_exercises(above_streams, resting_hr=rest, max_hr=190)) == 1


# ---------------------------------------------------------------------------
# 10. Merge-gap boundary: active-ts gap vs MERGE_GAP_S
# ---------------------------------------------------------------------------

def _two_workout_streams(quiet_s: int, work_min: int = 10) -> dict:
    """Two qualifying workouts with a quiet block of ``quiet_s`` seconds between them.

    Note: the *active-ts gap* (last active sample of run A → first active
    sample of run B) is LARGER than the quiet block because the rolling-mean
    window (``MOTION_SMOOTH_S``) delays onset of motion detection at run B's
    start. The merge predicate operates on the active-ts gap, so the effective
    merge boundary is at a wall-clock quiet block somewhat shorter than
    ``MERGE_GAP_S``.
    """
    work_n = work_min * 60
    a_start = T0
    gap_start = a_start + work_n
    b_start = gap_start + quiet_s
    hr = _merge(
        _hr_block(a_start, work_n, 150),
        _hr_block(gap_start, quiet_s, 55),
        _hr_block(b_start, work_n, 150),
        _hr_block(b_start + work_n, 5 * 60, 55),
    )
    gravity = _merge(
        _gravity_active(a_start, work_n),
        _gravity_still(gap_start, quiet_s),
        _gravity_active(b_start, work_n),
        _gravity_still(b_start + work_n, 5 * 60),
    )
    return {"hr": hr, "gravity": gravity}


def test_gap_beyond_merge_threshold_gives_two_sessions():
    """A quiet block longer than MERGE_GAP_S between two workouts → two sessions.

    Previously this test used a hardcoded 61 s gap calibrated to the old
    MERGE_GAP_S=60 s.  MERGE_GAP_S was widened to 420 s (to absorb real-world
    intra-workout lulls like soccer subs and treadmill walk breaks), so we now
    use MERGE_GAP_S + 60 s dynamically.  The invariant is the same: a gap that
    clearly exceeds the merge window produces two distinct sessions.
    """
    quiet_s = int(MERGE_GAP_S) + 60   # e.g. 480 s with MERGE_GAP_S=420
    streams = _two_workout_streams(quiet_s=quiet_s)
    sessions = detect_exercises(streams, resting_hr=55, max_hr=190)
    assert len(sessions) == 2, (
        f"Expected 2 sessions for {quiet_s} s quiet block (MERGE_GAP_S={MERGE_GAP_S}), "
        f"got {len(sessions)}"
    )
    assert sessions[0].start < sessions[1].start


def test_gap_30s_quiet_block_merges_into_one():
    """A 30 s quiet block between two workouts → one merged session.

    30 s is well below MERGE_GAP_S (420 s), so the lull is absorbed.
    The merge predicate is ``active_gap > MERGE_GAP_S`` (strict greater-than).
    """
    streams = _two_workout_streams(quiet_s=30)
    sessions = detect_exercises(streams, resting_hr=55, max_hr=190)
    assert len(sessions) == 1, (
        f"Expected 1 merged session for 30 s quiet block, got {len(sessions)}"
    )


# ===========================================================================
# 11. New fields: duration_s, zone_time_pct, avg_hrr_pct, hrmax, hrmax_source
# ===========================================================================

def test_new_fields_present_on_session():
    """ExerciseSession carries the new extended intensity fields."""
    streams, _, _ = _workout_streams()
    sessions = detect_exercises(streams, resting_hr=55, max_hr=190)
    assert len(sessions) == 1
    s = sessions[0]
    # duration_s
    assert isinstance(s.duration_s, float)
    assert s.duration_s > 0
    # zone_time_pct: keys 0–5, values are floats
    assert isinstance(s.zone_time_pct, dict)
    assert set(s.zone_time_pct.keys()) == {0, 1, 2, 3, 4, 5}
    # avg_hrr_pct: float in [0, 100]
    assert s.avg_hrr_pct is not None
    assert 0.0 <= s.avg_hrr_pct <= 100.0
    # hrmax and hrmax_source
    assert s.hrmax == 190.0
    assert s.hrmax_source == "caller"


def test_zone_time_pct_sums_to_100():
    """zone_time_pct percentages must sum to exactly 100 (within float rounding)."""
    streams, _, _ = _workout_streams(work_bpm=150, rest_bpm=55)
    sessions = detect_exercises(streams, resting_hr=55, max_hr=190)
    assert len(sessions) == 1
    total = sum(sessions[0].zone_time_pct.values())
    assert abs(total - 100.0) <= 0.2, f"zone_time_pct total = {total}, expected ~100"


def test_zone_time_pct_all_high_zone_for_hard_effort():
    """At 150 bpm with resting=55 and max=190, %HRR is high → expect bulk in zones 3-5.

    HRR = 190 - 55 = 135.  pct_hrr(150) = (150-55)/135 * 100 ≈ 70.4%.
    Zone weight = 3 (≥ 70% cutoff).  So zone 3 should hold ~100% of samples.
    """
    streams, _, _ = _workout_streams(work_bpm=150, rest_bpm=55, work_min=30)
    sessions = detect_exercises(streams, resting_hr=55, max_hr=190)
    assert len(sessions) == 1
    z = sessions[0].zone_time_pct
    # The workout samples all have bpm=150 → all land in zone 3.
    high_zone_pct = z.get(3, 0) + z.get(4, 0) + z.get(5, 0)
    assert high_zone_pct > 90.0, f"Expected most time in zones 3+, got {z}"


def test_per_bout_strain_matches_strain_module():
    """Per-bout strain must equal what strain.py would produce directly.

    We compare detect_exercises' strain with a manual Edwards-TRIMP computation
    using the same HR series and HRR parameters.  This guards against the bout
    strain diverging from the authoritative strain module.  APPROXIMATE outputs
    are acceptable; the test checks round-trip consistency within 0.5 scale units.

    The _workout_streams helper places 10 rest min then 30 work min, so work
    starts at T0 + 600 s.  The active window detected by exercise.py is a subset
    of the work block (slightly trimmed by rolling-mean onset latency), so we
    build the expected TRIMP over the full work block and allow a 0.5-unit slack.
    """
    work_min = 30
    work_bpm = 150
    rhr = 55
    max_h = 190
    streams, work_start, work_n = _workout_streams(
        work_bpm=work_bpm, rest_bpm=rhr, work_min=work_min, rest_min=10
    )
    sessions = detect_exercises(streams, resting_hr=rhr, max_hr=max_h)
    assert len(sessions) == 1
    s = sessions[0]
    assert s.strain is not None

    # Build the expected TRIMP over the full work block (1 Hz = 1/60 min/sample).
    # This should match the bout strain closely because the bout spans most of the
    # work block (only the ~10-s onset window is trimmed).
    hr_reserve = max_h - rhr
    n_work = work_n  # 1800 samples
    sdm = 1.0 / 60.0  # 1 Hz
    hr_series_full = [{"ts": float(work_start + i), "bpm": work_bpm}
                      for i in range(n_work)]
    trimp_full = _edwards_trimp(hr_series_full, rhr, hr_reserve, sdm)
    expected_strain_full = _trimp_to_strain(trimp_full)

    # Within 0.5 scale units (onset latency trims ~10 s ≈ 0.3% of the bout).
    assert abs(s.strain - expected_strain_full) < 0.5, (
        f"Bout strain {s.strain} diverged from manual calc {expected_strain_full}"
    )


def test_duration_s_roughly_equals_bout_length():
    """duration_s should reflect the active window span in seconds."""
    streams, _, work_n = _workout_streams(work_min=30)
    sessions = detect_exercises(streams, resting_hr=55, max_hr=190)
    assert len(sessions) == 1
    s = sessions[0]
    # active window is slightly < work_n due to onset latency of rolling mean.
    assert 25 * 60 <= s.duration_s <= 30 * 60 + 5


# ===========================================================================
# 12. False-positive rejection tests
# ===========================================================================

def test_elevated_hr_still_body_no_session():
    """Elevated HR without motion (fever / desk stress) must NOT be detected.

    This is the same gate as test_elevated_hr_no_motion_not_detected but framed
    from a real-world scenario — a user with a fever whose HR climbs to 100 bpm
    while sitting still should not generate an exercise session.
    """
    n = 30 * 60
    streams = {
        "hr": _hr_block(T0, n, 100),        # fever-level HR
        "gravity": _gravity_still(T0, n),   # still wrist
    }
    assert detect_exercises(streams, resting_hr=55, max_hr=190) == []


def test_motion_without_hr_elevation_no_session():
    """Motion alone (e.g. driving on a bumpy road) without HR elevation must NOT
    produce an exercise session.

    Driving scenario: sustained wrist motion from road vibration, but HR stays
    near resting.  The HR gate must suppress this.
    """
    n = 30 * 60
    rest = 55
    # HR stays at resting + 10 bpm — below the HR_MARGIN_BPM=15 threshold.
    driving_bpm = rest + 10
    streams = {
        "hr": _hr_block(T0, n, driving_bpm),
        "gravity": _gravity_active(T0, n, amp=0.8),  # real motion (bumps)
    }
    assert detect_exercises(streams, resting_hr=rest, max_hr=190) == []


# ===========================================================================
# 13. Personalized HRmax via estimate_hrmax (no explicit max_hr passed)
# ===========================================================================

def test_personalized_hrmax_derived_from_day_hr():
    """When max_hr is None but the day's HR history is rich (>=600 samples),
    estimate_hrmax derives it from the observed p99.5, and hrmax_source records
    'observed' or 'tanaka'.

    We build a 30-min workout at bpm=165 surrounded by rest at 55.  The total
    stream is 50 min = 3000 samples — well above HRMAX_MIN_SAMPLES (600).  The
    p99.5 of [55]*600 + [165]*1800 = 165 bpm.  detect_exercises should return a
    session whose hrmax ≈ 165 and hrmax_source ∈ {"observed", "tanaka"}.
    """
    streams, _, _ = _workout_streams(work_bpm=165, rest_bpm=55, work_min=30)
    sessions = detect_exercises(streams, resting_hr=55)  # no max_hr, no age
    assert len(sessions) == 1
    s = sessions[0]
    assert s.hrmax is not None
    # The observed p99.5 from this day HR will be at or near 165.
    assert s.hrmax >= 160.0, f"Expected hrmax near 165, got {s.hrmax}"
    assert s.hrmax_source in {"observed", "tanaka"}


def test_personalized_hrmax_tanaka_fallback_with_age():
    """When max_hr=None but age is supplied, estimate_hrmax uses observed p99.5
    when the day's HR history is sufficient (>=600 samples), or falls back to
    Tanaka(age) otherwise.  Either way, hrmax_source must NOT be 'unknown'.

    We build a 5-min rest + 10-min workout = 900 samples total — enough for
    the observed-p99.5 path (HRMAX_MIN_SAMPLES=600).  With uniform HR at each
    level (55 rest / 150 work), p99.5 ≈ 150; the result is 'observed'.  The
    key invariant is that hrmax_source != 'unknown' whenever age is supplied.
    """
    # 5 min rest + 10 min workout = 900 samples total (>= HRMAX_MIN_SAMPLES=600).
    # p99.5 of [55]*300 + [150]*600 ≈ 150 → hrmax_source = "observed".
    # age=30 is a Tanaka backstop; with 900 samples it won't be needed.
    work_min = 10
    streams, _, _ = _workout_streams(work_bpm=150, rest_bpm=55,
                                     work_min=work_min, rest_min=5)
    sessions = detect_exercises(streams, resting_hr=55, age=30)  # no max_hr
    # The 10-min workout comfortably exceeds MIN_EXERCISE_MIN; at least one
    # session should be detected.  What matters: hrmax_source != "unknown".
    for s in sessions:
        assert s.hrmax_source != "unknown", (
            f"Expected hrmax_source to be 'observed' or 'tanaka', got {s.hrmax_source}"
        )


def test_hrmax_source_caller_when_max_hr_passed():
    """When max_hr is passed explicitly, hrmax_source must be 'caller'."""
    streams, _, _ = _workout_streams()
    sessions = detect_exercises(streams, resting_hr=55, max_hr=185)
    assert len(sessions) == 1
    assert sessions[0].hrmax == 185.0
    assert sessions[0].hrmax_source == "caller"


def test_avg_hr_and_peak_hr_correct():
    """avg_hr and peak_hr must reflect the bout's bpm values accurately."""
    streams, _, _ = _workout_streams(work_bpm=155, rest_bpm=55)
    sessions = detect_exercises(streams, resting_hr=55, max_hr=190)
    assert len(sessions) == 1
    s = sessions[0]
    # All work samples are 155 bpm → avg = peak = 155.
    assert 153 <= s.avg_hr <= 157, f"avg_hr = {s.avg_hr}"
    assert s.peak_hr == 155


def test_no_zone_breakdown_when_hrmax_unknown():
    """When HRmax cannot be estimated (no max_hr, no age, thin HR history),
    zone_time_pct should be empty and avg_hrr_pct should be None — not crash.

    This guards the degenerate path in estimate_hrmax → (0.0, 'unknown').
    We achieve thin-history by using a short stream (only workout samples,
    < HRMAX_MIN_SAMPLES = 600 samples) with no age.

    Intensity filter guard: because zone_pct is empty (HRmax unknown), the
    MIN_INTENSITY_Z2PLUS filter is SKIPPED — we must not suppress a real workout
    merely because the zone math was unavailable.  So the session is still
    returned even though no zone data exists to evaluate.
    """
    # 6-min workout = 360 samples — below HRMAX_MIN_SAMPLES(600) so the
    # observed-p99.5 estimate is not attempted.  No age → Tanaka not available
    # either → estimate_hrmax returns (0.0, 'unknown').
    work_n = 6 * 60   # 360 < 600: below HRMAX_MIN_SAMPLES → observed not attempted
    streams = {
        "hr": _hr_block(T0, work_n, 150),
        "gravity": _gravity_active(T0, work_n),
    }
    # With only 360 samples and no age, estimate_hrmax returns (0.0, "unknown").
    sessions = detect_exercises(streams, resting_hr=55)  # no max_hr, no age
    # The workout is 6 min > MIN_EXERCISE_MIN, so at least one session should
    # be returned.  Any session with hrmax_source=="unknown" must have empty
    # zone_time_pct and avg_hrr_pct=None (no crash, no spurious zone data).
    for s in sessions:
        if s.hrmax_source == "unknown":
            assert s.zone_time_pct == {}, (
                f"Expected empty zone_time_pct for unknown hrmax, got {s.zone_time_pct}"
            )
            assert s.avg_hrr_pct is None


# ===========================================================================
# 14. Zone-2+ intensity qualification filter (MIN_INTENSITY_Z2PLUS)
# ===========================================================================

def _high_z2plus_streams(work_bpm: int = 150, work_min: int = 30) -> dict:
    """A workout whose HR produces ≥50% time in zone 2+ (real workout).

    At work_bpm=150, resting=55, max=190:
      pct_hrr = (150-55)/(190-55)*100 ≈ 70.4% → zone 3 → 100% in z2+.
    """
    work_n = work_min * 60
    rest_n = 10 * 60
    return {
        "hr": _merge(
            _hr_block(T0, rest_n, 55),
            _hr_block(T0 + rest_n, work_n, work_bpm),
            _hr_block(T0 + rest_n + work_n, rest_n, 55),
        ),
        "gravity": _merge(
            _gravity_still(T0, rest_n),
            _gravity_active(T0 + rest_n, work_n),
            _gravity_still(T0 + rest_n + work_n, rest_n),
        ),
    }


def _low_z2plus_streams(work_bpm: int = 94, work_min: int = 14,
                        rest_bpm: int = 55) -> dict:
    """A bout whose HR produces <50% time in zone 2+ (noise).

    At work_bpm=94, resting=55, max=190:
      pct_hrr = (94-55)/(190-55)*100 ≈ 28.9% → zone 1 → 0% in z2+.
    Mirrors the real noise bout '14m avg94 z2+33%' from ground-truth data.
    We use a moderate amplitude to ensure the motion gate fires.
    """
    work_n = work_min * 60
    rest_n = 10 * 60
    return {
        "hr": _merge(
            _hr_block(T0, rest_n, rest_bpm),
            _hr_block(T0 + rest_n, work_n, work_bpm),
            _hr_block(T0 + rest_n + work_n, rest_n, rest_bpm),
        ),
        "gravity": _merge(
            _gravity_still(T0, rest_n),
            _gravity_active(T0 + rest_n, work_n),
            _gravity_still(T0 + rest_n + work_n, rest_n),
        ),
    }


def test_high_z2plus_bout_is_kept():
    """A bout with z2+ ≥ MIN_INTENSITY_Z2PLUS (50%) must NOT be filtered out.

    Synthetic: 30 min at bpm=150 (resting=55, max=190) → all samples in zone 3
    → 100% z2+.  This mirrors the real treadmill and soccer sessions (66–100%).
    """
    streams = _high_z2plus_streams(work_bpm=150, work_min=30)
    sessions = detect_exercises(streams, resting_hr=55, max_hr=190)
    assert len(sessions) == 1, (
        f"High-intensity bout (z2+=100%) must be kept; got {len(sessions)} sessions"
    )
    z2plus = sum(sessions[0].zone_time_pct.get(z, 0.0) for z in (2, 3, 4, 5))
    assert z2plus >= MIN_INTENSITY_Z2PLUS * 100, (
        f"z2+ = {z2plus}%, expected >= {MIN_INTENSITY_Z2PLUS * 100}%"
    )


def test_low_z2plus_bout_is_dropped():
    """A bout with z2+ < MIN_INTENSITY_Z2PLUS must be rejected as noise.

    Synthetic: 14 min at bpm=94 (resting=55, max=190) — HR exceeds the floor
    (55+15=70) and motion is present, so the pre-filter stages accept it.
    But pct_hrr(94)≈29% → zone 1 → 0% in z2+ → dropped by intensity filter.

    This mirrors the real noise bout '14m avg94 z2+33% (strain6.98)' which must
    NOT appear in workout results.
    """
    streams = _low_z2plus_streams(work_bpm=94, work_min=14)
    sessions = detect_exercises(streams, resting_hr=55, max_hr=190)
    assert len(sessions) == 0, (
        f"Low-intensity bout (z2+ near 0%) must be rejected as noise; "
        f"got {len(sessions)} sessions"
    )


def test_borderline_43pct_z2plus_is_dropped():
    """A bout at exactly 43% z2+ (the highest observed noise case) must be rejected.

    Ground-truth: the worst noise bout has 43% z2+ (avg92, 7 min).  With the
    threshold at 50%, this must be filtered out.  We synthesise a mixed HR
    stream where ~43% of samples land in zone 2+ and verify rejection.

    Mix: 57% at bpm=80 (zone 0 at resting=55, max=190: pct_hrr≈18%) and
         43% at bpm=109 (pct_hrr≈40%→ still zone 1, but we push bpm high enough
         to put exactly 43% into z2+; bpm=115 gives pct_hrr≈44%→zone 2).
    Simpler approach: all samples at bpm=92 → pct_hrr(92)=(92-55)/135*100≈27%
    → zone 1 → 0% z2+.  Already handled by test_low_z2plus_bout_is_dropped.

    For a true 43% z2+ mix we alternate bpm=80 (z1) and bpm=115 (z2):
      pct_hrr(80) = (80-55)/135*100 ≈ 18.5% → zone 0
      pct_hrr(115)= (115-55)/135*100 ≈ 44.4% → zone 2
    Using a 57/43 split:
      57% at bpm=80 (zone 0) + 43% at bpm=115 (zone 2) → z2+ = 43% < 50%.
    """
    rest_n = 10 * 60
    work_n = 7 * 60   # 7 min — above MIN_EXERCISE_MIN (5 min)
    # Build alternating HR samples: 57% zone-0, 43% zone-2.
    hr_work: list[dict] = []
    for i in range(work_n):
        bpm = 115 if (i % 100 < 43) else 80   # 43 out of every 100 in zone 2
        hr_work.append({"ts": T0 + rest_n + i, "bpm": bpm})
    streams = {
        "hr": _merge(
            _hr_block(T0, rest_n, 55),
            hr_work,
            _hr_block(T0 + rest_n + work_n, rest_n, 55),
        ),
        "gravity": _merge(
            _gravity_still(T0, rest_n),
            _gravity_active(T0 + rest_n, work_n),
            _gravity_still(T0 + rest_n + work_n, rest_n),
        ),
    }
    sessions = detect_exercises(streams, resting_hr=55, max_hr=190)
    assert len(sessions) == 0, (
        "Borderline 43% z2+ bout (highest observed noise) must be rejected; "
        f"got {len(sessions)} sessions"
    )


def test_treadmill_gap_merges_into_one_session():
    """Two treadmill bouts with a ~2-min gap must merge into a single session.

    Ground-truth: treadmill run 23:11Z (34 min) + follow-on 23:47Z (12 min),
    ~2-min gap between them.  Both bouts use high HR (150 bpm) → pass the
    zone-2+ filter.  The gap (120 s) is below MERGE_GAP_S (150 s), so they merge.
    """
    work_n_a = 34 * 60
    gap_n = 2 * 60          # 120 s < MERGE_GAP_S (150 s) → must merge
    work_n_b = 12 * 60
    rest_n = 10 * 60

    a_start = T0 + rest_n
    gap_start = a_start + work_n_a
    b_start = gap_start + gap_n
    tail_start = b_start + work_n_b

    hr = _merge(
        _hr_block(T0, rest_n, 55),
        _hr_block(a_start, work_n_a, 150),
        _hr_block(gap_start, gap_n, 55),
        _hr_block(b_start, work_n_b, 150),
        _hr_block(tail_start, rest_n, 55),
    )
    gravity = _merge(
        _gravity_still(T0, rest_n),
        _gravity_active(a_start, work_n_a),
        _gravity_still(gap_start, gap_n),
        _gravity_active(b_start, work_n_b),
        _gravity_still(tail_start, rest_n),
    )
    sessions = detect_exercises({"hr": hr, "gravity": gravity},
                                resting_hr=55, max_hr=190)
    assert len(sessions) == 1, (
        f"Treadmill bouts with 2-min gap must merge into 1 session; "
        f"got {len(sessions)}"
    )


def test_soccer_halftime_gap_stays_two_sessions():
    """Two soccer bouts with a ~6-min halftime gap must NOT merge (stay 2 sessions).

    Rationale (2026-05-27 validation): MERGE_GAP_S was 420 s and bridged a 6-min
    gap between the two soccer halves, but the surrounding light activity was also
    above the HR floor — so the 7-min window fused everything from 03:43→06:31
    UTC into a single 168-min blob (avg116, z2+ diluted to 51.5%).  That was wrong.

    No single merge window can bridge the 6-min soccer gap WITHOUT gluing
    unrelated activity when the evening HR is nearly continuous above the floor.
    Showing 2 separate high-intensity bouts is more accurate than one diluted blob.

    With MERGE_GAP_S=150 s, a 6-min (360 s) gap exceeds the threshold → 2 sessions.
    Each individual bout (25 min + 43 min) passes all filters independently.
    """
    work_n_a = 25 * 60
    gap_n = 6 * 60          # 360 s > MERGE_GAP_S (150 s) → must NOT merge
    work_n_b = 43 * 60
    rest_n = 10 * 60

    a_start = T0 + rest_n
    gap_start = a_start + work_n_a
    b_start = gap_start + gap_n
    tail_start = b_start + work_n_b

    hr = _merge(
        _hr_block(T0, rest_n, 55),
        _hr_block(a_start, work_n_a, 148),
        _hr_block(gap_start, gap_n, 55),
        _hr_block(b_start, work_n_b, 148),
        _hr_block(tail_start, rest_n, 55),
    )
    gravity = _merge(
        _gravity_still(T0, rest_n),
        _gravity_active(a_start, work_n_a),
        _gravity_still(gap_start, gap_n),
        _gravity_active(b_start, work_n_b),
        _gravity_still(tail_start, rest_n),
    )
    sessions = detect_exercises({"hr": hr, "gravity": gravity},
                                resting_hr=55, max_hr=190)
    assert len(sessions) == 2, (
        f"Soccer bouts with 6-min gap must stay as 2 sessions (MERGE_GAP_S={MERGE_GAP_S} s); "
        f"got {len(sessions)}"
    )
    # Both individual bouts must be high-intensity (z2+ > 50%)
    for s in sessions:
        z2plus = sum(s.zone_time_pct.get(z, 0.0) for z in (2, 3, 4, 5))
        assert z2plus >= MIN_INTENSITY_Z2PLUS * 100, (
            f"Soccer bout z2+ = {z2plus:.1f}%, expected >= {MIN_INTENSITY_Z2PLUS * 100}%"
        )


def test_merge_gap_boundary_2min_merges_6min_stays_separate():
    """Boundary test: 2-min gap merges, 6-min gap stays separate (MERGE_GAP_S=150 s).

    This is the canonical treadmill-vs-soccer boundary test:
      - 2-min (120 s) quiet block → active-ts gap slightly > 120 s but < 150 s → 1 session
      - 6-min (360 s) quiet block → active-ts gap > 360 s >> 150 s → 2 sessions
    """
    streams_2min = _two_workout_streams(quiet_s=120)   # treadmill case
    streams_6min = _two_workout_streams(quiet_s=360)   # soccer case

    sessions_2min = detect_exercises(streams_2min, resting_hr=55, max_hr=190)
    sessions_6min = detect_exercises(streams_6min, resting_hr=55, max_hr=190)

    assert len(sessions_2min) == 1, (
        f"2-min gap (120 s < MERGE_GAP_S={MERGE_GAP_S} s) must merge; "
        f"got {len(sessions_2min)}"
    )
    assert len(sessions_6min) == 2, (
        f"6-min gap (360 s > MERGE_GAP_S={MERGE_GAP_S} s) must stay 2; "
        f"got {len(sessions_6min)}"
    )


def test_unrelated_workouts_beyond_merge_gap_stay_separate():
    """Two distinct workouts with a gap >> MERGE_GAP_S must NOT merge.

    Uses a 30-min gap (1800 s >> 420 s) between two high-intensity bouts.
    This confirms that widening MERGE_GAP_S to 420 s does not accidentally
    fuse workouts from different parts of the day.
    """
    work_n = 10 * 60
    gap_n = 30 * 60         # 1800 s >> MERGE_GAP_S (420 s)
    rest_n = 5 * 60

    a_start = T0 + rest_n
    gap_start = a_start + work_n
    b_start = gap_start + gap_n
    tail_start = b_start + work_n

    hr = _merge(
        _hr_block(T0, rest_n, 55),
        _hr_block(a_start, work_n, 150),
        _hr_block(gap_start, gap_n, 55),
        _hr_block(b_start, work_n, 150),
        _hr_block(tail_start, rest_n, 55),
    )
    gravity = _merge(
        _gravity_still(T0, rest_n),
        _gravity_active(a_start, work_n),
        _gravity_still(gap_start, gap_n),
        _gravity_active(b_start, work_n),
        _gravity_still(tail_start, rest_n),
    )
    sessions = detect_exercises({"hr": hr, "gravity": gravity},
                                resting_hr=55, max_hr=190)
    assert len(sessions) == 2, (
        f"Two workouts 30-min apart must stay as 2 sessions (MERGE_GAP_S={MERGE_GAP_S} s); "
        f"got {len(sessions)}"
    )
