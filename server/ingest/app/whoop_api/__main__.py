"""
app/whoop_api/__main__.py — CLI for the WHOOP official API client.

Usage:
  python -m app.whoop_api auth   — one-time authorization (get a refresh token)
  python -m app.whoop_api pull   — pull ground-truth days + workouts for a date range

All secrets are read from environment variables (never hard-coded):
  WHOOP_CLIENT_ID
  WHOOP_CLIENT_SECRET
  WHOOP_REDIRECT_URI
  WHOOP_REFRESH_TOKEN   (not required for 'auth'; produced by it)

See app/whoop_api/README.md for full instructions.
"""
from __future__ import annotations

import dataclasses
import json
import os
import sys
import webbrowser
from datetime import datetime, timezone

from .client import WhoopClient, build_auth_url, exchange_code, TOKEN_URL
from .models import GroundTruthDay, GroundTruthWorkout


def _env(name: str) -> str:
    v = os.environ.get(name, "").strip()
    if not v:
        print(f"ERROR: environment variable {name!r} is not set.", file=sys.stderr)
        sys.exit(1)
    return v


def _serialize(obj: object) -> object:
    """JSON-serializable form of a dataclass (dates as ISO strings)."""
    if dataclasses.is_dataclass(obj) and not isinstance(obj, type):
        d = dataclasses.asdict(obj)
        # Convert date keys
        for k, v in d.items():
            if hasattr(v, "isoformat"):
                d[k] = v.isoformat()
        return d
    raise TypeError(f"Not serializable: {type(obj)}")


def cmd_auth() -> None:
    """Interactive first-time authorization: prints refresh token."""
    client_id    = _env("WHOOP_CLIENT_ID")
    redirect_uri = _env("WHOOP_REDIRECT_URI")

    url = build_auth_url(client_id, redirect_uri)
    print("\nOpen this URL in your browser and authorize:")
    print(url)
    print()
    try:
        webbrowser.open(url)
    except Exception:
        pass

    redirected = input("Paste the full redirected URL here: ").strip()
    # Extract the 'code' query param
    from urllib.parse import urlparse, parse_qs
    qs   = parse_qs(urlparse(redirected).query)
    code = (qs.get("code") or [""])[0]
    if not code:
        print("ERROR: could not find 'code' param in the URL.", file=sys.stderr)
        sys.exit(1)

    client_secret = _env("WHOOP_CLIENT_SECRET")
    data = exchange_code(code, client_id, client_secret, redirect_uri)
    print("\nSuccess!", file=sys.stderr)
    print(f"  access_token  : {data.get('access_token', '(missing)')[:20]}...", file=sys.stderr)
    # Print the refresh token to stderr so it does not appear in shell history,
    # pipe captures, or log files that record stdout.
    print(f"\n  refresh_token : {data.get('refresh_token', '(missing)')}", file=sys.stderr)
    print("\n  Copy the refresh_token above and store it as WHOOP_REFRESH_TOKEN"
          " in your .env / secrets store.", file=sys.stderr)


def cmd_pull(args: list[str]) -> None:
    """Pull ground-truth days and workouts for a date range, write JSON."""
    import argparse
    p = argparse.ArgumentParser(prog="python -m app.whoop_api pull")
    p.add_argument("--start", required=True, help="Start date YYYY-MM-DD (inclusive)")
    p.add_argument("--end",   required=True, help="End date YYYY-MM-DD (inclusive)")
    p.add_argument("--out",   default="-",   help="Output JSON file path (default: stdout)")
    opts = p.parse_args(args)

    start = datetime.fromisoformat(opts.start).replace(tzinfo=timezone.utc)
    end   = datetime.fromisoformat(opts.end).replace(hour=23, minute=59, second=59,
                                                      tzinfo=timezone.utc)

    # Load the stored refresh token; persist any rotation back to env display
    rt_path = os.environ.get("WHOOP_REFRESH_TOKEN_FILE")
    rt_env  = os.environ.get("WHOOP_REFRESH_TOKEN", "")
    if rt_path:
        with open(rt_path) as _f:
            refresh_token = _f.read().strip()
    else:
        refresh_token = rt_env
    if not refresh_token:
        print("ERROR: WHOOP_REFRESH_TOKEN (or WHOOP_REFRESH_TOKEN_FILE) not set.",
              file=sys.stderr)
        sys.exit(1)

    rotated: list[str] = []

    def on_refresh(new_rt: str) -> None:
        rotated.append(new_rt)
        if rt_path:
            with open(rt_path, "w") as _f:
                _f.write(new_rt)
        else:
            print(f"\n[WARN] Refresh token rotated. New token:\n  {new_rt}\n"
                  "  Update WHOOP_REFRESH_TOKEN in your secrets store.", file=sys.stderr)

    client = WhoopClient(
        client_id=_env("WHOOP_CLIENT_ID"),
        client_secret=_env("WHOOP_CLIENT_SECRET"),
        refresh_token=refresh_token,
        on_token_refresh=on_refresh,
    )

    print(f"Pulling ground truth for {opts.start} → {opts.end} ...", file=sys.stderr)
    days     = client.ground_truth_days(start, end)
    workouts = client.ground_truth_workouts(start, end)
    print(f"  {len(days)} days, {len(workouts)} workouts.", file=sys.stderr)

    output = {
        "days":     [_serialize(v) for v in sorted(days.values(), key=lambda d: d.day)],
        "workouts": [_serialize(w) for w in workouts],
    }
    payload = json.dumps(output, indent=2, default=str)

    if opts.out == "-":
        print(payload)
    else:
        with open(opts.out, "w") as _f:
            _f.write(payload)
        print(f"Written to {opts.out}", file=sys.stderr)


def main() -> None:
    if len(sys.argv) < 2 or sys.argv[1] in {"-h", "--help"}:
        print(__doc__)
        sys.exit(0)

    sub = sys.argv[1]
    rest = sys.argv[2:]

    if sub == "auth":
        cmd_auth()
    elif sub == "pull":
        cmd_pull(rest)
    else:
        print(f"Unknown command {sub!r}. Use 'auth' or 'pull'.", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
