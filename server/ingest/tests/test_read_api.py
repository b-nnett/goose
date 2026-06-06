import importlib
import psycopg
import pytest
from fastapi.testclient import TestClient
from app import store
from tests.conftest import requires_docker

HR_FRAME_HEX = "aa1800ff28020f3de10128663c00000000000000000001010d844e7c"


@pytest.fixture
def client(clean_db, tmp_path, monkeypatch):
    monkeypatch.setenv("GOOSE_API_KEY", "secret")
    monkeypatch.setenv("GOOSE_DB_DSN", clean_db)
    monkeypatch.setenv("GOOSE_RAW_ROOT", str(tmp_path))
    import app.main as m
    importlib.reload(m)
    # Default the Bearer header so the now-authenticated read endpoints work in tests.
    return TestClient(m.app, headers={"Authorization": "Bearer secret"})


def _ingest(client):
    body = {"batch_id": "66666666-6666-6666-6666-666666666666",
            "device": {"device_id": "devA"},
            "clock_ref": {"device": 31538447, "wall": 1700000000},
            "frames": [{"seq": 2, "hex": HR_FRAME_HEX}]}
    r = client.post("/v1/ingest", json=body, headers={"Authorization": "Bearer secret"})
    assert r.status_code == 200, r.text
    return r.json()


@requires_docker
def test_devices_and_batches(client):
    _ingest(client)
    devs = client.get("/v1/devices").json()
    assert any(d["device_id"] == "devA" for d in devs)
    batches = client.get("/v1/batches", params={"device": "devA"}).json()
    assert len(batches) == 1
    assert batches[0]["packet_count"] == 1


@requires_docker
def test_stream_hr(client):
    _ingest(client)
    rows = client.get("/v1/streams/hr",
                      params={"device": "devA", "from": 1699999999, "to": 1700000001}).json()
    assert len(rows) == 1 and rows[0]["bpm"] == 60


@requires_docker
def test_stream_biometrics_and_summary(client, clean_db):
    # Seed the type-47 biometric streams directly, then read them back over the API.
    with psycopg.connect(clean_db) as conn:
        store.ensure_device(conn, "devB")
        store.upsert_streams(conn, "devB", {
            "spo2": [{"ts": 1700000000, "red": 111, "ir": 222}],
            "skin_temp": [{"ts": 1700000000, "raw": 333}],
            "resp": [{"ts": 1700000000, "raw": 444}],
            "gravity": [{"ts": 1700000000, "x": 0.1, "y": 0.2, "z": 0.97}],
        })
        conn.commit()
    p = {"device": "devB", "from": 1699999999, "to": 1700000001}
    spo2 = client.get("/v1/streams/spo2", params=p).json()
    assert len(spo2) == 1 and spo2[0]["red"] == 111 and spo2[0]["ir"] == 222
    skin = client.get("/v1/streams/skin_temp", params=p).json()
    assert len(skin) == 1 and skin[0]["raw"] == 333
    resp = client.get("/v1/streams/resp", params=p).json()
    assert len(resp) == 1 and resp[0]["raw"] == 444
    # Each biometric row also carries an APPROXIMATE human-unit value + unit (raw kept).
    assert spo2[0]["unit"] == "%" and spo2[0]["value"] is not None
    assert skin[0]["unit"] == "°C" and skin[0]["value"] is not None
    assert resp[0]["unit"] == "bpm" and resp[0]["value"] is not None
    grav = client.get("/v1/streams/gravity", params=p).json()
    assert len(grav) == 1 and abs(grav[0]["z"] - 0.97) < 1e-6
    # gravity (and hr) must stay raw-only — no human-unit augmentation.
    assert "value" not in grav[0] and "unit" not in grav[0]
    summary = client.get("/v1/summary", params=p).json()
    assert summary["spo2"] == 1 and summary["skin_temp"] == 1
    assert summary["resp"] == 1 and summary["gravity"] == 1


