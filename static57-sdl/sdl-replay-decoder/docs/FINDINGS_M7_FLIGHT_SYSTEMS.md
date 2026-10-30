# S57-003 SDL — Telemetry Validation and Flight Systems Findings (Milestone 7)

Decoder: `sdl-replay-decoder` 0.6.0. Date: 2026-10-30. Branch: `milestone-7`.

**What this covers:** one real 8111 capture, compared with two complete replays. Two aircraft, 638 s of aligned flight, game version 2.59.0.92.

**What it does not cover:** a controlled flight, the rest of the fleet, or a feet altimeter. The capture was of ordinary combat flying, recorded while the owner happened to be in the game.

Reproduce:

```bash
sdl-replay-decoder telemetry-validate telemetry/20261030_163709 "<Replays>/#2026.10.30 09.44.48.wrpl"
```

Captures and replays are not in Git. The capture's SHA-256 begins `31f3de9c`, and full hashes are in each `validation-report.md`.

## 1. What independent means here

The 8111 server and the replay file are both written by the same game client. They are separate outputs, reached by separate paths, and SDL parses them with unrelated code. Agreement shows SDL decodes the replay correctly. It does not show that the game's flight model matches real aircraft.

## 2. Association

| Check | `09.44.48` | `09.50.03` |
|---|---|---|
| Replay vehicle | `saab_a32a_red_adam` | `saab_j21a_1` |
| UTC overlap | Yes | Yes |
| Vehicle in capture | Yes | Yes |
| Alignment | established | established |
| Class | confirmed | confirmed |
| Share of capture aligned | 29.4%, 434 s | 13.8%, 204 s |

The rest of the capture flew other aircraft, whose replays had not been saved when this was run. They count only in the telemetry-only checks.

## 3. Time alignment

`t_replay = t_capture + offset + drift * (t_capture - reference)`

| Result | `09.44.48` | `09.50.03` |
|---|---|---|
| Offset, cockpit clock | 6580.970 s | 7766.063 s |
| Offset, motion correlation | 6581.017 s | 7766.183 s |
| Clock minus motion | 0.047 s | 0.120 s |
| Stated uncertainty | 0.050 s | 0.050 s |
| Clock ticks matched | 420 | 199 |
| Motion windows, peak | 8, 0.999 | 4, 1.000 |
| Drift | -1.1 ppm | Not measured, flight too short |
| Smallest measurable drift | 231 ppm | Not applicable |

Findings:

- **The cockpit clock is shared.** 8111 `clock_*` and the replay's cockpit ids 69, 71 and 73 match with scale 1.000 and residual 0.000. This is a direct time link. SDL found it in the replay data, and community documentation then confirmed the 8111 fields.
- **Two independent methods agree.** Clock and motion agree to 0.12 s or better, so the 0.05 s floor is optimistic for the second flight. Treat alignment as good to about 0.1 s.
- **Each replay needs its own offset.** Session time does not carry across battles: the offsets differ by 1,185 s within one capture. A single capture can validate several replays.
- **Drift is not resolved.** The figure is below what 434 s can measure. The host UTC clock agrees with the monotonic clock to -0.1 ppm.
- **HUD messages carry session time.** `/hudmsg` `time` is within -0.10 s and +0.41 s of the replay's kill time (median, 8 and 3 pairs). It is a third link, with too few pairs to rely on yet.
- **Gear events are a cross-check only.** They sit 2.6 to 2.7 s from each other after alignment, which fits the replay bit changing at the start of the gear cycle and `gears` crossing 0.5 later. This timing is an interpretation.

## 4. Flight-state results

Error is replay minus 8111, as bias, then median and p99 of absolute error.

| Quantity | Unit | `saab_a32a_red_adam` | `saab_j21a_1` |
|---|---|---|---|
| Altitude vs `H, m` | m | 0.06, 0.51, 9.7 | -0.73, 0.73, 2.9 |
| Vertical speed vs `Vy` | m/s | 0.00, 0.22, 2.6 | -0.03, 0.12, 1.2 |
| IAS: cockpit id 0 vs `IAS` | m/s | 0.00, 0.18, 2.4 | -0.15, 0.17, 1.3 |
| Pitch | deg | -0.01, 0.06, 1.1 | 0.00, 0.09, 1.9 |
| Roll | deg | 0.01, 0.05, 4.2 | 0.00, 0.17, 3.2 |
| Heading vs compass | deg | 0.00, 0.05, 1.3 | -0.03, 0.07, 1.5 |
| Load factor vs `Ny` | g | 0.00, 0.02, 1.3 | 0.00, 0.03, 0.35 |

Estimates and differences, kept apart from the rows above:

