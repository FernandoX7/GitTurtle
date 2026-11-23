#!/usr/bin/env python3
"""Automated website accessibility, contrast, layout-guard and optional browser visual QA.

Default mode uses only the Python standard library and runs with no network access:
  - HTML accessibility & CLS audit on every page in website/public/ (lang, viewport,
    skip-link + #main landmark, heading hierarchy, landmarks, explicit image width/height
    and alt text, SVG labels/aria-hidden, button types and accessible names).
  - CSS design-token WCAG 2.1 contrast audit and responsive/a11y media-query guards
    (focus-visible, reduced-motion, forced-colors, box-sizing, responsive img bounds).
  - Local loopback HTTP serving audit (127.0.0.1 ephemeral port, proxies bypassed) verifying
    every route in sitemap.xml plus local CSS/JS/font/image assets return HTTP 200.

Optional browser mode (`--browser` or `--browser-if-available`) serves website/public/
on 127.0.0.1 and drives headless Chromium through Playwright across desktop (1440x900)
and mobile (390x844) in light and dark prefers-color-scheme, blocking all outbound
network fetches, verifying zero horizontal overflow, decoded images, zero console/page/
request errors, and optional local axe-core violations (`--axe-script PATH`; never
fetches scripts from the network).
"""
from __future__ import annotations

import argparse
from dataclasses import asdict, dataclass
from functools import partial
from html.parser import HTMLParser
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import re
import sys
import threading
import urllib.error
import urllib.request
from urllib.parse import urlsplit
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parent
DEFAULT_VIEWPORTS = (
    ("desktop", 1440, 900),
    ("mobile", 390, 844),
)
DEFAULT_SCHEMES = ("light", "dark")

# Minimum WCAG 2.1 contrast thresholds for core site token pairs.
CONTRAST_CHECKS = (
    ("primary copy (--ink on --paper)", "--ink", "--paper", 7.0),
    ("secondary copy (--muted on --paper)", "--muted", "--paper", 4.5),
    ("accent link (--accent on --paper)", "--accent", "--paper", 4.5),
    ("dark section copy (#edf6ed on --deep)", "#edf6ed", "--deep", 7.0),
    ("dark code copy (#e6f6e8 on --deep)", "#e6f6e8", "--deep", 7.0),
    ("dark button copy (#f7fff9 on --ink)", "#f7fff9", "--ink", 7.0),
)


class BrowserUnavailableError(RuntimeError):
    """Raised when Playwright or its Chromium binary is unavailable in the environment."""


class ConfigError(RuntimeError):
    """Raised on invalid caller options (e.g., missing --axe-script or unsafe output path)."""


@dataclass(frozen=True)
class Finding:
    scope: str
    message: str

    def format(self) -> str:
        return f"{self.scope}: {self.message}"


def validate_local_path(path: str) -> str | None:
    """Return an error message if `path` is not a safe root-relative HTTP path, else None."""
    if not path or not path.startswith("/") or path.startswith("//"):
        return f"local path must start with a single '/': {path!r}"
    if "\\" in path or any(ch.isspace() or ord(ch) < 32 or ord(ch) == 127 for ch in path):
        return f"local path contains unsafe characters: {path!r}"
    if "?" in path or "#" in path:
        return f"local path must not contain query or fragment: {path!r}"
    parsed = urlsplit(f"http://127.0.0.1:80{path}")
    if parsed.scheme != "http" or parsed.hostname != "127.0.0.1":
        return f"local path escaped loopback origin: {path!r}"
    return None


def _sanitize_slug(route: str) -> str:
    raw = route.strip("/").replace("/", "_") or "index"
    slug = re.sub(r"[^A-Za-z0-9._-]+", "_", raw).strip("._-")
    return slug or "page"


def _add_srcset_assets(assets: set[str], srcset: str) -> None:
    for candidate in srcset.split(","):
        fields = candidate.split()
        if fields and fields[0].startswith("/"):
            assets.add(urlsplit(fields[0]).path)


