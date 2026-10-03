from __future__ import annotations

import base64
import importlib.util
import lzma
import os
import random
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from native_qa import frames, privacy

HAVE_PIL = importlib.util.find_spec("PIL") is not None
HAVE_CC = any(shutil.which(name) for name in ("cc", "gcc", "clang"))
REPO = Path(__file__).resolve().parents[2]
QA = Path(__file__).resolve().parent / "qa.py"


def synthetic(width: int = 90, height: int = 40, at: tuple[int, int] = (30, 12), invert: bool = False):
    """A flat frame with a seeded noise patch standing in for a rendered string, and that patch."""
    rng = random.Random(7)
    tw, th = 16, 8
    patch = bytes(rng.randrange(256) for _ in range(tw * th))
    frame = bytearray([40] * (width * height))
    for y in range(th):
        for x in range(tw):
            value = patch[y * tw + x]
            frame[(at[1] + y) * width + at[0] + x] = 255 - value if invert else value
    return bytes(frame), width, height, (patch, tw, th)


class EngineTest(unittest.TestCase):
    def test_python_engine_finds_the_patch(self) -> None:
        frame, width, height, template = synthetic()
        result = privacy.ncc_python(frame, width, height, *template)
        self.assertEqual((result["best"], result["at"], result["sign"], result["hits"]), (1.0, [30, 12], "+", 1))
        self.assertEqual(result["hit_list"], [[30, 12, 1.0]])

    def test_inverted_rendering_matches_with_a_negative_sign(self) -> None:
        frame, width, height, template = synthetic(invert=True)
        result = privacy.ncc_python(frame, width, height, *template)
        self.assertEqual((result["best"], result["at"], result["sign"], result["hits"]), (1.0, [30, 12], "-", 1))

    def test_flat_frame_and_flat_template_never_match(self) -> None:
        _, width, height, template = synthetic()
        flat = bytes([40] * (width * height))
        self.assertEqual(privacy.ncc_python(flat, width, height, *template)["hits"], 0)
        frame, *_ = synthetic()
        blank = (bytes([9] * 128), 16, 8)
        self.assertEqual(privacy.ncc_python(frame, width, height, *blank)["best"], 0.0)

    def test_parse_ncc_output(self) -> None:
        text = "tpl 0 best 0.9912 30 12 sign + hits 1\nhit 30 12 0.9912\ntpl 1 best 0.2100 4 5 sign - hits 0\n"
        parsed = privacy.parse_ncc_output(text, ["a", "b"])
        self.assertEqual(parsed["a"], dict(best=0.9912, at=[30, 12], sign="+", hits=1, hit_list=[[30, 12, 0.9912]]))
        self.assertEqual(parsed["b"]["hits"], 0)
        with self.assertRaises(RuntimeError):
            privacy.parse_ncc_output(text, ["a", "b", "c"])

    @unittest.skipUnless(HAVE_CC, "no C compiler")
    def test_c_helper_agrees_with_python_and_builds_outside_the_repository(self) -> None:
        with tempfile.TemporaryDirectory() as cache, mock.patch.dict(os.environ, {"XDG_CACHE_HOME": cache}):
            binary = privacy.helper()
            self.assertTrue(binary.is_relative_to(Path(cache)))
            self.assertEqual(privacy.helper(build=False), binary)  # cached per source digest
            for invert in (False, True):
                frame, width, height, template = synthetic(invert=invert)
                ours = privacy.ncc_python(frame, width, height, *template)
                theirs = privacy.ncc_c(binary, frame, width, height, {"t": template}, privacy.THRESHOLD)["t"]
                self.assertEqual((theirs["at"], theirs["sign"], theirs["hits"]), (ours["at"], ours["sign"], ours["hits"]))
                self.assertAlmostEqual(theirs["best"], ours["best"], places=3)


