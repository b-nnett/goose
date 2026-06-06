import psycopg
import pytest
from app import store
from tests.conftest import requires_docker


@requires_docker
def test_ensure_device_and_batch_idempotency(clean_db):
    dsn = clean_db
    with psycopg.connect(dsn) as conn:
        store.ensure_device(conn, "devA")
        assert store.batch_exists(conn, "11111111-1111-1111-1111-111111111111") is False
        store.insert_raw_batch(conn, {
            "batch_id": "11111111-1111-1111-1111-111111111111", "device_id": "devA",
            "device_clock_ref": 1000, "wall_clock_ref": 1700000000,
            "start_ts": 1700000000, "end_ts": 1700000005, "packet_count": 3,
            "file_path": "/data/raw/devA/2026-05-23/b.zst", "sha256": "abc", "byte_size": 42,
        })
        conn.commit()
        assert store.batch_exists(conn, "11111111-1111-1111-1111-111111111111") is True


@requires_docker
def test_upsert_streams_inserts_and_dedupes(clean_db):
    dsn = clean_db
    streams = {
        "hr": [{"ts": 1700000000, "bpm": 60}, {"ts": 1700000001, "bpm": 61}],
        "rr": [{"ts": 1700000000, "rr_ms": 850}],
        "events": [{"ts": 1700000000, "kind": "WRIST_ON(9)", "payload": {"x": 1}}],
        "battery": [{"ts": 1700000000, "soc": 25.5, "mv": 3900}],
    }
    with psycopg.connect(dsn) as conn:
        store.ensure_device(conn, "devA")
        store.upsert_streams(conn, "devA", streams)
        store.upsert_streams(conn, "devA", streams)  # second time must not duplicate
        conn.commit()
        with conn.cursor() as cur:
            cur.execute("SELECT count(*) FROM hr_samples WHERE device_id='devA'")
            assert cur.fetchone()[0] == 2
            cur.execute("SELECT bpm FROM hr_samples WHERE ts=to_timestamp(1700000000)")
            assert cur.fetchone()[0] == 60
            cur.execute("SELECT count(*) FROM events WHERE kind='WRIST_ON(9)'")
            assert cur.fetchone()[0] == 1
            cur.execute("SELECT soc FROM battery WHERE device_id='devA'")
            assert abs(cur.fetchone()[0] - 25.5) < 0.01


@requires_docker
def test_upsert_streams_biometric_v24(clean_db):
    """Type-47 V24 biometric streams (spo2/skin_temp/resp/gravity) persist as raw ADC,
    keyed by ts, idempotently. Realistic values from the real V24 record."""
    dsn = clean_db
    streams = {
        "hr": [{"ts": 1700000000, "bpm": 63}],
        "spo2": [{"ts": 1700000000, "red": 18000, "ir": 17000, "unit": "raw_adc"}],
        "skin_temp": [{"ts": 1700000000, "raw": 900, "unit": "raw_adc"}],
        "resp": [{"ts": 1700000000, "raw": 3000, "unit": "raw_adc"}],
        "gravity": [{"ts": 1700000000, "x": 0.05, "y": 0.10, "z": 0.993734, "unit": "g"}],
    }
    with psycopg.connect(dsn) as conn:
        store.ensure_device(conn, "devA")
        counts = store.upsert_streams(conn, "devA", streams)
        store.upsert_streams(conn, "devA", streams)  # second time must not duplicate
        conn.commit()
        assert counts["spo2"] == 1
        assert counts["skin_temp"] == 1
        assert counts["resp"] == 1
        assert counts["gravity"] == 1
        with conn.cursor() as cur:
            cur.execute("SELECT count(*), max(red), max(ir) FROM spo2_samples WHERE device_id='devA'")
            assert cur.fetchone() == (1, 18000, 17000)
            cur.execute("SELECT raw FROM skin_temp_samples WHERE device_id='devA'")
            assert cur.fetchone()[0] == 900
            cur.execute("SELECT raw FROM resp_samples WHERE device_id='devA'")
            assert cur.fetchone()[0] == 3000
            cur.execute("SELECT x, y, z FROM gravity_samples WHERE device_id='devA'")
            x, y, z = cur.fetchone()
            assert abs(x - 0.05) < 1e-4 and abs(y - 0.10) < 1e-4 and abs(z - 0.993734) < 1e-4


@requires_docker
def test_upsert_streams_backward_compat_no_biometric(clean_db):
    """A batch WITHOUT the new biometric streams still works and inserts nothing into them."""
    dsn = clean_db
    streams = {"hr": [{"ts": 1700000000, "bpm": 60}]}
    with psycopg.connect(dsn) as conn:
        store.ensure_device(conn, "devB")
        counts = store.upsert_streams(conn, "devB", streams)
        conn.commit()
        assert counts == {"hr": 1, "rr": 0, "events": 0, "battery": 0,
                          "spo2": 0, "skin_temp": 0, "resp": 0, "gravity": 0}
        with conn.cursor() as cur:
            for table in ("spo2_samples", "skin_temp_samples", "resp_samples", "gravity_samples"):
                cur.execute(f"SELECT count(*) FROM {table}")
                assert cur.fetchone()[0] == 0
