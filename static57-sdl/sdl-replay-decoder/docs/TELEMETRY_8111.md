# S57-003 SDL — localhost:8111 Telemetry

The flight instructions are in [RECORD_ONE_FLIGHT.md](RECORD_ONE_FLIGHT.md). Results are in [FINDINGS_M7_FLIGHT_SYSTEMS.md](FINDINGS_M7_FLIGHT_SYSTEMS.md).

## 1. Purpose

The replay decoder was validated against the replay's own cockpit message. That reference is recorded by the same game client in the same file. 8111 is captured separately, by a different path, so it gives a second check. It is also the only source that links world X/Z to map coordinates.

## 2. Recorder

```bash
sdl-replay-decoder telemetry-record [--out telemetry] [--host 127.0.0.1] [--port 8111] [--duration-s N]
```

| Property | Behaviour |
|---|---|
| Access | HTTP GET of a fixed path list. Nothing is written to the game |
| Dependencies | Standard library only. No cloud or AI service, and no link to Strike Master |
| Storage | `capture.jsonl`, one line per request, flushed as it arrives. `session.json` holds session metadata |
| Raw data | Each response body is stored unmodified, as text |
| Each record | Sequence, endpoint, monotonic start and end, UTC stamp, HTTP status, parse result, the response's own `valid` flag, byte count, error text |
| Time | Monotonic seconds are the measurement clock. UTC is used only to match a capture to a replay |
| Failures | A failed request is logged and that endpoint waits 1 s, so a closed game does not flood the log |
| Stopping | `q` then Enter, or `--duration-s`. If the process is killed, the file remains valid up to the last whole line |
| Size | About 200 MB per hour in a full battle, mostly `/map_obj.json` |

| Endpoint | Rate | Use |
|---|---|---|
| `/state` | 10 Hz | Flight model values |
| `/indicators` | 10 Hz | Cockpit instruments, including the clock |
| `/map_obj.json` | 2 Hz | Player map position and direction |
| `/map_info.json` | 0.2 Hz | Map limits. Polled because the map can change after recording starts |
| `/hudmsg` | 1 Hz | Event text for time correlation |

## 3. Fields

Names are REPORTED by community documentation and the Strike Master dossier, and SDL has seen them live. Sets vary by aircraft, so SDL keeps every numeric field and requires none.

| Endpoint | Fields used | Compared with |
|---|---|---|
| `/state` | `H, m` | Position Y |
| `/state` | `Vy, m/s` | Velocity Y |
| `/state` | `IAS, km/h` | Replay cockpit IAS, and an ISA estimate, reported separately |
| `/state` | `TAS, km/h` | Replay world speed, as a labelled difference. Replay speed is ground speed, so the difference includes wind |
| `/state` | `M` | World speed over ISA sound speed, labelled as an estimate |
| `/state` | `Ny` | Load factor from position |
| `/state` | `AoA, deg` | Pitch minus flight-path angle, an expectation only |
| `/indicators` | `aviahorizon_pitch`, `aviahorizon_roll` | Pitch and bank. The sign is measured, not assumed |
| `/indicators` | `compass` | Heading + 90 |
| `/indicators` | `clock_hour`, `clock_min`, `clock_sec` | Replay cockpit clock, for time alignment |
| `/indicators` | `altitude_*` | Position Y, as metres, game feet (1/3.28 m) and exact feet (0.3048 m), each reported separately |
| `/map_obj.json` | Player `x`, `y`, `dx`, `dy` | World X/Z and compass |
| Both | Every other numeric field | Scanned against every replay control, engine and cockpit field |

8111 does not supply world XYZ. `/map_obj.json` is 2D, normalized and limited in resolution, and `H, m` is a whole-metre scalar.

## 4. Associating a capture with a replay

Three separate tests:

1. **Time.** The replay's header start and span overlap the capture's UTC window, within 300 s.
2. **Vehicle.** `/indicators` `type` equals the replay author's vehicle.
3. **Alignment.** The methods in §5 reach `established`.

All three give `confirmed`. Without a confirmed replay, a capture is never called independent validation. If time or vehicle fails, comparisons are withheld.

## 5. Synchronization

```text
t_replay = t_capture + offset + drift * (t_capture - reference)
```

No common start is assumed.

| Method | How | Role |
|---|---|---|
| `cockpit_clock` | The clock appears in replay cockpit data and in `/indicators`, so matching second ticks gives offset and drift | Primary |
| `motion_correlation` | Full search by normalized cross-correlation of altitude, vertical speed, pitch and roll, then repeated over 120 s windows | Independent check, and fallback with no clock |
| `gear_events` | Gear changes in both records after alignment | Cross-check only |
| `hud_events` | HUD messages against replay kills after alignment | Cross-check only |
| Manual marks | Mapped into replay time and shown with altitude and speed | Sanity check only |

| Quality | Meaning |
|---|---|
| `established` | Clock and motion agree, or motion alone passes over at least 3 windows |
| `weak` | One method passes with no confirmation |
| `conflict` | Clock and motion disagree beyond their uncertainty |
| `failed` | No method passes |

Comparisons run only for `established` or `weak` alignment with no discontinuity. A game pause shows as a discontinuity, and the report says so.

**Uncertainty.** The offset uncertainty combines the scatter of the estimates with a 50 ms floor for replay cockpit message latency. Each comparison also lists the error the clock uncertainty alone could cause. Drift smaller than twice the uncertainty over the aligned span cannot be told from zero, and the report gives that limit.

## 6. Horizontal transform

All eight axis and sign combinations are tested against the map limits, then a free scale is fitted. A transform is accepted only if the aircraft moved over 500 m on both axes, the residual is under 500 m, and the next hypothesis is at least 5 times worse. The transform is found from the data. The compass result predicts `map_x = +X, map_y = -Z`.

## 7. Labels

- Comparison figures are DERIVED cross-instrument results, recorded with both file hashes.
- A capture marked `synthetic` is EXPERIMENTAL and is never independent validation. Synthetic captures come from `examples/sim_capture.rs` and test the software only.
- A field match in the scan gives a scale, and meaning needs review.
