#!/usr/bin/env python3
"""Check local HTML/CSS references, asset provenance and Pages inputs; no network."""
from fnmatch import fnmatchcase
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import quote, unquote, urlsplit
from urllib.robotparser import RobotFileParser
from xml.sax.saxutils import escape
import argparse
import hashlib
import json
import os
import re
import sys

ROOT = Path(__file__).resolve().parent
ORIGIN = "https://gitturtle.com"
REPOSITORY = "https://github.com/FernandoX7/GitTurtle"
GITHUB_DOMAINS = ("github.com", "githubusercontent.com", "github.io", "github.dev", "githubassets.com", "git.io")
NONPUBLIC_SUFFIXES = {
    ".pem", ".key", ".p12", ".pfx", ".jks", ".keystore",
    ".map", ".log", ".bak", ".backup", ".swp", ".swo", ".tmp",
    ".sql", ".sqlite", ".sqlite3", ".db", ".dump",
}


def check_public_files(public):
    """Reject accidental private outputs before reading any deployment content."""
    if public.is_symlink():
        return ["public/: symlinks are not deployable"]
    if not public.is_dir():
        return ["public/: deployment directory is missing"]
    errors = []

    def unreadable(_error):
        errors.append("public/: could not inspect a deployment directory")

    for directory, folders, files in os.walk(public, topdown=True, followlinks=False,
                                              onerror=unreadable):
        for name in sorted(folders + files):
            path = Path(directory) / name
            label = repr(path.relative_to(public).as_posix())
            problem = None
            if path.is_symlink():
                problem = "symlinks are not deployable"
            elif name.startswith("."):
                problem = "hidden files and directories are not deployable"
            elif name in files and path.suffix.lower() in NONPUBLIC_SUFFIXES:
                problem = "private or development output is not deployable"
            elif name in files and not path.is_file():
                problem = "only regular files are deployable"
            if problem:
                errors.append(f"public/{label}: {problem}")
                if name in folders:
                    folders.remove(name)
        folders.sort()
    return errors


