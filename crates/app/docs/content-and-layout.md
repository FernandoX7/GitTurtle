# Content and layout contracts

Read this when changing or reviewing themes, geometry, content preparation, editor behavior, images or cache accounting. Source ownership is mapped in [app guidance](../AGENTS.md); visual intent is in [design](../../../DESIGN.md).

## Theme and retained layout

Use semantic appearance colors for native controls and custom content together. GPUI Kit 0.6 reads component backgrounds from resolved `ThemeTokens` and foregrounds from `ThemeColor`; after overriding colors, rebuild `theme.tokens` before `Theme::sync_base`. Updating only the app palette leaves native controls stale. Theme changes invalidate concrete editor decorations without rereading immutable content or eagerly rebuilding hidden editors. A theme switch submits no worker job and keeps the retained patch and split editors and the patch editor's Find query and matches (`settings::theme_apply_tests`, which clicks real Settings theme cards). Under `GITTURTLE_TRACE` it prints [`gitturtle.theme_apply_frame_ms`](../../../docs/benchmarks/metrics.md), from `choose_theme` entry to the next frame callback; the [first release baseline](../../../docs/benchmarks/2026-09-18-theme-apply.md) is the reference the theme editor and picker work measures against. The theme editor's live preview uses the same path: `theme_editor::State::preview` holds the open draft, `apply_appearance` applies it in place of the saved selection, and the first edit of a frame applies in its handler while later edits in the same frame coalesce to one application at the next frame callback. Its own metric, [`gitturtle.theme_edit_frame_ms`](../../../docs/benchmarks/metrics.md), is stamped at entry to the token row's change handler before the value is parsed (or at `open_theme_editor` entry), kept in `theme_editor::State` until the root render (`views.rs`) takes it into `GitTurtle::edit_trace_probe`, a zero-size element deferred above the dialog layer, and printed from that element's paint: the last thing drawn in the first frame after the handler, on the frame tick or in the synchronous draw `Window::dispatch_key_event` makes, so the value is the handler plus that whole draw and never a callback wait. It submits no worker job (`theme_editor::tests`); closing the editor drops the draft and re-applies the saved selection.

