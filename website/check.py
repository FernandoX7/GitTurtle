#!/usr/bin/env python3
"""Check local HTML/CSS references, asset provenance and Pages inputs; no network."""
from fnmatch import fnmatchcase
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import urlsplit, unquote
import hashlib
import json
import os
import re
import sys

ROOT = Path(__file__).resolve().parent
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
        self.noindex = False

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
        if tag == "button" and attrs.get("type") != "button":
            self.errors.append("button requires explicit type=button")
        if tag == "meta" and (attrs.get("name") or "").lower() == "robots":
            self.noindex = "noindex" in {
                directive.strip() for directive in (attrs.get("content") or "").lower().split(",")
            }


def local_target(public, source, ref):
    url = urlsplit(ref)
    if url.scheme or url.netloc:
        return None
    path = unquote(url.path)
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


def check_site(root=ROOT):
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
        errors.extend(f"{source.name}: {message}" for message in doc.errors)
    for required in ("index.html", "404.html"):
        if public / required not in documents:
            errors.append(f"missing {required}")
    for source, doc in documents.items():
        for ref in doc.refs:
            target = local_target(public, source, ref)
            if target is None:
                continue
            if not target.is_file():
                errors.append(f"{source.name}: missing asset {ref}")
            fragment = unquote(urlsplit(ref).fragment)
            if fragment and target in documents and fragment not in documents[target].ids:
                errors.append(f"{source.name}: missing anchor {ref}")
            if source.name == "404.html" and urlsplit(ref).path and not ref.startswith("/"):
                errors.append(f"404.html: local URL must work at nested missing paths: {ref}")
    missing_page = documents.get(public / "404.html")
    if missing_page and (not missing_page.noindex or "/" not in missing_page.refs):
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
    errors.extend(check_headers(public))
    return errors


def main():
    try:
        errors = check_site()
    except (OSError, ValueError, KeyError) as error:
        errors = [str(error)]
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print("Static HTML/CSS references, anchors, asset hashes, immutable URLs and 404 inputs passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
