# GitTurtle design system

GitTurtle is a dense, full-height native Rust/GPUI desktop workspace for reading Git history and performing everyday Git work. Every surface prioritizes clear targets, readable hierarchy, effortless comparison, and immediate input feedback. Repository content fills the window; decorative charts, gradients, and oversized empty headers stay out of working views. Projects and Settings use grouped cards for discoverable choices.

This root document holds the living design tokens, workspace hierarchy, navigation rules, and section anchors. Detailed current-state rules live in five focused specs under `docs/design/`, and historical design rationales live as immutable ADRs in [docs/decisions/](docs/decisions/README.md):

- [Tokens & palettes](docs/design/tokens-and-palettes.md) — 20 built-in palettes, Omarchy live theme integration, OKLab/WCAG contrast floors, More contrast, syntax & diff tints
- [Controls & focus](docs/design/controls-and-focus.md) — shared button geometry, dialog footers, focus rings, disabled edges, motion & reduced-motion rules
- [Keyboard & menus](docs/design/keyboard-and-menus.md) — platform shortcuts, command palette, menu/popover placement, once-per-press write guards, accessibility roles
- [Comparison & previews](docs/design/comparison-and-previews.md) — unified/split diff, line staging, shared Find bar, image comparison, PDF/Markdown/Mermaid/3D previews
- [Responsive layout](docs/design/responsive-layout.md) — one responsive order, one left edge, compact chrome, narrow half-tile layouts, working changes & theme editor

