#!/usr/bin/env python3
"""Manually check a deployed GitTurtle site. Read-only; Python stdlib and curl."""

import argparse
from datetime import datetime, timezone
import hashlib
from html.parser import HTMLParser
import ipaddress
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time
from urllib.parse import urljoin, urlsplit, urlunsplit


MAX_BYTES = 2 * 1024 * 1024
MAX_REQUESTS = 10
TOTAL_SECONDS = 120
PROBE_PATH = "/__gitturtle_smoke__/missing/page?check=https&source=smoke"
CERTIFICATE_PROBE = """
import socket, ssl, sys
context = ssl.create_default_context()
with socket.create_connection((sys.argv[2], 443), timeout=8) as connection:
    with context.wrap_socket(connection, server_hostname=sys.argv[1]) as secure:
        print(ssl.cert_time_to_seconds(secure.getpeercert()["notAfter"]))
"""


class SmokeFailure(Exception):
    pass


def require(condition, message):
    if not condition:
        raise SmokeFailure(message)


def passed(message):
    print(f"PASS {message}", flush=True)


class Page(HTMLParser):
    def __init__(self, body):
        super().__init__(convert_charrefs=True)
        self.title = ""
        self.in_title = False
        self.refs = []
        self.styles = []
        self.scripts = []
        self.fonts = []
        self.screenshots = []
        self.home_links = []
        self.robots = ""
        self.feed(body.decode("utf-8"))

    def handle_starttag(self, tag, attributes):
        attrs = dict(attributes)
        if tag == "title":
            self.in_title = True
        for key in ("href", "src"):
            if attrs.get(key):
                self.refs.append(attrs[key])
        if tag == "link" and attrs.get("rel") == "stylesheet":
            self.styles.append(attrs.get("href", ""))
        if tag == "link" and attrs.get("as") == "font":
            self.fonts.append(attrs.get("href", ""))
        if tag == "script" and attrs.get("src"):
            self.scripts.append(attrs["src"])
        if tag == "img" and "app-shot" in attrs.get("class", "").split():
            self.screenshots.append(attrs.get("src", ""))
        if tag == "a" and attrs.get("href") == "/":
            self.home_links.append(attrs["href"])
        if tag == "meta" and attrs.get("name", "").lower() == "robots":
            self.robots = attrs.get("content", "").lower()

    def handle_endtag(self, tag):
        if tag == "title":
            self.in_title = False

    def handle_data(self, data):
        if self.in_title:
            self.title += data


def header_values(raw):
    """Keep the final HTTP header block, including when a proxy adds CONNECT."""
    headers = {}
    for line in raw.decode("iso-8859-1").splitlines():
        if line.startswith("HTTP/"):
            headers = {}
        elif ":" in line:
            key, value = line.split(":", 1)
            key = key.lower().strip()
            headers[key] = ", ".join(filter(None, (headers.get(key), value.strip())))
    return headers


def security_headers(headers, label):
    age = re.search(r"(?:^|;)\s*max-age=(\d+)", headers.get("strict-transport-security", ""), re.I)
    require(age and int(age[1]) >= 15552000, f"{label}: HSTS must cover at least 15552000 seconds")
    require(headers.get("x-frame-options", "").upper() == "DENY", f"{label}: missing X-Frame-Options DENY")
    require(headers.get("x-content-type-options", "").lower() == "nosniff", f"{label}: missing nosniff")
    directives = {}
    for part in headers.get("content-security-policy", "").split(";"):
        words = part.strip().split()
        if words:
            directives[words[0]] = words[1:]
    for name, sources in (("default-src", ["'none'"]), ("script-src", ["'self'"]),
                          ("style-src", ["'self'"]), ("img-src", ["'self'"]),
                          ("font-src", ["'self'"]), ("frame-ancestors", ["'none'"])):
        require(directives.get(name) == sources, f"{label}: unexpected CSP {name}")


def cache_headers(headers, label, immutable=False, explicit_revalidation=False, require_etag=True):
    cache = {item.strip().lower() for item in headers.get("cache-control", "").split(",")}
    expected = "max-age=31536000" if immutable else "max-age=0"
    require(expected in cache, f"{label}: expected Cache-Control {expected}")
    if immutable:
        require("immutable" in cache, f"{label}: fingerprinted font must be immutable")
    else:
        require("immutable" not in cache, f"{label}: stable URL must not be immutable")
    if explicit_revalidation:
        require("must-revalidate" in cache, f"{label}: stable image must revalidate")
    etag = headers.get("etag", "")
    if require_etag:
        require(re.fullmatch(r'(?:W/)?"[^"\r\n]+"', etag), f"{label}: missing or invalid ETag")
    return etag


