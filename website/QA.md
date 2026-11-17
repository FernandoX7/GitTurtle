# Website preview QA

The coordinator inspected the actual local static site at [http://127.0.0.1:4173](http://127.0.0.1:4173). This record distinguishes browser observations from source checks. The initial local review preceded publication; the later deployment checks are recorded below.

| Browser check | Observed result |
| --- | --- |
| Desktop widths 1440 and 1024 pixels | Layout inspected; no horizontal overflow. |
| Mobile widths 390 and 320 pixels | Layout inspected; no horizontal overflow. |
| Walkthrough keyboard navigation | Left, End and Tab retained the expected tab/panel focus. |
| Prepare a commit activation | Panel became selected; its images loaded. |
| Browser console | No warnings or errors reported. |

Actual `prefers-reduced-motion` emulation, 200% browser zoom and JavaScript-disabled runtime execution were unavailable through the current browser capability. They have **not** been marked passed. The source review confirms reduced-motion CSS removes smooth scrolling, animation, transitions and hover movement; without script enhancement, the tour buttons are hidden and all three screenshot panels remain available. These are implementation findings, not substitutes for runtime checks.

A final mobile screenshot exposed joined sentences where a desktop line break is hidden. An explicit space now separates the sentences; the 390-pixel browser check confirmed the hidden break, correct text and no overflow.

The local checks passed: JavaScript syntax, HTML local assets and same-page anchors, duplicate IDs, explicit button types, image alt attributes, and provenance hashes. A focused source audit also checked that the three screenshot PNG dimensions match the declared 1480 × 800 sizes, provenance byte lengths match, tour panel references resolve, and no panel starts hidden. `check.py` makes no external network requests and does not validate rendered accessibility or deployed headers.

The HTML, stylesheet and script inspected for this record have these SHA-256 hashes:

| File | SHA-256 |
| --- | --- |
| `public/index.html` | `56eb36f14ce57442c5ebdde3bc16c68e68e0ead15a2482ef7dbc7e78ffc4be54` |
| `public/styles.css` | `9958b801b50744b140a2f25720e067b67fcaa9cb1a8d9234937c5a979fe5fe40` |
| `public/site.js` | `13334bb59acf3a0c743da306974e20753bd46e34dcf6aa053df7933b9d5c35e3` |

The product screenshots retain native source `013e3348a29735617982bfd217551fdf91e4c694` and executable SHA-256 `164a93b7100ad9be311645f680bb6c7d76ebb6dde8cfea520a6c7e41d098e47c`. They were captured from real GPUI client windows in Ubuntu 24.04 virtual X11/Xvfb with Openbox, default Midnight, in a disposable Aurora repository. They are not physical-desktop or macOS captures. The final installed source `417b5e8d0c739ca3746d2cab379db58efa4d8ea9` adds a watcher sibling-event fix; these images have not been relabeled as that later build. Exact asset hashes and identities are in [asset-provenance.json](asset-provenance.json).

## Publication verification

On September 14, 2026, the owner approved publication. Wrangler 4.131.2 uploaded the exact `website/public/` files from source `0db617969b94c565ec1f89d94a03055e705c1efb` to production deployment `b39cf661-dde0-4d54-bfc1-347c9f62893e` in Cloudflare Pages project `gitturtle`. The deployment uses no Functions.

At `https://gitturtle.pages.dev`, the browser rendered the same reviewed composition. The desktop viewport reported 1440 CSS pixels; the mobile browser reported 354 CSS pixels (the requested outer viewport was 390 pixels). Neither had horizontal overflow. The loaded hero, self-hosted font, and activated walkthrough panel displayed correctly. Left and End selected the expected tabs and retained focus; the selected Prepare a commit image loaded. The browser reported no console warnings or errors.

HTTPS requests to the Pages hostname returned HTTP 200 and an `index.html` SHA-256 matching the table above. The deployed CSP, Permissions-Policy, Referrer-Policy and X-Content-Type-Options headers matched `_headers`. A missing path returned HTTP 404 and bytes identical to `public/404.html`. Cloudflare's first immutable-hostname TLS request failed during initial provisioning; the stable Pages hostname subsequently passed TLS and browser checks. Python's default HTTP client received 403 responses for some asset requests; the actual browser and curl checks are the evidence for the rendered site and headers.

The public screenshot-evidence link was verified after source publication: GitHub's contents endpoint returned the 43 evidence files. Public releases remained empty; source-preview buttons remain appropriate. Reduced-motion emulation, 200% zoom and JavaScript-disabled execution retain the limitations stated above.

The [deployment record](README.md#cloudflare-pages-deployment) contains the source/deployment identity, exact DNS change, explicit update procedure and rollback options.

### Custom domain

Cloudflare subsequently reported the domain, verification and certificate validation **active**. Authoritative DNS, `1.1.1.1` and `8.8.8.8` returned the proxied addresses. Curl requests for `https://gitturtle.com` using one of those publicly resolved addresses (TLS verification enabled, correct domain SNI) returned HTTP 200 with the exact local HTML and expected security headers; the custom missing path returned HTTP 404 with the correct file bytes. Styles, script, all three product screenshots and the self-hosted font were also checked against local bytes.

DNS propagation temporarily limited local browser navigation to the apex (`ERR_NAME_NOT_RESOLVED`). No resolver settings or hosts-file entries were changed to conceal that limitation. Actual browser layout/interaction checks used the same production files at `gitturtle.pages.dev`; a local browser check of the apex remains pending cache expiry. The Pages and canonical HTTPS checks are distinct observations.

## Production safeguards and marketing refinement

The owner requested a production-readiness audit and more prominent performance
and open-source positioning. The revised browser/social title is
`GitTurtle | Fast, Open Source Git Client`; the hero and screenshot-label dots
have been removed. The 60 GB statement is Fernando's reported origin story,
not a benchmark or supported-capacity guarantee. The existing composition and
real native screenshots remain intact.

Local browser checks of the revised copy at reported widths 1309 and 354 CSS
pixels found no horizontal overflow. The desktop hero and origin story and
mobile hero were visually inspected. The source-preview install state remains
visible. See [OPERATIONS.md](OPERATIONS.md) for the transport, certificate,
cache and validation responsibilities.

The revised assets are live in deployment `10a8ce6a-295e-44e4-a979-374420029a8b`,
from source `88b21e38f1282b0b228710bc0b240958f95cf21e`. The deployed browser
confirmed the new title/copy, zero status-dot elements, loaded fonts and working
walkthrough controls without console warnings or errors. The scoped [Website
CI run](https://github.com/FernandoX7/GitTurtle/actions/runs/34905275416) passed
on that revision. Native quality jobs are separate and were still pending.

The [live smoke output](evidence/2026-09-14-production-smoke.txt) records ten
successful HTTP checks and a verified TLS connection through the explicitly
resolved public address. It covers permanent HTTP redirects preserving paths
and queries, nested 404 status/content, HSTS/CSP/framing/MIME headers, asset
availability, expected cache policies, matching font fingerprint, and empty
304 responses for conditional stylesheet/font/image requests. The served
Google Trust Services certificate expires December 13, 2026, at 22:30:14 UTC.
Cloudflare remains responsible for renewal; a future renewal has not been
observed. [The manifest](evidence/2026-09-14-production.json) identifies the
exact deployed files and exercised smoke helper by SHA-256.

The initial cloud cache imposed four hours on browser caching. Its setting now
respects origin headers, the updated stylesheet URL breaks that earlier cache,
and only this site's cached URLs were purged. No unrelated DNS, cache, account
membership or paid service was changed. The custom-domain local-browser check
still depends on DNS propagation completing; the verified Pages
browser and explicit-address TLS checks are not presented as that check.

## Public artifact hygiene

A scoped review of the recent website changes and native validation artifacts
found no recognized credentials, private keys or nonpublic repository content.
Unnecessary account-resource identifiers were removed from deployment notes,
incident wording was generalized, and personal paths in retained installation
logs were replaced with placeholders while retaining explicit redaction and
hash provenance. Public deployment files are checked separately from local
authentication state and build/tool caches, which stay outside version control.
This is a review of the changed artifacts, not an assertion about every earlier
repository revision.

The deployment checker selects the resolved curl executable explicitly with no
shell and terminates option parsing before URL data. Its complete live HTTP/TLS
check passed again after this refinement; the evidence records the current
helper hash and unchanged deployed asset identity. Hosted security analysis is
tracked on the pull request separately from the live checks.

## November 16, 2026 content refresh

The page was brought up to date with the application the repository README describes: the same design, more of the app. It now runs hero, speed, a six-tab tour, themes, Omarchy, how it works, Get started and open source, with ten of the README's pictures copied byte for byte ([provenance](README.md#screenshot-provenance-and-replacement)). This revision is not deployed: publishing, the smoke check and the deployment record are the owner's steps.

Headless Chromium 153.0.8010.36 on Arch Linux ARM rendered the page from its local files over a DevTools pipe, with no window and with host resolution disabled. The page made sixteen requests, all for its own files, and none to another origin.

| Check | Observed result |
| --- | --- |
| Full page at 1440, 1000, 700 and 380 pixels | Every frame inspected: nothing cut, overlapping or misaligned; Manrope loaded; every rendered picture loaded at its declared aspect ratio. |
| Horizontal overflow at fourteen widths from 320 to 1920 pixels | None: the document and every element stayed inside the viewport. |
| Six tour tabs at 1440 and 380 pixels | Each tab's picture and caption inspected. The caption block keeps one height across the six tabs at 1440, 1000, 700 and 380. |
| Tabs by keyboard | Home, five presses of Right, a wrap in each direction and End selected and focused the expected tab, showed only its panel and kept one tab sequentially focusable; Tab then moved into the visible panel. |
| Reduced motion, emulated | Scroll behavior computed as `auto` and the button and link transitions as `0s`; without the emulation, `smooth` and `0.18s`. |
| Scripts disabled, at 1440 and 380 pixels | The tab buttons stay hidden and all six pictures, captions and full-size links appear in document order. |
| Custom 404 at 1440 and 380 pixels | Inspected from a copy with its root-relative URLs made relative, since a file cannot resolve them; no console message. |

From a file, Chromium refuses the font preload's `crossorigin` request and logs two errors and two warnings for each load of the page; the font itself loads through the stylesheet, and the 404 page, which has no preload, logs nothing. The page was not observed over HTTP in this round, so the console over HTTPS, the response headers and the content security policy against the new markup remain to be checked at deployment. The markup has no inline style or script. 200% browser zoom, a physical phone, a screen reader and forced colors were not exercised. The frames were reviewed locally and are not in the repository.

| File | Bytes | SHA-256 |
| --- | ---: | --- |
| `public/index.html` | 28,861 | `ca59da727800ccba68700ef50d55c546d1f4d2afd751a1039c530173510b00af` |
| `public/styles.css` | 16,164 | `bf4c86520f896add7730e69f53f832be2ec0fcf59c5b1175883b884c2ac0fbeb` |
| `public/site.js` | 1,693 | `13334bb59acf3a0c743da306974e20753bd46e34dcf6aa053df7933b9d5c35e3` |
| `public/404.html` | 705 | `8825cb47ad0a13243824fb07a9c9cd2c293fed0c840d460b84896d703b684bb0` |

The script is unchanged from the September record. The first view requests 612,660 bytes: the page, stylesheet, script, font, icon and the History picture. Every other picture loads lazily, and a tab's picture only when the tab is first shown; with all of them the page weighs 5,778,296 bytes. The largest response is `omarchy-themes.png` at 948,008 bytes, under the 2 MiB that `smoke.py` allows.

## November 16, 2026 design and copy follow-up

A second pass the same day followed a design and copy review of the refresh. The speed figures now say what was measured and on what in one sentence each, with their labels inside their headings. The Omarchy section opens with the desktop picture beside its lead, carries four points instead of six, and shows its theme sheet on a mat in the site's deep green with the 23 themes named in the caption. The details heading stays in view beside its list, each picture link is named by its picture's alt text, and the turtle is a smaller file. This revision is not deployed either.

The desktop picture is the one app capture the site scales down ([the rule and its record](README.md#screenshot-provenance-and-replacement)). At 2,208 pixels wide it weighed 1.73 MB, over the 1.5 MB allowed for it, so the site carries it at 1,728 × 1,692 and 1,177,156 bytes. It was judged at full size in frames rendered at one and at two device pixels per pixel in a 1440-pixel window, where it is 703 pixels wide: the app's and the terminal's text is small and crisp in both, and the two half-size desktops stay legible as pictures rather than as text.

The same headless Chromium 153.0.8010.36 rendered the page from its local files over a DevTools pipe, with no window, host resolution disabled and the user's browser flags left out. Every request in the run, 458 across all loads, was for the site's own files.

| Check | Observed result |
| --- | --- |
| Full page at 1440, 1000, 700 and 380 pixels | Every frame inspected: nothing cut, overlapping or misaligned; Manrope loaded; every rendered picture loaded at its declared size. |
| Horizontal overflow at fourteen widths from 320 to 1920 pixels | None. |
| A single word alone on a last line, at 1440, 1000, 700 and 380 pixels | None in any headline, paragraph or caption, the six tab captions included. Two were found and fixed at 380: the tour's headline and the hero's caption. |
| Theme names at 380 pixels | Both sheets' captions are whole and name every theme in text. |
| Six tour tabs at 1440 and 380 pixels, by keyboard | As in the first pass: Home, five presses of Right, a wrap each way and End selected and focused the expected tab and showed only its panel; the caption block keeps one height. |
| Picture links | Chromium's accessibility tree names each of the eleven by its picture's alt text followed by "View full size", which is visually hidden text; no picture link has an `aria-label`. |
| Speed headings | Each is one heading in the accessibility tree, such as "WINDOW UP: 115 ms". The colon is in the text and drawn at no size. |
| Details heading at 1440 pixels | Stays 40 pixels from the top while its list scrolls past; ordinary at 700 pixels, where the columns stack. Emulated reduced motion leaves it in place, with scroll behavior `auto` and transitions `0s`. |
| Inline code | 11.44 pixels in 13-pixel text, 0.88 of its size. |
| The sheet's mat | Computes to `color-mix` of `--deep` at 70% with black, the same green as the section behind it, darker. |
| Scripts disabled, at 1440 and 380 pixels | The tab buttons stay hidden and all six pictures, captions and links appear in order. |
| Custom 404 at 1440 and 380 pixels | Inspected from a copy with its root-relative URLs made relative; no console message. |

Manrope's Latin subset has no non-breaking hyphen: Chromium drew U+2011 from a fallback font, so "ad-hoc" is kept on one line with `white-space: nowrap` instead. The limits of the first pass stand: the page was not observed over HTTP, so the console over HTTPS, the response headers and the content security policy against this markup remain to be checked at deployment, and from a file the font preload logs the same errors as before. The accessible names were read from Chromium's accessibility tree, not heard in a screen reader. 200% browser zoom, a physical phone, a screen reader and forced colors were not exercised. The frames were reviewed locally and are not in the repository.

| File | Bytes | SHA-256 |
| --- | ---: | --- |
| `public/index.html` | 29,346 | `1c7963d56640a7e4582fbadde19a137b9f8aa7e6ce29f8b4ed960dd55751f279` |
| `public/styles.css` | 17,373 | `dbb524707416d81ef1c585561ca988d8c32afde3e7dd42391ac870391cf4462e` |
| `public/site.js` | 1,693 | `13334bb59acf3a0c743da306974e20753bd46e34dcf6aa053df7933b9d5c35e3` |
| `public/404.html` | 705 | `9c8136d61e8e2438c5ab9553dda1b5b0b5d2a2b03d76b59731cbbe91c7cf9ed8` |

The script is unchanged. The first view requests 614,354 bytes: the page, stylesheet, script, font, icon and the History picture. With every picture the page weighs 6,249,996 bytes in seventeen files: the desktop picture adds 1,177,156 bytes and the turtle falls from 942,906 to 235,756. The largest response is now `omarchy-desktop.png`, under the 2 MiB that `smoke.py` allows.

## November 16, 2026 production deployment

The owner asked for the refreshed site to be published on November 16, 2026, and signed the upload tool in to Cloudflare for it. The files uploaded are `website/public/` of `main` at `86ed3d8f04bb901fade372182df91546574b06af`, from a clean checkout of that commit, after `node --check website/public/site.js` and `python3 website/check.py` passed there.

- **Deployment:** Wrangler 4.131.2, `pages deploy public --project-name gitturtle --branch main` with that commit's hash: 21 files, 15 uploaded and 6 already present, then `_headers`. Production deployment `cdbe8670-ab65-4aa2-8f29-42fa433dbc65`, `https://cdbe8670.gitturtle.pages.dev`, completed at 23:26 UTC.
- **Smoke check:** `python3 smoke.py` against `https://gitturtle.com` passed: ten HTTP requests and one verified TLS connection (the certificate expires on February 11, 2027; the homepage, the redirect to HTTPS, the custom 404, the stylesheet, script, font and screenshot with their headers and cache rules, and three revalidations answered 304). [The output](evidence/2026-11-16-production-smoke.txt) and [the manifest](evidence/2026-11-16-production.json) of the 22 uploaded files with their SHA-256 values are kept.
- **Live bytes:** the homepage served at the canonical domain and at the deployment's own address equals the committed `index.html` byte for byte (29,356 bytes) when fetched without a browser's `Accept` header; the stylesheet, the script, the History picture and the Omarchy desktop picture equal the uploaded files.
- **Rendered:** the live page was drawn in a headless Chromium at 1,440 and 380 pixels wide: the font, the hero picture, the measured figures and the tour's heading as in the reviewed frames.
- **Before it:** the no-script page was looked at whole (the hero, the figures, all six tour panels stacked with their captions, the themes), as the design review asked.

**One observation, not caused by the upload:** for a request that asks for HTML as a browser does, the canonical domain's response carries one more tag before `</body>`, Cloudflare's Web Analytics beacon (`static.cloudflareinsights.com/beacon.min.js`), added by the zone and not present in the uploaded file or at the `pages.dev` address. The site's own policy (`script-src 'self'`, `connect-src 'none'`) refuses that script, so it does not run and nothing is collected, but a browser's console records the refusal on every visit. The site's documents say it carries no analytics script. Whether to switch the zone's injection off, or to adopt analytics deliberately (which would change the policy and those documents), is the owner's decision.

**Resolved on November 16, 2026:** the owner switched the zone's Web Analytics automatic setup to Disable in the Cloudflare dashboard. At 00:42 UTC on November 17, two requests that ask for HTML as a browser does returned the canonical domain's homepage byte for byte equal to the committed `index.html` (29,356 bytes), with no `cloudflareinsights` or `beacon.min.js` in it. The site carries no analytics script, as its documents say, and a browser's console no longer records a refused script.

The page still says that no download is published; the lines marked `data-release` change with the first release.

## November 17, 2026 local preview of the product tour and guides

The preview served the static files over local HTTP. It covers the homepage, seven product guides, the documentation index, installation overview, three platform guides, performance, releases, support and legal notices.

| Check | Observed result |
| --- | --- |
| Chromium 157.0.8103.79, all 17 indexable pages | Twelve viewport widths from 320 to 1920 CSS pixels showed no horizontal overflow or overlapping header links. Each page had one h1 and the expected production canonical. Under the site's configured content security policy, the tested pages logged no errors and issued only same-origin requests. |
| Tour, navigation and installation | Left, Right, Home and End selected and focused the expected tabs, including wraparound. The visible panel decoded its screenshot. The Compare link opened the comparison section of its guide. The Stage fragment, Arch/Omarchy install path, custom 404 home link and reduced-motion scrolling worked. |
| JavaScript disabled | All three screenshots, captions and guide links were visible. The macOS install commands were readable after following the homepage and platform links. |
| Copy controls | With the clipboard stubbed in Chromium, a click copied the complete command block. A rejected request produced selectable-text instructions. |
| Automated accessibility | axe-core's WCAG 2.0 and 2.1 A/AA rules reported zero violations across all 17 pages at 390 and 1440 CSS pixels. |
| WebKit 26.5 on macOS | At 390 × 844 and 1440 × 900, the tour, guide navigation, macOS installation path and Home breadcrumb worked. History, Compare and Stage decoded as 1200-pixel WebPs at phone width and 2960-pixel PNGs at desktop width. Every full-size link returned the original PNG. |
| Static publication checks | Guide regeneration matched the published HTML. Each indexable URL was reachable from the homepage within two ordinary links. The sitemap included all 17 canonical URLs. GitHub anchors resolved to the repository root. |

The three narrow-screen tour images total 543,648 bytes, compared with 1,360,788 bytes for their full-size PNGs. The homepage requests the selected view first. Those figures measure transfer size.

These are local browser and static-file checks. They cover emulated phone viewports. Physical devices, assistive-technology sessions, hosted responses for this revision and search inclusion were outside this preview.

### Full-size screenshots

The ten screenshot links on the homepage and four guides use a native modal dialog. Chromium 157 and WebKit 27.2 at 1440 × 900 and 390 × 844 returned to the original screenshot, selected tour tab and exact starting scroll offset after dismissal. Escape, Close and the backdrop dismissed the viewer. Tab and Shift+Tab cycled its controls. Actual size showed the 2960-pixel-wide original and supported two-axis panning via keyboard and emulated touch. The same images opened directly with scripts disabled. The Chromium checks also exercised a failed download and recovery and found zero axe WCAG 2.0/2.1 A/AA violations in the open viewer.
