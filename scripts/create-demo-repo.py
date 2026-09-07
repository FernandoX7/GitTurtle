#!/usr/bin/env python3
"""Create an isolated, deterministic GitTurtle native-UI QA repository.

Defaults to .local/demo-repository plus .local/demo-repository-worktree.
Both destinations must be absent or empty. Existing repositories are always
refused; this tool never deletes, resets, cleans, fetches, or overwrites one.
Only Python's standard library and a locally installed Git are required.

The generated PNGs are synthetic colored test panels, not product artwork.
"""

from __future__ import annotations

import argparse
from datetime import datetime, timedelta, timezone
import hashlib
import json
import os
from pathlib import Path
import stat
import struct
import subprocess
import sys
import zlib


PROJECT_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_OUTPUT = PROJECT_ROOT / ".local" / "demo-repository"
AUTHORS = (
    ("Maya Chen", "maya@aurora.example.invalid"),
    ("Alex Rivera", "alex@aurora.example.invalid"),
    ("Sam Patel", "sam@aurora.example.invalid"),
)


def require_empty(path: Path) -> None:
    """Check both destinations before creating either; never follow a target link."""
    if path.is_symlink():
        raise ValueError(f"Refusing symlink destination: {path}")
    if path.exists() and (not path.is_dir() or next(path.iterdir(), None) is not None):
        raise ValueError(f"Refusing nonempty destination (including existing repositories): {path}")


def png_panel(accent: tuple[int, int, int], alternate: bool = False) -> bytes:
    """A 720x440 RGBA test image with transparent padding and visible changes."""
    width, height = 720, 440
    rows = bytearray()
    for y in range(height):
        rows.append(0)  # PNG filter: None
        for x in range(width):
            if x < 28 or x >= width - 28 or y < 28 or y >= height - 28:
                pixel = (0, 0, 0, 0)
            elif x < 185:
                pixel = (24, 31, 47, 255)
                if 48 <= x < 165 and 90 <= y < 112:
                    pixel = (*accent, 220)
                elif 48 <= x < 150 and any(a <= y < a + 7 for a in (142, 180, 218)):
                    pixel = (84, 102, 130, 255)
            else:
                pixel = (239, 243, 250, 255)
                if 210 <= x < 656 and 55 <= y < 74:
                    pixel = (46, 60, 83, 255)
                elif 210 <= x < 525 and 88 <= y < 96:
                    pixel = (148, 161, 181, 255)
                card_x = 213 if not alternate else 242
                if card_x <= x < card_x + 185 and 133 <= y < 271:
                    pixel = (*accent, 205 if alternate else 255)
                elif 422 <= x < 657 and 133 <= y < 271:
                    pixel = (216, 226, 239, 255)
                if 214 <= x < 653 and 307 <= y < 322:
                    pixel = (*accent, 160)
                elif 214 <= x < (574 if alternate else 505) and 341 <= y < 350:
                    pixel = (123, 142, 168, 255)
            rows.extend(pixel)

    def chunk(name: bytes, data: bytes) -> bytes:
        return struct.pack(">I", len(data)) + name + data + struct.pack(">I", zlib.crc32(name + data) & 0xFFFFFFFF)

    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(bytes(rows), 9))
        + chunk(b"IEND", b"")
    )


class Demo:
    def __init__(self, path: Path) -> None:
        self.path = path
        self.index = 0
        self.commits: dict[str, dict[str, object]] = {}
        # Ignore ambient Git overrides/configuration, hooks, filters and traces.
        self.environment = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        self.environment.update({
            "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_CONFIG_GLOBAL": os.devnull,
            "GIT_TERMINAL_PROMPT": "0",
            "GIT_OPTIONAL_LOCKS": "0",
            "GIT_NO_LAZY_FETCH": "1",
            "LC_ALL": "C",
        })

    def git(self, *args: str, environment: dict[str, str] | None = None) -> str:
        command = [
            "git", "--no-pager", "-C", str(self.path),
            "-c", f"core.hooksPath={os.devnull}",
            "-c", "core.fsmonitor=false", "-c", "protocol.allow=never",
            "-c", "gc.auto=0", "-c", "maintenance.auto=false",
            "-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false",
            *args,
        ]
        result = subprocess.run(command, env=environment or self.environment, capture_output=True, timeout=30)
        if result.returncode:
            raise RuntimeError(f"Fixture Git command failed: {args!r}\n{result.stderr.decode('utf-8', 'replace')}")
        return result.stdout.decode("utf-8").rstrip("\n")

    def write(self, name: str, contents: str | bytes) -> None:
        path = self.path / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(contents.encode("utf-8") if isinstance(contents, str) else contents)

    def commit(self, key: str, subject: str, body: str, merge: str | None = None) -> str:
        author, email = AUTHORS[self.index % len(AUTHORS)]
        instant = datetime(2026, 9, 1, 15, 0, tzinfo=timezone.utc) + timedelta(minutes=self.index * 4)
        environment = self.environment | {
            "GIT_AUTHOR_NAME": author, "GIT_AUTHOR_EMAIL": email,
            "GIT_COMMITTER_NAME": author, "GIT_COMMITTER_EMAIL": email,
            "GIT_AUTHOR_DATE": instant.isoformat(), "GIT_COMMITTER_DATE": instant.isoformat(),
        }
        if merge:
            self.git("merge", "--no-ff", merge, "-m", subject, "-m", body, environment=environment)
        else:
            self.git("add", "--all")
            self.git("commit", "-m", subject, "-m", body, environment=environment)
        oid = self.git("rev-parse", "HEAD")
        parents = self.git("show", "--no-patch", "--format=%P", oid).split()
        self.commits[key] = {"oid": oid, "subject": subject, "parents": parents}
        self.index += 1
        return oid


