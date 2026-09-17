"""Filesystem-boundary checks use disposable records and adversarial replacements."""

from __future__ import annotations

import json
import os
from pathlib import Path
import stat
import tempfile
import time
import unittest
from unittest.mock import patch

from agent_loop import records
from agent_loop.records import (
    LoopError, atomic_json, atomic_json_at, open_directory, read_json, read_json_at,
    validate_basename,
)
# Records and run state stay private even when the host umask is permissive.
from agent_loop.test_support import setUpModule, tearDownModule


@unittest.skipUnless(os.name == "posix", "Record descriptors support macOS and Linux")
class RecordTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)

    def test_round_trip_creates_private_directories_and_durable_record(self):
        path = self.root / "run" / "attempt" / "record.json"
        value = {"message": "résumé", "count": 12}
        atomic_json(path, value)
        self.assertEqual(read_json(path), value)
        self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o600)
        self.assertEqual(stat.S_IMODE(path.parent.stat().st_mode), 0o700)
        self.assertEqual(list(path.parent.iterdir()), [path])
        atomic_json(path, {"replaced": True})
        self.assertEqual(read_json(path), {"replaced": True})

    def test_invalid_names_cannot_escape_directory(self):
        for name in ("", ".", "..", "../other", "/other", "nested/file", "x\\file", "x\0file"):
            with self.subTest(name=name), self.assertRaises(LoopError):
                validate_basename(name)
        self.assertEqual(list(self.root.iterdir()), [])

    def test_leaf_symlinks_cannot_read_or_replace_external_record(self):
        outside = self.root / "outside.json"
        outside.write_bytes(b'{"private": "preserve exactly"}\n')
        before = outside.read_bytes()
        directory = self.root / "records"
        directory.mkdir()
        leaf = directory / "record.json"
        leaf.symlink_to(outside)
        with self.assertRaises(LoopError):
            read_json(leaf)
        with self.assertRaises(LoopError):
            atomic_json(leaf, {"overwritten": True})
        self.assertTrue(leaf.is_symlink())
        self.assertEqual(outside.read_bytes(), before)
        self.assertEqual(list(directory.iterdir()), [leaf])

    def test_hardlinked_records_are_rejected_and_preserved(self):
        outside = self.root / "outside.json"
        outside.write_bytes(b'{"preserve": true}')
        linked = self.root / "linked.json"
        os.link(outside, linked)
        for operation in (lambda: read_json(linked), lambda: atomic_json(linked, {})):
            with self.assertRaisesRegex(LoopError, "singly-linked"):
                operation()
        self.assertEqual(outside.read_bytes(), b'{"preserve": true}')
        self.assertEqual(linked.stat().st_ino, outside.stat().st_ino)

    def test_fifo_and_directory_records_fail_without_waiting_for_a_writer(self):
        fifo = self.root / "fifo.json"
        os.mkfifo(fifo)
        directory = self.root / "directory.json"
        directory.mkdir()
        start = time.monotonic()
        for path in (fifo, directory):
            for operation in (lambda: read_json(path), lambda: atomic_json(path, {})):
                with self.subTest(path=path), self.assertRaises(LoopError):
                    operation()
        self.assertLess(time.monotonic() - start, 1)
        self.assertTrue(stat.S_ISFIFO(fifo.stat().st_mode))
        self.assertTrue(directory.is_dir())

    def test_oversized_and_invalid_records_are_rejected(self):
        path = self.root / "record.json"
        for raw in (b"{" * 65, b"[]", b"null", b"\xff", b'{"truncated":'):
            path.write_bytes(raw)
            with self.subTest(raw=raw), self.assertRaises(LoopError):
                read_json(path, max_bytes=64)
        for limit in (0, -1, True, 1.5):
            with self.subTest(limit=limit), self.assertRaises(LoopError):
                read_json(path, max_bytes=limit)

    def test_growth_after_open_is_still_bounded(self):
        path = self.root / "record.json"
        path.write_bytes(b"{}")
        original_read = os.read
        reads = []

        def grow_then_read(fd, count):
            if not reads:
                with path.open("ab") as stream:
                    stream.write(b" " * 4096)
            reads.append(count)
            return original_read(fd, count)

        with patch.object(records.os, "read", side_effect=grow_then_read):
            with self.assertRaisesRegex(LoopError, "size limit"):
                read_json(path, max_bytes=128)
        self.assertEqual(reads, [129])

    def test_leaf_replacement_after_open_does_not_redirect_the_read(self):
        path = self.root / "record.json"
        path.write_text('{"source": "opened record"}')
        outside = self.root / "outside.json"
        outside.write_text('{"source": "outside secret"}')
        original_read = os.read
        swapped = False

        def replace_then_read(fd, count):
            nonlocal swapped
            if not swapped:
                swapped = True
                path.unlink()
                path.symlink_to(outside)
            return original_read(fd, count)

        with patch.object(records.os, "read", side_effect=replace_then_read):
            self.assertEqual(read_json(path), {"source": "opened record"})
        self.assertEqual(outside.read_text(), '{"source": "outside secret"}')

    def test_renamed_parent_and_replacement_symlink_cannot_redirect_operations(self):
        directory = self.root / "records"
        directory.mkdir()
        (directory / "record.json").write_text('{"source": "original"}')
        outside = self.root / "outside"
        outside.mkdir()
        external = outside / "record.json"
        external.write_bytes(b'{"private": "unchanged bytes"}')
        before = external.read_bytes()
        moved = self.root / "moved"
        with open_directory(directory) as fd:
            directory.rename(moved)
            directory.symlink_to(outside, target_is_directory=True)
            self.assertEqual(read_json_at(fd, "record.json"), {"source": "original"})
            atomic_json_at(fd, "record.json", {"source": "updated original"})
        self.assertEqual(json.loads((moved / "record.json").read_bytes()),
                         {"source": "updated original"})
        self.assertEqual(external.read_bytes(), before)
        self.assertEqual(list(moved.iterdir()), [moved / "record.json"])

    def test_final_directory_symlink_is_rejected_but_ancestor_alias_is_supported(self):
        real = self.root / "real"
        nested = real / "nested"
        nested.mkdir(parents=True)
        alias = self.root / "alias"
        alias.symlink_to(real, target_is_directory=True)
        for create in (False, True):
            with self.subTest(create=create), self.assertRaises(LoopError):
                with open_directory(alias, create=create):
                    self.fail("final symlink was opened")
        atomic_json(alias / "nested" / "record.json", {"alias": True})
        self.assertEqual(read_json(nested / "record.json"), {"alias": True})

    def test_unsafe_directory_permissions_are_rejected(self):
        shared = self.root / "shared"
        shared.mkdir(mode=0o777)
        shared.chmod(0o777)
        with self.assertRaisesRegex(LoopError, "writable by other users"):
            atomic_json(shared / "record.json", {})
        self.assertEqual(list(shared.iterdir()), [])

    def test_group_writable_ancestor_is_named_with_its_mode_and_remedy(self):
        # An operator whose checkout was created under umask 002 must learn which
        # ancestor to fix, not just that some directory on the path is shared.
        ancestor = self.root / "shared"
        target = ancestor / "records"
        target.mkdir(parents=True)
        os.chmod(ancestor, 0o775)
        for create in (False, True):
            with self.subTest(create=create):
                with self.assertRaises(LoopError) as raised:
                    with open_directory(target, create=create):
                        self.fail("group-writable ancestor was accepted")
                message = str(raised.exception)
                self.assertIn(f"record directory {ancestor} is writable by other users",
                              message)
                self.assertIn("(mode 0775)", message)
                self.assertIn("chmod g-w,o-w", message)
                self.assertNotIn(str(target), message)
        self.assertEqual(stat.S_IMODE(ancestor.stat().st_mode), 0o775)

    def test_owner_mismatch_names_the_directory_and_both_user_ids(self):
        target = self.root / "records"
        target.mkdir()
        fd = os.open(target, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        self.addCleanup(os.close, fd)
        # Only the caller's identity can be varied without root, so the check is
        # driven directly: a foreign owner on the real path is not reproducible.
        owner = os.geteuid()
        cases = ((True, f"uid {owner + 1}"), (False, f"uid {owner + 1} or root"))
        with patch.object(records.os, "geteuid", return_value=owner + 1):
            for final, expected in cases:
                with self.subTest(final=final), self.assertRaises(LoopError) as raised:
                    records._check_directory(fd, final=final, path=target)
                message = str(raised.exception)
                self.assertIn(f"record directory {target} has an unexpected owner",
                              message)
                self.assertIn(f"(uid {owner}, expected {expected})", message)

    def test_writable_shared_record_is_rejected(self):
        path = self.root / "record.json"
        path.write_bytes(b'{"shared": true}')
        path.chmod(0o666)
        with self.assertRaises(LoopError):
            read_json(path)
        with self.assertRaises(LoopError):
            atomic_json(path, {})
        self.assertEqual(path.read_bytes(), b'{"shared": true}')

    def test_preexisting_temporary_symlink_is_neither_followed_nor_deleted(self):
        outside = self.root / "outside.json"
        outside.write_bytes(b'{"private": "preserve"}')
        temporary = self.root / ".write-collision"
        temporary.symlink_to(outside)
        with patch.object(records.secrets, "token_hex", return_value="collision"):
            with self.assertRaises(LoopError):
                atomic_json(self.root / "record.json", {})
        self.assertEqual(outside.read_bytes(), b'{"private": "preserve"}')
        self.assertTrue(temporary.is_symlink())
        self.assertFalse((self.root / "record.json").exists())

    def test_changed_target_before_commit_preserves_external_bytes_and_cleans_temp(self):
        path = self.root / "record.json"
        path.write_bytes(b'{"original": true}')
        outside = self.root / "outside.json"
        outside.write_bytes(b'{"private": "do not change"}')
        original_fsync = os.fsync
        swapped = False

        def replace_after_write(fd):
            nonlocal swapped
            original_fsync(fd)
            if not swapped:
                swapped = True
                path.unlink()
                path.symlink_to(outside)

        with patch.object(records.os, "fsync", side_effect=replace_after_write):
            with self.assertRaises(LoopError):
                atomic_json(path, {"replacement": True})
        self.assertTrue(path.is_symlink())
        self.assertEqual(outside.read_bytes(), b'{"private": "do not change"}')
        self.assertEqual(sorted(item.name for item in self.root.iterdir()),
                         ["outside.json", "record.json"])

    def test_late_leaf_symlink_cannot_turn_atomic_replace_into_external_write(self):
        path = self.root / "record.json"
        path.write_bytes(b'{"original": true}')
        outside = self.root / "outside.json"
        outside.write_bytes(b'{"private": "unchanged"}')
        original_replace = os.replace

        def replace_at_last_moment(source, target, **kwargs):
            path.unlink()
            path.symlink_to(outside)
            return original_replace(source, target, **kwargs)

        with patch.object(records.os, "replace", side_effect=replace_at_last_moment):
            atomic_json(path, {"replacement": True})
        self.assertEqual(read_json(path), {"replacement": True})
        self.assertEqual(outside.read_bytes(), b'{"private": "unchanged"}')

    def test_failed_serialization_preserves_existing_record_and_removes_temp(self):
        path = self.root / "record.json"
        path.write_bytes(b'{"original": true}')
        for value in ({"bad": object()}, {"large": "x" * 100}):
            with patch.object(records, "MAX_RECORD_BYTES", 64):
                with self.assertRaises(LoopError):
                    atomic_json(path, value)
            self.assertEqual(path.read_bytes(), b'{"original": true}')
            self.assertEqual(list(self.root.iterdir()), [path])


if __name__ == "__main__":
    unittest.main()
