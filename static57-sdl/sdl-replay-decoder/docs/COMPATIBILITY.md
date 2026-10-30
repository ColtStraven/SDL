# S57-003 SDL — Replay Compatibility Record

Every genuine replay used for compatibility testing. Files were **read only**, never modified or copied into the repository.

- Source: owner's War Thunder client, `%LOCALAPPDATA%\WarThunder\Replays`
- Recording type: client replays, Air Simulator Battles
- Format magic: `0x00018C1C` (all files)
- Codec: Zstandard, one frame per file
- Game version: the header's `game_version` field, interpreted EXPERIMENTALLY as `major.minor.patch.build`
- Decoder: `sdl-replay-decoder` 0.1.0 (Milestones 1–2), 0.2.0 (Milestone 3), 0.3.0 (Milestone 4), 0.4.0 (Milestone 5), 0.5.0 (Milestone 6, work in progress)
- Reproduce: `SDL_REPLAY_DIR=<folder> cargo test --release -- --ignored`, or `sdl-replay-decoder survey <folder>`

Result: **27 of 27 decode with status `ok`.** In every file, every compressed byte is consumed, every decompressed byte is framed into packets, there are 0 unknown packet types and 0 malformed records, the clock is monotonic, and the end marker is the final record.

"Packets" counts records. "Span" is the DERIVED difference between the last and first explicit timestamps. "Acct" means every byte is accounted for.

