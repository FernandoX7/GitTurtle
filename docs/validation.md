# Validation notes

Use the [current validation guidance](#current-validation-guidance) for the affected workflow and the canonical [design contract](../DESIGN.md) for expected behavior. Dated records below describe their identified builds; the completed [security, architecture and resource milestone](security-quality-milestone.md), [native polish](native-polish-milestone.md), [review and recovery verification](review-native-verification.md) and [macOS milestone](macos-native-verification.md) remain historical evidence. New development tasks and their acceptance requirements belong in the [development workflow](development/README.md).

This page contains current validation guidance and dated local evidence, with each completed stage tied to its exercised source/build. The September 7–8 records below cover earlier history, design and everyday Git workflows; the September 9 backend report covers its recorded review-milestone inputs. Native workflow evidence is primarily macOS-specific; the September 14 entries add Pop!_OS startup and clean Ubuntu/virtual-native checks. Platform execution and access limits belong to the applicable dated record and [platform runbook](linux.md); the earlier [environment report](benchmarks/2026-09-09-milestone-environment.md) describes its own session. The configured [quality workflow](../.github/workflows/quality.yml) alone is not evidence of hosted CI execution. Public binary release prerequisites belong in the [launch checklist](public-launch.md); the Linux runbook includes a local teammate bundle.

## September 19 Theme export and import

Native QA for `themes-import-export`, which adds **Export…** to each custom
theme row and **Import…** to the Settings › Your themes card, writing and
reading a `gitturtle-theme` JSON document through the platform's own save and
open dialogs. Linux/XWayland (GNOME 46.0 on Wayland, `DISPLAY=:1`,
`WAYLAND_DISPLAY` unset, `GPUI_X11_SCALE_FACTOR` 1), windows 1000x680, the app's
`window_min_size`, and 1440x900, with an absolute throwaway `XDG_CONFIG_HOME`
per launch seeded as a version-6 store. Fixture: `scripts/create-demo-repo.py`
at HEAD `52f471a1137c617fd8e36db2e6251a18f58c23eb`, unmodified and clean
afterwards; no network action was taken, and the only files written outside the
throwaway stores were the theme documents the test chose itself.

The captures were taken from build `301d82af8d1cd06fa8a1d7ded1892e89ffd1a4e0`
(GitTurtle 0.1.0, `source_tree` clean, release, `x86_64-unknown-linux-gnu`,
rustc 1.98.0 (88d9e12ae 2026-08-18), binary sha256
`84d80a41545fe6e40318cb5cedb773f187eaa28c60756523ff4e2d858f39acd2`). As with the
editor entry above, evidence committed to a repository cannot describe the commit
that contains it, so this entry names the revision under test; the build that
ships export and import reuses these captures only when it renders them
identically, which the evidence driver re-checks pixel for pixel. Two captures,
`transfer-1000x680-07-exported.png` and `transfer-1440x900-07-exported.png`,
quote the absolute path they wrote, so that re-check has to pass the same output
directories it used here, and in fact they did not: the shipping revision clamps that
path, so the notice became one line instead of two and the card 134 px instead of
153. **Those two frames were therefore re-taken from the shipping revision
`ea70c6a169882433c3b9659db5b391187c1488a1` and are the only two here not from the
revision named above**; the other 24 artifacts are byte-identical between the two
builds, which is what the re-check established.

Five launches, 52 checks passed and none failed. Export, an Escape-cancelled
export, a delete, an import of that document, two name collisions, an
Escape-cancelled import, nine refusals and the unknown-base notice were each
exercised at 1000x680 and the central ones again at 1440x900. Verified from the
preference store's own bytes rather than from the screen: the exported document
is 690 bytes, sha256
`c4c8271ed6eca156074fea6eb8b65f7b22ef6a078b247a3b6e4fe0fbd32b9ad8`, carrying
exactly the keys `format`, `version`, `name`, `base`, `tokens` and exactly the 21
snake_case token names in spec order, byte-identical from both window sizes and
equal to the stored theme; every refusal and every cancelled dialog left the
store byte-identical; the selection stayed `"midnight"` through all imports, with
the page colour unchanged, which is what "added, not applied" means; the
collisions saved `Harbor Dusk (imported)` then `Harbor Dusk (imported) (2)`; and
a document naming a base this build does not know was stored against the
`daylight` fallback with the notice the spec requires. Latency was dominated by
the portal, not the app: 1.25 s from the keystroke to a visible save dialog,
0.599 s from accepting it to the written-path report, and 1.228–1.244 s from
accepting the open dialog to the message across ten imports and refusals,
including a 70 KiB file refused on size.

**On the dialogs themselves this record is deliberately not a screenshot.**
`prompt_for_new_path` and `prompt_for_paths` reach a real
`xdg-desktop-portal-gnome` dialog here, which is a Wayland window of the
compositor: `org.gnome.Shell.Screenshot.ScreenshotArea` and
`Introspect.GetWindows` both refuse, and XTest cannot drive it. The dialogs are
therefore recorded as D-Bus transcripts and AT-SPI reads — `SaveFile` with
`current_name` `harbor-dusk.gitturtle-theme.json`, `OpenFile` with
`directory false`, `multiple false` and the app's own `Import theme` accept
label, `Response(0, uris)` on acceptance and `Response(2)` on Escape — which
establishes the spec's properties on the wire rather than by reading a picture,
and leaves the dialog's *appearance* unrecorded. That appearance belongs to the
portal backend rather than to GitTurtle. The app's own surfaces, where the
messages live, are captured normally. Because the portal works on this host, the
spec's guidance branch was exercised separately on a private session bus with no
FileChooser service, where both actions reported the picker guidance and changed
nothing.

Two limits of this record, and two defects found on the revision under test.
GPUI does not register with AT-SPI on this desktop, so the accessible names of
the new controls rest on the `#[gpui::test]` assertions rather than on anything
observed; and on Linux the portal navigates into a folder instead of returning
it, so the folder refusal cannot be reached. The defects: nothing bounds the
document text interpolated into the card's message, so a document that is valid
except for a ~64 KiB unknown `base` imports successfully and leaves about 64,700
characters in the notice until the next card action — the app stays responsive
(first repaint 0.582–0.600 s, 0.12–0.13 s of CPU, one Tab repainting in
0.030–0.065 s) but the card's lower border and the settings below it are pushed
off screen, the page growing from 30 wheel steps to 145; and the row's
**Export…** tooltip never appears, at either window size and after 5.2 s, while
the header's **Import…** tooltip appears in the same launch. Both are required to
be fixed in the revision that ships, together with two `docs/user-guide.md`
inaccuracies found here — the suggested file name is a slug of the theme name
rather than the name itself, and the promised folder refusal cannot occur on
Linux. None of those fixes changes a resting frame, which is why these captures
can still describe the shipping build; the pixel re-check against this set is
what establishes that, and it is a precondition of the attestation. The
measurements behind the defects are retained outside the repository with the rest
of the bundle.

Confirmed on September 20: the revision that shipped (`5637cec`) fixed both
defects. Every interpolated fragment is clamped at the source (64 characters for
a key, value or name, 48 for a quoted base) and the message to two lines that end
in an ellipsis when cut, and the row's **Export…** tooltip appears at 2.2 s and
5.2 s at both window sizes. The pixel re-check of that build reproduced all 26
committed artifacts byte-identically with 64 PASS / 0 FAIL, and the long-name
cases (a 64-character collision, an unknown base, and both in one import) read to
their end at both sizes. The two `docs/user-guide.md` corrections landed in the
same revision.

## September 19 Custom theme editor

Native QA for `themes-editor`, which adds Settings › Your themes (New theme…,
Edit…, Delete…) and the New theme / Edit theme dialog with a live preview.
Linux/XWayland (GNOME on Wayland, `DISPLAY=:1`, `WAYLAND_DISPLAY` unset,
`GPUI_X11_SCALE_FACTOR` 1), windows 1000x680, which is the app's
`window_min_size` and gives the stacked 640 px dialog, and 1440x900, which gives
the 1,000 px two-column dialog, with an absolute throwaway `XDG_CONFIG_HOME` per
launch seeded as a version-6 store. Fixture: `scripts/create-demo-repo.py` at
HEAD `52f471a1137c617fd8e36db2e6251a18f58c23eb`; the editor writes only app
preferences, nothing was written to the fixture and no network action was taken.

The captures were taken from build `3927b57bbb913e35ee4a8b48f12b5c7eaa19f686`
(GitTurtle 0.1.0, `source_tree` clean, release, `x86_64-unknown-linux-gnu`,
rustc 1.98.0 (88d9e12ae 2026-08-18), binary sha256
`1faff02061c9f07500cbf74827ae61d425d145caef6a153682230ef239bbe593`). Evidence
committed to a repository can never describe the commit that contains it, so
this entry names the revision under test; a later build that ships the editor
reuses it only when it renders the same captures, which the evidence driver
re-captures for a pixel comparison. Earlier builds of the same patch were
exercised first: in `a6cc84d` Return anywhere in the dialog saved, Keep colors
lost keyboard focus and focus moved to rows out of view; `bb9f332` fixed those
but squeezed the token rows to about 20 px, left the focused Readability list
out of view at 1000x680 and deleted the theme on Return in the Delete
confirmation's Cancel; `30e41b8` fixed those but opened the Delete confirmation
with keyboard focus on the title bar's Menu button behind it, scrolled the
focused Readability list into view at 1000x680 only on the next input event,
and returned focus to New theme… after a delete in only four of eight runs;
`cd563f6` fixed those and passed this flow, but its design review found three
defects: the token column scrolled with no scrollbar and hid the Diff group at
1440x900 and nine of twenty-one tokens at 1000x680 at rest, the 70 px hex field
scrolled the leading `#` out of view once seven characters were typed and kept
it hidden after blur (the driver had masked this by pressing Home before every
read), and an invalid hex value was signalled by the removed-colour outline
alone.

The dialog was operated from the keyboard alone, in a light base (Braden,
stored `daylight`, starting from Porcelain) and a dark base (Midnight, starting
from Graphite) at both sizes; the pointer only wheel-scrolled Settings to the
Your themes card and, after Delete, to the base's picker card. Token rows are
30 px apart with the group labels intact, Name and Base are 28 px tall, Base is
200 px with a menu wider than it and sized to the window, the picker shows a
1 px border-color edge at rest and a 2 px accent ring focused, and the Your
themes row aligns with the card title. The first valid edit recolored the
dialog and the page behind it to exactly the typed canvas in the frame of its
keystroke (no intermediate frame in 51–140 grabs). Two warnings appeared in
each base ("Muted text on Selected 3.9:1, needs 4.5:1"), invalid hex and an
invalid Name were outlined in the removed color while focused and disabled
Save, Save stored the theme in `custom_themes` of a version-6 store and
selected it, Edit… reopened it, Return in a hex field rewrote the value as
lowercase `#rrggbb`, kept the dialog open and saved nothing, Return on Cancel
and Escape restored a frame pixel-identical to the one before the dialog
opened, the Delete confirmation opened with focus on Cancel with Tab contained
and Escape closing it, the focused Readability list was in view at 1000x680 in
the first frame that showed its focus, and Space opened New theme after every
delete: 12 of 12 across 12 launches.

The three defects of `cd563f6` do not reproduce. The token column shows a 6 px
scrollbar thumb in the border color at rest inside a 16 px track at its right
edge in the wide and the stacked layout and in both bases (the track is painted
in the canvas color, like every scrollbar track in the app, so the thumb is what
shows); the track holds nothing but its background and the thumb, and the
focused picker's ring ends before it, 16 px left of where it ended on `cd563f6`.
Tab to the last row scrolls the column to its end with the thumb at the bottom
of the track and the Diff group label and rows painted at both sizes. The hex
field is 78 px: a typed `#rrggbb` shows all seven characters with the caret
after the last one, the value is complete after focus leaves, the typed row's
field is pixel-identical to the same value reopened from the store, and
Return's rewrite shows the full value, all read with no Home press. An invalid
value shows a 14 px rounded square on the removed fill with a contrasting × in
the row's warning slot in both bases at both sizes, replacing the readability
glyph while the value is invalid and staying after blur; a valid value removes
it and restores the readability glyph. The 39 screenshots are under
[`docs/evidence/themes/editor/`](evidence/themes/editor/) with
`flow-verification.txt`, which gives the key sequence and 221 checks, all
passing. A second run of the same binary passed the same 221 checks and
reproduced 133 of 134 frames pixel for pixel, including all 39 committed here;
the one bundle-only frame that differs (the Settings picker after the delete
at 1440x900) does so in one anti-aliased glyph-edge pixel by one level of one
channel.

Not covered: the 32-theme bound, name messages other than a built-in name, Reset
to base, **Replace colors**, a failed or refused save, closing the window with
the editor open, restart persistence, follow-system mode, other text sizes or
density, 2x scale, the picker popover itself, pointer scrolling of the token
column and dragging the thumb, the `theme_apply_frame_ms` budget (the
performance review's), and import and export (the next task); Linux/XWayland
only, with no macOS, native Wayland, packaging or accessibility-label coverage
(the app does not register with AT-SPI on this desktop, so the invalid row's
"value is not #rrggbb" label and the scrollbar's name rest on the `gpui::test`
assertions alone).

## September 18 Alucard and Kanagawa built-in themes

Native QA for `themes-batch-alucard-kanagawa`, which adds the Alucard, Kanagawa
Wave and Kanagawa Lotus built-ins. Linux/XWayland (GNOME on Wayland, `DISPLAY=:1`,
`GPUI_X11_SCALE_FACTOR` 1 and 2), window 1000x680, which is the app's
`window_min_size`, with an absolute throwaway `XDG_CONFIG_HOME` per launch.
Fixture: `scripts/create-demo-repo.py` at HEAD
`52f471a1137c617fd8e36db2e6251a18f58c23eb`, plus a disposable copy with a staged
rename, an edit, an untracked file and a deletion for the Compare and Changes
captures; nothing was committed to either and no network action was taken.

The captures were taken from build `e00e864ff8949e2e61d343feecca0561e9784173`
(GitTurtle 0.1.0, `source_tree` clean, release, `x86_64-unknown-linux-gnu`,
rustc 1.98.0 (88d9e12ae 2026-08-18), binary sha256
`b97b7393e31e247d26faa1c88088d222cc9539ef59914f56db2132c04ab06cf2`). Evidence
committed to a repository can never describe the commit that contains it, so
this entry names the revision under test; a later build that ships these themes
reuses it only when it renders the same captures.

Each theme was recorded in the Settings picker with its own card scrolled into
view and checkmarked, and in History with a selected row, a hovered row, a
visible accent focus ring, and a diff showing added and removed lines. The
fifteen screenshots are under
[`docs/evidence/themes/alucard-kanagawa/`](evidence/themes/alucard-kanagawa/)
with the per-capture palette check beside them. History for all twenty themes
at 1x and 2x matches every declared token (120 readings), and the seventeen
existing themes are unchanged against their earlier captures.

No reading fell below its rule floor on full-coverage glyph cores. The narrowest
margins are muted text on a hovered selected row at 4.51:1 (Alucard) and 4.52:1
(Kanagawa Lotus) against 4.5, Kanagawa Wave's selected row against panel at
1.153:1 against 1.15, and Kanagawa Lotus's added and removed text on their diff
tiles at 4.51:1. Rendered antialiased text sits below those floors, as recorded
in [the contrast-margins note](development/themes/contrast-margins.md). Not
covered: warning and conflict states, the canvas-label-on-fill rule, the primary
button's hover and pressed states, the picker at 2x, Split, Blame and image
diffs, and follow-system mode; `accent_foreground` is exact on screen at 2x
only; Linux/XWayland only, with no macOS, native Wayland, packaging or
accessibility-label coverage.

## September 18 Rosé Pine and Dracula built-in themes

Native QA for `themes-batch-rose-pine-dracula`, which adds the Rosé Pine, Rosé
Pine Dawn and Dracula built-ins. Linux/XWayland (GNOME on Wayland, `DISPLAY=:1`,
`GPUI_X11_SCALE_FACTOR` pinned per launch), window 1000x680, which is the app's
`window_min_size`, with a throwaway `XDG_CONFIG_HOME` per launch. Fixture:
`scripts/create-demo-repo.py` at HEAD `52f471a1137c617fd8e36db2e6251a18f58c23eb`,
plus a disposable copy with changed paths for the Compare and Changes captures;
nothing was committed to either and no network action was taken.

The captures were taken from build `83eaf81ae71bc077abe29c26d10e8f7afa97522b`
(GitTurtle 0.1.0, `source_tree` clean, release, `x86_64-unknown-linux-gnu`,
rustc 1.98.0 (88d9e12ae 2026-08-18), binary sha256
`531c6f63acd37dcc897394767a2338209a05af4e0e726de43e171a7d8031ff2a`). Evidence
committed to a repository can never describe the commit that contains it, so
this entry names the revision under test; a later build that ships these themes
reuses it only when its palettes resolve identically to that revision's.

Each theme was recorded in the Settings picker with its own card checkmarked, and
in History with a selected row, a hovered row, a visible accent focus ring, and a
diff showing added and removed lines. The fifteen screenshots are under
[`docs/evidence/themes/rose-pine-dracula/`](evidence/themes/rose-pine-dracula/)
with the per-capture palette check beside them; every sampled surface equals the
token the build declares, resolved from `crates/app/src/appearance.rs` and
`crates/app/src/appearance/sources.rs`.

No reading fell below its rule floor, measured on full-coverage glyph cores, but
these are the narrowest margins of any batch: Rosé Pine's hover against panel is
1.087:1 against a 1.08 floor, and muted text on a hovered selected row is 4.51–4.54:1
against 4.5 in all three themes. Rendered antialiased text sits below those
floors, as recorded in [the contrast-margins note](development/themes/contrast-margins.md).
Not covered: `warning` shares `modified`'s value in all three themes and no
warning or conflict state was reached; `accent_hover` and `accent_active` were
not exercised; `accent_foreground` is exact on screen at 2x only; the picker's
grouping belongs to the picker task; Linux/XWayland only, with no macOS,
packaging or accessibility-label coverage.

## September 18 Solarized and One built-in themes

Native QA for `themes-batch-solarized-one`, which adds the Solarized Dark,
Solarized Light, One Dark and One Light built-ins. Linux/XWayland (GNOME on
Wayland, `DISPLAY=:1`, `GPUI_X11_SCALE_FACTOR=1`), window 1000x680, which is the
app's `window_min_size`. Fixture: `scripts/create-demo-repo.py` at HEAD
`52f471a1137c617fd8e36db2e6251a18f58c23eb`, plus a disposable copy with one
renamed, modified and added path for the Compare and Changes captures; nothing
was committed to either and no network action was taken.

The captures were taken from build `d4ac46bd031d93ab83fa0e549988f1669dc9b4eb`
(GitTurtle 0.1.0, `source_tree` clean, release, `x86_64-unknown-linux-gnu`,
rustc 1.98.0 (88d9e12ae 2026-08-18), binary sha256
`4de9f6fa54463ef58f8dfcbe0b754b050d260b750e8aacc5526a170b414dd332`). Evidence
committed to a repository can never describe the commit that contains it, so
that revision is the one under test and this entry names it; the shipped
palettes are byte-identical to the ones photographed.

Each theme was recorded in the Settings picker with its own card checkmarked, and
in History with a selected row, a hovered row, a visible accent focus ring, and a
diff showing added and removed lines. The twenty screenshots are under
[`docs/evidence/themes/solarized-one/`](evidence/themes/solarized-one/) with the
per-capture palette check beside them; every sampled surface equals the token the
build declares, resolved from `crates/app/src/appearance.rs` and
`crates/app/src/appearance/sources.rs`.

No reading fell below its rule floor. The least headroom is the Solarized Dark
focus ring at 3.03:1 against the field fill it is painted on, against a 3:1
minimum. Not covered: `warning` shares `modified`'s value in all four themes and
no warning or conflict state was reached, so it is unverified as a distinct
token; `accent_foreground` is confirmed on screen at 2x and passes its declared
pair at 1x, where a small button label never reaches full glyph coverage; the
picker's grouping and breakpoints belong to the picker task; Linux/XWayland only,
with no macOS, packaging or accessibility-label coverage.

## September 16 project list pane

PR #22 adds an optional project list pane, a saved project library with nested
user-named groups, and the **Show the project list** setting. Its initial
revision (`f075c04` on main `7cd3744`) passed the Rust gates and an Xvfb
session on the contributor's Linux host. The review revision reworked the pane
into a keyboard tree that shares the History navigator's rows and bindings,
sorted presentation, hover/right-click/Shift-F10 actions with a shared menu,
disabled impossible move destinations, an in-pane error strip with Retry, a
pending-save guard against stale replies, cached presentation rows, and a View
menu / palette toggle. `cargo fmt --all -- --check`, `cargo clippy --locked
--workspace --all-targets -- -D warnings` and `cargo test --locked --workspace`
passed on this Linux host for the working tree over `f075c04` (debug profile;
app 456 passed with two existing ignores, preview 121 passed with one ignore,
core 32 of 33). The one core failure, the configured-askpass fixture, fails
identically on untouched main in this host's Git 2.43 environment and passed in
the PR's hosted Ubuntu and macOS runs, so it is environmental and unrelated.

A debug build of that working tree ran under XWayland on a GNOME Wayland
desktop (Pop!_OS, Linux 7.1.5, window 1480x980 logical at about 2.17 scale)
against three disposable `scripts/create-demo-repo.py` fixtures plus a second
copy named `alpha`, with `XDG_CONFIG_HOME` in a scratch directory. Pointer and
key input came from a local XTest helper. Verified by screenshot and by reading
the store: pane rows match the navigator's 30 px geometry beside it; groups
come first and each level is sorted by name; the two `alpha` projects show their
parent folders; the open project keeps the selected surface and accent text;
hovering a row reveals its actions control; the group menu omits the group's
own subgroups and checks its current place, and the project menu checks its
group; a right-click opens the same menu; Down moves the cursor marker,
Shift-F10 opens the menu under the cursor row, Down selects an item, Return
runs **Open project** and marks `beta`, and focus returns to the tree; **+**
opens the group dialog with the Rename project layout; Settings shows the pane
and the switch; the palette lists **Show or hide the project list**; replacing
the store with a directory made a collapse fail with the explanation and Retry
inside the pane while the collapse stayed on screen, and Retry after restoring
the file saved `collapsed: true`.

Limitations: debug profile only, so no timing claim. The compositor refused a
programmatic resize, so the 1000x680 clamp has automated coverage only. macOS,
native Wayland, a physical pointer session, screen readers, package builds, and
theme, density and text-size variations of the pane were not exercised. Group
and project removal, the depth and count limits, the stale-reply guard and the
refusal of a malformed saved list have automated coverage only.

## September 16 PR #17 file-discard review

Clean application source `e9b631f` includes main `5466b8a` and passed 847
workspace tests (five existing ignores), strict Clippy, formatting and native
debug compilation. The [discard validation record](discard-validation.md)
documents four preservation fixes, independent general/security review,
bounded destructive confirmation and actual Linux/X11 pointer/keyboard checks.
Native evidence covers stale content, rename/add/delete, untracked deletion,
conflict and directory-transition refusal, long paths, enlarged light UI,
dark UI, cancellation and unrelated index/worktree/ref preservation. macOS
native, physical desktop and hosted CI evidence remain separately scoped.

## September 16 PR #13 project-name review

The [project-name validation record](project-names-validation.md) covers main
integration, independent review fixes, 816 passing workspace tests (five existing
ignores), strict Clippy and real native Linux/X11 checks of clean source
`2c98108`. Screenshots cover dark/default and minimum-size light/enlarged text,
inline save failure/retry, focused editing, canceled edits, canonical aliases,
long names and restoring the folder label. macOS native interaction, screen
readers and installed-package checks remain outside this evidence.

## September 16 PR #15 force-removal review

Clean source `c3e4a0f` includes main `7c9dc6c` and passed 803 workspace tests
(five existing ignores), strict workspace Clippy, formatting and native debug
compilation. The [force-removal validation record](force-worktree-removal-validation.md)
documents preservation regressions, independent review, accessible destructive
confirmation, and real GPUI interaction on virtual Linux X11. Native checks
covered minimum-size enlarged light UI, dark UI, cancellation, nested-repository
refusal, stale ignored-content refusal and successful removal with independent
branch/index/sibling preservation. macOS, physical desktop and hosted-CI results
remain separate from this local evidence.

## Commit-inspector validation requirements

The persistent inspector requires candidate-bound native evidence in addition to
its consuming Rust layout/state tests. Exercise short/empty/Unicode/trailer
messages, long titles, many paragraphs, near-limit unbroken text and merge
parents. Check both densities and light/dark themes at minimum window size,
280-point inspector width and enlarged interface text. Confirm full-message/hash
copy, keyboard scrolling, selected changed-file visibility, rapid commit
selection, Compare/Back, nested File History, tab return and Projects/Settings.
Native records must identify the exact binary and platform; source tests do not
establish Linux/macOS runtime quality. The initiative's user-deferred macOS
verification remains open until that evidence is supplied.

## September 15 commit-inspector and Linux package checks

Clean release `eebf47a` exercised the persistent message inspector and an actual
Linux archive/install/relaunch in virtual Ubuntu 24.04 X11. The
[dated evidence](benchmarks/2026-09-15-commit-inspector.md) records layout, Unicode,
large-message copying, native accessibility, merge parents, retained navigation,
package digests and installer recovery. C0 license clearance, hosted artifact
transfer and user-owned macOS/Apple verification remain open. Later source and
fixture commits do not turn this into evidence for another executable.

## September 15 PR #5 macOS merge review

Source `0d4d8ce` passed 771 workspace tests, strict Clippy, formatting, app check
and release compilation. The [worktree-removal review](worktree-removal-validation.md#september-15-macos-merge-review-and-security-fixes)
records CodeQL flow fixes, explicit fixture copies, verified local macOS package
identity, native cancellation/refusal/removal checks, and independent branch,
index and sibling-content preservation. Hosted results are recorded on PR #5;
this entry does not claim a new Linux native or notarized-distribution pass.

## September 15 PR #5 maintainer review

Clean release source `3b0a958` passed 775 workspace tests (five existing ignores),
formatting, app check, strict workspace Clippy and release compilation. The
[maintainer validation record](worktree-removal-validation.md#september-15-maintainer-review-of-pr-5)
documents stale-review cancellation, checkout/admin identity guards, the Ubuntu
draft-shutdown fixture correction, and native Wayland checks for protected
targets, cancellation, fresh review, cleanup and branch retention. macOS native
interaction and hosted checks are scoped separately from this local evidence.

## September 14 guarded worktree removal

Implementation commits `95d97a7` and `9585271` add navigator removal actions,
keyboard access, hidden-change/lock guards and verified cleanup. The
[worktree removal validation record](worktree-removal-validation.md) documents
743 passing workspace tests (five intentionally ignored), strict Clippy, and
native Linux/X11 checks of protected targets, cancellation, stale review,
successful Git/filesystem cleanup and branch retention. macOS runtime and
hosted CI are separate from this local evidence.

## September 14 PR #4 desktop text and security validation

Clean application source `0700984951001289a6c7490f81e72b749cd4120e` was built,
packaged and installed as Linux x86-64 release 0.1.0, using Rust 1.98.0 and the
existing Ubuntu 24.04 build environment. The executable SHA-256 is
`ba54f643d8262c409715933c165bda0c91e052d70cd0a0d8f42eb3dc1e4b3a2c`.
Subsequent validation-record edits do not change that executable's source identity.

Formatting, `cargo check --locked -p gitturtle`,
`cargo test --locked --workspace` (**756 passed, zero failed, five explicit
ignores**), strict workspace Clippy with all targets, and release compilation
passed. Focused checks exercised observer cancellation and bounds, the repeated
asynchronous history fixture, actual rendered editor/list metrics, two-window
notifications, retained tabs, nested lists and passive-refresh anchors. The
[security review](pr4-security-review.md) accounts for every initial CodeQL alert,
the four reproduced tooling/fixture defects and the new defensive-check finding.
Fresh hosted checks and merged-main alert state are tracked separately in
[PR #4](https://github.com/FernandoX7/GitTurtle/pull/4).

Native interaction used the same release executable, disposable two-commit
repositories with a 600-line source file, multiple changed ranges, an inserted
alignment row and long lines. A native Wayland window ran under nested Weston
with software graphics and the host's real GNOME settings portal. Its desktop
lacks the newer `font-rendering` key, exercising that fallback. Screenshots and
scroll traces verified:

- Live 100%, 125% and 150% text sizes retained the selected source and logical
  viewport. Both split editors kept row 546 with offsets 9828, 12558 and 14742
  pixels for measured line heights 18, 23 and 27 pixels. Unified view retained
  patch row 150 through the fractional change.
- Find query, match, source selection and keyboard focus survived live changes;
  a retained repository tab reopened at the same logical row after a hidden
  size change. Literal source copying excluded gutters, and typing into the
  read-only source left the fixture unchanged.
- Code/gutter wheel input, reversal, horizontal scrolling and Back to the
  selected history context worked. Matching pane offsets and visible row ranges
  were checked in the rendered frames.
- Changing grayscale to `rgba` repainted the glyphs. `none` retained grayscale,
  matching the documented toolkit limitation. Original desktop preferences were
  restored after testing.

A separate native X11 window under Xvfb kept 18-pixel source lines at a portal
text factor of 150%, confirming no extra portal multiplier on that backend.
This is virtual X11 and nested Wayland evidence, not a physical-display,
Ubuntu-desktop, KDE, mixed-DPI or native macOS acceptance run. It establishes
neither a frame-rate improvement nor physical-panel sharpness. The cold split
regression verifies a retained initial-row request resolves after a measured
editor notification; it does not establish positioning on the first paint alone.

All **11 Linux installer tests** passed with the final real bundle, including
relocation and rollback. The installed executable matched the validated hash;
all three genuine configuration files remained byte-identical across installation,
and all four repository tabs retained their order and selection on restart.
The previous executable and all 1,228 recovery entries passed checksum/ownership
validation. Recovery uses the installed script's `--rollback` option described in
[the Linux runbook](linux.md). Existing public-binary notice gaps remain documented;
this was a local installation. Private captures and machine/repository details
are not part of the public evidence.

## September 14 refresh, history and split-diff preview

Source `417b5e8` was installed for this earlier Linux x86-64 release 0.1.0 Preview session, binary
SHA-256 `45606c5a195d1696096b93ea0fa3a38b67b025990ea794f25f91d4211e5729c2`.
The [native investigation and final acceptance](benchmarks/2026-09-14-native-preview.md)
record physical GNOME/Wayland scale-2 code/gutter scrolling, long lines,
Find/selection/copy, Show latest, build diagnostics and recoverable installation.
Final code and gutter runs had zero mismatched pane offsets; the evidence does
not establish a general frame-rate improvement. Supplemental native X11 checks
cover disposable stage/commit/View commit/fetch/pull/push and a linked-worktree
picker open. All genuine tabs and preferences survived the host upgrade.

The [refresh investigation](benchmarks/2026-09-14-refresh-reliability.md)
documents the reproduced directory limit, disappearing-directory handling,
ignore/tracked policy, linked-worktree roots and the native sibling-event
regression. Final workspace tests passed 729 tests with five explicit ignores;
strict Clippy and release build passed. Public binary notice gaps, untested
platforms and compositor/scale limits remain explicit. The separate static
[website](../website/README.md) has current native captures and a Cloudflare
deployment plan; no public deployment or DNS change is implied.

## September 15 client-side project names

Working-tree source (branch `fix/linux-watch-skip-ignored`, on `4dfc334`) adds a
client-only project name that replaces a project's folder name in the project
hub, repository tabs and their menu, the repository heading, saved workspaces and
operation status, while leaving the folder, the repository and Git configuration
untouched. Names are stored in the existing preferences file under a new
version-4 `project_names` section, read through the shared bounded store reader
and written through the serialized preference executor.

`cargo fmt --all -- --check`, `cargo clippy --locked --workspace --all-targets --
-D warnings` and `cargo test --locked --workspace` passed on this Linux host
(debug profile; 351 app tests including the new preference round-trip, bound and
hub-interaction cases).

A debug build was exercised in a virtual X11 session (Xvfb `:77`, 1600x1000)
against a disposable `scripts/create-demo-repo.py` fixture, with
`XDG_CONFIG_HOME` pointed at a scratch directory so no developer preferences were
touched. Verified by screenshot and by reading the store: the hub shows a chosen
name over the folder name and location; the rename dialog opens from the recent
row and from **Workspaces → Rename current project…**; saving writes
`project_names` and updates the hub, tab strip, repository heading and tab menu;
submitting an empty field removes the entry and restores the folder name
everywhere; the fixture directory keeps its own name throughout.

Limitations: debug profile only, so no timing claim. X11 pointer and key input
came from a local XTest helper rather than a desktop session, and macOS, Wayland,
VoiceOver/Orca, theme and density variations for the new dialog were not
exercised.

## September 14 public-launch preparation

Application commit `7d18fef` adds a visible Linux Menu and grouped searchable
shortcut help, with shared bindings and platform labels. Release build,
formatting, locked app check, locked workspace tests and strict workspace Clippy
passed in the existing Ubuntu userspace with reused caches. Virtual X11 and
native Wayland under nested Weston verified menus, keyboard search, focus/draft
retention, contextual disabling, narrow/enlarged layouts and 2× window controls.

Package commit `326fa0f` adds collected third-party notices, source obligations
and installed notice retention. Isolated Linux installation, relocation,
reinstallation, full notice checksums and rejection of corrupted/unchecked notices
passed. The two Linux and six macOS notice gaps remain public-binary release
prerequisites. macOS packaging received syntax/inventory checks only.

The [launch validation record](public-launch-validation.md) contains exact
commands/results, executable/source hashes, genuine README screenshots and
platform boundaries. This is not a new clean build, actual Ubuntu desktop pass,
or current native macOS pass. The corrected workflow started hosted jobs; a
completed hosted pass remains separately verifiable. The
[publication checklist](public-launch.md) covers privacy and owner decisions.

## September 14 Ubuntu teammate readiness

App source `6824c7d` and packaging source `d482a3f` add a pinned Rust toolchain,
a relocatable Linux user-local bundle/installer, complete build/runtime setup,
and visible no-display/picker-failure diagnostics. The [teammate runbook](linux.md)
includes existing feature limits and the remaining Ubuntu desktop checklist.

A verified Ubuntu Base 24.04.5 x86-64 root built the locked release from fresh
Cargo caches without host libraries or the local linker workaround. Formatting,
`cargo check`, **694 workspace tests (five intentionally ignored)** and strict
all-target Clippy passed. A separate pristine runtime root passed installation,
relocation/reinstallation, executable/ELF, launcher metadata, eight icon sizes,
and useful negative/headless checks with only the documented runtime packages.

The installed executable SHA-256 is
`3c39630d691de172ee8302ab0e8bf30976dbc9cd80c957919f357ff230c1af75`.
Virtual X11/Openbox and nested Wayland/Weston screenshots and native input
verified representative History/Compare/Back, text/PNG/SVG/Markdown/GLB previews,
whole-file stage/unstage, draft/session restoration, quitting and integer 2×
rendering. The physical Pop!_OS GNOME/Wayland probe established launch and
native picker exposure only; its automation could not verify later actions.

[Detailed evidence, failures, exact boundaries and screenshots](benchmarks/linux-ubuntu-20260914/README.md)
separate the clean build, runtime-only, headless, virtual native and physical-host
checks. Rootless package ownership needed a provisioning-only fakeroot repair;
GUI portal PID namespaces needed adjustment. Successful directory selection,
actual Ubuntu GNOME shell/driver integration, fractional/mixed-monitor scaling,
macOS runtime and hosted CI remain unverified. The roots shared the host kernel;
this is not evidence of a complete Ubuntu desktop installation.

Final host-created archive extraction exposed unmapped builder ownership in
the rootless runtime namespace. Neutral numeric archive ownership fixed it;
ordinary extraction and the isolated install/headless checks passed again.
The runbook also pins the manual Cargo target directory to match packaging.

## September 14 Linux icon and window-control correction

Source `f3bf253` corrects the user-reported missing launcher icon and absent
Linux window controls. The initial icon installation used a
`hicolor/1024x1024/apps` directory absent from this desktop's theme index;
GTK failed to resolve the icon by name at every checked size. The installed
desktop entry now references the unchanged PNG by absolute path. Its
`Gio.DesktopAppInfo` icon resolved to that file, GTK successfully decoded it
at 32, 48, 128, 256 and 512 pixels, and `desktop-file-validate` passed.

The Linux tab strip now supplies standard window controls when GPUI reports
client decorations, following the desktop's left/right button order and the
compositor's supported actions. Server decorations and macOS traffic lights
remain native. The existing close action and shutdown observers are reused;
the keyboard help now lists the existing Ctrl+Q / Cmd+Q shortcut. The
[Linux guide](linux.md#window-controls-and-quitting) records the platform
conventions and corrected installation recipe.

On the same Pop!_OS/GNOME Wayland machine, Rust 1.98.0 formatting, the locked
release build, locked workspace tests and strict all-target workspace Clippy
passed. The local linker setup is
the same as the initial startup check below. The installed release SHA-256 is
`9955debcfab6487f21e54f3c74c48e02cda91d51221324e34e72f650bc76d6fd`;
all 647 recorded source/manifest/asset inputs remained unchanged through the
build. Installation used an atomic executable replacement.

The user confirmed Ctrl+Q closed the preceding build. The new installed
build opened the disposable demo repository with isolated application
preferences. Enabling the session's accessibility bridge before launch made
its full native tree available: it exposed Minimize, Maximize and Close
buttons, and semantic activation of Maximize changed the control's label to
Restore. A following automated Restore activation did not establish a state
change; tool delivery alone is not recorded as a successful interaction.
The user then confirmed that the icon and controls worked after being asked
to maximize, restore and close the window. Separately, semantic activation
of the new Close button terminated the installed process and the session
file was saved at shutdown.

The temporary accessibility bridge setting was restored to its original
disabled value, including GNOME's `toolkit-accessibility` preference; the
screen reader remained disabled throughout. The disposable repository
remained clean. The application launcher then reopened the installed build
with the user's normal preferences/session, without the QA environment.
macOS runtime checks were unavailable on this Linux machine; no new macOS
native pass is claimed. Automated drag, minimize and alternate desktop
button-layout gestures remain outside this check.

## September 14 Linux installation and Wayland startup

Source `4ad8e27352c1f33cecd145eac866ee5ff9c33e26` built and launched on
Pop!_OS 24.04 LTS, x86-64, kernel `7.1.5-76070105-generic`, with a GNOME
Wayland session. Rust `1.98.0 (88d9e12ae 2026-08-18)` was installed alongside
the existing default toolchain. Vulkan enumerated Intel Graphics (ARL), an
NVIDIA GeForce RTX 5090 Laptop GPU and llvmpipe; the app's selected adapter
was not established.

`cargo +1.98.0 fmt --all -- --check` passed. The first
`cargo +1.98.0 build --release --locked -p gitturtle` reached linking and
failed on missing `-lxkbcommon-x11`. The distro runtime
`libxkbcommon-x11.so.0` was already installed, but its unversioned development
link was absent. A local `target/linux-native-lib/libxkbcommon-x11.so` link
to that system library, supplied through `LIBRARY_PATH`, allowed the same
release command to pass. No Rust source, lockfile, system package or global
toolchain default was changed. Installing the documented development packages
is the normal setup; see the [Linux guide](linux.md).

The release binary and installed `~/.local/bin/gitturtle` share SHA-256
`12b19bd7ea4ac98947f11b35d2fcc82187040d2c8a94be37605b06502fa961f6`.
`ldd` resolved every dependency; the ELF records the system library's versioned
SONAME and has no build-directory RPATH. The existing 1024-pixel app icon and
`com.gitturtle.desktop.desktop` application entry were installed for the user.
`desktop-file-validate` passed, the desktop database was updated, and
`gio launch` successfully started the installed executable on the GitTurtle
source repository. The running `/proc` executable matched the installed path.

The initial Wayland run used `scripts/create-demo-repo.py`'s disposable fixture
and isolated `XDG_CONFIG_HOME`. The saved session contained the expected
commit, changed-file selection and History mode. The user confirmed that
the demo history window looked correct and supplied a screenshot. Visual
inspection confirmed readable text, app/control icons, all eight fixture
commits, graph edges, branch navigation, selection, commit details and the
two changed files, with no obvious clipping or rendering failure at the
captured size. Fixture Git status remained clean.
The fixture instance was then stopped and the application launcher opened
the source repository with normal user preferences. Both launches produced
empty stderr/stdout logs during these checks.

This establishes a local release build, installation, repository startup and
visually inspected window appearance. The desktop tool could not discover the
GPUI window, advertised no screenshot capability, and GNOME denied direct
window screenshot access. The visual evidence came from the user's supplied
screenshot; no automated native gesture evidence was obtained. Compare/Back,
keyboard shortcuts, picker behavior, previews,
staging, network operations and accessibility remain unverified on this
desktop. Workspace tests and Clippy were not rerun for this installation-only
task; older Linux test results retain their own source/platform identities.
The [Linux platform limits](linux.md#platform-limits) still apply.

## September 10 GitHub review conversations

The [GitHub review validation record](github-review-validation.md) records native
conversation actions and durable reply recovery, live draft/ready PR creation,
range comments and collected reviews, live reply/resolve/reopen, and stale-head
refusal in the authorized private synthetic repository. Its source/build identities
keep baseline, integrated live, and final correction evidence separate.

## September 10 native GLB previews

The [appearance and animation workflow](glb-workflow-validation.md) records
material/texture revision comparisons, skins, morphs, native playback, quiet
hidden previews and restored app state at source `8908b35`. Its
[independent reference and current corpus](glb-workflow-reference.md) and
[release CPU/native measurements](benchmarks/2026-09-10-glb-workflow.md) distinguish
geometry support, approximate appearance, processing time and frame callbacks.

The preceding [meshopt validation record](meshopt-preview-validation.md) covers
embedded compression, independently verified current corpus coverage, native
comparison refinements and restart behavior. Its [release measurements](benchmarks/2026-09-10-meshopt.md)
separate actual decode/render coverage and CPU timings from native observations.

The [GLB validation record](glb-preview-validation.md) covers bounded static
geometry, faithful revision placement and scale, native camera controls,
History/Working Changes and retained-filter isolation, representative appearance
and window sizes, and restored application state. It identifies the exercised
local macOS packages separately from the [release decoder/raster measurements](benchmarks/2026-09-10-glb.md).
Compression, deformation and omitted appearance remain explicit in the
[finite GLB support contract](interactive-3d.md#glb-20-static-geometry).

## Final September 10 security milestone

[Source `b4440f1`](security-quality-milestone.md) passed the macOS and local
Linux formatting, workspace tests, strict Clippy and release gates. The
[20 minute 15 second native session](benchmarks/security-native-20260910.md)
covered eight tabs, demanding history/status/diff and preview fixtures,
same-path repository replacement, local cancellation and saved drafts.
The [large offline PR probe](benchmarks/large-pr-probe.md) separately exercised
300 files, 2,000 comments and 128 substantial drafts through actual backend APIs.
The [installed identity and restoration record](benchmarks/security-installed-20260910.json)
verifies UUID `2475FAA0-21B2-398D-9F9C-A2D57C92543A`, exact-path native smoke,
genuine state restoration and unchanged system settings. Live authenticated
GitHub, native Linux UI, hosted CI and maximum-size native PR interaction remain
outside this evidence. No application-wide speedup or leak conclusion is claimed.

## Final September 10 native polish milestone

[Final source `eb3dd26`](native-polish-milestone.md#final-corrections-installation-and-restoration)
passed macOS and Linux formatting, workspace tests, strict all-target Clippy and
release builds. The exact installed `/Applications/GitTurtle.app` passed bundle,
running-identity and native Compare/Back/Settings checks; original application
state and system settings were restored. [Validation identities and log hashes](benchmarks/native-polish-20260910/validation.json),
[32 unaltered native visuals](benchmarks/native-polish-20260910/visuals/README.md)
and the separately identified [21 min 58 sec resource session](benchmarks/native-polish-20260910/README.md)
retain their exercised source boundaries. Live GitHub, hosted CI, native Linux UI,
spoken VoiceOver and sleep/wake remain unverified.

## Final September 9 consistency milestone

[Source `92ea02c` installation and restoration](consistency-milestone.md#final-build-installation-and-state-restoration) records the installed UUID/signature/resource checks, 461 macOS and 457 Linux passing tests, native workflow coverage and original-state restoration. The [mixed native session](benchmarks/2026-09-09-milestone-native.md), [graph measurements](benchmarks/2026-09-09-milestone-graph.md) and [image-retention correction](benchmarks/2026-09-09-preview-lifetime.md) preserve distinct measurement boundaries and raw observations. That milestone ended with an original-repository access limitation; the current milestone records successful reopening of the unchanged build without permission changes. Its hosted/Keychain/hardware/Linux-window and tool-specific limits remain explicit in that record.

## Current validation guidance

The dated records on this page apply to their named builds and environments. They do not establish native or package coverage for later source changes. Use the [current feature overview](user-guide.md#current-source-features), [architecture and bounds](architecture.md), [preview matrix](file-previews.md), [profiles](profiles.md), [command palette](command-palette.md) and [rewritten-series review](rewritten-series.md) for implemented behavior. The completed [security milestone](security-quality-milestone.md), [macOS milestone](macos-milestone.md) and [everyday-work record](everyday-work-plan.md) remain evidence for their identified inputs, rather than an active feature queue.

For changes to the current workflows, use disposable repositories and local remotes for mutations, and select the relevant checks below. Record source revision and relevant dirty-input identity, executable/package identity, target/profile, OS, display backend and scale. Compare `--build-info` with the exercised artifact and independently inspect Git results; a control or passing core fixture does not by itself verify native interaction. Required evidence belongs to the current [task contract](development/README.md#task-contracts-and-ownership), with new results recorded under their actual date/build.

| Current workflow | Relevant validation |
| --- | --- |
| Repository tabs and local workspaces | Open a new repository after a search, verify independent inputs, canonical alias deduplication and linked-worktree identity, eight-tab bound, pin/group/reorder/close, captured in-flight writes, draft recovery and lazy restart bookmarks. Exercise moved/missing paths and explicit picker recovery. |
| Projects and repository opening | Open/cancel the native picker, search/clear recents, clone from a local remote and create an unborn repository. Check paths with spaces, nonempty destinations, missing/moved repositories, picker/tool failures, duplicate submission, retained form input and return to the captured repository. A failed open must preserve the prior worktree's outcome and cannot apply late content from another repository. |
| Persistent app state and shutdown | Use disposable app stores for supported-version migration, corrupt/unsupported/nonregular stores, capacity and failed-save paths. Verify visible Saving/Saved/error feedback, exact commit/conflict/GitHub draft text, tab/order/bookmark retention and normal quit/final-window close followed by restart. Preserve invalid originals and newer edits after a failed older save; a Git write followed by app-store failure must not replay Git. Follow the [persistence contract](../crates/app/docs/writes-and-persistence.md#preferences-and-commit-drafts): confirmed Saved is the durability boundary for slow I/O or forced termination; normal-shutdown evidence does not establish crash-time saving. |
| Incremental ordinary history | Page across the 5,000-row/64 MiB window with an older selection retained; verify stable OIDs, connected graph frontier and native Older/Previous/Latest behavior, top following without selection changes, and the Show latest cue while browsing older rows or Compare. Inspect slim and lane-overflow graphs, rapid selection, cancellation and search discontinuity. Use the [120k fixture measurements](benchmarks/2026-09-10-history-pagination.md) for backend comparisons and separate native callbacks. |
| Interactive model comparison | Exercise pointer and keyboard orbit/pan/zoom/fit, standard views, linked and independent cameras, edges, orientation, units, missing sides and captured originals. Compare GLB material-only and texture-only revisions, static skins/morphs, clip selection, Play/Pause, scrubbing, different durations and Reduce Motion. Check fixed cameras and current-pose Fit, malformed appearance fallback, paused/hidden quiescence, rapid activation/Back, tabs and closure. Compare changed transforms/scale, absent and unsupported sides in History and Working Changes; retain separate path filters through navigation and refresh. Compare curved analytic and mapped STEP fixtures within the [finite support matrix](file-previews.md), then repeat opens/tab changes/window closure while observing resource retirement. |
| PDF and rendered Markdown | On a supported platform, navigate PDF beyond page 8 with entry/previous/next, unequal counts, linked positions, zoom, extracted text and literal copy; retain state across Back/tabs/restart and evict bounded cached pages. Check native Markdown prose/tables/code/Mermaid, revision-correct local images, explicit local/external links, linked scrolling, keyboard reading and exact source staging. Verify explicit unsupported-format feedback against the [platform limits](linux.md#platform-limits), without treating metadata recognition as rendering. |
| GitHub collaboration | Open/close the offline native panel through the palette; check keyboard activation, visible composers, confirmations, refreshed PR and recovery lists at narrow/wide sizes, both densities, larger text and light/dark themes. Reply, resolve/reopen, page authoritative conversations and retain exact drafts through navigation, tabs and restart. Check stale account/head/thread identities, missing context, permissions, rate limits, partial failures, persistence failures, cancellation and uncertainty. Real connection, PRs/comments/reviews and hosted CI require specifically authorized disposable context; record independently verified live results and credential access separately. |
| Precise staging and commits | Exercise hunk and changed-line stage/unstage with mixed index/worktree edits; verify unrelated index entries and working bytes. Check whole-file fallback explanations, exact Title/Description bytes, hook/signing failures, and worktree-specific draft retention through navigation and restart. Exercise Discard changes and Delete untracked file from a working row's context menu on modified, staged, renamed, added, deleted and untracked rows: verify the review text, the stale refusal after a later edit, restored HEAD content, the deleted untracked file, and untouched sibling changes. |
| Conflicts and integration | Inspect base and both named sides, rebase labels, manual and complete-side resolution, external edits, stale-save refusal, and editor handoff. Verify Continue's staged-path review, external operation detection, Abort preservation, and Keep files without losing HEAD/index/worktree state. |
| Stashes and commit recovery | Inspect staged/unstaged/untracked saved content; restore with and without staged state; confirm the stash survives success and conflict until an explicit Drop. Check amend, eligible Undo, revert/cherry-pick and merge-parent choice, including stale targets, failures, and independent work. |
| Branches and remotes | Review actual switch/create/integration targets, invalid rename names and destination collisions, tracking/upstream changes, safe deletion, and linked-worktree occupancy. Verify remote configuration separately from explicit fetch/pull/push. |
| Named profiles and identity | Follow [profile semantics](profiles.md): create/edit/delete definitions without changing Git, then review and explicitly apply the actual author/signing settings and shared/private worktree scope. Check retained editor text, stale definitions/configuration, held locks, active-operation refusal and visible mismatch after external edits. Preserve includes, unrelated configuration and signing requirements. An assignment-save failure after a successful Git write must surface the storage problem without repeating or undoing that write. |
| Rewritten-series review and publication | Use the [native rewrite fixtures](recovery-rewrite-native-cases.md) and [series contract](rewritten-series.md) for original/new messages, changed/reordered/possible/ambiguous pairs, missing objects, inspector activation and Back/close cancellation. Publication requires a separate explicit destination check and reviewed exact lease. Exercise local/remote/URL movement, hook refusal, cancellation after remote update, explicit completion detection and fresh review after movement against a disposable local remote; returning focus or reopening the app must not publish or retry. |
| Search and file history | Find a match beyond loaded history, retain pinned scope across ref movement, cancel active work, and continue a bounded scan without claiming exhaustion. Cross file-history page and rename boundaries, inspect deletions/merge parents, and retain query, revision, selection, viewport, and focus through Compare/Back/Settings. |
| Diff and refresh interactions | Inspect unified and split alignment, Find, copying without padding/gutter text, opposite-side scrolling, and partial selections. Make external file/ref changes, switch focus away and back, and verify coalesced local refresh retains context/drafts while invalidating stale selections. Exercise watcher errors and manual recovery. |
| Authentication and cancellation | Use disposable loopback transports and configured helpers for username/token prompts, expired credentials, SSH-agent transport, host verification, configured commit/tag signatures and signing refusal, exact-secret diagnostic/progress masking, cancellation and no replay. Verify retained index/working state and explicit remote targets. Test detached helpers retaining output or input pipes; the app must stop and join its own I/O threads. [Local authentication/signing evidence](authentication.md#verification-and-limits) is separate from live-provider access, real Keychain unlock and hardware-backed signing. |
| Blame and line history | Exercise immutable revision attribution, raw working files against HEAD, staged-only and unstaged uncommitted lines, renames, shallow history and unavailable content. Copy exact source; inspect a line's commit and bounded first-parent history; verify Back, focus, selection, cancellation and stale-result rejection across nested inspections. |
| Tags and ignore | Filter and inspect tags; review lightweight/annotated creation and signing; refuse moved-tag deletion and changed remote destinations; verify that named-tag Push creates only that remote ref. Preview literal file/directory rules in shared/local destinations, preserve formatting and unrelated work, refuse stale/symbolic writes, and keep tracked paths tracked without staging. |
| Image comparison | Exercise side-by-side, Overlay opacity and draggable Wipe with linked pan/zoom, keyboard adjustment, checkerboards, different source sizes/downsample ratios and missing sides. Verify scale labels, gesture cancellation, retained navigation and unchanged decoder bounds. |
| macOS conventions and accessibility | Check menu availability, standard shortcuts, Help, Hide/Minimize/Close, captured Finder/editor handoff and launcher failures. Exercise follow-system appearance and manual themes without losing editor context. Inspect ordinary keyboard focus, names and supported selected/expanded/disabled states, and record available transparency/contrast/motion settings separately from unsupported hardware or OS versions. Exercise VoiceOver names, roles, selected/expanded/disabled states, current-row announcements, editing, modal containment, restored focus and status/error announcements using the [native accessibility contract](native-accessibility.md); record actual settings and build-specific results. |
| Linux desktop integration and text | Follow the [desktop checklist](linux.md#ubuntu-desktop-acceptance-checklist) for the affected session: visible Menu and shortcut help, Control-based shortcuts, client/server window decorations, move/maximize/restore/close, picker success/cancel/portal failure and explicit editor launch. Check live Wayland portal text-size/antialiasing changes, missing-portal/fontconfig fallback, focus-return recovery and X11 DPI without applying a second text multiplier. Retain logical source rows, split/gutter alignment, selection, Find and hidden-tab context through scaling. Record physical/nested/virtual backend, compositor, GPU and scale; semantic-tree exposure is not screen-reader or IME acceptance. |
| Packages, upgrades and build diagnostics | Use the [macOS](../.agents/skills/gitturtle-native-qa/references/macos-package.md) or [Linux](../.agents/skills/gitturtle-native-qa/references/linux-package.md) package procedure for the affected target. Match source/compiled identity, executable and artifact hashes, metadata, embedded/bundled assets, notices and the running path. Check About/Copy bug diagnostics and exact information flags without opening app state. On Linux, use package/installer fixtures and the actual extracted archive for identity/notice refusal, checksum verification, relocation, active-process refusal, corruption and rollback preservation. Match the archive and installed executable hashes; keep strict distribution refusal distinct from a development-bundle pass. Synthetic payload/tool tests do not establish archive or native acceptance; on macOS, distinguish local ad-hoc signing from notarization and other-machine acceptance. Package/library/headless checks do not replace real desktop interaction or clear the [public release requirements](public-launch.md#before-a-public-binary-release). |

Native checks should cover relevant narrow/wide layouts, long names, large lists, themes, densities, independent interface/code text sizes, keyboard focus, hover/selection/disabled states, and empty/loading/error states for the affected controls. Changes shared across the palette or scaling system need representative light/dark and boundary-size coverage; use all supported themes when the change affects every palette. Distinguish pointer, keyboard and screen-reader results. Final combined Rust/dependency gates and package checks follow [the project validation agreement](../AGENTS.md#validation); a docs-only update requires link and diff review, without rebuilding the app.

### Retained review and recovery workflow checks

These rows describe checks to select for the affected feature, not completed native passes or a mandatory full sweep. Record new results with the exercised build and task; retain [milestone evidence](security-quality-milestone.md) under its original identity. Keep final release, installed executable identity and platform/account-dependent evidence separate. Re-run a successful check only after relevant changes or a concrete unresolved concern.

| Required feature | Current validation scope |
| --- | --- |
| 1. Revision comparison | Use [the revision workflow](user-guide.md#browse-history-then-open-a-comparison) to compare diverged branches, tags and explicit commits in both directions and modes. Check resolved IDs, rename/mode/type changes, absent text/image sides, ambiguous names, moving refs, missing objects and unrelated/multiple-base ancestry. Cancel during a read; verify no checkout/fetch and Back/focus restoration, including a late preview after leaving Compare. |
| 2. Text review | Exercise [review variants](architecture.md#prepared-diff-presentation) in unified/split modes: intraline Unicode edits, CRLF, no final newline, long lines, whitespace suppression, context expansion through 192 lines, and Option-Up/Down. Verify literal source copy, Find, gutters and linked scrolling. Filtered/expanded variants must explain disabled partial staging; resetting must restore exact Git actions and preserve unrelated changes. |
| 3. Text size and accessibility | Follow [typography and density](../DESIGN.md#typography-and-density): independent interface/code settings and resets, persistence, both densities and twenty themes at minimum/wide sizes. Retain selection, focus, Find and viewports through scaling. Inspect names/roles/supported states and Increase Contrast, Reduce Transparency and system light/dark behavior where available. Exercise VoiceOver names, roles, selected/expanded/disabled states, current-row announcements, editing, modal containment, restored focus and status/error announcements using the [native accessibility contract](native-accessibility.md); record actual settings and build-specific results. |
| 4. Quick Open and path filters | Exercise Command-P immediate typing, worktree versus pinned revision scope, keyboard selection/Return/Escape, Unicode/long/raw-byte paths, deleted/conflicted/unsupported files, no matches and visible truncation. Verify File History/Blame use the inspected target and Back restores an interrupted source preview. Check [bounded discovery](architecture.md#revision-inspection-review-and-recovery), rapid query replacement, repository switching, and changed/working file filters. |
| 5. Multi-file staging | Exercise Command-toggle, Shift-click/arrow ranges, Command-A, selected counts, directory grouping and separate staged/unstaged identities. Compare Git index/worktree bytes before/after exact selected Stage/Unstage, including renames, binaries and mixed states. Filtering/grouping clears selection; refresh retains only visible survivors; switching repositories clears it. Check stale plans, partial failures, filtered all-files disabling and existing hunk/line staging. See [working operations](user-guide.md#open-a-project-and-work-with-git). |
| 6. Worktree management | Follow [worktree semantics](parallel-work-recovery.md#worktrees): review and create existing/new branch destinations, inspect state and hand off to GitTurtle/Finder/editor. Refuse occupied branches, stale identities, dirty/untracked/ignored content, locked/missing/main/current worktrees and active conflicts. Exercise Force remove worktree on a dirty target: its confirmation shows the file counts, ordinary removal stays refused, force removal deletes the folder and retains the branch, the confirmation lists the deleted paths and unfinished state, and locked/main/current/missing targets, held lock files, submodules and nested repositories stay refused. Verify shared versus private configuration/drafts, branch retention after removal, and honest partial-checkout failure feedback without recursive cleanup. |
| 7. Activity and reflog recovery | Exercise the bounded [activity/reflog workflows](parallel-work-recovery.md): captured repository/target/time, running and final outcomes, cancellation/uncertainty, restart, and explicit next actions without replay. Confirm retained activity excludes secrets and arbitrary diagnostics. Inspect available and expired/missing reflog commits; create the exact recovery branch after revalidation while preserving HEAD, index and working bytes. |
| 8. Conflict blocks | Follow [block resolution](conflict-blocks.md) across merge, rebase, cherry-pick and stash conflicts, including merge/diff3/zdiff3 markers. Test Previous/Next and shortcuts, Current/Incoming/Both, manual editing, unresolved counts, empty/CRLF sides, malformed markers and fallbacks. Save draft must leave the index conflicted; Save and stage must refuse remaining markers/stale sources and preserve unrelated entries. Retain drafts through file/view changes and unrelated refresh; exercise Continue/Abort/Keep files separately. |
| 9. Interactive rebase | Follow [native rebase](interactive-rebase.md): reviewed exclusive base, exact sequence, button and Option-arrow reorder, P/R/S/F/D actions, invalid squash/fixup positions, known-remote acknowledgment and stale plans. Verify native reword/squash messages, separate base/replayed-commit labels, hooks/signing failures, intermediate cancellation, conflicts, Continue/Abort and restart resume. Protect tracked/untracked/ignored work and explain unsupported histories; no automatic force-push. |
| 10. Explicit LFS downloads | Use [LFS preview semantics](lfs-previews.md) with a disposable local source. Verify reviewed file/object/source/size, one-object scope despite unrelated/recent pointers, missing tooling/configuration/credentials, cancellation, corrupt/unavailable objects and stale plans. Independently confirm SHA-256/size, unchanged HEAD/index/worktree and preview reload only for the still-selected target. Include raw working pointers and resolved LFS text's whole-file-only staging; ordinary browsing must remain passive. |

The [September 9 app preparation and path-filter report](benchmarks/2026-09-09-review-app.md) records seventeen release series with forty measurements and three warmups each, including bounded diff/intraline/split/context preparation and cached 50,000-path filters. It reports process memory and CPU-only timing separately from native frames.

The [September 9 release backend report](benchmarks/2026-09-09-review-backend.md) and [raw attempts](benchmarks/2026-09-09-review-backend.json) cover thirteen series with forty measured calls and three warmups each: endpoint/merge-base comparison, tracked-path searches, conflict parsing, rebase planning, worktree listing/details, reflog reads and passive LFS preparation. Source/fixture hashes were stable throughout the run. The report includes p50/p95/p99/max and substantial uncontrolled background load; it excludes native frames, transfer/cancellation latency, mutation, memory-growth analysis and final-package verification, and makes no speedup claim.

The [September 8 everyday backend report](benchmarks/2026-09-08-everyday-workflows.md) records release core measurements for status, working previews, search, file history, and changed-file reads with/without renames. Its raw data and fixture checks establish the stated backend observations; they exclude native frames, writes, watcher behavior, and package verification.

Current semantics and focused fixture commands are documented in [authentication](authentication.md), [tags and ignore](macos-git-actions.md), and [attribution, images and macOS conventions](macos-features.md). The [Liquid Glass investigation](liquid-glass-investigation.md) records the actual native prototype and compositing limitation; the integrated appearance remains opaque. The [previous macOS backend report](benchmarks/2026-09-08-macos-milestone-backend.md) identifies its source inputs and measurement scope independently of native frame evidence.

The [CI workflow](../.github/workflows/quality.yml) configures locked workspace tests and strict all-target Clippy on macOS 15 and Ubuntu 24.04, formatting, and release compilation/package checks on macOS 26 and Ubuntu 24.04. Python guidance/controller checks run on macOS 15 and Ubuntu 24.04. Disposable package checks cover identity, notices and applicable installation, ELF or Mach-O verification; diagnostics are uploaded, while [binary artifacts](ci-artifacts.md) require complete notices. [Actual hosted results](benchmarks/2026-09-15-ci.md) remain distinct from configured coverage and from physical-desktop, screen-reader, native-package or distribution acceptance.

## Review and recovery installed release — September 9, 2026

Final compiled source `0c8eaec31ae9fdbc7fe7f98c5822cc0561f96ad4` passed formatting, locked workspace tests, strict all-target workspace Clippy and release compilation. The package and installed `/Applications/GitTurtle.app` share UUID `B003BB92-D6C0-3009-8A75-96A85EEBB133` and executable SHA-256 `0f346ccc7a7149d5314cebc5c0893ee182d166e2c569c6b5051906b57dc69777`. Plist/signature checks, exact installed-path launch, essential native interactions and preservation of the original preferences/drafts passed.

The [complete native record](review-native-verification.md) attributes comparison, text review, file navigation, selection/staging, worktree/recovery, conflict/rebase and local LFS checks to their actual builds, including final minimum-size and modal-focus corrections. It includes representative screenshots, system appearance/accessibility observations and precise VoiceOver/GPUI limitations. The [finite acceptance ledger](review-milestone.md) is complete. Live-provider/Keychain/hardware and hosted CI/Linux checks remain unverified for the stated environment/access reasons; none is implied by these local passes.

## Everyday workflows release review — September 8, 2026

Source `f68bd2070d4c71eec00af02cb7f67b1be107e728` passed workspace tests, strict workspace Clippy, and a release build. The packaged executable has UUID `83F6445B-F09F-3AAC-A138-BA72E7D77CAF` and SHA-256 `672615cd826e9c7001e5ef774c08b90d350ee7d6935857668c336b7e315049b1`; package signature verification passed and its UUID matched the release executable. Native checks used that release on Apple M4 Max with 128 GiB memory and macOS 26.6.2. This is local macOS package evidence, without notarization or Linux validation.

The rich disposable fixture at HEAD `bcf41e2d24ff5582a145be5d34ebbdca774c95b4` contained 653 commits, 91 local branches, 1,043 files in the selected commit, and 996 distinct working changes. All six themes—Midnight, Daylight, Graphite, Tokyo Night, Catppuccin Mocha, and Nord—were inspected in both Comfortable and Compact density at the minimum 1,000 × 680 content size. Full-window screenshots measured 1,000 × 712 including the title bar. Resizing to approximately 1,480 × 980 also exercised History and a Midnight image comparison. Narrow checks concentrated on Split, Find, and the composer; this is not a claim that every workflow was repeated in every theme, density, and wide layout.

| Area | Observed behavior in `f68bd2` |
| --- | --- |
| Appearance and navigation | Inspected source colors, selected and hovered rows, focus transitions, disabled network controls when no remote was configured, status icons, and long nested Unicode paths. Find focus returned exactly after Settings and theme changes. |
| Editor Find | Copying the query while all source text was selected copied the query. The exercised match changed from zero with case sensitivity enabled to one with it disabled. Enter, Shift+Enter, Escape, and Cmd+Shift+F routing worked in the exercised comparison. |
| Working images | Added/untracked PNGs showed an absent Before side; modified transparent PNGs showed both sides at 100% with synchronized horizontal pan. Fit reset the view, and a deleted PNG showed an absent After side. |
| Projects | A no-match search and Clear updated immediately. Canceling the native Create picker retained the name and `trunk` branch input. Create produced the empty, clean disposable `native-release-created-20260908` repository with exact `refs/heads/trunk` identity. Clone refused that populated destination without changing it, then cloned a local bare origin into `native-release-cloned-20260908` at `3c0324d5a9bb8bdbf4ddbf4b70934820d159ce47`. |
| Identity settings | Native Save was exercised for repository-local name `Native Release QA` and email `native-release@example.invalid`. Direct `git config --local` inspection verified both exact values in the cloned fixture. |

The minimum-height review exposed insufficient Working files space beneath the expanded Targets and composer. Density/viewport changes could leave the selected file offscreen, and leaving Repository for Settings or Projects discarded manually resized pane widths. Find highlighting over patch colors and an anonymous Projects clear control also needed correction. These changes are implemented in subsequent source `b20dddb6bdc40ea332872333e2a59db80c0a7b9f`, which passed workspace tests, strict workspace Clippy, and a release build. Its package and focused native checks are recorded below; the `f68bd2` matrix does not itself validate those corrections.

Follow-up direct Git inspection completed the earlier debug branch checks from executable UUID `289AC96C-08CF-3C84-A839-125F76446075`. The native workflow removed only the local `main-native-renamed` branch, set the exact upstream `origin/release/stable`, and edited the origin URL to its intended trailing-slash form. Removing origin then removed its remote-tracking refs and upstream configuration while preserving HEAD `6f07fd3`, stash `64d6…`, and the mixed working changes. These results supplement that earlier debug workflow; they were not repeated as `f68bd2` native branch actions.

### Focused release follow-up

The `b20dddb` package passed signature/plist checks and launched with executable UUID `312A7611-5F59-33E8-9AD7-FFE389CBBFEB`, SHA-256 `5bcf7378e2b388f24d7c4d351684af2266a09a3e7abbadcba6b32828d44986d8`. The release executable and packaged UUID matched. Native checks used only disposable fixtures and the existing local bare remote.

| Area | Observed behavior in `b20dddb` |
| --- | --- |
| Scoped recovery | Reverting `a304837` on `qa/scoped-recovery` created `541d9f4` while retaining that scope, its query, and the pinned old result. Restart showed the revert and original commit, excluding the matching unrelated branch commit `0213cb3`. |
| Stash conflict | Restoring `9891d154` showed “Stash restoration produced conflicts. The stash remains saved.” Full Details retained Git output. Direct inspection verified the same stash OID, the saved untracked note, an unmerged file, and no merge/rebase/cherry-pick/revert operation metadata. |
| Responsive layout | At minimum size with Targets open, Daylight Compact and Graphite Comfortable kept the selected working file visible with approximately four to five file rows and a multiline Description. Collapsing Targets increased list space. Manual inspector and navigator widths survived Settings/Back and Projects/Back; inspector width also survived History/Compare transitions. |
| Find and project search | Unified active Find contrast over an added line passed. The accessible Projects “Clear project search” button cleared immediately and retained input focus, verified by typing the next filter without clicking the field. Split still showed intermittent background precedence defects; a subsequent correction is required. |
| Large branch lists | The menu bounded its alternatives to forty. Searching `component-001` reached an entry beyond that initial list. `review/accessibility` appeared disabled and labeled as occupied by another worktree. The search command was buried beneath the unfiltered list; a subsequent presentation change moves it first. |
| Local remote workflow | Explicit Fetch changed the clone from zero known commits behind to one; fast-forward Pull reached `4f8112a`. Native staging, Title/Description commit, and Push produced `7cffcf32` in both clone and bare remote. Raw commit bytes exactly preserved the title, blank line, and two-paragraph description. |
| Divergence and merge | Explicit Fetch showed one local and one upstream commit. Pull refused fast-forward and retained visible Merge/Rebase choices. Prepared Merge named the branches and both differing paths; execution created `3483b47b` with exact parents `10ca8026` and `74c8187c`. Both disjoint files and earlier pushed/pulled files remained, with a clean index/worktree. |
| Missing image and narrow Projects | A stored LFS pointer displayed its unavailable local object and stated that no download was attempted. At minimum size, the Projects Clone form retained readable fields and its lower action remained reachable by scrolling. |
| Focus and external changes | With TextEdit active, an external fixture file was created. Returning to GitTurtle exposed that exact new file without manual Refresh; this exercises watcher/focus handling together, not an isolated focus-event latency measurement. |

A complete before/after inventory of the rich fixture verified all 1,236 file/symlink entries, sizes, hashes, and modification times unchanged after these read-only checks, excluding immutable Git objects and access/directory metadata. Direct local-remote verification is retained in the disposable `native-final-remote-evidence.json` record. This pass also found a previous repository's error banner surviving a switch and a fast-forward failure headline dominated by Git's fetch progress. Their fixes, the Split highlight correction, and the branch-search ordering change require a subsequent package/native check; none is counted as verified here.

### Final package verification

Final application source `1eebcb2a54813c3d0c96e77e80d983234e7264ae` includes the native corrections above and the divergent Pull guidance from `d48d940`. Formatting, `cargo test --locked --workspace`, strict all-target workspace Clippy, and `cargo build --release --locked -p gitturtle` passed together on this source. The package was rebuilt with `--no-build` after closing the previous app. Plist and strict ad-hoc signature checks passed; the release and packaged executable UUIDs both equal `C81E4C94-08EB-3283-860E-BE6F84E69616` (arm64). The packaged executable SHA-256 is `01577ad8a2bd7ba162c17840de7fc75bbd9051b70dd9575baecb29b7ed099f2b`.

The exact package launched and passed the focused correction checks:

- Explicit Fetch showed two local and one upstream commit. Pull refused the fast-forward with “Branches have diverged; choose Merge or Rebase to continue.” The branch and merge HEAD `3483b47b` remained unchanged, as did the independent untracked focus-test note. Native invalid Rename retained `bad name` with an example and specific naming guidance, without reaching a write confirmation.
- The operation error survived Projects/Back to the same repository, cleared when another repository opened, and did not return when the first repository reopened. The current-branch menu placed Find/Create immediately above its forty bounded alternatives.
- Split Find on an added source line retained readable syntax, a distinct selected background, and an accent underline across all six themes at minimum size. Graphite was also checked before and after wide/minimum resizing. Closing Find restored the complete patch background; Unified Find highlighted both removed and added matches. Settings/theme/density changes retained Find focus, demonstrated by replacing the query without clicking its field. Selected-file visibility and the adaptive composer settled correctly after resizing.
- Graphite and Comfortable density were restored, Targets remained expanded, and the package returned to the GitTurtle project's History page for passive inspection. The rich fixture's 1,236 protected file/symlink entries retained their recorded sizes, hashes, and modification times after the final checks.

All artwork under `assets/` remains unchanged from the pre-goal `88153ce` revision; the rebuilt package continues to consume the same Icon Composer light/dark/clear sources and embedded turtle artwork. The dated Finder appearance evidence below remains attached to that unchanged artwork. The later documentation commit changes no executable source or dependencies, so it does not require another Rust build or package.

These results complete the requested implementation and relevant local validation. The earlier native workflows and measurements retain their stated build identities and measurement boundaries. Linux, hosting-provider credential interaction, universal binaries, and notarized distribution are not claimed; the configured-helper failure fixtures and local-remote native workflows establish their narrower coverage.

### Everyday release timing

The `f68bd2` package measured twenty callbacks in each of four native selection paths on the rich fixture. [Raw samples, phase boundaries, environment, cache assumptions, and preservation checks](benchmarks/2026-09-08-everyday-native.json) are retained.

| Interaction | Samples | Median | p95 | Maximum |
| --- | ---: | ---: | ---: | ---: |
| Commit to changed-file frame | 20 | 21.241 ms | 45.238 ms | 62.794 ms |
| Immutable file to prepared text-preview frame | 20 | 4.898 ms | 7.428 ms | 13.216 ms |
| Staged file to prepared working-preview frame | 20 | 21.838 ms | 21.947 ms | 22.043 ms |
| Unstaged file to prepared working-preview frame | 20 | 23.248 ms | 24.800 ms | 24.838 ms |

The fresh release process opened only the rich fixture. History, immutable-file, staged, and unstaged phases ran in that order with Daylight and Comfortable density. Initial activations were excluded, with no separate warmup series; filesystem caches were not flushed and return immutable selections could use the 32-entry preview cache. Mutable previews bypass that cache. History selection produced zero file-preview frames. No build, test, backend benchmark, or unrelated native interaction ran during the measured phases; other desktop applications remained open and OS load was uncontrolled.

These timings span the input handler to its generation/mode-checked GPUI callback. They exclude input delivery before the handler, OS display presentation, and completed GPU work; working-preview measurements also exclude status refresh and writes. The 1,236 recorded fixture entries retained their kinds, sizes, hashes, and modification times after the phases, with no Git writes during timing. This small uncontrolled sample has no before/after baseline and establishes neither a speed improvement nor a latency guarantee. Watcher, cancellation, write, image, and memory performance are outside these measurements.

## Native icon appearances — September 8, 2026

Source revision `174a68d` replaces the premasked tile with a layered Icon Composer document: the selected mint turtle on a full-bleed background, with macOS providing the final enclosure. The document was inspected and saved in Icon Composer from Xcode 26.6. Native renders verified [light](../assets/icon-previews/light.png), [dark](../assets/icon-previews/dark.png), [clear light](../assets/icon-previews/clear-light.png), and [clear dark](../assets/icon-previews/clear-dark.png); Mono also supplies tinted appearances. [The provenance record](../assets/app-icon.prompt.json) preserves both the original selection and foreground-extraction prompt.

The release executable was rebuilt because the embedded branding PNG changed. Both `dist/GitTurtle.app` and `.local/GitTurtle.app` were repackaged and registered with Launch Services. Each passed plist and strict ad-hoc signature verification and has executable UUID `F71AC1CB-DD6B-3552-8DCF-03EC31C8EB19`, matching the release executable. Their compiled catalogs contain Aqua, Dark Aqua, and tintable icon stacks; both fallback ICNS files have SHA-256 `5206a75c4c2ae53f7686565042bb7e4e67b1ca09ebc3d4f2e5a621f3be37fefb`. Generated metadata sets both `CFBundleIconName` and `CFBundleIconFile` to `AppIcon`. Raw artwork is excluded from the bundle. The catalog targets macOS 11.0, matching the executable's recorded minimum; the fallback was not exercised on an older operating system.

On macOS 26.6.2, Finder displayed the exact `dist` package in Default, Dark, Clear Light, and Clear Dark styles. Each used one enclosure with no inset tile. Cycling the icon style refreshed the initially cached image without deleting global caches. The packaged app displayed the updated embedded branding and returned to the user's repository in History with Nord preserved. System appearance preferences were restored to their original values. The Dock surface itself was unavailable to native capture, so the visual appearance checks above refer to Finder and the app. No Git write or network action was performed in the user's repository.

Both shell scripts passed syntax checks, the render and packaging commands completed successfully, metadata hashes matched their source assets, and the final diff passed whitespace checks. Rust and dependencies are unchanged from `e8f7d42`, whose 115 workspace tests and strict Clippy passed as recorded below; these gates were not repeated for this artwork and packaging change.

## Consistent selected icon — September 8, 2026

At `e8f7d42`, the repository header, Projects header, and collapsed sidebar switched from the old vector turtle to a shared 128-pixel PNG derived from the selected `assets/app-icon.png`. The original source remains byte-identical to its recorded SHA-256 `b40bbffd6d33be3981094f1886e953a114fada7ede83dcaa0393b415bd553209`. Regenerating all ten iconset sizes produced an ICNS byte-identical to the existing selected icon. The obsolete vector variants were removed; packaging now replaces its assets directory so retired files cannot linger.

Both `dist/GitTurtle.app` and the older Launch Services–registered `.local/GitTurtle.app` were rebuilt from the final release executable and re-registered. Both passed plist and ad-hoc signature verification and have executable UUID `7C5A16FA-4C95-3FB6-BD17-94B01CED85E4`, matching the release build. Their icon, original artwork, and small branding PNG match the source assets byte-for-byte; neither contains the retired vector files. Formatting, `cargo check`, all **115 workspace tests**, strict workspace Clippy, release build, and packaging-script syntax checks passed.

The exact `dist` package was relaunched. Native screenshots verified the selected turtle in both headers and the collapsed sidebar; Finder's icon view also displayed the selected artwork. History and the expanded sidebar were restored with the user's current Nord theme and repository. The Dock surface could not be captured through native automation, so Dock appearance itself is not claimed as a visual check; bundle resources and Launch Services registration were verified without resetting global icon caches. No repository write or network action was performed.

## Native refinement — September 8, 2026

Final source revision `76f7d9a` passed formatting, **115 tests: 69 app, 34 core, and 12 preview**, strict workspace Clippy, and release packaging. The release and packaged arm64 executable have UUID `6B05721F-2BDD-3DE0-9927-93431CC12047`; plist and ad-hoc signature verification passed. The last source adjustment isolated parallel test fixture directories with an atomic sequence after a timestamp collision; it does not change the release executable. The existing app icon was retained, as requested.

Native checks used packaged revisions `ddab7e3`, `1f7e6a4`, and final `76f7d9a` on Apple M4 Max with 128 GiB memory and macOS 26.6.2. The first exercised all application pages and Git actions; the second exercised named staging feedback, six themes, and successful clone/create; the final verified both project-search clear controls, clean-state guidance, and large-repository navigation and timing. Only disposable fixtures and local remotes were mutated.

| Area | Observed behavior |
| --- | --- |
| Hover and selection | Selected history, file, navigation, theme, and density controls retained selection while showing hover feedback. Primary action hover, input focus rings, disabled actions, and status icons were inspected in the native app. Palette tests cover selected-hover surfaces across all six themes, requiring 4.5:1 secondary-text and 3:1 status-icon contrast. |
| Settings and narrow layouts | Inspected Midnight, Daylight, Graphite, Tokyo Night, Catppuccin Mocha, and Nord. At the approximately 1,000-pixel minimum window width, settings groups stacked and lower identity/default-branch fields remained accessible by scrolling. Enter saved fixture-local identity and the default branch; Reset restored edited values. Restored Midnight, Comfortable density, and `main` for new repositories. |
| Working changes | New, modified, deleted, and renamed entries had distinct status presentation. Staged and unstaged README previews showed their different content and target badges. Stage all, Unstage all, and single-file staging worked; the latter named the affected path in feedback. The compact composer fitted at narrow width, and empty repositories displayed useful clean-state guidance. |
| Commit and preservation | Native commit `1eef17d` included the staged README introduction, new file, and rename, cleared the message, and preserved the unstaged README draft, stylesheet edit, and deletion. OIDs, index contents, and remaining changes were independently checked. |
| Local remote actions | Native Push placed `1eef17d` in a local bare remote. After a fixture peer created `5cd88b3`, Fetch displayed one commit behind and fast-forward Pull advanced HEAD to that exact OID while preserving unstaged work. |
| Branch actions | A fixture with fifty additional branches displayed the current branch and bounded alternatives. Find focused the branch field; filtering and switching to `review/option-49` worked and cleared the filter. Created/switched to `design/native-refinement`, then returned to `main`; remote targets followed the selection. |
| Projects | No-match searches displayed a clear recovery action. Native review found that clearing search did not immediately rebuild recent results; final `76f7d9a` fixed this and both clear controls restored the list. Clone errors appeared inline; canceling the native destination picker retained form input. A complete local clone succeeded, and Create produced an empty repository on the configured `trunk` branch. |
| Comparison and navigation | Diff, Before, and After showed the expected content. Settings and Escape retained comparison context. An added image had an absent Before side and a checkerboard matching Daylight. On a long multi-hunk patch, both code-area and gutter scrolling remained aligned; Back restored the selected commit and file. |
| History columns | Hiding References removed its contents while the other headings and cells remained aligned. References was restored after the check. The large repository retained the selected commit and its 1,429-file inspector during navigation. |

The final app passively inspected `world-of-claudecraft` with 500 commits loaded and 649 local branches. No Git write or network action was performed in that repository. These checks validate macOS interaction and local-remote workflows; they do not validate remote authentication, Linux, or notarized distribution. Earlier image zoom/pan and literal patch-copy checks remain tied to their builds below.

### Refinement release timing

The final package measured twenty History selections and twenty text-file selections, each ten Down followed by ten Up from commit `a461924`. Initial selection and file activation were excluded. History traversal produced zero file-preview frames.

| Interaction | Samples | Median | p95 | Maximum |
| --- | ---: | ---: | ---: | ---: |
| Commit to changed-file frame | 20 | 22.795 ms | 37.439 ms | 54.086 ms |
| File to prepared text-preview frame | 20 | 4.517 ms | 7.402 ms | 18.539 ms |

Each input was followed by a native accessibility observation. The fresh process had inspected an empty fixture and Projects before opening the large repository once. Filesystem caches were not flushed; return file selections could use the immutable preview cache. Other desktop applications remained running, with no Cargo build or test during timing. These handler-to-GPUI-frame measurements exclude pre-handler input delivery, OS presentation, and completed GPU work. They are a small uncontrolled sample, not an end-to-end speedup claim or latency guarantee. [Raw samples and conditions](benchmarks/2026-09-08-native-refinement.json) are retained.

The graph now shares worker-prepared edge arrays when visible rows are cloned. An optimized in-memory harness measured sixty-row clone medians of 1.161 → 0.120 µs for a linear fixture and 1.692 → 0.121 µs for a wide fixture. The tradeoff was a one-time worker layout increase: 1.196 → 1.616 ms for 20,000 linear commits and 4.018 → 4.288 ms for 8,001 commits with 21 lanes. Each case used 100 samples after warmup. The harness excludes Git and GPUI, and desktop load was uncontrolled; it supports the narrower row-clone improvement, not an application-wide speedup. [Raw graph results](benchmarks/2026-09-08-shared-graph-edges.json) and [the reproduction script](../scripts/bench-graph-layout.py) are retained.

## Selected icon package — September 8, 2026

At `9a2ae28`, the user selected the first generated turtle. `assets/app-icon.png` preserves that output byte-for-byte; [its record](../assets/app-icon.prompt.json) retains the original built-in ImageGen prompt and SHA-256. The RGBA source is 1254 × 1254 pixels. The existing `render_icon` example generated all ten iconset representations from 16 to 1024 pixels, and `iconutil` rebuilt the ICNS. The source and 32-pixel rendering were visually inspected.

No Rust source or dependencies changed from the tested `ca8d805` build. The existing release executable and previous package had matching UUID `567C6E63-F6C9-36D2-AF6F-6CF34C028A53`; the app was closed before repackaging with `--no-build`. The new bundle passed plist and ad-hoc signature verification, retained that UUID, and contained an ICNS byte-identical to the selected asset. The packaged app launched successfully. This validates the resource update; it does not rerun or replace the native workflow and performance evidence below.

## Design and interaction validation

Source revision `ca8d805` passed formatting, **115 tests: 69 app, 34 core, and 12 preview**, strict workspace Clippy, and a release build. The arm64 macOS package was ad-hoc signed and verified; executable UUID `567C6E63-F6C9-36D2-AF6F-6CF34C028A53` matched the release executable. Native checks exercised the design build leading to `b8b5799`, that packaged revision, and final `ca8d805` on Apple M4 Max with macOS 26.6.2. Only disposable fixtures and local remotes were mutated.

| Area | Observed behavior |
| --- | --- |
| Actions and branches | Fetch, Pull, and Push remained visible with Targets expanded by default. Created/switched to `design/native-check` and `design/final-check`, then returned to `main`. Successful branch creation cleared the input filter and restored other branch choices. The final build displayed the explicit target in Fetch feedback. |
| Staging and commit | A partially staged README showed different staged and unstaged previews. Commit `ae4ceb7` included the staged introduction, cleared its message, and retained the unstaged draft and two new files. Stage all and Unstage all moved all three files between groups; disabled commit prompts explained the next required action. |
| Local remote actions | Push placed the fixture commit in a local bare remote. A peer pushed `5fb9340`; Fetch showed one commit behind, and Pull advanced HEAD to that exact commit while preserving the unstaged work. OIDs and file/index contents were checked outside the UI. |
| Themes and hierarchy | Inspected Midnight, Daylight, Graphite, Tokyo Night, Catppuccin Mocha, and Nord in the real comparison view. Primary buttons used each theme's accent and readable foreground. Settings previews, file status colors/icons, commit details, and Comfortable/Compact spacing were exercised. |
| Columns and scrolling | Resized Graph, narrowed the window, and dragged the horizontal scrollbar to Author, Date, and SHA; headings and cells remained aligned. Vertical history scrolling retained the selected commit's inspector. Restored default columns, Comfortable density, and Midnight. |
| Projects and errors | Filtered recent projects and opened a result. Incomplete clone submission showed its nearby destination error. Opened and canceled the native folder picker without losing the form. |
| Comparison and focus | Settings and Escape restored the same comparison context. Typing did not edit a read-only patch; copying and pasting into a disposable draft preserved literal patch text without gutter numbers. Back retained the commit and selected file. |
| Icon and package | Inspected the new turtle/branch icon at small size. The production PNG has a real alpha channel; ICNS includes 16–1024 px representations. Large branding files are excluded from the application's embedded UI asset set. |

The final application also passively inspected `world-of-claudecraft` with 500 commits loaded and 649 local branches. Its branch menu showed the current branch, bounded alternatives, and Find/Create without building the entire branch list into the menu. No Git write or network action was performed in that repository. Remote authentication, Linux, and notarization remain outside this evidence; image comparison checks from earlier builds are recorded separately below.

### Design release timing

The final packaged release measured twenty History selections and twenty text-file selections, each ten Down followed by ten Up from commit `8cf37f7`. Initial activation was excluded. History traversal produced zero file-preview frames.

| Interaction | Samples | Median | p95 | Maximum |
| --- | ---: | ---: | ---: | ---: |
| Commit to changed-file frame | 20 | 24.052 ms | 28.125 ms | 29.046 ms |
| File to prepared text-preview frame | 20 | 4.336 ms | 7.438 ms | 7.523 ms |

Each input was followed by a native accessibility observation. Filesystem caches were not flushed, the process had already inspected fixtures, and return traversal could use the preview cache. Other desktop applications remained running. These handler-to-GPUI-frame measurements exclude pre-handler input delivery, OS presentation, and completed GPU work. They are a small uncontrolled sample, not an application speedup claim or latency guarantee. [Raw samples and conditions](benchmarks/2026-09-07-design-native.json) are retained.

A separate optimized Rust graph-layout harness measured a 20,000-commit linear fixture at 2.836 ms before and 1.203 ms after (median), and an 8,001-commit, 21-lane fixture at 4.807 ms before and 4.047 ms after. It used 20 warmups and 100 samples per case, reused in-memory inputs, and excluded Git and GPUI. The updated layout includes cancellation checkpoints. Development CPU load was uncontrolled; [the graph benchmark](benchmarks/2026-09-07-graph-layout.json) records raw values, tail latency, source hashes, and [reproduction instructions](../scripts/bench-graph-layout.py).

## Everyday Git workflow validation

Final source revision `55e7f14` passed formatting, **113 tests: 67 app, 34 core, and 12 preview**, strict workspace Clippy, and a release build. Native checks exercised packaged revisions `ba0ccb7`, `ceea5d9`, `2742212`, and the final `55e7f14` on Apple M4 Max with 128 GB memory and macOS 26.6.2. Only generated `.local` fixtures and local remotes were mutated during development validation.

The final arm64 macOS package was ad-hoc signed and verified. Its executable UUID, `984A3FEA-07EF-3446-93E4-7CE0412C5C87`, matched `target/release/gitturtle`.

| Area | Observed behavior |
| --- | --- |
| History columns | Resized References from 140 to 219 px, then reset the layout; hid Author; set Graph to 415 px and confirmed that width after restarting. Dragging the horizontal scrollbar revealed SHA while keeping header and rows aligned. |
| Appearance and settings | Exercised Daylight, Graphite, and Midnight, plus Compact and Comfortable density. Saved `trunk` as the default branch and used it for a new project. Repository identity edits updated repository-local configuration. |
| Project opening and drafts | Opened a repository through its nested `src` folder using the native picker and retained the existing commit draft. At this build, draft retention applied within the running session. |
| Create and first commit | Created an unborn repository on `trunk` and made its initial commit `cc69432`. |
| Staging and comparisons | Staged and unstaged the same file and verified that selecting its two groups showed the different staged and unstaged content. Image checks exercised Before/After, Fit, 200% zoom, and dragging. |
| Commit and push | Created commit `b54e318` on `main`, pushed to a local bare remote, and verified that the remote had the same OID. |
| Fetch and pull | A fixture peer created `0d6cbaa`; Fetch showed the local branch one commit behind, and fast-forward Pull updated HEAD to the peer commit. |
| Branch actions | Created/switched to `qa/native-workflow`, then switched to `main`; the remote-branch input tracked the selected local branch. |
| Clone and failures | Refused a nonempty destination. A missing-LFS smudge failure surfaced the normal Git error and preserved the partial destination. After the fixture peer removed the intentionally missing pointer, a complete clone into `native-clone-ready` succeeded. |
| Final Settings focus check | From a working-file preview, Command-, opened Settings. Switching to Graphite and pressing Escape restored the same README unified comparison and file-list focus; the editor remained read-only. |
| Final commit feedback | Staged and committed `61828ae` with the message “Verify final native workflow.” The result displayed the short OID and summary on one line, cleared the commit message, and showed a clean working state. |

Pull was fast-forward-only and Push did not force-update refs. These network-action checks used local remotes; remote authentication was not validated. Commit drafts were session-only in `55e7f14`. Linux interaction/builds, notarization, and remote credential flows remain outside this evidence.

The final application was also used for passive inspection of `world-of-claudecraft` with 500 commits loaded. Code-area scrolling kept the unified patch and old/new gutter aligned; Back retained the commit, selected file, and search query. Activating an image followed by Escape stayed in History. The final preferences were returned to Midnight, Comfortable density, default columns, and `main` for new repositories.

### Final release timing check

Release `55e7f14` used the hardware and package above. Twenty History selections (ten Down, ten Up from `69ffdab`) produced zero file-preview frames. A separate forty-selection code traversal in `1e11554` started at `headless/gathering_goal_protocol.ts`, moved twenty files down to `src/sim/professions/material_goal_projection.ts`, and returned. Each action was followed by a native accessibility observation.

| Interaction | Samples | Median | p95 | Maximum |
| --- | ---: | ---: | ---: | ---: |
| Commit to changed-file frame | 20 | 23.671 ms | 26.119 ms | 48.195 ms |
| File to prepared text-preview frame | 40 | 5.508 ms | 7.646 ms | 8.057 ms |

The initial selection and file activation were excluded. Filesystem caches were not flushed; the process had already inspected disposable fixtures, and return selections can use the 32-entry preview cache. Other desktop applications and development work remained running. These are application-handler-to-frame-callback measurements, excluding pre-handler input delivery, OS presentation, and completed GPU work. The small uncontrolled sample is not a speedup claim or latency guarantee. Raw values and conditions are in [the everyday-workflow timing record](benchmarks/2026-09-07-everyday-workflow.json). Historical measurements below remain tied to their original builds.

## Historical automated and build checks

The earlier history/comparison workspace run passed **73 tests**, strict workspace Clippy, and a release build. Tests used disposable repositories for operations that created Git objects, refs, or worktrees.

```sh
cargo fmt --all -- --check
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo build --release --locked -p gitturtle
```

Coverage includes repository roots and merge parents, branches and linked worktrees, unusual path bytes, binary/text/mode/type changes, SHA-256 repositories, missing partial-clone objects, local LFS integrity and symlink rejection, Git process deadlines, and repository-file snapshots with hostile configured helpers. Preview tests cover decoding limits, alpha handling, SVG resource rejection, and LFS pointer recognition. App tests exercise queue replacement, stale-work cancellation, cache identity and accounting, BGRA conversion, local LFS arrival, refreshed branch/worktree tips, and graph budgets. New coverage includes branch folder expansion/filtering, retained repository sessions, and old/new line numbering for unified patches, including header-like source text, accumulated gutter-wheel movement, worker-prepared patch metadata, and its cache allocation budget.

A local Apple Silicon `.app` can be built with [the packaging script](../scripts/package-macos.sh). It is signed ad-hoc for local use, not notarized. The [current package procedure and evidence boundary](benchmarks/ci-packages/macos-package-source.md) describe explicit arm64 target output, verified `--no-build` inputs, detached build identity, optional ZIPs and existing-output recovery. The new source fixtures do not update the historical Mac package passes; exact-candidate macOS execution remains open.

## Historical column layout update

The updated release has a full-height history table and persistent right-hand commit/file inspector. Explicit file activation opens a full-height comparison; Back returns to retained history. Local and remote references are grouped into branch folders. Unified patches now have a separately painted old/new line-number gutter, preserving the literal editor text.

The final run for this historical layout passed 73 tests (44 app, 17 core, 12 preview), strict workspace Clippy, and release packaging. Native checks resumed after the Mac was unlocked. On the demonstration repository, the full-height history/comparison layout, separately aligned old/new gutter, read-only typing, literal patch copying, keyboard activation, and Back navigation were exercised. The right inspector retained its dragged width across mode changes. Branch-folder expansion, temporary search expansion, branch scoping, merge-parent changes in both modes, added/modified/deleted images, and missing-LFS messages behaved as expected. Opening a non-repository folder cleared previous navigation and content and displayed the error.

The final scrolling pass verified linked drag-to-pan on both axes at 200%, reversal to the origin, and dragging across the preview toolbar. The CUA horizontal wheel gesture emitted a zero x/y delta during diagnostic tracing despite a positive horizontal scroll range, so horizontal trackpad behavior remains unverified by that tool. Vertical wheel panning was verified.

On `world-of-claudecraft`, both code-area and gutter wheel scrolling kept a 114-line patch aligned. Back restored the same history viewport (first visible `8b337c1`, selected `1e11554`), file selection, and inspector. Worktree filtering and opening the `feature/freeholds` worktree succeeded; the navigator showed 130 worktrees. An immediate image-open-and-Escape action remained in History after the preview completed. Earlier native checks below apply to the previous layout and are retained as historical evidence.

At `3053ac9`, patch gutter rows, width, and decoration ranges moved to the repository worker, with their retained allocations counted in the preview cache. The packaged release was checked again: gutter/code scrolling remained aligned, Before was empty for an added file, After displayed syntax-highlighted source, and Back retained the selected commit and file.

## Historical initial native macOS checks (before column layout)

The application was opened against the locally available `world-of-claudecraft` repository containing **5,577 branches and 130 worktrees**, and against a generated demonstration repository.

| Area | Exercised behavior |
| --- | --- |
| Repository navigation | Large branch/worktree lists, local/remote branches, native folder picker, invalid-repository errors, and draggable history/sidebar dividers |
| Commit inspection | Changed-file selection, explicit merge-parent comparisons, locked-worktree display, and selected-commit preservation while expanding history |
| Text | Unified patch and Before/After views; typing did not edit the preview; text selection and copying worked |
| Images | Modified, added, and deleted PNGs; transparent pixels; linked zoom and vertical panning |
| Unavailable content | Binary file information and missing local LFS image messages |

The earlier horizontal-wheel check was inconclusive. The column-layout update above adds and verifies two-axis mouse dragging; horizontal trackpad gestures remain unverified.

These are manual checks of the initial native build, not an exhaustive platform or accessibility certification. The [design document](../DESIGN.md) includes intended behavior beyond the implemented surface.

To create a fresh disposable demonstration repository:

```sh
python3 scripts/create-demo-repo.py
cargo run --release --locked -p gitturtle -- .local/demo-repository
```

The generator also creates a linked worktree. It refuses nonempty destinations, including existing repositories. For another run, choose a fresh destination with `--output /path/to/empty-or-new-directory`; do not point it at a working repository.

## Timing methodology

Initial backend observations, hardware/toolchain details, and the reproducible inspection command are recorded in the [Git service benchmark notes](../crates/git-core/README.md#initial-measurement-september-7-2026). The backend harness measures Git service work. It excludes UI dispatch, queuing, image decode, editor preparation, rendering, and presentation. Filesystem caches were not flushed, so those observations are not cold-disk results.

The later [everyday backend report](benchmarks/2026-09-08-everyday-workflows.md) provides p50/p95/maximum values from two warmups and twenty measured calls per eligible series, with [reproduction instructions](benchmarks/everyday-bench.md). It records current costs without a baseline or speed-improvement claim. The rich fixture's two-page file-history sample retains a continuation; the project staged-preview series is skipped because it had no staged changes. Neither result may be represented as zero latency or exhaustive work that was not performed.

The native app has a separate optional trace:

```sh
GITTURTLE_TRACE=1 target/release/gitturtle /path/to/repository
```

`gitturtle.commit_files_frame_ms` now measures a History selection through its changed-file list frame. `gitturtle.file_preview_frame_ms` measures an explicit file activation through its prepared comparison frame. History selection does not eagerly prepare a file preview. Returning to History uses retained state without a new Git request. Both metrics start in the application handler and use a generation- and mode-checked GPUI callback; they exclude input delivery before the handler, OS presentation, and completed GPU execution. Superseded interactions emit no sample.

`gitturtle.working_preview_frame_ms` measures working-file activation through the prepared-preview callback. It excludes the preceding status refresh or Git write. Working measurements must identify the staged/unstaged area and stable HEAD/index/worktree content; do not combine them with immutable-history preview samples or backend-only timings.

The earlier `gitturtle.selection_frame_ms` trace, used for the historical measurements below, starts in the application selection handler. For a commit selection, it includes reading the changed-file list and preparing the chosen file preview; a direct file selection starts at that file's handler. The value is emitted at a GPUI frame-completion callback after the current preview is prepared, with a generation check to suppress superseded results. It does not measure input delivery before the handler, OS display presentation, or completed GPU execution. Interactions without a completed preview do not produce this sample.

The status bar's **content read** duration covers worker processing, including a content-cache lookup on a hit. It excludes time waiting for the worker and subsequent editor construction or frame work. Comparing this value directly with another client's click-to-visible delay would be misleading.

Native trace samples should be reported with the build profile, repository, selected content, sample count, system load, and cache conditions. No frame-latency or memory guarantee is established by the current checks.

## Historical column-layout native measurements

On Apple M4 Max / macOS 26.6.2, release `9b47e08` opened `world-of-claudecraft` with 500 commits loaded. Twenty Down actions followed by twenty Up actions produced 40 changed-file-list frames and **zero file-preview frames** during History navigation. A separate 40-selection text/code traversal inspected commit `1e11554`.

| Build and interaction | Samples | Median | 95th percentile | Maximum |
| --- | ---: | ---: | ---: | ---: |
| `9b47e08`: commit to changed-file list | 40 | 29.913 ms | 46.964 ms | 64.041 ms |
| `9b47e08`: file to prepared text preview | 40 | 7.690 ms | 10.497 ms | 10.736 ms |
| `3053ac9`: file to prepared text preview | 40 | 5.523 ms | 7.517 ms | 7.722 ms |

The `3053ac9` confirmation used a fresh application process and the same immutable commit and file traversal after moving patch metadata preparation to the worker. Its initial file-open frame was excluded from traversal statistics. Both file runs started and ended on `headless/gathering_goal_protocol.ts`, traversing through `src/sim/professions/material_goal_projection.ts` before returning. Each action was followed by a native accessibility observation. Return selections can hit the 32-entry content cache; editors are constructed lazily.

Filesystem caches were not flushed, and other desktop applications and development activity remained running. These are small local measurements with different process/cache conditions, not a controlled comparison or proof of an improvement. The callbacks do not measure completed display presentation. Neither file run measures image decoding, and no latency or memory guarantee follows from these samples.

RSS after outbound/return traversal was approximately 129.9/130.0 MiB for History and 140.3/140.6 MiB for text comparisons at `9b47e08`; the final text run at `3053ac9` recorded 133.1/128.1 MiB. These are process snapshots, excluding separate GPU accounting. Raw samples, initial-frame exclusions, cache conditions, and boundaries are saved in [the column-layout record](benchmarks/2026-09-07-columns.json) and [the final prepared-diff record](benchmarks/2026-09-07-prepared-diffs.json).

## Historical initial optimized native measurements (before column layout)

On Apple M4 Max / macOS 26.6.2, the release build at `8b0799e` opened `world-of-claudecraft` with 500 loaded commits. We sent 25 Down actions and then 25 Up actions, observing the native accessibility state after each. The OS filesystem cache was not flushed; other desktop applications and development activity remained running. This is a small first measurement, not a comparative benchmark or latency guarantee.

| Completed preview samples | Count | Median | 95th percentile | Maximum |
| --- | ---: | ---: | ---: | ---: |
| Navigation, excluding initial selection | 49 | 21.882 ms | 29.838 ms | 308.598 ms |
| Return traversal (subset above) | 25 | 21.910 ms | 27.176 ms | 27.216 ms |

The initial selection callback took 72.441 ms; that excludes repository discovery and history loading and is not application startup time. Fifty navigation actions emitted 49 completed-preview samples; absent samples are not counted as zero latency. The 308.598 ms outlier is retained, and this trace does not isolate its cause. The raw samples, method, and limits are in [the measurement record](benchmarks/2026-09-07-native.json).

Process RSS snapshots were about 142.3 MiB after the outbound traversal and 143.4 MiB after the return. These snapshots exclude GPU memory accounting and do not establish a memory-growth guarantee. The locally packaged app occupies about 30 MiB on disk.

## Current limits and follow-up

- Ordinary history streams an immutable captured traversal in500-commit pages and retains a5,000-row/64MiB window, plus one selected inspection. Older continues forward; Previous replays and discards bounded earlier pages; Latest captures current local tips and returns to row zero while retaining the selected inspector. Refresh resolves current refs again. Repository-wide search is separate: it pins local tips or selected ancestry, supports cancellation and explicit continuation, and retains up to 10,000 matches or 64 MiB of metadata. Scan, byte, and time stops do not establish exhaustion.
- Local filesystem and regained-focus events request coalesced read-only refreshes; manual Refresh remains available. Active writes and foreground reads take priority, and failed watchers report a recovery action. No refresh fetches objects. Fetch, fast-forward Pull, and non-force Push require explicit actions; the dated native network checks used local remotes, not a hosting provider's credential flow.
- Supported text changes allow hunk and changed-line staging/unstaging. Binary, oversized, filtered/normalized, renamed, and mode/type-changing files use whole-file actions; ambiguous missing-final-newline selections require a complete replacement or hunk. Commit Title/Description drafts are persisted per worktree, subject to the bounded preference file.
- Unified and aligned split diffs coexist with Before/After source tabs. Text previews and manual conflict editors are bounded to 2 MiB and 100,000 lines per side. Parent controls expose the first 128 parents of unusually large merge commits with an explicit count notice. Rename detection uses a 1,000-candidate limit.
- File history follows first-parent lineage with exact revision paths, rather than every ancestry route through a merge. UI pages contain 100 rows; the core replays the bounded rename-following prefix, which can reach its 32 MiB or 15-second limit on deep pages and require an older anchor.
- Merge/rebase and conflict resolution support deliberate Continue, Abort, and Keep files actions. Native interactive rebase reviews up to 100 linear commits, with reword/squash message editing and interruption recovery. Root/merge-preserving rewrites, apply-backend continuation, non-UTF-8 messages and ambiguous external reword checkpoints require Git's configured tools. Abort refuses independent work it cannot safely preserve; rebase has a stricter dirty-work guard. A separate [rewritten-series review](rewritten-series.md) can publish one explicitly reviewed branch with an exact expected-OID lease; ordinary Push remains non-force. Submodule management remains outside the UI. See [rebase semantics](interactive-rebase.md).
- Conflict parsing supports at most 4,096 blocks within the 2 MiB/100,000-line text bound, preserving surrounding bytes. Save draft and Save and stage are separate actions. Conflict and rebase-message drafts persist atomically outside repositories, keyed by canonical worktree and exact source identities. Saving/Saved/error states distinguish pending persistence; stale drafts remain copyable and are never restored blindly. The separate recovery store holds at most 256 entries / 64 MiB of text and retains completed/stale drafts until explicit discard. [Block semantics](conflict-blocks.md) documents source validation and fallbacks.
- Stash restore keeps saved work until a separate Drop; prepared recovery actions refuse stale targets. Undo requires a named local tip with one parent and refuses known remote-tracking containment. Local refs cannot establish whether a commit is published on an unfetched remote. See [core recovery semantics and bounds](../crates/git-core/README.md#explicit-everyday-operations).
- Static images use a preview capped at a 1,600-pixel edge. GIF comparison supports explicit playback and frame stepping with bounded decoded frames at an 800-pixel edge; secondary image inspectors retain the first frame. JPEG 2000 uses macOS ImageIO, with a codec explanation on Linux. Side-by-side, Overlay and Wipe share a source coordinate system; zoom percentages use a bounded comparison scale and preserve original dimension differences. Source/decoded dimensions and the scale relationship are shown. See the [file-preview support matrix](file-previews.md) for per-format limits; SVG filters and embedded/external images remain unsupported.
- Missing LFS previews offer an explicit one-object download capped at 32 MiB, requiring Git LFS and a configured source. Size/SHA-256 and existing decoder bounds apply; no checkout, smudge or index/worktree rewrite is used for display. [Local LFS fixtures](lfs-previews.md#local-evidence) do not establish hosted transport coverage.
- Incremental graph preparation preserves a bounded frontier across pages, with at most128 simultaneous lanes and200,000 parent/edge budget entries as defined by the worker. Above the budget, the UI explains why connections are hidden and shows isolated nodes instead of incomplete ancestry lines.
- The preview cache is limited to 32 entries and 128 MiB of retained CPU content allocations. UI-held references and GPU resources have separate lifetimes. Search and file history can terminate active Git processes; other reads/decodes retain their individual bounds and cancellation checkpoints. Input/allocation limits are not a process sandbox or a hard end-to-end deadline.
- Blame and line history have bounded text/output/lineage, explicit uncommitted and shallow-boundary states, and cancellable Git reads. Tag actions preserve configured signing and captured local/remote identities; ignore actions preserve destination content, tracked files and unrelated index/worktree data. Their current semantics do not retroactively expand the historical native evidence.
- Available Linux/aarch64 checks and candidate failures are recorded by exact snapshot in the [current Linux evidence](benchmarks/native-polish-20260910/validation.json). Earlier source `92ea02c` passed457 unique tests as recorded in its [environment evidence](benchmarks/2026-09-09-milestone-environment.md#image-lifetime-correction-linux-validation). Linux native-window interaction, hosted CI execution, live-provider sign-in, real Keychain unlock and hardware-backed signing remain unverified here. A quality workflow is authored; local .app packaging is for development verification. Distribution, installers, notarization and publishing are outside this milestone.
