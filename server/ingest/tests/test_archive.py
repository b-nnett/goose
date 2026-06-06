import hashlib
import zstandard
from app.archive import write_raw_batch


def test_write_raw_batch_roundtrip(tmp_path):
    frames = [bytes.fromhex("aa1800ff2802"), bytes.fromhex("aa10005730262e")]
    res = write_raw_batch(str(tmp_path), device_id="devA", batch_id="b1",
                           day="2026-05-23", frames=frames)
    # file exists under <root>/<device>/<day>/<batch>.zst
    assert res["file_path"].endswith("devA/2026-05-23/b1.zst")
    import os
    assert os.path.exists(res["file_path"])
    # sha256 + byte_size describe the COMPRESSED file
    raw = open(res["file_path"], "rb").read()
    assert res["sha256"] == hashlib.sha256(raw).hexdigest()
    assert res["byte_size"] == len(raw)
    # decompresses to newline-delimited hex of the original frames
    text = zstandard.ZstdDecompressor().decompress(raw).decode()
    assert text.splitlines() == [f.hex() for f in frames]


def test_write_raw_batch_packet_count(tmp_path):
    res = write_raw_batch(str(tmp_path), "devA", "b2", "2026-05-23",
                          [b"\xaa\x00", b"\xaa\x01", b"\xaa\x02"])
    assert res["packet_count"] == 3
