# S57-003 SDL — Native Replay Decoder Architecture

## Pipeline (implemented: Milestones 1–2, Milestone 3 first stage)

```
.wrpl bytes (read-only)
  -> container    signature, 1234-byte header table, section layout and bounds
  -> versions     explicit format-magic registry (18c0a, 18c0b, 18c1c)
  -> compression  codec probe (zstd/zlib magic), bounded streaming inflation,
                  per-frame compressed/decompressed spans, trailing-byte accounting
  -> packets      size-prefix framer, type + inherit-time flag, clock reconstruction,
                  raw payload preservation, statistics
  -> manifest     evidence-classified report; promotes fields to VERIFIED only
                  after structural cross-checks
  -> flight/mpi   ownship sync record (type 2), cockpit instruments (MPI 0xD04A),
                  combat records (MPI 0xF058/0xF056/0xF118); altimeter cross-check
  -> export       manifest.json, packets.jsonl; flight_summary.json, ownship.jsonl,
                  cockpit.jsonl, weapons.jsonl
```

| Module | Responsibility |
|---|---|
| `evidence` | Classifications `VERIFIED` / `REPORTED` / `DERIVED` / `EXPERIMENTAL` / `UNKNOWN`, plus hashing helpers |
| `diagnostics` | Structured info/warning/error records: code, section, byte offset, message |
| `versions` | Format profiles keyed by the magic at offset 4, with expected codec and evidence status. Unknown magics are inspected best-effort and flagged |
| `container` | Header field table (tiles 0..1234 exactly, enforced by a test), raw bytes for every field, string residue, section layout, client/server heuristics |
| `compression` | `Inflater`, a `Read` adapter over the compressed section. Handles multiple zstd frames, skippable frames and zlib members. Enforces an output ceiling. Every compressed byte is classified as decoded or trailing |
| `packets` | `frame_packets` streams `PacketRecord`s into a sink. Never drops records: zero-size, malformed and truncated records are emitted with their bytes |
| `datablock` | BLK sections stay opaque (length, hash, leading byte, printable-string hints). A native BLK reader is a future module |
| `manifest` | `analyze()` runs the pipeline for `inspect` or `packets` depth |
| `bits` | `BitReader` / `BitWriter`: MSB-first bits, little-endian multi-byte values (the convention of packed payloads) |
| `mpi` | MPI signatures; `parse_cockpit` (0xD04A, length-checked), `parse_combat` (kill / damage / hit-marker records, EXPERIMENTAL) |
| `flight` | `parse_ownship_sync` for the leading `aircraft_small` record; `decode()` assembles ownship samples, cockpit records and weapon events, and cross-checks height against the altimeter. A position sample is VERIFIED only when that check passes |
| `export` | JSON writers. Refuses to overwrite the input |

### Design rules

- **Codec is probed, not assumed.** A mismatch with the version profile is warned, then decoding proceeds with the probed codec.
- **A new format magic is never treated as a known layout.** Its fields are downgraded to EXPERIMENTAL.
- **Separate decompression per segment.** The framer resets its running clock per call. Server parts must be decoded one at a time (`segment` = part number); compressed bytes are never concatenated across files.
- **Memory.** The replay file is held in memory (the largest local file is 42 MB). The decompressed stream is processed as a stream, and only the current packet body is buffered.

## Output contract

`manifest.json` (schema `static57.sdl.replay-manifest/0.1`) contains: input name, size and SHA-256; format magic and profile; session values, each with its evidence class; recording classification; section layout with a SHA-256 per section; BLK summaries; codec and frame table; packet summary (type histogram, prefix forms, clock report, byte accounting); all 41 header fields with raw hex; the full raw header; diagnostics.

`packets.jsonl` has one record per line: `index`, `segment`, `record` (`packet` / `zero_size` / `malformed` / `truncated`), `offset` (decompressed stream offset of the prefix), `prefix_len`, `prefix_hex`, `size`, `type_raw`, `type_id`, `kind`, `kind_evidence`, `timestamp_ms`, `timestamp_source` (`explicit` / `inherited` / `inherited_initial`), `payload_offset`, `payload_length`, `payload_sha256`, optional `payload_hex`, and `decode_state` (`raw` / `error`).

64-bit values such as `session_id` are emitted as exact JSON integers. JavaScript consumers should use the matching `*_hex` string.

The `flight` command adds (schema `static57.sdl.flight-summary/0.2`; 0.2 renamed the airspeed fields to IAS after the 8111 validation):

- `flight_summary.json`: sync tally (decoded / no ownship record / unexpected layout / truncated), cockpit message count and errors, parameter ids, the altimeter cross-check, airborne interval, counter range, combat record count.
- `ownship.jsonl`: per `aircraft_small` packet with an ownship record: `packet_index`, `timestamp_ms`, `on_ground`, `position [x, y, z]`, `position_evidence` (`VERIFIED` when the height check passes, else `EXPERIMENTAL`), `firing`, `triggers`, `counter`, `altimeter_delta_m`, `derived_speed_m_s`, `trailer_matches`, `segment`, `payload_offset`.
- `cockpit.jsonl`: per `0xD04A` message: `packet_index`, `timestamp_ms`, `params [{id, name?, value}]`, `declared_bits`, `segment`, `payload_offset`.
- `weapons.jsonl`: time-ordered events `trigger`, `counter_step` (with `counter_from` / `counter_to`), `kill`, `damage`, `hit_marker` (with the raw record), each EXPERIMENTAL.

## Future module boundaries (not implemented)

`flight` below now exists for the replay author only; other aircraft, attitude and velocity remain future work.

Each future module consumes `PacketRecord` plus its payload bytes, never the compressed file. Every output record must cite its source `(segment, packet index, payload offset)` and an evidence class.

| Module | Consumes | Produces |
|---|---|---|
| `flight` (Flight Dynamics) | type 2 `aircraft_small` payloads; MPI/ECS for identity | Ownship and other aircraft position, attitude, velocity, configuration, in the native coordinate frame |
| `entities` (Battlefield Dynamics) | ECS (6), MPI (4) | Persistent entity identity, spawns, UID reuse, AI, ground, naval and SAM units |
| `weapons` (Weapons Dynamics) | MPI/ECS weapon messages, entity tracks | Launch, trajectory and outcome records with match confidence |
| `sensors` (Sensor Dynamics) | Aircraft sync extensions, ECS | Radar and seeker state |
| `blk` | Settings and footer sections | Decoded DataBlock trees |
| `segments` | Multiple server `.wrpl` parts | Session assembly with continuity checks |
| `telemetry` (Experimental Telemetry) | SDL tracks plus 8111/HOTAS/MARK/TEST WPN logs | Time alignment with recorded uncertainty |
| `analysis` / `visualization` | The above | Kept out of the decoder crate |
