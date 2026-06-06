"""
export_parser.py — Parse a WHOOP data-export CSV bundle into GroundTruthDay records.

WHOOP data export (WHOOP app → More → App Settings → Data Export) ships a zip
containing physiological_cycles.csv, sleeps.csv, and workouts.csv.  Header
strings are verbose and unit-suffixed, and vary across app versions.

Strategy
--------
We normalize column headers (lower-case, strip, collapse spaces/parens/% to
underscores) before matching, so the parser is robust to minor header drift.
Units differ from the v2 API:
  - Stage durations: CSV exports MINUTES → we store MILLISECONDS (×60 000)
  - Energy:          CSV exports CALORIES → we store KILOJOULES (÷ 4.184... wait:
                     WHOOP CSV uses "cal" = kcal, so 1 cal (CSV) = 4.184 kJ)
  - All other numeric fields are the same scale as the API.

Missing columns produce None for those fields, not an error.

See docs/research/05-whoop-api.md §5 for the full column-name reference.
"""

from __future__ import annotations

import csv
import os
import re
import zipfile
from datetime import date, datetime, timezone
from pathlib import Path
from typing import Any

from .models import GroundTruthDay, GroundTruthWorkout


# ---------------------------------------------------------------------------
# Header normalization
# ---------------------------------------------------------------------------

def _norm(header: str) -> str:
    """Normalize a CSV header to a stable key for column-by-name lookup.

    e.g.  "Heart rate variability (ms)" -> "heart_rate_variability_ms"
          "Recovery score %"            -> "recovery_score_pct"
          "In bed duration (min)"       -> "in_bed_duration_min"
    """
    s = header.lower().strip()
    # Replace % with pct
    s = s.replace("%", "pct")
    # Drop parens but keep inner content
    s = re.sub(r"[()]", " ", s)
    # Collapse runs of non-alphanumeric chars to underscore
    s = re.sub(r"[^a-z0-9]+", "_", s)
    return s.strip("_")


def _norm_headers(row: dict) -> dict[str, str]:
    """Return {normalized_key: original_key} for every header in the CSV row."""
    return {_norm(k): k for k in row.keys()}


def _get(row: dict, norm_map: dict[str, str], *keys: str) -> str | None:
    """Try each normalized key in order; return the raw cell string or None."""
    for k in keys:
        orig = norm_map.get(k)
        if orig is not None:
            val = row.get(orig, "").strip()
            return val if val else None
    return None


def _float(row: dict, norm_map: dict[str, str], *keys: str) -> float | None:
    v = _get(row, norm_map, *keys)
    if v is None:
        return None
    try:
        return float(v)
    except (ValueError, TypeError):
        return None


def _int(row: dict, norm_map: dict[str, str], *keys: str) -> int | None:
    v = _float(row, norm_map, *keys)
    return None if v is None else int(round(v))


def _min_to_milli(row: dict, norm_map: dict[str, str], *keys: str) -> int | None:
    """Read a minutes-valued cell and convert to milliseconds."""
    v = _float(row, norm_map, *keys)
    return None if v is None else int(round(v * 60_000))


def _parse_date_from_ts(ts: str | None) -> date | None:
    """Parse 'YYYY-MM-DD HH:MM:SS' (WHOOP CSV format) and return the local date."""
    if not ts:
        return None
    try:
        return datetime.fromisoformat(ts).date()
    except ValueError:
        return None


# ---------------------------------------------------------------------------
# physiological_cycles.csv → GroundTruthDay
# ---------------------------------------------------------------------------

