"""
models.py — Ground-truth data shapes for WHOOP official-API validation.

A GroundTruthDay is keyed by local calendar date and holds every metric we
validate against.  A GroundTruthWorkout holds per-workout details.

## Day/cycle alignment
WHOOP's "cycle" is a physiological day bounded by consecutive sleep onsets,
NOT a midnight-to-midnight calendar day.  We derive the calendar date from the
cycle's *start* timestamp shifted by its timezone_offset:

    day = (parse(cycle["start"]) + parse_offset(cycle["timezone_offset"])).date()

Recovery, sleep, and cycle strain all join on cycle_id.  The "primary" sleep
for a cycle is the non-nap sleep associated with that cycle (nap == false).

## Unit conventions (match the WHOOP v2 API — see docs/research/05-whoop-api.md)
- Sleep stage durations are stored as MILLISECONDS (the API native unit).
  Helper properties convert to minutes for convenience.
- HRV is stored in milliseconds (hrv_rmssd_milli → ms = direct from API).
- Skin temperature is degrees Celsius.
- SpO2 is a percentage (0-100).
- Strain uses the WHOOP 0-21 logarithmic scale.
- Energy is kilojoules (kJ).  CSV export uses calories; parser converts.
- Heart rate is bpm (int).

## CSV export vs API
The CSV data-export (§5 of the research doc) reports stage durations in
MINUTES and energy in CALORIES.  export_parser.py converts to the units above
so both sources produce identical GroundTruthDay/GroundTruthWorkout objects.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import date


@dataclass
class GroundTruthDay:
    """One physiological day (cycle) with its recovery and primary sleep.

    Fields are None when the WHOOP score_state is not SCORED, or when the
    CSV export did not contain that column.
    """

    # Identity
    day: date           # local calendar date derived from cycle start + tz offset
    cycle_id: int | None  # WHOOP numeric cycle id (None when derived from CSV)

    # Cycle / day strain
    day_strain: float | None = None      # 0-21 WHOOP strain
    kilojoule: float | None = None       # energy (kJ)
    avg_hr: int | None = None            # average HR for the cycle (bpm)
    max_hr: int | None = None            # max HR for the cycle (bpm)

    # Recovery
    recovery_score: int | None = None    # 0-100 %
    resting_hr: int | None = None        # bpm
    hrv_rmssd_milli: float | None = None # RMSSD (ms)
    spo2_percentage: float | None = None # %
    skin_temp_celsius: float | None = None
    user_calibrating: bool = False       # True during the first ~4 days; scores are not stable

    # Sleep — primary non-nap sleep for this cycle
    sleep_id: str | None = None          # UUID (v2 API) or None (CSV)
    sleep_start: str | None = None       # ISO-8601 datetime string
    sleep_end: str | None = None         # ISO-8601 datetime string
    in_bed_milli: int | None = None
    awake_milli: int | None = None
    no_data_milli: int | None = None     # time with no signal (subtracted from TST)
    light_milli: int | None = None       # light NREM
    sws_deep_milli: int | None = None    # slow-wave / deep
    rem_milli: int | None = None
    sleep_cycle_count: int | None = None
    disturbance_count: int | None = None
    respiratory_rate: float | None = None        # breaths/min
    sleep_performance_pct: float | None = None   # % asleep vs sleep need
    sleep_efficiency_pct: float | None = None    # % asleep vs in-bed time
    sleep_consistency_pct: float | None = None   # % schedule consistency

    # Convenience properties (minutes)
    @property
    def total_sleep_min(self) -> float | None:
        """Total sleep time in minutes: in_bed minus awake minus no_data.

        no_data_milli is subtracted when present (defaults to 0 when absent),
        matching how WHOOP calculates TST on-device.
        """
        if self.in_bed_milli is None or self.awake_milli is None:
            return None
        no_data = self.no_data_milli or 0
        return (self.in_bed_milli - self.awake_milli - no_data) / 60_000

    @property
    def deep_sleep_min(self) -> float | None:
        return None if self.sws_deep_milli is None else self.sws_deep_milli / 60_000

    @property
    def rem_sleep_min(self) -> float | None:
        return None if self.rem_milli is None else self.rem_milli / 60_000

    @property
    def light_sleep_min(self) -> float | None:
        return None if self.light_milli is None else self.light_milli / 60_000

    @property
    def awake_min(self) -> float | None:
        return None if self.awake_milli is None else self.awake_milli / 60_000


@dataclass
class GroundTruthWorkout:
    """One workout activity from the WHOOP API or CSV export."""

    workout_id: str          # UUID (v2 API) or "csv-<row_idx>" for CSV
    start: str               # ISO-8601
    end: str                 # ISO-8601
    sport_name: str | None = None
    sport_id: int | None = None
    strain: float | None = None
    avg_hr: int | None = None            # bpm
    max_hr: int | None = None            # bpm
    kilojoule: float | None = None       # kJ
    distance_meter: float | None = None
