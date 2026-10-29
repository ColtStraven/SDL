# S57-003 SDL — `.wrpl` Format Findings

Decoder: `sdl-replay-decoder` 0.1.0. Date: 2026-10-29.
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
| 2 | aircraft_small | 103,698 | 247,873,046 | 36..5,596 | Flight-model sync. Target of Milestone 3 |
| 3 | chat | 417 | 27,919 | 23..154 | In 18 of 27 replays |
| 4 | mpi | 3,485,535 | 312,940,321 | 4..27,726 | Most frequent type |
| 6 | ecs | 496,438 | 51,796,163 | 4..208,943 | |
| 8 | replay_header_info | 27 | 8,100 | 300..300 | Always first; payload always 300 bytes |
| 5, 7 | next_segment, snapshot | 0 | — | — | Not present in client replays. Reported for server parts |

Type names come from third-party documentation. **No payload has been decoded or validated by Static 57.** Every record's `decode_state` is `raw`.

## 4. Open questions

1. The `aircraft_small` payload layout: bit-packed, quantized flight state per the references. Needs a decoder, plus validation against localhost:8111 telemetry.
2. What the `replay_header_info` 300-byte payload means (reported as ECS message hashes).
3. Server-replay segmentation (`next_segment`, part numbering): needs genuine server parts.
4. The meaning of the BLK leading byte `0x01`, and a native BLK reader for settings and results.
5. The `unk0`, `unk1`, `unk3`, `unk4` and `unk5` header regions. They are preserved raw in every manifest. Example: `unk5` = `01 00…` and `unk3` ends `ff ff ff ff`.
6. The cause of non-canonical size prefixes.