| Quantity | Unit | `saab_a32a_red_adam` | `saab_j21a_1` |
|---|---|---|---|
| IAS estimate from ground speed and ISA | m/s | -0.22, 0.27, 2.7 | -0.43, 0.43, 1.2 |
| Ground speed minus `TAS` | m/s | 0.74, 0.80, 1.9 | -0.07, 0.28, 0.9 |
| Mach estimate from ground speed and ISA | Mach | 0.001, 0.002, 0.010 | -0.001, 0.002, 0.007 |
| Pitch minus flight-path angle vs `AoA` | deg | 0.00, 0.10, 13.6 | 0.07, 0.13, 2.0 |

How to read these:

- **Median errors are near 8111's resolution.** `H, m` is a whole metre, `IAS` and `TAS` are whole km/h, about 0.28 m/s. Most of the 0.5 m altitude median is that rounding.
- **The p99 values come from fast motion.** The replay has 4 Hz samples and 8111 has 10 Hz, so hard rolls and pulls show interpolation error.
- **Ground speed is not TAS.** The 0.74 and -0.07 m/s biases are small but differ between flights. Their cause is not established. Wind is one candidate.
- **AoA is an expectation.** Pitch minus flight-path angle equals AoA only in wings-level flight, which explains the 13.6° tail.
- **Load factor has outliers on the A32.** Its correlation with `Ny` is 0.79 against 0.95 on the J21, while the median is 0.02 g. Replay values come from 4 Hz positions.

**Open question:** the J21's -0.73 m altitude bias is larger than rounding explains. It is not a clock effect, since vertical speed is unbiased, and the A32 does not show it. The cause is unknown.

The earlier milestones checked these fields against the replay's own cockpit message. These results are the first check from a separate path. The evidence for position, orientation and velocity is stronger now, on these two aircraft.

## 5. World frame and map transform

Before 8111 was available, the replay's own compass (id 64) predicted the frame: compass = heading + 90 on all 33 replays with at least 100 samples, with replay medians within 0.06°. That means +X east and +Z north.

The real capture then confirmed it, on two flights:

| Result | `09.44.48` | `09.50.03` |
|---|---|---|
| Best of 8 hypotheses | `map_x = +X, map_y = -Z` | `map_x = +X, map_y = -Z` |
| RMS, m | 10.5 | 7.7 |
| Next hypothesis | 1,879× worse | 715× worse |
| Free scale x, per m | 7.629e-6 | 7.639e-6 |
| Free scale y, per m | -7.630e-6 | -7.634e-6 |
| Scale from map limits | 7.629e-6 | 7.629e-6 |
| Map direction vs compass (p50) | 0.05° | 0.07° |

```text
map_x = (X - map_min_x) / (map_max_x - map_min_x)
map_y = (map_max_y - Z) / (map_max_y - map_min_y)
```

This also confirms Strike Master's existing map assumption in `src/core/geo.ts`: x east, y down.

The 8 to 10 m RMS is near the resolution of a 6-decimal map fraction combined with 2 Hz sampling at 200 m/s. A hypothesis about map symbol lag was not tested.

## 6. Altimeter units

The two compared aircraft have metric altimeters: `altitude_hour` fits `H, m` at 1.0001 m per unit. So this capture says nothing new about the game's foot.

The earlier finding stands from the cockpit message only: feet altimeters use 3.28 ft/m (1/3.28 m), not the exact 3.28084 ft/m. SDL keeps both constants separate, and the validator reports the error under each. A feet-altimeter aircraft is needed, and the `av_8b_na` is the obvious choice.

## 7. Aircraft systems

Evidence classes are in `CANDIDATES` in [src/systems.rs](../src/systems.rs). None is VERIFIED: two aircraft do not establish a fleet.

### Control and engine fields

Seven bytes hold 14 nibbles, high nibble first. Offsets are from the start of the `controls` block.

| Field | Location | Scale | 8111 comparison | Class |
|---|---|---|---|---|
| Gear not retracted | State bit 0, formerly `ground_flag` | 0 up, 1 otherwise | `gear, %`: r 0.995, 0.994 | DERIVED |
| Flaps | Byte 0 high nibble | `floor(flaps * 15)` | `flaps, %`: r 0.991, 0.965, 0 and 33% only | DERIVED |
| Stick pitch | Byte 0 low nibble | Signed, 8 neutral | `stick_elevator`: monotonic except value 5 | DERIVED |
| Stick roll | Byte 1 high nibble | Signed, 8 neutral | `stick_ailerons`: monotonic except values 4 and 5 | DERIVED |
| Pedals | Byte 1 low nibble | Signed, 8 neutral | `rudder, %`: r 0.715, one aircraft | DERIVED |
| Follows `blister1` | Byte 2 high nibble | 0 or 15 | `blister1`: r 0.971, 0.976 | DERIVED |
| Throttle, coarse | Byte 5 low nibble | `floor(throttle * 15)`, capped | `throttle 1, %`: r 0.964, 0.995 | DERIVED |
| Throttle above 100% | Engine `boost_raw` | `raw / 32767 * 1.1` | 1.011, 1.022, 1.100 read 101, 102, 110% | DERIVED |
| Engine state | Engine state byte | See below | `RPM 1` | DERIVED |
| Variable geometry | Byte 5 high nibble | 0 to 15 | Not in capture | EXPERIMENTAL |
| Unassigned | Byte 2 low nibble, byte 4 high nibble | 0 or 15 | Never set in these flights | UNKNOWN |

