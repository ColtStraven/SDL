# S57-003 SDL — Research Workflow

SDL turns replays into research reports that an analyst can read directly. It runs locally, uses no AI service, and never modifies replays or the Strike Master databases.

## Process

1. Fly a test or a sortie and save the replay.
2. Generate reports. From `C:\Users\Shadow\Desktop\Projects\SDL`:

   ```bash
   static57-sdl/sdl-replay-decoder/target/release/sdl-replay-decoder research-survey "C:/Users/Shadow/AppData/Local/WarThunder/Replays" --out research --db-dir ../Strike_Master_Elite
   ```

   For one replay use `research <replay.wrpl>` with the same options.
3. Claude Code reads `research/research-summary.md`, then the report for each replay of interest.
4. Claude identifies engineering findings and recommends tests, models or changes.
5. The owner approves any change before it reaches Strike Master or its databases.

## Output

Default location on this PC: `C:\Users\Shadow\Desktop\Projects\SDL\research`. Each command prints the absolute path of every file it writes.

```
research/
  INDEX.md                every replay, newest recording first, start here
  LATEST.md               the most recently recorded replay
  research-summary.md     cross-replay analysis and research opportunities
  research-summary.json   the same, machine-readable
  <replay name>/
    research-report.md    concise per-replay report
    research-data.json    tracks, weapons, findings, engineering findings
    weapon_tracks.jsonl   every weapon sample
    validation-report.md  8111 comparison, when a capture exists
    validation-data.json  the same, machine-readable
```

`INDEX.md` and `LATEST.md` are rebuilt from every report folder after each command, so older reports stay listed. "Latest" uses the replay header's recording time, never a file time. With `--out a/b`, the index goes in `a`. With a bare `--out b`, it goes in `b`.

After `telemetry-validate`, run `research` on that replay to bring the validation into its engineering findings.

## Reading a replay report

Each report opens with an executive summary and main observations, then **Strike Master Engineering Findings** and **Recommended research priorities**. Detail follows, and full values stay in the JSON.

A finding appears only if the replay holds evidence for it. Each has a stable id made from its rule and the replay hash, for example `FLT-FUEL-92fbc521`. In `research-data.json` the findings are under `engineering.findings`, schema `static57.sdl.engineering-findings/0.1`, and a missing measurement is `null`. New fields are additive, so readers of older datasets keep working.

| Priority | Meaning |
|---|---|
| HIGH | Potentially significant problem or finding affecting Strike Master reliability, calculations or core capabilities |
| MEDIUM | Promising improvement or research opportunity that needs more evidence |
| LOW | Useful observation, documentation point or longer-term idea |

Priorities reflect evidence quality and engineering impact, not combat results.

| Nature | Meaning |
|---|---|
| `measurement` | What SDL measured |
| `research_opportunity` | Worth investigating |
| `data_gap` | Something is missing |
| `unresolved_discrepancy` | Two things disagree, cause not established |
| `software_error` | SDL's own structural checks failed. Nothing else is called a bug |

Findings are recommendations for owner review. SDL never changes Strike Master or its databases.

After a flight recorded with `telemetry-record`, `telemetry-validate` adds `validation-report.md` and `validation-data.json` to the replay's folder. See [RECORD_ONE_FLIGHT.md](RECORD_ONE_FLIGHT.md).

`research/` and `telemetry/` are generated output and are not committed. Aircraft sample data can be produced on demand with `sdl-replay-decoder flight <replay>`; it reaches several hundred MB for a long battle.

Reports are generated from the JSON datasets alone, and tests check that they agree. Unmeasured values print as `unavailable` in Markdown and `null` in JSON. Neither means zero.

## Reading the evidence

| Class | Meaning here |
|---|---|
| VERIFIED | Structure proven by exact parsing, and meaning checked against an independent source |
| REPORTED | From third-party documentation only |
| DERIVED | Calculated, or joined by geometry and time |
| EXPERIMENTAL | Plausible, awaiting validation |
| UNKNOWN | Not established |

## Cautions for analysis

- **Replay agreement validates decoding,** not real-world aircraft realism. Any model built from these figures is an extrapolation until validated.
- **Weapon results are not an envelope.** Names come from kill records or a shared per-replay type id (306 of 306 in hold-out), so types that never killed stay unnamed. SDL does not classify misses, and an `unknown` outcome contains hits, misses and everything else. Do not infer hit probability or envelope limits from them.
- **A kill link is a candidate,** not proof that the weapon caused the kill. "Confirmed" means a battle-log record links to the weapon.
- **Launchers are measured, targets are inferred.** The launcher is read from the creation record. Targets, names and outcomes are inferred associations, labelled DERIVED or UNKNOWN. An `ambiguous` link supplies no name.
- **Remote aircraft are network copies.** Maneuver figures are produced for the recording author only.
- **Combat maximums are lower bounds,** not performance limits, and aircraft configuration is unknown.
- **Speed saturates at 500 m/s** for aircraft. Those values are lower bounds and are flagged.
- **+X is east and +Z is north.** One real 8111 capture confirmed this on two flights.
- **Validation covers few aircraft.** Real 8111 comparisons so far cover two aircraft. A report's `independent_validation` is true only for a real capture, a confirmed replay and established alignment. A synthetic capture proves nothing about the game.
- **Replay speed is ground speed,** not TAS or IAS. Keep the three apart.
- **Time of flight** ends at the last sample. Its uncertainty is the sample interval.

## Strike Master proposals

With `--db-dir`, SDL reads `static57-war-thunder-aircraft.json` and `static57-war-thunder-weapons.json` and maps names to permanent ids. Aircraft match on the wiki slug. Weapons match on an exact designator, and an ambiguous name stays unmapped. `static57-war-thunder-threats.json` was not found in the Strike Master folder on 2026-10-30.

Each named shot appears in `strike_master.proposals` in `research-data.json` with status `proposed_for_review`, its name source, launch conditions, measurements, uncertainty and provenance. Nothing is applied automatically.
