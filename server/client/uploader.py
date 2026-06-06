"""Replay WHOOP capture files into the ingest service.

Supports two capture formats:
  - full frames:  {"full_hex": "...", ...}                  (e.g. capture.jsonl)
  - bare payload: {"datalen": N, "data_hex": "...", "cmd": C} (motion/optical captures;
                  reframed via whoop_protocol.frame_from_payload)

Clock correlation: if any record carries a `wall` field we anchor wall_clock_ref to the
first such record and device_clock_ref to the device timestamp in its payload (data[4:8]).
Otherwise we fall back to the file mtime as wall_clock_ref and the first decodable device
timestamp as device_clock_ref. This is best-effort for REPLAY; the live client captures a
real correlation at connect time.

Usage:
  uploader.py <capture.jsonl> --url http://localhost:8770 --token <WHOOP_API_KEY> \
              --device <device_id> [--batch-size 100]
"""
import argparse
import json
import os
import struct
import uuid

import urllib.request

from whoop_protocol import frame_from_payload


def _iter_records(path):
    with open(path) as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            try:
                yield json.loads(line)
            except ValueError:
                continue


def _record_to_hex(rec):
    if "full_hex" in rec:
        return rec["full_hex"]
    if "datalen" in rec and "data_hex" in rec:
        data = bytes.fromhex(rec["data_hex"])
        return frame_from_payload(data, type_byte=43, seq=0, cmd=int(rec.get("cmd", 0))).hex()
    return None


def load_frames(path):
    """Return [{seq, hex}] for every decodable record in the capture."""
    out = []
    for rec in _iter_records(path):
        hexs = _record_to_hex(rec)
        if hexs is not None:
            out.append({"seq": rec.get("seq", 0), "hex": hexs})
    return out


def _device_ts_from_frame(frame: bytes):
    """Best-effort device timestamp: REALTIME_DATA(40) ts@6, type-43 ts@11."""
    if len(frame) < 6 or frame[0] != 0xAA:
        return None
    t = frame[4]
    if t == 40 and len(frame) >= 10:
        return struct.unpack("<L", frame[6:10])[0]
    if t in (43, 47) and len(frame) >= 15:
        return struct.unpack("<L", frame[11:15])[0]
    return None


def derive_clock_ref(path):
    """Return {device, wall} anchoring device-epoch to wall-clock unix seconds."""
    wall = None
    device = None
    for rec in _iter_records(path):
        if wall is None and "wall" in rec:
            wall = int(rec["wall"])
            hexs = _record_to_hex(rec)
            if hexs:
                device = _device_ts_from_frame(bytes.fromhex(hexs))
            if device is not None:
                break
        if device is None:
            hexs = _record_to_hex(rec)
            if hexs:
                device = _device_ts_from_frame(bytes.fromhex(hexs))
    if wall is None:
        wall = int(os.path.getmtime(path))
    if device is None:
        device = 0
    return {"device": device, "wall": wall}


def chunk(items, size):
    for i in range(0, len(items), size):
        yield items[i:i + size]


def post_batch(url, token, batch):
    req = urllib.request.Request(
        url.rstrip("/") + "/v1/ingest",
        data=json.dumps(batch).encode(),
        headers={"Content-Type": "application/json", "Authorization": f"Bearer {token}"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=30) as resp:
        return json.loads(resp.read())


def main(argv=None):
    ap = argparse.ArgumentParser(description="Replay a WHOOP capture into the ingest service.")
    ap.add_argument("capture")
    ap.add_argument("--url", default="http://localhost:8770")
    ap.add_argument("--token", required=True)
    ap.add_argument("--device", default="whoop4-dev")
    ap.add_argument("--batch-size", type=int, default=100)
    args = ap.parse_args(argv)

    frames = load_frames(args.capture)
    clock_ref = derive_clock_ref(args.capture)
    print(f"{len(frames)} frames; clock_ref={clock_ref}")
    totals = {"hr": 0, "rr": 0, "events": 0, "battery": 0}
    for group in chunk(frames, args.batch_size):
        batch = {
            "batch_id": str(uuid.uuid4()),
            "device": {"device_id": args.device},
            "clock_ref": clock_ref,
            "frames": group,
        }
        res = post_batch(args.url, args.token, batch)
        for k in totals:
            totals[k] += res.get("decoded_counts", {}).get(k, 0)
        print(f"  batch {res['batch_id'][:8]} deduped={res.get('deduped')} {res.get('decoded_counts')}")
    print(f"done. decoded totals: {totals}")


if __name__ == "__main__":
    main()
