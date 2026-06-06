from whoop_protocol.framing import crc8, crc32, verify_frame, Reassembler
from tests.fixtures import frames


def test_crc8_over_length_bytes():
    # crc8 is computed over the 2 length bytes (frame[1:3]) and stored at frame[3].
    frame = frames.REALTIME_DATA_HR60
    assert crc8(frame[1:3]) == frame[3]


def test_crc32_over_inner_packet():
    frame = frames.REALTIME_DATA_HR60
    length = int.from_bytes(frame[1:3], "little")
    inner = frame[4:length]
    want = int.from_bytes(frame[length:length + 4], "little")
    assert crc32(inner) == want


def test_verify_frame_ok():
    v = verify_frame(frames.REALTIME_DATA_HR60)
    assert v.ok is True
    assert v.length == 24
    assert v.crc8_ok is True
    assert v.crc32_ok is True


def test_verify_frame_rejects_non_sof():
    v = verify_frame(b"\x00\x01\x02\x03\x04\x05\x06\x07")
    assert v.ok is False


def test_verify_frame_detects_crc32_mismatch():
    bad = bytearray(frames.REALTIME_DATA_HR60)
    bad[-1] ^= 0xFF  # corrupt the crc32 trailer
    v = verify_frame(bytes(bad))
    assert v.crc32_ok is False


def test_reassembler_joins_fragments():
    whole = frames.REALTIME_DATA_HR60
    r = Reassembler()
    out = []
    for i in range(0, len(whole), 10):
        out.extend(r.feed(whole[i:i + 10]))
    assert out == [whole]


def test_reassembler_handles_back_to_back_frames():
    whole = frames.REALTIME_DATA_HR60
    r = Reassembler()
    out = r.feed(whole) + r.feed(whole)
    assert out == [whole, whole]


from whoop_protocol.framing import frame_from_payload, verify_frame


def test_frame_from_payload_builds_valid_envelope():
    data = bytes(range(20))  # 20-byte fake payload
    frame = frame_from_payload(data, type_byte=43, seq=0, cmd=41)
    # data sits at offset 7; length field = len(inner)+4 = (3+20)+4 = 27
    assert frame[0] == 0xAA
    assert int.from_bytes(frame[1:3], "little") == 27
    assert frame[4] == 43 and frame[6] == 41
    assert frame[7:27] == data
    # crc32 is valid (crc8 is a placeholder, so verify_frame.crc8_ok is False)
    chk = verify_frame(frame)
    assert chk.crc32_ok is True
