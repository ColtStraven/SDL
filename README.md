# Static 57 — Simulation Dynamics Laboratory (SDL)

**Project S57-003** · Static 57 — Experimental Avionics & Simulation Technologies

SDL is a simulation research environment for flight dynamics, weapons testing, replay forensics, avionics calibration and simulation analytics. The first supported platform is **War Thunder Air Simulator Battles**.

Its foundation is a native `.wrpl` replay decoder, written in Rust and owned by Static 57. It does not depend on any third-party replay parser.

## Status

| Milestone | Scope | State |
|---|---|---|
| M1 — Replay file inspector | Signature, version registry, 1234-byte header, sections, codec detection, JSON manifest | **Done** |
| M2 — Packet extractor | Bounded zstd/zlib streaming, packet framing, timestamps, raw payload preservation, JSONL export | **Done** |
| M3 — Aircraft track | Decode `aircraft_small` flight-model sync; validate against localhost:8111 | **Started**: author position track + cockpit instruments, altimeter-verified on 1 replay; other aircraft not yet |
| M4+ | Entities, server segments, weapons, sensors, telemetry fusion | Not started |

The decoder is validated on **28 genuine War Thunder 2.59 client replays** (format `0x00018C1C`, Zstandard), with 4,303,777 packets framed and every byte accounted for. See [COMPATIBILITY.md](static57-sdl/sdl-replay-decoder/docs/COMPATIBILITY.md).

Payload decoding has started with the replay author's own aircraft: its world position track, cockpit instruments, firing events and kill/damage records. Other aircraft, sensors and missiles are **not decoded yet**. See [FORMAT_FINDINGS §4](static57-sdl/sdl-replay-decoder/docs/FORMAT_FINDINGS.md).

## Repository layout

```
Static57_SDL_Native_Replay_Decoder_Design_v0.1.md   design specification v0.1
static57-sdl/
  sdl-replay-decoder/                               native Rust decoder (CLI + library)
    src/          container, versions, compression, packets, datablock, manifest, export
    tests/        synthetic fixtures + genuine-replay compatibility test
    docs/         ARCHITECTURE, FORMAT_FINDINGS, COMPATIBILITY
```

## Quick start

```
cd static57-sdl/sdl-replay-decoder
cargo build --release
./target/release/sdl-replay-decoder inspect  path/to/replay.wrpl
./target/release/sdl-replay-decoder packets  path/to/replay.wrpl   # writes sdl-out/<name>/manifest.json + packets.jsonl
./target/release/sdl-replay-decoder flight   path/to/replay.wrpl   # author track, cockpit, weapon events
./target/release/sdl-replay-decoder survey   path/to/Replays/
cargo test --release -- --include-ignored                           # includes genuine replays (SDL_REPLAY_DIR)
```

The full CLI reference is in the [decoder README](static57-sdl/sdl-replay-decoder/README.md).

## Scientific data integrity

Every reported value carries an evidence class:

- **VERIFIED**: demonstrated through documented structure plus a reproducible cross-check.
- **REPORTED**: documented by external researchers, not yet confirmed by Static 57.
- **DERIVED**: calculated from measurements.
- **EXPERIMENTAL**: a plausible interpretation awaiting validation.
- **UNKNOWN**: not established.

Original replay files are only ever read, and they are not stored in this repository. Unknown structures are preserved, never silently dropped. Passing synthetic fixtures alone is never treated as proof of War Thunder compatibility.

## Documentation

- [Design specification v0.1](Static57_SDL_Native_Replay_Decoder_Design_v0.1.md)
- [Architecture](static57-sdl/sdl-replay-decoder/docs/ARCHITECTURE.md)
- [Format findings](static57-sdl/sdl-replay-decoder/docs/FORMAT_FINDINGS.md)
- [Compatibility record](static57-sdl/sdl-replay-decoder/docs/COMPATIBILITY.md)
- [Replay reports](reports/README.md): one analysis report per genuine replay

## Roles

- **Static 57 owner:** research direction, flight testing, approvals.
- **ChatGPT:** format research, specifications, review.
- **Claude Code:** implementation, testing, documentation.
