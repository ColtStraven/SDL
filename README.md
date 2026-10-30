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
| M4 — Research reports and weapon extraction | Automated reports, aircraft identity, weapon trajectories, battle log, Strike Master proposals | **Done** (branch `milestone-4`) |
| Next | Weapon type decoding, sensors and seekers, ground units, telemetry fusion | Not started |

The decoder is validated on **27 genuine War Thunder 2.59 client replays** (format `0x00018C1C`, Zstandard), with 4,086,169 packets framed and every byte accounted for. All 103,698 aircraft sync packets decode, giving 2,468,023 timestamped 3D aircraft samples in 2,460 provisional track segments. The recording author's decoded altitude, attitude and vertical speed agree with the replay's own cockpit instruments in 19 replays: median errors of 0.000–0.65 m, 0.002–0.24° roll and 0.02–0.08° pitch. See [COMPATIBILITY.md](static57-sdl/sdl-replay-decoder/docs/COMPATIBILITY.md) and [FORMAT_FINDINGS.md](static57-sdl/sdl-replay-decoder/docs/FORMAT_FINDINGS.md).

Across 42 replays, SDL now also tracks 5,817 weapons from launch through flight, resolves vehicles for most aircraft tracks, reads the launcher of 5,523 of them from the creation record, names 3,982 (306 of 306 in a hold-out test), and classes outcomes from battle-log records. SDL does not classify misses, so an unknown outcome is not a miss. Milestone 7 adds a read-only 8111 recorder and independent validation: one real capture agrees with two replays to about 0.5 m altitude and 0.2 m/s IAS (median), and confirms +X east, +Z north. Every replay report now includes Strike Master engineering findings, and `research/INDEX.md` lists all reports. Milestones 6 and 7 are work in progress on the `milestone-7` branch.

SDL is a scientific extraction and reporting engine for Static 57 avionics research, not a replay viewer. See the [research workflow](static57-sdl/sdl-replay-decoder/docs/RESEARCH_WORKFLOW.md).

Weapon names are inferred, not read from a decoded type field. Sensors, seekers and ground units are **not decoded yet**, and the map-north orientation of the horizontal axes is unresolved.

## Repository layout

```
Static57_SDL_Native_Replay_Decoder_Design_v0.1.md   design specification v0.1
static57-sdl/
  sdl-replay-decoder/                               native Rust decoder (CLI + library)
    src/          container, versions, compression, packets, datablock, manifest, export,
                  bitstream, aircraft, flight, cockpit, tracks, ecs, units, idfield, combat,
                  shells, weapons, lab, measure, research, strikemaster, survey, report
    tests/        synthetic fixtures + genuine-replay compatibility and flight tests
    examples/     research probes used for format work
    docs/         ARCHITECTURE, RESEARCH_WORKFLOW, FORMAT_FINDINGS, FINDINGS_M4_WEAPONS_IDENTITY,
                  COMPATIBILITY, TELEMETRY_8111, examples/
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
./target/release/sdl-replay-decoder research path/to/replay.wrpl    # research report and dataset
./target/release/sdl-replay-decoder research-survey path/to/Replays/ --out research --db-dir path/to/Strike_Master_Elite
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
