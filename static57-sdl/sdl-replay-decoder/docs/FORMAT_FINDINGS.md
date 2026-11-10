# S57-003 SDL — `.wrpl` Format Findings

Decoder: `sdl-replay-decoder` 0.1.0. Date: 2026-10-29, updated 2026-11-09 (container counts; new section 4, payload decoding); 2026-11-10 (§4.6 projectile hit events, §6 future research; 29th replay, §4.6.10 reproduction).
Evidence base: 29 genuine client replays recorded by the Static 57 owner, 2026-10-22 to 2026-10-25 and 2026-11-09 to 2026-11-10 (see [COMPATIBILITY.md](COMPATIBILITY.md)).

Classifications: **VERIFIED** (documented structure with a reproducible cross-check on Static 57 replays), **REPORTED** (external documentation only), **DERIVED**, **EXPERIMENTAL**, **UNKNOWN**.

## 1. Container

| Finding | Class | Evidence |
|---|---|---|
| Signature `E5 AC 00 10` (`u32le 0x1000ACE5`) at offset 0 | VERIFIED | All 29 files |
| Format magic at offset 4 = `0x00018C1C` (`1C 8C 01 00`) | VERIFIED | All 29 files |
| Fixed header is 1234 bytes; settings BLK begins at 1234 | VERIFIED | The zstd magic sits exactly at `1234 + settings_blk_size` in all 29 files |
| `settings_blk_size` is a `u16le` at offset 748 | VERIFIED | Same cross-check (observed values 682–1356) |
| `footer_blk_offset` is a `u32le` at offset 684 and marks the end of the packet stream | VERIFIED | In all 29 files the zstd frame ends exactly at this offset, with 0 trailing bytes |
| The footer/results BLK runs from `footer_blk_offset` to EOF | REPORTED | Starts with byte `0x01` and contains key names such as `timePlayed` and `authorUserId`; not decoded |
| The settings BLK leads with byte `0x01` | EXPERIMENTAL | All 29 files. Meaning of the byte not established |
| `session_id` is a `u64le` at offset 732 | REPORTED | Present and non-zero. Not cross-checked against server parts |
| `start_time` is Unix seconds (`u32le`) at offset 908 | REPORTED, plausibility DERIVED | Example: `1792795650` = 2026-10-23T22:47:30Z, which matches file name `#2026.10.23 15.47.41` in local time (UTC−7) with the save time 11 s later |
| `game_version` (`u64le` at 1226) holds four `u16le` words `[build, patch, minor, major]` | EXPERIMENTAL | Reads 2.59.0.80, then .83, then .86 across consecutive days, then 2.59.0.100 on 2026-11-09. Consistent with game updates, not documented anywhere |
| Client classification (`player_no != 0x80000000`, `segment_length_sec != 90`) | REPORTED | All 29 classify as client. No server replay available |

The string fields (level, mission, environment, weather, location) decode as plain NUL-terminated ASCII, for example `levels/air_denmark.bin`, `air_denmark_I2M`, `evening`, `cloudy`.

## 2. Compression

| Finding | Class | Evidence |
|---|---|---|
| Format `18c1c` uses Zstandard for the packet stream | VERIFIED | All 29 files |
| Exactly **one** zstd frame per client replay | VERIFIED (for these files) | All 29 files |
| The frame decodes with a pure streaming decoder; no content-size field is needed | VERIFIED | `ruzstd` streaming decoder |
| Compression ratio | DERIVED | 1.33× to 4.84× (2026-11-09 replay: 4.10×; 2026-11-10: 4.06×) |
| zlib for `18c0b` / `18c0a` | REPORTED | No genuine sample. zlib support is tested only on synthetic fixtures |

## 3. Packet framing (decompressed stream)

| Finding | Class | Evidence |
|---|---|---|
| Big-endian variable-length size prefix (see `src/packets.rs`) | VERIFIED | 4,577,991 packets frame with **zero** leftover bytes. Every replay ends with an end marker as its final record |
| Body = `u16le type`, then a `u32le` millisecond timestamp unless `type & 0x10` | VERIFIED | Same accounting. Explicit timestamps never go backwards in any file |
| Prefix forms observed | VERIFIED | 1-byte: 2,618,783. 2-byte: 1,958,966. 3-byte: 242. No 4- or 5-byte prefixes seen |
| **Non-canonical prefixes**: 21.7% of prefixes are longer than the shortest encoding (for example, size 6 written as `40 06`) | VERIFIED (new finding) | 994,498 of 4,577,991. The decoder accepts and counts them. The likely cause is the writer reserving prefix space before the body size is known (EXPERIMENTAL) |
| No bits outside `0x1F` are set in any type field | VERIFIED | `type_high_bits_packets = 0` in all files |
| The running time starts at 0. Packets that inherit time before the first explicit timestamp get 0 ms | EXPERIMENTAL | 2 replays recorded from session start. Their first explicit times are 27 ms and 32 ms, after 328 and 352 inherited packets. Output marks these `inherited_initial` |
| Timestamps are session time, not replay-relative | DERIVED | First explicit timestamps range from 27 ms to 7,294,000 ms. Replays started mid-session begin at large values |

