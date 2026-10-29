# S57-003 SDL — `.wrpl` Format Findings

Decoder: `sdl-replay-decoder` 0.2.0. Date: 2026-10-29 (Milestones 1–3).
Evidence base: 27 genuine client replays recorded by the Static 57 owner, 2026-10-22 to 2026-10-25 (see [COMPATIBILITY.md](COMPATIBILITY.md)).

Classifications: **VERIFIED** (documented structure with a reproducible cross-check on Static 57 replays), **REPORTED** (external documentation only), **DERIVED**, **EXPERIMENTAL**, **UNKNOWN**.

## 1. Container

| Finding | Class | Evidence |
|---|---|---|
| Signature `E5 AC 00 10` (`u32le 0x1000ACE5`) at offset 0 | VERIFIED | All 27 files |
| Format magic at offset 4 = `0x00018C1C` (`1C 8C 01 00`) | VERIFIED | All 27 files |
| Fixed header is 1234 bytes; settings BLK begins at 1234 | VERIFIED | The zstd magic sits exactly at `1234 + settings_blk_size` in all 27 files |
| `settings_blk_size` is a `u16le` at offset 748 | VERIFIED | Same cross-check (observed values 682–1356) |
| `footer_blk_offset` is a `u32le` at offset 684 and marks the end of the packet stream | VERIFIED | In all 27 files the zstd frame ends exactly at this offset, with 0 trailing bytes |
| The footer/results BLK runs from `footer_blk_offset` to EOF | REPORTED | Starts with byte `0x01` and contains key names such as `timePlayed` and `authorUserId`; not decoded |
| The settings BLK leads with byte `0x01` | EXPERIMENTAL | All 27 files. Meaning of the byte not established |
| `session_id` is a `u64le` at offset 732 | REPORTED | Present and non-zero. Not cross-checked against server parts |
| `start_time` is Unix seconds (`u32le`) at offset 908 | REPORTED, plausibility DERIVED | Example: `1792795650` = 2026-10-23T22:47:30Z, which matches file name `#2026.10.23 15.47.41` in local time (UTC−7) with the save time 11 s later |
| `game_version` (`u64le` at 1226) holds four `u16le` words `[build, patch, minor, major]` | EXPERIMENTAL | Reads 2.59.0.80, then .83, then .86 across consecutive days. Consistent with game updates, not documented anywhere |
| Client classification (`player_no != 0x80000000`, `segment_length_sec != 90`) | REPORTED | All 27 classify as client. No server replay available |

The string fields (level, mission, environment, weather, location) decode as plain NUL-terminated ASCII, for example `levels/air_denmark.bin`, `air_denmark_I2M`, `evening`, `cloudy`.

## 2. Compression

| Finding | Class | Evidence |
|---|---|---|
| Format `18c1c` uses Zstandard for the packet stream | VERIFIED | All 27 files |
| Exactly **one** zstd frame per client replay | VERIFIED (for these files) | All 27 files |
| The frame decodes with a pure streaming decoder; no content-size field is needed | VERIFIED | `ruzstd` streaming decoder |
| Compression ratio | DERIVED | 1.33× to 4.84× |
| zlib for `18c0b` / `18c0a` | REPORTED | No genuine sample. zlib support is tested only on synthetic fixtures |

## 3. Packet framing (decompressed stream)

| Finding | Class | Evidence |
|---|---|---|
| Big-endian variable-length size prefix (see `src/packets.rs`) | VERIFIED | 4,086,169 packets frame with **zero** leftover bytes. Every replay ends with an end marker as its final record |
| Body = `u16le type`, then a `u32le` millisecond timestamp unless `type & 0x10` | VERIFIED | Same accounting. Explicit timestamps never go backwards in any file |
| Prefix forms observed | VERIFIED | 1-byte: 2,186,795. 2-byte: 1,899,150. 3-byte: 224. No 4- or 5-byte prefixes seen |
| **Non-canonical prefixes**: 23.3% of prefixes are longer than the shortest encoding (for example, size 6 written as `40 06`) | VERIFIED (new finding) | 952,524 of 4,086,169. The decoder accepts and counts them. The likely cause is the writer reserving prefix space before the body size is known (EXPERIMENTAL) |
| No bits outside `0x1F` are set in any type field | VERIFIED | `type_high_bits_packets = 0` in all files |
| The running time starts at 0. Packets that inherit time before the first explicit timestamp get 0 ms | EXPERIMENTAL | 2 replays recorded from session start. Their first explicit times are 27 ms and 32 ms, after 328 and 352 inherited packets. Output marks these `inherited_initial` |
| Timestamps are session time, not replay-relative | DERIVED | First explicit timestamps range from 27 ms to 7,294,000 ms. Replays started mid-session begin at large values |

