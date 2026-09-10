# Supplied-byte preview security audit — 2026-09-10

This is the bounded decoder workstream of the security/architecture milestone.
The coordinator owns combined checks, native interaction, profiling, packaging,
installation and final build identities. Decoder tests below do not establish
native responsiveness, a memory sandbox, or Linux interaction coverage.

## Trust boundary and accepted scope

Repository blobs and captured working files supply untrusted bytes. Filenames
are format hints, never paths opened by this crate. The application worker calls
the preview APIs and receives owned pixels, text or geometry. Rust image codecs,
usvg/resvg, the supplied-memory ufbx loader and macOS frameworks execute inside
the application process; input bounds are independent from their internal
allocation behavior and individual calls are not preemptible.

| Surface | Examined boundary | Result |
| --- | --- | --- |
| Raster/GIF | Header dimensions, source/output pixels, codec allocation limit, frame aggregation, cancellation | Existing independent limits retained; focused raster and GIF fixtures pass. |
| SVG/Mermaid | XML bounds, self-contained resources, internal reference expansion, generated geometry | Two concrete expansion gaps corrected below. Mermaid continues through the same preflight. |
| 3MF | ZIP directory preflight, selected-member decompression, XML, instance expansion | Directory/member/geometry bounds and no extraction retained; malformed/instance/cancellation fixtures pass. |
| STL/OBJ/FBX/STEP | Supplied bytes, native loader callbacks, geometry/instance/polygon budgets | No new concrete defect established; external-file denial, allocator budgets and finite geometry checks retained. |
| ImageIO/CoreGraphics/PDFKit | Supplied CFData, stack-owned native references, page/output/text bounds | Reviewed create/copy/release order, borrowed pages/strings and scoped autorelease pool; macOS decoder fixtures pass. |
| Generic container metadata | ZIP/font/media declarations and strict UTF-16 | No extraction/execution introduced; existing bounded malformed-input fixtures pass. |

Finite acceptance:

- [x] Reproduce compact SVG `use` expansion beyond the XML node budget.
- [x] Reproduce marker children multiplied by target path segments.
- [x] Refuse over-budget expansion before usvg conversion and system-font loading.
- [x] Preserve ordinary internal references, inherited arrow markers and supported Mermaid diagrams.
- [x] Cover fragment/namespace/CSS boundary cases and run a finite mutation probe.
- [x] Document compatibility tradeoffs and remaining decoder limits.

## Findings and corrections

**P1 — Small SVG reference graphs amplify preparation work.** The original
[SVG decoder](../crates/preview/src/lib.rs) limited XML to 10,000 nodes and depth
128, but then passed the tree to usvg, which clones `use` targets. A 43-element,
less-than-1-KiB SVG with thirteen levels of two references to the preceding group
rendered successfully despite producing more than 10,000 expanded nodes. The
new regression first failed with an `Ok(ImagePreview)` result. The pinned
usvg 0.48.1 parser has its own much larger one-million-node and 1,024-depth checks;
those checks did not enforce GitTurtle's intended work budget. Repeated large
path data could also multiply geometry without exceeding the source byte cap.

The private [SVG expansion preflight](../crates/preview/src/svg_limits.rs) now
counts each expanded visit and repeated attribute/text payload, including
definitions, before calling usvg. It rejects more than 10,000 visits, 2 MiB of
payload or 128 levels, and rejects cycles. ID and fragment handling preserve the
renderer’s first matching ID, unqualified `href` precedence over `xlink:href`,
trailing-space handling and support for namespace-free SVG. Tests verify nested
reference pixels and reject compact fan-out, repeated path payload, cycles and
deep reference chains. This is a structural work bound, not a measured RSS cap.

**P1 — SVG markers multiply geometry independently of `use`.** usvg converts
marker children anew at each marker placement. A less-than-4-KiB fixture with
fifty paths inside a marker and 256 target path segments rendered successfully
before the marker correction, expanding more than 10,000 paths. Its behavioral
regression also failed first with `Ok(ImagePreview)`.

The same preflight now charges marker children at start/end placements and a
conservative upper bound on middle-marker placements. Referenced markers inherit
from their definitions; `use` subtrees inherit from the referring element. Marker
and `use` visits share node, payload and depth limits, so nested combinations
cannot escape the new accounting by switching reference types. The middle-marker
estimate allows four segments per source geometry byte to cover arc conversion
without a second geometry parser; it can refuse some otherwise valid documents.

To keep marker accounting finite without implementing another CSS cascade, SVGs
with marker definitions refuse marker/resource-bearing or escaped CSS, and
namespaced marker/geometry attributes. Ordinary inline colors/strokes and marker
presentation attributes remain supported. Foreign-namespace `style` elements are
also inspected because the pinned renderer recognizes their CSS. CSS attempts
to restore an inherited marker after a `none` presentation attribute are refused.
Direct inherited-arrow pixel tests and existing flowchart, sequence, class,
state, ER and pie tests preserve the normal Mermaid path. Errors leave captured
source available under the existing preview contract.

## Executed verification

- The two reproducer tests failed before their respective correction by receiving
  successful pixel output, then passed with explicit preflight errors.
- `cargo test --locked -p gitturtle-preview`: 59 tests passed after the marker
  correction, including supplied-byte ImageIO/PDFKit fixtures on macOS and all
  existing Mermaid/model/container tests. The later namespace cases passed in
  the focused nine-test `svg_` run. The coordinator's final workspace gates cover
  the integrated final source, including the private-module extraction.
- `cargo test --release --locked -p gitturtle-preview svg_bounded_mutation_probe -- --ignored --nocapture`:
  six in-memory seeds (path, nested reference, cycle, marker, marker CSS, DTD/image),
  deterministic seed `4153545241535647`, 16,384 byte-mutation attempts, 593 accepted
  and 15,791 refused; 10.750 ms reported for the mutation loop. Every success had
  positive dimensions at most 32 × 32 and exactly four bytes per output pixel.
  No panic occurred. The harness stops after 16,384 attempts or five seconds;
  this run reached the attempt limit. It has no coverage instrumentation,
  sanitizer or native-framework fuzzing and is not a latency benchmark or proof
  of parser security.

No dependencies changed. The extraction places the new policy behind one
private preflight call, keeping raster/native decoder ownership unchanged. Native
application source/build/package identity and final combined results belong to
the coordinator's milestone evidence.

## Residual risks and verification limits

These corrections bound the demonstrated `use` and marker amplification paths.
They do not establish a complete bound for CSS selector matching, font shaping,
paint servers, object-bounding-box clip/mask duplication, or all rasterization
costs. CSS and geometry are interpreted by the pinned in-process renderer;
adversarial documents can still be costly within the independent bounds. A
process-wide resource sandbox or hard decoder deadline remains outside this
finite correction. Native ImageIO/PDFKit and ufbx rely on their upstream safety
and allocation behavior; cancellation is cooperative between phases.

The previous initial Markdown transient remains a separate profiling question.
Its small captured document and image bytes do not attribute the transient to
these SVG expansion gaps. This audit makes no leak claim, no memory-reduction
claim, and no native speed claim. The coordinator's allocation/VM observations
must retain their own build identity, tooling limitations and interpretation.
