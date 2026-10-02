# Native-QA tooling

Drives the real GitTurtle window on Linux under XWayland for the [native-QA skill](../../.agents/skills/gitturtle-native-qa/SKILL.md). One entry point, run from the repository root:

```sh
python3 scripts/native_qa/qa.py <command> --help
```

Python 3.11 or newer. `launch`, `scenario run` and `recheck` need Pillow and python-xlib, plus dbus-python with `--input mutter`; `compare` and `privacy` need Pillow; `scenario check`, `scenario fixture` and `attestation` need only Git and the standard library; and the portal helpers need PyGObject (AT-SPI) and dbus-python. They are imported only by the commands that use them. Exit status: 0 clean, 1 a finding, 2 a refusal, usage error or inconclusive check.

| Command | What it does |
| --- | --- |
| `scenario check SPEC` | Validates a [scenario](#scenarios) strictly, without a display, and lists the crops it commits. |
| `scenario fixture SPEC [--fixtures-dir DIR]` | Builds the scenario's fixture recipe, or reuses an unchanged earlier build of it, without a display. |
| `scenario run SPEC --build base=EXE --build cand=EXE --out BUNDLE` | Every build role x variant through `launch`'s isolation, then the committed crops, their manifest, the analyses and the privacy scan; see [scenarios](#scenarios). |
| `recheck SPEC --exe EXE --committed DIR [--out DIR]` | Re-captures the scenario's candidate crops with `EXE` and compares their bytes with the committed files; exit 0 only when every one is byte-identical. |
| `attestation --bundle B --task T --candidate SHA --base SHA [--recheck R] [--evidence-commit SHA] --out FILE` | Writes the native attestation JSON from a passing bundle and, for a rebuilt candidate, its re-check. |
| `display-check [--display :1] [--allow-pid N]` | Prints `date -u`, anchored `pgrep -af` matches for GitTurtle executables and QA drivers (this process tree excluded), and every GitTurtle window with its `_NET_WM_PID`. Exits 1 if anything not allowed is running. Run it before taking the display and when handing it back. |
| `identity BINARY [CANDIDATE]` | sha256 plus `--build-info`, run with no display and a throwaway HOME. Flags a `source_tree` other than `clean`; a pair with the same sha256 or `source_revision` is refused (a shared `CARGO_TARGET_DIR` once made a candidate build a no-op). |
| `launch --binary B --fixture F --run-dir /abs/empty [--input mutter\|xtest] ...` | One isolated launch, described below. |
| `compare BASE CANDIDATE [--mask status-timing] [--mask x0,y0,x1,y1]` | Two frames or two directories of frames. Reports the differing pixel count outside the masks, the pixels masked, and the regions. Masked pixels are still counted, so a mask never hides that something changed. `status-timing` covers the per-launch timing that History and Changes print in the left-aligned status-bar message, at scale 1 (`96,H-18,208,H-7`); another message or scale needs an explicit rectangle. |
| `privacy scan FRAME... --templates DIR` | Template matching (zero-mean normalized cross-correlation, either polarity) of personal strings in PNG or JPEG frames. Exits 1 on any match at or above 0.80. `--packed FILE` reads a packed set instead, `--jobs N` scans N images at once, and `--redacted` prints only each image's path and `clean` or `MATCH`. Every frame of an animation is scanned; more than 64 frames is refused (exit 2). |
| `privacy crop FRAME x0,y0,x1,y1 OUT.png` | Cuts a template from a frame that shows a personal string. |
| `privacy pack --templates DIR --output FILE` | Packs a template directory into one base64 file (grayscale pixels only, no names) for the CI secret, mode 0600, and prints only the template count and byte size. Refuses a set above 48,000 bytes. |

## Scenarios

A scenario is the evidence for one task as data: a versioned JSON spec that builds its fixture, drives every build through the same steps, crops the frames to commit, measures them and records what each shows. It replaces the per-task drivers, crop scripts and ring analyses that each native-QA run used to write. The spec is committed beside its frames as `docs/evidence/<task>/scenario.json`, so any later build is re-checked by one command: `qa.py recheck docs/evidence/<task>/scenario.json --exe EXE --committed docs/evidence/<task>`. The first is [`docs/evidence/tab-reveals-branch-and-tag-rows/scenario.json`](../../docs/evidence/tab-reveals-branch-and-tag-rows/scenario.json) (sha256 `754e88f7…`), which reproduces that task's committed crops byte for byte: on 2026-10-02 `recheck` of candidate `db6d441` re-captured all 8 candidate crops identical (00:48–00:52 UTC, bundle `/tmp/gitturtle-evidence/runs/tab-reveals-scenario-recheck-db6d441-2`), and `scenario run` of base `ee87282` and candidate `db6d441` produced all 20 committed crops identical, with 36 of 36 analyses as expected and no privacy match (00:52–01:01 UTC, 518 s, bundle `/tmp/gitturtle-evidence/runs/tab-reveals-scenario-run`). Both used the 2026-10-01 fixture through `--fixture`; the spec's recipe builds a repository identical to it in every ref, commit, tree and reflog.

Validation is strict and runs before anything is built or launched: an unknown key, a wrong type, a box outside the window, a reference to an undefined crop, capture or mark, a filter that matches no variant, or two crops with one committed name stops with the JSON path at fault (`$.steps[12].crop: no crop box named 'dialog'`). Top-level keys:

- `version` (1), `task` (the task id, which names `docs/evidence/<task>/`), `summary` and `limitations` (prose the attestation quotes), `window` (`[1000, 680]` by default), and `roles` (`["base", "cand"]` by default; a light-tier spec can name `cand` alone).
- `fixture`: the recipe below; without one, `--fixture PATH` is required.
- `variants`: `{"palettes": [...], "text_sizes": [...]}` for every combination, or a list of `{"palette", "text_size", "id"}`. The palette is a built-in theme key and the text size the interface size (11 to 18 pt), both seeded into each launch's generated store as `launch --theme` does; `settings` adds other store settings and `env` extra variables, as `launch --env` does; neither can set what the run itself records or isolates (`DISPLAY`, `GPUI_X11_SCALE_FACTOR`, HOME, `XDG_*`, `WAYLAND_*`, D-Bus or `GIT_*`).
- `crops`: named boxes, `[x0, y0, x1, y1]` with exclusive ends in window pixels.
- `steps`: one ordered list that every role runs identically. Each step has one action and may add `note` and `when` (`{"palette": [...], "text_size": [...], "variant": [...]}`, so a step runs only for those variants):
  - the `launch --scenario` actions below, with `key` taking `repeat` and `await_change` (seconds to wait for the window to change after each press, logged as its latency), and `stable` taking `quiet`;
  - `palette`: a command-palette entry by its visible name. Ctrl+Shift+P, the name, Return; it stops before typing unless the palette's modal dialog covered the window (at least half its pixels changed, where a caret blink changes a few), and after Return unless the window changed;
  - `mark`: keeps the current frame under a name for later guards (saved under `marks/`, never committed);
  - `guard`: an analysis (below, without `name`) on `@now`, a mark or an earlier capture. If it fails, the step's `on_fail` keys are sent (`[{"key": "Escape"}]`) and the run stops as inconclusive, so a route that went astray never presses Return on the wrong item;
  - `capture`: grabs the window after parking the pointer and waiting up to `stable_within` seconds (8) for two identical grabs `quiet` seconds (1.15) apart. `crop` names its box, `shows` is the one line on what it shows (a string, or one per role), `roles` limits the roles that commit it, and `"commit": false` keeps it as a raw frame for analyses and guards. Every role still grabs every capture, so input stays identical. A committed capture whose window did not settle stops the run as inconclusive, since its frame may not reproduce.
- `analyses`: each with a `name`, a `kind`, its frames and thresholds, and `expect` (true, or `{"base": false, "cand": true}`). A frame is a capture of the analysis's own role, or `base:capture` and `cand:capture`; an analysis whose frames all name a role runs once per variant, the others once per role (`roles` limits them). The kinds, pure functions in `analysis.py`:
  - `ring`: around a control's `rect`, which sides show a continuous ring of `colour` (`"detect"` takes the most saturated accent covering a quarter of the perimeter), its usual width, the outlines in that colour, its outer box and its WCAG contrast against the surface just outside it; `min_contrast`, `min_width`, `max_outlines`, and `uniform_width` (default true, so a ring clipped by 1 px on one side fails);
  - `clearance`: surface pixels from a ring's outer edge (`"ring": "detect"` or a colour) or the rect's own edge to the next non-surface pixel on `side`, along scan lines `at`; `surface` is a colour or `{"at": [x, y]}`; `min_px` or `max_px`;
  - `fill`: the WCAG contrast of a region's dominant fill against a `reference` colour or another region's dominant colour; `min_contrast` or `max_contrast`;
  - `compare`: `frames.compare` of frames `a` and `b` inside a `region` (or a named `crop`) and outside `masks` (named masks or boxes), with the changed row `bands`; `max_pixels` (default 0), `min_pixels` or exact `bands` with `band_min` px per row.

The fixture recipe builds a disposable repository under `/tmp/gitturtle-evidence/fixtures/<name>/` with `operations`: `commit` (a message and `files`: path to text, `{"base64": ...}` or null to delete; `allow_empty`), `branch` and `tag` (`at`; a tag with `message` is annotated), `checkout` (`create`, `at`, `detach`), `merge` (`message`), `reset` (`mode`), `remote` (a name and the local bare repository at `bare`), `push` (`refs`, only to a remote the recipe defined) and `worktree` (`new_branch`, `at`). `@N` names the N-th commit the recipe made. Every operation runs as `GitTurtle QA <qa@example.invalid>` at a fixed date, its own `date` or `clock.start` plus its index times `clock.step`, for author, committer, tagger and reflog alike, with no system or global Git configuration, an empty template (no hooks) and no network. Nothing depends on the current time, so the recipe gives the same object IDs, and `expect` (`{"@0": "<oid>", "HEAD": "<oid>"}`) stops the run if another Git version or an edit would change the IDs a frame shows. A fixture is built once and reused while its recipe and repository state are unchanged; anything else at that path is refused, never deleted.

`scenario run` refuses unless `identity` accepts the pair, the privacy templates exist (`--templates`, else `$GITTURTLE_PRIVACY_TEMPLATES` or `.local/privacy/templates` as the gate finds them) and the output directory is absolute, empty and outside a home root. It builds or reuses the fixture, then launches each variant with each role exactly as `launch --for-commit` does, each in `BUNDLE/<role>/<variant>/`, checking the fixture's HEAD, status, index, refs, reflogs and every linked worktree's HEAD, status and index before and after every launch. The privacy templates are loaded before the first launch. A failed guard, a refused input, a crash or a changed fixture stops the run. It then writes the bundle:

- `scenario.json`, the spec byte for byte; `fixture-manifest.json`, the recipe build;
- `commit/`, the crops under their committed names, `{base|candidate}-{palette}[-{size}pt]-{W}x{H}-{capture}.png`, cut from the raw frames as RGB PNGs, so the same pixels give the same bytes (a raw frame of another size than the window is refused, never padded);
- `commit-manifest.json`: each crop's name, sha256, bytes, size, role, variant, capture, box, source and what it shows;
- `analysis.json`: every analysis's numbers, verdict and expectation;
- `run.json`: the builds, host, input, fixture, each launch and the redacted privacy scan of the crops in `commit/`, with the sha256 each had when scanned.

It exits 0 when every launch passed, every analysis met its expectation and no crop matched a template, 1 on a finding, and 2 on a refusal or an inconclusive run. `recheck` launches only the variants with committed candidate crops, writes `recheck.json` and lists each crop as `identical`, `pixels-identical` (other bytes, same pixels), `different` (with the pixel count and regions), `missing`, or `not-in-spec` (a committed candidate crop the spec does not produce). `attestation` refuses a bundle or re-check that did not pass, bundle files written for another `scenario.json`, a privacy scan that did not cover exactly the manifest's crops, a re-check that did not compare exactly the bundle's candidate crops (names and sha256), and a candidate other than the attested executable's `source_revision`; it takes identity, times, host, input, fixture, frames and verdicts from the files and prose from the spec, or from `--what` and `--limitations`.

## Launch

Each launch gets an absolute, absent or empty `--run-dir` holding `home/`, `config/`, `data/`, `cache/`, `state/` and `captures/`. HOME and every `XDG_*_HOME` point there. A relative path, a non-empty directory or the operator's own HOME or XDG directory is refused before anything is created. `home/.gitconfig` sets the identity `GitTurtle QA <qa@example.invalid>`, and `config/gitturtle/preferences.json` is written just before the launch. It is either generated (`--theme`, default `midnight`, Follow system off, and repeatable `--project PATH[=NAME]`) or copied byte for byte from `--preferences FILE`, which is how a store with custom themes is supplied. `WAYLAND_DISPLAY` and inherited `GIT_*` variables are removed. `DISPLAY` defaults to `:1` and `GPUI_X11_SCALE_FACTOR` to 1.

The window is found only by the launched process's `_NET_WM_PID` and resized with `--size` (default `1000x680`, the app's minimum). The app is stopped with SIGTERM to that PID alone. The tool never kills by name and never sends SIGKILL; a process that outlives SIGTERM is reported and left running. `flow-log.json` records the binary identity, store digest, environment, every input event, each capture's sha256, and the fixture's HEAD, status and index digest before and after the launch.