# Normalized key candidates for each logical field (listed longest-first so the
# most specific match wins when headers are ambiguous).
_CYCLES_FIELDS: dict[str, tuple[str, ...]] = {
    "day":            ("cycle_start_time",),
    "recovery_score": ("recovery_score_pct",),
    "resting_hr":     ("resting_heart_rate_bpm",),
    "hrv":            ("heart_rate_variability_ms",),
    "skin_temp":      ("skin_temp_celsius",),
    "spo2":           ("blood_oxygen_pct",),
    "day_strain":     ("day_strain",),
    "energy_cal":     ("energy_burned_cal",),
    "max_hr":         ("max_hr_bpm",),
    "avg_hr":         ("average_hr_bpm",),
    "sleep_onset":    ("sleep_onset",),
    "wake_onset":     ("wake_onset",),
    "sleep_perf":     ("sleep_performance_pct",),
    "resp":           ("respiratory_rate_rpm",),
    "asleep_min":     ("asleep_duration_min",),
    "in_bed_min":     ("in_bed_duration_min",),
    "light_min":      ("light_sleep_duration_min",),
    "deep_min":       ("deep_sws_duration_min",),
    "rem_min":        ("rem_duration_min",),
    "awake_min":      ("awake_duration_min",),
    "sleep_eff":      ("sleep_efficiency_pct",),
    "sleep_cons":     ("sleep_consistency_pct",),
}


def parse_cycles_csv(path: str | Path) -> dict[date, GroundTruthDay]:
    """Parse physiological_cycles.csv into GroundTruthDay records keyed by date."""
    days: dict[date, GroundTruthDay] = {}
    with open(path, newline="", encoding="utf-8-sig") as fh:
        reader = csv.DictReader(fh)
        for row in reader:
            norm_map = _norm_headers(row)

            ts = _get(row, norm_map, "cycle_start_time")
            day = _parse_date_from_ts(ts)
            if day is None:
                continue

            # Energy: CSV is in cal (= kcal); convert to kJ: 1 kcal = 4.184 kJ
            energy_cal = _float(row, norm_map, "energy_burned_cal")
            kj = energy_cal * 4.184 if energy_cal is not None else None

            # Stage durations: CSV is in minutes → milliseconds
            in_bed_milli  = _min_to_milli(row, norm_map, "in_bed_duration_min")
            awake_milli   = _min_to_milli(row, norm_map, "awake_duration_min")
            light_milli   = _min_to_milli(row, norm_map, "light_sleep_duration_min")
            sws_milli     = _min_to_milli(row, norm_map, "deep_sws_duration_min")
            rem_milli     = _min_to_milli(row, norm_map, "rem_duration_min")

            sleep_onset = _get(row, norm_map, "sleep_onset")
            wake_onset  = _get(row, norm_map, "wake_onset")

            gtd = GroundTruthDay(
                day      = day,
                cycle_id = None,     # CSV doesn't expose the cycle_id integer

                day_strain = _float(row, norm_map, "day_strain"),
                kilojoule  = kj,
                avg_hr     = _int(row, norm_map, "average_hr_bpm"),
                max_hr     = _int(row, norm_map, "max_hr_bpm"),

                recovery_score    = _int(row, norm_map, "recovery_score_pct"),
                resting_hr        = _int(row, norm_map, "resting_heart_rate_bpm"),
                hrv_rmssd_milli   = _float(row, norm_map, "heart_rate_variability_ms"),
                spo2_percentage   = _float(row, norm_map, "blood_oxygen_pct"),
                skin_temp_celsius = _float(row, norm_map, "skin_temp_celsius"),

                sleep_id    = None,
                sleep_start = sleep_onset,
                sleep_end   = wake_onset,
                in_bed_milli       = in_bed_milli,
                awake_milli        = awake_milli,
                light_milli        = light_milli,
                sws_deep_milli     = sws_milli,
                rem_milli          = rem_milli,
                respiratory_rate      = _float(row, norm_map, "respiratory_rate_rpm"),
                sleep_performance_pct = _float(row, norm_map, "sleep_performance_pct"),
                sleep_efficiency_pct  = _float(row, norm_map, "sleep_efficiency_pct"),
                sleep_consistency_pct = _float(row, norm_map, "sleep_consistency_pct"),
            )
            days[day] = gtd

    return days


# ---------------------------------------------------------------------------
# sleeps.csv → supplement or standalone
# ---------------------------------------------------------------------------

