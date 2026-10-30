# sdl-replay-decoder

Static 57 — Simulation Dynamics Laboratory (S57-003): a native War Thunder `.wrpl` replay decoder, written in Rust.

**Status:**

| Milestone | State |
|---|---|
| 1. File inspector | Implemented |
| 2. Packet extractor | Implemented |
| 3. Aircraft flight-state reconstruction | Implemented |
| 4. Research reports, identity and weapon extraction | Implemented |

All are validated on 42 genuine 2.59 client replays (format `0x00018C1C`, zstd); see [docs/COMPATIBILITY.md](docs/COMPATIBILITY.md).

SDL produces research reports from replays: aircraft tracks with vehicle identity, weapon trajectories with launch conditions, battle-log outcomes, and proposals for review against the Strike Master databases. Weapon types are inferred from kill records through a per-replay type id, and 3,284 of 5,817 tracked weapons are named. Sensors, seekers and ground units are not decoded, and the horizontal map frame is not yet tied to map north. See [docs/RESEARCH_WORKFLOW.md](docs/RESEARCH_WORKFLOW.md).

## Build

Requires a Rust stable toolchain. On this machine that is the `x86_64-pc-windows-gnu` toolchain; no Visual Studio is needed.

```
cargo build --release
```

## Usage

```
sdl-replay-decoder inspect <replay.wrpl> [--out DIR] [--json]
sdl-replay-decoder packets <replay.wrpl> [--out DIR] [--payloads]
sdl-replay-decoder flight  <replay.wrpl> [--out DIR]
sdl-replay-decoder research <replay.wrpl> [--out DIR] [--db-dir DIR]
sdl-replay-decoder research-survey <folder-or-files...> [--out DIR] [--db-dir DIR]
sdl-replay-decoder survey        <folder-or-files...> [--out survey.json]
sdl-replay-decoder flight-survey <folder-or-files...> [--out flight_survey.json]
```

- `inspect` checks the header, sections, codec and full decompression, and prints a summary. `--out` writes `manifest.json`; `--json` prints the manifest instead of the summary.
- `packets` adds packet framing. It writes `manifest.json` and `packets.jsonl` to `--out`, or by default to `./sdl-out/<replay name>/`. `--payloads` adds each payload as hex (large).
- `flight` reconstructs aircraft tracks. It writes `flight_tracks.jsonl` (one sample per aircraft record) and `flight_summary.json` (coverage, tracks, validation), and prints a validation summary. Output for a long battle is a few hundred MB.
- `research` writes `research-report.md`, `research-data.json` and `weapon_tracks.jsonl` to `--out`, or by default to `./research/<replay name>/`. `--db-dir` reads the Strike Master databases (never written) to map names to permanent ids.
- `research-survey` does that for every replay and adds `research-summary.md` and `research-summary.json`.
- `survey` and `flight-survey` process many replays and print compatibility or validation tables.

Exit codes: `0` ok, `1` partial (report produced, with errors), `2` rejected or I/O failure.

Replays are opened read-only. Output is never written next to the replay unless `--out` points there.

## Tests

```
cargo test                                   # synthetic fixtures (110 tests)
cargo test --release -- --include-ignored    # plus 3 genuine-replay tests (113 total)
```

The genuine-replay tests read `SDL_REPLAY_DIR`, or `%LOCALAPPDATA%\WarThunder\Replays` when that variable is unset. Passing the synthetic fixtures alone does **not** demonstrate War Thunder compatibility.

Research probes used during format work are in `examples/`, and run with `cargo run --release --example <name> -- <args>`.

## Documents

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): modules, output contracts, future module boundaries
- [docs/FORMAT_FINDINGS.md](docs/FORMAT_FINDINGS.md): confirmed format structures and decoded fields, each with its evidence class and validation
- [docs/COMPATIBILITY.md](docs/COMPATIBILITY.md): every genuine replay tested, with SHA-256 and per-replay validation and extraction
- [docs/RESEARCH_WORKFLOW.md](docs/RESEARCH_WORKFLOW.md): how to generate and read research reports
- [docs/FINDINGS_M4_WEAPONS_IDENTITY.md](docs/FINDINGS_M4_WEAPONS_IDENTITY.md): identity, battle log and weapon structures
- [docs/FINDINGS_M5_WEAPON_TYPE.md](docs/FINDINGS_M5_WEAPON_TYPE.md): weapon naming and its validation
- [docs/examples/](docs/examples/): real generated reports
- [docs/TELEMETRY_8111.md](docs/TELEMETRY_8111.md): plan for validation against localhost:8111 recordings (not yet executed)

## Provenance

The header layout, framing rules, packet-type names and the type-2 record structure were researched from published documentation and source: WT_KNOWLEDGE_DOCS, wrpl-inspector (AGPL-3.0) and WrplReplayParser. Those projects were used as references only. No code from them is included or linked. Field meanings and scales (velocity packing, conventions, altimeter ids) were established or confirmed by SDL's own measurements. Dependencies: `ruzstd` (zstd), `flate2`/miniz_oxide (zlib), `lz4_flex` (LZ4), `sha2`, `serde`/`serde_json`, `clap`.
