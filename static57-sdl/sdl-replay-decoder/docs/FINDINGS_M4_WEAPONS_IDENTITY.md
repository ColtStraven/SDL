# S57-003 SDL — Identity, Battle Log and Weapon Findings (Milestone 4)

Decoder: `sdl-replay-decoder` 0.3.0. Date: 2026-10-30.
Evidence base: 42 genuine client replays (27 from Milestones 1–3 plus 15 recorded 2026-10-29, game version field 2.59.0.92), all format `0x00018C1C`. See [COMPATIBILITY.md](COMPATIBILITY.md).
Reproduce: `sdl-replay-decoder research-survey <Replays folder>`; research probes are in `examples/`.

Classes as in [FORMAT_FINDINGS.md](FORMAT_FINDINGS.md). "SDL finding" marks results that differ from, or are not in, the third-party references.

## 1. Weapon trajectories: MPI `0xF0DB` (`ShellsData`) — `src/shells.rs`

| Field | Binary location / type | Scale | Meaning | Class | Validation |
|---|---|---|---|---|---|
| Message | MPI id `0xF0DB`, object `0x5802` | — | Client-replay in-flight stores. The server equivalent `0xF11A` does not occur in client replays | VERIFIED | 93,810 messages in 34 of 42 replays |
| Length | body starts with varint bit length | bits | Payload length | VERIFIED | 93,810 of 93,810 messages parse to exactly the declared length |
| Lists | three `u8`-counted lists | — | List 0 mainly rockets/missiles, 1 bombs, 2 torpedoes (from ECS templates) | DERIVED | 256,513 records |
| Weapon id | `u32le` after two flag bits | `entity_index << 10 \| (generation + 2)` | Links to the ECS creation record | VERIFIED (SDL finding) | The reference reads it as a compact id; that fails 88% of messages. A plain u32 consumes all of them. Link confirmed on creation records |
| Position | flag bit, then `f32[3]`, or two `u32` packed 22/20/22 bits over ±15 km | m | World position | VERIFIED | Matches launch position and aircraft geometry; movement agrees with velocity |
| Velocity | three `i16le` | 2000/32767 m/s per step | World velocity | VERIFIED | Direction vs measured motion: median 0.02°, p99 0.18° |
| Timer 0 | varint, `(v − 1) / 48` | s | Time since launch | VERIFIED | Advances with packet time in 99.8% of 250,449 steps |
| Other timers, rates, `direction`, index, seeker block | see `src/shells.rs` | — | Not established | UNKNOWN | Parsed for structure, preserved in part; seeker blocks are skipped |

**Limits:** sample spacing varies from about 20 ms to over 1.5 s. The last sample is not an established impact. Weapon speed in this message is not capped at 500 m/s (up to 1,689 m/s observed), unlike the aircraft speed field.

## 2. ECS framing: packet type 6 — `src/ecs.rs`

| Finding | Class | Evidence |
|---|---|---|
| Creation packets `0x24` (plain) and `0x25` (LZ4 block): count byte, then entity id, varint block length, block | VERIFIED | 32,121 creation packets parse with 0 failures |
| Block: varint template id; on first use, name and component table | VERIFIED | Same. Template names include `rocket`, `bomb`, `torpedo`, `rocket+arh_shell`, `aircraft+player_unit` |
| Component values | not decoded | Only fixed offsets below are used |

## 3. Store creation (`rocket`, `bomb`, `torpedo`, `rocket+...`) — `src/weapons.rs`

| Field | Location | Class | Evidence |
|---|---|---|---|
| Launch position | three `f32`, 11 bytes after the `01 00` marker (marker found at byte 6–10) | DERIVED (SDL finding) | Agrees with the first in-flight sample and with launchers. 5,525 of 5,817 weapons have one (the other 292 have no creation record) |
| Bytes before the marker | raw `creation_prefix` | UNKNOWN | **Not a weapon type:** one missile model shows many prefixes. Tested and rejected as an owner uid too |

Weapon ids are reused within a replay. SDL starts a new track when the timer runs backwards or after 15 s of silence.

## 4. Aircraft identity — `src/units.rs`

