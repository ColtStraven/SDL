# sdl-replay-decoder

Static 57 — Simulation Dynamics Laboratory (S57-003): a native War Thunder `.wrpl` replay decoder, written in Rust.

**Status:** Milestones 1 (file inspector) and 2 (packet extractor) are implemented. They are validated on 27 genuine 2.59 client replays (format `0x00018C1C`, zstd); see [docs/COMPATIBILITY.md](docs/COMPATIBILITY.md). Packet *payloads* are not decoded yet: no flight, weapon or sensor data.

## Build

Requires a Rust stable toolchain. On this machine that is the `x86_64-pc-windows-gnu` toolchain; no Visual Studio is needed.

```
cargo build --release
```

## Usage

```
sdl-replay-decoder inspect <replay.wrpl> [--out DIR] [--json]
sdl-replay-decoder packets <replay.wrpl> [--out DIR] [--payloads]
sdl-replay-decoder survey  <folder-or-files...> [--out survey.json]
```

- `inspect` checks the header, sections, codec and full decompression, and prints a summary. `--out` writes `manifest.json`; `--json` prints the manifest instead of the summary.
- `packets` adds packet framing. It writes `manifest.json` and `packets.jsonl` to `--out`, or by default to `./sdl-out/<replay name>/`. `--payloads` adds each payload as hex (large).
- `survey` frames many replays and prints a compatibility table.

Exit codes: `0` ok, `1` partial (report produced, with errors), `2` rejected or I/O failure.

Replays are opened read-only. Output is never written next to the replay unless `--out` points there.

## Tests

```
cargo test                                   # synthetic fixtures (40 tests)
cargo test --release -- --include-ignored    # plus genuine replays
```

The genuine-replay test reads `SDL_REPLAY_DIR`, or `%LOCALAPPDATA%\WarThunder\Replays` when that variable is unset. Passing the synthetic fixtures alone does **not** demonstrate War Thunder compatibility.

## Documents

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): modules, output contract, future module boundaries
- [docs/FORMAT_FINDINGS.md](docs/FORMAT_FINDINGS.md): confirmed format structures, each with its evidence class
- [docs/COMPATIBILITY.md](docs/COMPATIBILITY.md): every genuine replay tested, with SHA-256

## Provenance

The header layout, framing rules and packet-type names were researched from published documentation: WT_KNOWLEDGE_DOCS, wrpl-inspector (AGPL-3.0) and WrplReplayParser. Those projects were used as references only. No code from them is included or linked. Dependencies: `ruzstd` (zstd), `flate2`/miniz_oxide (zlib), `sha2`, `serde`/`serde_json`, `clap`.
