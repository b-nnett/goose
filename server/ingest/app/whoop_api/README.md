# whoop_api — WHOOP Official API Client

Read-only client for the [WHOOP Developer API v2](https://developer.whoop.com/api/)
that produces `GroundTruthDay` records for validation against our BLE pipeline.
Also parses WHOOP CSV data exports as a no-registration fallback.

**Not on the server hot path.** Run this offline/manually, then feed its output
to the Task-10 validation harness.

---

## Quick-start

### Option A: API (recommended — live, precise, date-filterable)

#### Step 1 — Register a WHOOP app (one time)

1. Go to <https://developer-dashboard.whoop.com/apps/create> (create a Team if prompted).
2. Create an app with these settings:
   - **Scopes:** `read:recovery`, `read:sleep`, `read:cycles`, `read:workout`,
     `read:body_measurement`, `read:profile`, **`offline`**
   - **Redirect URI:** register the exact string you will use in step 3, e.g.
     `https://localhost/callback` (or any URI you can capture the redirect from).
3. Copy your **Client ID** and **Client Secret**.

#### Step 2 — Authorize once (get a refresh token)

Run the helper script:

```sh
cd stacks/whoop/ingest
WHOOP_CLIENT_ID=<id> \
WHOOP_CLIENT_SECRET=<secret> \
WHOOP_REDIRECT_URI=<uri> \
  ~/Developer/home-server/venv/bin/python -m app.whoop_api auth
```

The script prints an authorization URL.  Open it in your browser, log in to WHOOP,
click **Allow**, then paste the full redirected URL back into the terminal.  It
will print your **refresh token** — store it somewhere safe (`.env`, 1Password, etc).
**Never commit it.**

#### Step 3 — Pull ground truth

```sh
WHOOP_CLIENT_ID=<id> \
WHOOP_CLIENT_SECRET=<secret> \
WHOOP_REDIRECT_URI=<uri> \
WHOOP_REFRESH_TOKEN=<refresh_token> \
  ~/Developer/home-server/venv/bin/python -m app.whoop_api pull \
    --start 2026-05-01 --end 2026-05-26 \
    --out /tmp/whoop_ground_truth.json
```

The JSON output is a list of serialized `GroundTruthDay` dicts, one per cycle/date.

#### Using the client in Python

```python
import os
from datetime import datetime, timezone
from app.whoop_api import WhoopClient

# Load the rotated refresh token from disk / env on each run
def load_refresh_token() -> str:
    return open(".whoop_refresh_token").read().strip()

def save_refresh_token(new_rt: str) -> None:
    open(".whoop_refresh_token", "w").write(new_rt)

client = WhoopClient(
    client_id=os.environ["WHOOP_CLIENT_ID"],
    client_secret=os.environ["WHOOP_CLIENT_SECRET"],
    refresh_token=load_refresh_token(),
    on_token_refresh=save_refresh_token,   # persists the rotated token automatically
)

days = client.ground_truth_days(
    start=datetime(2026, 5, 1, tzinfo=timezone.utc),
    end=datetime(2026, 5, 26, tzinfo=timezone.utc),
)
for day_date, gt in sorted(days.items()):
    print(day_date, "recovery:", gt.recovery_score, "hrv:", gt.hrv_rmssd_milli, "ms")

workouts = client.ground_truth_workouts(
    start=datetime(2026, 5, 1, tzinfo=timezone.utc),
    end=datetime(2026, 5, 26, tzinfo=timezone.utc),
)
```

---

### Option B: CSV export (no app registration required)

1. WHOOP app → **More → App Settings → Data Export → request export**.
2. Wait for the email (usually <1 hr). Download the zip.
3. Parse it:

```python
from app.whoop_api import parse_export_bundle

days, workouts = parse_export_bundle("/path/to/whoop_export.zip")
for day_date, gt in sorted(days.items()):
    print(day_date, "strain:", gt.day_strain, "sleep:", gt.total_sleep_min, "min")
```

Or parse individual files:

```python
from app.whoop_api import parse_cycles_csv, parse_workouts_csv

days     = parse_cycles_csv("/path/to/physiological_cycles.csv")
workouts = parse_workouts_csv("/path/to/workouts.csv")
```

---

## What the user must provide (for Option A)

| Secret | Where to get it |
|---|---|
| `WHOOP_CLIENT_ID` | WHOOP Developer Dashboard → your app |
| `WHOOP_CLIENT_SECRET` | same app (server-side only — never log/commit) |
| `WHOOP_REDIRECT_URI` | the URI you registered on the app |
| `WHOOP_REFRESH_TOKEN` | produced by the one-time `auth` step above |

Store all four in environment variables or a `.env` file (gitignored).
The client automatically rotates the refresh token on every use — call
`on_token_refresh` to persist the new token back to your secrets store.

---

## Data model

```
GroundTruthDay (keyed by local calendar date)
├── Cycle      day_strain, kilojoule, avg_hr, max_hr
├── Recovery   recovery_score, resting_hr, hrv_rmssd_milli, spo2_percentage, skin_temp_celsius
└── Sleep      sleep_id, sleep_start/end, in_bed/awake/light/sws_deep/rem_milli,
               respiratory_rate, sleep_performance_pct, sleep_efficiency_pct

GroundTruthWorkout (list, one per workout)
    workout_id, start, end, sport_name, strain, avg_hr, max_hr, kilojoule, distance_meter
```

**Day alignment:** A WHOOP "cycle" is sleep-onset to next sleep-onset, not midnight-to-midnight.
The calendar `day` is derived from `cycle.start` shifted by `cycle.timezone_offset`.
Recovery and sleep join to their cycle via `cycle_id`.

**Units:** stage durations are stored in **milliseconds** (API native).
Use the `total_sleep_min`, `deep_sleep_min`, `rem_sleep_min`, etc. properties for minutes.
HRV is in **ms**. Energy in **kJ**. Temperature in **°C**.
CSV exports (minutes/cal) are automatically converted to these units by the parser.

---

## Running tests

```sh
cd stacks/whoop/ingest
~/Developer/home-server/venv/bin/python -m pytest tests/test_whoop_api.py -q
```

All 82 tests run fully offline (no real network calls — httpx.MockTransport).