A palette application costs one whole-window layout, paint and present, so the path is built to ask for exactly one and to make it cheap. `apply_appearance` invalidates through its own `cx.notify()` and calls `appearance::sync_text_sizes`; it must not call `Window::refresh`, which marks the same window dirty but additionally bars GPUI's cached-view reuse for that frame. `appearance::apply_text_sizes` keeps that refresh for callers that own no entity to notify, where it is right: a text-size change moves every measured box. Reuse is what makes the Settings picker affordable. The rest of the frame is kept small by node count: the picker captions and the Your themes swatch strips are one painted element each (`settings::swatch_run`, the quads a row of divs painted), a token row of the editor lays out no spacer where no glyph is shown and no wrapper around its hex field (the kit `Input` takes the authored width and mono face through its own style refinement), a picker caption lays out the name row only on the selected card that shows the badge beside it, and the Your themes rows are a `uniform_list` that keeps the kit focus ring's 3 px of room inside its own bounds (its content mask) and chains a wheel step to the Settings page only once the list has reached its end (`on_scroll_wheel` with `stop_propagation`, as nested scrolling behaves natively). That list is positioned absolutely over a flow box that is exactly the rows (`custom-themes-rows-box`), not laid out with negative margins: an absolute child does not enter taffy's sizing of the card, whereas a flow child whose margins pull its content contribution below its flex basis collapsed the card's content height under max-content sizing (taffy 0.13 scales that negative difference by the item's inner flex basis); `the_card_keeps_the_plain_stacks_geometry_around_the_rows` pins the card, the rows' box and the setting below for zero, one, two, eight and 32 themes. Because that content mask includes the ring's room, a row partly scrolled out of the viewport would paint into it: `render_custom_theme_rows` covers the room with two absolute strips of the card's surface, painted after the rows, while the rows stand off a row boundary, and leaves the room to a focused edge-slot action's ring on one; the boundary is judged where the list will stand after the frame's pending reveal, not where it stood (`settings::rows_off_boundary` resolves `reveal_focused_row`'s Nearest scroll as the list's prepaint will), so the frame that reveals a partly hidden row never draws a strip over its ring (`strips_cover_the_ring_room_while_the_rows_stand_off_a_boundary`). Each of the twenty cards embeds a `settings::ThemePreviewBody` entity with `Entity::cached`, because a miniature draws its own built-in palette and a palette change does not alter a pixel of it; the wrapper pins `text_color` to the previewed palette so the reuse key's inherited text style stays stable, and the caption stays outside the cached subtree because its selected badge follows the active accent and its background follows the card's hover state. The theme editor holds the same body beside its draft and pushes the draft in only when the palette really changed, so a frame that only renames, warns or moves focus reuses it too. A completed preference save asks for a frame only when it changed the status line or the error the page shows; otherwise every switch after the first would pay a second whole-window frame for nothing. `theme_editor::tests::an_edit_and_a_switch_each_draw_the_window_once` records the palette of every draw and asserts one draw per edit and per switch, none of them in the old palette and no draw from the save completion; the switch is a real click on the card, and because GPUI's click path calls `Window::refresh` on that mouse up, the test records the twenty miniatures that release rebuilds and asserts the reuse on the edit frames instead, where every picker miniature is reused and only the draft's own is rebuilt. The Your themes card keeps its rows off every other frame's bill the same way History does: `settings::render_custom_theme_rows` is a `uniform_list` `min(rows, 8) × 30 px` tall, so a Settings or dialog frame lays out at most eight rows however many themes are saved. Its rows are drawn only in view, so their Edit…, Export… and Delete… (the kit's own tab stops) exist only while drawn; each row tracks one app-owned handle that is not a stop itself (`theme_editor::State::row_focus`), a render finds the row holding focus through it and scrolls that row into view once per focus change, and `GitTurtle::tab_theme_rows` (bound to Tab and Shift-Tab in the Settings key context, ahead of the toolkit's Root binding) lets GPUI's own step stand whenever the row it should reach is drawn and otherwise scrolls the target row in, keeps focus where it was for that frame, and has the list's next render focus the action once the row's stops exist (`tab_walks_every_row_action_and_keeps_the_focused_row_in_view`). Because the miniatures are retained, a card is paired with its body by lookup: each body is stored beside the `ThemeChoice` it draws and found with `GitTurtle::theme_preview_body`, never by position, since `ThemeChoice::ALL` is in display order and `choice as usize` is declaration order (`every_picker_card_draws_its_own_miniature` checks, from the rendered tree, that the body laid out inside each card draws that card's palette). The guarded save notification and the missing refresh must not cost a result its frame either: `a_finished_transfer_and_a_confirmed_delete_draw_their_result` answers the native dialog across a window deactivation and reactivation, forces no draw afterwards, and asserts that the finished export, the finished import and the confirmed delete each draw the window with the notice or the updated list in it.

`Palette::readability_issues` (`src/appearance/custom.rs`) is the single readability rule set: the palette tests assert it is empty for every built-in, and the theme editor (`src/theme_editor.rs`) shows the same issues as warnings for custom palettes instead of adding its own checks. Each issue names the foreground token or graph lane, the background token or selected-row hover blend, the measured ratio and the minimum; the rows and thresholds are the [themes specification's rules table](../../../docs/development/themes/spec.md#readability-rules). `Palette::is_light` (canvas brighter than text) selects the graph lane set, so the lanes judged by the rules are the lanes painted.

Headers and rows share one `ColumnLayout` and horizontal offset. Align header/text/reference insets and graph node margins within allocated widths. Preserve chosen columns at narrow widths and keep the commit-message column visible. Keep `content_panels` and `history_panels` as app-owned `Entity<ResizableState>` values passed through `with_state`; stable element IDs alone do not retain sizes when page/mode changes remove a group. Observe the retained entities without constructing hidden repository views.

The repository title shares remaining header width within its scaled 100–200 point bounds. Let it shrink before wrapping action controls at narrow widths with enlarged text; the full path stays available through its tooltip and accessibility label.

Interface and code text sizes are independent saved preferences. Use `appearance::ui_text` for UI labels and `ui_size` for fixed text-row heights; native rem-based controls follow the interface font. Source editors and custom gutters use `code_text` and `code_scale`. Density row heights include interface scaling. Size changes retain editor identities, selection and Find. Preserve list viewports using the ratio of each window’s pixel-snapped old/new row heights, including active and nested attribution/file-history lists and warm/cold repository tabs. Invalidate cached list dimensions after this adjustment so Working Changes does not reveal an offscreen selection over the retained viewport. Editor viewports follow the measured line metrics of the replacement layout. Keep code and UI scaling separate when adding an editor, canvas gutter, or dialog. Revision, Quick Open, rebase, worktree, reflog and activity dialogs bound their scrolling body to the available window height, leaving their footer reachable at larger interface sizes. When measured Working geometry changes during painting, defer its redraw until painting finishes; unchanged bounds must not schedule another frame.

Desktop text-factor changes use the same viewport update in every window and
retained repository tab, without saving new application text-size preferences.
Editor font changes defer scroll clamping until the new text geometry exists;
ordinary wheel and linked-scroll requests still clamp to the current geometry.

Pair file-status SVGs with a short label and semantic color; color alone must not distinguish New, Modified, Deleted, Renamed, Type or Conflict. Working rows use the status for their staged/unstaged area, with conflicts taking precedence and untracked files shown as New. Keep small controls/status icons tintable SVGs; packaged app artwork is a separate asset.

## Commit-message inspector

`commit_message::State` retains a selected immutable OID, shared display pieces,
variable-height `ListState`, and keyboard focus. History/Compare allocate at most
45% of the inspector to this message region; its copy toolbar remains outside
the scroll viewport. File History shares the presentation inside the remaining
height between its bounded header and footer, leaving revision rows usable.
Reading order is title, available body, then author/date/parent/path metadata.
Empty bodies add neither filler nor a blank clipboard suffix.

Preparing a newly displayed OID partitions the already-loaded model once; paints
clone only shared pieces. Each text-shaping input is at most 1 KiB and 16 source
newlines. Pieces prefer source lines, whitespace and grapheme boundaries; an
individual grapheme exceeding 1 KiB continues at a UTF-8 boundary to keep shaping
bounded. No source scalars are dropped. Copy reads the complete loaded title/body
only on explicit activation and is independent of presentation segmentation.
Navigation row labels are bounded summaries; they do not shape or announce a
megabyte title. The inspector never creates an editor or starts a Git read.

The variable list owns measured row geometry and remeasures proportionally when
interface scale changes. Text and list reservations count toward retained-tab
admission, including nested return contexts. Keyboard arrows, Page Up/Down and
Home/End scroll the focused message; labelled native buttons provide message and
hash copy. Native platform quality remains a separate validation requirement.

## Working composer

Size the composer from the list/composer body's laid-out available height after the header, Targets, feedback, and wrapping path/selection controls. Reserve several file rows, share a very short body between files and composer, keep the commit footer outside the scrolling fields, and let additional guidance/errors scroll without clipping the action. Header geometry must not depend on the measured body height. When bounds or density change, bring the selected working row into view using its current identity, not the previous geometry's pixel offset. Layout observers notify only on changed bounds.

## Prepared content

`worker::text_content` prepares `text::PatchPresentation` before delivery. `text::editor` and `diff_view::new` consume its shared rows, gutter width and decoration ranges without rescanning the patch. Partial controls use worker-validated changed-line identities beside the exact displayed hunk; clear stale selections when a snapshot changes. Keep literal patch text unchanged for copy/search; presentation-only numbers belong in the gutter. Preserve byte-indexed UTF-8 decorations and hunk-side numbers for empty sides, CRLF and no-newline markers.

`text_review` rebuilds optional review presentations from the retained original snapshot through the replaceable read worker. Whitespace filtering and context sizes 12/48/192 disable partial staging and explain that whole-file actions include all changes; Reset restores the original patch and captured partial IDs. Never feed review text back into staging or rebuild a filter from already filtered content. Review jobs check cancellation between bounded phases and during row preparation, stop at 4 MiB/100,000 source lines/4,096 hunks, and preserve the original Before/After bytes. Word highlighting uses a shared one-million-cell allowance and at most 256 tokens per line; long or expensive lines retain whole-line highlighting. Decorations are disjoint so Find can mask their backgrounds reliably. Previous/next change advances through prepared row identities without changing text selection.

Gutter scrolling follows the editor's actual text geometry and accumulates wheel input until the next paint; preserve horizontal offset and bounds. Patch wrapping/folding stays disabled; enabling either requires coordinated gutter layout and native checks. Check gutter and code-area scrolling after changes. Construct only the requested Diff, Split, Before or After editor. Split rows and copy spans are worker-prepared; synthetic alignment blanks never enter copied source text. Refresh mutable editors in place to preserve focus, search and viewport.

Laid-out editors publish accepted linked offsets immediately; cold editors retain their initial request until measured geometry exists. `split_diff::LinkedScroll` distinguishes pending acknowledgments from fresh movement, so a new wheel gesture or reversal supersedes an old request. Keep initial changed-row positioning until a measured line height is available. `diff_view::Viewport` observes geometry changes instead of requesting another frame for every editor paint notification. The consuming `scroll_tests` and `split_diff` regressions exercise real editor layout, fractional font changes, cold requests and same-paint pane alignment; native scrolling quality still needs the affected platform session. `GITTURTLE_TRACE_SCROLL` observes existing input/paint work without scheduling frames; its [measurement boundary](../../../.agents/skills/gitturtle-performance/references/measurement.md) is distinct from display frame rate.

## Editor Find

Create patch/split styles through `editor_find::patch_decorations` and keep its handle through theme and mutable refreshes. The pinned renderer combines overlapping properties in hash order; collection creation order cannot guarantee Find precedence. Remove patch backgrounds only within painted Find ranges, retain source foreground/font styles, and identify the active match with an accent underline. Closing Find restores retained patch styles.

Reserve the empty Find collection without constructing an input/panel or replacing the source editor. Keep handles bounded with weak source ownership. Use the measured Find header height for gutter and split alignment.

## Images and cache accounting

Prepare render-image pixels on the worker. Before/After image sides share bounded pan state; active drags continue across app content and clear on release or preview replacement. Overlay/wipe composes existing images in one source coordinate system without copying pixels on drag. Preserve absent/error sides, original dimension ratios when sides are independently downsampled, and the distinction between source dimensions and decoded-preview zoom. Retained navigation copies exclude active drag gestures.

GIF comparisons start paused and retain a bounded set of worker-prepared frames.
Play/Pause and frame stepping share one time position across Before/After; a
shorter side holds its last frame until both loop. Painting selects an existing
BGRA frame and requests the next native frame only for active playback in an
active window. Hidden and paused canvases schedule no animation loop; retained
navigation clones freeze playback. Cache accounting includes every frame,
timeline metadata, first-frame RGBA and captured compressed bytes. The static
`decode_image` API and secondary static image surfaces still show frame one.

The preview cache counts retained source buffers, patch metadata and CPU image allocations. UI-held `Arc`s, editors and GPU textures may outlive eviction; its budget is not a total-memory cap. Changes to `Content` must keep `Content::bytes` accurate.

Register every directly painted `RenderImage` with `image_lifetime`, including
static dialog/model/PDF surfaces and every GIF frame's shared image. The global
registry owns one cleanup reference and retires all atlas frames through GPUI's
`App::drop_image` only when no cache, view, retained inspection or canvas owns
another reference. Every real workspace render schedules one coalesced check
after its frame/element cleanup, including Back and Projects with no new image;
window close also checks after its entities are released. Sweeps never schedule
themselves or keep idle animation demand alive. Dropping CPU pixels alone does
not remove the pinned renderer's atlas tiles.

Dialog children use the generic `rich_preview::render_comparison` helper with owned preview data and a weak owner handle. Rendering must not read or update the parent `GitTurtle` entity while its dialog layer is rendering; that reenters GPUI's active entity borrow. Parent updates belong in explicit action callbacks, and PDF page changes notify the rendering child.

Opt-in `GITTURTLE_TRACE` records new registered preview images, completed
`App::drop_image` retirement counts (including all GIF frames), retained image
counts, and the count after window closure. It emits only lifecycle events and
does not schedule draws or polling. This proves application retirement calls;
it is not a measurement of Metal driver allocation bytes. Pair native repeat
cycles with process/resource observations when evaluating preview cleanup.

## Captured documents

`rich_preview` owns shared comparison presentation and static metadata/page surfaces. Interactive PDF navigation belongs to `pdf_view`: initial preparation uses the repository worker; later pages use a separate serialized render lane and bounded per-side page/text cache. `model_view` retains decoded geometry and uses one active render with one replaceable pending pair. Hidden or retained views cancel pending work and schedule no idle frames. Their generation checks, retained-memory reservations and `image_lifetime` registration must follow the content into tabs and nested inspections.

GLB appearance and deformation data stay immutable and share their texture and
animation storage with evaluated frames. The model worker evaluates the requested
pose, rasterizes, and converts pixels before publishing the current generation.
Each side chooses a clip or Default pose; both use one comparison clock in seconds.
The shorter clip holds its endpoint until the longest loops. Playback requests at
most 30 samples per second at 360 pixels, skips elapsed samples while a frame is
pending, and uses 720 pixels for paused inspection. No frame history is cached.
Clip changes, scrubbing, lifecycle pauses and failures stop both clocks together;
hidden views invalidate their work, including stash preview replacement/closure.
Reduce Motion disables Play and leaves clip selection, scrubbing and stepping
available. A paused comparison never resumes automatically.

Playback preserves the camera. Fit uses the displayed poses' bounds (their union
for linked cameras); Reset restores the initial authored comparison framing.
The timeline and per-side displayed-pose time distinguish requested comparison
time from the last completed frame. Appearance labels distinguish unlit rendering,
base-color inspection and geometry-only output; bounded details explain omitted
appearance resources and unsupported clips. Source bytes remain independent on
each side and never become a rendering-resource resolver.

`markdown_view` prepares native blocks and captured local resources off the UI thread. Resource reads use core's explicit revision/index/working scope and byte-safe resolver. Remote images and arbitrary URL schemes are refused; HTTP/HTTPS navigation requires an explicit browser action. Keep exact source/diff available; rendered documents, diagrams, PDF text and model frames never become staging input. PDF and decoded-encoding views never acquire partial staging actions. Consult the [document preview contract](../../../docs/document-previews.md) and [interactive 3D contract](../../../docs/interactive-3d.md) for finite format support, external-resource restrictions and separate input/output bounds.

Before/After retain independent captured identities and absent sides; Quick Open uses a single Source side. System Quick Look receives an explicit, private, read-only copy of captured bytes with a safe suffix, never a substituted working path. Images also retain captured bytes and optional literal SVG source for system inspection/copy, counted in the cache. The [file support matrix](../../../docs/file-previews.md) owns codec, encoding and external-viewer limits.
