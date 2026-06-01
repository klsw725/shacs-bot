#!/usr/bin/env python3
from __future__ import annotations

import os
import tempfile
import unittest
from pathlib import Path

import generate_builtins


@unittest.skipUnless(hasattr(os, "symlink"), "symlinks unavailable")
class GenerateBuiltinsSymlinkTests(unittest.TestCase):
    def test_rejects_symlink_skill_directory(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            crate = Path(tmp) / "crate"
            outside = Path(tmp) / "outside"
            crate.mkdir()
            outside.mkdir()
            (outside / "SKILL.md").write_text("---\ndescription: outside\n---\n", encoding="utf-8")
            os.symlink(outside, crate / "linked-skill")

            with self.assertRaisesRegex(ValueError, "symlink"):
                generate_builtins.generate(crate)

    def test_rejects_symlink_file_inside_skill(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            crate = Path(tmp) / "crate"
            skill = crate / "safe-skill"
            outside = Path(tmp) / "outside"
            skill.mkdir(parents=True)
            outside.mkdir()
            (skill / "SKILL.md").write_text("---\ndescription: safe\n---\n", encoding="utf-8")
            (outside / "secret.txt").write_text("secret", encoding="utf-8")
            os.symlink(outside / "secret.txt", skill / "secret.txt")

            with self.assertRaisesRegex(ValueError, "symlink"):
                generate_builtins.generate(crate)


if __name__ == "__main__":
    unittest.main()