class PackTest(unittest.TestCase):
    def test_round_trip_keeps_pixels_in_name_order_and_drops_names(self) -> None:
        _, _, _, template = synthetic()
        wide = (bytes(range(24)), 6, 4)
        packed = privacy.pack_templates({"QA-TEMPLATE-b": wide, "QA-TEMPLATE-a": template})
        self.assertNotIn("QA-TEMPLATE", base64.b64decode(packed).decode("latin-1"))
        self.assertEqual(privacy.unpack_templates(packed[:40] + "\n" + packed[40:]), {"t1": template, "t2": wide})

    def test_malformed_or_unbounded_packs_are_refused(self) -> None:
        good = base64.b64decode(privacy.pack_templates({"a": (bytes(12), 4, 3)}))
        header = privacy.PACK_MAGIC
        cases = {
            "not base64": "%%%",
            "unknown format": base64.b64encode(b"GTPRIV0\n" + good[len(header):]).decode(),
            "truncated stream": base64.b64encode(good[:-8]).decode(),
            "trailing data": base64.b64encode(good + b"x").decode(),
            "short record": base64.b64encode(header + lzma.compress(b"\x00\x04\x00\x03" + bytes(5))).decode(),
            "zero width": base64.b64encode(header + lzma.compress(b"\x00\x00\x00\x03")).decode(),
            "oversized side": base64.b64encode(header + lzma.compress(b"\x10\x00\x00\x01" + bytes(4096))).decode(),
            "empty": base64.b64encode(header + lzma.compress(b"")).decode(),
        }
        for name, text in cases.items():
            with self.subTest(name), self.assertRaises(SystemExit) as refused:
                privacy.unpack_templates(text)
            self.assertEqual(str(refused.exception), "the packed templates are malformed")

    def test_the_real_limit_fits_a_github_secret(self) -> None:
        self.assertLessEqual(privacy.SECRET_LIMIT, 48 * 1000)


class HelperFailureTest(unittest.TestCase):
    def test_a_failing_helper_reports_fixed_text_without_its_arguments(self) -> None:
        frame, width, height, template = synthetic()
        with tempfile.TemporaryDirectory() as scratch:
            fake = Path(scratch) / "ncc"
            fake.write_text("#!/bin/sh\necho \"$@\" >&2\nexit 3\n")
            fake.chmod(0o755)
            with self.assertRaises(SystemExit) as failed:
                privacy.ncc_c(fake, frame, width, height, {"QA-TEMPLATE": template}, privacy.THRESHOLD)
        self.assertEqual(str(failed.exception), "the ncc helper failed with exit status 3")


class TrackedPathTest(unittest.TestCase):
    def test_templates_inside_the_work_tree_must_be_ignored(self) -> None:
        if not (REPO / ".git").exists():
            self.skipTest("not a Git work tree")
        with self.assertRaises(SystemExit):
            privacy.refuse_tracked(REPO / "scripts" / "native_qa" / "templates", "template directory")
        privacy.refuse_tracked(REPO / ".local" / "privacy-templates", "template directory")  # ignored
        with tempfile.TemporaryDirectory() as scratch:
            privacy.refuse_tracked(Path(scratch) / "tpl", "template directory")  # outside any work tree


