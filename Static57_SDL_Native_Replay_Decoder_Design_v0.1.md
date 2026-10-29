# STATIC 57 — SIMULATION DYNAMICS LABORATORY (SDL)
## S57-003 — Native Replay Decoder Design Specification v0.1
Date: 2026-10-29
Status: DRAFT ARCHITECTURE / NOT YET IMPLEMENTED OR VALIDATED AGAINST USER REPLAY
Owner: Static 57 — Simulation Dynamics Laboratory (SDL)
Roles: Owner directs testing and approvals; ChatGPT researches/specifies; Claude Code implements when authorized.

## 1. Purpose and first proof
Build an independent, Static 57-owned War Thunder `.wrpl` decoder, not a dependency on a third-party replay parser. The first proven outcome is **a validated timeline of packet types and timestamps from an actual current-version client replay**, followed by a timestamped flight trajectory. Preserve raw evidence so formats can be reinterpreted later.

No visual replay UI, data-science model, or weapons-diagnostics claim is a prerequisite for the initial parser. The native decoder is the foundation of S57-003 SDL.

## 2. Evidence baseline / confidence
The following are **documented in third-party code and sample analyses**, not yet validated on a Static 57-provided replay.

- A common `.wrpl` container header is 1,234 bytes, starting with four bytes `E5 AC 00 10` (little-endian integer `0x1000ACE5`). The next 32-bit field at offset 4 is a format/version magic.
- Format `0x00018C0B` is documented in existing parsers; October 2026 game 2.59 examples report `0x00018C1C` and a Zstandard-compressed packet stream in place of earlier zlib. Treat neither as guaranteed for later files.
- Representative header offsets: footer offset 684 (`u32`), session ID 732 (`u64`), replay part number 740 (`u8`), settings size 748 (`u16`). Validate against actual files, record file magic, and refuse nonsensical section offsets.
- Data sections: header; optional settings DataBlock; compressed packet stream; optional result/footer DataBlock.
- Decompressed packets have a variable-length size field followed by a packet body. Body begins with a 16-bit packet type. Packet timestamps can be explicit 32-bit milliseconds or inherited from the previous packet when flag bit `0x10` is set.
- Published packet classes include 0 end, 1 start, 2 aircraft, 3 chat, 4 MPI, 5 next segment, 6 ECS, 7 snapshot, 8 header info.
- Server replays can be segmented. Segments have their own compressed frames and timestamp state; never concatenate the compressed bytes and decode as one stream.
- October 2026 client examples have Zstd frames without a known decompressed content size: use streaming decompression rather than a guessed fixed-size buffer.

References:
1. https://furro404.github.io/WT_KNOWLEDGE_DOCS/03-replays/
2. https://github.com/LivingTheDagor/WrplReplayParser
3. https://github.com/maxsupermanhd/wrpl-inspector

## 3. Product/ownership boundary
Static 57 owns implementation of file parsing, replay version registry, packet framing, ECS/MPI/flight sync decoding, evidence archive, entity/time reconstruction, data schema, analyses, and software UI. General purpose libraries for Zstd, zlib, hashing, storage, graphical rendering, and math are allowed as foundational dependencies. Do not import third-party replay decoding engines.

## 4. Proposed architecture
Prefer a small native **Rust decoder core** (proposal, not approved technology lock-in) with a command-line interface and a version-independent output contract; Electron/TypeScript application can call the tool later. Advantages: checked binary reads, controlled memory use, clean separation from desktop UI, portability. Alternative: TypeScript/Node if development integration outweighs native binary advantages. Benchmark before a final choice.

Pipeline:
```
WRPL bytes
  -> Container / header probe
  -> Sections and compression detection
  -> Streaming zlib or zstd inflater
  -> Raw packet framer + clock reconstruction
  -> Version-specific payload decoders (aircraft, ECS, MPI, weapons, sensors)
  -> Entity/flight track reconstruction
  -> Evidence-rich normalized timeline
  -> Future analytics / reports / 3D visualization
```

Module boundaries:
- `container`: verify signatures, parse fixed header and section offsets; identify client/server/parts without guessing.
- `compression`: choose codec based on inspected magic and version policy; bounded streaming decode; protect against malformed files and decompression bombs.
- `packets`: size-prefix decoder, type flags, timestamps, packet offsets, payload byte ranges; retain unknown packets.
- `versions`: explicit known/research/unknown profiles keyed by replay magic, game build, observed stream behavior; never silently interpret new layouts as old.
- `datablock`: independent future BLK settings/results reader; keep opaque bytes during first milestone.
- `aircraft`: flight-state bit reader, quantized values, trajectories, sensor extensions (later milestone).
- `entities`: persistent identity across entity handles, spawns, unit ID reuse, player and equipment references.
- `weapons`: launch/entity/trajectory/outcome matching; preserve match confidence and unknowns.
- `export`: JSON Lines events and machine-readable manifest; provenance required on every derived record.
- `diagnostics`: format errors with section, byte offset, packet index, version, and reason.

