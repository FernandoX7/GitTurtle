"""Tests for website/visual_qa.py (HTML a11y, CSS contrast/guards, HTTP serving, SSRF & browser guards)."""
from __future__ import annotations

import json
from pathlib import Path
import sys
from tempfile import TemporaryDirectory
import unittest
from unittest import mock

WEBSITE_DIR = Path(__file__).resolve().parent
if str(WEBSITE_DIR) not in sys.path:
    sys.path.insert(0, str(WEBSITE_DIR))

import visual_qa  # noqa: E402


GOOD_CSS = """\
:root {
  --paper: #f6f8f3;
  --ink: #163d31;
  --muted: #50685d;
  --mint: #d0ecdc;
  --accent: #267556;
  --deep: #102c25
}
* { box-sizing: border-box; }
img { max-width: 100%; height: auto; }
:where(a, button):focus-visible { outline: 3px solid #267556; }
@media (prefers-reduced-motion: reduce) { * { animation: none !important; } }
@media (forced-colors: active) { .button { border: 2px solid ButtonText; } }
"""

GOOD_PAGE = """\
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>GitTurtle</title>
  <link rel="stylesheet" href="/styles.css">
</head>
<body>
  <a class="skip-link" href="#main">Skip to content</a>
  <header><nav><a href="/">Home</a></nav></header>
  <main id="main">
    <h1>GitTurtle</h1>
    <h2>History</h2>
    <img src="/assets/shot.png" width="100" height="80" alt="History window">
    <button type="button">Expand</button>
    <svg aria-hidden="true"></svg>
  </main>
  <footer><a href="/">Footer</a></footer>
</body>
</html>
"""


