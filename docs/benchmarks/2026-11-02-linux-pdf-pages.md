# Linux PDF pages through the confined helper, release, 2026-11-02

This is the release measurement that [the Linux PDF pages review](../development/2026-11-02-linux-pdf-pages-architecture.md#5-measurement-and-native-evidence) asks for, taken after PDF-T3 landed, at `4cfc9a39` on `claude/omarchy-support`. The opt-in benchmark `crates/app/tests/pdf_pages_bench.rs` drives the release `gitturtle` executable (embedded commit checked) as the app's client does: one confined helper per request, 20 runs per page. The data is in [`2026-11-02-linux-pdf-pages/`](2026-11-02-linux-pdf-pages/): `record.json` (the benchmark's own record: fixture names and SHA-256, per-run phases, the executable's identity) and `fixtures.sha256`.

## Host and fixtures

Asahi Linux on an M2 Pro (AArch64), kernel 7.1.12, governor `schedutil`, the benchmark pinned to the eight P-cores (3,264 MHz maximum); build slots held quiet; load average 1.9 during the 14 s run (one IDE process kept about a core busy, and P-core frequency dipped to 1.7–2.4 GHz in some one-second samples). The executable was in the page cache; a dropped-cache start was not measured.

Fixtures: six files from hayro's test corpus at `ea9c81dc` (embedded CJK CID font, a scanned JPEG 2000 page, a JBIG2 scan, a CMYK image with an ICC profile, a luminosity soft mask, a Coons-patch mesh shading), a generated 300-page dense-text document and a generated 32 MiB image-heavy document (Flate-compressed), plus the benchmark's own two in-memory documents. Hashes are in `fixtures.sha256` and the record.

## Result

All 760 requests succeeded. Times are milliseconds, warm p50 / p95 over 19 runs (so p95 is the slowest run).

| Document, page | Whole request | First request | Render | Start + transfer + parse p95 (share of request p95) |
|---|---|---|---|---|
| Dense text, 300 pages (first, middle, last) | 13.2–17.7 / ≤ 26.3 | 10.1–17.9 | 9.0–13.7 / ≤ 20.8 | ≤ 4.1 (0.11–0.21) |
| Image-heavy, 32 MiB | 14.3–42.8 / ≤ 67.3 | 13.2–45.2 | 3.2–31.1 / ≤ 52.2 | ≤ 15.0 (0.21–0.75) |
| Embedded CJK CID font | 2.8–4.2 / ≤ 5.1 | 2.5–3.4 | ≤ 1.3 / ≤ 1.6 | ≤ 2.7 (0.49–0.56) |
| Scanned JPEG 2000 | 29.2 / 35.3 | 28.2 | 27.8 / 33.4 | 2.0 (0.06) |
| JBIG2 scan | 12.6 / 18.4 | 13.2 | 11.1 / 16.5 | 1.8 (0.10) |
| CMYK with ICC | 23.0 / 23.3 | 23.2 | 13.5 / 13.7 | 2.0 (0.09) |
| Luminosity soft mask | 1.3 / 2.2 | 1.4 | 0.2 / 0.4 | 1.8 (0.80) |
| Mesh shading | 20.4 / 36.8 | 20.6 | 19.3 / 34.7 | 2.0 (0.05) |

Helper start (spawn until `/proc` shows it confined): 2.02 ms for the session's first, 0.83 / 0.85 ms warm. `VmSize` at confinement about 77 MiB (78,880 KiB at most), `VmRSS` about 9 MiB. The helper's own peak (`VmHWM`, read separately) is at most 51.6 MiB, on the 32 MiB document. Kill to reap: 0.08 ms idle, under 0.5 ms while rendering. The release executable is 667,904 bytes (0.66%) larger than `gitturtle-8b916c5`, a span of 47 commits that also changed other views, so this is not T3's cost alone (the renderer and helper were already in that build).

## Budgets

- Warm next page p95 at most 120 ms for text (worst 26.3 ms) and 400 ms for image-heavy pages (worst 67.3 ms): pass.
- A first page adds at most 60 ms (at most 4.1 ms over the warm median, with the executable cached): pass.
- Legitimate pages under 2 s (slowest 67.3 ms): pass.
- Twice the largest legitimate peak within 1 GiB (2 × 51.6 MiB): pass. `VmSize` at confinement at most 512 MiB (77 MiB): pass.
- Start, transfer and parse above 40 ms at p95: not reached (worst 15.0 ms). Above 35% of the warm next page: reached for the image-heavy documents (piping 32 MiB costs about 11–12 ms a request), the CJK document and the soft mask (fixed 1–2 ms in requests of 1–5 ms). **Decision (coordinator, 2026-11-02):** no per-document helper for now. The overhead is at most 15 ms against budgets of 120 and 400 ms, and a helper per request keeps a hostile document from carrying state from one page into the next; revisit if a real repository's documents make the transfer dominate a page that takes longer than a frame budget would allow.

## Limitations

One session, no repeat run; on this host results move by a few milliseconds between sessions, which matters for the 1–5 ms documents and the 35% ratios, not for the budgets. The benchmark's own `maxrss_kib` reads the test process's high-water mark (the child inherits it at `exec`), not the helper's, so the helper's peak above comes from `/proc/<pid>/status` `VmHWM` read separately; fixing the benchmark to record it is queued. UI frame times while a 2 s page renders were not measured here; native QA found History's filter answering in about 95 ms during a slow page and the window responsive (see `docs/validation.md`, November 2 afternoon).