| Field | Location in `aircraft*` creation data | Class | Evidence |
|---|---|---|---|
| Unit uid | `u16le` at byte 30, after marker `30 70` at byte 28 | VERIFIED (SDL finding) | Vehicle names agree with the battle log (below) |
| Spawn position | three `f32` at byte 11 | DERIVED | Near the first sync record in 62% of checks; many tracks begin mid-life, so the check does not always apply |
| Vehicle, slot, loadout | first three length-prefixed strings after byte 32 | DERIVED | `f-86f-40_japan`, `st_t2_sentinel_01_1`, `f_86_hvar_rockets` |
| Author marker | template contains `controlled_hero` | REPORTED | Seen only sometimes. SDL identifies the author from the sync trailer instead |
| `dummy_plane` | pre-spawn placeholder holding a uid | DERIVED | 893 entities. Excluded from identity |

Vehicle agreement across the 42 replays: kill records 884 match, 91 mismatch; damage records 439 match, 29 mismatch. Uids are reused, so a vehicle is resolved at a time. Per-track confidence is reported: `battle_log_confirmed` 386, `spawn_match` 965, `battle_log_only` 114, `registry_only` 1,812, `conflict` 44, `unresolved` 302.

Note: `ariete` is the Aerfer Ariete jet, not the tank of that name.

## 5. Battle log — `src/combat.rs`, `src/idfield.rs`

Id-field messages: `offset:u16`, `mask:varint`, fields, then 3-bit size codes at `offset`.

| Message | Field | Type | Meaning | Class |
|---|---|---|---|---|
| `0xF058` kill | 1 | u32 | Killer slot | REPORTED |
| | 2 | len8 string | **Killer's** vehicle | VERIFIED against the registry |
| | 3 / 4 | u16 | Victim / killer uid (low 11 bits) | VERIFIED |
| | 10 | len8 string | Weapon name, e.g. `us_agm_65b` | DERIVED (matches launcher and geometry) |
| `0xF056` critical, `0xF157` severe | 1 / 4 | u16 | Victim / attacker uid | REPORTED |
| | 3 | len8 string | **Attacker's** vehicle | VERIFIED (SDL finding) |

**SDL finding:** third-party documentation calls field 3 of the damage record the victim's vehicle. It is the attacker's: 439 agree with the attacker's unit, while the victim reading matched 4 of 674.

2,051 kill records decode with 0 parse failures. Their weapon strings cover 73 distinct names in kill-linked shots.

## 6. Weapon joins — `src/weapons.rs`

These are DERIVED associations, not decoded fields.

| Join | Method | Coverage |
|---|---|---|
| Launcher | Nearest aircraft within 150 m of the creation position at creation time; or the killer of a linked kill | 2,247 of 5,817 |
| Author weapons | Launcher is an author uid | 159 |
| Kill link | Each kill picks the nearest track end within −3 s/+1.5 s and 400 m of the victim, whose launcher can be the killer | 285 of 2,051 kills |
| Name | The linked kill record's weapon string (two linked kills carry none) | 283 weapons |
| Target at launch | The linked victim's position at launch time | Linked weapons only |

**Limits that matter for LAR work:**

1. **Success bias.** Names exist only for kill-linked weapons. Misses are unnamed, and their target is unknown. Nothing here estimates Pk or an envelope edge.
2. The 3,570 weapons without a launcher include ground launches. Ground unit positions are not decoded.
3. A kill link is a candidate. One VT-1 shows 0.23 s flight and 12 m range: that link is not credible as a causal hit.
4. The target of an unlinked weapon is unknown. Sensor and seeker records are not decoded.

## 7. Altimeter scale — `src/cockpit.rs`

**SDL finding:** feet altimeters use 3.28 ft per metre. Decoded altitude fits `y = 0.304877 × ft + 0.03 m` over 4–8 km. With the exact 0.3048 the error grew linearly to 2 m at 8 km; with 1/3.28 the worst per-replay median across 31 replays is 0.042 m. The Milestone 3 figures in `FORMAT_FINDINGS.md` that used 0.3048 are superseded.

Strike Master relevance: any feet-to-metres conversion of game altitude should be checked against this factor. This is an observation for review, and nothing has been changed.

## 8. Other observations

- Sharp speed changes above 60 m/s per second appear on 31 author windows, with engine state moving to 8 shortly after in the cases inspected: collision, break-up or impact candidates. They are reported as anomalies and excluded from maneuver statistics.
- Engine start-up steps through states 0–5 then 7, matching the third-party description.
- The aircraft speed field saturated on 15,613 samples across the corpus.

## 9. Not decoded

Weapon type field, aircraft controls and engine scaling (raw only), sensors and radar, missile seeker blocks, countermeasures, ground unit positions (`0xF073`), and ECS component values. See section 8 of any research report for the current inventory.
