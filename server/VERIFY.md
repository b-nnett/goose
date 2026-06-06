# Whoop datastore — end-to-end verification

Reproducible proof that the write path works: raw frames replayed from a capture are
archived and decoded into TimescaleDB. Run on a machine with Docker + the repo checked
out (decode tests need `~/Developer/whoop/*.jsonl` present).

## Run

```bash
cd stacks/whoop
cat > .env <<'EOF'
WHOOP_API_KEY=localtest
WHOOP_DB_NAME=whoop
WHOOP_DB_USER=whoop
WHOOP_DB_PASSWORD=localtest
WHOOP_INGEST_PORT=8770
EOF
DATA_ROOT=/tmp/whoop-e2e TZ=UTC docker compose up -d --build
sleep 8 && curl -s localhost:8770/healthz            # -> {"status":"ok"}

# replay the real capture (full frames with HR/events/battery)
python client/uploader.py ~/Developer/whoop/capture.jsonl \
  --url http://localhost:8770 --token localtest --device whoop4-dev

# confirm decoded rows + raw archive
docker exec whoop-db psql -U whoop -d whoop -c \
  "SELECT (SELECT count(*) FROM hr_samples) hr, (SELECT count(*) FROM events) events,
          (SELECT count(*) FROM battery) battery, (SELECT count(*) FROM raw_batches) batches;"
find /tmp/whoop-e2e/whoop/raw -name '*.zst'

# teardown
DATA_ROOT=/tmp/whoop-e2e docker compose down && rm -rf /tmp/whoop-e2e
```

## Observed result (2026-05-23, replaying `capture.jsonl`, 483 frames)

- `healthz` → `{"status":"ok"}`.
- Uploader decoded totals: `hr=7, rr=0, events=5, battery=1` across 5 batches.
- DB counts: `hr=7  events=5  battery=1  batches=5  devices=1`.
- HR rows: bpm 59–61, monotonically timestamped (sample: `60, 59, 60, 61, 61`).
- Events at their real RTC time: `2025-01-08 19:46:33+00` (RAW_DATA_COLLECTION_ON/OFF,
  BLE_CONNECTION_DOWN, BATTERY_LEVEL, EXTENDED_BATTERY_INFORMATION).
- Raw archive: 5 `*.zst` files under `whoop4-dev/2026-05-23/`.
- **Idempotency:** re-running the uploader (fresh batch_ids) left `hr=7 events=5 battery=1`
  unchanged — decoded rows dedupe on (device_id, ts[, kind]).

## Known replay limitations (not bugs)

- **Two clocks.** `REALTIME_DATA` uses the device's monotonic epoch (needs a device→wall
  correlation); `EVENT` uses real RTC unix seconds (stored as-is). `capture.jsonl` has no
  wall-clock field, so the uploader anchors the device epoch to the file mtime — HR rows
  therefore land at "replay time" (2026-05-23), while events show their true recorded time
  (2025-01-08). The live phone/Mac client captures a real correlation at connect time, so
  this anchoring gap only affects replay of headerless old captures.
- Re-running the uploader re-archives raw under new batch files (decoded rows still dedupe).
  Content-addressed raw dedupe is a future optimization, out of scope here.
