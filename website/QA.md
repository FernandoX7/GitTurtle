# Website preview QA

The coordinator inspected the actual local static site at [http://127.0.0.1:4173](http://127.0.0.1:4173). This record distinguishes browser observations from source checks. No public deployment or DNS change was made.

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

Before the approved Cloudflare domain cutover, exercise the unavailable runtime modes and verify HTTPS, CSP/security headers, and the custom missing-page response on the Pages preview. The Python local server does not apply `_headers` or automatically serve `404.html` for unknown paths. The [deployment plan](README.md#proposed-cloudflare-pages-deployment) specifies the account/zone inspection, preview approval and rollback steps.