class PageAuditor(HTMLParser):
    """Inspect structural accessibility, heading levels, landmarks and image dimensions."""

    def __init__(self, *, is_404: bool = False) -> None:
        super().__init__(convert_charrefs=True)
        self.is_404 = is_404
        self.findings: list[str] = []
        self.html_lang: str | None = None
        self.has_viewport = False
        self.has_skip_link = False
        self.has_main_id = False
        self.landmarks: set[str] = set()
        self.headings: list[tuple[int, str]] = []
        self.local_assets: set[str] = set()

        self._heading_level: int | None = None
        self._heading_parts: list[str] = []
        self._stack: list[dict] = []
        self._svg_title_parts: list[str] | None = None
        self._hidden_depth = 0

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        self._enter(tag.lower(), dict(attrs), self_closing=False)

    def handle_startendtag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        self._enter(tag.lower(), dict(attrs), self_closing=True)

    def _enter(self, tag: str, attrs: dict[str, str | None], *, self_closing: bool) -> None:
        is_aria_hidden = (attrs.get("aria-hidden") or "").lower() == "true"
        if is_aria_hidden and not self_closing:
            self._hidden_depth += 1

        if tag == "html":
            self.html_lang = (attrs.get("lang") or "").strip() or None
        elif tag == "meta":
            if (attrs.get("name") or "").lower() == "viewport":
                content = (attrs.get("content") or "").lower()
                if "width=device-width" in content:
                    self.has_viewport = True
        elif tag in ("header", "main", "nav", "footer"):
            self.landmarks.add(tag)
            if tag == "main" and attrs.get("id") == "main":
                self.has_main_id = True
        elif tag == "img":
            self._check_img(attrs)
        elif tag == "link":
            href = attrs.get("href") or ""
            rel = (attrs.get("rel") or "").lower().split()
            if href.startswith("/") and any(r in rel for r in ("stylesheet", "icon", "preload")):
                self.local_assets.add(urlsplit(href).path)
        elif tag == "script":
            src = attrs.get("src") or ""
            if src.startswith("/"):
                self.local_assets.add(urlsplit(src).path)
        elif tag == "source":
            if attrs.get("srcset"):
                _add_srcset_assets(self.local_assets, attrs["srcset"] or "")
        elif tag == "title" and not self_closing:
            if any(entry["tag"] == "svg" for entry in self._stack):
                self._svg_title_parts = []

        if tag in ("h1", "h2", "h3", "h4", "h5", "h6") and not self_closing:
            self._heading_level = int(tag[1])
            self._heading_parts = []

        if tag == "a":
            href = (attrs.get("href") or "").strip()
            classes = (attrs.get("class") or "").split()
            if href == "#main" and "skip-link" in classes:
                self.has_skip_link = True

        if tag in ("a", "button", "svg"):
            entry = {
                "tag": tag,
                "attrs": attrs,
                "text": [],
                "has_labeled_child": False,
                "has_title": False,
                "aria_hidden": is_aria_hidden,
            }
            if self_closing:
                self._finalize_interactive(entry)
            else:
                self._stack.append(entry)
        elif is_aria_hidden and not self_closing:
            self._stack.append({"tag": tag, "aria_hidden": True, "sentinel": True})

    def _check_img(self, attrs: dict[str, str | None]) -> None:
        src = attrs.get("src") or ""
        if src.startswith("/"):
            self.local_assets.add(urlsplit(src).path)
        if attrs.get("srcset"):
            _add_srcset_assets(self.local_assets, attrs["srcset"] or "")
        alt = attrs.get("alt")
        if alt is None:
            self.findings.append(f"<img> missing alt attribute ({src})")
        elif alt.strip() and self._hidden_depth == 0:
            for ancestor in self._stack:
                if not ancestor.get("sentinel") and ancestor["tag"] in ("a", "button"):
                    ancestor["has_labeled_child"] = True
        for dim in ("width", "height"):
            raw = (attrs.get(dim) or "").strip()
            if not raw.isdigit() or int(raw) <= 0:
                self.findings.append(f"<img> missing positive integer {dim} attribute ({src})")

    def handle_data(self, data: str) -> None:
        if self._heading_level is not None:
            self._heading_parts.append(data)
        if self._svg_title_parts is not None:
            self._svg_title_parts.append(data)
        if data.strip() and self._hidden_depth == 0:
            for entry in self._stack:
                if not entry.get("sentinel"):
                    entry["text"].append(data)

    def handle_endtag(self, tag: str) -> None:
        tag = tag.lower()
        if self._heading_level is not None and tag == f"h{self._heading_level}":
            text = " ".join("".join(self._heading_parts).split())
            self.headings.append((self._heading_level, text))
            self._heading_level = None
            self._heading_parts = []
        if tag == "title" and self._svg_title_parts is not None:
            title_text = "".join(self._svg_title_parts).strip()
            self._svg_title_parts = None
            if title_text:
                for entry in reversed(self._stack):
                    if not entry.get("sentinel") and entry["tag"] == "svg":
                        entry["has_title"] = True
                        break
        for idx in range(len(self._stack) - 1, -1, -1):
            if self._stack[idx]["tag"] == tag:
                entry = self._stack.pop(idx)
                if entry.get("aria_hidden"):
                    self._hidden_depth = max(0, self._hidden_depth - 1)
                if not entry.get("sentinel"):
                    self._finalize_interactive(entry)
                break

    def _finalize_interactive(self, entry: dict) -> None:
        tag = entry["tag"]
        attrs = entry["attrs"]
        text = " ".join("".join(entry["text"]).split())
        aria_label = (attrs.get("aria-label") or "").strip()
        aria_labelledby = (attrs.get("aria-labelledby") or "").strip()
        aria_hidden = entry["aria_hidden"]

        if tag == "button":
            if attrs.get("type") != "button":
                self.findings.append('button missing type="button"')
            if not (text or aria_label or aria_labelledby or entry["has_labeled_child"]):
                self.findings.append("button missing accessible name")
        elif tag == "a":
            if attrs.get("href") and not (text or aria_label or aria_labelledby or entry["has_labeled_child"]):
                self.findings.append(f"link missing accessible name ({attrs.get('href')})")
        elif tag == "svg":
            if not aria_hidden and not (aria_label or aria_labelledby or entry["has_title"]):
                self.findings.append('<svg> missing aria-hidden="true" or accessible label')

    def finalize(self) -> list[str]:
        issues = list(self.findings)
        if not self.html_lang:
            issues.append("<html> missing non-empty lang attribute")
        if not self.has_viewport:
            issues.append("missing responsive viewport meta tag")
        if "main" not in self.landmarks:
            issues.append("missing <main> landmark")
        if not self.is_404:
            if not self.has_skip_link:
                issues.append('missing skip-to-content link (<a class="skip-link" href="#main">)')
            if not self.has_main_id:
                issues.append('missing id="main" on <main> landmark')
            for required in ("header", "nav", "footer"):
                if required not in self.landmarks:
                    issues.append(f"missing <{required}> landmark")

        h1_count = sum(1 for level, _ in self.headings if level == 1)
        if h1_count != 1:
            issues.append(f"expected exactly one <h1>, found {h1_count}")
        prev_level = 0
        for level, text in self.headings:
            if not text:
                issues.append(f"empty <h{level}> heading")
            if prev_level and level > prev_level + 1:
                issues.append(f"skipped heading level: <h{prev_level}> followed by <h{level}> ({text})")
            prev_level = level
        return issues