| File | Bytes | Game version | Codec | Packets | Span (s) | Acct | SHA-256 |
|---|---|---|---|---|---|---|---|
| #2026.10.22 16.18.52.wrpl|3957005|2.59.0.80|Zstd|111543|639.7|true|a51df5020ce86a6a79c99b778145cd788c81f3d83bd1792f042ed78f6da1c386 |
| #2026.10.23 07.50.42.wrpl|373575|2.59.0.83|Zstd|4026|26.7|true|998695280b848590e8968962818b23a748a2a2c35204e75fc659ff1621316d7e |
| #2026.10.23 10.09.23.wrpl|235342|2.59.0.86|Zstd|1629|17.8|true|3bb6d8079e8d65b7f7a38eecae471f0b2ad0ddde670bfe152e2bccf0795b3d2e |
| #2026.10.23 11.20.27.wrpl|17080869|2.59.0.86|Zstd|381938|2349.6|true|148145cc912e55808d29d3638c7a0c1ebcc7319acb968e7de2029bf7b988a0a9 |
| #2026.10.23 11.36.11.wrpl|4470345|2.59.0.86|Zstd|78967|827.3|true|d97195285a9d586418e8ae1433ca91b9d6e7243bc7deae8483b6fe0b3dd59232 |
| #2026.10.23 11.39.53.wrpl|544196|2.59.0.86|Zstd|4367|53.9|true|cdca491145a2d72b729b69feb65bef0a72ebba810910dd35ebcc3bf307127d5d |
| #2026.10.23 15.24.22.wrpl|4868999|2.59.0.86|Zstd|110924|1011.8|true|af53e7a2d76d54a4c61c11b2f01a55e213ab46adc40de3384458782293e4fcad |
| #2026.10.23 15.34.36.wrpl|3197047|2.59.0.86|Zstd|64519|554.7|true|dfa0c3cb207d11d033aae161349688d64b58fe7c6983ad6d5c5808aabac89cbb |
| #2026.10.23 15.37.33.wrpl|849540|2.59.0.86|Zstd|6940|66.0|true|b48bf8e61efaf829b8f3c8bf1f4782ee7b3bf9ce39d4e89b18234312a6689b86 |
| #2026.10.23 15.46.48.wrpl|2343381|2.59.0.86|Zstd|73576|425.7|true|0d75caa99c7eb02b8173ed343753f1b3a21dc85a83400cfa04ff412df8a6de39 |
| #2026.10.23 15.47.41.wrpl|166725|2.59.0.86|Zstd|1134|4.7|true|592bca00c071b7b51d7358bb6bc2c2ced464242cf2552fae94f80b4b40a28688 |
| #2026.10.23 16.31.10.wrpl|11277373|2.59.0.86|Zstd|269716|2561.5|true|8dff5d77bf1ee42c5e2447e2daace8f427af9bcf7a6f3a4fb81419decaa273c0 |
| #2026.10.23 16.37.32.wrpl|369357|2.59.0.86|Zstd|2895|25.2|true|930074a2218fb2d2ab6ea06119af4b01738067314dd7707dec05fba715ec622a |
| #2026.10.23 17.07.27.wrpl|9719368|2.59.0.86|Zstd|211788|1758.9|true|d622e82cc511620f1e893a9d447bfa167c0c99b9888c2750043b19d5f7bbdd6e |
| #2026.10.23 17.09.25.wrpl|292135|2.59.0.86|Zstd|1549|25.5|true|12b70f7116e1c6b648c636cb4cd6ab2ffbc869a9baa93f95d78b5e27d4e10566 |
| #2026.10.24 06.51.45.wrpl|8959730|2.59.0.86|Zstd|180146|1075.9|true|cfa3f6c1c0460ef6e3846535ebcc4e41ef7dff51f1e4314df575ce8323732ecb |
| #2026.10.24 06.56.29.wrpl|897231|2.59.0.86|Zstd|6911|63.1|true|5f967128e660ea5f381232976c4cfa8458dadffcda10917047cec4d788e8db71 |
| #2026.10.24 08.23.02.wrpl|25962134|2.59.0.86|Zstd|609236|4676.6|true|11d0895710ce1865d73246c148f1f630ed152432e61458f2e179b7a08f2d2532 |
| #2026.10.24 12.32.14.wrpl|15571002|2.59.0.86|Zstd|398393|2990.7|true|7dedfd0e964ed32b0e07311b0f29d90b38dd7031c20f94fa6c995554e341edeb |
| #2026.10.24 13.34.27.wrpl|5051956|2.59.0.86|Zstd|104002|796.8|true|932409045094404dd1bca741ae30ea6f840db169dc49433d5443c66ed0588c71 |
| #2026.10.24 14.05.54.wrpl|2802437|2.59.0.86|Zstd|53047|563.0|true|6d7e4e43ec5bf6451a21f7a4399c885551c193611fe26d623a93c9b79be0e339 |
| #2026.10.24 14.23.19.wrpl|17227193|2.59.0.86|Zstd|274615|796.6|true|875a88ca6b9d0a89eb826a37fb7f569ece65e568cb9e723485be5bc05eb3a302 |
| #2026.10.24 14.25.22.wrpl|1997783|2.59.0.86|Zstd|22578|85.8|true|00b669abf1e60a90029a98d2c984404d73e42a00344dca80d9c89c3e8eef272b |
| #2026.10.24 15.03.43.wrpl|42304246|2.59.0.86|Zstd|778482|2166.1|true|67b1c1c2f85ec68657184969adb0a3d880a0af78bb4c0d01709a20cbca9f995d |
| #2026.10.24 15.28.36.wrpl|9276218|2.59.0.86|Zstd|185908|1240.1|true|0561e2c025225a60806b4a823f57ef7b1de3bbbae4ba7cd86c5a950ca3dc6df1 |
| #2026.10.25 04.37.02.wrpl|2718072|2.59.0.86|Zstd|55068|602.5|true|553ac174a88035e012c1626cf9249ddb1c1ab9229cbbea2a9ea251f242ae8eef |
| #2026.10.25 06.01.19.wrpl|5296707|2.59.0.86|Zstd|92272|589.9|true|352317cb86a051cf40c024292837f63f8c71825470ed0332cfef6c1e9ede03b5 |

Not yet tested, because no genuine sample exists: format `18c0b` (pre-2.59 zlib), `18c0a` (dev), server replay parts, and any later client builds.

## Milestone 3 — flight-state reconstruction (decoder 0.2.0)