### Packet type inventory (all 29 replays)

| Type | Name (REPORTED) | Packets | Payload bytes | Payload min..max | Notes |
|---|---|---|---|---|---|
| 0 | end_marker | 29 | 0 | 0..0 | Always the last record (VERIFIED) |
| 1 | start_marker | 29 | 0 | 0..0 | One per replay |
| 2 | aircraft_small | 109,583 | 260,619,427 | 17..5,596 | Flight-model sync (§4.3). Syncs under 36 bytes occur only in the two build-.100 replays (19 bytes on 2026-11-09, 17 on 2026-11-10), before the author spawns; the earlier 27 never went below 36 |
| 3 | chat | 417 | 27,919 | 23..154 | In 18 of 29 replays |
| 4 | mpi | 3,966,741 | 330,528,096 | 4..27,726 | Most frequent type |
| 6 | ecs | 501,163 | 53,138,338 | 4..208,943 | |
| 8 | replay_header_info | 29 | 8,700 | 300..300 | Always first; payload always 300 bytes |
| 5, 7 | next_segment, snapshot | 0 | — | — | Not present in client replays. Reported for server parts |

Type names come from third-party documentation. `packets.jsonl` keeps every record's `decode_state` as `raw`; decoded payload content is written separately by the `flight` command (section 4).

## 4. Payload decoding (added 2026-11-09)

Evidence base: replay `#2026.11.09 18.50.56` (build 2.59.0.100, Me 262 A-1a/U4, `air_vietnam`). Decoder module `flight` / `mpi`, command `sdl-replay-decoder flight`. Sections 4.1–4.6 were first established on that replay. A second replay, `#2026.11.10 04.23.23` (same build, aircraft and mission), reproduces §4.3 and §4.6 (see §4.3 and §4.6.10). Both are the same aircraft and mission, so every finding still needs confirmation on other aircraft, maps and builds.

### 4.1 Bit order

| Finding | Class | Evidence |
|---|---|---|
| Packed payloads are read **MSB-first within each byte**; multi-byte values are 8-bit groups assembled **little-endian** | VERIFIED | An `f32` at the non-aligned bit 129 of `aircraft_small` matches the cockpit altimeter in 71 of 71 sampled packets. LSB-first and big-endian readings match at no fixed offset |

### 4.2 MPI message signatures

| Finding | Class | Evidence |
|---|---|---|
| Client MPI messages start `02 58 <id lo> <id hi>` or `03 58 <id lo> <id hi>` | REPORTED, consistent | Every message family below |
| `0xD04A ReplayCockpitParams` = `03 58 4A D0`, LEB128 bit length, `u16le count`, `count × (u16le id, f32le value)` | VERIFIED | 2,495 of 2,495 messages: declared length equals `16 + 48·count` bits and ends exactly at the payload end |
| Cockpit ids are the localhost:8111 `/indicators` gauges, sent about every 250 ms. Full id→key table in `src/mpi.rs` (`COCKPIT_PARAMS`) | VERIFIED (this replay and aircraft) | Each id matches one `/indicators` field in the simultaneous Phosphor capture (§4.5) |
| id 0 is **indicated** airspeed (8111 `speed`), m/s | VERIFIED | RMS 0.06 m/s against `speed`; 10.3 m/s below 8111 TAS on average |
| id 40 altitude (m above sea level), id 39 vertical speed (m/s, up positive), id 64 compass (deg clockwise from north) | VERIFIED | RMS 0.23 m, 0.17 m/s, median 0.01° |
| ids 52 and 53 are roll and pitch in degrees, **both inverted** (positive = left wing down / nose down) | VERIFIED | RMS 0.32° and 0.11° against raw `aviahorizon_roll` / `aviahorizon_pitch`. Also `−IAS·sin(id 53)` correlates with id 39 at r = 0.982 (DERIVED) |
| Engine pairs on this twin-engine aircraft: 81/82 RPM, 129/130 oil pressure, 141/142 water temperature, 240/241 throttle; 195/196 are **fuel mass (kg)**, not temperatures | VERIFIED | Phosphor A.2, reproduced for ids 0, 39, 40, 52, 53, 64 |
| id 273 appears only near shots (13 records, each within 0.2 s of a counter step) | EXPERIMENTAL | Probably a firing indicator; left unnamed |
| ids 228–230 read 1.0 only while parked; records are sparse (28–40 of 41 ids per record) | EXPERIMENTAL | Probably gear lamps; zero-valued ids appear to be omitted |
| The cockpit stream does **not** carry 8111 `/state` values (TAS, AoA, sideslip, Ny, Mach) | VERIFIED (this replay) | No id matched them |

