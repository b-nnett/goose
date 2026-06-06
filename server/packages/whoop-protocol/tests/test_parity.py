import os
import sys
import pytest

from whoop_protocol.interpreter import parse_frame
from tests.fixtures import frames

_LEGACY_DIR = os.path.expanduser("~/Developer/whoop/dashboard")
legacy = None
if os.path.isdir(_LEGACY_DIR):
    sys.path.insert(0, _LEGACY_DIR)
    sys.path.insert(0, os.path.expanduser("~/Developer/whoop/whoomp/scripts"))
    try:
        # The frozen original decoder. (whoop_fields.py itself is now a shim delegating
        # to whoop_protocol, so comparing against it would be a tautology — use the
        # preserved legacy copy to keep this a real cross-implementation cross-check.)
        import whoop_fields_legacy as legacy  # type: ignore
    except Exception:
        try:
            import whoop_fields as legacy  # pre-shim fallback  # type: ignore
        except Exception:
            legacy = None

pytestmark = pytest.mark.skipif(legacy is None, reason="legacy whoop_fields.py not available")


@pytest.mark.parametrize("frame", [
    frames.REALTIME_DATA_HR60, frames.EVENT_RAW_ON, frames.CMD_RESP_BATTERY,
])
def test_parsed_matches_legacy(frame):
    new = parse_frame(frame)["parsed"]
    old = legacy.parse_frame(frame)["parsed"]
    # compare the keys both implementations populate (new may add a couple)
    for k in old:
        assert k in new, f"missing key {k}"
        assert new[k] == old[k], f"mismatch on {k}: {new[k]} != {old[k]}"
