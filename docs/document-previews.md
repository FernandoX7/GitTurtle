# Captured document previews

GitTurtle's PDF, Markdown and model views inspect the bytes captured for each
comparison side. Their native controls retain the Before/After distinction,
including an absent side. Quick Open uses a single Source side. Repository reads,
parsing, geometry preparation, decoding and render-pixel conversion run off the
UI thread. The original source/diff remains the authority for staging.

This document records the finite implementation contract. The
[format matrix](file-previews.md) covers other image and metadata formats;
[the milestone record](next-milestone.md) records native acceptance and remaining
work. Unit/decoder tests establish data behavior, not native layout, latency,
GPU cleanup or packaged-build identity.

## PDF pages and text

On macOS, [CoreGraphics rendering](../crates/preview/src/native.rs) reads an
in-memory data provider, and [PDFKit text extraction](../crates/preview/src/native/pdf_text.rs)
uses `PDFDocument.init(data:)`, `page(at:)` and `PDFPage.string`. Native document,
page and string references stay on the worker stack; a scoped autorelease pool
releases PDFKit temporaries. No document URL, PDF action, script or launch-link
API is invoked. See Apple's [data initializer](https://developer.apple.com/documentation/pdfkit/pdfdocument/init(data:)),
[page lookup](https://developer.apple.com/documentation/pdfkit/pdfdocument/page(at:))
and [page text API](https://developer.apple.com/documentation/pdfkit/pdfpage/string).

On Linux (x86-64 and AArch64), [hayro](https://github.com/LaurenzV/hayro)
renders the page and extracts its text inside GitTurtle's own helper process,
never in the app: [`pdf_view::prepare_page`](../crates/app/src/pdf_view.rs)
starts one helper per page through the client in
[`pdf_helper.rs`](../crates/app/src/pdf_helper.rs), writes the captured bytes
to its standard input and reads one bounded reply of pixels, page count and
text. The helper confines itself before it reads a byte (resource limits,
`no_new_privs` and a seccomp allow-list with no file, network or process
calls). The client kills it at a 10-second deadline, and within 25 ms once
the page is no longer wanted. A first page is prepared by the repository
worker's read, which a newer read (another file or commit), a tab switch or
close, or closing the window cancels. Back from Compare cancels only the
comparison's own preview read, the one labelled “Loading preview…”
(`PREVIEW_LABEL` in [`main.rs`](../crates/app/src/main.rs)): a captured
comparison restored with its tab (“Restoring captured comparison…”,
[`repository_tabs.rs`](../crates/app/src/repository_tabs.rs)) keeps reading
after Back until it completes or the helper reaches its 10-second deadline,
and its late result is dropped. A later page
is prepared on the render lane, whose request a page change, Retry page, Back,
another file, a tab switch or leaving the repository page for Settings or
Projects supersedes; leaving the page keeps a first page's read, whose
comparison waits for the return. The parent-death signal ends a helper when
the app exits. At most two helpers run at once. The
[architecture review](development/2026-11-02-linux-pdf-pages-architecture.md)
records the design and its security review. Other Linux targets and other
platforms keep metadata and an explicit unsupported result.

| Behavior | Bound or limitation |
| --- | --- |
| Captured input | At most 32 MiB with a PDF header. Locked, corrupt and unsupported documents retain an explicit unavailable result. |
| Navigation | One selected page is rendered at a time, including pages beyond the initial view. Documents may contain 1–100,000 pages. Previous/Next and direct page-number entry use each side's actual page count. Linked navigation clamps each side independently when counts differ. |
| Page pixels | At most a 1,000-pixel output edge, preserving aspect ratio and page rotation. Source geometry must also pass the shared edge/pixel bounds. Fit and 25–400% display zoom reuse the bounded raster; zoom does not request a higher-resolution decode. |
| Retention | [The native page cache](../crates/app/src/pdf_view.rs) retains up to four pages and 16 MiB per side, whichever limit is reached first. Render pixels, extracted text and text notices share that budget. The enclosing immutable preview cache reserves the full per-side allowance before demand-loaded pages arrive. |
| Text alternative | “Read page text” opens a read-only, selectable native text editor for the selected captured page. Returned UTF-8 is capped at 256 KiB; truncation is explicit and never splits a Unicode character. Text absence is explicit. No OCR is requested, and text-layer reading order can differ from visual layout. Extracted text is never a Git patch. |
| Scheduling | Both call sites prepare a page through `prepare_page`: the first page on the cancellable repository worker, later pages on one serialized render lane with a bounded queue. Generation/page checks reject stale results; cancellation is checked between parsing, drawing, extraction and conversion phases, and on Linux at least every 25 ms while the helper runs, killing it once the request is superseded. |
| Platform | macOS renders with CoreGraphics and reads text with PDFKit; Linux on x86-64 and AArch64 renders and reads text with hayro in the confined helper, which needs a kernel with seccomp filters (without them the page says “The PDF renderer could not start.”). Other platforms retain metadata and an explicit unsupported result. |

macOS framework calls cannot be interrupted mid-call: on macOS, input,
returned text, retained pixels and queue limits are independent and do not
form a hard wall-clock deadline or process-memory sandbox. The Linux helper
is both, within its limits. A page can still be displayed when its text layer
cannot be extracted.

A page that fails says why where the page would be drawn, worded by the app,
never with text from the helper: the document's own errors use the macOS
sentences, and the helper's are “The PDF renderer could not start.”, “The PDF
page took longer than 10 seconds and was stopped.”, “The PDF renderer stopped
on this page.” and “The PDF renderer returned an unreadable result.” A later
page offers Retry page, which asks for that page again; a failed first page
leaves the side without pages and offers none, as on macOS. Nothing retries
by itself, since a hostile page would only burn the CPU again. Read page text
is offered only for a page whose pixels and text result arrived; a page whose
text alone failed shows its pixels, and its text dialog says why. A reply that
puts a requested page outside the document is called unreadable rather than
shown with its own page count, which would contradict the count the view
already shows (every page the view asks for lies within the count of the
first page's validated reply). System preview is an explicit user
action on an isolated copy of captured bytes.

## Native Markdown and local resources

[Markdown preparation and presentation](../crates/app/src/markdown_view.rs) use a
CommonMark/GFM syntax tree and a virtualized native block list. Supported blocks
include headings, prose/emphasis, block quotes, ordered/unordered/task lists,
code fences, tables, rules, links, local images and up to four static Mermaid
diagrams. HTML is shown literally; it never becomes executable markup. Unsupported
constructs and rendering limits stay visible, with the source view available
within the existing text-preview limits.

Rendered input is capped at 256 KiB, 16,384 syntax nodes, 64 nesting levels and
2,048 displayed blocks. Individual text blocks are capped at 16 KiB; tables show
at most twelve columns. Code is divided into bounded blocks. Before/After block
scrolling can be linked, and each side provides Top/End navigation. These are
document-position controls, not semantic alignment of changed paragraphs.

Local images use the [captured asset resolver](../crates/git-core/src/preview_assets.rs).
The Markdown path is the base directory, and the scope is explicit:

- A historical scope must be a full commit object ID. The document and each
  resource resolve inside that commit, even when the working files differ.
- An index scope captures one stage-zero metadata listing and then reads those
  blob IDs. The selected document's expected blob identity is checked when
  supplied; unmerged entries are refused.
- A working scope reads tracked regular files as they exist during capture.
  Descriptor-relative opens refuse symlinks in every component, and changing
  files are rejected. This is not an atomic snapshot of the whole worktree.

The app requests at most 32 images per side, at most 4 MiB per image and 16 MiB
of aggregate captured asset bytes. A separate 16 MiB rendered-pixel budget and
1,000-pixel image edge cap apply after decoding. Source dimensions remain subject
to the decoder's independent bounds. Missing or rejected images show their alt
text/destination and an error; comparisons with image errors skip immutable
caching so unavailable resources can be retried.

Resource paths are literal repository paths, not Git pathspec patterns.
Percent-encoded filename bytes are preserved. Relative `.`/`..` components may
normalize only within the repository. Absolute paths, URL schemes, network paths,
backslashes, NULs, `.git`, repository escapes, queries and resource fragments are
refused. Stored symlinks are never followed. Git reads disable external helpers,
filters, lazy fetching and optional locks. Image references do not trigger
downloads or LFS materialization; an unresolved pointer remains unavailable.

Local document links reuse the same resolver and selected revision/index/working
scope, with a 256 KiB text limit. They open read-only native document dialogs with
Exact source/Rendered controls and at most four nested local documents. A
same-document `#heading` link uses the rendered heading lookup; full browser URL
and fragment semantics are not implemented. HTTP/HTTPS links require the explicit
“Open in browser” action. Other schemes and remote image loads are refused.

Captured local-document dialogs focus the displayed rendered view or exact-source
editor when opened or switched. Exact source retains its editor and Find query
across those switches. Find stays within the captured document; Escape closes
Find before dismissing the dialog. Repository Search cannot change the page or
focus behind an open dialog or sheet.

The source/diff tabs retain the literal captured Markdown. Rendered text,
Mermaid pixels, image bytes and linked-document contents never produce selected
staging edits. Returning to History, changing application pages, replacing the
document or suspending its repository tab cancels pending local-document navigation; publication checks cancellation
before opening a dialog.

## Retained 3D geometry and STEP boundaries

[The native model view](../crates/app/src/model_view.rs) retains immutable triangles
and renders a requested orthographic camera frame. Orbit, Shift-drag pan, wheel
zoom, Fit/Reset, seven standard views and all-edge wireframe have keyboard or
named button equivalents. The labelled X/Y/Z indicator follows the completed
frame. Initial cameras fit the union of both revisions' bounds, preserving
relative size and position; users can unlink the cameras for independent views.
Camera/view settings have a serializable bookmark separate from pointer gestures
and pending work.

The renderer has one active request and one replaceable pending pair. Queued
replacement drops obsolete scene references; generation checks reject stale
frames. Settled frames have a 720-pixel edge and active dragging uses 360 pixels.
Geometry, raster samples and retained frame buffers have separate bounds.
Neither model parsing nor frame preparation resolves material libraries, textures,
external geometry, scripts or repository-relative resources.

With `GITTURTLE_TRACE`, `model_frame_callback_ms` records dispatch through the
next frame callback after a still-current result is published. It includes render
lane waiting, CPU rasterization, pixel conversion and UI delivery, with edge and
triangle count. It excludes pointer/key delivery, source decoding and completed
GPU/OS presentation. Superseded results are omitted, never counted as zero.

STL, OBJ, FBX, GLB 2.0 and 3MF use the documented retained-mesh decoders. GLB
uses captured embedded geometry, supported appearance, skins/morphs and animation,
with nested scene placements and glTF meters converted to Z-up millimeters.
Embedded resources and EXT_meshopt_compression stay independently bounded;
unsupported structural geometry and optional appearance/clip failures follow
the [GLB contract](interactive-3d.md#glb-20-appearance-deformation-and-animation).
Per-side clip selection, Play/Pause, scrubbing and sample stepping preserve the
camera. Warm retained views keep their playback state paused when hidden;
the serialized camera bookmark does not currently persist clip/time selection
across a cold restore. The selected scene and any fallback are disclosed.
A missing or corrupt local LFS model keeps
its exact pointer and explicit download action, while the opposite available
model remains usable; preview activation never downloads an object. STEP accepts
faceted B-rep faces, analytic `CSG_SOLID` sphere/cylinder/torus/block entities and
nested mapped representations with supported placements. Declared supported
length units normalize to millimeters; missing units stay unknown. Curved solids
use fixed bounded tessellation. General trimmed `ADVANCED_FACE` B-rep/NURBS,
boolean trees, face holes, nonuniform maps and product-relationship assemblies
remain unsupported. A filename or faceted example does not establish general
STEP compatibility.

The exact entities, tessellation counts, coordinates, units, transform rules,
expansion limits and maintained-kernel research are in
[the interactive 3D contract](interactive-3d.md). Model frames are visual inspection
aids; they do not edit geometry, create staging patches or validate manufacturing
tolerances.

## Focused verification

`cargo test --locked -p gitturtle -- pdf_view::tests model_view::tests markdown_view::tests`
checks native-view state/cache behavior and captured-resource integration.
`cargo test --locked -p gitturtle-preview native::` checks the macOS raster/text
decoder fixtures; `cargo test --locked -p gitturtle-preview model3d::tests` checks
geometry, coordinates, transformations, raster bounds and cancellation.
`cargo test --locked -p gitturtle-core --test preview_assets` checks captured
commits, index/working scope, literal paths, symlink refusal, budgets and raw reads.
Use the current [native validation guidance](validation.md#current-validation-guidance)
for interaction, focus, appearance, package and release evidence.