`--for-commit` refuses a fixture outside `/tmp/gitturtle-evidence/` and a run directory under `/home`, `/Users` or the operator's home. Without it, the tool only warns. Use `/tmp/gitturtle-evidence/runs/<name>` for frames meant for the repository, and run `privacy scan` on the exact files you commit.

`--scenario FILE` is a JSON list of steps, each with exactly one action: `key` (with `mods`, `wait_after`, `repeat`, `await_change`), `type`, `palette`, `move`, `glide` (a two-step approach, `settle`), `click`, `press`/`release` (button number), `wheel` (`[x, y]` with `steps`), `park`, `wait`, `stable` (seconds to wait for two identical grabs, `quiet` apart), `mark`, `resize` and `capture` (a name, `what`, `stable_within`, `quiet`). Coordinates are window-relative. A capture parks the pointer off the window first unless it sets `"keep_pointer": true`, which a hover or pressed frame needs. Without a scenario the launch settles and captures `00-launch`. Add `note` to any step to explain it in the log. A [scenario spec](#scenarios) adds the fixture, variants, guards, crops and analyses around such steps.

```json
[{"key": "comma", "mods": ["Control_L"], "wait_after": 1.8, "note": "open Settings"},
 {"wheel": [60, 400], "steps": 18}, {"stable": 4.0}, {"capture": "rest"},
 {"glide": [772, 410], "settle": 1.2}, {"capture": "button-hover", "keep_pointer": true}]
```

Evidence needs no driver of its own: write a scenario. Code that needs more than a scenario imports the library from `scripts/`: `session.Session` for the lifecycle, `x11.Driver` (XTest) or `mutter.MutterDriver` for input and grabs, `frames` for comparison and pixel measurements (`modal`, `contrast`, `find_hline`), and `portal` for the GNOME file-chooser dialog. That dialog is a Wayland client that XTest cannot reach, so the tool reads it through AT-SPI, types into it through Mutter RemoteDesktop, records its D-Bus traffic (`launch --monitor-portal`), and can run a private session bus that has no portal. `portal.dialog_windows` returns every top-level of `xdg-desktop-portal-gnome` and `-gtk`, and, where Nautilus hosts the FileChooser (AT-SPI application `org.gnome.Nautilus`, as on GNOME 50), only its windows titled exactly `Open File` or `Save File`; no other Nautilus window or application is read.

## Input backends

`launch --input` picks how input reaches the app:

- `xtest` sends XTest events through python-xlib. It is the default on hosts whose XWayland does not run with `-enable-ei-portal`.
- `mutter` sends every key, pointer motion, button and wheel step through `org.gnome.Mutter.RemoteDesktop` on the session bus, which needs no prompt. It is the default when any `Xwayland` command line in `/proc` has `-enable-ei-portal` (the Ubuntu 26.04 GNOME 50 host). There, XTest goes through the RemoteDesktop portal, whose "Allow Remote Interaction" prompt swallows keys, so `--input xtest` is refused (exit 2) before the run directory is created. X is still used for window lookup, focus, geometry and grabs. `flow-log.json` records the backend in its header and launch entry.

Guards in the Mutter backend, since Mutter delivers keys to whatever surface has compositor focus:

- **No input while locked.** Before the launch and before every step that can send input (each press of a repeated key included), `org.gnome.ScreenSaver.GetActive` must report the desktop unlocked; locked or unreadable refuses (exit 2) with nothing sent, because the lock screen's password field would otherwise receive the keys.
- **App keys** go only while X input focus is on the app's window or a descendant. Otherwise the window is activated once and checked again; if focus is still elsewhere, the key is refused (exit 2) and nothing is sent. While typing, focus is checked again before every character.
- **Dialog keys** (`portal.Keyboard`, or `MutterDriver.dialog_keyboard()` on the launch's session) go only while a widget inside the matched dialog reports FOCUSED, searched 40 levels deep because Nautilus nests its widgets, and, with the driver's keyboard, while X focus is off the app. The guard polls for about 6 s, then refuses. GTK4 never reports ACTIVE on the dialog frame, so FOCUSED is the signal.
- **Clicks and wheel steps** go only once XWayland reports the pointer at the target inside the app window.

Known Mutter quirks:

- XWayland learns the pointer position only while the pointer is over an X window, so the driver tracks it: a large relative move anchors it in the bottom-right screen corner, then exact relative moves follow. This assumes monitor scale 1.0, where logical coordinates equal X root coordinates. A position on the app window that XWayland does not confirm is re-anchored once, then the run aborts.
- Mutter drops the first discrete wheel click of each batch, so a `wheel` of N steps sends N + 1 clicks and logs both numbers.

## Privacy templates

Tracked files never contain the personal strings. Supply templates at run time: crop them from a frame that showed the string (`privacy crop`), or render them with `--text STRING --font FILE [--size N]`, where the font is the one the app draws with. Crops match more reliably. A template directory or crop inside a Git work tree must be ignored there, for example under `.local/`. The default engine is `ncc.c`, compiled on first use into `$XDG_CACHE_HOME/gitturtle-native-qa/`; `--engine python` runs the same rules without a compiler, and one 130x15 template takes 0.23 s in C against 11 s in pure Python.

Cost per frame, measured 2026-09-26 on an Intel Core Ultra 9 275HX with the current engine: the local set of 33 templates (about 198,000 template pixels in all, two of them over 200 px on a side) takes 17.6 s of one core per 1000x680 frame (three committed theme frames in 52.9 s), and `--jobs 3` scans the same three in 18.0 s. The cost grows with the frame's and the templates' pixel counts; budget a scan for every frame you commit, not a sample.

Template names can be private too. The interactive command prints them with each score; the automated callers below use `--redacted`, which loads templates by position and prints only the frame and its verdict.

### Automated scans

- **Local gate.** `python3 scripts/gate.py full` scans every image added or changed since the gate's base when a templates directory is configured: `GITTURTLE_PRIVACY_TEMPLATES`, or `.local/privacy/templates` in the checkout (or in the main checkout of a linked worktree). Without one, the stage prints a `warn:` line and the images count as not scanned.
- **CI.** The Quality workflow's `Image privacy` job scans added or changed images with the templates packed into the `NATIVE_QA_PRIVACY_TEMPLATES` repository secret. The secret is optional, and fork pull requests never receive it; those runs, and every run without it, end with a notice that the images were not scanned rather than a pass. [CI validation](../../docs/ci.md#image-privacy-scan) describes the job and its trust boundary.
- **Creating the secret** is the repository owner's step. From the repository root, with the template directory at the gate's default `.local/privacy/templates`:

  ```sh
  python3 scripts/native_qa/qa.py privacy pack --templates .local/privacy/templates --output .local/privacy/templates.b64
  python3 scripts/native_qa/qa.py privacy scan --redacted --packed .local/privacy/templates.b64 docs/evidence/themes/alucard-kanagawa/alucard-history-focus.png
  gh secret set NATIVE_QA_PRIVACY_TEMPLATES < .local/privacy/templates.b64
  rm .local/privacy/templates.b64
  ```

  `pack` refuses a set above 48,000 bytes, under GitHub's 48 KB secret limit, and the scan confirms that the packed set loads. Repack and set the secret again whenever a template changes.

## Tests

```sh
python3 -m unittest discover -s scripts/native_qa -t scripts -p 'test_*.py'
```

They use temporary directories, fake X displays, D-Bus sessions and AT-SPI trees, and never open a display, a bus or the app. Tests that need Pillow or a C compiler skip when those are missing. The controller's tooling profile runs them as its `native-qa-tooling` gate, without the inherited `DISPLAY`, `WAYLAND_DISPLAY`, `WAYLAND_SOCKET` and `DBUS_SESSION_BUS_ADDRESS` and with an empty private `XDG_RUNTIME_DIR` removed afterwards. That withholds the operator's session only: a test that names a display itself, as the launch default `:1` does, could still reach it.