@unittest.skipUnless(HAVE_PIL, "Pillow is not installed")
class ScanTest(unittest.TestCase):
    def test_scan_and_cli_on_synthetic_frames(self) -> None:
        from PIL import Image

        frame, width, height, (patch, tw, th) = synthetic()
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            (root / "tpl").mkdir()
            Image.frombytes("L", (tw, th), patch).save(root / "tpl" / "name.png")
            Image.frombytes("L", (width, height), frame).convert("RGB").save(root / "leaky.png")
            Image.new("RGB", (width, height), (40, 40, 40)).save(root / "clean.png")
            templates = privacy.load_templates(root / "tpl")
            report = privacy.scan([root / "leaky.png", root / "clean.png"], templates, engine="python")
            self.assertEqual(report[str(root / "leaky.png")]["templates"]["name"]["at"], [30, 12])
            self.assertEqual((report[str(root / "leaky.png")]["hits"], report[str(root / "clean.png")]["hits"]), (1, 0))

            run = [sys.executable, "-B", str(QA), "privacy", "scan", "--templates", str(root / "tpl"), "--engine", "python"]
            leaky = subprocess.run(run + [str(root / "leaky.png")], capture_output=True, text=True)
            self.assertEqual(leaky.returncode, 1, leaky.stderr)
            self.assertIn("HIT name at (30,12)", leaky.stdout)
            clean = subprocess.run(run + [str(root / "clean.png")], capture_output=True, text=True)
            self.assertEqual(clean.returncode, 0, clean.stderr)

            privacy.crop(root / "leaky.png", (30, 12, 30 + tw, 12 + th), root / "cut" / "again.png")
            with Image.open(root / "cut" / "again.png") as cut:
                self.assertEqual(cut.tobytes(), patch)

    def test_redacted_scan_and_pack_never_print_a_template_name(self) -> None:
        from PIL import Image

        frame, width, height, (patch, tw, th) = synthetic()
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            (root / "tpl").mkdir()
            Image.frombytes("L", (tw, th), patch).save(root / "tpl" / "QA-TEMPLATE.png")
            Image.frombytes("L", (width, height), frame).convert("RGB").save(root / "leaky.png")
            Image.new("RGB", (width, height), (40, 40, 40)).save(root / "clean.png")
            self.assertEqual(list(privacy.load_templates(root / "tpl", anonymous=True)), ["t1"])

            pack = subprocess.run([sys.executable, "-B", str(QA), "privacy", "pack", "--templates", str(root / "tpl"),
                                   "--output", str(root / "set.b64")], capture_output=True, text=True)
            self.assertEqual(pack.returncode, 0, pack.stderr)
            self.assertEqual((root / "set.b64").stat().st_mode & 0o777, 0o600)
            frames = [str(root / "leaky.png"), str(root / "clean.png")]
            for source in (["--templates", str(root / "tpl")], ["--packed", str(root / "set.b64")]):
                with self.subTest(source=source[0]):
                    run = subprocess.run([sys.executable, "-B", str(QA), "privacy", "scan", "--redacted", "--jobs", "2",
                                          "--engine", "python", *source, *frames], capture_output=True, text=True)
                    self.assertEqual(run.returncode, 1, run.stderr)
                    self.assertEqual(run.stdout.splitlines(), [f"{frames[0]}: MATCH", f"{frames[1]}: clean",
                                                               "privacy scan: 2 image(s), 1 matched a template"])
                    self.assertNotIn("QA-TEMPLATE", run.stdout + run.stderr + pack.stdout + pack.stderr)

            (root / "tpl" / "QA-TEMPLATE-broken.png").write_bytes(b"not an image")
            with self.assertRaises(SystemExit) as refused:
                privacy.load_templates(root / "tpl", anonymous=True)
            self.assertNotIn("QA-TEMPLATE", str(refused.exception))

    def test_a_magnified_crop_is_also_scanned_at_1x(self) -> None:
        from PIL import Image

        frame, width, height, (patch, tw, th) = synthetic()
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            (root / "tpl").mkdir()
            Image.frombytes("L", (tw, th), patch).save(root / "tpl" / "name.png")
            leaky = Image.frombytes("L", (width, height), frame).convert("RGB")
            leaky.save(root / "leaky.png")
            # A scenario's 4x crop around the string: 1x templates cannot match its 4x4 blocks.
            frames.compose(leaky, [(20, 6, 60, 26)], scale=4)[0].save(root / "leaky-4x.png")
            report = privacy.scan([root / "leaky-4x.png", root / "leaky.png"], privacy.load_templates(root / "tpl"),
                                  engine="python")
            scaled, plain = report[str(root / "leaky-4x.png")], report[str(root / "leaky.png")]
            self.assertEqual((scaled["size"], scaled["magnified"], scaled["hits"]), ([160, 80], [2, 4], 1))
            self.assertEqual((scaled["templates"]["name"]["at"], scaled["templates"]["name"]["magnified"],
                              scaled["templates"]["name"]["hit_list"]), ([40, 24], 4, [[40, 24, 1.0]]))
            self.assertEqual((plain["magnified"], plain["hits"]), ([], 1))
            self.assertNotIn("magnified", plain["templates"]["name"])
            run = [sys.executable, "-B", str(QA), "privacy", "scan", "--templates", str(root / "tpl"), "--engine",
                   "python", str(root / "leaky-4x.png")]
            named = subprocess.run(run, capture_output=True, text=True)
            redacted = subprocess.run(run + ["--redacted"], capture_output=True, text=True)
        self.assertEqual((named.returncode, redacted.returncode), (1, 1), named.stderr + redacted.stderr)
        self.assertIn("best name 1.00 at [40, 24] (read at 1/4 size, as an exact 4x magnification)", named.stdout)
        self.assertIn("HIT name at (40,24)", named.stdout)
        self.assertIn("leaky-4x.png: MATCH", redacted.stdout)

    def test_a_4x_crop_of_blocky_content_is_read_at_every_exact_factor(self) -> None:
        from PIL import Image

        frame, width, height, _ = synthetic()
        # The app drew the string 2x2-blocky, so its template, cut at the app's size, is blocky too.
        drawn = Image.frombytes("L", (width, height), frame).resize((width * 2, height * 2), Image.Resampling.NEAREST)
        template = drawn.crop((60, 24, 92, 40))
        crop = frames.compose(drawn.convert("RGB"), [(40, 12, 120, 52)], scale=4)[0]  # an exact 8x as well
        with tempfile.TemporaryDirectory() as scratch:
            path = Path(scratch) / "blocky-4x.png"
            crop.save(path)
            report = privacy.scan([path], {"t1": privacy.gray(template)}, engine="python")[str(path)]
        self.assertEqual((report["magnified"], report["hits"]), ([2, 4, 8], 1))
        self.assertEqual((report["templates"]["t1"]["magnified"], report["templates"]["t1"]["at"]), (4, [80, 48]))

    def test_every_animation_frame_is_scanned_up_to_the_cap(self) -> None:
        from PIL import Image

        frame, width, height, (patch, tw, th) = synthetic()
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            flat = Image.new("L", (width, height), 40)
            leaky = Image.frombytes("L", (width, height), frame)
            flat.save(root / "late.gif", save_all=True, append_images=[leaky], duration=100, loop=0)
            templates = {"t1": (patch, tw, th)}
            report = privacy.scan([root / "late.gif"], templates, engine="python")[str(root / "late.gif")]
            self.assertEqual((report["frames"], report["hits"], report["templates"]["t1"]["frame"]), (2, 1, 1))
            with mock.patch.object(privacy, "MAX_SCAN_FRAMES", 1), self.assertRaises(SystemExit) as refused:
                privacy.scan([root / "late.gif"], templates, engine="python")
            self.assertEqual(str(refused.exception), f"cannot scan {root / 'late.gif'}: more than 1 animation frames")

    def test_anonymous_loading_never_names_the_directory(self) -> None:
        from PIL import Image

        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch) / "QA-TEMPLATE-dir"
            with self.assertRaises(SystemExit) as missing:
                privacy.load_templates(root, anonymous=True)
            root.mkdir()
            with self.assertRaises(SystemExit) as empty:
                privacy.load_templates(root, anonymous=True)
            (root / "a.png").write_bytes(b"not an image")
            with self.assertRaises(SystemExit) as unreadable:
                privacy.load_templates(root, anonymous=True)
            subprocess.run(["git", "init", "-q", str(root)], check=True)
            (root / "a.png").unlink()
            Image.new("L", (4, 4), 9).save(root / "a.png")
            with self.assertRaises(SystemExit) as tracked:
                privacy.load_templates(root, anonymous=True)
        messages = [str(error.exception) for error in (missing, empty, unreadable, tracked)]
        self.assertEqual(messages[:3], ["the template directory cannot be read",
                                        "no PNG or JPEG templates in the template directory",
                                        "template 1 in the template directory is not a readable image"])
        self.assertTrue(messages[3].startswith("refusing: the template directory is inside a Git work tree"))
        self.assertFalse(any("QA-TEMPLATE" in message or scratch in message for message in messages))

    def test_pack_refuses_a_set_above_the_secret_limit(self) -> None:
        from PIL import Image

        from native_qa import qa

        rng = random.Random(3)
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            (root / "tpl").mkdir()
            Image.frombytes("L", (64, 64), bytes(rng.randrange(256) for _ in range(4096))).save(root / "tpl" / "a.png")
            with mock.patch.object(privacy, "SECRET_LIMIT", 100), mock.patch("sys.stderr"):
                self.assertEqual(qa.main(["privacy", "pack", "--templates", str(root / "tpl"),
                                          "--output", str(root / "set.b64")]), 1)
            self.assertFalse((root / "set.b64").exists())


if __name__ == "__main__":
    unittest.main()
