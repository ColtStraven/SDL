# S57-003 SDL — Weapon Engagement Findings (Milestone 6)

Decoder: `sdl-replay-decoder` 0.5.0. Date: 2026-10-30. Corpus: 42 replays, 42 decoded ok, 5,817 weapons.

Status: **work in progress on branch `milestone-6`, not merged.** The launcher, identity and naming work below is complete. Miss classification was removed and is not part of this milestone.

Reproduce: `sdl-replay-decoder research-survey <Replays folder>`. The figures come from `research-summary.json`, each replay's `research-data.json` under `weapons.stats`, and `examples/probe_launcher.rs`.

## 1. What is measured and what is inferred

| Item | Class | Basis |
|---|---|---|
| Weapon position, velocity, time since launch | VERIFIED | Exact bit-length parsing of `ShellsData` |
| Launcher entity | VERIFIED | Read from the store creation record, then checked against kill records (§2) |
| Launcher vehicle, loadout, author flag | VERIFIED | Follow from the entity through the unit registry |
| Launch geometry | DERIVED | Calculated from tracks at launch time, for a target that is itself inferred |
| Seeker target | DERIVED | Inferred association. Seeker data matched to an aircraft track |
| Kill-record target | DERIVED | Inferred association. The victim of a linked kill |
| Weapon name | DERIVED | Inferred association. From a kill record, or a shared type id |
| Designated target candidate | UNKNOWN | Candidate only. Never sets a target |
| Kill, damage and hit classes | DERIVED | Inferred association between a record and a weapon. A link is not proof of cause |
| `unknown` outcome | UNKNOWN | Nothing established. Not a miss |

## 2. Launcher entity association

Store creation data carries the launcher as a compact entity id, between the per-replay type id and the `01 00` marker, in the 2-, 3- or 4-byte Dagor form. This replaces the Milestone 4 proximity attribution, which has been removed.

| Result | Weapons |
|---|---|
| Launcher from creation record | 5,523 of 5,817 (94.9%) |
| Aircraft | 2,402 |
| Ground units | 2,929 |
| Ships (entity and template known, no uid decoded) | 192 |
| Entity id not found | 2 |
| Weapon seen with no creation record | 292 |

All 35,813 store creations were tested, and 35,808 name an entity that exists. Respawns are separated by the generation in the id.

**Author weapons.** All 159 author weapons from Milestone 5 are attributed by entity id, with none lost and none gained.

**Launch point to launcher.** For aircraft launchers, p50 36 m, p90 69 m, p99 131 m, n 2,132. Four exceed 500 m (646 to 1,673 m) and are not explained. They are reported as measured.

**Late creation.** 106 weapons have a creation record more than 2 s after their computed launch, so that position is mid-flight. SDL now withholds their launch position, launch altitude, launcher distance and launch geometry, and sets `trajectory.launch_late`. The launcher entity is still reported. This removed the outliers up to 38 km seen before the check. The cause of late creation is not established.

**Independent check.** For kills by a non-gun weapon on an aircraft, take the weapon that ended nearest the victim with no launcher constraint, and compare its launcher with the kill record's killer.

| Result | Kills |
|---|---|
| Tested | 299 |
| Launcher uid agrees | 254 |
| Differs | 19 |
| Launcher is a ship with no uid | 26 |

The check is deliberately crude, so a disagreement is not proof of a launcher error. Of the 19, 14 have a weapon name that does not match the kill and a different launcher model, so a different weapon was nearest. The other 5 have the same model and weapon name, so a sibling launcher's missile was nearest. No case shows a creation record naming the wrong unit, but this check alone cannot rule that out. A launcher's relationship to its weapon has not been checked against a source outside the replay.

**Comparison with Milestone 5.** Of weapons with a launcher uid in both versions, 2,205 agree and 14 differ. A further 27 were credited by proximity with no uid under the entity method, and 3,301 are newly attributed. The 14 are false associations that the old method produced, for example `av_8b_na` credited with an R-60MK fired by `mig-21_bis_lazur`.

## 3. Unit identity

The uid marker is `30 70` on aircraft and `38 70` on ground units, normally at byte 28 and sometimes later. Scanning bytes 24 to 48 recovered 724 launchers that a fixed offset missed, and reduced kill vehicle mismatches from 91 to 1 and damage mismatches from 29 to 0.

| Check | Match | Mismatch | Unresolved |
|---|---|---|---|
| Kill record vehicle | 1,211 | 1 | 839 |
| Damage record vehicle | 628 | 0 | 279 |

## 4. Weapon-name hold-out

Milestone 5 had two disagreements, and fixing the launcher exposed two more. All four were investigated as data quality. The cause each time was the kill link supplying the name, not the type id.