class VisualQaTests(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.root = Path(self._tmp.name)
        self.public = self.root / "public"
        (self.public / "assets").mkdir(parents=True)
        (self.public / "assets" / "shot.png").write_bytes(b"\x89PNG\r\n\x1a\n")
        (self.public / "styles.css").write_text(GOOD_CSS, encoding="utf-8")
        (self.public / "guides.css").write_text(".guide-copy { overflow-wrap: anywhere; }\n", encoding="utf-8")
        (self.public / "index.html").write_text(GOOD_PAGE, encoding="utf-8")
        (self.public / "404.html").write_text(
            '<!doctype html><html lang="en"><head><meta name="viewport" content="width=device-width, initial-scale=1">'
            '<title>404</title></head><body><main><h1>Missing</h1><a href="/">Home</a></main></body></html>',
            encoding="utf-8",
        )
        (self.public / "sitemap.xml").write_text(
            '<?xml version="1.0" encoding="UTF-8"?>\n'
            '<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n'
            '  <url><loc>https://gitturtle.com/</loc></url>\n'
            '</urlset>\n',
            encoding="utf-8",
        )

    def test_clean_fixture_passes_static_and_http_checks(self) -> None:
        code, report = visual_qa.run_qa(self.root)
        self.assertEqual(code, 0, report)
        self.assertEqual(report["pages_checked"], 2)
        self.assertGreaterEqual(report["http_urls_checked"], 3)
        self.assertGreater(report["contrast_ratios"]["primary copy (--ink on --paper)"], 10.0)

    def test_flags_skipped_heading_missing_dimensions_empty_svg_title_and_hidden_link_text(self) -> None:
        bad = (
            GOOD_PAGE.replace("<h2>History</h2>", "<h3>Skipped</h3>")
            .replace('width="100" height="80"', "")
            .replace('<svg aria-hidden="true"></svg>', "<svg><title>   </title></svg>")
            .replace('<a class="skip-link" href="#main">Skip to content</a>', "")
            .replace('<a href="/">Footer</a>', '<a href="/"><svg aria-hidden="true">hidden</svg></a>')
        )
        (self.public / "index.html").write_text(bad, encoding="utf-8")
        code, report = visual_qa.run_qa(self.root, skip_http=True)
        self.assertEqual(code, 1)
        messages = [item["message"] for item in report["findings"]]
        self.assertTrue(any("skipped heading level" in m for m in messages), messages)
        self.assertTrue(any("positive integer width" in m for m in messages), messages)
        self.assertTrue(any("aria-hidden" in m for m in messages), messages)
        self.assertTrue(any("skip-to-content" in m for m in messages), messages)
        self.assertTrue(any("link missing accessible name" in m for m in messages), messages)

    def test_flags_low_contrast_and_commented_out_css_guards(self) -> None:
        bad_css = (
            GOOD_CSS.replace("--muted: #50685d;", "--muted: #dde3dc;")
            .replace(
                "@media (prefers-reduced-motion: reduce) { * { animation: none !important; } }",
                "/* @media (prefers-reduced-motion: reduce) { * { animation: none !important; } } */",
            )
            .replace("img { max-width: 100%; height: auto; }", ".hero-img { max-width: 100%; }")
        )
        (self.public / "styles.css").write_text(bad_css, encoding="utf-8")
        code, report = visual_qa.run_qa(self.root, skip_http=True)
        self.assertEqual(code, 1)
        messages = [item["message"] for item in report["findings"]]
        self.assertTrue(any("secondary copy (--muted on --paper)" in m for m in messages), messages)
        self.assertTrue(any("reduced-motion" in m for m in messages), messages)
        self.assertTrue(any("responsive image max-width" in m for m in messages), messages)

    def test_flags_missing_http_asset_and_img_srcset(self) -> None:
        (self.public / "index.html").write_text(
            GOOD_PAGE.replace(
                'src="/assets/shot.png"',
                'src="/assets/shot.png" srcset="/assets/missing-2x.png 2x"',
            ),
            encoding="utf-8",
        )
        code, report = visual_qa.run_qa(self.root)
        self.assertEqual(code, 1)
        self.assertTrue(any("/assets/missing-2x.png" in item["scope"] for item in report["findings"]))

    def test_rejects_ssrf_and_non_root_relative_routes(self) -> None:
        for bad_route in ("@attacker.invalid/pwn", "docs/staging", "//evil.example/", "/a?b=1", "/a\\b"):
            with self.subTest(bad_route=bad_route):
                self.assertIsNotNone(visual_qa.validate_local_path(bad_route))
                code, report = visual_qa.run_qa(self.root, routes=[bad_route])
                self.assertEqual(code, 1)
                self.assertTrue(any(bad_route == item["scope"] for item in report["findings"]))

    def test_missing_playwright_and_config_errors_behave_distinctly(self) -> None:
        with mock.patch.object(
            visual_qa,
            "run_browser_qa",
            side_effect=visual_qa.BrowserUnavailableError("Playwright is not installed"),
        ):
            code_strict, report_strict = visual_qa.run_qa(self.root, browser=True, skip_http=True)
            self.assertEqual(code_strict, 2)
            self.assertIn("Playwright", report_strict["error"])

            code_opt, report_opt = visual_qa.run_qa(self.root, browser_if_available=True, skip_http=True)
            self.assertEqual(code_opt, 0)
            self.assertFalse(report_opt["browser"]["ran"])
            self.assertIn("Playwright", report_opt["browser"]["skipped"])

        # Missing --axe-script or symlinked --out must fail loudly (exit 2) even under --browser-if-available.
        code_bad_axe, report_bad_axe = visual_qa.run_qa(
            self.root,
            browser_if_available=True,
            axe_script=self.root / "nonexistent-axe.js",
            skip_http=True,
        )
        self.assertEqual(code_bad_axe, 2)
        self.assertIn("axe-core", report_bad_axe["error"])

        real_dir = self.root / "real_out"
        real_dir.mkdir()
        link_out = self.root / "link_out"
        link_out.symlink_to(real_dir)
        code_link, report_link = visual_qa.run_qa(
            self.root,
            browser_if_available=True,
            out_dir=link_out,
            skip_http=True,
        )
        self.assertEqual(code_link, 2)
        self.assertIn("symlinked", report_link["error"])

    def test_live_repository_website_passes_visual_qa(self) -> None:
        code, report = visual_qa.run_qa(WEBSITE_DIR)
        self.assertEqual(code, 0, json.dumps(report, indent=2))


if __name__ == "__main__":
    unittest.main()
