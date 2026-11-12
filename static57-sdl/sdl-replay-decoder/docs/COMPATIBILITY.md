# S57-003 SDL — Replay Compatibility Record

Every genuine replay used for compatibility testing. Files were **read only**, never modified or copied into the repository.

- Source: owner's War Thunder client, `%LOCALAPPDATA%\WarThunder\Replays`
- Recording type: client replays, Air Simulator Battles. Aircraft, where identified: Me 262 A-1a/U4 (2026-11-09..10), F-84B (2026-11-11)
- Format magic: `0x00018C1C` (all files up to 2026-11-10); `0x00018C2D` (2026-11-11, game build 2.61)
- Codec: Zstandard, one frame per file
- Game version: the header's `game_version` field, interpreted EXPERIMENTALLY as `major.minor.patch.build`
- Decoder: `sdl-replay-decoder` 0.1.0. First 27 files tested 2026-10-29. The 28th file was tested 2026-11-09 and the 29th on 2026-11-10, on a second machine (Rust 1.99.1, GNU toolchain), where the first 27 were not available to re-run
- Reproduce: `SDL_REPLAY_DIR=<folder> cargo test --release -- --ignored`, or `sdl-replay-decoder survey <folder>`

Result: **30 of 30 decode with status `ok`.** In every file, every compressed byte is consumed, every decompressed byte is framed into packets, there are 0 unknown packet types and 0 malformed records, the clock is monotonic, and the end marker is the final record.

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
| #2026.11.09 18.50.56.wrpl|3640549|2.59.0.100|Zstd|217608|665.7|true|84aff3c26e7bca3306d1df31e2d308337fdd97eebf52bb01cf0d1556180a183f |
| #2026.11.10 04.23.23.wrpl|4661984|2.59.0.100|Zstd|274214|811.1|true|f4b1b2ae004f5abd16775dba7cfc7965958ec9ec6595d839e8d2e035fdc75b33 |
| #2026.11.11 17.32.49.wrpl|4954983|2.61.0.11|Zstd|273366|796.3|true|af9af7ba4c80cd4204357d5d8d4cd2ec6cce05f03693b0ee218d4b44ff5f9f23 |

Not yet tested, because no genuine sample exists: format `18c0b` (pre-2.59 zlib), `18c0a` (dev), server replay parts, and client builds other than those listed above.
