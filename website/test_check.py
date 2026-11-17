"""Regression fixtures for nested static pages and the generated sitemap."""
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from check import ORIGIN, REPOSITORY, check_site


class PageChecks(unittest.TestCase):
    def setUp(self):
        directory = TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.public = self.root / "public"
        self.public.mkdir()
        (self.root / "asset-provenance.json").write_text('{"assets": []}')
        (self.public / "_headers").write_text("/*\n  X-Content-Type-Options: nosniff\n")
        (self.public / "robots.txt").write_text(f"User-agent: *\nAllow: /\nSitemap: {ORIGIN}/sitemap.xml\n")
        (self.public / "404.html").write_text('<meta name="robots" content="noindex"><a href="/">Home</a>')
        self.home = self.page("/", "Home", '<a href="/docs/">Guides</a>')
        self.guides = self.page("/docs/", "Guides", '<a href="install/#mac">Mac</a>')
        self.install = self.page("/docs/install/", "Install", f'<h1 id="mac">Mac</h1><a href="{REPOSITORY}">Source</a>')
        self.assertEqual(check_site(self.root, write_sitemap=True), [])

    def page(self, route, title, body):
        path = self.public / route.strip("/") / "index.html"
        path.parent.mkdir(parents=True, exist_ok=True)
        schema = ('<script type="application/ld+json">{"@context":"https://schema.org",'
                  '"@type":"SoftwareApplication","name":"GitTurtle","url":"https://gitturtle.com/"}</script>')
        path.write_text(f'<title>{title}</title><meta name="description" content="{title} information">'
                        f'<link rel="canonical" href="{ORIGIN}{route}">{schema if route == "/" else ""}{body}')
        return path

    def test_sitemap_tracks_added_and_removed_pages(self):
        sitemap = self.public / "sitemap.xml"
        self.assertIn(f"<loc>{ORIGIN}/docs/install/</loc>", sitemap.read_text())
        self.assertNotIn("404", sitemap.read_text())
        self.install.unlink()
        errors = check_site(self.root)
        self.assertTrue(any("docs/index.html: missing target install/#mac" in issue for issue in errors))
        self.assertTrue(any("sitemap.xml" in issue for issue in errors))
        self.guides.write_text(self.guides.read_text().replace('href="install/#mac"', 'href="/"'))
        self.assertEqual(check_site(self.root, write_sitemap=True), [])
        self.assertNotIn("/docs/install/", sitemap.read_text())

    def test_sitemap_and_canonicals_do_not_make_orphans_reachable(self):
        self.home.write_text(self.home.read_text().replace('<a href="/docs/">Guides</a>', '<link href="/docs/">'))
        self.install.write_text(self.install.read_text() + '<a href="../">Guides</a>')
        errors = check_site(self.root)
        self.assertEqual(sum("no hyperlink path" in issue for issue in errors), 2)

    def test_homepage_schema_and_nested_page_metadata(self):
        self.home.write_text(self.home.read_text().replace('"@type":', '"@type"'))
        self.install.write_text(self.install.read_text().replace("/docs/install/", "/docs/")
                                .replace("Install information", "Guides information"))
        errors = check_site(self.root)
        self.assertTrue(any("invalid JSON-LD" in issue for issue in errors))
        self.assertTrue(any("expected one canonical URL" in issue for issue in errors))
        self.assertTrue(any("same description" in issue for issue in errors))

    def test_search_bot_override_blocks_guides(self):
        robots = self.public / "robots.txt"
        robots.write_text(robots.read_text() + "\nUser-agent: OAI-SearchBot\nDisallow: /docs/\n")
        errors = check_site(self.root)
        self.assertCountEqual(errors, [
            f"robots.txt: blocks OAI-SearchBot from indexable page {ORIGIN}/docs/",
            f"robots.txt: blocks OAI-SearchBot from indexable page {ORIGIN}/docs/install/",
        ])

    def test_missing_responsive_image(self):
        self.guides.write_text(self.guides.read_text() + '<picture><source srcset="/missing.webp"></picture>')
        self.assertTrue(any("missing target /missing.webp" in issue for issue in check_site(self.root)))

    def test_same_site_absolute_fragments_and_github_asset_links(self):
        self.guides.write_text(self.guides.read_text().replace("install/#mac", f"{ORIGIN}/docs/install/#missing")
                               + '<a href="https://raw.githubusercontent.com/FernandoX7/GitTurtle/main/README.md">Read</a>')
        errors = check_site(self.root)
        self.assertTrue(any("missing anchor" in issue for issue in errors))
        self.assertTrue(any("GitHub link" in issue for issue in errors))


if __name__ == "__main__":
    unittest.main()