class Client:
    def __init__(self, base, resolved_ip, directory):
        self.base = base
        self.host = urlsplit(base).hostname
        self.resolved_ip = resolved_ip
        self.directory = Path(directory)
        self.deadline = time.monotonic() + TOTAL_SECONDS
        self.requests = 0
        self.curl = shutil.which("curl")
        require(self.curl, "curl is required but was not found on PATH")

    def remaining(self):
        seconds = min(15, self.deadline - time.monotonic())
        require(seconds >= 1, "120-second smoke-check deadline reached")
        return seconds

    def local_url(self, reference, relative_to=None):
        url = urljoin(relative_to or self.base, reference)
        require(urlsplit(url).netloc == urlsplit(self.base).netloc and urlsplit(url).scheme == "https",
                f"asset is not on the tested HTTPS origin: {reference}")
        return url

    def request(self, url, etag=None):
        require(self.requests < MAX_REQUESTS, "request budget exceeded")
        self.requests += 1
        prefix = self.directory / str(self.requests)
        header_file, body_file = prefix.with_suffix(".headers"), prefix.with_suffix(".body")
        # curl does not create its output file for an empty 304 response.
        body_file.touch()
        seconds = self.remaining()
        command = [self.curl, "--disable", "--silent", "--show-error", "--globoff",
                   "--proto", "=http,https", "--connect-timeout", "5", "--max-time", str(seconds),
                   "--max-filesize", str(MAX_BYTES), "--dump-header", str(header_file),
                   "--output", str(body_file), "--write-out", "%{http_code}"]
        if self.resolved_ip:
            address = f"[{self.resolved_ip}]" if ":" in self.resolved_ip else self.resolved_ip
            for port in (80, 443):
                command.extend(("--resolve", f"{self.host}:{port}:{address}"))
            command.extend(("--noproxy", self.host))
        if etag:
            command.extend(("--header", f"If-None-Match: {etag}"))
        command.extend(("--url", url))
        result = subprocess.run(command, capture_output=True, timeout=seconds + 1, check=False)
        require(result.returncode == 0, f"curl {url}: {result.stderr.decode(errors='replace').strip()}")
        require(body_file.stat().st_size <= MAX_BYTES, f"response exceeds {MAX_BYTES} bytes: {url}")
        return int(result.stdout), header_values(header_file.read_bytes()), body_file.read_bytes()

    def certificate(self):
        # A child bounds OS DNS resolution as well as the verified TLS handshake.
        result = subprocess.run([sys.executable, "-c", CERTIFICATE_PROBE, self.host,
                                 self.resolved_ip or self.host], capture_output=True,
                                timeout=min(10, self.remaining()), check=False)
        require(result.returncode == 0, "certificate verification failed: "
                + result.stderr.decode(errors="replace").strip())
        expiry = float(result.stdout)
        days = (expiry - time.time()) / 86400
        stamp = datetime.fromtimestamp(expiry, timezone.utc).strftime("%Y-%m-%d %H:%M UTC")
        require(days > 14, f"certificate expires {stamp}, only {days:.1f} days remaining")
        passed(f"verified certificate for {self.host}; expires {stamp} ({days:.1f} days remaining)")

    def check(self):
        self.certificate()
        status, headers, body = self.request(self.base)
        require(status == 200, f"homepage returned HTTP {status}, expected 200")
        page = Page(body)
        require(page.title.strip().startswith("GitTurtle | ") and len(page.title.strip()) > 12,
                "unexpected homepage title")
        security_headers(headers, "homepage")
        # The proxied Pages HTML response may omit ETag. Stable static assets
        # below must still supply validators and support conditional requests.
        cache_headers(headers, "homepage", require_etag=False)
        passed("homepage 200, expected title, security headers and revalidating cache")

        secure_probe = urljoin(self.base, PROBE_PATH)
        parsed = urlsplit(secure_probe)
        insecure_probe = urlunsplit(("http", parsed.netloc, parsed.path, parsed.query, ""))
        status, headers, _ = self.request(insecure_probe)
        require(status in (301, 308), f"HTTP redirect returned {status}, expected 301 or 308")
        require(headers.get("location") == secure_probe, "HTTP redirect did not preserve HTTPS host, nested path and query")
        passed("HTTP permanently redirects to HTTPS with nested path and query intact")

        status, headers, body = self.request(secure_probe)
        require(status == 404, f"missing nested path returned HTTP {status}, expected 404")
        missing = Page(body)
        require("noindex" in re.split(r"[\s,]+", missing.robots), "custom 404 must carry robots noindex")
        require(missing.home_links, "custom 404 needs a root-absolute home link")
        require(all(ref.startswith("/") and not ref.startswith("//") for ref in missing.refs),
                "custom 404 links/assets must be root-absolute for nested paths")
        security_headers(headers, "custom 404")
        passed("nested custom 404, noindex, root-absolute links/assets and security headers")

        require(len(page.styles) == len(page.scripts) == len(page.fonts) == 1 and page.screenshots,
                "expected one stylesheet, script and font preload, plus a product screenshot")
        resources = (("stylesheet", page.styles[0]), ("script", page.scripts[0]),
                     ("font", page.fonts[0]), ("screenshot", page.screenshots[0]))
        fetched = {}
        for label, reference in resources:
            url = self.local_url(reference)
            status, headers, body = self.request(url)
            require(status == 200, f"{label} returned HTTP {status}, expected 200")
            security_headers(headers, label)
            etag = cache_headers(headers, label, immutable=label == "font", explicit_revalidation=label == "screenshot")
            if label == "stylesheet":
                require("text/css" in headers.get("content-type", ""), "stylesheet has incorrect content type")
                urls = re.findall(r"url\(\s*['\"]?([^'\")\s]+)", body.decode("utf-8"))
                require(self.local_url(page.fonts[0]) in [self.local_url(ref, url) for ref in urls],
                        "stylesheet and HTML preload must reference the same font")
            elif label == "script":
                require("javascript" in headers.get("content-type", ""), "script has incorrect content type")
            elif label == "font":
                fingerprint = re.search(r"\.([a-f0-9]{12,64})\.woff2$", urlsplit(url).path)
                require(body.startswith(b"wOF2") and fingerprint, "font must be WOFF2 with a content fingerprint in its name")
                require(hashlib.sha256(body).hexdigest().startswith(fingerprint[1]), "font fingerprint does not match its bytes")
            elif label == "screenshot":
                require(body.startswith(b"\x89PNG\r\n\x1a\n"), "product screenshot is not a PNG")
            fetched[label] = (url, etag)
            passed(f"{label} 200, security headers, ETag and expected cache policy")

        for label in ("stylesheet", "font", "screenshot"):
            url, etag = fetched[label]
            status, _, body = self.request(url, etag)
            require(status == 304 and not body, f"{label}: If-None-Match must return empty 304, received {status}")
            passed(f"{label} ETag revalidation returns 304")
        print(f"Smoke checks passed: {self.requests} HTTP requests and one verified TLS connection. "
              "Certificate renewal is provider-managed; this checks current expiry, not a future renewal.")


