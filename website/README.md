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

The coordinator verified the rendered site at 1440, 1024, 390 and 320 pixels with no horizontal overflow. Left/End/Tab navigation retained the expected keyboard focus; activating Prepare a commit selected the panel and its images loaded. The browser reported no console warnings or errors. Actual reduced-motion emulation, 200% zoom and JavaScript-disabled runtime checks were unavailable in the current browser capability; their fallback behavior was reviewed in source only. See [the website QA record](QA.md) for the executed scope and limits.

### Motion and JavaScript fallback

Source inspection confirms these behaviors; it does not substitute for browser emulation:

- Normal motion is limited to smooth in-page anchor scrolling and 180 ms button/link hover transitions, including a two-pixel hover offset. The walkthrough has no autoplay, timer, scrolling animation, or animated panel transition.
- With `prefers-reduced-motion: reduce`, anchor scrolling becomes immediate, animations and transitions are disabled (including pseudo-elements), and button/link hover translations are removed. The turtle's fixed rotation remains a static composition. The skip link still appears immediately when focused.
- Without JavaScript, the tour buttons remain hidden and all three screenshots, captions, and full-size links appear in document order. Navigation, source-install links and the rest of the page remain ordinary HTML. No panel starts with a `hidden` attribute.
- With JavaScript, the walkthrough initially selects Review changes. The script adds tab/tablist/tabpanel semantics, makes only the selected tab sequentially focusable, and shows its panel. Left/Right wrap and select; Home/End select the first/last tab. Click and native button Enter/Space activation select a view. The selected panel and its image link remain keyboard-accessible. Selection has no network side effects.

`check.py` parses the top-level HTML files, checks local `href`/`src` targets, same-page fragment IDs, duplicate IDs, explicit button types, image alt attributes, and hashes of the provenance assets. It does not render CSS, execute JavaScript, validate every accessibility interaction, check external link availability, or emulate reduced motion. A source audit also confirmed that all three current PNG screenshots are 1480 × 800, matching their HTML declarations. The recorded native hashes and captions are consistent with these captures.

The local Python preview server does not interpret Cloudflare's `_headers` file or automatically use the custom `404.html` for missing paths. Verify the security headers and actual missing-path behavior on the approved Pages preview before the domain cutover. No browser, native UI, deployment or DNS action was performed for this source/documentation audit.

## Content and release truth

