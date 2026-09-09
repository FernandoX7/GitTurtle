#!/usr/bin/env python3
"""Create disposable GIF/JPEG 2000 native QA history. Requires Pillow."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess

from PIL import Image, ImageDraw

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("destination", type=Path)
args = parser.parse_args()
destination = args.destination.expanduser().absolute()
checkout = Path(__file__).resolve().parents[1]
if destination.exists() or destination == checkout or checkout in destination.parents:
    raise SystemExit("Choose a new disposable directory outside the checkout")
destination.mkdir(parents=True)
env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL="/dev/null")


def git(*arguments):
    return subprocess.check_output(
        ["git", "-C", str(destination), *arguments], env=env, text=True
    ).strip()


def animation(path, label, color, frames=12):
    images = []
    for index in range(frames):
        image = Image.new("RGBA", (320, 180), (238, 241, 247, 255))
        draw = ImageDraw.Draw(image)
        x = 12 + index * 22
        draw.rectangle((x, 62, x + 44, 125), fill=color)
        draw.text((12, 18), f"{label} FRAME {index + 1:02}/{frames:02}", fill=(20, 25, 35, 255), font_size=18)
        images.append(image)
    images[0].save(path, save_all=True, append_images=images[1:], duration=100, loop=0, disposal=2)


git("init", "-b", "main")
(destination / ".git/qa-empty-hooks").mkdir()
for key, value in [("user.name", "Animation Fixture"), ("user.email", "animation@example.invalid"), ("commit.gpgSign", "false"), ("tag.gpgSign", "false"), ("core.hooksPath", ".git/qa-empty-hooks")]:
    git("config", key, value)
animation(destination / "moving.gif", "BEFORE", (214, 70, 55, 255), 8)
for suffix in ["jp2", "j2k"]:
    shutil.copyfile(checkout / f"crates/preview/tests/fixtures/half-red-blue.{suffix}", destination / f"pattern.{suffix}")
git("add", ".")
git("commit", "-m", "Original animated GIF and JPEG 2000")
before = git("rev-parse", "HEAD")
animation(destination / "moving.gif", "AFTER", (30, 115, 225, 255), 12)
git("add", "moving.gif")
git("commit", "-m", "Longer animation for shared-time comparison")
after = git("rev-parse", "HEAD")
animation(destination / "working.gif", "WORKING", (30, 145, 90, 255), 10)
(destination / "corrupt.gif").write_bytes(b"GIF89a\x01\x00")
manifest = {"repository": str(destination), "before": before, "after": after,
            "native_cases": ["Open HEAD moving.gif; initially frame1 paused", "Play and pause; Before stops on frame8 while After reaches12", "Next/Previous/First frame and Overlay/Wipe", "Back/Projects during play: no hidden frame demand", "Quick Open working.gif playback", "Quick Open pattern.jp2 and pattern.j2k: red top / blue bottom", "corrupt.gif: explicit decoder error"]}
print(json.dumps(manifest, indent=2))
