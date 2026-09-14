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