def parse_hex_color(value: str) -> tuple[int, int, int]:
    raw = value.strip().lstrip("#")
    if len(raw) == 3 and all(ch in "0123456789abcdefABCDEF" for ch in raw):
        raw = "".join(ch * 2 for ch in raw)
    if len(raw) != 6 or not all(ch in "0123456789abcdefABCDEF" for ch in raw):
        raise ValueError(f"unsupported hex color: {value!r}")
    return int(raw[0:2], 16), int(raw[2:4], 16), int(raw[4:6], 16)


def relative_luminance(rgb: tuple[int, int, int]) -> float:
    def channel(value: int) -> float:
        srgb = value / 255.0
        return srgb / 12.92 if srgb <= 0.03928 else ((srgb + 0.055) / 1.055) ** 2.4

    r, g, b = rgb
    return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)


def contrast_ratio(fg: str, bg: str) -> float:
    l1 = relative_luminance(parse_hex_color(fg))
    l2 = relative_luminance(parse_hex_color(bg))
    lighter, darker = (l1, l2) if l1 >= l2 else (l2, l1)
    return (lighter + 0.05) / (darker + 0.05)


def strip_css_comments(css_text: str) -> str:
    return re.sub(r"/\*.*?\*/", "", css_text, flags=re.S)


def extract_root_tokens(css_text: str) -> dict[str, str]:
    clean = strip_css_comments(css_text)
    tokens: dict[str, str] = {}
    for match in re.finditer(r":root\s*\{([^}]*)\}", clean, flags=re.S):
        for prop, value in re.findall(r"(--[A-Za-z0-9-]+)\s*:\s*([^;}\n]+?)\s*(?:;|$)", match.group(1)):
            tokens[prop.strip()] = value.strip()
    return tokens


