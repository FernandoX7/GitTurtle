# Native Markdown baseline memory investigation — 2026-09-10

The baseline process rose from a 139.1 MiB physical footprint before Markdown activation to 794.9 MiB in the first post-activation VM snapshot. A later snapshot was 575.6 MiB. Large regions labelled `MALLOC_LARGE (empty)` and a temporary increase in graphics residency were observable; these captures do not identify every allocation responsible for the peak, establish a leak, or demonstrate an improvement.

The [sanitized numeric record](security-markdown-20260910.json) contains timestamps, selected VM categories, stack observations, source hashes and limitations. Original VM maps, heap output, samples, application state and profiling traces remain private. No raw user paths or memory addresses are copied here.

## Build, fixture and method

- Compiled baseline source, from the installed handoff: `eb3dd264c3d03eda7a3e937d307bf100c603ef02`. The checkout's final handoff documentation commit was `b36efd036a4c61931844726ca526833aa1208098`; it is not a different compiled application.
- Exercised executable UUID, also present in the sample: `37B51CFC-5658-3D0D-A83A-0C87DC4FA564`. The backed-up executable SHA-256 is `b2deab6aec9d33a77caab0fad7a814a927fbe862c7705f9effed3dcfdd70b610`.
- Host: macOS 26.6.2 (25G83), arm64, `Mac16,5`, 128 GiB memory and 16 logical CPUs. VM pages are 16 KiB. The native viewport was 1480 × 981 with Midnight/Comfortable appearance.
- The disposable comparison contains 1,156 bytes of original Markdown and 1,153 bytes of changed Markdown, Mermaid content and a 320 × 160 local image. The JSON records both Markdown content hashes.
- This evidence covers one Markdown activation in a fresh second baseline process, launched at `03:39:24.881 -05:00`. The sample includes initialization of the SVG font database in that process. OS/filesystem caches were not cleared, and background load was not controlled or quantified.

`vmmap` snapshots, a `heap` inventory and a requested 12-second `sample` recording at 1 ms intervals were collected around that activation. The native trace reports **41.210 ms** for the commit changed-file frame and **1,583.827 ms** for the file-preview frame. These are individual observations without a latency distribution. The [measurement contract](../../.agents/skills/gitturtle-performance/references/measurement.md) defines these as handler-to-frame-callback measurements; they exclude input delivery, OS presentation and completed GPU execution.

## Observed memory categories

All table entries are MiB converted from the tools' rounded binary K/M/G output. “Not listed” means the category was absent from the summary, rather than a separately measured exact zero. All three VM snapshots belong to the same process.

| Metric | Before Markdown, 03:39:37.445 | Initial Markdown, 03:39:52.890 | Later settled, 03:40:23.176 |
| --- | ---: | ---: | ---: |
| Physical footprint | 139.1 | 794.9 | 575.6 |
| Process lifetime footprint peak | 385.4 | 794.9 | 802.0 |
| `MALLOC_LARGE (empty)`, resident | Not listed | 416.6 | 420.9 |
| `MALLOC_LARGE`, resident | Not listed | 4.266 | Not listed |
| Total allocated bytes reported by malloc zones | 15.4 | 20.5 | 18.0 |
| `MALLOC_SMALL`, resident | 18.7 | 22.7 | 23.5 |
| `owned unmapped (graphics)`, resident | 32.0 | 256.0 | 72.0 |
| `IOSurface`, resident | 67.6 | 67.6 | 67.6 |

At `03:39:53.940 -05:00`, the separate heap inventory reported **65,063 live allocations totalling 19,070,169 bytes (18.187 MiB)** while its process footprint was 801.6 MiB. This inventory is later than the initial VM snapshot and uses different accounting. It does not measure all process, graphics or allocator residency.

The initial footprint increase was 655.8 MiB. The graphics category alone gained 224.0 MiB of resident memory, while `MALLOC_LARGE (empty)` appeared at 416.6 MiB. These are two large changing VM categories, not proof of the allocating call sites or a complete accounting of the peak. By the later snapshot, footprint fell 219.3 MiB and graphics residency fell, but 420.9 MiB remained labelled as empty large-malloc regions. The process lifetime peak counter can remain high after current usage falls and must not be interpreted as current live allocation size.

## Stack evidence and attribution limits

The sample began at `03:39:50.548 -05:00`. The main and reader threads each had 10,325 sampled observations. The reader's call tree contains:

```text
worker::attach_mermaid                         1,043 inclusive samples
  markdown_view::prepare_document
    gitturtle_preview::mermaid::preview
      decode_image / SVG decode
        OnceLock fontdb initialization         1,034 inclusive samples
          fontdb::Database::load_system_fonts
            load_font_file_impl → filesystem read/open
```

This establishes that first-use system-font discovery/loading occurred on the background reader during the activation. It makes that path a concrete subject for further allocation investigation. It does **not** connect the empty large-malloc regions to individual font reads or explain the graphics allocation increase. Nested sample counts are inclusive and must not be added together. They include I/O waits and are not allocation bytes or measured CPU utilization.

The main-thread call tree includes 10,307 samples in its run-loop `mach_msg2_trap` branch. That indicates the main thread was mostly waiting during this recording; it does not prove every frame was responsive or rule out a short stall outside the observed samples.

An earlier `xctrace` Allocations attachment failed, despite writing a trace container. Its log explicitly reports failure to attach, so that file supplies no usable allocation attribution. Later malloc-stack-logging attempts ran in **lite mode**; the tools refused the requested high-water-mark queries. Freed allocation stacks therefore could not be recovered from those attempts. A separate trace under that instrumentation is not a matched latency comparison with the uninstrumented activation above.

## What this evidence supports

The earlier transient Markdown memory concern was reproduced in the baseline and narrowed to observed VM categories with simultaneous worker-stack and small-live-heap evidence. The remaining question is allocation provenance and retention/reclamation behaviour under repeated controlled activations. The current captures neither prove a persistent leak nor identify all peak causes, and they contain no matched final-build measurement from which to claim lower latency or memory usage. Final milestone changes, mixed-session behaviour and installed-build verification are recorded separately in the [milestone](../security-quality-milestone.md).
