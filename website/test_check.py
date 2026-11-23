"""Regression fixtures for nested static pages and the generated sitemap."""
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from check import ORIGIN, REPOSITORY, allowed_github_url, check_site


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

    def test_sharing_image_is_first_party_and_exists(self):
        self.home.write_text(self.home.read_text() + '<meta property="og:image" content="/social.png">'
                             + f'<meta name="twitter:image" content="{ORIGIN}/missing.png">')
        errors = check_site(self.root)
        self.assertTrue(any("absolute first-party HTTPS URL" in issue for issue in errors))
        self.assertTrue(any(f"missing target {ORIGIN}/missing.png" in issue for issue in errors))

    def test_missing_responsive_image(self):
        self.guides.write_text(self.guides.read_text() + '<picture><source srcset="/missing.webp"></picture>')
        self.assertTrue(any("missing target /missing.webp" in issue for issue in check_site(self.root)))

    def test_same_site_absolute_fragments_and_github_asset_links(self):
        self.guides.write_text(self.guides.read_text().replace("install/#mac", f"{ORIGIN}/docs/install/#missing")
                               + '<a href="https://raw.githubusercontent.com/FernandoX7/GitTurtle/main/README.md">Read</a>')
        errors = check_site(self.root)
        self.assertTrue(any("missing anchor" in issue for issue in errors))
        self.assertTrue(any("GitHub link" in issue for issue in errors))

    def test_orphaned_public_asset_is_rejected(self):
        assets = self.public / "assets"
        assets.mkdir()
        (assets / "unused.png").write_bytes(b"\x89PNG\r\n\x1a\n")
        errors = check_site(self.root)
        self.assertTrue(any("orphaned public asset" in issue and "unused.png" in issue for issue in errors))

    def test_canonical_github_release_urls_allowed_and_smuggling_rejected(self):
        allowed = (
            REPOSITORY,
            f"{REPOSITORY}/releases",
            f"{REPOSITORY}/releases/latest",
            f"{REPOSITORY}/releases/tag/v0.1.0",
            f"{REPOSITORY}/releases/tag/v0.1.0-rc.1",
            f"{REPOSITORY}/releases/download/v0.1.0/GitTurtle-0.1.0-abc123def456-linux-x86_64-release.tar.gz",
            f"{REPOSITORY}/releases/download/v0.1.0-beta.2/SHA256SUMS",
        )
        for url in allowed:
            with self.subTest(allowed=url):
                self.assertTrue(allowed_github_url(url))

        rejected = (
            f"{REPOSITORY}/issues",
            f"{REPOSITORY}/blob/main/README.md",
            f"{REPOSITORY}/releases/",
            f"{REPOSITORY}/releases/tag/latest",
            f"{REPOSITORY}/releases/tag/v0.1.0-rc..1",
            f"{REPOSITORY}/releases/download/v0.1.0/sub/file.tar.gz",
            f"{REPOSITORY}/releases/download/v0.1.0/..%2Fsecret",
            f"{REPOSITORY}/releases/download/v0.1.0/file..tar.gz",
            f"{REPOSITORY}/releases/download/v0.1.0/.hidden",
            f"{REPOSITORY}/releases/download/v0.1.0/file.tar.gz?download=1",
            f"{REPOSITORY}/releases?",
            f"{REPOSITORY}/releases#top",
            f"http://github.com/FernandoX7/GitTurtle/releases",
            f"https://github.com:443/FernandoX7/GitTurtle/releases",
            f"https://user@github.com/FernandoX7/GitTurtle/releases",
            f"https://github.com./FernandoX7/GitTurtle/releases",
            f"https://raw.githubusercontent.com/FernandoX7/GitTurtle/main/README.md",
            f"https://github.com/OtherOwner/GitTurtle/releases/latest",
            f"https://github.com/FernandoX7/GitTurtle/releases\\latest",
            f"https://github.com/FernandoX7/GitTurtle//releases",
            f"https://github.com/FernandoX7/GitTurtle/releases/%6catest",
            "https://github.com/FernandoX7/GitTurtle/releases/tag/v\uff10.\uff11.\uff10",
            "https://github.com/FernandoX7/GitTurtle/releases/tag/v\u0660.\u0661.\u0660",
            "https://github.com\\OtherOwner/Evil",
            "http:github.com/OtherOwner/Evil",
            "https://github\u3002com/OtherOwner/Evil",
            "https://\uff47ithub.com/OtherOwner/Evil",
        )
        for url in rejected:
            with self.subTest(rejected=url):
                self.assertFalse(allowed_github_url(url))

        # Integration through check_site: canonical release links pass; arbitrary and parser-bypass GitHub links fail.
        self.install.write_text(
            self.install.read_text()
            + f'<a href="{REPOSITORY}/releases/tag/v0.1.0">Release</a>'
            + f'<a href="{REPOSITORY}/releases/download/v0.1.0/SHA256SUMS">Checksums</a>'
        )
        self.assertEqual(check_site(self.root), [])
        for bad_href in (
            f"{REPOSITORY}/releases/tag/v0.1.0?ref=1",
            "https://github.com\\OtherOwner/Evil",
            "http:github.com/OtherOwner/Evil",
            "https://github\u3002com/OtherOwner/Evil",
            "https://\uff47ithub.com/OtherOwner/Evil",
        ):
            with self.subTest(bad_href=bad_href):
                original = self.install.read_text()
                self.install.write_text(original + f'<a href="{bad_href}">Bad</a>')
                errors = check_site(self.root)
                self.install.write_text(original)
                self.assertTrue(any("GitHub link" in issue for issue in errors), errors)


if __name__ == "__main__":
    unittest.main()