The GitHub Releases API returned an empty array on 2026-09-14. The public [Releases page](https://github.com/FernandoX7/GitTurtle/releases) also reported no releases. Therefore the site offers **source build guides**, not download buttons. Never change a CTA to Download until the target public release asset exists, its platform/architecture and signature status are verified, and installation guidance matches it. No hosted endpoint is queried automatically by the website.

macOS and Linux are described as source previews. Linux's initial build target is Ubuntu 24.04 x86-64, not a promise of distribution-wide support. Local macOS bundles are ad-hoc signed and not notarized. Platform limits and the validation record remain one click away. Site claims come from the current repository README and user guide; no benchmark, endorsement, testimonial or release date has been invented.

### Screenshot provenance and replacement

History, Split and Working Changes use unaltered client-area captures of native preview source `013e3348a29735617982bfd217551fdf91e4c694` in a disposable Aurora fixture.

| Website asset | Capture | Exercised application identity |
| --- | --- | --- |
| `public/assets/history.png` | `final-history.png` | Source `013e3348a29735617982bfd217551fdf91e4c694`; release binary SHA-256 `164a93b7100ad9be311645f680bb6c7d76ebb6dde8cfea520a6c7e41d098e47c`; Ubuntu 24.04 virtual X11/Xvfb with Openbox, default Midnight theme; 1480 × 800. |
| `public/assets/compare.png` | `final-split.png` | Same captured executable/environment as History; actual Split view with Before and After source and highlighted changes; 1480 × 800. |
| `public/assets/changes.png` | `final-working.png` | Same captured executable/environment as History. Shows an unstaged `App.tsx` edit, staged `notes/preview-check.md`, and the composer title `Document the preview workflow` with Commit 1 file available; 1480 × 800. |

All native inputs for the captured executable were committed; its source tree was marked modified because of untracked docs/site work. These images were captured in a virtual X11 desktop, not on the physical desktop. They establish the displayed application state, not scrolling smoothness or macOS coverage. The coordinator records native evidence in [the current preview evidence directory](../docs/benchmarks/native-preview-20260914). `asset-provenance.json` records the copied bytes, image hashes, source commit and binary identity.

The final installed native source is `417b5e8d0c739ca3746d2cab379db58efa4d8ea9`. Its later change filters unrelated sibling events from recovery sentinel watches in `local_refresh.rs`; it does not change the displayed UI. The website screenshots remain captures of `013e3348…` and are not represented as captures of `417b5e8…`.

All initial bootstrap images have been replaced. For later image updates, retain real fixture content and app pixels, update this table, the provenance hashes, HTML image dimensions/alt text and tour captions, and the visible screenshot-evidence link. No app UI has been generated, composited or painted over. A short real recording may be added with visible playback controls, no autoplay, and a still-image fallback.

The app icon and turtle use existing project artwork from `assets/branding/app-icon.png` and `assets/AppIcon.icon/Assets/turtle.png`. Manrope Latin is self-hosted from Google Fonts (variable 400–800); its original SIL Open Font License is retained in `public/fonts/OFL.txt`. Typeface source: [Google Fonts Manrope](https://github.com/google/fonts/tree/main/ofl/manrope). No image-generation tool was used to alter product evidence.

## Proposed Cloudflare Pages deployment

No Cloudflare project, DNS record, paid service or public deployment has been created by this work. `gitturtle.com` did not return readable content through the research browser; that does not establish its DNS configuration or account state. Inspect the existing zone and Workers/Pages projects read-only before making changes.

After approving the rendered website and current screenshots:

1. In the user's existing Cloudflare account, inspect the `gitturtle.com` zone and any existing Pages/Worker route for the apex. Record the current apex and `www` records and existing destination before proposing a change. Preserve MX, TXT, mail, verification and unrelated subdomains.
2. Create a Pages project connected only to `FernandoX7/GitTurtle`, or use an existing appropriate project. Framework preset: **None**. Root directory: **website**. Build command: **exit 0**. Build output directory: **public**. No environment variables or secrets are needed. Configure build watch paths to include only `website/**`, separating site deployments from native application changes. Confirm the production branch with the owner; do not enable automatic production publication without approval.
3. Deploy the approved revision to its Pages preview hostname first. Review desktop/mobile appearance, HTTPS, 404 handling, `_headers`, source links, keyboard operation, and reduced motion there. A preview URL is still a public publication action and requires the final publication approval.
4. In the Pages project's **Custom domains**, add `gitturtle.com`. For a zone already in the same Cloudflare account, use the Pages flow so the custom-domain binding and the appropriate apex DNS record are established together. If an apex destination already exists, show the precise replacement before applying it. Do not change nameservers or remove unrelated records.
5. Add `www.gitturtle.com` only if the owner wants it. If added, configure one explicit redirect to `https://gitturtle.com` preserving path and query. Do not add a blanket redirect until existing `www` usage is known. The canonical, sitemap, and social metadata already use the apex.
6. Verify the final domain over HTTPS, its certificate, links and headers. Record the Pages project/deployment ID, deployed Git revision, approved DNS change and rollback destination. Cloudflare Pages can roll production back to an earlier deployment; preserve the pre-existing DNS destination if this is the first site cutover.

The exact remaining access is authorization for the existing Cloudflare account/project and zone plus approval to publish the reviewed revision. The final DNS change must be reviewed against the real zone; no placeholder record is safe to apply blindly.

Official references: [Static HTML deployment](https://developers.cloudflare.com/pages/framework-guides/deploy-anything/), [custom domains](https://developers.cloudflare.com/pages/configuration/custom-domains/), [build watch paths](https://developers.cloudflare.com/pages/configuration/build-watch-paths/), [rollbacks](https://developers.cloudflare.com/pages/configuration/rollbacks/).
