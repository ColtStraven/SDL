# S57-003 SDL — `.wrpl` Format Findings

Decoder: `sdl-replay-decoder` 0.1.0. Date: 2026-10-29, updated 2026-11-09 (container counts; new section 4, payload decoding).
Evidence base: 28 genuine client replays recorded by the Static 57 owner, 2026-10-22 to 2026-10-25 and 2026-11-09 (see [COMPATIBILITY.md](COMPATIBILITY.md)).

Classifications: **VERIFIED** (documented structure with a reproducible cross-check on Static 57 replays), **REPORTED** (external documentation only), **DERIVED**, **EXPERIMENTAL**, **UNKNOWN**.

## 1. Container

| Finding | Class | Evidence |
|---|---|---|
| Signature `E5 AC 00 10` (`u32le 0x1000ACE5`) at offset 0 | VERIFIED | All 28 files |
| Format magic at offset 4 = `0x00018C1C` (`1C 8C 01 00`) | VERIFIED | All 28 files |
| Fixed header is 1234 bytes; settings BLK begins at 1234 | VERIFIED | The zstd magic sits exactly at `1234 + settings_blk_size` in all 28 files |
| `settings_blk_size` is a `u16le` at offset 748 | VERIFIED | Same cross-check (observed values 682–1356) |
| `footer_blk_offset` is a `u32le` at offset 684 and marks the end of the packet stream | VERIFIED | In all 28 files the zstd frame ends exactly at this offset, with 0 trailing bytes |
| The footer/results BLK runs from `footer_blk_offset` to EOF | REPORTED | Starts with byte `0x01` and contains key names such as `timePlayed` and `authorUserId`; not decoded |
| The settings BLK leads with byte `0x01` | EXPERIMENTAL | All 28 files. Meaning of the byte not established |
| `session_id` is a `u64le` at offset 732 | REPORTED | Present and non-zero. Not cross-checked against server parts |
| `start_time` is Unix seconds (`u32le`) at offset 908 | REPORTED, plausibility DERIVED | Example: `1792795650` = 2026-10-23T22:47:30Z, which matches file name `#2026.10.23 15.47.41` in local time (UTC−7) with the save time 11 s later |
| `game_version` (`u64le` at 1226) holds four `u16le` words `[build, patch, minor, major]` | EXPERIMENTAL | Reads 2.59.0.80, then .83, then .86 across consecutive days, then 2.59.0.100 on 2026-11-09. Consistent with game updates, not documented anywhere |
| Client classification (`player_no != 0x80000000`, `segment_length_sec != 90`) | REPORTED | All 28 classify as client. No server replay available |

The string fields (level, mission, environment, weather, location) decode as plain NUL-terminated ASCII, for example `levels/air_denmark.bin`, `air_denmark_I2M`, `evening`, `cloudy`.

## 2. Compression

| Finding | Class | Evidence |
|---|---|---|
| Format `18c1c` uses Zstandard for the packet stream | VERIFIED | All 28 files |
| Exactly **one** zstd frame per client replay | VERIFIED (for these files) | All 28 files |
| The frame decodes with a pure streaming decoder; no content-size field is needed | VERIFIED | `ruzstd` streaming decoder |
| Compression ratio | DERIVED | 1.33× to 4.84× (2026-11-09 replay: 4.10×) |
| zlib for `18c0b` / `18c0a` | REPORTED | No genuine sample. zlib support is tested only on synthetic fixtures |

## 3. Packet framing (decompressed stream)

| Finding | Class | Evidence |
|---|---|---|
| Big-endian variable-length size prefix (see `src/packets.rs`) | VERIFIED | 4,303,777 packets frame with **zero** leftover bytes. Every replay ends with an end marker as its final record |
| Body = `u16le type`, then a `u32le` millisecond timestamp unless `type & 0x10` | VERIFIED | Same accounting. Explicit timestamps never go backwards in any file |
| Prefix forms observed | VERIFIED | 1-byte: 2,377,912. 2-byte: 1,925,632. 3-byte: 233. No 4- or 5-byte prefixes seen |
| **Non-canonical prefixes**: 22.6% of prefixes are longer than the shortest encoding (for example, size 6 written as `40 06`) | VERIFIED (new finding) | 971,333 of 4,303,777. The decoder accepts and counts them. The likely cause is the writer reserving prefix space before the body size is known (EXPERIMENTAL) |
| No bits outside `0x1F` are set in any type field | VERIFIED | `type_high_bits_packets = 0` in all files |
| The running time starts at 0. Packets that inherit time before the first explicit timestamp get 0 ms | EXPERIMENTAL | 2 replays recorded from session start. Their first explicit times are 27 ms and 32 ms, after 328 and 352 inherited packets. Output marks these `inherited_initial` |
| Timestamps are session time, not replay-relative | DERIVED | First explicit timestamps range from 27 ms to 7,294,000 ms. Replays started mid-session begin at large values |

### Packet type inventory (all 28 replays)

| Type | Name (REPORTED) | Packets | Payload bytes | Payload min..max | Notes |
|---|---|---|---|---|---|
| 0 | end_marker | 28 | 0 | 0..0 | Always the last record (VERIFIED) |
| 1 | start_marker | 28 | 0 | 0..0 | One per replay |
| 2 | aircraft_small | 106,345 | 253,576,117 | 19..5,596 | Flight-model sync. Target of Milestone 3. The 19-byte minimum comes from the 2026-11-09 replay (build .100); the earlier 27 never went below 36 |
| 3 | chat | 417 | 27,919 | 23..154 | In 18 of 28 replays |
| 4 | mpi | 3,697,865 | 320,562,423 | 4..27,726 | Most frequent type |
| 6 | ecs | 499,066 | 52,448,157 | 4..208,943 | |
| 8 | replay_header_info | 28 | 8,400 | 300..300 | Always first; payload always 300 bytes |
| 5, 7 | next_segment, snapshot | 0 | — | — | Not present in client replays. Reported for server parts |

Type names come from third-party documentation. `packets.jsonl` keeps every record's `decode_state` as `raw`; decoded payload content is written separately by the `flight` command (section 4).

## 4. Payload decoding (added 2026-11-09)

Evidence base: replay `#2026.11.09 18.50.56` (build 2.59.0.100, Me 262 A-1a/U4, `air_vietnam`). Decoder module `flight` / `mpi`, command `sdl-replay-decoder flight`. One replay only: every finding below needs confirmation on more replays and aircraft.

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
| 3 × f32 | **World position `(x, y, z)` in metres; `y` is height above sea level, +x east, +z north** | VERIFIED (this replay) | `y`: 2,494 of 2,494 within 20 m of the cockpit altimeter (median 0.45 m, max 11.3 m). `x`/`z`: RMS 2.2 m, max 3.4 m against all 113 independent 8111 map positions (§4.5). 0 speed discontinuities; max derived speed 214 m/s against max IAS 204 m/s |
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

## 5. Open questions

1. The other aircraft records in `aircraft_small`: per-record layout, uids, orientation and velocity. (The author's x/z is now validated against 8111, §4.5.)
2. What the `replay_header_info` 300-byte payload means (reported as ECS message hashes).
3. Server-replay segmentation (`next_segment`, part numbering): needs genuine server parts.
4. The meaning of the BLK leading byte `0x01`, and a native BLK reader for settings and results.
5. The `unk0`, `unk1`, `unk3`, `unk4` and `unk5` header regions. They are preserved raw in every manifest. Example: `unk5` = `01 00…` and `unk3` ends `ff ff ff ff`.
6. The cause of non-canonical size prefixes.
