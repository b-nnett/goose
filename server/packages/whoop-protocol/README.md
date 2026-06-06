# whoop-protocol

Shared decoder for WHOOP 4.0 BLE frames. One implementation of decode truth:

- `whoop_protocol/schema/whoop_protocol.json` — declarative field layout + enums.
  **Edit this to improve decode** (offsets, scales, new fields). Both this Python
  interpreter and the future Swift interpreter read it, so layout never drifts.
- `framing.py` — CRC8 (poly 0x07, over the 2 length bytes), zlib CRC32 (over the
  inner packet), and BLE fragment reassembly. Stable; re-implemented per language.
- `interpreter.py` — `parse_frame(frame) -> {ok, type_name, seq, fields[], parsed{}, crc_ok}`
  (a generic schema walk + small per-type post-hooks for irregular fields) and
  `extract_streams(parsed_results, device_clock_ref, wall_clock_ref) -> {hr, rr, events, battery}`
  rows (wall-clock timestamps) for the datastore.

Ported from `~/Developer/whoop/dashboard/whoop_fields.py`. See
`home-server/docs/specs/2026-05-23-whoop-backbone-datastore-design.md`.

## Use

    pip install -e packages/whoop-protocol[dev]
    pytest packages/whoop-protocol
