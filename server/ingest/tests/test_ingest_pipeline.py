import psycopg
import pytest
from app import ingest
from app.config import Config
from tests.conftest import requires_docker

# A real REALTIME_DATA frame (hr=60, device ts=31538447) from the captures.
HR_FRAME_HEX = "aa1800ff28020f3de10128663c00000000000000000001010d844e7c"


@requires_docker
def test_process_batch_archives_and_stores(clean_db, tmp_path):
    cfg = Config(api_key="k", db_dsn=clean_db, raw_root=str(tmp_path))
    batch = {
        "batch_id": "22222222-2222-2222-2222-222222222222",
        "device": {"device_id": "devA"},
        "clock_ref": {"device": 31538447, "wall": 1700000000},
        "frames": [{"seq": 2, "hex": HR_FRAME_HEX}],
    }
    with psycopg.connect(clean_db) as conn:
        result = ingest.process_batch(conn, cfg, batch)
        conn.commit()
    assert result["accepted"] is True
    assert result["deduped"] is False
    assert result["decoded_counts"]["hr"] == 1
    # raw archived
    import os
    assert os.path.exists(result["file_path"])
    # hr row landed at the mapped wall clock (device ref == frame ts -> wall ref exactly)
    with psycopg.connect(clean_db) as conn:
        n = conn.execute("SELECT bpm FROM hr_samples WHERE ts=to_timestamp(1700000000)").fetchone()
        assert n[0] == 60


@requires_docker
def test_process_batch_idempotent(clean_db, tmp_path):
    cfg = Config(api_key="k", db_dsn=clean_db, raw_root=str(tmp_path))
    batch = {
        "batch_id": "33333333-3333-3333-3333-333333333333",
        "device": {"device_id": "devA"},
        "clock_ref": {"device": 31538447, "wall": 1700000000},
        "frames": [{"seq": 2, "hex": HR_FRAME_HEX}],
    }
    with psycopg.connect(clean_db) as conn:
        ingest.process_batch(conn, cfg, batch); conn.commit()
    with psycopg.connect(clean_db) as conn:
        second = ingest.process_batch(conn, cfg, batch); conn.commit()
    assert second["deduped"] is True
    with psycopg.connect(clean_db) as conn:
        cnt = conn.execute("SELECT count(*) FROM hr_samples WHERE device_id='devA'").fetchone()[0]
    assert cnt == 1  # not doubled