def check_css(public: Path) -> tuple[list[Finding], dict[str, float]]:
    findings: list[Finding] = []
    ratios: dict[str, float] = {}
    styles_path = public / "styles.css"
    if not styles_path.is_file():
        return [Finding("styles.css", "missing primary stylesheet")], ratios
    css = strip_css_comments(styles_path.read_text(encoding="utf-8"))
    tokens = extract_root_tokens(css)

    for label, fg_spec, bg_spec, minimum in CONTRAST_CHECKS:
        fg = tokens.get(fg_spec) if fg_spec.startswith("--") else fg_spec
        bg = tokens.get(bg_spec) if bg_spec.startswith("--") else bg_spec
        if not fg or not bg:
            findings.append(Finding("styles.css", f"missing CSS token for {label}"))
            continue
        try:
            ratio = contrast_ratio(fg, bg)
        except ValueError as error:
            findings.append(Finding("styles.css", f"{label}: {error}"))
            continue
        ratios[label] = round(ratio, 2)
        if ratio < minimum:
            findings.append(
                Finding("styles.css", f"{label} contrast {ratio:.2f}:1 is below {minimum:.1f}:1 ({fg} on {bg})")
            )

    required_patterns = (
        ("focus-visible outline", r":focus-visible\s*\{[^}]*outline\s*:"),
        ("border-box sizing", r"box-sizing\s*:\s*border-box"),
        ("responsive image max-width", r"(?:^|[\s},])img\s*\{[^}]*max-width\s*:\s*100%"),
        ("reduced-motion media query", r"@media\s*\(\s*prefers-reduced-motion\s*:\s*reduce\s*\)"),
        ("forced-colors media query", r"@media\s*\(\s*forced-colors\s*:\s*active\s*\)"),
    )
    for label, pattern in required_patterns:
        if not re.search(pattern, css, flags=re.I | re.S):
            findings.append(Finding("styles.css", f"missing required accessibility/layout guard: {label}"))

    guides_css = public / "guides.css"
    if guides_css.is_file():
        gtext = strip_css_comments(guides_css.read_text(encoding="utf-8"))
        if not re.search(r"overflow-wrap\s*:\s*anywhere", gtext, flags=re.I):
            findings.append(Finding("guides.css", "missing overflow-wrap: anywhere guard for narrow mobile screens"))
    return findings, ratios


def sitemap_routes(public: Path) -> list[str]:
    sitemap = public / "sitemap.xml"
    if not sitemap.is_file():
        return ["/"]
    tree = ET.fromstring(sitemap.read_text(encoding="utf-8"))
    routes = []
    for loc in tree.findall(".//{*}loc"):
        if loc.text:
            path = urlsplit(loc.text.strip()).path or "/"
            routes.append(path)
    return sorted(dict.fromkeys(routes)) or ["/"]


def check_html(public: Path) -> tuple[list[Finding], set[str], int]:
    findings: list[Finding] = []
    assets: set[str] = set()
    page_count = 0
    for html_path in sorted(public.rglob("*.html")):
        page_count += 1
        rel = html_path.relative_to(public).as_posix()
        auditor = PageAuditor(is_404=(rel == "404.html"))
        auditor.feed(html_path.read_text(encoding="utf-8"))
        auditor.close()
        for issue in auditor.finalize():
            findings.append(Finding(rel, issue))
        assets.update(auditor.local_assets)
    return findings, assets, page_count


class _QuietHandler(SimpleHTTPRequestHandler):
    def log_message(self, format: str, *args) -> None:  # noqa: A002
        return


class _LoopbackHTTPServer(ThreadingHTTPServer):
    daemon_threads = True