### Packet type inventory (all 27 replays)

| Type | Name (REPORTED) | Packets | Payload bytes | Payload min..max | Notes |
|---|---|---|---|---|---|
| 0 | end_marker | 27 | 0 | 0..0 | Always the last record (VERIFIED) |
| 1 | start_marker | 27 | 0 | 0..0 | One per replay |
| 2 | aircraft_small | 103,698 | 247,873,046 | 36..5,596 | Flight-model sync. Decoded in Milestone 3 (§5) |
| 3 | chat | 417 | 27,919 | 23..154 | In 18 of 27 replays |
| 4 | mpi | 3,485,535 | 312,940,321 | 4..27,726 | Most frequent type |
| 6 | ecs | 496,438 | 51,796,163 | 4..208,943 | |
| 8 | replay_header_info | 27 | 8,100 | 300..300 | Always first; payload always 300 bytes |
| 5, 7 | next_segment, snapshot | 0 | — | — | Not present in client replays. Reported for server parts |

Type names come from third-party documentation. In `packets.jsonl` every record stays `decode_state: raw`. Payload decoding happens in separate modules: type 2 is decoded by the flight module (§5, Milestone 3), and type 4 message `0xD04A` is decoded only as a validation reference (§6). No other payload is decoded.

## 4. Open questions

1. What the `replay_header_info` 300-byte payload means (reported as ECS message hashes).
2. Server-replay segmentation (`next_segment`, part numbering): needs genuine server parts.
3. The meaning of the BLK leading byte `0x01`, and a native BLK reader for settings and results.
4. The `unk0`, `unk1`, `unk3`, `unk4` and `unk5` header regions. They are preserved raw in every manifest. Example: `unk5` = `01 00…` and `unk3` ends `ff ff ff ff`.
5. The cause of non-canonical size prefixes.
6. Type-2 open questions are listed in §5.6.

## 5. Aircraft flight state: packet type 2 (Milestone 3)

**Evidence base:** the same 27 genuine 2.59 client replays (format `0x00018C1C`, game version field 2.59.0.80–86), with 103,698 type-2 packets and 2,468,023 aircraft state records.
**Reproduce:** `sdl-replay-decoder flight-survey <Replays folder>`, or `SDL_REPLAY_DIR=<folder> cargo test --release -- --ignored`. Per-replay numbers are in [COMPATIBILITY.md](COMPATIBILITY.md).

### 5.1 Bit conventions (`src/bitstream.rs`)

| Finding | Class | Evidence |
|---|---|---|
| Bits are read MSB-first within each byte. Byte-sized reads need not be aligned. Multi-byte values are little-endian | VERIFIED | Every one of the 103,698 payloads parses to the `0x7FF` terminator with zero structural errors. A misread would desynchronise every later record |
| "Compressed" integers are little-endian base-128 varints, read byte-wise; zigzag is used for signed values | VERIFIED | Same evidence (uids and weapon lists use them) |

### 5.2 Record structure (`src/aircraft.rs`)

The layout is REPORTED by two independent third-party readers. SDL implemented it independently, and it is **structurally VERIFIED**: 103,698 of 103,698 payloads reach the terminator inside the payload, and every bit is classified (§5.5).

```
entry := uid_explicit:1 [uid:varint & 0x7FF] (else previous + 1)   uid 0x7FF terminates
         no_data:1 [hidden_a:1 hidden_b:1 [if !(a&&b): state]]
state := ground_flag:1 unk_u32:32 unk_bit:1 guns weapons
         position:f32[3] euler:u32 packed_velocity:u32 <align> controls:u8[7]
         engines sensors countermeasures targets [flag u16 u16] [flag seeker_len:u32 seeker]
after terminator: trailer f32[3], then 0, 13, 24 or 35 bytes of unparsed data
```