def parse_sleeps_csv(path: str | Path) -> list[dict]:
    """Parse sleeps.csv into a list of raw normalized dicts (one per sleep row).

    Useful for cross-referencing against parse_cycles_csv output when you need
    nap filtering or per-sleep granularity.  Returns dicts with normalized-key
    names and ms-converted stage durations.
    """
    records = []
    with open(path, newline="", encoding="utf-8-sig") as fh:
        reader = csv.DictReader(fh)
        for row in reader:
            norm_map = _norm_headers(row)

            # "nap" column: truthy strings "true"/"yes"/"1"
            nap_raw = _get(row, norm_map, "nap")
            is_nap  = nap_raw is not None and nap_raw.lower() in {"true", "yes", "1"}

            records.append({
                "is_nap":              is_nap,
                "sleep_start":         _get(row, norm_map, "sleep_onset"),
                "sleep_end":           _get(row, norm_map, "wake_onset"),
                "in_bed_milli":        _min_to_milli(row, norm_map, "in_bed_duration_min"),
                "awake_milli":         _min_to_milli(row, norm_map, "awake_duration_min"),
                "light_milli":         _min_to_milli(row, norm_map, "light_sleep_duration_min"),
                "sws_deep_milli":      _min_to_milli(row, norm_map, "deep_sws_duration_min"),
                "rem_milli":           _min_to_milli(row, norm_map, "rem_duration_min"),
                "respiratory_rate":    _float(row, norm_map, "respiratory_rate_rpm"),
                "sleep_performance_pct": _float(row, norm_map, "sleep_performance_pct"),
                "sleep_efficiency_pct":  _float(row, norm_map, "sleep_efficiency_pct"),
                "sleep_consistency_pct": _float(row, norm_map, "sleep_consistency_pct"),
            })
    return records


# ---------------------------------------------------------------------------
# workouts.csv → GroundTruthWorkout
# ---------------------------------------------------------------------------

def parse_workouts_csv(path: str | Path) -> list[GroundTruthWorkout]:
    """Parse workouts.csv into GroundTruthWorkout records."""
    workouts = []
    with open(path, newline="", encoding="utf-8-sig") as fh:
        reader = csv.DictReader(fh)
        for idx, row in enumerate(reader):
            norm_map = _norm_headers(row)

            # Energy: cal → kJ
            energy_cal = _float(row, norm_map, "energy_burned_cal")
            kj = energy_cal * 4.184 if energy_cal is not None else None

            w_start = _get(row, norm_map, "workout_start_time")
            w_end   = _get(row, norm_map, "workout_end_time")

            workouts.append(GroundTruthWorkout(
                workout_id    = f"csv-{idx}",
                start         = w_start or "",
                end           = w_end   or "",
                sport_name    = _get(row, norm_map, "activity_name"),
                sport_id      = None,   # CSV doesn't have numeric sport_id
                strain        = _float(row, norm_map, "activity_strain"),
                avg_hr        = _int(row, norm_map, "average_hr_bpm"),
                max_hr        = _int(row, norm_map, "max_hr_bpm"),
                kilojoule     = kj,
                distance_meter= _float(row, norm_map, "distance_meters"),
            ))
    return workouts


# ---------------------------------------------------------------------------
# Bundle (zip) convenience entry point
# ---------------------------------------------------------------------------

def parse_export_bundle(
    zip_path: str | Path,
    extract_dir: str | Path | None = None,
) -> tuple[dict[date, GroundTruthDay], list[GroundTruthWorkout]]:
    """Extract a WHOOP data-export zip and parse cycles + workouts.

    Returns (days_by_date, workouts).

    If extract_dir is None, a sibling directory next to the zip is used.
    The caller is responsible for cleanup if a temporary directory is used.
    """
    zip_path = Path(zip_path)
    if extract_dir is None:
        extract_dir = zip_path.parent / zip_path.stem

    extract_dir = Path(extract_dir)
    extract_dir.mkdir(parents=True, exist_ok=True)

    with zipfile.ZipFile(zip_path) as zf:
        zf.extractall(extract_dir)

    # Locate files case-insensitively
    files: dict[str, Path] = {}
    for p in extract_dir.rglob("*.csv"):
        files[p.name.lower()] = p

    cycles_path   = files.get("physiological_cycles.csv")
    workouts_path = files.get("workouts.csv")

    days     = parse_cycles_csv(cycles_path)   if cycles_path   else {}
    workouts = parse_workouts_csv(workouts_path) if workouts_path else []

    return days, workouts