class LocalSiteServer:
    """Serve website/public over 127.0.0.1 on an ephemeral port with zero external network."""

    def __init__(self, public: Path) -> None:
        self.public = public
        self._server: _LoopbackHTTPServer | None = None
        self._thread: threading.Thread | None = None

    def __enter__(self) -> str:
        handler = partial(_QuietHandler, directory=str(self.public))
        server = _LoopbackHTTPServer(("127.0.0.1", 0), handler)
        port = server.server_address[1]
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        try:
            thread.start()
        except BaseException:
            server.server_close()
            raise
        self._server = server
        self._thread = thread
        return f"http://127.0.0.1:{port}"

    def __exit__(self, *_exc) -> None:
        server = self._server
        thread = self._thread
        self._server = None
        self._thread = None
        if server is not None:
            try:
                server.shutdown()
            finally:
                server.server_close()
        if thread is not None:
            thread.join(timeout=5.0)


def check_http_serving(public: Path, routes: list[str], assets: set[str]) -> tuple[list[Finding], int]:
    findings: list[Finding] = []
    checked = 0
    targets = list(routes) + sorted(assets)
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    with LocalSiteServer(public) as base_url:
        for path in targets:
            problem = validate_local_path(path)
            if problem is not None:
                findings.append(Finding(path, problem))
                continue
            url = f"{base_url}{path}"
            if urlsplit(url).hostname != "127.0.0.1":
                findings.append(Finding(path, f"refused non-loopback fetch: {url}"))
                continue
            try:
                with opener.open(url, timeout=10) as response:
                    status = response.status
                    body = response.read()
                    checked += 1
                    if status != 200:
                        findings.append(Finding(path, f"expected HTTP 200, got {status}"))
                    elif not body:
                        findings.append(Finding(path, "served empty response body"))
            except urllib.error.HTTPError as error:
                try:
                    findings.append(Finding(path, f"local HTTP fetch failed: HTTP {error.code}"))
                finally:
                    error.close()
            except (urllib.error.URLError, OSError) as error:
                findings.append(Finding(path, f"local HTTP fetch failed: {error}"))
    return findings, checked


