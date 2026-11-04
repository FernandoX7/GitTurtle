# Linux AVIF and JPEG 2000 through the confined helper, release, 2026-11-03

This is the release measurement that [the Linux AVIF and JPEG 2000 review](../development/2026-11-02-linux-avif-jpeg2000-architecture.md#5-measurement-and-native-evidence) asks for (slice A4), taken after A4 landed, at `36d977aa` on `claude/omarchy-support`. The opt-in benchmark `crates/app/tests/image_decode_bench.rs` drives the release `gitturtle` executable (embedded commit checked, source tree clean) as the app's client does: one confined helper per request, 20 runs per image at History's 1,600-pixel edge, and every confined image of at most 512 KiB again at Markdown's 1,000-pixel edge; PNG and JPEG go through `decode_image` in process for comparison. The data is in [`2026-11-03-linux-avif-jpeg2000/`](2026-11-03-linux-avif-jpeg2000/): `record.json` (the benchmark's own record: input names and SHA-256, per-run phases, the executable's identity) and `fixtures.sha256`.

## Host and inputs

Asahi Linux on an M2 Pro (AArch64, 12 CPUs), kernel 7.1.12, governor `schedutil`, system libdav1d 1.5.4; build slots held quiet; load average 2.46 at the start of the 442 s run (one IDE process kept about a core busy). The executable was in the page cache.

Inputs come from `scripts/create-image-bench-inputs.py` (ffmpeg 9.0.1, aomenc 3.15.0, OpenJPEG 2.5.4, Pillow 13.0.0), plus one JP2 encoded by hand for the Markdown bound: the 5600 × 3700 no-levels pixels in 88 × 58 tiles of 64 × 64 (`opj_compress -t 64,64 -n 1 -r 200`, 143 KB), to check whether many small tiles are slower than the generator's 1,024-pixel tiles at the same size. Hashes are in `fixtures.sha256`.

## Result

Every request succeeded (no input has an error in the record). Times are milliseconds through the app's client, first request / warm p50 / warm p95 (19 warm runs, so p95 is the slowest).

| Input | First | p50 | p95 | Helper decode + convert p95 | Peak (`VmHWM`, KiB) |
|---|---|---|---|---|---|
| AVIF 4032 × 3024, 8-bit 4:2:0 | 317 | 327 | **344** (budget 350) | 187 + 144 | 146,704 |
| AVIF 4032 × 3024, 10-bit 4:2:0 + alpha | 425 | 426 | **443** (budget 600) | 215 + 222 | 145,328 |
| AVIF 6720 × 4480, 12-bit 4:4:4 + alpha + grain (largest accepted) | 1,014 | 1,005 | 1,024 | 545 + 470 | **383,568** |
| AVIF 6912 × 4608, 12-bit 4:4:4 + grain, alpha also 4:4:4 + grain (hostile) | 880 | 875 | 891 | 379 + 503 | **522,096** |
| JP2 6000 × 4000 lossy, reduced resolution | 311 | 314 | **336** (budget 400) | 279 + 44 | 159,360 |
| JP2 6000 × 4000 lossless | 1,308 | 1,317 | 1,351 | 1,277 + 44 | 183,904 |
| JP2 5600 × 3700, no levels, 1,024 tiles (slowest accepted) | 3,146 | 3,146 | **3,169** (budget 3,000) | 3,057 + 95 | 343,680 |
| JP2 3000 × 2000 with an ICC profile (one level deeper) | 139 | 137 | 180 | 122 + 26 | 56,448 |
| JP2 3264 × 1836 with an ICC profile / padded past 10 MB | 126 / 221 | 134 / 224 | 140 / 247 | 116 + 23 / 205 + 23 | 56,448 / 134,896 |
| JP2 3000 × 2000 CMYK | 338 | 342 | 361 | 307 + 45 | 219,888 |
| J2K 64 × 64 | 5.2 | 3.7 | 6.9 | 2.3 + 0.0 | 10,448 |
| PNG / JPEG 4032 × 3024, in process | 189 / 162 | 189 / 161 | 191 / 164 | | |
| PNG / JPEG 6000 × 4000, in process | 342 / 257 | 342 / 257 | 349 / 257 | | |

The 3000 × 2000 ICC JP2, refused before A4's fix passes ("Decoded image exceeds the 256 MiB limit"), now decodes one level deeper and replies 1,600 × 1,067.

Helper start (spawn to confined) 2.1 ms first, warm p95 2.2 ms; `VmSize` once confined p95 80,992 KiB, `VmRSS` 9,136 KiB; kill-to-reap idle p95 0.39 ms, at work 0.12–1.47 ms (budget 10 ms). Request overhead outside the helper's decode (spawn, input write, reply read, reap) p95 3–35 ms, the larger figures from writing 23–29 MB inputs.

**Requirement 18** (the largest accepted AVIF against half of `RLIMIT_DATA`, 512 MiB): the designated input peaks at 383,568 KiB (375 MiB), within budget. The hostile grained alpha, whose two items each hold a reference and a grained picture, peaks at 522,096 KiB (509.9 MiB), within 512 MiB by 2,192 KiB; it stays below the 1 GiB limit either way, so the architecture doc's question 8 action (taken only above 512 MiB) does not apply.

**Markdown's bound** (requirement 17): the slowest confined image of at most 512 KiB at the 1,000-pixel edge is the hostile AVIF (460 KB) at 848 ms warm p95 (T). Its 32 references on one side would take 27.1 s; the document total stops them at 10.85 s (the deadline plus T). The many-small-tiles JP2 (225 ms p95 at 1,600 px) is faster than the 1,024-tile one (338 ms).

## Budgets

| Proposed budget (warm p95) | Measured | |
|---|---|---|
| 12 MP 8-bit AVIF ≤ 350 ms | 344 ms | met |
| 12 MP 10-bit AVIF with alpha ≤ 600 ms | 443 ms | met |
| Lossy 24 MP JP2 at reduced resolution ≤ 400 ms | 336 ms | met |
| Slowest accepted file ≤ 3 s (30 % of the deadline) | 3.17 s (32 %) | missed by 0.17 s |
| Helper start and exit ≤ 10 ms | 2.2 ms, 1.5 ms | met |
| Largest accepted peak ≤ 512 MiB | 375 MiB (hostile alpha 509.9 MiB) | met |
| PNG and JPEG unchanged | in process, never in the helper | met |

**Decision (coordinator):** the slowest-accepted miss is accepted rather than met by lowering the 256 MiB budget. The file is the synthetic worst case the budget admits, a JP2 with no decomposition levels decoded whole (3.06 s of its 3.17 s in hayro's decode), and real encoders write levels: the same pixels at 24 MP with levels decode in 336 ms. At 32 % of the 10 s deadline the helper keeps about three times its decode before the kill. Shrinking the budget to fit would refuse legitimate images to meet a target set for margin. Queued: profile hayro-jpeg2000 0.4.1's full-resolution decode, and the same measurement on x86-64 when a host is available.

Not measured here: UI frame times while a slow JP2 decodes (the native evidence round checks that the first frame is not blocked); a dropped-cache start; x86-64.
