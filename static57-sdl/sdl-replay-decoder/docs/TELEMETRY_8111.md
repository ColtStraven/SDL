# S57-003 SDL — localhost:8111 Validation Plan

Status: **prepared, not executed.** No synchronized 8111 recording has been provided yet. Nothing here is a result.

## 1. Purpose

The replay decoder's position, attitude and velocity are already validated against the replay's own cockpit message (MPI `0xD04A`, `FORMAT_FINDINGS.md` §6). That reference is recorded by the same game client in the same file. 8111 gives a second reference that is captured independently, and it is the only one available for the **horizontal frame**: how world X/Z relate to the map.

## 2. What 8111 provides, and what it does not

| Endpoint | Fields used | Units (to confirm per recording) | Compared with |
|---|---|---|---|
| `/state` | `H, m` (altitude), `TAS, km/h`, `IAS, km/h`, `Vy, m/s`, `AoA, deg` | As labelled in the JSON keys | Altitude: Y. TAS: decoded speed (÷3.6). Vy: decoded vy. AoA: pitch − flight-path angle (an expectation, not identity) |
| `/indicators` | `aviahorizon_roll`, `aviahorizon_pitch`, `compass` | deg (sign conventions to confirm) | Bank, pitch, heading |
| `/map_obj.json` | Player object `x`, `y` (normalized 0–1 map coordinates), `dx`, `dy` (direction) | Fraction of map extent | X/Z after the transform below |
| `/map_info.json` | `map_min`, `map_max` (world metres), `grid_steps`, `grid_zero` | m | Defines the map-normalized ↔ world transform |

8111 does **not** supply world XYZ. `/map_obj.json` is 2D, normalized and low-resolution. `/state` altitude is a separate scalar. Do not combine them into a "3D truth".

## 3. Horizontal transform to be determined (EXPERIMENTAL until fitted)

Hypothesis to test: `map_x = (X − map_min.x) / (map_max.x − map_min.x)` and `map_y = (Z − map_min.y) / (map_max.y − map_min.y)`, possibly with one axis flipped (`1 − ...`). The flip and the axis pairing are unknowns. Fit all eight sign/axis combinations by least squares over the flight. Accept one only if its residual is consistent with 8111's position quantization and the others are clearly worse. The heading convention (`(cos h, 0, −sin h)` in world axes) then gives compass heading once north is fixed. Cross-check against `/indicators` `compass`.

## 4. Clock alignment

1. Record each 8111 poll with the host's monotonic time.
2. Replay timestamps are session milliseconds (`FORMAT_FINDINGS.md` §3). Estimate `offset = t_8111 − t_replay` by maximising cross-correlation of altitude (or vertical speed) over a manoeuvre, then refine with the MARK events.
3. Report the offset, its uncertainty (half the 8111 poll interval combined with the cross-correlation peak width), and residual drift (linear fit of offset over time).
4. Compare at 8111 sample times, using replay values linearly interpolated between 250 ms sync records (the same method as the cockpit validation).

## 5. Recording requirements

- One short custom battle, flown by the owner on the current build. 10–15 minutes is enough.
- 8111 polled at ≥ 10 Hz for `/state` and `/indicators`, ≥ 2 Hz for `/map_obj.json`; `/map_info.json` once.
- Include: level flight on four headings 90° apart, a climb and a dive, a 360° roll, and a sustained turn.
- Press MARK at known events (wheels-up, start of each heading leg). This lets the clock alignment be checked.
- Keep the `.wrpl`, the raw 8111 JSON log (unmodified, with host timestamps), the War Thunder build and SHA-256 of both files.

## 6. Acceptance metrics (proposed)

| Quantity | Metric |
|---|---|
| Altitude | Median and p99 abs error after clock alignment |
| TAS | Median ratio and abs error; saturated samples (≥ 500 m/s) excluded |
| Vertical speed | Median abs error |
| Roll and pitch | Median and p99 abs error, sign convention confirmed |
| Horizontal position | Residual of the fitted transform, in metres and in map fraction |
| Heading | Median abs error vs compass after north is fixed |

All results must be recorded with the evidence class DERIVED (cross-instrument comparison) and the exact replay and 8111 log hashes.
