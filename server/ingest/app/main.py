"""FastAPI ingest service. Bearer-auth write endpoint + health check + read API +
the static datastore dashboard."""
import datetime as _dt
import logging
import os
import secrets
import threading
import time

import psycopg
from fastapi import Depends, FastAPI, Header, HTTPException, Query
from fastapi.responses import FileResponse
from fastapi.staticfiles import StaticFiles
from pydantic import BaseModel, Field

from . import db, ingest, read, store
from .analysis import daily
from .config import load_config

_log = logging.getLogger("goose.ingest")

cfg = load_config()
db.bootstrap_schema(cfg.db_dsn)

# Docs/schema disabled: don't advertise the API surface publicly (every /v1 route is
# Bearer-gated, but the OpenAPI schema + Swagger UI were world-readable).
app = FastAPI(title="Goose Ingest", docs_url=None, redoc_url=None, openapi_url=None)

_STATIC = os.path.join(os.path.dirname(__file__), "static")
app.mount("/static", StaticFiles(directory=_STATIC), name="static")

# --- Auto-recompute throttle -------------------------------------------------
# The phone uploads opportunistically (every ~30s while connected, plus backlog
# drains), so /v1/ingest-decoded can fire many times per minute — each touching
# the SAME current day. compute_day now runs the heavy neurokit sleep-staging
# pipeline, so recomputing a day on every upload saturates CPU/memory. We
# therefore (a) single-flight recomputes (never run two at once) and (b) debounce
# per (device, day) so a day recomputes at most once per cooldown. On-demand
# freshness is always available via POST /v1/compute-daily.
_RECOMPUTE_COOLDOWN_S = 120.0
_recompute_lock = threading.Lock()
_last_recompute: dict[tuple[str, _dt.date], float] = {}


@app.get("/")
def dashboard():
    """Serve the datastore dashboard (static SPA reading the /v1 read API)."""
    return FileResponse(os.path.join(_STATIC, "index.html"))


@app.get("/architecture")
def architecture():
    """Serve the device-link architecture page (how we talk to the strap, no byte detail)."""
    return FileResponse(os.path.join(_STATIC, "architecture.html"))


def require_auth(authorization: str = Header(default="")) -> None:
    expected = f"Bearer {cfg.api_key}"
    if not secrets.compare_digest(authorization, expected):
        raise HTTPException(status_code=401, detail="unauthorized")


class Frame(BaseModel):
    seq: int | None = None
    hex: str


class ClockRef(BaseModel):
    device: int
    wall: int


class Device(BaseModel):
    device_id: str
    mac: str | None = None
    name: str | None = None


class IngestBatch(BaseModel):
    batch_id: str
    device: Device
    clock_ref: ClockRef
    frames: list[Frame]
    decode_streams: bool = True


# ── Decoded-upload models ────────────────────────────────────────────────────

class DecodedDevice(BaseModel):
    id: str
    mac: str | None = None
    name: str | None = None


class DecodedStreams(BaseModel):
    hr: list[dict] = []
    rr: list[dict] = []
    events: list[dict] = []
    battery: list[dict] = []
    # Type-47 V24 biometric history (optional; older clients omit these). Values are
    # raw ADC for spo2/skin_temp/resp; gravity is the accel-derived vector in g.
    spo2: list[dict] = []
    skin_temp: list[dict] = []
    resp: list[dict] = []
    gravity: list[dict] = []


class DecodedBatch(BaseModel):
    device: DecodedDevice
    streams: DecodedStreams
    # WHOOP device generation that produced these streams (Phase 05, D-10 / SRV-01).
    # Optional + defaulted for backward compatibility: clients that omit it (e.g. the
    # 4.0 reference app) are classified '5.0' on this 5.0-only deployment. Validated
    # as a plain string by Pydantic; never feeds dynamic SQL (persisted parametrised).
    device_generation: str | None = "5.0"


@app.get("/healthz")
def healthz():
    try:
        with psycopg.connect(cfg.db_dsn, connect_timeout=3) as conn:
            conn.execute("SELECT 1")
        return {"status": "ok"}
    except Exception as e:
        raise HTTPException(status_code=503, detail=f"db unavailable: {e}")


@app.post("/v1/ingest", dependencies=[Depends(require_auth)])
def ingest_batch(batch: IngestBatch):
    payload = batch.model_dump()
    with psycopg.connect(cfg.db_dsn) as conn:
        result = ingest.process_batch(conn, cfg, payload)
        conn.commit()
    return result


