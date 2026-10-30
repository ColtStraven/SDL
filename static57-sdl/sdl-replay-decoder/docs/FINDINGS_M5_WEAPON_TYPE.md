# S57-003 SDL — Weapon Type Findings

Decoder: `sdl-replay-decoder` 0.4.0. Date: 2026-10-30.
Evidence base: 42 genuine client replays, format `0x00018C1C`, game version field 2.59.0.80–92.
Reproduce: `sdl-replay-decoder research-survey <Replays folder>`. Hold-out counts are in each `research-data.json` under `weapons.stats`.

## Result

SDL now names **3,284 of 5,817** tracked weapons, up from 283, including shots with no linked kill. Of 159 author releases, 52 are named. No direct type field has been decoded: a name is inferred from kill records through a per-replay type id.

| Source | Weapons | Evidence |
|---|---|---|
| Kill record linked to this weapon | 280 | DERIVED |
| Same type id as a kill-named weapon in this replay | 3,004 | DERIVED |
| Unavailable | 2,533 | UNKNOWN |

## The type id

| Property | Finding | Class |
|---|---|---|
| Location | First varint of the store creation data, at byte 3 (`rocket`, `bomb`, `torpedo`, `rocket+...`) | VERIFIED as a type grouping |
| Scope | **Per replay.** The same value means different weapons in different replays, so a name never carries across replays | VERIFIED |
| Allocation | Consecutive blocks per loadout. In one replay, 297 is the JA37DI's non-radar missile and 298–299 its RB 99s; 235 is the F-16XL's AIM-9L and 236–237 its AIM-120s | DERIVED |
| One weapon, several ids | Normal: a single missile model can hold several ids in a replay | DERIVED |
| Name table | Not found. Nothing in the replay has yet been shown to map an id directly to a name | UNKNOWN |

The remaining prefix bytes are preserved in `creation_prefix`; their meaning is not established.

## Validation

**Hold-out.** For each weapon named by a kill record, the name was hidden and recovered from other kill-named weapons of the same id.

| Method | Correct | Attempted | Decision |
|---|---|---|---|
| Shared type id | 160 | 162 (98.8%) | Accepted |
| Launcher equipment list, single candidate | 3 | 24 (12.5%) | **Rejected:** candidates only, never a name |

The equipment list names weapons a unit can carry, in the same form kill records use, but it does not identify which one was fired. It is exported as `weapon_candidates`, class UNKNOWN.

**Flight physics.** For the 19 weapons with at least 3 examples from each source, median top speed was compared between kill-named and type-id-named shots. All 19 agree within ±25%, and 14 within 3%.

| Weapon | Kill-named: shots, median top speed m/s | Type-id-named: shots, median top speed m/s |
|---|---|---|
| `us_mim_23b` | 17, 731 | 362, 732 |
| `swd_rb99` | 6, 940 | 40, 937 |
| `163mm_roland_3` | 29, 561 | 468, 559 |
| `508mm_5v55r` | 4, 1,669 | 165, 1,689 |
| `us_aim9l_sidewinder` | 13, 775 | 52, 731 |
| `us_aim_120a` | 4, 832 | 48, 956 |

**Structure.** No type id spans two store templates (0 of 542).

**Launcher consistency.** 19 of the 367 ids with a known launcher vehicle show more than one vehicle. At least some are proximity errors: `f-5t_thailand` appears as a launcher of a Roland SAM because it was within 150 m of the launch.

A disputed id, where no name holds two thirds of its kill records, stays unnamed and lists its names as candidates.

## Corrections made during this work

- **Gun rounds:** `ap_i` and similar had been linked to passing missiles. Kills by gun rounds no longer link to stores, which removed 3 links.
- **Seeker block:** nearly all stores have a block in the seeker position, bombs included. It does not mean guided, so the field is exported as `has_seeker_block`.

## What this changes for LAR work

Named misses and non-kills are now available, so launch conditions can be studied for all shots of a type, not only successful ones. Three limits remain:

1. **An unknown outcome is not a miss.** Kill records cover kills only: a hit that only damaged the target, or a kill that failed to link, reads as unknown.
2. **Target distance exists only for kill-linked shots.** Other shots have no established target until sensor or seeker records are decoded.
3. **Unnamed types are not random.** A type with no kill in a replay stays unnamed, so named data still leans toward effective weapons. Most of the 107 unnamed author shots fall here.

## Next steps

- Decode ECS store components for a direct name or type, which would remove the dependence on kills.
- Establish targets from sensor and seeker records.
