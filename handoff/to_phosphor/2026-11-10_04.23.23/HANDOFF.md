# SDL → Phosphor Handoff: flight data for replay `#2026.11.10 04.23.23.wrpl`

**From:** S57-003 SDL · **To:** Project Phosphor · **Date:** 2026-11-10 · **Handoff schema:** 0.1
**Related:** [`reports/2026-11-10_04.23.23_report.md`](../../../reports/2026-11-10_04.23.23_report.md) and FORMAT_FINDINGS §4.3, §4.6 and §4.6.10 (snapshot included)

> **Purpose.** This hands over SDL's decoded flight data and current format documentation for the second Me 262 A-1a/U4 replay, for use with Phosphor's ballistic engine. It is data and findings only. How Phosphor uses it is Phosphor's decision.

Evidence classes: **VERIFIED**, **REPORTED**, **DERIVED**, **EXPERIMENTAL**, **UNKNOWN**, as defined in FORMAT_FINDINGS.

## 1. Source

| Item | Value |
|---|---|
| Replay | `#2026.11.10 04.23.23.wrpl`, 4,661,984 bytes, SHA-256 `f4b1b2ae004f5abd16775dba7cfc7965958ec9ec6595d839e8d2e035fdc75b33` |
| Game build | 2.59.0.100 (EXPERIMENTAL word order) |
| Mission / aircraft | `air_vietnam_ec_BfD_norespawn` / `me-262a-1a_u4` |
| Decoder | `sdl-replay-decoder` 0.1.0, `flight` command, repository commit `16e17e2`. The decoder code is unchanged since `a6c961d` |
| Reproduce | `sdl-replay-decoder flight "<replay>" --out <dir>`. The output is deterministic: a re-run gave byte-identical files |

**No 8111 capture exists for this flight.** Height is checked against the replay's own altimeter, but x/z has no independent check here. The x/z validation in FORMAT_FINDINGS §4.5 comes from the 2026-11-09 replay.

## 2. Files

SHA-256 values are in `SHA256SUMS`. The folder is marked `-text` in git, so line endings stay byte-exact.

| File | Rows | Content |
|---|---|---|
| `flight_summary.json` | 1 object | Schema `static57.sdl.flight-summary/0.2`: sync tally, cockpit counts, altimeter cross-check, airborne interval, counter range |
| `ownship.jsonl` | 3,141 | Author position per `aircraft_small` sync (about 4 Hz) |
| `cockpit.jsonl` | 3,141 | Cockpit instruments per `0xD04A` message (about 4 Hz) |
| `weapons.jsonl` | 29 | `trigger`, `counter_step` and `kill` events |
| `manifest.json` | 1 object | Container, session, layout, codec and framing report |
| `FORMAT_FINDINGS.md` | — | Snapshot of `static57-sdl/sdl-replay-decoder/docs/FORMAT_FINDINGS.md` at commit `16e17e2` |

## 3. Using the data

- **Time.** `timestamp_ms` is replay session time in milliseconds, the same clock in every file. `start_time` + session time matches the file name to within 0.5 s (DERIVED).
- **Frame.** Native world frame in metres:
  - `y` is height above sea level.
  - +x is east and +z is north (VERIFIED against 8111 on the 2026-11-09 replay; it assumes a north-up map).
- **`ownship.jsonl`:**
  - `position` is `[x, y, z]`.
  - `position_evidence` is `VERIFIED` when the height is within 20 m of the cockpit altimeter. All 3,141 are; median difference 0.45 m, max 5.5 m.
  - `derived_speed_m_s` is a scalar finite difference (DERIVED).
  - The position layout itself is EXPERIMENTAL (FORMAT_FINDINGS §4.3).
- **`cockpit.jsonl`:**
  - Ids and names are in FORMAT_FINDINGS §4.2 and `src/mpi.rs` (`COCKPIT_PARAMS`); they match 8111 `/indicators` keys.
  - id 0 is **IAS** in m/s.
  - ids 52 and 53 (roll and pitch) are **inverted**: positive means left wing down / nose down.
  - ids 195/196 are fuel in kg.
  - Records are sparse: zero-valued ids appear to be omitted.
- **`weapons.jsonl`:**
  - `counter_step` events sit on the about-250 ms sync grid. A shot happened somewhere between the previous sync and `timestamp_ms`.
  - The counter is EXPERIMENTAL as an ammunition count (32 → 17, 15 shots).
  - `kill` is the `0xF058` record (EXPERIMENTAL field reading).

## 4. Projectile hit data (not in the JSONL files)

The 9 author hits on this replay, with launch positions, reconstructed launch times, recorded hit times, flight times (0.378–0.704 s), ranges (377–719 m) and target positions, are tabulated in **FORMAT_FINDINGS §4.6.10**. The `0xF0C2` field layout and the per-shell launch and velocity vectors are in §4.6.4. These are **not** decoder outputs yet; they come from read-only analysis.

Classes to keep in mind:
- **Recorded:** hit times (1 ms resolution, accuracy unverified) and firing windows.
- **DERIVED, not measured:** launch times and flight times.
- **EXPERIMENTAL:** the units of the velocity vector, and the meaning of the launch position (aircraft origin or muzzle).
- **Not available:** measured impact coordinates and missed-shell trajectories (§4.6.8).

## 5. Known limitations

1. Same aircraft, mission and build as the 2026-11-09 replay. The findings repeat, but generality is untested.
2. Only the author's aircraft is decoded. Ground-target positions are available only through the `0xF0C2` hit messages (§4.6).
3. The aircraft's flight-model attitude and velocity vector are not decoded. Attitude comes from the cockpit gauges (ids 52, 53, 64) at about 4 Hz.
4. Launch-time accuracy and hit-time quantization are open questions (FORMAT_FINDINGS §5, items 7–8).
