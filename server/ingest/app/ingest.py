"""Ingest pipeline: validate a batch, archive raw frames, decode via whoop-protocol,
store decoded streams. Idempotent on batch_id."""
import datetime

import psycopg
from whoop_protocol import parse_frame, extract_streams

from . import archive, store


def process_batch(conn: psycopg.Connection, cfg, batch: dict) -> dict:
    batch_id = batch["batch_id"]
    device_id = batch["device"]["device_id"]
    device_ref = int(batch["clock_ref"]["device"])
    wall_ref = int(batch["clock_ref"]["wall"])
    frames = [bytes.fromhex(f["hex"]) for f in batch["frames"]]

    store.ensure_device(conn, device_id, mac=batch["device"].get("mac"),
                        name=batch["device"].get("name"))

    if store.batch_exists(conn, batch_id):
        return {"batch_id": batch_id, "accepted": True, "deduped": True,
                "decoded_counts": {"hr": 0, "rr": 0, "events": 0, "battery": 0}}

    # archive raw (always — regardless of decode_streams flag)
    day = datetime.datetime.fromtimestamp(wall_ref, datetime.timezone.utc).strftime("%Y-%m-%d")
    meta = archive.write_raw_batch(cfg.raw_root, device_id, batch_id, day, frames)

    if batch.get("decode_streams", True):
        # decode -> streams
        parsed = [parse_frame(fr) for fr in frames]
        streams = extract_streams(parsed, device_clock_ref=device_ref, wall_clock_ref=wall_ref)

        # time bounds for the batch index (min/max wall ts across decoded streams)
        all_ts = [row["ts"] for s in streams.values() for row in s]
        start_ts = min(all_ts) if all_ts else wall_ref
        end_ts = max(all_ts) if all_ts else wall_ref

        store.insert_raw_batch(conn, {
            "batch_id": batch_id, "device_id": device_id,
            "device_clock_ref": device_ref, "wall_clock_ref": wall_ref,
            "start_ts": start_ts, "end_ts": end_ts,
            "packet_count": meta["packet_count"], "file_path": meta["file_path"],
            "sha256": meta["sha256"], "byte_size": meta["byte_size"],
        })
        counts = store.upsert_streams(conn, device_id, streams)
    else:
        # archive-only: skip decode/upsert; use wall_ref as time bounds placeholder
        store.insert_raw_batch(conn, {
            "batch_id": batch_id, "device_id": device_id,
            "device_clock_ref": device_ref, "wall_clock_ref": wall_ref,
            "start_ts": wall_ref, "end_ts": wall_ref,
            "packet_count": meta["packet_count"], "file_path": meta["file_path"],
            "sha256": meta["sha256"], "byte_size": meta["byte_size"],
        })
        counts = {"hr": 0, "rr": 0, "events": 0, "battery": 0}

    return {"batch_id": batch_id, "accepted": True, "deduped": False,
            "decoded_counts": counts, "file_path": meta["file_path"]}
