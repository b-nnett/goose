import json
import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
import uploader


def test_load_frames_full_hex(tmp_path):
    p = tmp_path / "cap.jsonl"
    p.write_text(json.dumps({"type": "REALTIME_DATA", "seq": 2,
        "full_hex": "aa1800ff28020f3de10128663c00000000000000000001010d844e7c"}) + "\n")
    frames = uploader.load_frames(str(p))
    assert len(frames) == 1
    assert frames[0]["hex"].startswith("aa1800ff")


def test_load_frames_reframes_payload(tmp_path):
    # datalen + data_hex (bare payload) must be reframed into a full frame
    data = "00" * 30
    p = tmp_path / "m.jsonl"
    p.write_text(json.dumps({"wall": 1779570923.6, "cmd": 41, "datalen": 30, "data_hex": data}) + "\n")
    frames = uploader.load_frames(str(p))
    assert len(frames) == 1
    fr = bytes.fromhex(frames[0]["hex"])
    assert fr[0] == 0xAA and fr[4] == 43  # reframed as type-43


def test_derive_clock_ref_prefers_record_wall(tmp_path):
    p = tmp_path / "m.jsonl"
    # a record with a wall clock + a type-43 payload carrying device ts at data[4:8]
    # device ts bytes (LE) for 31538885 = c53ee101
    data = "7c953102" + "c53ee101" + "00" * 22
    p.write_text(json.dumps({"wall": 1779570923.0, "cmd": 41, "datalen": 30, "data_hex": data}) + "\n")
    ref = uploader.derive_clock_ref(str(p))
    assert ref["wall"] == 1779570923
    assert ref["device"] == 31538885


def test_chunk_batches():
    frames = [{"seq": i, "hex": "aa00"} for i in range(250)]
    batches = list(uploader.chunk(frames, size=100))
    assert [len(b) for b in batches] == [100, 100, 50]
