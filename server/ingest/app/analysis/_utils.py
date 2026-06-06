"""
_utils.py — small shared helpers for the analysis modules.

Houses the single ``to_epoch`` timestamp-coercion helper used by sleep.py,
recovery.py, and activity.py (previously copy-pasted identically in each).
"""
from __future__ import annotations

import datetime as _dt
from typing import Any


def to_epoch(ts: Any) -> float:
    """Coerce a ts (float/int epoch seconds, or datetime) to epoch seconds.

    Naive datetimes are assumed UTC.
    """
    if isinstance(ts, _dt.datetime):
        if ts.tzinfo is None:
            ts = ts.replace(tzinfo=_dt.timezone.utc)
        return ts.timestamp()
    return float(ts)