def run_browser_qa(
    public: Path,
    routes: list[str],
    *,
    out_dir: Path | None = None,
    axe_script: Path | None = None,
    viewports=DEFAULT_VIEWPORTS,
    schemes=DEFAULT_SCHEMES,
) -> tuple[list[Finding], dict]:
    """Run headless Chromium visual/layout/a11y checks via Playwright across viewports & schemes."""
    axe_source: str | None = None
    if axe_script is not None:
        if axe_script.is_symlink() or not axe_script.is_file():
            raise ConfigError(f"axe-core script not found or is a symlink: {axe_script}")
        axe_source = axe_script.read_text(encoding="utf-8")

    resolved_out: Path | None = None
    if out_dir is not None:
        if out_dir.is_symlink():
            raise ConfigError(f"refusing symlinked screenshot output directory: {out_dir}")
        out_dir.mkdir(parents=True, exist_ok=True)
        resolved_out = out_dir.resolve()

    for route in routes:
        problem = validate_local_path(route)
        if problem is not None:
            raise ConfigError(problem)

    try:
        from playwright.sync_api import Error as PlaywrightError  # type: ignore[import-not-found]
        from playwright.sync_api import sync_playwright  # type: ignore[import-not-found]
    except ImportError as error:
        raise BrowserUnavailableError(
            "Playwright is not installed (`pip install playwright && playwright install chromium`)"
        ) from error

    findings: list[Finding] = []
    checks_run = 0
    screenshots: list[str] = []

    with LocalSiteServer(public) as base_url:
        base_parsed = urlsplit(base_url)
        allowed_origin = f"{base_parsed.scheme}://{base_parsed.netloc}"
        with sync_playwright() as playwright:
            try:
                browser = playwright.chromium.launch(
                    args=["--host-resolver-rules=MAP * ~NOTFOUND, EXCLUDE 127.0.0.1"]
                )
            except PlaywrightError as error:
                raise BrowserUnavailableError(f"Playwright Chromium browser unavailable: {error}") from error
            try:
                for scheme in schemes:
                    for vp_name, width, height in viewports:
                        context = browser.new_context(
                            viewport={"width": width, "height": height},
                            color_scheme=scheme,
                        )
                        try:
                            for route in routes:
                                scope = f"{route} [{vp_name} {width}x{height} {scheme}]"
                                console_errors: list[str] = []
                                page_errors: list[str] = []
                                failed_requests: list[str] = []
                                blocked_external: list[str] = []

                                page = context.new_page()
                                try:
                                    def _guard_route(route_req, _scope=scope) -> None:
                                        req_url = route_req.request.url
                                        parsed = urlsplit(req_url)
                                        req_origin = f"{parsed.scheme}://{parsed.netloc}"
                                        if req_origin != allowed_origin:
                                            blocked_external.append(req_url)
                                            route_req.abort("blockedbyclient")
                                        else:
                                            route_req.continue_()

                                    page.route("**/*", _guard_route)
                                    page.on(
                                        "console",
                                        lambda msg: console_errors.append(msg.text)
                                        if msg.type == "error"
                                        else None,
                                    )
                                    page.on("pageerror", lambda err: page_errors.append(str(err)))
                                    page.on(
                                        "response",
                                        lambda res: failed_requests.append(f"{res.status} {res.url}")
                                        if res.status >= 400
                                        else None,
                                    )
                                    page.on(
                                        "requestfailed",
                                        lambda req: failed_requests.append(
                                            f"requestfailed {req.url} ({req.failure})"
                                        ),
                                    )

                                    response = page.goto(f"{base_url}{route}", wait_until="networkidle")
                                    checks_run += 1
                                    if response is None or response.status != 200:
                                        findings.append(
                                            Finding(
                                                scope,
                                                f"navigation returned {response.status if response else 'None'}",
                                            )
                                        )
                                        continue

                                    metrics = page.evaluate(
                                        """() => {
                                            const doc = document.documentElement;
                                            const imgs = Array.from(document.querySelectorAll('img'))
                                                .filter(img => img.offsetParent !== null)
                                                .map(img => ({
                                                    src: img.getAttribute('src'),
                                                    complete: img.complete,
                                                    naturalWidth: img.naturalWidth,
                                                }));
                                            return {
                                                scrollWidth: doc.scrollWidth,
                                                innerWidth: window.innerWidth,
                                                brokenImgs: imgs.filter(i => !i.complete || i.naturalWidth === 0),
                                            };
                                        }"""
                                    )
                                    if metrics["scrollWidth"] > metrics["innerWidth"] + 1:
                                        findings.append(
                                            Finding(
                                                scope,
                                                f"horizontal overflow: scrollWidth={metrics['scrollWidth']} > "
                                                f"innerWidth={metrics['innerWidth']}",
                                            )
                                        )
                                    for broken in metrics["brokenImgs"]:
                                        findings.append(Finding(scope, f"broken image: {broken['src']}"))
                                    for ext_url in blocked_external:
                                        findings.append(Finding(scope, f"blocked outbound request: {ext_url}"))
                                    for err in console_errors:
                                        findings.append(Finding(scope, f"console error: {err}"))
                                    for err in page_errors:
                                        findings.append(Finding(scope, f"page error: {err}"))
                                    for err in failed_requests:
                                        findings.append(Finding(scope, f"failed request: {err}"))

                                    if axe_source is not None:
                                        page.add_script_tag(content=axe_source)
                                        axe_result = page.evaluate(
                                            "async () => await axe.run(document, "
                                            "{ runOnly: ['wcag2a', 'wcag2aa', 'wcag21aa'] })"
                                        )
                                        for violation in axe_result.get("violations", []):
                                            impact = violation.get("impact") or "unknown"
                                            if impact in ("serious", "critical"):
                                                findings.append(
                                                    Finding(
                                                        scope,
                                                        f"axe [{impact}] {violation.get('id')}: {violation.get('help')}",
                                                    )
                                                )

                                    if resolved_out is not None:
                                        slug = _sanitize_slug(route)
                                        shot = resolved_out / f"{slug}-{vp_name}-{scheme}.png"
                                        if shot.is_symlink():
                                            raise ConfigError(f"refusing symlinked screenshot target: {shot}")
                                        if not shot.resolve().is_relative_to(resolved_out):
                                            raise ConfigError(f"screenshot path escaped output dir: {shot}")
                                        page.screenshot(path=str(shot), full_page=True)
                                        screenshots.append(str(shot))
                                finally:
                                    page.close()
                        finally:
                            context.close()
            finally:
                browser.close()

    return findings, {
        "checks": checks_run,
        "screenshots": screenshots,
        "axe_enabled": axe_source is not None,
    }