These are 4-bit fields. A correlation near 0.7 on stick fields reflects that resolution and a stray cluster, so read the value tables from `examples/probe_fields.rs`.

**Corrections:**

- `ground_flag` is a gear bit. It stays set at speed with gear down. Agreement with cockpit gear is over 99.9% of about 19,900 samples.
- Nibble 4 is not a wheel brake, as first guessed. It follows `blister1`. What that field represents is not established.
- Nibble 0 was listed as unassigned until 8111 showed it is flaps.

**Engine state against RPM:**

| State | 8111 RPM | Reading |
|---|---|---|
| 0 | 0 | Stopped |
| 1 | 12 to 18 | Starting |
| 2 | About 29 | Starting |
| 4 | 120 | Starting |
| 5 | 190 to 210 | Starting |
| 7 | Full | Running |
| 8 | 0 | Stopped, possibly after running |

### Cockpit ids matched to 8111

Each pair below has r of at least 0.99 on the aircraft that carry it. Which ids exist depends on the aircraft's instruments.

| Id | 8111 field | Id | 8111 field |
|---|---|---|---|
| 0 | `speed`, `IAS` | 129 | `oil_pressure` |
| 15 | `pedals` | 135 | `oil_temperature` |
| 23, 24 | `stick_elevator` | 141 | `water_temperature` |
| 31 | `stick_ailerons` | 195, 196 | `fuel`, `Mfuel` (kg) |
| 39 | `vario`, `Vy` | 228-230 | `gear, %`, gear lamps |
| 40, 41 | `altitude_hour`, `altitude_min` | 239 | `trimmer` |
| 52, 53 | `aviahorizon_roll`, `aviahorizon_pitch` (pitch opposite sign) | 240 | `throttle` |
| 64 | `compass` | 276 | `prop_pitch` |
| 69, 71, 73 | `clock_hour`, `clock_min`, `clock_sec` | 386 | `radiator 1, %` |
| 75 | `manifold_pressure` | 423 | `oil_radiator_lever1_1` |
| 81 | `rpm`, `RPM 1` | 453 | `mach` |
| 482, 484 | `g_meter`, `g_meter_max` | | |

### Load factor

Load factor from the second difference of position agrees with the cockpit g-meter (id 482) at r of 0.93 to 0.99 on 11 aircraft, and with 8111 `Ny` to a median of 0.03 g. Replay data can therefore give author load factor for the whole corpus. Short peaks are smoothed at 4 Hz.

### Fuel

Fuel mass is now available in replays on most aircraft, at about 4 Hz, in kg. From the capture alone, in combat flying:

| Vehicle | Seconds | Burned kg | kg/s | Mean throttle |
|---|---|---|---|---|
| `ffa_p16` | 631 | 513 | 0.81 | 86% |
| `saab_a32a_red_adam` | 472 | 931 | 1.97 | 83% |
| `saab_j21a_1` | 218 | 15 | 0.07 | 82% |

These are observations, not performance figures. Altitude, loadout and throttle varied. The J21 figure is far below the others and is not explained.

## 8. Software validation

Separate from the real results above:

- **Tests:** 160 pass, with 0 build warnings. They include a fake HTTP server, synthetic flights with known clocks, and the real capture.
- **Regression:** all 48 replays in the folder decode.
- **Synthetic alignment on genuine replays:** 23 of 24 flown replays recovered a known offset within 0.25 s, and the 24th reported failure rather than a wrong answer. These captures are built from the replay, so they test the software and say nothing about the game.

Faults the tests and real data found:

- A simulator bias of 0.116 s, caught because motion and clock disagreed.
- A false "0 ppm" drift, now reported with its measurable limit.
- Position jumps that ruined derived load factor, which also made id 482 look aircraft-dependent. That claim is withdrawn.

## 9. Limitations

- Two aircraft, one capture, ordinary combat flying, 638 s.
- Controls were lightly exercised: flaps saw two positions, and airbrake was never used.
- No feet altimeter was compared.
- Drift is unmeasured. A flight of 30 minutes or more without a battle change would help.
- A game pause breaks alignment, and SDL reports that rather than repairing it.
- The replay's speed saturates at 500 m/s.
- Remote aircraft are not validated. 8111 reports only the player.

## 10. Unresolved questions

1. What causes the J21's -0.73 m altitude bias?
2. What are nibbles 5 and 8? Airbrake is a candidate.
3. Why do stick nibbles have a stray cluster at value 5? It appears at low mean speed, on about 3% of samples.
4. What does `blister1` represent?
5. Why did the J21 burn so little fuel?
6. Is the 0.74 m/s ground speed minus TAS bias wind?
7. Do feet altimeters match 3.28 ft/m in 8111?
8. What is the variable-geometry nibble's scale?