def _batch_dates_utc(streams: dict) -> set[_dt.date]:
    """UTC calendar dates spanned by every stream-row ts in an ingest batch."""
    days: set[_dt.date] = set()
    for rows in streams.values():
        for r in rows or []:
            ts = r.get("ts")
            if ts is None:
                continue
            days.add(_dt.datetime.fromtimestamp(float(ts), _dt.timezone.utc).date())
    return days


@app.post("/v1/ingest-decoded", dependencies=[Depends(require_auth)])
def ingest_decoded(batch: DecodedBatch):
    payload = batch.model_dump()
    device_id = payload["device"]["id"]
    with psycopg.connect(cfg.db_dsn) as conn:
        store.ensure_device(conn, device_id,
                            mac=payload["device"].get("mac"),
                            name=payload["device"].get("name"))
        counts = store.upsert_streams(conn, device_id, payload["streams"])
        conn.commit()
        # Recompute the day(s) this batch touched — throttled (see _RECOMPUTE_*).
        # Best-effort: a compute error must NOT fail the ingest (the raw streams
        # are already persisted) — log + move on.
        for day in _batch_dates_utc(payload["streams"]):
            key = (device_id, day)
            if time.monotonic() - _last_recompute.get(key, 0.0) < _RECOMPUTE_COOLDOWN_S:
                continue  # debounce: this day was recomputed very recently
            if not _recompute_lock.acquire(blocking=False):
                continue  # single-flight: a recompute is already running; a later upload catches up
            try:
                daily.compute_day(conn, device_id, day)
                conn.commit()
            except Exception:
                conn.rollback()
                _log.exception("compute_day failed for %s %s (ingest still 200)", device_id, day)
            finally:
                _last_recompute[key] = time.monotonic()  # throttle successes AND failures
                _recompute_lock.release()
    return {"upserted": counts}


@app.get("/v1/devices", dependencies=[Depends(require_auth)])
def get_devices():
    with psycopg.connect(cfg.db_dsn) as conn:
        return read.list_devices(conn)


@app.get("/v1/batches", dependencies=[Depends(require_auth)])
def get_batches(device: str, limit: int = 100):
    with psycopg.connect(cfg.db_dsn) as conn:
        return read.list_batches(conn, device_id=device, limit=limit)


@app.get("/v1/summary", dependencies=[Depends(require_auth)])
def get_summary(device: str,
                from_: int = Query(0, alias="from"),
                to: int = Query(2_000_000_000, alias="to")):
    """Exact (unlimited) counts per decoded stream + raw batches, for accurate dashboard totals."""
    with psycopg.connect(cfg.db_dsn) as conn:
        return read.counts(conn, device_id=device, start=from_, end=to)


@app.get("/v1/streams/{kind}", dependencies=[Depends(require_auth)])
def get_stream(kind: str, device: str,
               from_: int = Query(0, alias="from"),
               to: int = Query(2_000_000_000, alias="to"),
               limit: int = 5000,
               max_points: int | None = None):
    try:
        with psycopg.connect(cfg.db_dsn) as conn:
            return read.query_stream(conn, kind, device_id=device, start=from_, end=to,
                                     limit=limit, max_points=max_points)
    except ValueError:
        raise HTTPException(status_code=404, detail=f"unknown stream kind: {kind}")


# ── Daily analysis endpoints (Task 2.5) ──────────────────────────────────────

class ComputeDaily(BaseModel):
    device: str
    date: str  # YYYY-MM-DD


def _parse_date(s: str) -> _dt.date:
    try:
        return _dt.date.fromisoformat(s)
    except ValueError:
        raise HTTPException(status_code=400, detail=f"invalid date (want YYYY-MM-DD): {s!r}")


@app.post("/v1/compute-daily", dependencies=[Depends(require_auth)])
def compute_daily(body: ComputeDaily):
    """Compute + persist the daily metrics for a device/date, returning the summary."""
    day = _parse_date(body.date)
    with psycopg.connect(cfg.db_dsn) as conn:
        result = daily.compute_day(conn, body.device, day)
        conn.commit()
    return result


@app.get("/v1/daily", dependencies=[Depends(require_auth)])
def get_daily(device: str,
              from_: str = Query(..., alias="from"),
              to: str = Query(..., alias="to")):
    """daily_metrics rows over the inclusive [from, to] date range (YYYY-MM-DD)."""
    start, end = _parse_date(from_), _parse_date(to)
    with psycopg.connect(cfg.db_dsn) as conn:
        return read.query_daily(conn, device, start, end)


@app.get("/v1/today", dependencies=[Depends(require_auth)])
def get_today(device: str):
    """Most-recent daily_metrics row for the device (ORDER BY day DESC LIMIT 1), or null."""
    with psycopg.connect(cfg.db_dsn) as conn:
        return read.query_today(conn, device)