def run_qa(
    root: Path = ROOT,
    *,
    browser: bool = False,
    browser_if_available: bool = False,
    out_dir: Path | None = None,
    axe_script: Path | None = None,
    routes: list[str] | None = None,
    skip_http: bool = False,
) -> tuple[int, dict]:
    public = (root / "public").resolve()
    if not public.is_dir():
        return 1, {"findings": [asdict(Finding("public/", "deployment directory is missing"))]}

    all_findings: list[Finding] = []
    html_findings, assets, page_count = check_html(public)
    all_findings.extend(html_findings)

    css_findings, contrast_ratios = check_css(public)
    all_findings.extend(css_findings)

    selected_routes = routes if routes is not None else sitemap_routes(public)
    http_checked = 0
    if not skip_http:
        http_findings, http_checked = check_http_serving(public, selected_routes, assets)
        all_findings.extend(http_findings)

    browser_summary: dict = {"ran": False}
    if browser or browser_if_available:
        try:
            b_findings, b_meta = run_browser_qa(
                public, selected_routes, out_dir=out_dir, axe_script=axe_script
            )
            all_findings.extend(b_findings)
            browser_summary = {"ran": True, **b_meta}
        except ConfigError as error:
            return 2, {
                "error": str(error),
                "findings": [asdict(f) for f in all_findings],
            }
        except BrowserUnavailableError as error:
            if browser:
                return 2, {
                    "error": str(error),
                    "findings": [asdict(f) for f in all_findings],
                }
            browser_summary = {"ran": False, "skipped": str(error)}

    report = {
        "pages_checked": page_count,
        "routes_checked": len(selected_routes),
        "http_urls_checked": http_checked,
        "contrast_ratios": contrast_ratios,
        "browser": browser_summary,
        "findings": [asdict(f) for f in all_findings],
    }
    return (1 if all_findings else 0), report


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", type=Path, default=ROOT, help="website directory containing public/")
    parser.add_argument("--browser", action="store_true", help="require Playwright Chromium desktop/mobile visual checks")
    parser.add_argument(
        "--browser-if-available",
        action="store_true",
        help="run Playwright Chromium checks when installed, otherwise run static + HTTP checks only",
    )
    parser.add_argument("--out", type=Path, help="directory to save browser screenshots when --browser is enabled")
    parser.add_argument("--axe-script", type=Path, help="local axe.min.js path for offline browser accessibility checks")
    parser.add_argument("--route", action="append", help="specific route to test (repeatable; defaults to sitemap.xml)")
    parser.add_argument("--skip-http", action="store_true", help="skip local loopback HTTP verification")
    parser.add_argument("--json", type=Path, help="write machine-readable JSON report to this path")
    args = parser.parse_args(argv)

    if args.json is not None and args.json.is_symlink():
        print(f"visual_qa: refusing symlinked --json output: {args.json}", file=sys.stderr)
        return 2

    code, report = run_qa(
        root=args.root,
        browser=args.browser,
        browser_if_available=args.browser_if_available,
        out_dir=args.out,
        axe_script=args.axe_script,
        routes=args.route,
        skip_http=args.skip_http,
    )
    if args.json is not None:
        args.json.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")

    if code == 2:
        print(f"visual_qa: {report['error']}", file=sys.stderr)
        return 2
    if code != 0:
        for item in report["findings"]:
            print(f"{item['scope']}: {item['message']}", file=sys.stderr)
        return 1

    browser_note = (
        f", {report['browser']['checks']} browser views"
        if report["browser"].get("ran")
        else ""
    )
    print(
        f"Website visual QA passed ({report['pages_checked']} pages, "
        f"{report['http_urls_checked']} local HTTP resources, "
        f"{len(report['contrast_ratios'])} WCAG contrast pairs{browser_note})."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