## 5. Proposed initial output contract
`manifest.json`:
- input filename (sanitized), SHA-256, file size, capture version/magic
- game version (raw plus interpreted if known), session ID, session part number, optional map/mode
- client/server classification and confidence
- settings/stream/footer offsets and sizes
- stream codec, decoder version, parse status, packet count, unknown-type tally, warnings

`packets.jsonl` (one record per parsed packet):
- packet index; segment index; compressed/uncompressed stream-relative offset when available
- packet type numeric; decoded kind/name where established
- `timestamp_ms` and `timestamp_source` (`explicit` or `inherited`)
- payload length; optional raw payload reference/hash
- decode state (`raw`, `partial`, `decoded`, `error`)

Later `tracks.jsonl`: entity handle and spawn instance; aircraft/weapon classification confidence; time; position and orientation in native coordinate frame; transform provenance; unit semantics; source packet reference.

Keep decoded and inferred values separate. NEVER fabricate a missing field from interpolation without marking the origin as derived.

## 6. First development milestones and acceptance gates
### M0 — Evidence fixtures
Acquire a local current-version `.wrpl` client file from a short custom battle, plus optionally matching server segments; retain raw immutable copies and record War Thunder build and replay SHA-256. Construct synthetic valid/malformed header and packet framing fixtures for deterministic tests.

### M1 — Header and container inspector
Read the file signature, version magic, session ID, part number, settings size, stream boundaries, and footer presence. Emit `manifest.json`. Verify against a manual hex inspection. Reject short files, overflowing offsets and wrong signatures with meaningful errors. **No flight decoding required.**

### M2 — Streaming packet extractor
Support codec probing (zlib/zstd), streaming bounded inflation, packet-size variants, timestamp inheritance and raw packet preservation. Emit `packets.jsonl` with a packet-type histogram and monotonic/clock diagnostic report. Prove all bytes consumed or explicitly account for padding, terminal marker and trailing content. Test malformed and truncated files without crashes.

### M3 — Real aircraft track
Decode flight-model sync for at least one known replay format. Extract timestamped ownship positions and orientation, identify gaps, compare representative positions to independent 8111 capture (when available). Determine coordinate conventions empirically. Do not claim sensor/weapon support at this gate.

### M4 — Multi-part and entity identity
Associate server segments by session, verify numbering and continuity, decode selected ECS/MPI entity identity messages, handle respawn/UID reuse and client/server differences.

### M5 — Weapons/sensors
Add narrowly scoped evidence-backed decoders one family at a time; start with AIM-7 and a simple unguided bomb, or another pairing supported by fixtures. Include sample-level confidence and known-version compatibility. Weapon launch/outcome association must be audited against in-game events.

### M6 — 8111 fusion and analyses
Align replay time and recorded telemetry via common markers, record synchronization uncertainty, and produce calibrated metrics only after validation.

## 7. Validation strategy
- Fixture tests for versioned header parsing, truncation, wrong endian, absurd section bounds, size-prefix encodings and malformed timestamp flags.
- Round-trip synthetic packet framing tests; independent expected packet counts and timestamp results.
- Known replay regression fixtures from different game versions; no automatic upgrade of unknown format versions.
- For segmentation: reject conflicting session IDs, missing parts and out-of-order continuity; independently inflate each part.
- Performance tests for multi-hour replay and a deliberately oversized compressed stream.
- Accurate packet provenance, no dropped unknown records, no silent fallback or silent truncation.
- Manually check the first aircraft track against replay-viewer observations / 8111 where synchronized.

## 8. Live data needed for full validation
One short custom Air Battle recording in the current War Thunder build, ideally generated by the owner on their game installation; optional simultaneous 8111 capture and matching server replay. **These data have not been provided or processed.** A synthetic fixture can verify reader behavior but cannot prove compatibility with actual War Thunder recordings.

## 9. Decisions to review before implementation
1. Rust core versus TypeScript/Node core; Rust is the initial technical recommendation, not a signed decision.
2. Prioritize client replay M1–M3 before server segments and weapons decoding.
3. Preserve unknown raw packets and payloads by design rather than require complete decoding.
4. Maintain compatibility profiles and immutable replay fixtures for every supported version.
5. Keep the decoder separate from replay visualization and Static 57's existing three JSON databases.

## 10. Explicit nonclaims
- No Static 57 `.wrpl` decoder has yet been implemented or flight-tested.
- Current compatibility is unknown until a sample from the owner's version is inspected.
- A parsed packet stream does not imply missile seeker state decoding.
- A full weapons failure explanation is not guaranteed by any one replay record.
