# GitTurtle website

A standalone static marketing site for `gitturtle.com`. The deployable directory is `website/public/`. HTML, CSS, and a small optional script provide a manual, keyboard-operable screenshot walkthrough. There is no package manager, build step, native dependency, application server, analytics script, form, or runtime third-party request.

## Preview and validate locally

From the repository root:

```sh
python3 -m http.server 4173 --bind 127.0.0.1 --directory website/public
```

Open `http://127.0.0.1:4173`. Stop with Ctrl+C. Check syntax and local asset/anchor links with:

```sh
node --check website/public/site.js
python3 website/check.py
```

The initial review verified the rendered site at 1440, 1024, 390 and 320 pixels with no horizontal overflow. Left/End/Tab navigation retained the expected keyboard focus; activating Prepare a commit selected the panel and its images loaded. The browser reported no console warnings or errors. Actual reduced-motion emulation, 200% zoom and JavaScript-disabled runtime checks were unavailable in the current browser capability; their fallback behavior was reviewed in source only. See [the website QA record](QA.md) for the executed scope and limits.

### Motion and JavaScript fallback

Source inspection confirms these behaviors; it does not substitute for browser emulation:

- Normal motion is limited to smooth in-page anchor scrolling and 180 ms button/link hover transitions, including a two-pixel hover offset. The walkthrough has no autoplay, timer, scrolling animation, or animated panel transition.
- With `prefers-reduced-motion: reduce`, anchor scrolling becomes immediate, animations and transitions are disabled (including pseudo-elements), and button/link hover translations are removed. The turtle's fixed rotation remains a static composition. The skip link still appears immediately when focused.
- Without JavaScript, the tour buttons remain hidden and all three screenshots, captions, and full-size links appear in document order. Navigation, source-install links and the rest of the page remain ordinary HTML. No panel starts with a `hidden` attribute.
- With JavaScript, the walkthrough initially selects Review changes. The script adds tab/tablist/tabpanel semantics, makes only the selected tab sequentially focusable, and shows its panel. Left/Right wrap and select; Home/End select the first/last tab. Click and native button Enter/Space activation select a view. The selected panel and its image link remain keyboard-accessible. Selection has no network side effects.

`check.py` first rejects hidden files, symlinks and common private or development outputs in the upload directory. It then checks HTML recursively, CSS `url()` assets, same-page and cross-page fragment IDs, duplicate IDs, explicit button types, image alt attributes, provenance hashes, Pages-header syntax, content fingerprints for immutable URLs, and the custom 404's root-relative references and `noindex`. This file guard does not replace reviewing public content for sensitive information. It makes no network requests and does not render CSS, execute JavaScript, test deployed responses or replace accessibility checks. `smoke.py` checks the deployed HTTP/TLS behavior with bounded read-only requests.

The local Python preview server does not interpret Cloudflare's `_headers` file or automatically use the custom `404.html` for missing paths. Both were verified on the published Pages deployment; see [the publication QA record](QA.md#publication-verification).

The production response policies, managed certificate renewal, TLS settings, cache behavior and post-deployment checks are documented in [OPERATIONS.md](OPERATIONS.md).

## Content and release truth

