"""Thin shim: delegate WHOOP frame decode to the shared `whoop-protocol` package.

This used to be the hand-written decoder; it is now the single live consumer of the
productionized package at `~/Developer/home-server/packages/whoop-protocol` (installed
editable into this venv). One decoder, one source of truth — improve decode by editing
`whoop_protocol/schema/whoop_protocol.json`, and both this live dashboard and the
home-server ingest pick it up.

The original imperative implementation is preserved alongside as `whoop_fields_legacy.py`.
`dashboard/server.py` imports `parse_frame` + `CATEGORIES` from here, so both are kept.
"""
from whoop_protocol.interpreter import parse_frame  # noqa: F401  (re-exported for server.py)

# Category legend for the dashboard's color UI. Mirrors the `cat` values the interpreter
# emits (see the schema field definitions).
CATEGORIES = ["frame", "cmd", "time", "hr", "rr", "accel", "gyro", "ppg",
              "battery", "event", "meta", "text", "unknown"]