Entry kinds across the corpus: 2,468,023 state records, 1,050,657 withheld (both hidden flags set), and 4,269,919 no-data.

### 5.3 Decoded fields (`src/flight.rs`)

| Field | Location / type | Quantisation | Meaning, units | Class | Validation |
|---|---|---|---|---|---|
| Position | `state` bits after the weapon list, 3 × `f32le` (unaligned) | IEEE float | World X, Y, Z in metres; **+Y up** | VERIFIED | (1) The recording author's Y equals the cockpit altimeter. (2) Displacement between 250 ms samples matches decoded velocity × dt: author median residual 0.000–0.081 m. (3) Finite-difference speeds have median 165 m/s and p99.9 445 m/s (plausible aircraft speeds) |
| Altitude | Position Y | — | Metres above mean sea level | VERIFIED | Matches the cockpit altimeter (0xD04A id 40, 42 or 43, in m or ft) in 19 replays. Median abs error 0.000–0.65 m; p99 ≤ 2.7 m |
| Heading | `euler` bits 19–28 magnitude, bit 31 sign | 10 bits over [0°, 180°], 0.176° step | Yaw about +Y, right-handed, zero along +X; the nose points along `(cos h, 0, -sin h)` | VERIFIED | Median abs(heading − velocity track angle) is 0.17–5.4° per replay. This includes sideslip and remote-aircraft lag |
| Pitch | `euler` bits 10–18 magnitude, bit 30 sign | 9 bits over [0°, 90°], 0.176° step | Nose-up positive | VERIFIED | Equals −(cockpit id 53): median abs error 0.020–0.075° in 19 replays (p99 ≤ 2.0°). Pitch − flight-path angle has median −0.2 to +1.6° (angle of attack) |
| Bank | `euler` bits 0–9 magnitude, bit 29 sign | 10 bits over [0°, 180°], 0.176° step | Same sign as cockpit roll (id 52) | VERIFIED | Median abs error 0.002–0.235° in 19 replays (p99 ≤ 7.6° during fast rolls) |
| Speed | `packed_velocity` bits 19–28 | 10 bits, **500/1023 m/s per step**; 1023 means saturated (≥ 500 m/s) | World-frame speed, m/s | VERIFIED | (1) Steady-flight finite-difference speed minus `A·500/1023` has median 0.0–0.4 m/s for A in 64–767. (2) Converted to indicated airspeed with ISA density, it matches cockpit IAS (id 0) with pooled median ratio 1.0001 and per-replay median abs error 0.1–4.6 m/s (16 replays) |
| Velocity direction | bits 9–17 = abs(vx)/speed and bits 0–8 = abs(vy)/speed (9 bits each); signs in bits 31, 30, 29 for x, y, z; abs(vz)/speed = sqrt(1 − rx² − ry²) | 1/511 | Unit direction of world velocity | VERIFIED | Angle to the finite-difference velocity: median 0.08°, p99 1.9°. Author displacement direction: median 0.09–0.37° |
| Vertical speed | Velocity y component | DERIVED from the above | m/s | VERIFIED | Matches cockpit id 39: median abs error 0.005–0.26 m/s in 18 replays |
| Bit 18 of `packed_velocity` | 1 bit | — | No effect on speed or direction; meaning not established | UNKNOWN | Set in about 44% of moving records |
| `ground_flag` | First state bit | — | Reported as on-ground | EXPERIMENTAL | Set in 73% of records at 5–40 m/s and 2.6% above 80 m/s. Exported raw |
| Trailer | 3 × `f32` after terminator | — | Recording author's position (REPORTED) | VERIFIED as an author locator | The nearest state record is within 0.01 m (median) in 99% of packets. Those records validate against cockpit instruments |

The speed field saturates at 500 m/s: 2,945 records across the corpus. Their exported speed is a lower bound, flagged `speed_saturated`.

### 5.4 Track identity (`src/tracks.rs`)

