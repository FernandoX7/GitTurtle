# Omarchy theme: reread, switch and choice latency, 2026-09-28

This is the release measurement that item D's contract asks for: the reread of the desktop's current Omarchy theme (read, parse, map, fit and readability check) and the latency from a switch to the frame that shows it. It uses the traces defined in [metrics](metrics.md): `gitturtle.omarchy_reread_ms`, `gitturtle.omarchy_apply_frame_ms` and, for choosing the card, `gitturtle.theme_apply_frame_ms`. Raw samples are in [the JSON](2026-09-28-omarchy-theme.json). The [validation entry](../validation.md#september-28-omarchy-theme-on-linux) covers the frames.

## Setup

- Build: `5b7c25c` (release, clean tree, sha256 `cba30b88…`). The earlier base `1a9fc3e` (release, clean, sha256 `0e074362…`) served only as the other window for focus returns. Neither has an Omarchy baseline, because the feature is new.
- Host: AMD 3020e, two cores with one thread each, `schedutil`, 5.7 GiB, Omarchy 4.0.4 on Linux 7.2.5 and Hyprland 0.56.2, native Wayland. Nothing was pinned, because the host has only two cores. The window was fullscreen on a temporary 1400 × 2100 headless output at 60 Hz and scale 1.
- Cache state: warm page cache. Every launch had a fresh HOME and XDG directories. The repository was `scripts/create-demo-repo.py`'s demo under `/tmp/gitturtle-evidence/omarchy-d/`, and its fingerprint was the same before and after every run.
- Omarchy state: every launch had its own `$HOME/.local/state/omarchy/current/`, copied from `/usr/share/omarchy/themes/<name>` and switched with `omarchy-theme-set`'s writes in its order: the next theme inside `current/`, remove `theme/`, move it into place, `theme.name`, then the background link. Only the real-switch run used the desktop's own state and `omarchy theme set`.
- Load: the choice run went from 1.28 to 0.45, and the reread and switch run from 1.03 to 0.76. Nothing else ran at the time.

## Result

| Path | Metric | n | p50 | p95 | max |
| --- | --- | ---: | ---: | ---: | ---: |
| Focus return, theme unchanged | `omarchy_reread_ms` | 32 | 0.291 | 0.710 | 1.061 |
| Focus return, theme unchanged | `omarchy_apply_frame_ms` | 0 | — | — | — |
| Switch, catppuccin-latte ↔ tokyo-night | `omarchy_reread_ms` | 32 | 0.938 | 2.399 | 2.427 |
| Switch, catppuccin-latte ↔ tokyo-night | `omarchy_apply_frame_ms` | 32 | 0.510 | 0.582 | 0.613 |
| Switch through all 22 themes | `omarchy_apply_frame_ms` | 21 | 0.576 | 0.729 | 0.752 |
| Real `omarchy theme set`, five themes | `omarchy_apply_frame_ms` | 5 | 3.307 | 16.542 | 16.542 |
| Choosing the Omarchy card | `theme_apply_frame_ms` | 30 | 55.956 | 66.132 | 72.813 |
| Choosing Midnight, same launch | `theme_apply_frame_ms` | 30 | 0.251 | 1.403 | 2.162 |

- **Reread.** It runs on the background executor, never on the UI thread, and stays under 2.5 ms including the fit. A focus return that finds the same theme applies nothing: 32 returns printed 32 rereads and no application.
- **Switch.** Every switch printed exactly one application; a switch through all 22 themes gave 21 applications for 21 switches. Each application takes about half a millisecond, up to the next frame callback. The real `omarchy theme set` runs took longer, up to 16.5 ms, while Hyprland reloaded and the bar restarted in the same moment; there is one sample per theme.
- **Choosing the card.** By owner decision, choosing the card reads `current/` first and applies what that read finds, falling back to the cached palette after 100 ms ([spec](../development/themes/spec.md#omarchy-theme)). All 30 reads landed inside the wait, so each choice applied once and printed no second application. The metric runs from the key's handler to the frame after that application. Along the way the window repaints Settings once to show the card as chosen, the read runs, and the result returns to the UI thread, so the value is about two window draws on this host rather than the read. Choosing a built-in theme applies inside its handler and stays under a millisecond at p50. The spec's 8 ms median and 16 ms p95 budget for a picker switch therefore does not describe the Omarchy card; its bound is the 100 ms wait the owner chose.
- **After an unseen switch.** When the desktop changed from Tokyo Night to White while another theme was selected and nothing watched, choosing the card printed one reread (0.811 ms) and one `theme_apply_frame_ms` of 46.49 ms, with no stale application first. Before the read-first change, a stale palette applied first, and the right one followed about 47 ms later.

## Compared with the first measurement

The first release run of this procedure, on `3846dc1` before the fix rounds and not kept as a record, gave a focus reread of 0.223/0.302 ms (p50/p95), a switch reread of 0.766/2.784 ms and a switch application of 0.509/0.656 ms. The final build's switch reread is about 0.17 ms slower at p50, consistent with the heavier fit (hover step, pressed-step loop and hue windows). During the last fix round, before its final two changes (which shorten the pressed-step loop), a debug-build timing of mapping the 24 fixtures moved from 0.406 to 0.465 ms at the median. The application time is unchanged.

## Not measured

- Nothing here is a same-session A/B against a base, because no base build has the feature. The figures are a first baseline.
- Mapping cost in isolation was timed only in debug. The reread includes it.
- No cold page cache, and no other refresh rate than the headless output's 60 Hz.
- No X11 or XWayland, no scale other than 1, and only this two-core host.
