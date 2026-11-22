"""Tests for deterministic release benchmark comparison driver."""
from __future__ import annotations

from pathlib import Path
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT / "scripts") not in sys.path:
    sys.path.insert(0, str(ROOT / "scripts"))

from dev import bench_compare  # noqa: E402


class BenchCompareTests(unittest.TestCase):
    def test_compare_series_detects_speedup_regression_and_noise(self) -> None:
        faster = bench_compare.compare_series([100.0, 101.0, 99.0, 100.5, 99.5], [80.0, 81.0, 79.0, 80.5, 79.5])
        self.assertEqual(faster["verdict"], "faster")
        self.assertTrue(faster["significant"])
        self.assertAlmostEqual(faster["delta_pct"], -20.0, places=1)

        slower = bench_compare.compare_series([50.0, 50.5, 49.5, 50.0], [60.0, 60.5, 59.5, 60.0])
        self.assertEqual(slower["verdict"], "slower")
        self.assertTrue(slower["significant"])

        noisy = bench_compare.compare_series([100.0, 120.0, 80.0, 110.0, 90.0], [98.0, 118.0, 78.0, 108.0, 88.0])
        self.assertEqual(noisy["verdict"], "neutral")
        self.assertFalse(noisy["significant"])

    def test_run_comparison_alternates_and_hashes_binaries(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            bin_a = root / "base.bin"
            bin_b = root / "cand.bin"
            bin_a.write_bytes(b"baseline")
            bin_b.write_bytes(b"candidate")
            cmd = [sys.executable, "-c", "pass"]
            report = bench_compare.run_comparison(
                cmd,
                cmd,
                rounds=3,
                warmup=1,
                cwd=root,
                base_binary=bin_a,
                cand_binary=bin_b,
            )
            self.assertEqual(report["rounds"], 3)
            self.assertEqual(len(report["baseline_samples_ms"]), 3)
            self.assertEqual(len(report["candidate_samples_ms"]), 3)
            self.assertNotEqual(report["baseline_sha256"], report["candidate_sha256"])


if __name__ == "__main__":
    unittest.main()
