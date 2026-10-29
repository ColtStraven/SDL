# Static 57 — Simulation Dynamics Laboratory (SDL)

**Project S57-003** · Static 57 — Experimental Avionics & Simulation Technologies

SDL is a simulation research environment for flight dynamics, weapons testing, replay forensics, avionics calibration and simulation analytics. The first supported platform is **War Thunder Air Simulator Battles**.

Its foundation is a native `.wrpl` replay decoder, written in Rust and owned by Static 57. It does not depend on any third-party replay parser.

## Status

| Milestone | Scope | State |
|---|---|---|
| M1 — Replay file inspector | Signature, version registry, 1234-byte header, sections, codec detection, JSON manifest | **Done** |
| M2 — Packet extractor | Bounded zstd/zlib streaming, packet framing, timestamps, raw payload preservation, JSONL export | **Done** |
| M3 — Aircraft flight state | Native type-2 decoder: position, altitude, attitude, velocity; provisional tracks; `flight_tracks.jsonl` | **Done** (branch `milestone-3`). 8111 validation prepared, not yet run |
| M4+ | Entities and identity, server segments, weapons, sensors, telemetry fusion | Not started |

The decoder is validated on **27 genuine War Thunder 2.59 client replays** (format `0x00018C1C`, Zstandard), with 4,086,169 packets framed and every byte accounted for. All 103,698 aircraft sync packets decode, giving 2,468,023 timestamped 3D aircraft samples in 2,460 provisional track segments. The recording author's decoded altitude, attitude and vertical speed agree with the replay's own cockpit instruments in 19 replays: median errors of 0.000–0.65 m, 0.002–0.24° roll and 0.02–0.08° pitch. See [COMPATIBILITY.md](static57-sdl/sdl-replay-decoder/docs/COMPATIBILITY.md) and [FORMAT_FINDINGS.md](static57-sdl/sdl-replay-decoder/docs/FORMAT_FINDINGS.md).

Weapons, sensors and ECS payloads are **not decoded yet**. Aircraft-to-vehicle identity and the map-north orientation of the horizontal axes are unresolved.

## Repository layout

```
Static57_SDL_Native_Replay_Decoder_Design_v0.1.md   design specification v0.1
static57-sdl/
  sdl-replay-decoder/                               native Rust decoder (CLI + library)
    src/          container, versions, compression, packets, datablock, manifest, export,
                  bitstream, aircraft, flight, cockpit, tracks
    tests/        synthetic fixtures + genuine-replay compatibility and flight tests
    examples/     research probes used for format work
    docs/         ARCHITECTURE, FORMAT_FINDINGS, COMPATIBILITY, TELEMETRY_8111
```

## Quick start

```
cd static57-sdl/sdl-replay-decoder
cargo build --release
./target/release/sdl-replay-decoder inspect  path/to/replay.wrpl
./target/release/sdl-replay-decoder packets  path/to/replay.wrpl   # writes sdl-out/<name>/manifest.json + packets.jsonl
./target/release/sdl-replay-decoder flight   path/to/replay.wrpl   # writes flight_tracks.jsonl + flight_summary.json
./target/release/sdl-replay-decoder survey   path/to/Replays/
./target/release/sdl-replay-decoder flight-survey path/to/Replays/  # validation table for every replay
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

## Roles

- **Static 57 owner:** research direction, flight testing, approvals.
- **ChatGPT:** format research, specifications, review.
- **Claude Code:** implementation, testing, documentation.