def arguments():
    parser = argparse.ArgumentParser(description=__doc__, epilog=(
        "At most 10 HTTP requests (2 MiB each), one TLS connection, 5-second connect/15-second request "
        "timeouts and a 120-second overall deadline. No retries or redirect following. "
        "Exit 0 means passed; 1 means a check or connection failed; 2 means invalid arguments."))
    parser.add_argument("base_url", nargs="?", default="https://gitturtle.com",
                        help="HTTPS origin to check (default: https://gitturtle.com)")
    parser.add_argument("--resolve", metavar="IP", help=(
        "explicit origin IP for a stale local DNS cache; bypasses proxy for this host, "
        "preserves hostname/SNI and certificate verification; does not change system DNS"))
    args = parser.parse_args()
    try:
        parsed = urlsplit(args.base_url)
        if (parsed.scheme != "https" or not parsed.hostname or parsed.username or parsed.password
                or parsed.port not in (None, 443) or parsed.path not in ("", "/") or parsed.query or parsed.fragment):
            raise ValueError("base_url must be an HTTPS origin on port 443, without credentials, path or query")
        if args.resolve:
            args.resolve = str(ipaddress.ip_address(args.resolve))
    except ValueError as error:
        parser.error(str(error))
    host = f"[{parsed.hostname}]" if ":" in parsed.hostname else parsed.hostname
    args.base_url = urlunsplit(("https", host, "/", "", ""))
    return args


def main():
    args = arguments()
    print(f"Checking {args.base_url}", flush=True)
    if args.resolve:
        print(f"Explicit DNS override: {urlsplit(args.base_url).hostname} -> {args.resolve}; "
              "system DNS unchanged; TLS verification and hostname/SNI retained.", flush=True)
    try:
        with tempfile.TemporaryDirectory(prefix="gitturtle-smoke-") as directory:
            Client(args.base_url, args.resolve, directory).check()
    except (SmokeFailure, OSError, ValueError, KeyError, subprocess.TimeoutExpired) as error:
        print(f"FAIL {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
