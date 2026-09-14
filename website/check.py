#!/usr/bin/env python3
"""Check the static site's local references and screenshot provenance; no network."""
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import urlsplit, unquote
import hashlib
import json
import sys

ROOT = Path(__file__).resolve().parent
PUBLIC = ROOT / "public"


class References(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.ids = set()
        self.refs = []
        self.errors = []

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


def main():
    errors = []
    for source in sorted(PUBLIC.glob("*.html")):
        doc = References()
        doc.feed(source.read_text())
        errors.extend(f"{source.name}: {message}" for message in doc.errors)
        for ref in doc.refs:
            url = urlsplit(ref)
            if url.scheme or url.netloc:
                continue
            if not url.path and url.fragment and url.fragment not in doc.ids:
                errors.append(f"{source.name}: missing anchor {ref}")
            if url.path:
                target = PUBLIC / unquote(url.path.lstrip("/"))
                if url.path == "/":
                    target = PUBLIC / "index.html"
                if not target.is_file():
                    errors.append(f"{source.name}: missing asset {ref}")
    for item in json.loads((ROOT / "asset-provenance.json").read_text())["assets"]:
        target = ROOT / item["site_path"]
        digest = hashlib.sha256(target.read_bytes()).hexdigest()
        if digest != item["sha256"]:
            errors.append(f"{target.name}: provenance hash needs updating")
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print("Static HTML references, image alt attributes, unique IDs, and asset hashes passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
