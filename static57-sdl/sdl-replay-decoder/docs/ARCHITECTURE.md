# S57-003 SDL — Native Replay Decoder Architecture

## Pipeline (implemented, Milestones 1–4)

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
  -> export       manifest.json, packets.jsonl

  Milestone 3, layered on PacketRecord + payload bytes:
  type 2 payload -> bitstream -> aircraft (structure) -> flight (physical meaning)
                                                      -> tracks (segments, author, validation)
  type 4 0xD04A  -> cockpit (validation reference only) -----^
                                                      -> export: flight_tracks.jsonl, flight_summary.json

  Milestone 4, one read of the replay (lab.rs):
  type 6 ECS creation -> ecs -> units (identity), weapons (launch)
  type 4 0xF0DB       -> shells -> weapons (trajectory)
  type 4 0xF058/56/157 -> idfield -> combat (battle log) -> weapons (kill link, name)
  all of the above -> measure -> research (ResearchData) -> report (Markdown)
                                                         -> strikemaster (proposals)
  many ResearchData -> survey (CrossReplay) -> report (research-summary.md)
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
| `bitstream` | `BitReader` for Dagor bitstreams: MSB-first bits, unaligned bytes, LE integers and floats, varints, zigzag, alignment. Every read is bounds-checked and errors carry the bit offset |
| `aircraft` | Structural decoder for the type-2 sync: uid sequence, no-data/withheld entries, the full state record including the optional gun, weapon, engine, sensor, countermeasure, target and seeker blocks. Assigns no physical meaning. Unknown fields are kept raw, with bit offsets |
| `flight` | Physical interpretation, each field independently validated: packed orientation (heading/pitch/bank), packed velocity (speed + unit direction), conventions, quantization constants, ISA helper |
| `cockpit` | Minimal MPI `0xD04A` decoder, used only as an independent reference |
| `tracks` | `Reconstructor`: provisional track segments per uid (split on gap or unexplained jump, never merged), author identification via the sync trailer, sample flags, kinematic and cockpit validation statistics, bit-coverage accounting |
| `ecs` | ECS creation framing: entity id, template name, raw component block. Component values are not decoded |
| `units` | Vehicle, slot and loadout per uid and per entity from creation records, resolved at a time, for aircraft, ground units and ships |
| `idfield`, `combat` | Id-field message tables; kill, damage and hit records |
| `shells` | MPI `0xF0DB` weapon records, accepted only when they consume exactly the declared length |
| `seeker` | Seeker blocks carried by `ShellsData`: target estimate and line of sight. The range field is rejected |
| `weapons` | Joins creation, flight records, aircraft state, seeker data and battle-log records into `WeaponEvent`s. The launcher is VERIFIED from the creation record. Target, name and outcome are DERIVED or UNKNOWN with explicit status. SDL does not classify misses. Names come from kill records or a per-replay type id, with a built-in hold-out test |
| `systems` | Author samples with their cockpit message; control, engine and fuel candidates; load factor from position. `CANDIDATES` holds each field's evidence |
| `telemetry` | Read-only 8111 recorder and capture loader |
| `sync` | Capture-to-replay time alignment by cockpit clock and motion, with cross-checks |
| `validate` | Replay association, flight-state comparison, altimeter units, map fit, field scan |
| `simcapture` | Labelled synthetic captures for software tests |
| `lab` | One extraction pass over a replay |
| `measure` | Pure measurement functions: statistics, maneuver figures, transitions, gaps, robust z-score |
| `research` | Per-replay `ResearchData`: tracks, identity, anomalies, missing data, findings, questions, coverage |
| `strikemaster` | Read-only database link and proposals for review |
| `survey` | Cross-replay analysis |
| `report` | Markdown from `ResearchData` and `CrossReplay` only, so reports cannot disagree with the JSON |
| `export` | JSON writers (`export_packets`, `analyze_flight`, `export_flight`, `export_research`). Refuse to overwrite the input |

### Design rules