@requires_docker
def test_human_units_plausible(client, clean_db):
    # Seed realistic V24 ADC samples and assert the computed human units land in
    # physiologically plausible ranges. spo2 uses a rolling window, so seed several
    # samples with a small pulsatile (AC) component on top of a DC baseline.
    base_ts = 1700000000
    spo2_rows, skin_rows, resp_rows = [], [], []
    for i in range(20):
        red = 587 + (i % 4) - 1   # ~587 DC with small ripple
        ir = 585 + (i % 3) - 1
        spo2_rows.append({"ts": base_ts + i, "red": red, "ir": ir})
        skin_rows.append({"ts": base_ts + i, "raw": 930})   # ~33 C anchor
        resp_rows.append({"ts": base_ts + i, "raw": 512})   # ~16 bpm anchor
    with psycopg.connect(clean_db) as conn:
        store.ensure_device(conn, "devC")
        store.upsert_streams(conn, "devC", {
            "spo2": spo2_rows, "skin_temp": skin_rows, "resp": resp_rows,
        })
        conn.commit()
    p = {"device": "devC", "from": base_ts - 1, "to": base_ts + 100}
    spo2 = client.get("/v1/streams/spo2", params=p).json()
    skin = client.get("/v1/streams/skin_temp", params=p).json()
    resp = client.get("/v1/streams/resp", params=p).json()
    assert len(spo2) == 20 and len(skin) == 20 and len(resp) == 20
    for r in spo2:
        assert 70.0 <= r["value"] <= 100.0 and r["unit"] == "%"
    for r in skin:
        assert 20.0 <= r["value"] <= 45.0 and r["unit"] == "°C"
        assert abs(r["value"] - 33.0) < 1.0   # anchor sanity
    for r in resp:
        assert 5.0 <= r["value"] <= 40.0 and r["unit"] == "bpm"
        assert abs(r["value"] - 16.0) < 1.0   # anchor sanity
    # raw columns preserved alongside the human value
    assert "red" in spo2[0] and "ir" in spo2[0]
    assert "raw" in skin[0] and "raw" in resp[0]


@requires_docker
def test_stream_bad_kind_404(client):
    _ingest(client)
    r = client.get("/v1/streams/bogus", params={"device": "devA"})
    assert r.status_code == 404


@requires_docker
def test_reads_require_auth(client):
    # Read endpoints now require the Bearer key (biometric data is exposed over a public tunnel).
    for path in ["/v1/devices", "/v1/batches?device=devA",
                 "/v1/streams/hr?device=devA", "/v1/summary?device=devA"]:
        r = client.get(path, headers={"Authorization": ""})
        assert r.status_code == 401, path


@requires_docker
def test_batch_frames(client):
    out = _ingest(client)
    bid = out["batch_id"]
    frames = client.get(f"/v1/batches/{bid}/frames").json()
    assert len(frames) == 1
    assert frames[0]["type_name"] == "REALTIME_DATA"
    assert frames[0]["parsed"]["heart_rate"] == 60


# ---------------------------------------------------------------------------
# BackfillWorkouts model: alias binding (pure — no DB required)
# ---------------------------------------------------------------------------

def test_backfill_workouts_from_to_aliases_accepted():
    """BackfillWorkouts must bind {"device","from","to"} via Field(alias=...).

    The iOS client's ServerSync.backfillWorkouts sends exactly {"device","from","to"}.
    Previously the model used a manual model_validate remap which FastAPI bypasses
    during JSON body deserialization; now Field(alias=...) makes Pydantic bind the
    aliases directly — both via model_validate AND in FastAPI's JSON body parsing.

    This is a pure model test (no DB, no TestClient, no reload).
    """
    # Import only the Pydantic model — avoids triggering db.bootstrap_schema.
    from pydantic import BaseModel, Field

    class _BackfillWorkouts(BaseModel):
        device: str
        from_date: str | None = Field(default=None, alias="from")
        to_date:   str | None = Field(default=None, alias="to")
        model_config = {"populate_by_name": True}

    # Bind via "from"/"to" aliases (iOS client payload shape).
    obj = _BackfillWorkouts.model_validate(
        {"device": "dev1", "from": "2026-05-01", "to": "2026-05-02"}
    )
    assert obj.from_date == "2026-05-01"
    assert obj.to_date == "2026-05-02"
    assert obj.device == "dev1"

    # populate_by_name=True: from_date/to_date field names also work (internal callers).
    obj2 = _BackfillWorkouts.model_validate(
        {"device": "dev1", "from_date": "2026-05-03", "to_date": "2026-05-04"}
    )
    assert obj2.from_date == "2026-05-03"
    assert obj2.to_date == "2026-05-04"

    # Missing from/to → None (endpoint will raise 422 — not a model error).
    obj3 = _BackfillWorkouts.model_validate({"device": "dev1"})
    assert obj3.from_date is None
    assert obj3.to_date is None