Reproduce with `sdl-replay-decoder flight-survey <folder>`. **All 103,698 type-2 packets in the 27 replays decode structurally (0 failures).**

Column key:
- **Resid all** and **Resid author** (m): median / p99 of abs(displacement − mean decoded speed × dt) for consecutive samples ≤ 300 ms apart.
- **Dir author** (°): median angle between the author's displacement and decoded velocity.
- **Ck**: cockpit messages compared.
- **Alt id**: cockpit altimeter id and unit chosen.
- Error columns are median / p99 abs error against cockpit instruments, at the cockpit time, using interpolated author state.
- **IAS**: median abs error of ISA-converted decoded speed vs cockpit IAS (m/s).
- "–" means not available: no cockpit message, no author, or a parked author.

| Replay | Sync pkts | States | Tracks | UIDs | Resid all m | Resid author m | Dir author ° | Ck | Alt id | Alt m | VS m/s | Roll ° | Pitch ° | IAS m/s |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 2026.10.22 16.18.52 | 2546 | 59063 | 69 | 39 | 0.65 / 47.9 | 0.000 / 0.00 | – | 2517 | 40 ft | 0.153 / 0.15 | 0.010 | 0.056 / 0.06 | 0.052 / 0.05 | – |
| 2026.10.23 07.50.42 | 100 | 1379 | 16 | 16 | 2.70 / 627.3 | – / – | – | 0 | – | – | – | – | – | – |
| 2026.10.23 10.09.23 | 62 | 0 | 0 | 0 | – / – | – / – | – | 0 | – | – | – | – | – | – |
| 2026.10.23 11.20.27 | 9381 | 291388 | 147 | 63 | 1.24 / 5.7 | 0.059 / 0.24 | 0.111 | 9300 | 40 m | 0.005 / 0.12 | 0.186 | 0.055 / 1.26 | 0.045 / 0.26 | 1.46 |
| 2026.10.23 11.36.11 | 3302 | 86430 | 91 | 49 | 1.72 / 50.3 | 0.081 / 0.29 | 0.116 | 3240 | 40 m | 0.005 / 0.10 | 0.257 | 0.050 / 0.52 | 0.044 / 0.16 | 0.93 |
| 2026.10.23 11.39.53 | 210 | 4440 | 26 | 25 | 1.20 / 74.7 | – / – | – | 0 | – | – | – | – | – | – |
| 2026.10.23 15.24.22 | 4037 | 62710 | 67 | 38 | 1.56 / 70.2 | 0.044 / 0.31 | 0.092 | 3836 | 40 m | 0.004 / 0.10 | 0.140 | 0.039 / 4.54 | 0.042 / 0.87 | 1.97 |
| 2026.10.23 15.34.36 | 2204 | 59985 | 75 | 38 | 1.62 / 64.1 | 0.000 / 0.00 | – | 1850 | 40 m | 0.000 / 0.00 | 0.005 | 0.002 / 0.00 | 0.020 / 0.02 | – |
| 2026.10.23 15.37.33 | 251 | 3674 | 26 | 24 | 2.20 / 159.1 | 0.000 / 0.00 | – | 162 | 40 m | 0.000 / 0.00 | 0.023 | 0.005 / 0.00 | 0.040 / 0.04 | – |
| 2026.10.23 15.46.48 | 1689 | 27571 | 31 | 22 | 1.59 / 48.2 | 0.054 / 0.20 | 0.110 | 1542 | 42 ft | 0.655 / 1.00 | 0.146 | 0.050 / 2.83 | 0.047 / 1.49 | 0.73 |
| 2026.10.23 15.47.41 | 9 | 164 | 19 | 19 | 40.26 / 68.0 | 0.000 / 0.00 | – | 0 | – | – | – | – | – | – |
| 2026.10.23 16.31.10 | 10238 | 190234 | 135 | 54 | 1.82 / 50.7 | 0.064 / 0.25 | 0.116 | 10177 | 40 m | 0.011 / 0.11 | 0.218 | 0.052 / 1.11 | 0.045 / 0.57 | 1.76 |
| 2026.10.23 16.37.32 | 92 | 2539 | 30 | 28 | 1.61 / 801.2 | – / – | – | 0 | – | – | – | – | – | – |
| 2026.10.23 17.07.27 | 7027 | 217578 | 198 | 79 | 1.76 / 68.9 | 0.047 / 0.25 | 0.102 | 6866 | – | – | – | – | – | – |
| 2026.10.23 17.09.25 | 92 | 828 | 9 | 9 | 2.31 / 187.7 | – / – | – | 0 | – | – | – | – | – | – |
| 2026.10.24 06.51.45 | 4292 | 114793 | 96 | 57 | 1.45 / 9.3 | 0.062 / 0.29 | 0.103 | 4074 | 42 m | 0.012 / 0.18 | – | 0.042 / 3.47 | 0.044 / 0.82 | 2.11 |
| 2026.10.24 06.56.29 | 243 | 3830 | 21 | 21 | 2.29 / 141.4 | – / – | – | 0 | – | – | – | – | – | – |
| 2026.10.24 08.23.02 | 18699 | 404550 | 240 | 67 | 1.98 / 59.1 | 0.061 / 0.27 | 0.099 | 18493 | 42 ft | 0.294 / 1.33 | 0.189 | 0.042 / 3.28 | 0.049 / 0.71 | 1.03 |
| 2026.10.24 12.32.14 | 11951 | 262973 | 253 | 70 | 2.01 / 60.2 | 0.072 / 0.30 | 0.103 | 11753 | 42 ft | 0.370 / 2.67 | 0.238 | 0.054 / 3.94 | 0.047 / 0.83 | 0.90 |
| 2026.10.24 13.34.27 | 3170 | 54534 | 42 | 27 | 1.50 / 29.7 | 0.051 / 0.21 | 0.093 | 3011 | 40 ft | 0.203 / 0.46 | 0.137 | 0.068 / 5.83 | 0.053 / 0.75 | 2.67 |
| 2026.10.24 14.05.54 | 2245 | 35613 | 33 | 24 | 1.66 / 46.7 | 0.066 / 0.27 | 0.105 | 1705 | 40 m | 0.022 / 0.27 | 0.248 | 0.065 / 3.72 | 0.051 / 0.77 | 3.25 |
| 2026.10.24 14.23.19 | 3161 | 76197 | 174 | 32 | 1.31 / 44.3 | 0.067 / 0.30 | 0.126 | 3011 | 40 m | 0.016 / 0.43 | 0.228 | 0.207 / 6.68 | 0.055 / 2.00 | 0.58 |
| 2026.10.24 14.25.22 | 325 | 8454 | 55 | 32 | 1.02 / 55.4 | 0.054 / 0.10 | 0.373 | 122 | 40 ft | 0.014 / 0.02 | 0.010 | 0.021 / 0.08 | 0.029 / 0.35 | 0.12 |
| 2026.10.24 15.03.43 | 8653 | 259024 | 342 | 32 | 1.00 / 5.3 | 0.052 / 0.23 | 0.222 | 8434 | 42 ft | 0.042 / 0.28 | 0.121 | 0.059 / 7.61 | 0.056 / 1.80 | 0.56 |
| 2026.10.24 15.28.36 | 4948 | 134185 | 109 | 54 | 1.43 / 10.7 | 0.062 / 0.25 | 0.108 | 4717 | 42 ft | 0.332 / 0.94 | 0.198 | 0.066 / 6.35 | 0.063 / 1.08 | 2.37 |
| 2026.10.25 04.37.02 | 2411 | 54709 | 77 | 40 | 0.06 / 0.2 | 0.062 / 0.23 | 0.147 | 2198 | 42 ft | 0.649 / 1.19 | 0.232 | 0.235 / 2.70 | 0.075 / 1.99 | 3.91 |
| 2026.10.25 06.01.19 | 2360 | 51178 | 79 | 38 | 0.06 / 0.2 | 0.065 / 0.22 | 0.127 | 2149 | 40 ft | 0.381 / 0.79 | 0.238 | 0.155 / 2.88 | 0.071 / 0.76 | 4.58 |
| **Total** | **103,698** | **2,468,023** | **2,460** | **997** | | | | **99,157** | | | | | | |