- **Codec is probed, not assumed.** A mismatch with the version profile is warned, then decoding proceeds with the probed codec.
- **A new format magic is never treated as a known layout.** Its fields are downgraded to EXPERIMENTAL.
- **Separate decompression per segment.** The framer resets its running clock per call. Server parts must be decoded one at a time (`segment` = part number); compressed bytes are never concatenated across files.
- **Memory.** The replay file is held in memory (the largest local file is 42 MB). The decompressed stream is processed as a stream, and only the current packet body is buffered. Flight reconstruction keeps one last-sample state per uid, plus author points and cockpit messages for validation.
- **Structure before meaning.** `aircraft` proves record boundaries, and `flight` assigns units only to fields validated against independent evidence. Everything else is exported raw or counted as structural-only bits.

## Output contract

`manifest.json` (schema `static57.sdl.replay-manifest/0.1`) contains: input name, size and SHA-256; format magic and profile; session values, each with its evidence class; recording classification; section layout with a SHA-256 per section; BLK summaries; codec and frame table; packet summary (type histogram, prefix forms, clock report, byte accounting); all 41 header fields with raw hex; the full raw header; diagnostics.

`packets.jsonl` has one record per line: `index`, `segment`, `record` (`packet` / `zero_size` / `malformed` / `truncated`), `offset` (decompressed stream offset of the prefix), `prefix_len`, `prefix_hex`, `size`, `type_raw`, `type_id`, `kind`, `kind_evidence`, `timestamp_ms`, `timestamp_source` (`explicit` / `inherited` / `inherited_initial`), `payload_offset`, `payload_length`, `payload_sha256`, optional `payload_hex`, and `decode_state` (`raw` / `error`).

`flight_tracks.jsonl` has one aircraft state sample per line:

| Field | Content |
|---|---|
| `replay_sha256`, `format_magic`, `game_version` | Source replay identity |
| `packet_index`, `entry_index`, `timestamp_ms` | Provenance: matches `packets.jsonl` `index`. `timestamp_ms` is session time |
| `track_id`, `uid`, `segment`, `is_author` | Provisional identity (`u<uid>.s<segment>`) |
| `position` `{x_m, y_m, z_m}`, `altitude_msl_m` | World metres, +Y up |
| `orientation` `{heading_deg, pitch_deg, bank_deg}` | Conventions in `FORMAT_FINDINGS.md` §5.3 |
| `velocity` `{vx_mps, vy_mps, vz_mps, speed_mps, saturated, clamped, flag_bit18}` | World frame |
| `derived` `{groundspeed_mps, vertical_speed_mps, track_deg, flight_path_deg, fd_speed_mps}` | DERIVED. Unavailable values are `null`, for example `fd_speed_mps` on a segment's first sample |
| `evidence` | Class per field group |
| `flags` | `segment_start`, `position_jump`, `speed_saturated`, `direction_clamped`, `repeated_timestamp`, `timestamp_reversal`, `non_finite_position` |
| `raw` | Packed orientation and velocity words, ground flag, unknown u32 and bit, control bytes, engine states, bit offsets of the record and of the position |

`flight_summary.json` (schema `static57.sdl.flight-tracks/0.1`) contains the configuration, bit coverage, track count, segment-start reasons, kinematic validation, cockpit validation and one summary per track.

64-bit values such as `session_id` are emitted as exact JSON integers. JavaScript consumers should use the matching `*_hex` string.

## Future module boundaries (not implemented)

Each future module consumes `PacketRecord` plus its payload bytes, never the compressed file. Every output record must cite its source `(segment, packet index, payload offset)` and an evidence class.

| Module | Consumes | Produces |
|---|---|---|
| `ground` (Battlefield Dynamics) | MPI `0xF073`, ECS components | Ground, naval and SAM unit positions and engagement behavior |
| `sensors` (Sensor Dynamics) | Type-2 sensor/target/seeker blocks (already delimited by `aircraft`), ECS | Radar and seeker state |
| `blk` | Settings and footer sections | Decoded DataBlock trees |
| `segments` | Multiple server `.wrpl` parts | Session assembly with continuity checks |
| `telemetry` (Experimental Telemetry) | SDL tracks plus 8111/HOTAS/MARK/TEST WPN logs | Time alignment with recorded uncertainty (plan: `TELEMETRY_8111.md`) |
