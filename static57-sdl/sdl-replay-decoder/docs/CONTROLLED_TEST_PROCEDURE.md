# S57-003 SDL — Controlled 8111 Recording Procedure

Status: **prepared, not executed.** No controlled recording exists yet, and nothing here is a result.

This procedure records a replay and localhost:8111 telemetry from the same flight, so SDL's replay measurements can be checked against a source captured separately. Field mapping, the horizontal transform, clock alignment and acceptance metrics are in [TELEMETRY_8111.md](TELEMETRY_8111.md).

## 1. What it tests

| Question | Sequence |
|---|---|
| Are decoded altitude, speed, attitude and heading correct? | A |
| How do world X/Z relate to the map? | A |
| Do raw control and engine fields follow pilot input? | B |
| Does SDL report the launcher, launch time and weapon name for a known shot? | C |
| Does a claimed target match the aircraft actually engaged? | C |

**Out of scope:** miss classification, closest approach and LAR calibration. The pilot's note of what happened to a shot goes in the log as REPORTED context, and SDL does not use it.

## 2. Setup

1. Use a custom battle on the current build, with one known aircraft and a fixed loadout.
2. Before takeoff, record the test log header (§5).
3. Start the 8111 logger before spawning:
   - `/state` and `/indicators` at 10 Hz or faster.
   - `/map_obj.json` at 2 Hz or faster.
   - `/map_info.json` once.
   - Store each raw response unmodified, with the host's monotonic time.
4. Confirm the logger is writing, then spawn.
5. Save the replay when the flight ends. Never edit the `.wrpl` or the raw log.

## 3. MARK events

Press MARK at each listed point, and write the session time in the test log. If MARK events prove not to reach the logger or replay, record the time by hand and state that in the log. A MARK helps align clocks and is not a measurement.

## 4. Sequences

Fly them in order. Each one stands alone, so a failed sequence can be flown again.

### Sequence A: flight state (about 8 minutes)

| Step | Action | MARK |
|---|---|---|
| A1 | Takeoff and climb to a chosen altitude | Wheels up |
| A2 | Level flight, 60 s on each of four headings 90° apart | Start of each leg |
| A3 | Steady climb, then steady dive | Start of each |
| A4 | Full 360° roll, then a sustained turn of at least 360° | Start of each |

### Sequence B: controls and engine (about 4 minutes)

| Step | Action | MARK |
|---|---|---|
| B1 | Level flight, throttle stepped idle, 50%, 100% and back, 15 s each | Each step |
| B2 | Separate pitch, roll and yaw inputs, each held 3 s | Each input |
| B3 | Flaps, gear and airbrake, each cycled once at a safe speed | Each change |

The 8111 field names for control surfaces, throttle, engine and fuel vary by aircraft, so confirm them from the first log.

### Sequence C: weapon launches (repeat per weapon variant)

Rules:

- One aircraft and **one weapon variant per sequence**. Load no other guided weapon.
- One known target at a time, recorded in the log with its vehicle. A single AI target is preferred, with no other aircraft nearby.
- At least 30 s between shots, so kill and seeker records cannot overlap.
- Fly straight and level for 5 s before each shot.

| Step | Action | MARK |
|---|---|---|
| C1 | Note range, altitude and aspect from the cockpit | Before the shot |
| C2 | Fire one weapon | At release |
| C3 | Fly straight until the weapon's flight ends | When it ends |
| C4 | Note what was observed as REPORTED context | None |

## 5. Test log

Keep one log per flight, in plain text.

**Header:** date, game build, mission, aircraft, loadout, weapon variant per sequence, target vehicle, altimeter units, pilot.

**One line per event:** session time, MARK or not, sequence and step, action, note.

## 6. Files to keep

| File | Requirement |
|---|---|
| `.wrpl` | Original, unmodified |
| Raw 8111 log | Original, unmodified, with host timestamps |
| Test log | As written during the flight |
| Hashes | SHA-256 of the replay and the 8111 log |

## 7. Analysis

1. Run `sdl-replay-decoder research <replay> --out research` and keep the decoder version with the results.
2. Align clocks and compute the metrics in `TELEMETRY_8111.md` §4 and §6.
3. For Sequence C, compare each logged shot with SDL's output:

| Item | Compared with | Agreement means |
|---|---|---|
| Launcher | Logged aircraft | Same unit and vehicle |
| Launch time | MARK at release | Within the alignment uncertainty |
| Launch position | Aircraft track and 8111 position at release | Within the measured launch-point spread |
| Weapon name | Logged variant | Same variant. A null name is a gap, not an error |
| Target | Logged target | Same unit, or `unavailable` |

4. Count disagreements and unavailable values separately, and never drop a shot from the count.
5. Record results as DERIVED, with both file hashes. A pass supports only the conditions flown.

## 8. What a good result does and does not allow

- **It would allow:** the flight-state fields to be called independently validated for the aircraft flown, and the launcher, launch time and name to be called checked for the variants flown.
- **It would not allow:** any claim of miss validation, hit probability, or LAR calibration readiness.
