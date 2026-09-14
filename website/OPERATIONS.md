# Operating gitturtle.com

GitTurtle's marketing site is a static Cloudflare Pages deployment. The owner
approved publication and the production-readiness changes on September 14, 2026.
There is no application server, database, server-side secret, analytics script,
or runtime package installation. Deployable files live only in `website/public/`.

## Transport and certificates

- Cloudflare **Universal SSL is enabled**. The active managed certificate covers
  `gitturtle.com` and `*.gitturtle.com`. At this audit its issuer was Let's Encrypt
  and its expiry was December 13, 2026, at 18:26:35 UTC. That is a point-in-time
  observation, not a fixed renewal date. The Pages domain also has an active
  Google Trust Services certificate; the live TLS probe served that certificate,
  expiring December 13, 2026, at 22:30:14 UTC. No custom certificate was uploaded.
- Cloudflare handles certificate issuance, deployment and renewal. Keep the zone
  active, the site record proxied, and Universal SSL enabled. There is no local
  Certbot, renewal cron, private key or manual certificate upload to maintain.
  Do not add restrictive CAA records without checking Cloudflare's current
  certificate-authority requirements.
- **Always Use HTTPS is on.** HTTP requests redirect permanently while preserving
  the path and query. The minimum visitor TLS version is **1.2**; TLS 1.3 remains
  enabled. Actual TLS 1.2 and 1.3 handshakes succeeded and a TLS 1.1 handshake was
  rejected with a protocol-version alert.
- Pages sends `Strict-Transport-Security: max-age=15552000` (180 days). It does
  not preload the domain or extend the policy to unconfigured subdomains. HSTS
  persists in visitors' browsers: maintain valid HTTPS when migrating hosts.
  To retire that policy, first serve `max-age=0` over valid HTTPS; deleting the
  header alone does not clear an already cached policy.

Cloudflare's documentation explains [managed Universal SSL renewal](https://developers.cloudflare.com/ssl/edge-certificates/universal-ssl/)
and [Always Use HTTPS](https://developers.cloudflare.com/ssl/edge-certificates/additional-options/always-use-https/).
A future renewal cannot be claimed tested at initial installation.

## Responses and caching

- Unknown paths return HTTP **404**, the custom accessible error page, a working
  home link, root-relative assets and `noindex`. They do not masquerade as HTTP 200.
- The CSP allows only this site's scripts, styles, images and fonts, and blocks
  connections, framing, forms and base-URL injection. `X-Frame-Options: DENY`
  also covers older clients. MIME sniffing is disabled, referrers are restricted,
  and camera, microphone and geolocation are disabled.
- Cloudflare Browser Cache TTL respects existing headers (value `0`), replacing
  the initial four-hour override. HTML, CSS, JavaScript and stable image URLs
  require revalidation. Cloudflare's
  asset ETags let unchanged files return 304 without downloading the body again.
  The proxied HTML response can omit an ETag and return its small body on
  revalidation; the smoke check does not incorrectly require an HTML validator.
- The font has a SHA-256 prefix in its filename and a one-year immutable cache.
  When its bytes change, rename it using the new hash and update the stylesheet,
  preload and provenance file together. Never overwrite an immutable URL with
  different bytes. Font-license text is not given an immutable lifetime.
- Canonical metadata, Open Graph, sitemap and robots all use `https://gitturtle.com`.
  `www` is not configured. Source-preview copy must remain until downloadable,
  validated and properly licensed release assets actually exist.

## Checks and release procedure

The scoped website CI job runs the local static checks and JavaScript syntax
validation when website files change. It has read-only repository permissions,
pinned Actions and no Cloudflare credentials. Deployments remain explicit.

1. Run `python3 website/check.py` and `node --check website/public/site.js`.
2. Review the affected page in a browser, including mobile and keyboard behavior.
3. Commit the reviewed files and deploy through the [recorded Pages procedure](README.md#publish-an-update).
4. Run `python3 website/smoke.py` against the canonical domain. This makes bounded,
   read-only public HTTP/TLS requests and fails on incorrect responses, headers,
   assets or a certificate expiring within 14 days. It does not deploy or renew.
5. Record the deployment revision and verification in [QA.md](QA.md). If verification
   fails, use the [production rollback procedure](README.md#rollback).

The smoke check is an explicit validation tool, not a continuously running uptime
or alerting service. There is no claim of 24/7 monitoring. Certificate lifecycle
management belongs to Cloudflare; the smoke check independently examines the
certificate currently served. Account recovery and two-factor authentication
remain account-owner settings and were not audited through browser credentials.

During the initial launch, public resolvers had the new DNS record while the
home router still cached its earlier empty response. A transparent
`--resolve` override can check HTTPS against an address returned by authoritative
DNS without disabling certificate verification; it does not establish that the
local resolver has recovered. Never use `curl -k` to make a certificate check pass.