Native visual verification and build-specific results belong in [docs/validation.md](docs/validation.md#current-validation-guidance).

## Core tokens

| Token | Value | Usage |
| --- | --- | --- |
| Default interface text | `13 pt` (`13 px` root rem) | Controls, lists, labels, headings scaled relative to root rem |
| Default code text | `12 pt` | Code editors, diffs, patches, monospace object IDs |
| Section label | `11 px` medium | Menu section headings, inspector labels (never larger than menu items) |
| Compact button | `28 px` height (`28 × 28` icon-only) | Toolbar buttons, pickers, dialog footer actions, Find controls (`10 px` / `6 px` horizontal padding, `6 px` icon/label gap) |
| Named Git action | `34 px` height | Primary Git toolbar actions (Fetch, Pull, Push) |
| Commit action | `36 px` height | Working Changes commit button |
| Corner radii | `7 px` / `12 px` | `7 px` shared controls and rows; `12 px` large surfaces, cards, dialogs |
| Focus ring | `2 px` accent, `1 px` outset | Drawn outside control bounds (`3 px` clip clearance, snapped to whole device pixels) |
| Contrast floors | `4.5:1` / `3:1` / `2:1` | `4.5:1` primary/secondary text & syntax; `3:1` glyphs, focus rings, dialog/secondary edges; `1.5:1+` disabled edges; `1.4:1+` popup hairlines |
| Transitions | `120–180 ms` ease-out | Only for user-caused changes (row staging flight `150 ms`; toolkit dialog `250 ms` / dropdown `150 ms`); disabled immediately under Reduce motion |

## Shared control contract

- Use the shared compact app button helpers (`28 px` standard, `34 px` named Git action, `36 px` Commit) with fixed hit targets, borders, and padding across hover, pressed, and focus states.
- One glyph stands for one meaning everywhere (`remote.svg` for remotes, cloud only for Fetch, commit glyph for detached HEAD).
- Each surface fills at most one primary action leading the current workflow (e.g., Working Changes fills Commit while Push takes the bordered secondary style using `bordered_in_place` so switching modes shifts nothing). Dialog Cancel uses a consistent `1 px` bordered secondary style matching the dialog edge (`3:1+`).
- Hover never outweighs selection (`hover` always stands nearer the canvas than `selected` in OKLab lightness, ΔEOK, and luminance). Disabled filled controls keep a subtle `1.5:1+` edge and dimmed label (`3:1` floor, `<= 60%` of secondary text contrast) and suppress activation.
- Full rules: [docs/design/controls-and-focus.md](docs/design/controls-and-focus.md).

## Navigation transition table

| Event | Destination and retained context |
| --- | --- |
| Select a commit | Remain in History; load only metadata and changed file list. Explicit file activation enters Compare. |
| Enter an inspection | Retain prior comparison, exact targets, selection, query, scroll, and focus in the bounded inspection chain. |
| Back / `Primary+[` | Close the top inspection first; else return from Compare/Changes to retained History; at root History open Projects. |
| `Escape` | Dismiss active dialog, menu, Find, or transient surface first; then step back; at root History clear search and restore focus. |
| Explicit History | Exit repository inspections and show retained History without clearing queries, columns, or drafts. |
| Explicit Projects | Show only the project hub (no hidden repository status/previews/focus); retain current repository for one-click return. |
| Reopen retained project | Restore mode, inspection chain, selections, queries, scroll, pane sizes, focus, and drafts. |
| Choose different repository | Initialize History for the canonical worktree, clear old targets, and load only that worktree's drafts and identity. |
| Settings | Retain preceding page and repository context; preserve unfinished fields across visits. |
| Cancel a modal | Cancel replaceable reads, refuse late reopening, restore attached visible focus. Never replay accepted Git writes. |

Keep one return action in the upper-left page navigation slot (`Projects` at root History; Back arrow with destination name in comparisons and Settings). Page shortcuts never fire beneath a modal or on a hidden workspace.

## Semantic palette ownership

[`crates/app/src/appearance.rs`](crates/app/src/appearance.rs) owns all **20 built-in palettes** (Midnight default dark, Braden/`daylight` original light, Graphite, Tokyo Night, Catppuccin Mocha, Nord, Solarized Dark/Light, One Dark/Light, Rosé Pine/Dawn, Dracula/Alucard, Kanagawa Wave/Lotus, Porcelain, Sandstone, Deep Sea, Ember) plus live Linux Omarchy theme sync and custom themes.

- Theme application synchronizes both `ThemeTokens` and `ThemeColor` before `Theme::sync_base` so native controls, custom drawing, and code editors match.
- Syntax roles map semantically (`renamed` keywords, `added` strings, `warning` numbers/constants, `modified` types, `hunk` functions, `accent` tags/properties/links, `muted` comments, `text` variables/operators) and auto-fit to `>= 4.75:1` (`>= 4.5:1` floor; `7:1` under More contrast).
- File status always pairs an icon, short label, and semantic color (New/green, Modified/amber, Deleted/rose, Renamed/violet, Type/amber, Conflict/warning).
- Full palette tables, surface ladder, and More contrast transforms: [docs/design/tokens-and-palettes.md](docs/design/tokens-and-palettes.md).

## Typography and density

- Default interface text is `13 pt` and default code text is `12 pt`, scaled independently via Settings without losing hit-target proportions.
- Control rows keep one consistent height across siblings; explicit app text sizes, icon sizes, and icon/label gaps survive toolkit size categories.
- Every count agrees grammatically with its noun and following verb (`"1 commit"`, `"2 commits"`).

## One responsive order

When horizontal or vertical room shrinks, chrome gives way in one deterministic order — computed in the current frame from actual bounds (never previous-frame scroll feedback) so nothing clips or flashes across frames:

1. Secondary text/wrap lines and optional metadata dimensions give way first (preserved in tooltips and accessible descriptions).
2. Secondary action labels collapse to `28 px` icon-only controls with explicit accessible names and tooltips.
3. Groups fold into compact menus (`Comparison mode`, `Zoom`, `Pan`, overflow actions) keeping stable widths matching their widest choice so changing a selection never reflows rows.
4. Optional footers yield before primary content drops below its minimum usable viewport (`>= 3` text rows, `>= 96 px` scaled image box).

Full responsive rules: [docs/design/responsive-layout.md](docs/design/responsive-layout.md).

## One left edge

Controls, headings, search fields, and content rows inside each panel align to one shared content inset (`PANE_INSET`). Transparent leading controls compensate for their horizontal inset so visible labels align cleanly with neighboring headings and rows.

## Workspace modes and hierarchy

GitTurtle uses a full-height workspace with four primary surfaces:
- **Repository — History:** collapsible left navigation rail (branches, remotes, tags, stashes, worktrees), central virtualized commit graph and columns, and right-side changed-files/commit inspector.
- **Repository — Compare:** full-width text, image, document, or 3D comparison with persistent file inspector on the right and compact navigation rail on the left.
- **Repository — Working Changes:** staged/unstaged file groups, inline/split partial staging, conflict-block resolution, and commit composer anchored cleanly at the inspector's foot.
- **Projects & Settings:** grouped surfaces with zero hidden repository editors or background focus theft.

### Working changes

- File staging moves a single row between Staged and Unstaged with a `150 ms` eased flight inside the list clip (disabled under Reduce motion). Bulk actions (`Stage all`, `Unstage all`, `Stage selected`) and external refreshes re-sort immediately.
- After rows shift beneath a resting pointer, the newly arrived write control ignores hover, tooltips, and clicks until the pointer moves more than `5 px` along either axis (preventing accidental double-click writes on the wrong file or `Stage all`).
- Conflict blocks render **Accept current**, **Accept incoming**, and **Accept both** above an editable result draft (targeting 8 visible code lines, shrinking cleanly to a 3-line minimum inside a vertically scrolling review body on narrow half-tiles).
- Full Working Changes layout rules: [docs/design/responsive-layout.md](docs/design/responsive-layout.md).

### Custom theme editor

- Live custom themes edit semantic palette tokens directly with real-time WCAG readability diagnostics (advisory on saved drafts, never silently mutated unless More contrast is active), full import/export validation, and keyboard-accessible roving color swatches with HSLA channel units.
- Full token and theme-editor rules: [docs/design/tokens-and-palettes.md](docs/design/tokens-and-palettes.md).

## Selection, focus, and feedback

- Every focused button, input, switch, checkbox, tab, and swatch renders the `2 px` accent ring `1 px` outside its bounds, with clipping containers reserving `3 px` (rounded up to whole device pixels) so rings never clip at `1.25×`, `1.5×`, `1.75×`, or `2×`.
- Every list with keyboard selection and the commit message editor is a single Tab stop in visual order. Modal dialogs trap Tab/Shift-Tab cleanly and hide background content from assistive technology.
- Every write action surfaces explicit in-place progress and final outcome (never relying solely on a spinner disappearing); errors wrap cleanly with tooltips when truncated and expose recovery actions.
- Full focus and motion rules: [docs/design/controls-and-focus.md](docs/design/controls-and-focus.md).

## Text and image comparison

- Text comparison supports unified **Diff**, **Split**, **Before**, and **After** modes (defaulting to Diff), preserving selection, scroll, and partial-staging checkboxes across mode switches. Changed-line tints span full visible row width; unchanged and filler rows stay untinted.
- Line-level staging uses `Space` on the caret/selection inside the diff editor (never one Tab stop per line), announcing changes via a polite live region and preserving caret, scroll, and focus across partial writes.
- One shared **Find** bar spans the full comparison width (sharing a single query across Split sides with an accent bottom-rule indicator for the active side), closing cleanly on `Escape` and returning focus to the searched editor.
- Image comparison defaults to side-by-side on a quiet checkerboard with linked pan/zoom, Overlay, and Wipe modes, honest absent-side labels (`"No image: file added"` / `"file deleted"`), and stable toolbar widths.
- Full comparison rules: [docs/design/comparison-and-previews.md](docs/design/comparison-and-previews.md).

### Documents, media, and exact source

- PDF pages, Markdown/Mermaid diagrams, animated GIFs, and 3D/CAD models (GLB, STEP, STL, OBJ, 3MF) always keep exact literal source accessible alongside rendered inspection views, enforce strict byte/dimension/frame/time bounds off the UI thread, and pause animations automatically under Reduce motion or when hidden.
- Full preview rules: [docs/design/comparison-and-previews.md](docs/design/comparison-and-previews.md) and [docs/file-previews.md](docs/file-previews.md).

## Keyboard contract

- All platform shortcuts route through [`crates/app/src/shortcuts.rs`](crates/app/src/shortcuts.rs) (`Cmd` on macOS, `Ctrl` on Linux), exposed in the searchable Command Palette and Keyboard Shortcuts overlay.
- Destructive and write-triggering keyboard activations (`Enter`/`Space` on buttons, dialogs, menus, and diff line staging) fire **once per physical key press**, suppressing held-key repeats on Wayland/macOS/X11 and refusing stale presses after a dialog closes or a diff updates underneath.
- Full shortcut, menu, and accessibility rules: [docs/design/keyboard-and-menus.md](docs/design/keyboard-and-menus.md) and [docs/native-accessibility.md](docs/native-accessibility.md).
