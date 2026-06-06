import os
import psycopg
import pytest
from fastapi.testclient import TestClient
from tests.conftest import requires_docker

HR_FRAME_HEX = "aa1800ff28020f3de10128663c00000000000000000001010d844e7c"


@pytest.fixture
def client(clean_db, tmp_path, monkeypatch):
    monkeypatch.setenv("GOOSE_API_KEY", "secret")
    monkeypatch.setenv("GOOSE_DB_DSN", clean_db)
    monkeypatch.setenv("GOOSE_RAW_ROOT", str(tmp_path))
    import app.main as m
    import importlib
    importlib.reload(m)  # rebuild app with the patched env
    # Default the Bearer header so the now-authenticated read endpoints work in tests.
    return TestClient(m.app, headers={"Authorization": "Bearer secret"})


@requires_docker
def test_healthz_ok(client):
    r = client.get("/healthz")
    assert r.status_code == 200
    assert r.json()["status"] == "ok"


@requires_docker
def test_ingest_requires_auth(client):
    # Override the fixture's default Bearer with an empty one to prove auth is enforced.
    r = client.post("/v1/ingest", json={}, headers={"Authorization": ""})
    assert r.status_code == 401


@requires_docker
def test_ingest_stores_batch(client):
    body = {
        "batch_id": "44444444-4444-4444-4444-444444444444",
        "device": {"device_id": "devA"},
        "clock_ref": {"device": 31538447, "wall": 1700000000},
        "frames": [{"seq": 2, "hex": HR_FRAME_HEX}],
    }
    r = client.post("/v1/ingest", json=body, headers={"Authorization": "Bearer secret"})
    assert r.status_code == 200, r.text
    out = r.json()
    assert out["accepted"] is True and out["deduped"] is False
    assert out["decoded_counts"]["hr"] == 1
    # re-POST is idempotent
    r2 = client.post("/v1/ingest", json=body, headers={"Authorization": "Bearer secret"})
    assert r2.json()["deduped"] is True


@requires_docker
def test_ingest_archive_only(client):
    """decode_streams=false: raw batch IS archived, decoded streams are NOT upserted."""
    headers = {"Authorization": "Bearer secret"}
    body = {
        "batch_id": "55555555-5555-5555-5555-555555555555",
        "device": {"device_id": "devArchiveOnly"},
        "clock_ref": {"device": 31538447, "wall": 1700000000},
        "frames": [{"seq": 2, "hex": HR_FRAME_HEX}],
        "decode_streams": False,
    }
    r = client.post("/v1/ingest", json=body, headers=headers)
    assert r.status_code == 200, r.text
    out = r.json()
    assert out["accepted"] is True
    assert out["deduped"] is False

    # Raw batch must be indexed
    batches = client.get("/v1/batches", params={"device": "devArchiveOnly"}).json()
    assert len(batches) == 1
    assert batches[0]["batch_id"] == "55555555-5555-5555-5555-555555555555"

    # Frames must be retrievable from the archive
    frames = client.get("/v1/batches/55555555-5555-5555-5555-555555555555/frames").json()
    assert len(frames) == 1

    # No decoded rows must have been written for any stream kind
    for kind in ("hr", "rr", "events", "battery"):
        rows = client.get(f"/v1/streams/{kind}", params={"device": "devArchiveOnly"}).json()
        assert rows == [], f"Expected no {kind} rows for archive-only batch, got: {rows}"

    # Confirm decoded_counts are all zero
    assert out["decoded_counts"] == {"hr": 0, "rr": 0, "events": 0, "battery": 0}


@requires_docker
def test_ingest_decode_default_still_decodes(client):
    """Omitting decode_streams (default True) must still produce decoded rows."""
    headers = {"Authorization": "Bearer secret"}
    body = {
        "batch_id": "66666666-6666-6666-6666-666666666666",
        "device": {"device_id": "devDefaultDecode"},
        "clock_ref": {"device": 31538447, "wall": 1700000000},
        "frames": [{"seq": 2, "hex": HR_FRAME_HEX}],
        # decode_streams omitted — default True
    }
    r = client.post("/v1/ingest", json=body, headers=headers)
    assert r.status_code == 200, r.text
    out = r.json()
    assert out["accepted"] is True
    assert out["decoded_counts"]["hr"] == 1

    # Decoded HR rows must exist
    hr_rows = client.get("/v1/streams/hr", params={"device": "devDefaultDecode"}).json()
    assert len(hr_rows) == 1
