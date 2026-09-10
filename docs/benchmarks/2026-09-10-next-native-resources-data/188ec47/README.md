# Native resource evidence for source 188ec47

This is the sanitized durable projection of the completed 2026-09-10 native session, PID 14206, source `188ec47199a0dc31f333fff6673d1d6afd7766f1`, executable SHA-256 `f70097158c2b54b84c023ecee4c53e0916ea70791131de3d84602696a0bf5afe`. Read [the analysis and limitations](../../2026-09-10-next-native-resources.md#session-b-measured-188ec47-completed-review-and-window-cleanup).

- `resources.csv` preserves all 268 target/resource samples, in original order. CPU times are cumulative seconds; RSS and descendant RSS are KiB. Interval CPU uses one logical CPU as 100%. Empty cells are unavailable, not zero. Descriptor values can be cached; use only rows with `fd_fresh=True` for fresh-FD statistics. Background load/compiler fields are retained only on fresh samples. The source resource JSONL includes command paths and background identities, so it is not versioned here.
- `events.jsonl` is the exact 18-record operator marker log, audited for private repository paths. Notes retain corrections and imperfect phase naming. `trace_bytes` offsets refer to the exact trace file, not the resource CSV. Marker endpoint CPU is a separate measurement from the observer's five-second CPU intervals.
- `trace.log` is the exact 149-line callback/image-lifecycle trace, including all 25 callback values and 124 lifecycle records. It has no per-line timestamps. An image-retire event is emitted after GPUI's image-drop calls; it is not a GPU byte measurement.
- `summary.json` contains build identity, numeric statistics, complete phase/resource endpoint summaries, trace values/counts, original raw hashes and limitations. Private paths and background process names have been removed.
- `SHA256.json` hashes the four evidence files above. The original full resource JSONL hash is recorded in the summary and report; the projected CSV has its own hash.

Medians use the middle value (or average of the middle pair); p95 is nearest-rank `sorted(values)[ceil(0.95*n)-1]`. Nothing is trimmed as an outlier. The following reproduces the headline RSS/CPU distributions directly from the durable CSV:

```python
import csv, math, statistics
from pathlib import Path
rows = list(csv.DictReader(Path('resources.csv').open()))
for key, scale in [('rss_kib', 1 / 1024), ('cpu_percent_interval', 1)]:
    values = [float(row[key]) * scale for row in rows if row[key] != '']
    ordered = sorted(values)
    print(key, len(values), statistics.median(values),
          ordered[math.ceil(.95 * len(values)) - 1], max(values))
```

The source snapshot was frozen at `/tmp/gitturtle-next-20260910/final-native-analysis-188ec47`. The standalone analyzer used for the full original files is identified by SHA-256 in `summary.json`; its default tools are outside the repository. These records concern the measured 188ec47 release, not later UI corrections or the ultimately installed package.
