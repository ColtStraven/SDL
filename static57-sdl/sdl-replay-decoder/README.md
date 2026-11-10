# sdl-replay-decoder

Static 57 — Simulation Dynamics Laboratory (S57-003): a native War Thunder `.wrpl` replay decoder, written in Rust.

**Status:** Milestones 1 (file inspector) and 2 (packet extractor) are implemented. They are validated on 29 genuine 2.59 client replays (format `0x00018C1C`, zstd); see [docs/COMPATIBILITY.md](docs/COMPATIBILITY.md).

Milestone 3 has started: the `flight` command decodes the **replay author's** world position track, cockpit instruments (airspeed, altitude, vertical speed, attitude, engines), firing events, a per-shot counter and kill/damage records. Position height is verified against the cockpit altimeter on every sample, and x/z, height, attitude and airspeed were checked against an independent localhost:8111 capture (x/z RMS 2.2 m). The ownship record and altimeter check also hold on a second replay (2026-11-10); both are the same aircraft and mission. Other aircraft, sensors and missiles are not decoded.

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
sdl-replay-decoder survey  <folder-or-files...> [--out survey.json]
```

- `inspect` checks the header, sections, codec and full decompression, and prints a summary. `--out` writes `manifest.json`; `--json` prints the manifest instead of the summary.
- `packets` adds packet framing. It writes `manifest.json` and `packets.jsonl` to `--out`, or by default to `./sdl-out/<replay name>/`. `--payloads` adds each payload as hex (large).
- `flight` decodes the author's flight and weapon data. It writes `manifest.json`, `flight_summary.json`, `ownship.jsonl`, `cockpit.jsonl` and `weapons.jsonl` (default `./sdl-out/<replay name>/`) and prints the altimeter cross-check.
- `survey` frames many replays and prints a compatibility table.

Exit codes: `0` ok, `1` partial (report produced, with errors), `2` rejected or I/O failure.

Replays are opened read-only. Output is never written next to the replay unless `--out` points there.

## Tests

```
cargo test                                   # synthetic fixtures (50 tests)
cargo test --release -- --include-ignored    # plus genuine replays
```

The genuine-replay test reads `SDL_REPLAY_DIR`, or `%LOCALAPPDATA%\WarThunder\Replays` when that variable is unset. Passing the synthetic fixtures alone does **not** demonstrate War Thunder compatibility.

## Documents

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): modules, output contract, future module boundaries
- [docs/FORMAT_FINDINGS.md](docs/FORMAT_FINDINGS.md): confirmed format structures, each with its evidence class
- [docs/COMPATIBILITY.md](docs/COMPATIBILITY.md): every genuine replay tested, with SHA-256

## Provenance

The header layout, framing rules and packet-type names were researched from published documentation: WT_KNOWLEDGE_DOCS, wrpl-inspector (AGPL-3.0) and WrplReplayParser. Those projects were used as references only. No code from them is included or linked. Dependencies: `ruzstd` (zstd), `flate2`/miniz_oxide (zlib), `sha2`, `serde`/`serde_json`, `clap`.
