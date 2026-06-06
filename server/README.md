# Server — self-hosted datastore + ingest

Optional FastAPI + TimescaleDB server that receives decoded WHOOP biometric streams from the
iOS app, archives raw frames, stores decoded rows in hypertables, and runs daily analysis.
Supports both **WHOOP 5.0** and WHOOP 4.0 streams via the `device_generation` field.

## Architecture

```
iOS app (CoreBluetooth)
    │  POST /v1/ingest-decoded  (Bearer token)
    ▼
whoop-ingest  (FastAPI, port 8770)
    │  writes decoded rows
    ▼
whoop-db  (TimescaleDB)
    └── hr_samples, rr_samples, events, battery_samples,
        spo2_samples, skin_temp_samples, resp_samples, gravity_samples
        (all with device_generation column — '5.0' or '4.0')
```

Decoding is handled by the iOS app before upload. The server receives already-decoded streams
and stores them directly — no frame parsing on the server side.

## Deploy

```bash
cp .env.example .env
# Set: WHOOP_API_KEY, WHOOP_DB_PASSWORD
# Optional: DATA_ROOT (default: /srv/whoop-data)

export DATA_ROOT=/srv/whoop-data
docker compose up -d --build
```

This starts:
- `whoop-db` — TimescaleDB, data at `${DATA_ROOT}/whoop/db`
- `whoop-ingest` — FastAPI at port 8770

Check it started: `curl -s localhost:8770/healthz` → `{"status":"ok"}`

## API

### Write endpoints (Bearer-authed)

```
POST /v1/ingest-decoded
```

Accepts decoded biometric streams. The `device_generation` field distinguishes 5.0 from 4.0:

```json
{
  "device_id": "my-whoop-5",
  "device_generation": "5.0",
  "captured_at": "2026-05-31T10:00:00Z",
  "hr": [{"ts": 1748686800, "bpm": 75}],
  "rr": [{"ts": 1748686800, "ms": 800}],
  "events": [{"ts": 1748686800, "event_id": 7, "raw": "..."}],
  "battery": [{"ts": 1748686800, "pct": 82}],
  "spo2": [],
  "skin_temp": [],
  "resp": [],
  "gravity": []
}
```

After each ingest, `compute_day()` runs automatically to update daily metrics for the affected
device and day.

### Read endpoints (unauthenticated, LAN/tunnel only)

```
GET /healthz
GET /v1/devices
GET /v1/daily-metrics?device=<id>&from=<date>&to=<date>
GET /v1/sleep-sessions?device=<id>&from=<date>&to=<date>
GET /v1/workouts?device=<id>&from=<date>&to=<date>
GET /v1/streams/{hr|rr|events|battery|spo2|skin_temp|resp|gravity}?device=<id>&from=<unix>&to=<unix>&limit=5000
GET /v1/batches?device=<id>&limit=100
GET /v1/batches/{batch_id}/frames
```

### Legacy endpoint (4.0 raw frame ingest)

```
POST /v1/ingest
```

Accepts raw BLE frames (hex-encoded) for WHOOP 4.0. Decoded server-side using the 4.0 Python
package. Still functional but new deployments should prefer `POST /v1/ingest-decoded`.

## Database schema

All hypertables have a `device_generation TEXT DEFAULT '5.0'` column, added idempotently via
`db/init.sql`. This allows querying by generation:

```sql
SELECT * FROM hr_samples WHERE device_generation = '5.0' AND ts > NOW() - INTERVAL '7 days';
```

## Tests

```bash
pip install -r ingest/requirements-dev.txt
cd ingest && pytest
```

Integration tests spin a throwaway TimescaleDB container and skip if Docker is absent.

## Verify end-to-end

See [`VERIFY.md`](VERIFY.md) for the full E2E test: start the server, POST a test payload, and
confirm rows appear in TimescaleDB.

```bash
# Quick smoke test
curl -s localhost:8770/healthz
curl -s -X POST localhost:8770/v1/ingest-decoded \
  -H "Authorization: Bearer $WHOOP_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"device_id":"test","device_generation":"5.0","captured_at":"2026-05-31T00:00:00Z","hr":[{"ts":1748649600,"bpm":60}],"rr":[],"events":[],"battery":[],"spo2":[],"skin_temp":[],"resp":[],"gravity":[]}'
```

## Dashboard

`whoop-ingest` serves a static datastore dashboard at `/` (e.g. `http://<host>:8770`):
device + time-range picker, HR/battery charts, events list, batch browser, and a hex inspector
that re-parses any archived 4.0 frame (category-coloured byte grid + field readout).