### 4.3 `aircraft_small` leading record (the replay author)

| Bits | Field | Class | Evidence |
|---|---|---|---|
| 0–37 | Constant `11010110 10000001 11110101 10000000 011000` | VERIFIED (constant) | All 2,494 post-spawn syncs. The 153 pre-spawn 19-byte syncs differ and carry no ownship record |
| 38 | Flag, REPORTED as on-ground | EXPERIMENTAL | Clears 69 m above the field after take-off and sets 28 m above it on approach: better matches landing-gear state than wheel contact |
| 39 | Unknown, always 0 | UNKNOWN | |
| 40–71 | `u32`, always 0 here | UNKNOWN | REPORTED as sometimes `0x3E555556` |
| 72 | Firing flag | EXPERIMENTAL | Set in 12 packets, each during a counter decrement |
| 73–76 | Trigger bits; `0100` while firing | EXPERIMENTAL | |
| 77–79 | Always `001` | UNKNOWN | Decoder refuses other values (`unexpected_layout`) |
| +8 | Present only when firing; always `00000001` | UNKNOWN | Shifts everything after it by 8 bits |
| u8 | Counter: starts at 32, falls by exactly one per step, 16 steps, all during firing bursts | EXPERIMENTAL | Consistent with the ammunition count of one gun group |
| 9 bits | Always 0 | UNKNOWN | |
| 3 × f32 | **World position `(x, y, z)` in metres; `y` is height above sea level, +x east, +z north** | VERIFIED (this replay; `y` also on replay 2026-11-10) | `y`: 2,494 of 2,494 within 20 m of the cockpit altimeter (median 0.45 m, max 11.3 m); replay 2026-11-10: 3,141 of 3,141 (median 0.45 m, max 5.5 m), 0 speed discontinuities. `x`/`z`: RMS 2.2 m, max 3.4 m against all 113 independent 8111 map positions (§4.5). 0 speed discontinuities; max derived speed 214 m/s against max IAS 204 m/s |
| last 96 bits (+0–7 padding) | Second copy of the author position | VERIFIED | 2,494 of 2,494 within 1 cm of the record position, 748 bit-identical (REPORTED by WT_KNOWLEDGE_DOCS as the trailing author position) |

The records after the first one (other aircraft) are **not decoded**. Their sizes give the plateau pattern in the packet-size timeline.

### 4.4 Combat records (client signatures)

| Finding | Class | Evidence |
|---|---|---|
| `0xF058` kill record contains the length-prefixed (`u8`) vehicle name `me-262a-1a_u4` followed by `u16` 2332 and 428 | EXPERIMENTAL | One record, at 426.265 s, 0.36 s after the 16th counter step. Field order (killer vehicle, victim id, killer id) is REPORTED |
| `0xF056` damage record contains `me-262a-1a_u4` and `ffff` | EXPERIMENTAL | One record, at 379.144 s, inside the 374–380 s firing burst |
| `0xF118` hit markers | — | None in this replay |
| The kill victim is probably an enemy SPAA | EXPERIMENTAL | Phosphor: the only 8111 map object to disappear between 425.35 and 426.36 s was a red SPAA at (2,966, −12,278), dead ahead of the author about 450 m away |
| The counter is the `cannonMk214a` ammunition (32 rounds) | REPORTED | Datamine of build 2.60.0.35, not the replay's 2.59.0.100 (via Phosphor). 8111 gives no ammunition count for this aircraft |

### 4.5 Independent validation: Phosphor localhost:8111 capture

