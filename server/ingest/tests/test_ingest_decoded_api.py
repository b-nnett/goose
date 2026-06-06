import importlib

import pytest
from fastapi.testclient import TestClient
from tests.conftest import requires_docker


@pytest.fixture
def client(clean_db, tmp_path, monkeypatch):
    monkeypatch.setenv("GOOSE_API_KEY", "secret")
    monkeypatch.setenv("GOOSE_DB_DSN", clean_db)
    monkeypatch.setenv("GOOSE_RAW_ROOT", str(tmp_path))
    import app.main as m
    importlib.reload(m)
    # Default the Bearer header so the now-authenticated read endpoints work in tests.
    return TestClient(m.app, headers={"Authorization": "Bearer secret"})


FULL_BODY = {
    "device": {"id": "devA", "mac": "AA:BB", "name": "whoop"},
    "streams": {
        "hr": [{"ts": 1700000000, "bpm": 60}, {"ts": 1700000001, "bpm": 61}],
        "rr": [{"ts": 1700000000, "rr_ms": 1000}],
        "events": [{"ts": 1700000000, "kind": "WRIST_ON", "payload": {}}],
        "battery": [{"ts": 1700000000, "soc": 80.0, "mv": 3900}],
    },
}

PARTIAL_BODY = {
    "device": {"id": "devB"},
    "streams": {"hr": [{"ts": 1, "bpm": 50}]},
}

# Type-47 V24 biometric payload — realistic values from the real on-device record.
BIOMETRIC_BODY = {
    "device": {"id": "devA", "mac": "AA:BB", "name": "whoop"},
    "streams": {
        "hr": [{"ts": 1700000000, "bpm": 63}],
        "spo2": [{"ts": 1700000000, "red": 18000, "ir": 17000, "unit": "raw_adc"}],
        "skin_temp": [{"ts": 1700000000, "raw": 900, "unit": "raw_adc"}],
        "resp": [{"ts": 1700000000, "raw": 3000, "unit": "raw_adc"}],
        "gravity": [{"ts": 1700000000, "x": 0.05, "y": 0.10, "z": 0.993734, "unit": "g"}],
    },
}


def test_ingest_decoded_requires_auth_no_docker(tmp_path, monkeypatch):
    """401 check — does NOT need Docker/DB; patches out the schema bootstrap."""
    monkeypatch.setenv("GOOSE_API_KEY", "secret")
    monkeypatch.setenv("GOOSE_DB_DSN", "postgresql://x:x@localhost:5432/x")
    monkeypatch.setenv("GOOSE_RAW_ROOT", str(tmp_path))
    import app.db as _db
    monkeypatch.setattr(_db, "bootstrap_schema", lambda dsn: None)
    import app.main as m
    importlib.reload(m)
    c = TestClient(m.app, raise_server_exceptions=False)
    r = c.post("/v1/ingest-decoded", json=FULL_BODY)
    assert r.status_code == 401


@requires_docker
def test_ingest_decoded_full(client):
    r = client.post(
        "/v1/ingest-decoded", json=FULL_BODY,
        headers={"Authorization": "Bearer secret"},
    )
    assert r.status_code == 200, r.text
    out = r.json()
    assert out == {"upserted": {"hr": 2, "rr": 1, "events": 1, "battery": 1,
                                "spo2": 0, "skin_temp": 0, "resp": 0, "gravity": 0}}


@requires_docker
def test_ingest_decoded_idempotent(client):
    """Re-posting the same body must return 200 and not duplicate rows."""
    headers = {"Authorization": "Bearer secret"}
    r1 = client.post("/v1/ingest-decoded", json=FULL_BODY, headers=headers)
    assert r1.status_code == 200, r1.text

    r2 = client.post("/v1/ingest-decoded", json=FULL_BODY, headers=headers)
    assert r2.status_code == 200, r2.text

    # Verify DB row count via the read API — should still equal first-post counts.
    hr = client.get("/v1/streams/hr", params={"device": "devA"}).json()
    assert len(hr) == 2, f"Expected 2 HR rows but got {len(hr)}: {hr}"

    rr = client.get("/v1/streams/rr", params={"device": "devA"}).json()
    assert len(rr) == 1

    events = client.get("/v1/streams/events", params={"device": "devA"}).json()
    assert len(events) == 1

    battery = client.get("/v1/streams/battery", params={"device": "devA"}).json()
    assert len(battery) == 1


@requires_docker
def test_ingest_decoded_biometric_v24(client, clean_db):
    """POST a V24 biometric batch; rows land in spo2/skin_temp/resp/gravity tables."""
    import psycopg
    r = client.post(
        "/v1/ingest-decoded", json=BIOMETRIC_BODY,
        headers={"Authorization": "Bearer secret"},
    )
    assert r.status_code == 200, r.text
    assert r.json() == {"upserted": {"hr": 1, "rr": 0, "events": 0, "battery": 0,
                                     "spo2": 1, "skin_temp": 1, "resp": 1, "gravity": 1}}
    # Verify rows landed (read API doesn't expose these kinds; query DB directly).
    with psycopg.connect(clean_db) as conn, conn.cursor() as cur:
        cur.execute("SELECT red, ir FROM spo2_samples WHERE device_id='devA'")
        assert cur.fetchone() == (18000, 17000)
        cur.execute("SELECT raw FROM skin_temp_samples WHERE device_id='devA'")
        assert cur.fetchone()[0] == 900
        cur.execute("SELECT raw FROM resp_samples WHERE device_id='devA'")
        assert cur.fetchone()[0] == 3000
        cur.execute("SELECT x, y, z FROM gravity_samples WHERE device_id='devA'")
        x, y, z = cur.fetchone()
        assert abs(x - 0.05) < 1e-4 and abs(y - 0.10) < 1e-4 and abs(z - 0.993734) < 1e-4


@requires_docker
def test_ingest_decoded_partial(client):
    """Only hr provided — rr/events/battery counts should be 0."""
    r = client.post(
        "/v1/ingest-decoded", json=PARTIAL_BODY,
        headers={"Authorization": "Bearer secret"},
    )
    assert r.status_code == 200, r.text
    assert r.json() == {"upserted": {"hr": 1, "rr": 0, "events": 0, "battery": 0,
                                     "spo2": 0, "skin_temp": 0, "resp": 0, "gravity": 0}}