@app.get("/v1/sleep", dependencies=[Depends(require_auth)])
def get_sleep(device: str, date: str):
    """Sleep sessions whose night ENDS on ``date`` (YYYY-MM-DD)."""
    day = _parse_date(date)
    with psycopg.connect(cfg.db_dsn) as conn:
        return read.query_sleep(conn, device, day)


# ── Profile endpoints ─────────────────────────────────────────────────────────

_VALID_SEX = {"male", "female", "nonbinary"}


class ProfileBody(BaseModel):
    device: str
    height_cm: float | None = None
    weight_kg: float | None = None
    age: int | None = None
    sex: str | None = None


@app.get("/v1/profile", dependencies=[Depends(require_auth)])
def get_profile(device: str):
    """Return the stored profile for a device, or {} if none exists."""
    with psycopg.connect(cfg.db_dsn) as conn:
        row = read.query_profile(conn, device)
    return row or {}


@app.post("/v1/profile", dependencies=[Depends(require_auth)])
def upsert_profile(body: ProfileBody):
    """Create or update the user profile (height/weight/age/sex) for a device."""
    sex = body.sex
    if sex is not None:
        sex = sex.lower().strip()
        if sex not in _VALID_SEX:
            raise HTTPException(
                status_code=422,
                detail=f"sex must be one of {sorted(_VALID_SEX)} or null; got {body.sex!r}",
            )
    with psycopg.connect(cfg.db_dsn) as conn:
        store.ensure_device(conn, body.device)
        store.upsert_profile(conn, body.device,
                             height_cm=body.height_cm,
                             weight_kg=body.weight_kg,
                             age=body.age,
                             sex=sex)
        conn.commit()
        row = read.query_profile(conn, body.device)
    return row


# ── Workouts endpoint ─────────────────────────────────────────────────────────

@app.get("/v1/workouts", dependencies=[Depends(require_auth)])
def get_workouts(device: str,
                 from_: str = Query(..., alias="from"),
                 to: str = Query(..., alias="to")):
    """Exercise sessions whose start_ts (UTC date) is in [from, to] (YYYY-MM-DD)."""
    start, end = _parse_date(from_), _parse_date(to)
    with psycopg.connect(cfg.db_dsn) as conn:
        return read.query_workouts(conn, device, start, end)


# ── Backfill workouts endpoint ────────────────────────────────────────────────

class BackfillWorkouts(BaseModel):
    device: str
    # "from"/"to" are Python keywords; declare them via alias so FastAPI/Pydantic
    # deserialises {"from": "...", "to": "..."} directly without a manual remap.
    # populate_by_name=True keeps from_date/to_date working for any internal callers.
    from_date: str | None = Field(default=None, alias="from")
    to_date:   str | None = Field(default=None, alias="to")

    model_config = {"populate_by_name": True}


@app.post("/v1/backfill-workouts", dependencies=[Depends(require_auth)])
def backfill_workouts(body: BackfillWorkouts):
    """Recompute exercise sessions (with calories) over a date range by replaying
    compute_day for each date. Idempotent — safe to re-run. May be slow for large
    ranges (runs the full daily pipeline per day). Auth-gated."""
    from_str = body.from_date
    to_str = body.to_date
    if from_str is None or to_str is None:
        raise HTTPException(status_code=422, detail="'from' and 'to' are required (YYYY-MM-DD)")
    start = _parse_date(from_str)
    end = _parse_date(to_str)
    if end < start:
        raise HTTPException(status_code=422, detail="'to' must be >= 'from'")
    results = []
    with psycopg.connect(cfg.db_dsn) as conn:
        day = start
        while day <= end:
            try:
                result = daily.compute_day(conn, body.device, day)
                conn.commit()
                results.append({"date": day.isoformat(), "status": "ok",
                                "exercises": result.get("exercises", [])})
            except Exception as exc:
                conn.rollback()
                _log.exception("backfill-workouts compute_day failed for device day=%s", day)
                results.append({"date": day.isoformat(), "status": "error", "detail": type(exc).__name__})
            day += _dt.timedelta(days=1)
    return {"recomputed": len(results), "days": results}


@app.get("/v1/batches/{batch_id}/frames", dependencies=[Depends(require_auth)])
def get_batch_frames(batch_id: str):
    with psycopg.connect(cfg.db_dsn) as conn:
        row = conn.execute(
            "SELECT file_path FROM raw_batches WHERE batch_id = %s", (batch_id,)
        ).fetchone()
    if row is None:
        raise HTTPException(status_code=404, detail="batch not found")
    return read.read_batch_frames(row[0])
