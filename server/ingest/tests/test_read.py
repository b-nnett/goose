import psycopg
import pytest
from app import read, store
from tests.conftest import requires_docker


def _seed(dsn):
    with psycopg.connect(dsn) as conn:
        store.ensure_device(conn, "devA")
        store.upsert_streams(conn, "devA", {
            "hr": [{"ts": 1700000000, "bpm": 60}, {"ts": 1700000060, "bpm": 62}],
            "rr": [{"ts": 1700000000, "rr_ms": 850}],
            "events": [{"ts": 1700000000, "kind": "WRIST_ON(9)", "payload": {"a": 1}}],
            "battery": [{"ts": 1700000000, "soc": 25.5, "mv": 3900}],
            "spo2": [{"ts": 1700000000, "red": 12345, "ir": 23456}],
            "skin_temp": [{"ts": 1700000000, "raw": 9876}],
            "resp": [{"ts": 1700000000, "raw": 5432}],
            "gravity": [{"ts": 1700000000, "x": 0.01, "y": -0.5, "z": 0.98}],
        })
        store.insert_raw_batch(conn, {
            "batch_id": "55555555-5555-5555-5555-555555555555", "device_id": "devA",
            "device_clock_ref": 1, "wall_clock_ref": 1700000000, "start_ts": 1700000000,
            "end_ts": 1700000060, "packet_count": 2, "file_path": "/x.zst",
            "sha256": "deadbeef", "byte_size": 10})
        conn.commit()


@requires_docker
def test_list_devices(clean_db):
    _seed(clean_db)
    with psycopg.connect(clean_db) as conn:
        devs = read.list_devices(conn)
    assert any(d["device_id"] == "devA" for d in devs)


@requires_docker
def test_list_batches(clean_db):
    _seed(clean_db)
    with psycopg.connect(clean_db) as conn:
        batches = read.list_batches(conn, device_id="devA", limit=10)
    assert len(batches) == 1
    assert batches[0]["batch_id"] == "55555555-5555-5555-5555-555555555555"
    assert batches[0]["packet_count"] == 2


@requires_docker
def test_query_stream_hr_with_range(clean_db):
    _seed(clean_db)
    with psycopg.connect(clean_db) as conn:
        rows = read.query_stream(conn, "hr", device_id="devA",
                                 start=1699999999, end=1700000001, limit=100)
    assert len(rows) == 1  # only the ts=1700000000 sample falls in range
    assert rows[0]["bpm"] == 60


@requires_docker
def test_query_stream_biometrics(clean_db):
    _seed(clean_db)
    with psycopg.connect(clean_db) as conn:
        spo2 = read.query_stream(conn, "spo2", device_id="devA",
                                 start=1699999999, end=1700000001, limit=100)
        skin = read.query_stream(conn, "skin_temp", device_id="devA",
                                 start=1699999999, end=1700000001, limit=100)
        resp = read.query_stream(conn, "resp", device_id="devA",
                                 start=1699999999, end=1700000001, limit=100)
        grav = read.query_stream(conn, "gravity", device_id="devA",
                                 start=1699999999, end=1700000001, limit=100)
    assert len(spo2) == 1 and spo2[0]["red"] == 12345 and spo2[0]["ir"] == 23456
    assert len(skin) == 1 and skin[0]["raw"] == 9876
    assert len(resp) == 1 and resp[0]["raw"] == 5432
    assert len(grav) == 1 and abs(grav[0]["z"] - 0.98) < 1e-6


@requires_docker
def test_counts_includes_biometrics(clean_db):
    _seed(clean_db)
    with psycopg.connect(clean_db) as conn:
        c = read.counts(conn, device_id="devA", start=1699999999, end=1700000001)
    assert c["spo2"] == 1 and c["skin_temp"] == 1 and c["resp"] == 1 and c["gravity"] == 1


