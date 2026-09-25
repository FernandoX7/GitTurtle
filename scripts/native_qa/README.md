# Native-QA tooling

Drives the real GitTurtle window on Linux under XWayland for the [native-QA skill](../../.agents/skills/gitturtle-native-qa/SKILL.md). One entry point, run from the repository root:

```sh
python3 scripts/native_qa/qa.py <command> --help
```

Python 3.11 or newer. `launch` needs Pillow and python-xlib, `compare` and `privacy` need Pillow, and the portal helpers need PyGObject (AT-SPI) and dbus-python. They are imported only by the commands that use them. Exit status: 0 clean, 1 a finding, 2 a refusal, usage error or inconclusive check.

| Command | What it does |
| --- | --- |
| `display-check [--display :1] [--allow-pid N]` | Prints `date -u`, anchored `pgrep -af` matches for GitTurtle executables and QA drivers (this process tree excluded), and every GitTurtle window with its `_NET_WM_PID`. Exits 1 if anything not allowed is running. Run it before taking the display and when handing it back. |
| `identity BINARY [CANDIDATE]` | sha256 plus `--build-info`, run with no display and a throwaway HOME. Flags a `source_tree` other than `clean`; a pair with the same sha256 or `source_revision` is refused (a shared `CARGO_TARGET_DIR` once made a candidate build a no-op). |
| `launch --binary B --fixture F --run-dir /abs/empty ...` | One isolated launch, described below. |
| `compare BASE CANDIDATE [--mask status-timing] [--mask x0,y0,x1,y1]` | Two frames or two directories of frames. Reports the differing pixel count outside the masks, the pixels masked, and the regions. Masked pixels are still counted, so a mask never hides that something changed. `status-timing` covers the per-launch timing that History and Changes print in the left-aligned status-bar message, at scale 1 (`96,H-18,208,H-7`); another message or scale needs an explicit rectangle. |
| `privacy scan FRAME... --templates DIR` | Template matching (zero-mean normalized cross-correlation, either polarity) of personal strings in PNG or JPEG frames. Exits 1 on any match at or above 0.80. |
| `privacy crop FRAME x0,y0,x1,y1 OUT.png` | Cuts a template from a frame that shows a personal string. |

## Launch

Each launch gets an absolute, absent or empty `--run-dir` holding `home/`, `config/`, `data/`, `cache/`, `state/` and `captures/`. HOME and every `XDG_*_HOME` point there. A relative path, a non-empty directory or the operator's own HOME or XDG directory is refused before anything is created. `home/.gitconfig` sets the identity `GitTurtle QA <qa@example.invalid>`, and `config/gitturtle/preferences.json` is written just before the launch. It is either generated (`--theme`, default `midnight`, Follow system off, and repeatable `--project PATH[=NAME]`) or copied byte for byte from `--preferences FILE`, which is how a store with custom themes is supplied. `WAYLAND_DISPLAY` and inherited `GIT_*` variables are removed. `DISPLAY` defaults to `:1` and `GPUI_X11_SCALE_FACTOR` to 1.

The window is found only by the launched process's `_NET_WM_PID` and resized with `--size` (default `1000x680`, the app's minimum). The app is stopped with SIGTERM to that PID alone. The tool never kills by name and never sends SIGKILL; a process that outlives SIGTERM is reported and left running. `flow-log.json` records the binary identity, store digest, environment, every input event, each capture's sha256, and the fixture's HEAD, status and index digest before and after the launch.

`--for-commit` refuses a fixture outside `/tmp/gitturtle-evidence/` and a run directory under `/home`, `/Users` or the operator's home. Without it, the tool only warns. Use `/tmp/gitturtle-evidence/runs/<name>` for frames meant for the repository, and run `privacy scan` on them before committing.

`--scenario FILE` is a JSON list of steps, each with exactly one action: `key` (with `mods`, `wait_after`), `type`, `move`, `glide` (a two-step approach, `settle`), `click`, `press`/`release` (button number), `wheel` (`[x, y]` with `steps`), `park`, `wait`, `stable` (seconds to wait for two identical grabs), `resize` and `capture` (a name, `what`). Coordinates are window-relative. A capture parks the pointer off the window first unless it sets `"keep_pointer": true`, which a hover or pressed frame needs. Without a scenario the launch settles and captures `00-launch`. Add `note` to any step to explain it in the log.

```json
[{"key": "comma", "mods": ["Control_L"], "wait_after": 1.8, "note": "open Settings"},
 {"wheel": [60, 400], "steps": 18}, {"stable": 4.0}, {"capture": "rest"},
 {"glide": [772, 410], "settle": 1.2}, {"capture": "button-hover", "keep_pointer": true}]
```

Drivers that need more than steps import the library from `scripts/`: `session.Session` for the lifecycle, `x11.Driver` for input and grabs, `frames` for comparison and pixel measurements (`modal`, `contrast`, `find_hline`), and `portal` for the GNOME file-chooser dialog. That dialog is a Wayland client that XTest cannot reach, so the tool reads it through AT-SPI, types into it through Mutter RemoteDesktop, records its D-Bus traffic (`launch --monitor-portal`), and can run a private session bus that has no portal.

## Privacy templates

Tracked files never contain the personal strings. Supply templates at run time: crop them from a frame that showed the string (`privacy crop`), or render them with `--text STRING --font FILE [--size N]`, where the font is the one the app draws with. Crops match more reliably. A template directory or crop inside a Git work tree must be ignored there, for example under `.local/`. The default engine is `ncc.c`, compiled on first use into `$XDG_CACHE_HOME/gitturtle-native-qa/`: 33 templates of up to 130x17 px take about 18 s per 1000x680 frame, and one 130x15 template takes 0.23 s against 11 s in pure Python. `--engine python` runs the same rules without a compiler.

## Tests

```sh
python3 -m unittest discover -s scripts/native_qa -t scripts -p 'test_*.py'
```

They use temporary directories and never open a display or launch the app. Tests that need Pillow or a C compiler skip when those are missing.
