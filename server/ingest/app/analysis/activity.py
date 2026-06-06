"""
activity.py — Per-record motion-intensity series from the gravity vector(s).

We surface a simple, well-established actigraphy proxy for body movement: the
Euclidean (L2) magnitude of the change in the gravity/acceleration vector
between two consecutive 1 Hz samples. When the wrist is still the gravity vector
is nearly constant, so consecutive samples differ by ~0; when the wrist moves the
orientation of gravity (and any linear acceleration leaking into the channel)
shifts, producing a larger inter-sample difference. Summing/averaging this proxy
over time is the basis of count-based actigraphy.

This is an APPROXIMATE movement proxy, not a calibrated accelerometer activity
count and not medical advice.

Method / references
-------------------
The per-sample difference-magnitude of a 3-axis accelerometer is a standard
movement feature used to derive actigraphy "activity counts":

  - te Lindert, B.H.W. & Van Someren, E.J.W. (2013). "Sleep estimates using
    microelectromechanical systems (MEMS)." *Sleep*, 36(5), 781–789. — derives
    activity counts from the magnitude of successive acceleration differences.
  - Standard vector-magnitude / signal-magnitude formulation
    ``|Δa| = sqrt(Δx² + Δy² + Δz²)`` between consecutive samples.

------------------------------------------------------------------------------
Two-triplet handling
------------------------------------------------------------------------------
The V24 history stream may carry a SECOND gravity/accel triplet. When a row has
``x2``/``y2``/``z2`` we compute the difference-magnitude of EACH triplet against
the previous row and average the two (a simple sensor-fusion smoothing); when the
2nd triplet is absent we use the single triplet alone.

NOTE: gravity2 (``x2``/``y2``/``z2``) is NOT yet persisted server-side, so in
practice ``activity_series`` runs on the single triplet today; the two-triplet
path is forward-compatible and exercised by tests.

------------------------------------------------------------------------------
Input / output shapes
------------------------------------------------------------------------------
``gravity_rows: list[dict]`` —
    ``{"ts": <unix seconds>, "x": float, "y": float, "z": float}`` and
    OPTIONALLY ``{"x2": float, "y2": float, "z2": float}``.

Returns ``[{"ts": <unix seconds>, "intensity": float}, ...]`` — one point per
input row, time-ordered. The FIRST row's intensity is 0.0 (no predecessor).

A dropout (any missing/non-numeric coordinate in a triplet) is deliberately
treated as an INFINITE difference (``math.inf``): a lost sample is far more
likely to coincide with motion than with perfect stillness, so we never let a
dropout masquerade as "no movement". When both triplets are present and one is a
dropout, averaging a finite value with ``inf`` is ``inf``.
"""
from __future__ import annotations

import math
import statistics
from typing import Any, Sequence

from ._utils import to_epoch as _to_epoch

# A missing-coordinate triplet reads as a large (infinite) movement so a sensor
# dropout is never mistaken for stillness. See module docstring.
_DROPOUT_INTENSITY = math.inf


def _coords(row: dict, keys: tuple[str, str, str]) -> tuple[float, float, float] | None:
    """Read a numeric (x, y, z) triplet from ``row`` under ``keys``.

    Returns ``None`` if any of the three components is absent or non-numeric.
    """
    out: list[float] = []
    for k in keys:
        try:
            v = row[k]
        except KeyError:
            return None
        if v is None:
            return None
        try:
            out.append(float(v))
        except (TypeError, ValueError):
            return None
    return out[0], out[1], out[2]


def _vector_change(
    a: tuple[float, float, float] | None,
    b: tuple[float, float, float] | None,
) -> float:
    """Euclidean magnitude of (a − b). ``inf`` if either sample is a dropout."""
    if a is None or b is None:
        return _DROPOUT_INTENSITY
    return math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2])


def activity_series(gravity_rows: Sequence[dict[str, Any]]) -> list[dict[str, float]]:
    """Per-record motion-intensity series (inter-sample gravity change magnitude).

    For each row, intensity is the L2 magnitude of the change in the gravity
    vector vs the previous row. When the second triplet (``x2``/``y2``/``z2``) is
    present, the two per-triplet magnitudes are averaged. The first row has no
    predecessor and is assigned intensity 0.0. Empty input → ``[]``.

    Args:
        gravity_rows: time-ordered gravity rows (see module docstring).

    Returns:
        ``[{"ts": <unix seconds>, "intensity": float}, ...]``.
    """
    if not gravity_rows:
        return []

    # Copy, normalize timestamps, and sort by time (input order is not trusted).
    rows = [dict(r) for r in gravity_rows]
    for r in rows:
        r["ts"] = _to_epoch(r["ts"])
    rows.sort(key=lambda r: r["ts"])

    series: list[dict[str, float]] = []
    prev_primary: tuple[float, float, float] | None = None
    prev_secondary: tuple[float, float, float] | None = None

    for position, row in enumerate(rows):
        primary = _coords(row, ("x", "y", "z"))
        has_secondary = any(k in row for k in ("x2", "y2", "z2"))
        secondary = _coords(row, ("x2", "y2", "z2")) if has_secondary else None

        if position == 0:
            intensity = 0.0
        elif has_secondary:
            mags = (
                _vector_change(primary, prev_primary),
                _vector_change(secondary, prev_secondary),
            )
            intensity = statistics.fmean(mags)
        else:
            intensity = _vector_change(primary, prev_primary)

        series.append({"ts": row["ts"], "intensity": intensity})
        prev_primary, prev_secondary = primary, secondary

    return series


def summarize_activity(series: Sequence[dict[str, float]]) -> dict[str, float]:
    """Summary stats over an ``activity_series`` output (finite points only).

    Returns ``{"mean": float, "peak": float, "n": int}``. Non-finite intensities
    (sensor dropouts → inf) are excluded from mean/peak; ``n`` counts the finite
    points. Empty / all-infinite input → ``{"mean": 0.0, "peak": 0.0, "n": 0}``.
    """
    finite = [p["intensity"] for p in series if math.isfinite(p["intensity"])]
    if not finite:
        return {"mean": 0.0, "peak": 0.0, "n": 0}
    return {"mean": statistics.fmean(finite), "peak": max(finite), "n": len(finite)}
