# Strike Master Recommendations from Milestone 7

Status: **proposals for owner review.** SDL changed nothing in Strike Master or its databases. Evidence is in [FINDINGS_M7_FLIGHT_SYSTEMS.md](FINDINGS_M7_FLIGHT_SYSTEMS.md): two aircraft, one capture, 638 s of aligned flight.

Strike Master references below come from a read-only look at its source on 2026-10-30.

| Area | Finding | Recommendation | Readiness |
|---|---|---|---|
| Navigation | Map transform confirmed: x east, y down, with scale from `map_min`/`map_max` (RMS 8 to 10 m) | Keep the frame assumed in `src/core/geo.ts`. It can now cite measured evidence | Ready to adopt as evidence |
| Navigation | Map scale error under 0.2% on two flights | Distance and bearing from map fractions can be trusted to that level, on these maps | Needs more maps |
| HUD altitude | `H, m` agrees with replay altitude to about 0.5 m | Keep `H, m` as the altitude source | Ready |
| HUD altitude in feet | `src/shared/units.ts` uses 3.28084 ft/m. Cockpit feet altimeters in the game use 3.28, per earlier replay findings | At 30,000 ft the two differ by about 8 ft. Decide whether the HUD should be physically exact or match the cockpit. Label the choice | Needs a feet-altimeter capture first |
| HUD speed | IAS is exact to rounding. Ground speed and TAS differ by up to about 2 m/s (p99) | Keep IAS, TAS and ground speed as separate labelled values. Do not substitute one for another | Ready |
| HUD attitude | Pitch and roll within 0.2° (median). 8111 pitch sign is opposite to the replay's | No change indicated. Check that any replay-driven display flips pitch | Ready |
| Compass | 8111 `compass` equals world heading + 90, within 0.1° | Compass needs no map-specific correction on these flights | Needs more maps |
| Mission timing | ETA needs ground speed, and 8111 gives TAS. The difference was under 1 m/s (median) | TAS is a fair ETA input in light wind. Wind has not been measured, so show ETA as an estimate | Provisional |
| Fuel prediction | Replay cockpit id 195 is fuel mass in kg, so every replay holds a fuel history | SDL can build burn tables by aircraft, altitude and throttle from existing replays, for review against Strike Master's fuel logic | Next research step |
| Performance models | Author load factor, throttle, RPM and fuel are now available together across replays | Propose level acceleration and sustained-turn runs on the controlled procedure. Combat data shows lower bounds only | Needs controlled flights |
| Flight-test reporting | `telemetry-validate` gives provenance, alignment and error figures in one report | Attach this report to a Strike Master test sortie as the independent record | Ready |
| Avionics calibration | Gear, flaps and throttle replay fields follow 8111 | Replay data can check Strike Master's logged state after a sortie. It is not for live use, because the file only completes after the battle | Provisional |

**Not supported:** weapon envelope, hit probability or LAR calibration. SDL still does not classify misses, and nothing here changes that.

**A fleet caveat:** instrument fields vary by aircraft. Strike Master's `fields.ts` already treats fields as optional, and that should stay.
