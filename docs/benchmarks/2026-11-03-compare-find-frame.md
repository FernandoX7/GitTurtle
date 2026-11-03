# A Compare frame with Find open, 2026-11-03

This is the procedure for the release measurement of QA89F-B on the Compare render path. `3ee892ca` (`fix(app): give the find bar's controls icon forms before the field shrinks`) lays the find bar out from its own width in the frame it is drawn: `editor_find::Fitted` measures the controls (`Widths`) and takes a one-line plan from the bar's width (`Widths::plan`: the `Forms` and the plan's `lines`) as it prepaints. Split sizes both sides' bars in one layout pass (`split_diff.rs`), and the unified bar spans the line numbers (`diff_view.rs`). `88bbfa75` (Keyboard Shortcuts' count) landed with it and is not on this path. The question is what a Compare frame with Find open costs before and after: a redraw, a resize step that changes the bar's plan and one that does not, and a key typed into the field.

## Driver

The opt-in test `find_bench::find_bar_frame_release_benchmark` ([`crates/app/src/find_bench.rs`](../../crates/app/src/find_bench.rs)) builds the app's real window on a disposable repository it makes in a temporary directory: two commits of `turtle.txt`, 5,000 numbered lines, then every 37th line rewritten, a line added after every 211th and every 293rd removed, so the unified patch and Split both have changes all through the file. It selects the newest commit's file, which enters Compare, and shows the unified patch, then Split. In each layout it runs every case with Find closed, then with Find open (over the unified patch, or over Split's first side) on the query `turtl`, which every line of both sides matches, at 13 and then 18 pt, in a window 980 px tall:

- `redraw`: the whole window redrawn at 1,480 px (`Window::refresh`).
- `resize_wrap_boundary`: one `simulate_resize` step, alternating between two widths either side of the width at which the bar, open on that query, wraps from one line to two in the candidate, so its plan changes on every step: 600 ↔ 596 px (unified, 13 pt), 823 ↔ 819 (unified, 18 pt), 930 ↔ 925 (Split, 13 pt) and 1,263 ↔ 1,258 (Split, 18 pt). Each series records its widths in `pair`.
- `resize_one_form`: the same between two widths at which the candidate draws the same forms (every control its icon) on the same two lines, so only the field's width changes: 592 ↔ 588, 815 ↔ 811, 920 ↔ 915 and 1,250 ↔ 1,245.

The widths (`STEPS` in the driver) were read from the plan the candidate's bar drew (`editor_find::DRAWN_FORMS`) at every step of the driver's own sequence, at `b9db23b8` on the Asahi host's test text system; the rest of Compare's chrome is the same at all four widths of each layout and size. The repository columns rescale from their sizes before (`ResizableState::fit_container`), so the content column's width at a given window width depends on the widths visited earlier, and Split's sides with it; the boundary widths hold for this sequence, not for any path to them.
- `keystroke`, with Find open: `e` typed into the field at 1,480 px, which searches `turtle` and draws the frame that shows it; it is erased again, untimed, before the next sample.

Each sample is the action and every frame drawn as its effects flush (layout, prepaint and paint on the test platform; no GPU, compositor or display link), until the executor parks, in microseconds (`us`). The frame callbacks the action left are then delivered apart and timed in `next_frame_us`. `frames` and `next_frame_frames` count the frames that laid Compare's preview out in each. Making the fixture, opening Compare and Find, and each series' warm-ups (a tenth of its samples, at least one) are untimed, and every series starts with the window at rest: no frame callback left (a newly shown Split leaves them for a dozen frames at 13 pt as its sides first lay out). Each resize series also records where the code editor starts at each of its two widths (`code_editor_top`), below the bar where Find is open: in the candidate it differs between the two widths of a `resize_wrap_boundary` pair by the bar's added line and is equal for a `resize_one_form` pair, which checks the plan change on the measuring host.

`GITTURTLE_FIND_FRAME_BENCH_OUTPUT` names the JSON file the raw samples are written to, with each series' layout, Find state, text size and case. `GITTURTLE_FIND_FRAME_BENCH_SAMPLES` sets the samples per series, 200 unless set. A debug build refuses more than ten: it only checks that the driver runs.

The driver uses only what the app had before QA89F-B (`8e7998ac`, the head just before it landed), so the base and the candidate run the same test module.

## Builds

- **Candidate:** the branch head with QA89F-B and this driver.
- **Base:** `8e7998ac`'s product code with the driver commit applied on top (`git cherry-pick --no-commit <driver commit>`: the two new files and the `mod find_bench;` line in `crates/app/src/main.rs`).

Build each in its own checkout and target directory, designate the base in writing (commit and executable sha256) before measuring, and confirm each executable embeds its own revision (`strings <exe> | grep <sha>`).

## Commands

From each build's checkout root, with its own target directory:

```sh
cargo test --locked --release -p gitturtle --bin gitturtle --no-run
# copy the printed target/release/deps/gitturtle-<hash> to .local/find-frame-2026-11-03/bench-<build>
```

Then, for each round, alternating base and candidate, pinned to one performance core (cores 4–11 on the Asahi M2 Pro host), with the gate's Git isolation:

```sh
env -u GIT_ASKPASS -u SSH_ASKPASS -u SSH_ASKPASS_REQUIRE -u GIT_CONFIG_PARAMETERS -u GIT_CONFIG_COUNT \
  GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL="$(mktemp)" \
  GITTURTLE_FIND_FRAME_BENCH_OUTPUT="$PWD/.local/find-frame-2026-11-03/<build>-<round>.json" \
  GITTURTLE_FIND_FRAME_BENCH_SAMPLES=200 \
  taskset -c 8 .local/find-frame-2026-11-03/bench-<build> \
  find_bench::find_bar_frame_release_benchmark --exact --ignored --test-threads=1
```

## Limits of the method

This measures the UI thread's handling in a GPUI test window, not a frame of the running app: no renderer encoding, GPU work, compositor or display link is timed, and the test platform draws a dirty window as an update's effects flush rather than on a display tick. The boundary widths are the candidate's on the test platform's text system on the Asahi host; another host's fonts can move them, which `code_editor_top` shows. In the base the bar has no plan, so its two pairs differ only in the widths stepped between.
