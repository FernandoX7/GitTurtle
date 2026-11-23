"""Structural validation for packaging/flatpak manifest and AppStream metainfo."""
from __future__ import annotations

from pathlib import Path
import unittest
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[3]
FLATPAK_DIR = ROOT / "packaging/flatpak"
MANIFEST = FLATPAK_DIR / "com.gitturtle.GitTurtle.yml"
METAINFO = FLATPAK_DIR / "com.gitturtle.GitTurtle.metainfo.xml"
APP_ID = "com.gitturtle.GitTurtle"


class FlatpakManifestTests(unittest.TestCase):
    def test_manifest_has_required_id_permissions_and_unreleased_guard(self):
        text = MANIFEST.read_text(encoding="utf-8")
        for expected in (
            f"app-id: {APP_ID}",
            "command: gitturtle",
            "--socket=wayland",
            "--socket=fallback-x11",
            "--device=dri",
            "--talk-name=org.freedesktop.secrets",
            "SingleMainWindow=true",
            "Keywords=Git;VersionControl;VCS;Repository;Commit;Diff;Branch;PR;Merge;",
            "only-arches: [x86_64]",
            "only-arches: [aarch64]",
            "sha256: not-released",
        ):
            self.assertIn(expected, text, expected)

    def test_metainfo_xml_is_valid_and_matches_app_id(self):
        tree = ET.parse(METAINFO)
        root = tree.getroot()
        self.assertEqual(root.tag, "component")
        self.assertEqual(root.attrib.get("type"), "desktop-application")
        self.assertEqual(root.findtext("id"), APP_ID)
        self.assertEqual(root.findtext("project_license"), "MIT")
        self.assertEqual(root.findtext("metadata_license"), "CC0-1.0")
        self.assertEqual(root.findtext("name"), "GitTurtle")
        launchable = root.find("launchable")
        self.assertIsNotNone(launchable)
        self.assertEqual(launchable.attrib.get("type"), "desktop-id")
        self.assertEqual(launchable.text, f"{APP_ID}.desktop")
        rating = root.find("content_rating")
        self.assertIsNotNone(rating)
        self.assertEqual(rating.attrib.get("type"), "oars-1.1")


if __name__ == "__main__":
    unittest.main()