The GitHub Releases API returned an empty array on 2026-09-14. The public [Releases page](https://github.com/FernandoX7/GitTurtle/releases) also reported no releases. Therefore the site offers **source build guides**, not download buttons. Never change a CTA to Download until the target public release asset exists, its platform/architecture and signature status are verified, and installation guidance matches it. No hosted endpoint is queried automatically by the website.

macOS and Linux are described as source previews. Linux's initial build target is Ubuntu 24.04 x86-64, not a promise of distribution-wide support. Local macOS bundles are ad-hoc signed and not notarized. Platform limits and the validation record remain one click away. Site claims come from the current repository README and user guide; no benchmark, endorsement, testimonial or release date has been invented. The performance positioning and 60 GB worktree origin story come from the owner’s experience; 60 GB is not advertised as a capacity limit or a measured cross-client benchmark.

### Screenshot provenance and replacement

Every product picture is a byte-for-byte copy of one of the repository README's pictures under `docs/screenshots/readme/`. They are unaltered native captures of `gitturtle-025eaf4`, a clean release build of `025eaf40bf3b1f565b4488bd4434e2e3b1c6c141`, on an M2 Pro MacBook Pro running Omarchy 4.5.0-3 (Hyprland 0.56.2 on native Wayland, at scale 2). They show a disposable demo repository whose people, addresses and paths are invented. [The November 16 record](../docs/validation.md#november-16-the-readmes-screenshots-are-retaken-on-omarchy-45) holds the build, the method, the fixture, each picture's state and hash, and the privacy check.

| Website asset | README picture | Shown state |
| --- | --- | --- |
| `public/assets/history.png` | `history.png` | History in the Omarchy theme following Tokyo Night, project list on, a commit selected with its seven changed files; 2960 × 1946. |
| `public/assets/compare.png` | `compare.png` | Compare, the split view of `src/App.tsx` in the selected commit; 2960 × 1946. |
| `public/assets/changes.png` | `changes.png` | Changes, the unstaged diff of `src/App.tsx` with two hunks, two files staged and a message typed; 2960 × 1946. |
| `public/assets/file-history.png` | `file-history.png` | File history of `src/theme.css`, revision 2 of 5; 2960 × 1946. |
| `public/assets/image-compare.png` | `image-compare.png` | Compare of a PNG, Side by side at Fit; 2960 × 1946. |
| `public/assets/keyboard-shortcuts.png` | `keyboard-shortcuts.png` | Keyboard Shortcuts over History, 64 shortcuts listed; 2960 × 1946. |
| `public/assets/settings-themes.png` | `settings-themes.png` | Settings, the Omarchy card selected and following Tokyo Night; 2960 × 1946. |
| `public/assets/theme-editor.png` | `theme-editor.png` | The theme editor, an unsaved theme based on Midnight with its accent changed; 2960 × 1946. |
| `public/assets/themes-builtin.png` | `themes-builtin.png` | A sheet: the History frame in eight built-in themes, four dark and four light; 1836 × 694. |
| `public/assets/omarchy-themes.png` | `omarchy-themes.png` | A sheet: the History frame under 23 stock Omarchy themes, eighteen dark and five light; 1836 × 1002. |

The eight 2960 × 1946 pictures are single grabs of the app's window. The two sheets are composed from frames taken the same way, each reduced to a labeled tile on a transparent background, and are not single grabs. All ten come from Linux on Wayland: none is a macOS or X11 capture. They establish the displayed application state, not speed. `asset-provenance.json` records each copy's source path, bytes, SHA-256 and evidence; `check.py` compares the hashes.

Three of the README's pictures are not on the site. `command-palette.png` and `markdown.png` are not used by the page. `omarchy-desktop.png`, the whole-desktop sheet (3456 × 3383), is 2,405,054 bytes: more than the 2 MiB (2,097,152 bytes) that `smoke.py` allows one response. A picture is copied whole or not at all, so it stays out until a smaller capture exists.

The first three pictures the site shipped, captures of preview source `013e3348a29735617982bfd217551fdf91e4c694` from September 14, 2026, were replaced on November 16, 2026. Their record remains in [the September preview evidence directory](../docs/benchmarks/native-preview-20260914).

For later picture updates, copy the README's picture byte for byte: never resample, crop, composite or paint over one. Give each `img` its true `width` and `height` and a real alt text, and update this table, the provenance entries, the captions and the visible evidence link together. A short real recording may be added with visible playback controls, no autoplay, and a still-image fallback.

The app icon and turtle use existing project artwork from `assets/branding/app-icon.png` and `assets/AppIcon.icon/Assets/turtle.png`. Manrope Latin is self-hosted from Google Fonts (variable 400–800); its original SIL Open Font License is retained in `public/fonts/OFL.txt`. Typeface source: [Google Fonts Manrope](https://github.com/google/fonts/tree/main/ofl/manrope). No image-generation tool was used to alter product evidence.

## Cloudflare Pages deployment

The owner approved publication on September 14, 2026. Cloudflare Pages hosts the static files through **Direct Upload**, with no Git integration, automatic builds, Functions, analytics, environment secrets, or native compilation. Only `website/public/` is uploaded. Deployment tooling lives outside the native build; local Wrangler cache files stay in `website/.wrangler/` and are ignored.

| Item | Published identity |
| --- | --- |
| Canonical domain | `https://gitturtle.com` |
| Pages project | `gitturtle` |
| Pages hostname | `https://gitturtle.pages.dev` |
| Initial production deployment | `b39cf661-dde0-4d54-bfc1-347c9f62893e` |
| Immutable deployment URL | `https://b39cf661.gitturtle.pages.dev` |
| Initial deployed source | `0db617969b94c565ec1f89d94a03055e705c1efb`, exact `website/public/` bytes |
| Production branch label | `main` |
| Upload tool | Wrangler `4.131.2` |
| Current production deployment | `10a8ce6a-295e-44e4-a979-374420029a8b` |
| Current deployed source | `88b21e38f1282b0b228710bc0b240958f95cf21e` |

Publication created the Pages project and custom-domain binding, then added one proxied CNAME: `gitturtle.com` → `gitturtle.pages.dev`, automatic TTL. Cloudflare reports the domain, verification and certificate validation active. Both public DNS resolvers checked resolve the apex; domain TLS and file-byte checks passed against those resolved addresses. DNS propagation temporarily limited local browser verification, so browser checks used the working Pages hostname.

The native and website source commits were pushed after merging the remote's workflow-only update, preserving both histories. The resulting source revision is `483782dcc1187533c59bc1f54656821a29fb5588`. Hosted checks are recorded separately from source publication. Changes follow the pull-request workflow.

### Publish an update

Review the local page and run the focused website checks before an explicit deployment. From a clean, committed checkout, with Node.js 22 or newer and a Cloudflare login authorized for Pages:

```sh
node --check website/public/site.js
python3 website/check.py
cd website
npx --yes wrangler@4.131.2 login --scopes account:read user:read pages:write
npx --yes wrangler@4.131.2 pages deploy public --project-name gitturtle --branch main --commit-hash "$(git rev-parse HEAD)" --commit-dirty=false
```

Login is needed only when the CLI is not already authenticated. Select the existing owner's account if prompted. For a review deployment, use a distinct `--branch` value instead of `main`; this still publishes a public Pages URL. The deployment command uploads only `public/`, requires no build command, and does not install website packages in the native workspace. Credentials remain in Wrangler's user configuration, never in this repository.

Run `python3 smoke.py` from `website/` after deployment. Verify the returned deployment URL, then the canonical domain: HTTPS, page and asset bytes, security headers, a missing path returning the custom 404, desktop/mobile layout, keyboard navigation, and console errors. Update the identity and QA record when deploying changed assets.

Direct Upload projects cannot later be converted to Git-integrated projects in place. If automatic deployment becomes desirable, create a separate Git-integrated Pages project, restrict build watch paths to `website/**`, validate it, and explicitly migrate the domain.

### Rollback

For a later bad deployment, open Cloudflare → Workers & Pages → `gitturtle` → Deployments and roll back to a known successful **production** deployment. The first known deployment is recorded above. Recheck the canonical domain after rollback; the DNS destination remains unchanged.

To unpublish this first launch completely, remove only the `gitturtle.com` custom-domain binding and the launch CNAME, then delete the Pages project if its public `pages.dev` URLs must also disappear. Preserve any records serving other purposes.

Official references: [Direct Upload](https://developers.cloudflare.com/pages/get-started/direct-upload/), [custom domains](https://developers.cloudflare.com/pages/configuration/custom-domains/), [rollbacks](https://developers.cloudflare.com/pages/configuration/rollbacks/).
