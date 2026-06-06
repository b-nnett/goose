"""
whoop_api — read-only client for the WHOOP Developer API (v2) and CSV data-export parser.

Produces GroundTruthDay records for use as validation ground truth by the
metrics-accuracy validation harness (Task 10).

NOT imported by the server hot path — standalone offline/manual tool only.

Docs: docs/research/05-whoop-api.md
"""

from .models import GroundTruthDay, GroundTruthWorkout
from .client import WhoopClient
from .export_parser import parse_export_bundle, parse_cycles_csv, parse_sleeps_csv, parse_workouts_csv

__all__ = [
    "GroundTruthDay",
    "GroundTruthWorkout",
    "WhoopClient",
    "parse_export_bundle",
    "parse_cycles_csv",
    "parse_sleeps_csv",
    "parse_workouts_csv",
]
