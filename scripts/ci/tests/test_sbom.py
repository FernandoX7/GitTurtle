"""Tests for deterministic CycloneDX 1.5 SBOM generation."""
from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location("generate_sbom", ROOT / "scripts/generate-sbom.py")
sbom_mod = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(sbom_mod)


class SbomTests(unittest.TestCase):
    def test_generates_deterministic_cyclonedx_from_workspace_lockfile(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out1 = Path(tmp) / "a.cdx.json"
            out2 = Path(tmp) / "b.cdx.json"
            doc1 = sbom_mod.write_sbom(out1, ROOT, version="0.1.0", commit="a" * 40)
            sbom_mod.write_sbom(out2, ROOT, version="0.1.0", commit="a" * 40)
            self.assertEqual(out1.read_bytes(), out2.read_bytes())
            self.assertEqual(doc1["bomFormat"], "CycloneDX")
            self.assertEqual(doc1["specVersion"], "1.5")
            self.assertGreater(len(doc1["components"]), 100)
            names = {comp["name"] for comp in doc1["components"]}
            self.assertIn("gitturtle", names)
            self.assertIn("anyhow", names)
            parsed = json.loads(out1.read_text(encoding="utf-8"))
            self.assertEqual(parsed["metadata"]["component"]["version"], "0.1.0")
            self.assertEqual(parsed["metadata"]["properties"][0]["value"], "a" * 40)


if __name__ == "__main__":
    unittest.main()