class References(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.ids = set()
        self.refs = []
        self.errors = []
        self.links = []
        self.titles = []
        self.descriptions = []
        self.canonicals = []
        self.statements = []
        self.noindex = False
        self.capture = None
        self.parts = []

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if "id" in attrs:
            if attrs["id"] in self.ids:
                self.errors.append(f"duplicate id: {attrs['id']}")
            self.ids.add(attrs["id"])
        if tag == "img" and "alt" not in attrs:
            self.errors.append("image missing alt text")
        for key in ("href", "src"):
            if attrs.get(key):
                self.refs.append(attrs[key])
        if tag in ("img", "source") and attrs.get("srcset"):
            for candidate in attrs["srcset"].split(","):
                fields = candidate.split()
                if fields:
                    self.refs.append(fields[0])
        if tag == "a" and attrs.get("href"):
            self.links.append(attrs["href"])
        if tag == "title" or (tag == "script" and attrs.get("type") == "application/ld+json"):
            self.capture = tag
            self.parts = []
        if tag == "link" and "canonical" in (attrs.get("rel") or "").lower().split():
            self.canonicals.append(attrs.get("href"))
        if tag == "meta" and (attrs.get("name") or "").lower() == "description":
            self.descriptions.append((attrs.get("content") or "").strip())
        if tag == "button" and attrs.get("type") != "button":
            self.errors.append("button requires explicit type=button")
        if tag == "meta" and (attrs.get("name") or "").lower() == "robots":
            directives = set(re.split(r"[,\s]+", (attrs.get("content") or "").lower()))
            self.noindex |= bool({"noindex", "none"} & directives)

    def handle_data(self, data):
        if self.capture:
            self.parts.append(data)

    def handle_endtag(self, tag):
        if tag != self.capture:
            return
        content = "".join(self.parts).strip()
        self.capture = None
        if tag == "title":
            self.titles.append(content)
        else:
            try:
                value = json.loads(content)
                self.statements.extend(value if isinstance(value, list) else [value])
            except ValueError as error:
                self.errors.append(f"invalid JSON-LD: {error}")


def local_target(public, source, ref):
    url = urlsplit(ref)
    if (url.scheme or url.netloc) and url.hostname != urlsplit(ORIGIN).hostname:
        return None
    path = unquote(url.path) or ("/" if url.netloc else "")
    target = (public / path.lstrip("/") if path.startswith("/")
              else source.parent / path) if path else source
    target = target.resolve()
    if not target.is_relative_to(public.resolve()):
        raise ValueError(f"{source.name}: reference leaves the public directory: {ref}")
    if target.is_dir():
        target /= "index.html"
    elif not target.suffix and target.with_suffix(".html").is_file():
        target = target.with_suffix(".html")
    return target


def page_url(public, source):
    path = source.relative_to(public).as_posix()
    if source.name == "index.html":
        path = path.removesuffix("index.html")
    else:
        path = path.removesuffix(".html")
    return f"{ORIGIN}/{quote(path)}"


def matches_application(statement):
    if not isinstance(statement, dict):
        return False
    types = statement.get("@type", [])
    types = types if isinstance(types, list) else [types]
    if ("SoftwareApplication" in types and statement.get("name") == "GitTurtle"
            and statement.get("url") == ORIGIN + "/"):
        return True
    graph = statement.get("@graph", [])
    return isinstance(graph, list) and any(matches_application(node) for node in graph)


def check_pages(public, documents, write_sitemap):
    errors = []
    homepage = public / "index.html"
    searchable = {source for source, doc in documents.items() if not doc.noindex and source != public / "404.html"}
    for source, doc in documents.items():
        if source.name == "index.html" and doc.noindex:
            errors.append(f"{source.relative_to(public)}: public index page declares noindex")

    seen = {"title": {}, "description": {}}
    edges = {}
    for source, doc in documents.items():
        label = source.relative_to(public)
        if source in searchable:
            if doc.canonicals != [page_url(public, source)]:
                errors.append(f"{label}: expected one canonical URL {page_url(public, source)}")
            for key, values in (("title", doc.titles), ("description", doc.descriptions)):
                if len(values) != 1 or not values[0]:
                    errors.append(f"{label}: expected one nonempty {key}")
                    continue
                value = " ".join(values[0].split()).casefold()
                if value in seen[key]:
                    errors.append(f"{label}: same {key} as {seen[key][value]}")
                seen[key][value] = label
        edges[source] = set()
        for ref in doc.links:
            host = (urlsplit(ref).hostname or "").lower().rstrip(".")
            if any(host == domain or host.endswith("." + domain) for domain in GITHUB_DOMAINS) and ref != REPOSITORY:
                errors.append(f"{label}: GitHub link must point to the project repository: {ref}")
            target = local_target(public, source, ref)
            if target in searchable:
                edges[source].add(target)

    # A canonical, script or sitemap entry cannot make an orphan page browsable.
    reached = set()
    todo = [homepage]
    while todo:
        source = todo.pop()
        if source not in reached:
            reached.add(source)
            todo.extend(edges.get(source, ()))
    for source in sorted(searchable - reached):
        errors.append(f"{source.relative_to(public)}: no hyperlink path from homepage")

    if homepage in documents and not any(matches_application(item) for item in documents[homepage].statements):
        errors.append("index.html: expected SoftwareApplication JSON-LD for GitTurtle at its canonical URL")

    urls = sorted(page_url(public, source) for source in searchable)
    robots = RobotFileParser()
    robots.parse((public / "robots.txt").read_text().splitlines())
    if ORIGIN + "/sitemap.xml" not in (robots.site_maps() or []):
        errors.append("robots.txt: missing canonical sitemap")
    for url in urls:
        for agent in ("*", "Googlebot", "bingbot", "OAI-SearchBot"):
            if not robots.can_fetch(agent, url):
                errors.append(f"robots.txt: blocks {agent} from indexable page {url}")

    lines = ['<?xml version="1.0" encoding="UTF-8"?>',
             '<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">']
    lines.extend(f"  <url><loc>{escape(url)}</loc></url>" for url in urls)
    sitemap = "\n".join([*lines, "</urlset>", ""])
    path = public / "sitemap.xml"
    if not path.is_file() or path.read_text() != sitemap:
        if write_sitemap and not errors:
            path.write_text(sitemap)
        else:
            errors.append("sitemap.xml: regenerate current page URLs with python3 website/check.py --write-sitemap")
    return errors


def check_headers(public):
    """Lint the used Pages syntax and content-addressed immutable URLs.

    Actual response status, routing and effective headers belong to smoke.py.
    """
    rules = {}
    current = None
    errors = []
    for number, line in enumerate((public / "_headers").read_text().splitlines(), 1):
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        if not line[0].isspace():
            current = line.strip()
            if not current.startswith("/") or current in rules:
                errors.append(f"_headers:{number}: expected a unique local path rule")
            rules[current] = {}
        elif current is None or ":" not in line:
            errors.append(f"_headers:{number}: header must follow a path rule")
        else:
            name, value = line.strip().split(":", 1)
            name = name.lower()
            if not re.fullmatch(r"[a-z0-9-]+", name) or not value.strip() or name in rules[current]:
                errors.append(f"_headers:{number}: invalid or duplicate header")
            rules[current][name] = value.strip()
    for pattern, headers in rules.items():
        directives = {item.strip().lower() for item in headers.get("cache-control", "").split(",")}
        if "immutable" not in directives:
            continue
        assets = [path for path in public.rglob("*")
                  if path.is_file() and fnmatchcase("/" + path.relative_to(public).as_posix(), pattern)]
        if not assets:
            errors.append(f"_headers: immutable rule matches no files: {pattern}")
        for asset in assets:
            digest = hashlib.sha256(asset.read_bytes()).hexdigest()
            if not any(part == digest[:12] for part in asset.name.split(".")):
                errors.append(f"{asset.name}: immutable URL needs its current 12-character SHA-256 prefix")
    return errors


def check_site(root=ROOT, write_sitemap=False):
    public = root / "public"
    errors = check_public_files(public)
    if errors:
        return errors
    public = public.resolve()
    documents = {}
    for source in sorted(public.rglob("*.html")):
        doc = References()
        doc.feed(source.read_text())
        documents[source] = doc
        doc.close()
        errors.extend(f"{source.relative_to(public)}: {message}" for message in doc.errors)
    for required in ("index.html", "404.html"):
        if public / required not in documents:
            errors.append(f"missing {required}")
    for source, doc in documents.items():
        for ref in doc.refs:
            target = local_target(public, source, ref)
            if target is None:
                continue
            if not target.is_file():
                errors.append(f"{source.relative_to(public)}: missing target {ref}")
            fragment = unquote(urlsplit(ref).fragment)
            if fragment and target in documents and fragment not in documents[target].ids:
                errors.append(f"{source.relative_to(public)}: missing anchor {ref}")
            if source.name == "404.html" and urlsplit(ref).path and not ref.startswith("/"):
                errors.append(f"404.html: local URL must work at nested missing paths: {ref}")
    missing_page = documents.get(public / "404.html")
    if missing_page and (not missing_page.noindex or "/" not in missing_page.links):
        errors.append("404.html: requires robots noindex and a root home link")
    for source in sorted(public.rglob("*.css")):
        css = re.sub(r"/\*.*?\*/", "", source.read_text(), flags=re.S)
        # The site uses ordinary url() references, with optional quotes. This
        # intentionally is not a complete CSS parser or a browser substitute.
        for _, ref in re.findall(r"url\(\s*(['\"]?)(.*?)\1\s*\)", css, flags=re.I):
            target = local_target(public, source, ref.strip())
            if target is not None and not target.is_file():
                errors.append(f"{source.name}: missing CSS asset {ref}")
    for item in json.loads((root / "asset-provenance.json").read_text())["assets"]:
        target = (root / item["site_path"]).resolve()
        if not target.is_relative_to(public):
            errors.append("provenance asset must be inside public/")
            continue
        digest = hashlib.sha256(target.read_bytes()).hexdigest()
        if digest != item["sha256"]:
            errors.append(f"{target.name}: provenance hash needs updating")
        # A picture scaled down as a whole names its source's hash and the method.
        # A picture taken from the repository's own captures is either that
        # capture byte for byte or such a derivative: one that differs from its
        # source without saying how is refused, and so is one whose source is gone.
        derivative = "source_sha256" in item or "method" in item
        source = root.parent / item["source"]
        from_capture = str(item["source"]).startswith("docs/")
        if derivative and (not item.get("source_sha256") or not item.get("method")):
            errors.append(f"{target.name}: a derivative needs source_sha256 and method")
        elif (derivative or from_capture) and not source.is_file():
            errors.append(f"{target.name}: its source {item['source']} is missing")
        elif derivative or from_capture:
            source_digest = hashlib.sha256(source.read_bytes()).hexdigest()
            if derivative and source_digest != item["source_sha256"]:
                errors.append(f"{target.name}: derivative source hash needs updating")
            elif not derivative and source_digest != digest:
                errors.append(f"{target.name}: differs from its source, so it needs source_sha256 and method")
    errors.extend(check_headers(public))
    errors.extend(check_pages(public, documents, write_sitemap and not errors))
    return errors


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write-sitemap", action="store_true", help="regenerate sitemap.xml from indexable pages")
    args = parser.parse_args()
    try:
        errors = check_site(write_sitemap=args.write_sitemap)
    except (OSError, ValueError, KeyError) as error:
        errors = [str(error)]
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print("Static links, crawl paths, page metadata, sitemap, asset hashes, headers and 404 inputs passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
