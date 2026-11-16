"""Supplement fixtures for the license collector; no registry, network or real Cargo.

The real collector is copied into a disposable source tree, where a stand-in
`cargo` prints a fixed `cargo metadata` document: the application and the
dependencies a test names, none of which ships a license file of its own.
"""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

COLLECTOR = Path(__file__).resolve().parents[2] / "collect-third-party-licenses.py"
TARGET = "x86_64-unknown-linux-gnu"
FETCHED = "https://example.invalid/upstream/0123abc/LICENSE"
STANDARD = "https://example.invalid/license-list/0123abc/text/MIT.txt"
FILLED = 'Standard MIT text with its copyright line filled in as "Copyright (c) Fixture Author".'


class SupplementTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve() / "source"
        self.licenses = self.root / "docs/licenses"
        (self.licenses / "texts").mkdir(parents=True)
        (self.licenses / "assets").mkdir()
        (self.licenses / "assets/sources.json").write_text("[]")
        (self.root / "scripts").mkdir()
        shutil.copyfile(COLLECTOR, self.root / "scripts" / COLLECTOR.name)
        for name in ("LICENSE", "THIRD_PARTY_NOTICES.md", "Cargo.lock"):
            (self.root / name).write_text("fixture\n")
        tools = self.root / "tools"
        tools.mkdir()
        (tools / "cargo").write_text('#!/bin/sh\ncat "$FIXTURE_METADATA"\n')
        (tools / "cargo").chmod(0o755)
        self.environment = dict(os.environ, PATH=f"{tools}{os.pathsep}{os.environ['PATH']}",
                                FIXTURE_METADATA=str(self.root / "metadata.json"))
        self.output = self.root / "collected"

    def supplement(self, name, text, **record):
        """A supplement for `name` whose one text has the given source fields."""
        digest = hashlib.sha256(text).hexdigest()
        path = self.licenses / "texts" / f"{digest}.txt"
        path.write_bytes(text)
        note = {"note": record.pop("note")} if "note" in record else {}
        return path, dict(name=name, version="1.0.0", **note,
                          files=[dict(path=f"texts/{digest}.txt", sha256=digest, **record)])

    def collect(self, supplements, dependencies):
        (self.licenses / "dependencies.json").write_text(json.dumps(supplements))
        packages = [dict(id="gitturtle 0.1.0", name="gitturtle", version="0.1.0",
                         manifest_path=str(self.root / "crates/app/Cargo.toml"))]
        for name in dependencies:
            crate = self.root / "registry" / f"{name}-1.0.0"
            crate.mkdir(parents=True, exist_ok=True)
            packages.append(dict(id=f"{name} 1.0.0", name=name, version="1.0.0", license="MIT/Apache-2.0",
                                 license_file=None, authors=["Fixture Author"], repository=None,
                                 source="registry+fixture", manifest_path=str(crate / "Cargo.toml")))
        edges = [dict(pkg=f"{name} 1.0.0", dep_kinds=[dict(kind=None, target=None)]) for name in dependencies]
        nodes = [dict(id="gitturtle 0.1.0", deps=edges)] + [dict(id=f"{name} 1.0.0", deps=[]) for name in dependencies]
        (self.root / "metadata.json").write_text(json.dumps(dict(
            packages=packages, resolve=dict(nodes=nodes), workspace_members=["gitturtle 0.1.0"])))
        return subprocess.run([sys.executable, "-B", str(self.root / "scripts" / COLLECTOR.name), "--target", TARGET,
                               "--require-complete", str(self.output)],
                              env=self.environment, text=True, capture_output=True, timeout=60)

    def test_composed_text_completes_the_inventory_and_is_recorded_as_composed(self):
        _, fetched = self.supplement("fetched", b"upstream text\n", url=FETCHED)
        _, composed = self.supplement("composed", b"composed text\n", note="Decided by review.",
                                      standard_text=STANDARD, composed=FILLED)
        result = self.collect([fetched, composed], ["fetched", "composed"])
        self.assertEqual(result.returncode, 0, result.stderr)
        inventory = json.loads((self.output / "dependencies.json").read_text())
        self.assertEqual(inventory["review_required"], [])
        entries = {entry["name"]: entry for entry in inventory["dependencies"]}
        self.assertEqual(entries["composed"]["notices"], [dict(
            path="notices/composed-1.0.0/composed-1.txt", sha256=hashlib.sha256(b"composed text\n").hexdigest(),
            standard_text=STANDARD, composed=FILLED)])
        self.assertEqual(entries["composed"]["notice_provenance"],
                         f"Decided by review. composed-1.txt: {FILLED} Standard text: {STANDARD}")
        self.assertEqual((self.output / "notices/composed-1.0.0/composed-1.txt").read_bytes(), b"composed text\n")
        # A fetched text keeps its record and its upstream name, and gains no provenance.
        self.assertEqual(entries["fetched"]["notices"], [dict(
            path="notices/fetched-1.0.0/upstream-1.txt", sha256=hashlib.sha256(b"upstream text\n").hexdigest(),
            url=FETCHED)])
        self.assertNotIn("notice_provenance", entries["fetched"])

    def test_supplement_without_exactly_one_recorded_source_is_refused(self):
        cases = {"neither source": {}, "what was filled in alone": dict(composed=FILLED),
                 "standard text alone": dict(standard_text=STANDARD), "blank url": dict(url=" "),
                 "fetched and composed": dict(url=FETCHED, standard_text=STANDARD, composed=FILLED)}
        for case, record in cases.items():
            with self.subTest(case=case):
                _, supplement = self.supplement("fixture", b"some text\n", **record)
                result = self.collect([supplement], ["fixture"])
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertIn("needs either a fetch url or a composed record: fixture-1.0.0", result.stderr)
                self.assertFalse(self.output.exists())
        # A record for a package this target does not select is checked as well.
        _, supplement = self.supplement("elsewhere", b"some text\n")
        result = self.collect([supplement], ["fixture"])
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("elsewhere-1.0.0", result.stderr)
        self.assertFalse(self.output.exists())

    def test_changed_composed_text_is_refused(self):
        path, supplement = self.supplement("fixture", b"composed text\n", standard_text=STANDARD, composed=FILLED)
        path.write_bytes(b"composed text, edited\n")
        result = self.collect([supplement], ["fixture"])
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("Supplemental notice checksum changed: fixture-1.0.0", result.stderr)
        self.assertFalse((self.output / "dependencies.json").exists())


if __name__ == "__main__":
    unittest.main()