@requires_docker
def test_query_stream_downsamples_to_max_points(clean_db):
    # Seed WAY more than max_points rows at 1 s spacing, then assert the bucketed
    # result is ~max_points, spans the FULL range (latest data represented, not the
    # oldest-truncated), and values are plausible averages.
    base = 1700000000
    n = 6000
    with psycopg.connect(clean_db) as conn:
        store.ensure_device(conn, "devDS")
        store.upsert_streams(conn, "devDS", {
            "hr": [{"ts": base + i, "bpm": 60 + (i % 40)} for i in range(n)],
        })
        conn.commit()
        rows = read.query_stream(conn, "hr", device_id="devDS",
                                 start=base, end=base + n - 1, limit=20000,
                                 max_points=1000)
    # ~max_points (bucketed), NOT the raw 6000. time_bucket aligns to the epoch grid,
    # so the count can land a hair over max_points (boundary straddle); allow ~10%.
    assert 1 < len(rows) <= 1100, f"expected ~1000 buckets, got {len(rows)}"
    assert len(rows) < n
    # Spans from ~start to the last bucket (latest data represented).
    first_ts = rows[0]["ts"].timestamp()
    last_ts = rows[-1]["ts"].timestamp()
    assert abs(first_ts - base) < 60
    # Last bucket start must be within one bucket-width of the final sample's ts.
    width = -(-(n - 1) // 1000)  # ceil(span/max_points)
    assert (base + n - 1) - last_ts <= width
    # Values are plausible averages within the seeded bpm band [60, 99].
    assert all(60 <= r["bpm"] <= 99 for r in rows)


@requires_docker
def test_query_stream_downsample_uses_data_span_not_window(clean_db):
    # The dashboard "all" range is a giant sentinel window (from=0&to=2e9). The bucket
    # width must derive from the ACTUAL data extent, not the window — otherwise hours of
    # data collapse into a single bucket. Seed 3000 rows over 3000 s but query [0, 2e9].
    base = 1700000000
    n = 3000
    with psycopg.connect(clean_db) as conn:
        store.ensure_device(conn, "devSP")
        store.upsert_streams(conn, "devSP", {
            "hr": [{"ts": base + i, "bpm": 80} for i in range(n)],
        })
        conn.commit()
        rows = read.query_stream(conn, "hr", device_id="devSP",
                                 start=0, end=2_000_000_000, limit=20000,
                                 max_points=500)
    # Must produce many buckets (~max_points over the real ~3000 s span), NOT 1.
    assert len(rows) > 100, f"sentinel window collapsed to {len(rows)} bucket(s)"
    assert len(rows) <= 600
    assert all(r["bpm"] == 80 for r in rows)


@requires_docker
def test_query_stream_no_downsample_when_under_budget(clean_db):
    # max_points set but row count <= max_points → raw rows unchanged (no regression).
    _seed(clean_db)
    with psycopg.connect(clean_db) as conn:
        rows = read.query_stream(conn, "hr", device_id="devA",
                                 start=1699999999, end=1700000061, limit=100,
                                 max_points=1000)
    assert len(rows) == 2
    assert rows[0]["bpm"] == 60 and rows[1]["bpm"] == 62


@requires_docker
def test_query_stream_max_points_none_unchanged(clean_db):
    # max_points=None → identical to today's behaviour (raw rows).
    base = 1700000000
    with psycopg.connect(clean_db) as conn:
        store.ensure_device(conn, "devNP")
        store.upsert_streams(conn, "devNP", {
            "hr": [{"ts": base + i, "bpm": 70} for i in range(50)],
        })
        conn.commit()
        rows = read.query_stream(conn, "hr", device_id="devNP",
                                 start=base, end=base + 49, max_points=None)
    assert len(rows) == 50
    assert all(r["bpm"] == 70 for r in rows)


@requires_docker
def test_query_stream_events_not_downsampled(clean_db):
    # events has text/jsonb cols → must be skipped (capped, not averaged).
    base = 1700000000
    with psycopg.connect(clean_db) as conn:
        store.ensure_device(conn, "devEV")
        store.upsert_streams(conn, "devEV", {
            "events": [{"ts": base + i, "kind": "WRIST_ON(9)", "payload": {"i": i}}
                       for i in range(100)],
        })
        conn.commit()
        rows = read.query_stream(conn, "events", device_id="devEV",
                                 start=base, end=base + 99, limit=10, max_points=5)
    # Not bucketed; just capped at limit, kind text intact.
    assert len(rows) == 10
    assert rows[0]["kind"] == "WRIST_ON(9)"


@requires_docker
def test_query_stream_downsample_preserves_units(clean_db):
    # spo2 downsampled rows still get value/unit augmentation.
    base = 1700000000
    n = 3000
    with psycopg.connect(clean_db) as conn:
        store.ensure_device(conn, "devSU")
        store.upsert_streams(conn, "devSU", {
            "spo2": [{"ts": base + i, "red": 12000 + (i % 100), "ir": 23000 + (i % 100)}
                     for i in range(n)],
        })
        conn.commit()
        rows = read.query_stream(conn, "spo2", device_id="devSU",
                                 start=base, end=base + n - 1, max_points=500)
    assert len(rows) < n
    assert all("unit" in r and r["unit"] == "%" for r in rows)
    assert all("value" in r for r in rows)


@requires_docker
def test_query_stream_unknown_kind_raises(clean_db):
    with psycopg.connect(clean_db) as conn:
        with pytest.raises(ValueError):
            read.query_stream(conn, "bogus", device_id="devA", start=0, end=1, limit=1)


from app import archive, read as read_mod


def test_read_batch_frames_parses_archive(tmp_path):
    # write a real REALTIME_DATA frame to an archive, then read+parse it back
    hr_frame = bytes.fromhex("aa1800ff28020f3de10128663c00000000000000000001010d844e7c")
    meta = archive.write_raw_batch(str(tmp_path), "devA", "b1", "2026-05-23", [hr_frame])
    frames = read_mod.read_batch_frames(meta["file_path"])
    assert len(frames) == 1
    f = frames[0]
    assert f["type_name"] == "REALTIME_DATA"
    assert f["parsed"]["heart_rate"] == 60
    assert f["hex"] == hr_frame.hex()
    assert any(fld["name"] == "heart_rate" for fld in f["fields"])