| Case | Label from kill | Type id said | Cause |
|---|---|---|---|
| `14.23.19 w3223.2` | `72mm_9m39` | `209mm_9m33m3` | Old link credited launcher 731. The entity id says 732, a different vehicle. Kill link no longer forms |
| `14.23.19 w4399.3` | `209mm_9m33m3` | `400mm_9m38m1` | Old link credited launcher 275. The entity id says 189. Kill link no longer forms |
| `14.23.19 w5232.0` | `apcr` | `us_agm_65b` | A gun round was not recognized, so a gun kill linked to a passing missile. Fixed in `is_gun_round` |
| `15.03.43 w4177.5` | `su_9M127` | `su_kh_29te` | A Kh-29TE and a 9M127 from the same aircraft both ended near a surface victim. An ambiguous link supplied the wrong name |

The last case led to a rule: **an ambiguous kill link no longer supplies a name.** The link itself remains in the record with class `ambiguous`.

| Stage | Hold-out |
|---|---|
| Milestone 5 | 98.8% (reported then) |
| Entity launchers, before the fixes | 417 of 419 |
| After the `apcr` fix | 417 of 418 |
| After the ambiguous-link rule | **306 of 306** |

306 of 306 is not proof that the method is free of error. The rule removed 112 test cases, one of them the failure, and the test still relies on kill links as its ground truth. Types that never killed cannot be tested at all.

Current naming: 3,982 of 5,817 weapons, 429 by kill record and 3,553 by type id. 1,835 are unnamed. Against Milestone 5, 2 names changed, 202 were lost and 900 gained.

## 5. Target research

Existing methods and classifications are preserved. No new targeting method was added.

| Method | Weapons | Class |
|---|---|---|
| Seeker observation | 1,306 | DERIVED |
| Kill record only | 396 | DERIVED |
| Launch geometry available | 1,393 | DERIVED |

Claims tested on kill-linked weapons, with the victim as reference:

| Method | Claims | Named the victim |
|---|---|---|
| Seeker line of sight | 144 | 142 |
| Seeker target estimate | 56 | 53 |
| Launcher designation (candidate only) | 13 | 10 |

Cautions:

- 109 of the 396 kill-record targets sit on ambiguous links. Treat them as weaker than the rest.
- These rates are measured only where a kill exists. They do not show accuracy on shots that did not kill.
- A target is the aircraft a seeker appears to observe. It does not establish an outcome.
- The seeker range field is rejected, with a median error of 692 m. It is not used.
- Designated points were only evaluated, and are not used. Kind 2 fell within 300 m of the weapon end in 52 of 119 cases, other kinds almost never.

## 6. Outcomes

| Class | Weapons | Meaning |
|---|---|---|
| `confirmed_kill` | 431 | A kill record from this launcher, with the weapon ending at the victim |
| `confirmed_damage` | 66 | A damage record from this launcher, with the victim at the weapon end |
| `confirmed_hit` | 2 | An author hit marker, with the target at the weapon end |
| `ambiguous` | 145 | Another weapon from the launcher ended about as close |
| `unknown` | 5,173 | Nothing established |

"Confirmed" means a battle-log record links to the weapon. It does not prove cause.

**Miss classification is withdrawn.** An earlier working result of 980 provisional misses was never validated and is not reported. The separation measurement it relied on has been removed from the code, and those weapons are now `unknown` with the rest. `unknown` contains hits, misses and everything else. SDL makes no miss claim, and no replacement method was attempted.

113 ambiguous weapons carry a kill link. The other 32 competed for a kill without winning it. Two links were rejected as physically impossible.

## 7. Limitations

- SDL has no miss classification, so shots cannot be used for hit rate, envelope limits or LAR calibration.
- Launchers, names, targets and links have no check from outside the replay. The procedure in [CONTROLLED_TEST_PROCEDURE.md](CONTROLLED_TEST_PROCEDURE.md) is prepared but has not been run.
- 294 weapons have no launcher. 292 of these have no creation record.
- 192 ship launchers have no uid.
- 1,835 weapons are unnamed, and unnamed types lean toward those that did not kill.
- Surface units have no decoded track, so a surface victim's position is its spawn point. The surface link limit is therefore 1,500 m, against 400 m for aircraft.
- Remote aircraft are network copies.
- X/Z are not tied to map north.

## 8. Tests

127 tests pass, including 3 genuine-replay tests, with no warnings on `cargo build --release --all-targets`. New fixtures name a launcher entity in the creation record and cover exact launchers, an unknown entity staying UNKNOWN with an aircraft at the launch point, respawn generations, ground launchers, the moved uid marker, impossible links, ambiguity, damage and hit classes, seeker blocks, designation candidates, and late creation.