Project Phosphor recorded localhost:8111 during the same flight (handoff `project_phosphor/handoff/to_sdl/2026-11-09_18.50.56/`, files checked against the handoff's SHA-256). 8111 answered only between replay times 373.7 and 487.4 s, so these checks cover that window: 431 `/state`+`/indicators` samples and 113 `/map_obj.json` positions. SDL reproduced the comparisons below itself from its own `flight` output, with linear interpolation.

| Check | Result | Class |
|---|---|---|
| Clock: replay session time = 8111 request time + **124.05 s** | Best fit of cockpit id 40 to `altitude_hour` (RMS 0.23 m at 124.05 s; 1.86 m at ±0.05 s). Map positions fit best at 124.06–124.07 s | DERIVED |
| Ownship `x`/`z` vs 8111 map position (113 samples) | RMS 2.21 m, median 2.17 m, max 3.37 m | VERIFIED |
| Track heading `atan2(Δx, Δz)` vs 8111 map heading | Median difference 0.24° (larger only in turns, from finite differencing) | VERIFIED: +x east, +z north, assuming the 8111 map is north-up |
| Ownship `y` vs `altitude_hour` / `/state H` | RMS 0.24 m / 0.91 m (`H` is rounded to 1 m) | VERIFIED |
| Firing: 8111 `weapon2` = 1 only inside SDL's firing bursts | Phosphor §4.3 | VERIFIED (by Phosphor) |
| Replay `start_time` vs capture wall clock | Implies 118.5 s, not 124.05 s: a ~5.5 s gap | UNKNOWN cause; needs other replays |

### 4.6 Projectile hit events (added 2026-11-10)

**Scope and provenance**

| Item | Value |
|---|---|
| Replay | `#2026.11.09 18.50.56.wrpl`, SHA-256 `84aff3c26e7bca3306d1df31e2d308337fdd97eebf52bb01cf0d1556180a183f` |
| Game build | 2.59.0.100 (header `game_version`; word order EXPERIMENTAL, §1) |
| Format | magic `0x00018C1C`, client replay |
| Aircraft | `me-262a-1a_u4` |
| Decoder | `sdl-replay-decoder` at commit `a6c961d`. **None of this section is implemented in the decoder.** It comes from read-only analysis of the decoder's packet export |
| Reproduce | Run `sdl-replay-decoder packets <replay> --payloads`, then read the type-4 (MPI) records. Use `sdl-replay-decoder flight <replay>` for `ownship.jsonl` (the aircraft track and counter) |
| Analysis | One replay, one aircraft, one weapon. Everything below needs confirmation on more replays |

**Classes in this section.** VERIFIED is used **only** for raw packet observations: byte or bit values, counts, offsets, identity of values across messages, and direct numeric agreement with SDL's own decoded track. Every interpretation is EXPERIMENTAL. Reconstructed launch times, flight times, velocity units and muzzle speeds are DERIVED, and none of them is an independently verified measurement.

#### 4.6.1 MPI object addressing (needed to find these messages)

| Observation | Class | Evidence |
|---|---|---|
| An MPI payload starts with `u16le object_id`. When `object_id & 0x7FF == 0x7FF`, a LEB128 extended uid follows. Next comes `u16le message_id`, then the message body | VERIFIED (consistent framing) | All 212,330 MPI packets in this replay split into 45 message ids under this rule. The layout is REPORTED by WT_KNOWLEDGE_DOCS §2.4 |
| `object_id >> 11` gives the object kind. Hit messages use `0x0FFF` (kind 1, ground unit) | REPORTED kinds; VERIFIED values | |
| The two targets have extended-uid bytes `81 d0 11` (target **A**) and `81 e8 12` (target **B**) | VERIFIED (raw bytes) | How these relate to unit ids elsewhere (e.g. kill-record victim 2332) is UNKNOWN |

Below, "body offset" means bytes or bits counted from the first byte after `message_id`. Bit-level fields use the §4.1 convention: MSB-first bits, values assembled little-endian.

#### 4.6.2 Projectile key

| Observation | Class | Evidence |
|---|---|---|
| A 6-byte key `u16le offender, u8 generation, u16le index, u8 0` at body byte 3 of `0xF117`, `0xF13C`, `0xF0C2` and `0xF133`, and at body byte 5 of `0xF0E9` | VERIFIED (raw) | Same bytes in every message of a hit, e.g. `ac 01 01 d0 27 00`. The field names are REPORTED (WT_KNOWLEDGE_DOCS "projectile key") |
| Offender `0x01AC` = 428 is the replay author | EXPERIMENTAL | 428 is the killer-id value in the `0xF058` kill record (§4.4). It is the only offender in hits during the author's firing windows |
| **A key identifies one projectile** | EXPERIMENTAL (strongly supported) | (a) Each author hit has a distinct key, and every message of that hit carries it. (b) The key reappears only in the kill-time `0xF0C2` repeat of the same hit. (c) Another shooter's single key (`fe 01 03 ac 1c 00`, 526.300 s) lands on 7 ground units in the same millisecond, consistent with one projectile's splash |
| **Keys are not recorded at firing** | VERIFIED (absence) | None of the 9 author keys occurs before its hit in any type 2, 4 or 6 payload, at any bit offset. There is no spawn record carrying the key |
| **The index is not a sequential shot counter** | VERIFIED (raw) / EXPERIMENTAL (meaning) | Across all 587 distinct keys in the replay, every index is a multiple of 4. One shooter produces different generations 24 ms apart (`0x0201`: gen 02 at 531.244 s, gen 03 at 531.268 s). The author's burst-B indices run 9048, 9228, 9192, 9236 in hit order. Most likely a slot in a shared projectile pool, with a per-slot generation |

#### 4.6.3 Hit-message cluster

Each author hit appears as consecutive MPI packets on the target object, all with the same timestamp:
- `0xF0E9` (44–45-byte body).
- `0xF117` (17-byte body) or `0xF13C` (18-byte body).
- `0xF0C2` (199–556-byte body).

The hit at 425.660 s has no `0xF117` or `0xF13C`. At the kill (426.265 s), `0xF04B` (empty body) and a byte-identical repeat of the last `0xF0C2` follow the `0xF058` kill record.

| Raw observation | Class |
|---|---|
| `0xF117` body: `0e 00 0e`, key, `u8`, `f32le` (412.6, 826.8, 437.4, 1572.0 …), `06 0a 00` | VERIFIED (raw). The `u8` and `f32` are REPORTED as damage class and amount; this is not checked |
| `0xF13C` body: `0f 00 1e`, key, 2 bytes, `f32le`, `06 09 40` | VERIFIED (raw); meaning UNKNOWN |
| `0xF0E9` body: `29 00 02 9b 02` or `2a 00 02 a3 02`, key, then about 30 bytes | VERIFIED (raw); meaning UNKNOWN |

#### 4.6.4 `0xF0C2` coordinate block

The block starts at a **variable** body bit offset T₁ (1540, 2940, 2636, 2034, 2098, 2067, 1092, 927 and 3779 bits in the 9 messages). It follows a variable-length region, and was located by scanning for a world-coordinate triple. Offsets below are **relative to T₁** and were identical in all 9 messages.

| Rel. bits | Raw content | Class (raw) | Interpretation | Class (interp.) |
|---|---|---|---|---|
| −59 | `f32` = 2 in all 9 | VERIFIED | — | UNKNOWN |
| −45 | `f32` = 2.001 or 8.004 | VERIFIED | — | UNKNOWN |
| −29 | `f32` 9,616–46,660, varying | VERIFIED | — | UNKNOWN (may overlap other fields) |
| −12 | `f32` constant per target (191.6 for A, −299.1 for B) | VERIFIED | Per-target property | UNKNOWN |
| **+0, +32, +64** | `f32` ×3: **A (3039.3, 582.1, −12345.9)**, **B (2965.8, 581.5, −12277.9)**, identical across all hits on each target | VERIFIED | **Target position** (world frame, §4.3) | EXPERIMENTAL. B matches Phosphor's 8111 map position of the destroyed SPAA, (2965.8, −12277.9), to the decimetre |
| +22, +26, +56 | `f32` values constant per target | VERIFIED | Probably overlaps of neighbouring fields | UNKNOWN |
| **+96, +128, +160** | `f32` ×3, different in each hit | VERIFIED | **Projectile launch position** | EXPERIMENTAL (see 4.6.5) |
| **+210, +242, +274** | `f32` ×3, 13.7–14.7 m from the launch position | VERIFIED | A point on the initial path | EXPERIMENTAL |
| **+306, +338, +370** | `f32` ×3, magnitude 1,342–1,419 | VERIFIED | **Initial projectile velocity**, world frame, apparently m/s | Direction EXPERIMENTAL; **units DERIVED, not verified** |
| after +402 | Variable-length remainder | VERIFIED (length) | Contains 0–4 small `f32` triples (a few metres) at varying offsets, often in near-identical pairs, e.g. (−2.19, 1.19, 0.03) and (−2.14, 1.18, 0.04) | **UNKNOWN.** A target-relative hit point is possible but **not established** (4.6.8) |

| Raw vectors per hit (time = hit packet) | Launch position (+96) | Path point (+210) | Velocity vector (+306) |
|---|---|---|---|
| 329.354 | (2607.3, 913.0, −13273.3) | (2613.1, 908.7, −13260.8) | (546.6, −412.7, 1174) |
| 330.513 | (2736.9, 814.4, −12995.8) | (2742.8, 810.1, −12983.1) | (561.6, −423.3, 1204) |
| 331.153 | (2805.4, 762.5, −12849.2) | (2811.3, 758.2, −12836.5) | (571.3, −435.0, 1224) |
| 379.139 | (3221.9, 743.2, −11659.8) | (3216.7, 740.1, −11672.4) | (−501.2, −312.3, −1210) |
| 380.016 | (3140.8, 690.5, −11856.9) | (3135.6, 687.4, −11869.7) | (−520.5, −324.4, −1262) |
| 423.473 | (2302.1, 903.1, −13158.9) | (2310.1, 899.4, −13148.3) | (802.4, −382.9, 1065) |
| 424.504 | (2446.9, 832.7, −12966.7) | (2455.0, 829.1, −12955.9) | (775.0, −369.1, 1032) |
| 425.660 | (2608.8, 754.7, −12751.8) | (2617.0, 750.9, −12741.0) | (778.9, −377.6, 1035) |
| 426.246 | (2693.4, 713.5, −12639.7) | (2701.6, 709.8, −12628.9) | (778.6, −373.9, 1034) |

Supporting observations (DERIVED):
- The direction from launch position to path point matches the velocity vector's direction to within a fraction of a degree, and both lie 0.57–0.92° from the line from launch position to target.
- Subtracting the aircraft velocity (from SDL's 4 Hz track) gives 1,151–1,209 m/s, within 0.05–0.73° of the aircraft's velocity direction. The values fall in two groups, about 1,152 and 1,186–1,209 m/s.

#### 4.6.5 Launch position vs SDL's aircraft track

| Observation | Class | Evidence |
|---|---|---|
| Each launch position lies on SDL's linearly interpolated ownship track (`ownship.jsonl`, §4.3) | VERIFIED (numeric agreement with SDL's own track) | Closest approach **0.03–0.08 m** for all 9 hits |
| So the launch position is the author's position at firing | EXPERIMENTAL | Whether it is the aircraft origin or the gun muzzle is UNKNOWN |

#### 4.6.6 Hits, firing windows and reconstructed flight times

Sources:
- **Firing window:** the two `aircraft_small` syncs between which the ammo counter (§4.3) dropped. These are recorded timestamps (VERIFIED).
- **Hit time:** the packet timestamp of the hit cluster. It is a recorded value, 1 ms resolution, inherited from the batch (VERIFIED as recorded).
- **Launch time:** when the interpolated track passes the launch position. **DERIVED, not recorded.**
- **Flight time:** hit time − launch time. **DERIVED.**
- **Range:** the distance from launch position to target position (DERIVED).

| # | Counter | Firing window (sync, s) | Launch time (DERIVED, s) | Hit time (recorded, s) | Flight time (DERIVED) | Range (DERIVED) | Target | Packets (F0E9 / F117·F13C / F0C2) |
|---|---|---|---|---|---|---|---|---|
| 1 | 30→29 | (328.157, 328.407] | 328.229 | 329.354 | 1.125 s | 1,075.3 m | A | 82288 / 82289 / 82290 |
| 2 | 29→28 | (329.656, 329.908] | 329.792 | 330.513 | 0.721 s | 753.5 m | A | 82765 / 82766 / 82767 |
| 3 | 28→27 | (330.405, 330.658] | 330.604 | 331.153 | 0.549 s | 583.6 m | A | 83031 / 83032 / 83033 |
| 4 | 24→23 | (378.405, 378.655] | 378.459 | 379.139 | 0.680 s | 688.3 m | B | 101702 / 101703 / 101705 |
| 5 | 23→22 | (379.408, 379.657] | 379.583 | 380.016 | 0.433 s | 468.8 m | B | 102065 / 102066 / 102067 |
| 6 | 20→19 | (422.156, 422.405] | 422.250 | 423.473 | 1.223 s | 1,148.9 m | B | 119081 / 119082 / 119083 |
| 7 | 19→18 | (423.406, 423.658] | 423.583 | 424.504 | 0.921 s | 898.2 m | B | 119482 / 119483 / 119484 |
| 8 | 18→17 | (424.908, 425.156] | 425.041 | 425.660 | 0.619 s | 618.1 m | B | 119938 / — / 119939 |
| 9 | 17→16 | (425.656, 425.907] | 425.792 | 426.246 | 0.454 s | 471.8 m | B | 120229 / 120230 / 120231 |

- **Pairing (VERIFIED as an observation):** every reconstructed launch time falls inside exactly one counter window. That ties each hit to one counter decrement.
- **Kill (VERIFIED raw):** hit 9 is followed at 426.265 s by the `0xF058` kill record, `0xF04B` on target B, and the `0xF0C2` repeat (packet 120245).
- **Misses (VERIFIED absence):** 7 counter decrements have no hit message carrying an author key: 32→31, 31→30, 27→26, 26→25, 25→24, 22→21 and 21→20.
- **Internal consistency (DERIVED):**
  - Average speed (range / flight time) falls from about 1,083 m/s at 469 m to about 939 m/s at 1,149 m.
  - The target origin sits below the straight launch line by 0.6–1.1 m at about 0.45 s and 4.4–4.5 m at about 1.1–1.2 s.
  - Both trends are consistent with drag and gravity.

#### 4.6.7 Recorded timestamps vs inferred times

| Quantity | Nature | Resolution | Accuracy |
|---|---|---|---|
| Hit time | Recorded packet timestamp, inherited within a batch | 1 ms | **Not verified.** It may be quantized to server ticks |
| Firing window | Recorded sync timestamps | about 250 ms window | Recorded |
| Launch time | **Inferred** by interpolating the 4 Hz track to the launch position | Sub-millisecond numerically | **Not verified.** Linear interpolation of a curved path, and the origin-vs-muzzle question (about 5 ms per metre at about 195 m/s), are unquantified |
| Flight time | Inferred (hit time − launch time) | — | Inherits both uncertainties; not independently measured |

#### 4.6.8 Not found

- **No independently measured impact coordinates.** No world-frame position near either target, other than the constant target position, occurs in any author hit message. The small triples in the `0xF0C2` remainder are not established as hit points: adding them to the target position (with unknown target rotation) does not consistently bring them closer to the launch line.
- **No trajectories for missed projectiles.** The 7 missed shots leave no message carrying their key, and no intermediate projectile positions were found for any shot. `0xF11A` (REPORTED as missile and rocket sync) does not occur in this replay.
- **No firing-time record with the key** (4.6.2).
- **Explosion messages can't be attributed.** `0xD08E` (220 effect-kind-9 records, from 157 s onward) carries world positions, but these could not be attributed to the author's shells. Only one coincides with an author hit (329.354 s, about 420 m from target A).

#### 4.6.9 Implications for Project Phosphor's ballistic engine

- **A validation set exists, but only for hits.** For the 9 hits, the replay gives each shell's launch position, initial velocity vector (§4.6.4 caveats), recorded hit time and target position. Fly Phosphor's trajectory from that launch state and compare the predicted arrival:
  - against the target position, within the target's size;
  - against the derived flight time.

  It needs no video. It tests the trajectory integration (drag, gravity). It does not test aircraft-state reconstruction, which Phosphor does from 8111 and SDL, or sight and HUD geometry.
- **Not a full impact check.** It bounds the impact only to within a few metres around the target origin. It says nothing about misses or terrain impacts.
- **Known limits on comparisons:**
  - Flight-time comparisons inherit the unverified launch-time accuracy (4.6.7).
  - The velocity-vector units are inferred.
  - The two muzzle-speed groups are unexplained.
- **Video still needed.** Measured impact points, misses and HUD pixel alignment still need screen recordings or another visual reference.

#### 4.6.10 Reproduction on a second replay (added 2026-11-10)

Replay `#2026.11.10 04.23.23.wrpl` (SHA-256 `f4b1b2ae004f5abd16775dba7cfc7965958ec9ec6595d839e8d2e035fdc75b33`, build 2.59.0.100, `me-262a-1a_u4`, `air_vietnam_ec_BfD_norespawn`). Same method as above; read-only, no decoder changes. Because this is the same aircraft, mission and build, it confirms repeatability, not generality.

| Check | Result | Class |
|---|---|---|
| Author offender id | `0x01AC` = 428 again; kill-record killer id 428 | VERIFIED (raw) |
| Hit clusters with an author key | 9 (on three targets). 8 carry `0xF0C2`; the hit at 418.165 s has `0xF0E9` and `0xF117` only. Key layout as in 4.6.2; indices all multiples of 4; generations 02 and 03 | VERIFIED (raw) |
| `0xF0C2` block | Same relative layout. T₁ at body bits 471–4603 | VERIFIED (raw) |
| Launch position vs SDL ownship track | **0.01–0.13 m** for all 8 | VERIFIED (agreement with SDL's track) |
| Launch times vs counter windows | All 8 reconstructed launch times fall inside one counter window each | Launch times DERIVED; window membership observed |
| Misses | 6 of 15 counter decrements have no author hit: 32→31, 31→30, 30→29, 27→26, 25→24, 21→20 | VERIFIED (absence) |

| # | Counter | Firing window (s) | Launch (DERIVED, s) | Hit (recorded, s) | Flight time (DERIVED) | Range (DERIVED) | Target (ext-uid bytes) | `0xF0C2` packet |
|---|---|---|---|---|---|---|---|---|
| 1 | 29→28 | (353.841, 354.093] | 353.938 | 354.496 | 0.558 s | 609.7 m | `81 d0 11` at (5215.8, 397.2, 4362.1) | 96146 |
| 2 | 28→27 | (354.345, 354.594] | 354.541 | 354.975 | 0.434 s | 456.1 m | `81 d0 11` | 96344 |
| 3 | 26→25 | (384.094, 384.345] | 384.208 | 384.647 | 0.439 s | 468.5 m | `81 e8 12` at (5370.5, 402.0, 4521.0) | 108304 |
| 4 | 24→23 | (415.844, 416.092] | 416.000 | 416.696 | 0.696 s | 708.5 m | `81 d0 11` | 121336 |
| 5 | 23→22 | (417.096, 417.345] | 417.250 | 417.697 | 0.447 s | 465.5 m | `81 d0 11` (kill record at the same ms) | 121778 |
| 6 | 22→21 | (417.591, 417.846] | — (no `0xF0C2`) | 418.165 | 0.319–0.574 s (from window) | — | `81 d0 11` | — |
| 7 | 20→19 | (446.594, 446.841] | 446.812 | 447.516 | 0.704 s | 719.3 m | `81 f0 12` at (5855.8, 374.7, 4316.9) | 134014 |
| 8 | 19→18 | (447.846, 448.093] | 447.979 | 448.498 | 0.519 s | 510.1 m | `81 f0 12` | 134414 |
| 9 | 18→17 | (448.591, 448.846] | 448.709 | 449.087 | 0.378 s | 377.3 m | `81 f0 12` | 134682 |

New observations:
- **The same extended-uid bytes appear at different positions in the two sessions.** `81 d0 11` was at (3039.3, 582.1, −12345.9) on 2026-11-09 and at (5215.8, 397.2, 4362.1) here. These bytes name a mission unit slot, not a fixed object. EXPERIMENTAL.
- **Possible kill-victim mapping** (open question 11):

  | Replay | Killed target's ext-uid bytes | LEB128 value | Kill-record victim id |
  |---|---|---|---|
  | 2026-11-09 | `81 e8 12` | 308,225 | 2332 |
  | 2026-11-10 | `81 d0 11` | 288,769 | 2313 |

  The LEB128 values differ by 19 × 1024, and the victim ids by 19, consistent with `victim_id = (ext_uid >> 10) + 2031`. **Two points only; EXPERIMENTAL.**

## 5. Open questions

1. The other aircraft records in `aircraft_small`: per-record layout, uids, orientation and velocity. (The author's x/z is now validated against 8111, §4.5.)
2. What the `replay_header_info` 300-byte payload means (reported as ECS message hashes).
3. Server-replay segmentation (`next_segment`, part numbering): needs genuine server parts.
4. The meaning of the BLK leading byte `0x01`, and a native BLK reader for settings and results.
5. The `unk0`, `unk1`, `unk3`, `unk4` and `unk5` header regions. They are preserved raw in every manifest. Example: `unk5` = `01 00…` and `unk3` ends `ff ff ff ff`.
6. The cause of non-canonical size prefixes.
7. **Projectile velocity (§4.6.4):**
   - Confirm that the +306 vector is world velocity in m/s.
   - Explain the two muzzle-speed groups (about 1,152 vs 1,186–1,209 m/s): belt shell types, dispersion or something else.
   - Establish what the +210 path point represents.
8. **Launch-time accuracy (§4.6.7):**
   - Is the launch position the aircraft origin or the muzzle?
   - How large is the error from interpolating the 4 Hz track?
   - Are hit timestamps quantized to server ticks?
9. **Target-relative collision coordinates:**
   - Do the small triples in the variable `0xF0C2` remainder, or another field, encode the hit point on the target?
   - Find the target's orientation needed to test this.
10. **Hit-message fields:** the `0xF0E9` body, the `0xF117`/`0xF13C` trailing bytes, the per-target `0xF0C2` constants (rel −12, +22, +26, +56), and the variable region before T₁.
11. **Kill-record victim id:** 2332 (`0x091C`) vs the hit messages' extended-uid bytes for target B (`81 e8 12`). Two replays fit `victim_id = (ext_uid >> 10) + 2031` (§4.6.10); this needs more kills to confirm.

## 6. Future research (recorded, not started)

**Camera state: MPI `0xD043 ReplayCameraParams`.** This is a separate item, kept apart from the projectile work. It is relevant to Project Phosphor's HUD and screen projection.

Raw observations (VERIFIED, replay `#2026.11.09 18.50.56`, read-only probe):
- 14,963 messages, signature `03 58 43 D0`, 70-byte payloads, about 24 Hz across the flight.
- A unit-norm `f32le` quaternion at payload byte 8.
- A unit-norm vector at byte 52.
- A 3-vector at byte 40.

Interpretations (EXPERIMENTAL; layout REPORTED by WT_KNOWLEDGE_DOCS):
- The quaternion is the camera orientation.
- The byte-40 vector is the camera offset from the aircraft: about (−16.7, 3.9, −0.4) early in the flight (a chase view), and about (−0.42, 0.81, −0.07) while firing at 422.8 s (a cockpit eye position).
- A field that reads 1 early and 2.75 while firing may be zoom.

Not decoded or validated. Validate against the 8111 compass, pitch and roll before use. Pixel alignment will still need screenshots or video.