def create(output: Path, worktree: Path) -> dict[str, object]:
    require_empty(output)
    require_empty(worktree)
    if output == worktree or output in worktree.parents or worktree in output.parents:
        raise ValueError("Repository and linked worktree destinations must be separate sibling locations")
    output.mkdir(parents=True, exist_ok=True)
    # Exclusive ownership marker prevents a second generator from claiming this
    # directory. Nothing is removed if generation fails; reruns then refuse it.
    marker = output / ".gitturtle-demo-fixture"
    with marker.open("x", encoding="utf-8") as handle:
        handle.write("Disposable GitTurtle demo fixture. Created by scripts/create-demo-repo.py.\n")
    demo = Demo(output)
    demo.git("init", "--template=", "--object-format=sha1", "--initial-branch=main")
    demo.git("config", "core.fileMode", "true")
    demo.write(".gitignore", ".gitturtle-demo-fixture\n.fixture-manifest.json\nnode_modules/\ndist/\n")
    demo.write("README.md", """# Aurora UI

A tiny, fictional workspace dashboard used to inspect GitTurtle's native UI.
All files are synthetic test fixtures; the colored PNG panels are not artwork.

The fixture includes text, code, images, transparent pixels, SVG, binary data,
file renames, permission changes, a symlink, and a deliberately missing LFS asset.
""")
    demo.write("package.json", '{\n  "name": "aurora-ui-demo",\n  "private": true,\n  "version": "0.1.0",\n  "scripts": { "preview": "./scripts/preview.sh" }\n}\n')
    demo.write("src/App.tsx", """import './theme.css';

const projects = ['Design system', 'Launch notes', 'Customer research'];

export function App() {
  return (
    <main className="workspace">
      <aside className="sidebar" aria-label="Workspace navigation">
        <h1>Aurora</h1>
        <a href="#overview">Overview</a>
        <a href="#projects">Projects</a>
      </aside>
      <section id="overview" className="dashboard">
        <h2>A clearer view of your work</h2>
        <p>Everything your team needs, in one quiet place.</p>
        <div className="project-grid">
          {projects.map(name => <article key={name}>{name}</article>)}
        </div>
      </section>
    </main>
  );
}
""")
    demo.write("src/theme.css", """:root {
  --accent: #3b82f6;
  --surface: #f3f6fb;
  --ink: #243047;
  --space: 20px;
}

body { margin: 0; color: var(--ink); background: var(--surface); }
.workspace { display: grid; grid-template-columns: 220px 1fr; min-height: 100vh; }
.sidebar { padding: 28px; background: #182030; color: white; }
.sidebar a { display: block; color: inherit; padding: 10px 0; }
.dashboard { padding: 40px; }
.project-grid { display: grid; grid-template-columns: repeat(3, 1fr); gap: var(--space); }
.project-grid article { border-radius: 16px; padding: 24px; background: white; }
""")
    demo.write("notes/design.md", """# Workspace design notes

Give the work room to breathe. Keep the sidebar calm and content readable.

- Use generous space between project cards.
- Keep navigation labels short and recognizable.
- Preserve keyboard focus when content refreshes.
- Use an accent color to make the selected destination clear.
""")
    demo.write("scripts/preview.sh", "#!/bin/sh\n# This fixture script is never run by GitTurtle.\nprintf 'Aurora UI fixture preview\\n'\n")
    demo.write("public/panels/overview.png", png_panel((52, 120, 235)))
    demo.write("public/panels/legacy.png", png_panel((220, 64, 82)))
    demo.write("public/icons/orbit.svg", '<svg xmlns="http://www.w3.org/2000/svg" width="240" height="180" viewBox="0 0 240 180">\n  <circle cx="120" cy="90" r="56" fill="none" stroke="#3b82f6" stroke-width="12"/>\n  <circle cx="120" cy="90" r="16" fill="#172033"/>\n</svg>\n')
    demo.write("data/palette.bin", b"AURORA\x00PALETTE\x01\x00\x3b\x82\xf6\xff")
    root = demo.commit("root", "Start Aurora UI with a calm application shell", "Establish readable navigation, a small card grid, and blue synthetic panels.\nThis root commit exercises additions with no previous image or text side.")

    app = (output / "src/App.tsx").read_text()
    demo.write("src/App.tsx", app.replace("'Customer research'", "'Customer research', 'Release checklist'").replace("A clearer view of your work", "Make room for your next idea").replace("Everything your team needs, in one quiet place.", "A thoughtful workspace for projects, people, and progress."))
    theme = (output / "src/theme.css").read_text()
    demo.write("src/theme.css", theme.replace("#3b82f6", "#e34b5f").replace("--space: 20px", "--space: 24px") + "\n.project-grid article:hover { box-shadow: 0 8px 24px #24304714; }\n")
    demo.write("public/panels/overview.png", png_panel((227, 75, 95), alternate=True))
    demo.write("public/panels/activity.png", png_panel((37, 140, 216), alternate=True))
    (output / "public/panels/legacy.png").unlink()
    demo.write("public/icons/orbit.svg", '<svg xmlns="http://www.w3.org/2000/svg" width="240" height="180" viewBox="0 0 240 180">\n  <rect x="38" y="30" width="164" height="120" rx="32" fill="#e34b5f" fill-opacity="0.18"/>\n  <circle cx="120" cy="90" r="48" fill="none" stroke="#e34b5f" stroke-width="12"/>\n  <circle cx="120" cy="90" r="16" fill="#172033"/>\n</svg>\n')
    demo.write("data/palette.bin", b"AURORA\x00PALETTE\x02\x00\xe3\x4b\x5f\xff")
    mixed = demo.commit("mixed_media", "Refine the dashboard and compare updated panel artwork", "Update the overview from blue to warm red, shift its card, and preserve transparent padding.\nAdd an activity image, remove the legacy image, and adjust TSX, CSS, SVG, and binary data together.")

    (output / "docs").mkdir()
    (output / "notes/design.md").rename(output / "docs/workspace-guide.md")
    os.chmod(output / "scripts/preview.sh", stat.S_IRUSR | stat.S_IWUSR | stat.S_IXUSR | stat.S_IRGRP | stat.S_IXGRP | stat.S_IROTH | stat.S_IXOTH)
    (output / "docs/current-theme.css").symlink_to("../src/theme.css")
    demo.commit("rename_and_modes", "Rename the workspace notes and enable the preview script", "Move the design notes without editing their content, add an executable bit, and store a relative symlink.\nThe viewer should read the symlink's Git content rather than follow it.")

    demo.git("checkout", "-b", "feature/activity-feed")
    demo.write("src/ActivityFeed.tsx", """type Activity = { id: string; title: string; time: string; project: string };

const activities: Activity[] = [
  { id: 'a1', title: 'Updated the component library', time: '10:42', project: 'Design system' },
  { id: 'a2', title: 'Reviewed the launch checklist', time: '09:18', project: 'Release' },
  { id: 'a3', title: 'Published interview notes', time: 'Yesterday', project: 'Research' },
];

export function ActivityFeed() {
  return (
    <section aria-labelledby="activity-title" className="activity-feed">
      <h2 id="activity-title">Recent activity</h2>
      <ol>
        {activities.map(activity => (
          <li key={activity.id}>
            <time>{activity.time}</time>
            <a href={`#${activity.id}`}>{activity.title}</a>
            <span>{activity.project}</span>
          </li>
        ))}
      </ol>
    </section>
  );
}
""")
    demo.write("src/activity.css", ".activity-feed ol { list-style: none; padding: 0; }\n.activity-feed li { display: grid; gap: 12px; padding: 18px 0; border-bottom: 1px solid #dbe2ec; }\n.activity-feed time { color: #68778f; font-size: 12px; }\n.activity-feed a { color: #243047; text-decoration: none; }\n")
    demo.write("public/panels/activity-states.png", png_panel((217, 131, 52), alternate=True))
    demo.commit("activity_feed", "Build a readable activity feed with grouped timestamps", "Give each event a readable title, project context, and quiet timestamp.\nInclude a synthetic warm panel for image-addition review.")
    demo.write("src/activity.css", (output / "src/activity.css").read_text() + "\n.activity-feed a:focus-visible { outline: 3px solid #3b82f6; outline-offset: 5px; border-radius: 4px; }\n@media (prefers-reduced-motion: reduce) { .activity-feed * { scroll-behavior: auto; } }\n")
    feature = demo.commit("keyboard_focus", "Add keyboard focus states to the activity feed", "Use a visible focus ring and respect reduced-motion preferences.")

    demo.git("checkout", "main")
    demo.write("docs/interaction.md", """# Interaction review

The sidebar should feel steady when the content changes. Keep selected items
visible and make keyboard navigation predictable.

## Review checklist

- Tab moves through navigation and content in a logical order.
- Focus remains visible against both light and dark surfaces.
- Image zoom and pan stay linked while comparing the two versions.
- An added image has a clearly absent before side.
- A deleted image has a clearly absent after side.
- A missing local LFS object shows its identity and expected size.
- File renames retain the old and new names.
- Merge commits offer both parents for comparison.
""")
    missing_oid = hashlib.sha256(b"Aurora demo image intentionally unavailable locally").hexdigest()
    demo.write("public/lfs/canvas-photo.png", f"version https://git-lfs.github.com/spec/v1\noid sha256:{missing_oid}\nsize 128000\n")
    demo.write(".gitattributes", "public/lfs/*.png filter=lfs diff=lfs merge=lfs -text\n")
    demo.commit("missing_lfs", "Document sidebar spacing and add a pending LFS image", "Add interaction review notes and a valid pointer whose large image is deliberately absent locally.\nNo network endpoint is configured; the fixture requires no downloads.")
    merge = demo.commit("merge", "Merge the activity feed into the Aurora dashboard", "Join the interaction documentation and activity-feed branches.\nCompare parent 1 for the feature additions or parent 2 for the documentation and LFS pointer.", merge="feature/activity-feed")

    demo.git("checkout", "-b", "experiment/compact-navigation", mixed)
    demo.write("src/theme.css", (output / "src/theme.css").read_text().replace("220px 1fr", "176px 1fr").replace("padding: 28px", "padding: 20px"))
    demo.write("public/panels/compact.png", png_panel((64, 157, 128), alternate=True))
    compact = demo.commit("compact_navigation", "Explore a compact sidebar with warm accent colors", "Keep this experiment on its own branch to exercise the commit graph and branch switching.\nThe original dashboard remains available on main.")
    demo.git("checkout", "main")
    demo.git("branch", "release/0.1", merge)
    demo.git("tag", "v0.1-demo", root)
    for name, oid in (("main", merge), ("feature/activity-feed", feature), ("experiment/compact-navigation", compact)):
        demo.git("update-ref", f"refs/remotes/origin/{name}", oid)
    demo.git("symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main")
    demo.git("worktree", "add", "-b", "review/accessibility", str(worktree), merge)
    demo.git("worktree", "lock", "--reason", "Disposable GitTurtle native UI fixture", str(worktree))

    manifest: dict[str, object] = {
        "repository": str(output),
        "linked_worktree": str(worktree),
        "current_branch": "main",
        "commits": demo.commits,
        "recommended_first_commit": mixed,
        "image_cases": {
            "modified": "public/panels/overview.png",
            "added": "public/panels/activity.png",
            "deleted": "public/panels/legacy.png",
            "svg": "public/icons/orbit.svg",
            "missing_lfs": "public/lfs/canvas-photo.png",
        },
        "missing_lfs_oid": missing_oid,
        "notes": [
            "All PNGs are synthetic 720×440 RGBA panels with transparent padding.",
            "origin/* names are local remote-tracking refs; no remote URL or network operation exists.",
            "The linked worktree is intentionally locked to exercise that navigation state.",
            "The generator refuses any nonempty destination; it never rebuilds by deleting an existing repository.",
        ],
    }
    (output / ".fixture-manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    if demo.git("status", "--porcelain"):
        raise RuntimeError("Generated fixture unexpectedly has uncommitted changes")
    return manifest


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT, help="Absent or empty output directory")
    args = parser.parse_args()
    # Preserve the final path component until require_empty checks for symlinks.
    output = args.output.expanduser().absolute()
    worktree = output.with_name(output.name + "-worktree")
    try:
        manifest = create(output, worktree)
    except (OSError, ValueError, RuntimeError, subprocess.TimeoutExpired) as error:
        print(f"Fixture creation refused or failed: {error}", file=sys.stderr)
        return 1
    print(json.dumps(manifest, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
