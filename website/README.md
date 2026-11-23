# GitTurtle website

GitTurtle’s homepage, product documentation and installation guides ship together as static HTML. Cloudflare Pages serves the committed files in `website/public/`. The site uses self-hosted Manrope, native application screenshots and two small optional scripts.

## Preview and validate locally

From the repository root, start a server and open [the local preview](http://127.0.0.1:4173/).

```sh
python3 -m http.server 4173 --bind 127.0.0.1 --directory website/public
```

Edit the homepage in `website/public/index.html`. Edit guide content in `website/guides/` and run `python3 website/build_guides.py` to regenerate the published HTML. When adding or removing routes, refresh the sitemap with `python3 website/check.py --write-sitemap`. See the [guide authoring notes and product sources](guides/README.md).

Run the same checks as the website CI job.

```sh
python3 website/build_guides.py --check
python3 website/test_check.py
python3 website/check.py
python3 website/visual_qa.py
python3 -m unittest website/test_visual_qa.py
node --check website/public/site.js
node --check website/public/guide.js
```

The site checker validates internal links and fragments, public file hygiene, screenshot provenance, GitHub destinations, per-page metadata, structured data, crawler rules and sitemap coverage. `website/visual_qa.py` adds automated landmark, responsive-image, contrast-token, CSS-overflow-guard and local HTTP serving checks (plus optional headless Chromium / Playwright desktop `1440×900` and mobile `390×844` light/dark layout, overflow and `axe-core` accessibility verification when `--browser` is passed). The [QA record](QA.md) covers browser testing. The local Python server serves `404.html` directly and ignores Cloudflare’s `_headers` rules. See [hosting and deployment checks](OPERATIONS.md) for production behavior.

### Navigation, motion and search

The homepage tour shows History first. Clicking a tab or using Left, Right, Home or End selects a screenshot and its guide. Direct fragment links such as `/#panel-stage` open the corresponding panel. With JavaScript disabled, all three figures and article links appear in order. Full size opens a native image dialog. Choose Actual size to read the original pixels and pan, or Fit to window to see the full image. Closing the dialog returns to the screenshot. Without JavaScript the link opens the original PNG. Reduced motion uses immediate anchor scrolling. Installation commands are selectable text, with an optional copy button on compatible browsers.

Each indexable page has its own canonical URL, title, description and social sharing metadata. The homepage identifies GitTurtle as the site name with WebSite JSON-LD and describes the Git client with SoftwareApplication data. Guide breadcrumbs are ordinary links and carry matching BreadcrumbList data. The sitemap uses the public domain. Visitors and search crawlers can read the complete instructions in the original HTML. The static checks catch crawler-specific rules that would block Googlebot, Bingbot, DuckDuckBot, DuckAssistBot, Yahoo Slurp or OAI-SearchBot from a canonical page.

### Social sharing card

The homepage and guides point Open Graph and X to the same [1200 × 630 PNG](public/assets/gitturtle-social.png). Each guide shares its own title and description. The [card artwork](share-card.html) combines the app icon, the full History capture and the bundled Manrope font. To update it, open the artwork locally in Chromium, set the viewport to 1200 × 630 at device scale 1 and capture the viewport once the images and font have loaded. Update the source and image hashes in `asset-provenance.json`. Check the wordmark, headline and application window in wide and square crops.

## Content and release truth

The site's GitHub links open the project repository or its canonical GitHub Releases URLs (`allowed_github_url` in `website/check.py`). The guide articles, platform installation instructions, release status, benchmark records and notices have first-party destinations. Product guides are curated from the [repository sources](guides/README.md#content-sources). Update the relevant articles when a release changes a workflow or system requirement.

The November 2026 release check found no published downloads. The acquisition path currently offers builds from source for Apple Silicon Macs, Arch and Omarchy on x86-64 and AArch64, and Ubuntu 24.04 on x86-64. The [release page source](guides/reference/releases.html) records what visitors can install. When a verified release becomes available, update the homepage, installation overview, affected platform guides and release status together. Verify the actual artifact, architecture and signing status before advertising a download.

The performance page identifies the source build, hardware, sample size and timing boundaries of the homepage measurements. Its underlying records live in `public/performance/records/`. The 60 GB worktree story describes the owner’s experience.

### Screenshot provenance and replacement

All product images come from the repository captures in `docs/screenshots/readme/`. Seven full-size `2960 × 1946` PNGs (`history.png`, `history-filters.png`, `compare.png`, `changes.png`, `image-compare.png`, `theme-editor.png`, and `github-review.png`) are byte-for-byte copies of native Wayland captures from a clean release build of `0cd375246c4688ae7e76058c2f137f6eb117be30` (`a297bdcc…`, `rustc 1.100.0`) on an M2 Pro MacBook Pro running Omarchy 4.5.0-3 (Hyprland 0.56.2 on native Wayland, headless `2960 × 1998` output at scale 2). The three responsive tour WebPs and `gitturtle-social.png` are derivatives of those captures. `omarchy-desktop.png` retains its verified November 16 whole-monitor composite from `025eaf40bf3b1f565b4488bd4434e2e3b1c6c141` (`4bbb41f4…`). All captures show a disposable demo repository (and the offline GitHub review fixture) whose people, addresses and paths are invented. [The November 23 record](../docs/validation/2026-11.md#november-23-the-readmes-and-websites-screenshots-are-refreshed-on-omarchy-45) and [November 16 record](../docs/validation/2026-11.md#november-16-the-readmes-screenshots-are-retaken-on-omarchy-45) hold the builds, methods, fixtures, hashes, and privacy checks.

| Website asset | README picture | Shown state |
| --- | --- | --- |
| `public/assets/history.png` | `history.png` | History in the Omarchy theme following Tokyo Night, project list on, sidebar Tags and Stashes, `f121aaf` selected with its seven changed files; 2960 × 1946. |
| `public/assets/history-filters.png` | `history-filters.png` | History with the collapsible Search Filters bar expanded (`-S`/`-G`, author, path, date range, regex, first-parent, no merges); 2960 × 1946. |
| `public/assets/compare.png` | `compare.png` | Compare, the split view of `src/App.tsx` in the selected commit; 2960 × 1946. |
| `public/assets/changes.png` | `changes.png` | Changes, the unstaged diff of `src/App.tsx` with two hunks (`Stage hunk` / `Discard hunk`), two files staged and a commit draft; 2960 × 1946. |
| `public/assets/image-compare.png` | `image-compare.png` | Compare of a PNG, Side by side at Fit; 2960 × 1946. |
| `public/assets/theme-editor.png` | `theme-editor.png` | The custom theme editor with role swatches, live preview card, and readability feedback; 2960 × 1946. |
| `public/assets/github-review.png` | `github-review.png` | Native GitHub pull request review panel (offline fixture PR #42, Files tab on `src/review/session.rs` with inline diff and comment controls); 2960 × 1946. |
| `public/assets/omarchy-desktop.png` | `omarchy-desktop.png`, scaled down | A sheet: the whole monitor with the app tiled beside a terminal on Tokyo Night, and on Catppuccin Latte and Gruvbox at half size; 1728 × 1692, from 3456 × 3383. |

The seven 2960 × 1946 pictures are single grabs of the app's window; the desktop sheet holds composed whole-monitor frames. All eight come from Linux on Wayland: none is a macOS or X11 capture. They establish the displayed application state, not speed. `asset-provenance.json` records each picture's source path, bytes, SHA-256 and evidence; `check.py` compares the hashes, checks scaled-down source hashes, and rejects unreferenced orphan files under `public/assets/`.

On screens up to 710 CSS pixels wide, browsers that support WebP load 1200 × 789 previews of History, Compare and Stage. Each image still links to its full-size PNG.

The README's `omarchy-desktop.png` is 2,405,054 bytes, more than the 2 MiB (2,097,152 bytes) that `smoke.py` allows one response, so the site carries the whole picture scaled to half size, 1,728 pixels wide, with ImageMagick's Lanczos filter and its alpha channel kept: 1,177,156 bytes. A 2,208-pixel copy weighed 1.73 MB, over the 1.5 MB allowed for this picture.

README captures not referenced by the site (`command-palette.png`, `file-history.png`, `keyboard-shortcuts.png`, `markdown.png`, `omarchy-themes.png`, `settings-themes.png`, `themes-builtin.png`) stay in `docs/screenshots/readme/` only and are not duplicated into `public/assets/`.

The first three pictures the site shipped, captures of preview source `013e3348a29735617982bfd217551fdf91e4c694` from September 14, 2026, were replaced on November 16, 2026. Their record remains in [the September preview evidence directory](../docs/benchmarks/native-preview-20260914).

For later picture updates, copy the README's picture byte for byte. An app capture may be scaled down as a whole, never cropped, retouched or recolored, and the derivative's provenance entry names its source file, the source's sha256 and the method. Give each `img` its true `width` and `height` and a real alt text, and update this table, the provenance entries, the captions and the visible evidence link together. A short real recording may be added with visible playback controls, no autoplay, and a still-image fallback.

The app icon uses existing project artwork from `assets/branding/app-icon.png`. Manrope Latin is self-hosted from Google Fonts (variable 400–800); its original SIL Open Font License is retained in `public/fonts/OFL.txt`. Typeface source: [Google Fonts Manrope](https://github.com/google/fonts/tree/main/ofl/manrope). No image-generation tool was used to alter product evidence. `website/build_guides.py` also generates and verifies `public/legal/notices/` directly from root `LICENSE`, `THIRD_PARTY_NOTICES.md`, and `docs/licenses/assets/`.

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
| Current production deployment | `3932cdbc-a435-49ba-8729-e62ed7f39ca9` (November 17, 2026, `https://3932cdbc.gitturtle.pages.dev`) |
| Current deployed source | `d72dfab5d6da84557ecf8a63612ec2751a0dc3ad` |

Publication created the Pages project and custom-domain binding, then added one proxied CNAME: `gitturtle.com` → `gitturtle.pages.dev`, automatic TTL. Cloudflare reports the domain, verification and certificate validation active. Both public DNS resolvers checked resolve the apex; domain TLS and file-byte checks passed against those resolved addresses. DNS propagation temporarily limited local browser verification, so browser checks used the working Pages hostname.

The native and website source commits were pushed after merging the remote's workflow-only update, preserving both histories. The resulting source revision is `483782dcc1187533c59bc1f54656821a29fb5588`. Hosted checks are recorded separately from source publication. Changes follow the pull-request workflow.

### Publish an update

Review the local page and run the [focused website checks](#preview-and-validate-locally) before an explicit deployment. From a clean, committed checkout, with Node.js 22 or newer and a Cloudflare login authorized for Pages:

```sh
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