Notes:
- 2026.10.23 10.09.23 contains no aircraft state records (62 sync packets, all entries no-data).
- Seven replays have no `0xD04A` messages, and 2026.10.23 17.07.27 has a cockpit set without altitude or attitude ids, so cockpit validation covers 19 replays.
- Replays where the author is parked show constant, exactly matching values. These are not evidence of in-flight accuracy.

## Milestone 6 — all 42 replays (decoder 0.5.0, 2026-10-30)

42 of 42 decode with status `ok`, on game versions 2.59.0.80, .83, .86 and .92. Coverage is in [FINDINGS_M6_ENGAGEMENTS.md](FINDINGS_M6_ENGAGEMENTS.md). The launcher entity in store creation data was found in all of those versions.

## Milestone 4 — all 42 replays (decoder 0.3.0, 2026-10-30)

Reproduce with `sdl-replay-decoder research-survey <folder>`. **42 of 42 decode with status `ok`. All 93,810 `ShellsData` messages consume exactly their declared length, and all 32,121 ECS creation packets frame with 0 failures.**

Additional replays recorded 2026-10-29 (the "Packets" column here counts aircraft sync packets):

| File | Bytes | Game version | Codec | Sync packets | Span (s) | Acct | SHA-256 |
|---|---|---|---|---|---|---|---|
| #2026.10.29 17.46.28.wrpl | 6590517 | 2.59.0.92 | Zstd | 4300 | 1077.391 | true | a5d4a3907610b45f808eb94e9079e8c8bccbd7f7529c14b890174d605fc8d2f2 |
| #2026.10.29 17.52.20.wrpl | 2437491 | 2.59.0.92 | Zstd | 1211 | 304.847 | true | 8b695ba6905746120111e0b6819d9b615e7139277919749795b89f0fb3e4ecd2 |
| #2026.10.29 17.59.38.wrpl | 3035635 | 2.59.0.92 | Zstd | 1680 | 421.627 | true | a438c0479bbd7925e1e4c91722bfea479d6472e77b5c9e3b836913d935361305 |
| #2026.10.29 18.12.52.wrpl | 966465 | 2.59.0.92 | Zstd | 669 | 168.149 | true | 9adb24ea50b6aba7144bb2629241ef1e346a7c19231ee89af4978aa326fc0a8a |
| #2026.10.29 19.16.29.wrpl | 23887598 | 2.59.0.92 | Zstd | 14441 | 3613.157 | true | 3e95ae58edaa5bd054f7eb036a5b8f5e73f06018949cbc5ae650f5cb6ae39d05 |
| #2026.10.29 19.34.21.wrpl | 3556970 | 2.59.0.92 | Zstd | 2313 | 580.629 | true | c4fbc3a8359a0606a77d66de734e67f89190de5afb35b453f2b16be5a213e93c |
| #2026.10.29 19.49.34.wrpl | 4692740 | 2.59.0.92 | Zstd | 3233 | 810.367 | true | add305afcd0016a70c516e98cc616d8a615a5ef5f8d0f9314df7789e8664590d |
| #2026.10.29 20.06.24.wrpl | 5482377 | 2.59.0.92 | Zstd | 3881 | 973.171 | true | 538764b8a2ee7e13c0b95f0402251b15689c5b0bedef21dd64fbab50207d0e99 |
| #2026.10.29 20.14.12.wrpl | 2958554 | 2.59.0.92 | Zstd | 1208 | 304.573 | true | 151db012bb4b630aaf113ddebf69578686c83cc67c86be6dd7f5e991c8129f31 |
| #2026.10.29 20.21.00.wrpl | 2590032 | 2.59.0.92 | Zstd | 1499 | 376.862 | true | 5578efeaf6d24385059d7ba10a570d4e488d6e164712c6f12eaf1445bc814886 |
| #2026.10.29 20.23.08.wrpl | 1304494 | 2.59.0.92 | Zstd | 389 | 100.298 | true | a2b30be4526cff362229a28e1f3fbf73ecf0f8025c1813958417198c468e93c9 |
| #2026.10.29 20.34.30.wrpl | 2360602 | 2.59.0.92 | Zstd | 1221 | 307.591 | true | 225aa27fd51ddf68131d0bb2bb13cabbd179d27197b7af8de3b0346a6332f69b |
| #2026.10.29 20.49.14.wrpl | 17052284 | 2.59.0.92 | Zstd | 2787 | 702.047 | true | f34ef4d3558e699d8bf04c4cc463df91f63a417595f004463de17e2143060da2 |
| #2026.10.29 21.35.10.wrpl | 268389 | 2.59.0.92 | Zstd | 58 | 16.479 | true | e019e92ffc9ddba2f2478c27e2efeb6fce685c540e9214defe35dac8a5322acf |
| #2026.10.29 21.51.38.wrpl | 1908325 | 2.59.0.92 | Zstd | 1113 | 280.586 | true | 6d7e1dd82374520e8a519c8ec3c567c0e6d321179885226b0f10559274f7f325 |