| Finding | Class | Evidence |
|---|---|---|
| The record key is an 11-bit unit uid, explicit or implicit (+1). It is **not** a vehicle or player identity | REPORTED | Uid-to-vehicle mapping needs ECS (Milestone 4) |
| Records arrive at 4 Hz per aircraft (dt = 250 ms) | VERIFIED | 2,464,418 of 2,467,026 consecutive intervals are ≤ 300 ms |
| Remote aircraft show network position corrections | VERIFIED | 0.17% of 250 ms steps jump 200 m–2 km beyond velocity × dt. The author's own track does not (author residual p99 ≤ 0.31 m) |
| Uids are reused and aircraft respawn | EXPERIMENTAL | Gaps over 5 s and multi-kilometre jumps occur within one uid. One replay (2026.10.25 04.37.02) contains two author uids |

SDL's track policy is conservative. A track is a provisional segment `u<uid>.s<n>`. A new segment starts on a gap over 5 s or an unexplained jump over 2 km. Jumps of 200 m–2 km are flagged and do not split a segment. Segments are never merged. Corpus totals: 2,460 segments over 997 uid series, starting as 997 first records, 735 gaps and 728 position jumps; 3,700 further samples flagged `position_jump`.

### 5.5 Decoding coverage

Over all 1,982,984,368 type-2 payload bits:

| Category | Share |
|---|---|
| Physically decoded (position, orientation and velocity words) | 19.91% |
| Parsed by structure, meaning unknown or not exported (uids, flags, guns, weapons, controls, engines, sensors, countermeasures, targets, seeker) | 78.87% |
| Trailer floats | 0.50% |
| Unparsed (after the trailer) | 0.72% |

Every state record yields position, altitude, orientation and velocity. Withheld and no-data entries carry no measurements and are counted, not exported.

### 5.6 Known limitations and open questions

1. **Horizontal frame:** X and Z are metres in the world frame, but their relation to map north and to map grid coordinates is unknown. It needs `/map_info.json` plus `/map_obj.json` from 8111 (see [TELEMETRY_8111.md](TELEMETRY_8111.md)).
2. **Remote aircraft** are network-replicated states in a client replay. Their kinematic self-consistency is lower (median displacement residual 0.06–40 m per replay), and no independent instrument exists for them.
3. **Identity:** the uid-to-vehicle, player and life relationship is unresolved. Track ids are provisional.
4. **Not decoded:** `unk_u32`, `unk_bit`, bit 18 of the velocity word, the per-entry hidden flags, gun-entry bits, weapon list values, the seven control bytes (reported: stick, flaps, rudder, throttle), engine fields, sensors, countermeasures, targets, the seeker block, and the 13/24/35 bytes after the trailer (multiples of 11 plus 2). Controls and engine states are exported raw.
5. **No server replay** has been tested.
6. **Wind:** decoded velocity is world-frame. Treating it as airspeed assumes no wind; the ISA/IAS check is consistent with that within about 1–3 m/s.

## 6. Cockpit parameters: MPI `0xD04A` (validation reference only)

| Finding | Class | Evidence |
|---|---|---|
| MPI payload = `object_id:u16le, message_id:u16le, body` | VERIFIED for `0xD04A` | Object `0x5803` in all cockpit messages |
| Body = varint bit length, align, `u16 count`, `count × (u16 id, f32 value)` | VERIFIED | 99,587 messages in 20 replays decode with 0 failures; the declared length always fits |
| One message per author sync, offset from it by a median 36 ms (p99 124 ms) | VERIFIED | Pairing statistics |
| id 0 is IAS (m/s); 39 vertical speed (m/s); 52 roll (°); 53 pitch (°, sign opposite to the flight model) | REPORTED, then VERIFIED | Agreement figures in §5.3 |
| Altimeter: id 40 is metres on some aircraft and feet on others; on some feet altimeters id 40 is the value **modulo 10,000 ft** and id 42 is the full altitude | VERIFIED (new finding) | Replay 2026.10.24 08.23.02: id 40 = id 42 mod 10,000 ft. SDL chooses the (id, unit) pair per replay |
| Replay 2026.10.23 17.07.27 has no ids 39/40/42/43/52/53 | Observation | Its aircraft's instrument set differs; no cockpit validation is possible |

Comparisons use the author state linearly interpolated (DERIVED) between the two bracketing 250 ms sync records at the cockpit timestamp.