Extraction by replay:

| Replay | Game version | Author vehicle | Tracks | ShellsData msgs | Weapons | By author | Named | Kills |
|---|---|---|---|---|---|---|---|---|
| 2026.10.22_16.18.52 | 2.59.0.80 | a_10a_early | 69 | 402 | 39 | 0 | 7 | 28 |
| 2026.10.23_07.50.42 | 2.59.0.83 | none | 16 | 47 | 18 | 0 | 0 | 3 |
| 2026.10.23_10.09.23 | 2.59.0.86 | none | 0 | 0 | 0 | 0 | 0 | 0 |
| 2026.10.23_11.20.27 | 2.59.0.86 | saab_a32a_red_adam | 147 | 1606 | 122 | 0 | 8 | 66 |
| 2026.10.23_11.36.11 | 2.59.0.86 | mig-17_f5 | 91 | 843 | 29 | 2 | 4 | 26 |
| 2026.10.23_11.39.53 | 2.59.0.86 | none | 26 | 26 | 9 | 0 | 1 | 2 |
| 2026.10.23_15.24.22 | 2.59.0.86 | su_22m4_de_wtd61 | 67 | 536 | 97 | 2 | 3 | 21 |
| 2026.10.23_15.34.36 | 2.59.0.86 | su_22m4_de_wtd61 | 75 | 484 | 44 | 0 | 3 | 19 |
| 2026.10.23_15.37.33 | 2.59.0.86 | ffa_p16 | 26 | 31 | 30 | 0 | 0 | 0 |
| 2026.10.23_15.46.48 | 2.59.0.86 | meteor_nfmk13 | 31 | 4 | 1 | 0 | 0 | 3 |
| 2026.10.23_15.47.41 | 2.59.0.86 | ariete | 19 | 0 | 0 | 0 | 0 | 0 |
| 2026.10.23_16.31.10 | 2.59.0.86 | mig-17_f5 | 135 | 1897 | 94 | 6 | 2 | 21 |
| 2026.10.23_16.37.32 | 2.59.0.86 | none | 30 | 0 | 0 | 0 | 0 | 0 |
| 2026.10.23_17.07.27 | 2.59.0.86 | j_22m1a | 198 | 2555 | 101 | 8 | 5 | 43 |
| 2026.10.23_17.09.25 | 2.59.0.86 | none | 9 | 0 | 0 | 0 | 0 | 0 |
| 2026.10.24_06.51.45 | 2.59.0.86 | jh_7a_prototype | 96 | 3911 | 140 | 11 | 9 | 66 |
| 2026.10.24_06.56.29 | 2.59.0.86 | none | 21 | 68 | 16 | 0 | 1 | 5 |
| 2026.10.24_08.23.02 | 2.59.0.86 | harrier_t10 | 240 | 15273 | 289 | 27 | 9 | 74 |
| 2026.10.24_12.32.14 | 2.59.0.86 | mirage_milan | 253 | 1756 | 75 | 0 | 4 | 48 |
| 2026.10.24_13.34.27 | 2.59.0.86 | a_10a_early | 42 | 3425 | 38 | 10 | 1 | 27 |
| 2026.10.24_14.05.54 | 2.59.0.86 | su_22m4_de_wtd61 | 33 | 423 | 11 | 2 | 3 | 10 |
| 2026.10.24_14.23.19 | 2.59.0.86 | j_7d | 174 | 7896 | 752 | 4 | 33 | 308 |
| 2026.10.24_14.25.22 | 2.59.0.86 | hawk_209_indonesia | 55 | 639 | 42 | 0 | 2 | 28 |
| 2026.10.24_15.03.43 | 2.59.0.86 | f_111f_japan_killstreak, hawk_209_indonesia | 342 | 20101 | 2287 | 21 | 87 | 710 |
| 2026.10.24_15.28.36 | 2.59.0.86 | hawk_209_indonesia | 109 | 1813 | 39 | 6 | 6 | 36 |
| 2026.10.25_04.37.02 | 2.59.0.86 | ariete | 77 | 0 | 0 | 0 | 0 | 3 |
| 2026.10.25_06.01.19 | 2.59.0.86 | ariete | 79 | 0 | 0 | 0 | 0 | 4 |
| 2026.10.29_17.46.28 | 2.59.0.92 | f_16xl | 107 | 3908 | 114 | 7 | 5 | 16 |
| 2026.10.29_17.52.20 | 2.59.0.92 | saab_ja37di_f21 | 35 | 1537 | 27 | 4 | 4 | 10 |
| 2026.10.29_17.59.38 | 2.59.0.92 | saab_ja37di_f21 | 42 | 1485 | 62 | 4 | 2 | 11 |
| 2026.10.29_18.12.52 | 2.59.0.92 | saab_ja37di_f21 | 7 | 0 | 0 | 0 | 0 | 5 |
| 2026.10.29_19.16.29 | 2.59.0.92 | saab_ja37di_f21 | 288 | 12889 | 343 | 35 | 36 | 120 |
| 2026.10.29_19.34.21 | 2.59.0.92 | f-4s | 82 | 330 | 4 | 0 | 1 | 6 |
| 2026.10.29_19.49.34 | 2.59.0.92 | av_8b_na | 109 | 160 | 20 | 0 | 11 | 18 |
| 2026.10.29_20.06.24 | 2.59.0.92 | av_8b_na, f-4s | 91 | 473 | 48 | 0 | 2 | 19 |
| 2026.10.29_20.14.12 | 2.59.0.92 | saab_j35a | 76 | 1010 | 49 | 4 | 2 | 11 |
| 2026.10.29_20.21.00 | 2.59.0.92 | saab_ja37di_f21 | 55 | 582 | 50 | 0 | 3 | 6 |
| 2026.10.29_20.23.08 | 2.59.0.92 | saab_ja37di_f21 | 45 | 105 | 6 | 0 | 1 | 3 |
| 2026.10.29_20.34.30 | 2.59.0.92 | f_16xl | 34 | 1064 | 32 | 3 | 1 | 12 |
| 2026.10.29_20.49.14 | 2.59.0.92 | saab_ja37di_f21 | 137 | 6461 | 782 | 3 | 26 | 253 |
| 2026.10.29_21.35.10 | 2.59.0.92 | none | 9 | 0 | 0 | 0 | 0 | 0 |
| 2026.10.29_21.51.38 | 2.59.0.92 | ariete | 46 | 70 | 7 | 0 | 1 | 10 |

Not yet tested: `18c0b` (pre-2.59 zlib), `18c0a` (dev), server replay parts, and builds after 2.59.0.92.
