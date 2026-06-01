#!/usr/bin/env python3
"""Generate the Rust built-in skill catalog."""

from __future__ import annotations

import argparse
import re
import subprocess
import tempfile
from pathlib import Path

PREFERRED_ORDER = [
    "cron",
    "weather",
    "tmux",
    "my",
    "github",
    "skill-creator",
    "clawhub",
    "summarize",
    "memory",
]

EXCLUDED_DIRS = {"src", "scripts", "target", ".git"}
DEFERRED_POLICY = "deferred_builtins.txt"


def rust_string(value: str) -> str:
    return '"' + value.replace("\\", "\\\\").replace('"', '\\"') + '"'


def const_name(skill_name: str) -> str:
    name = re.sub(r"[^A-Za-z0-9]+", "_", skill_name).strip("_").upper()
    if not name or name[0].isdigit():
        name = f"SKILL_{name}"
    return f"{name}_FILES"


def executable(path: Path) -> bool:
    return bool(path.stat().st_mode & 0o111)


def reject_symlink(path: Path, label: str) -> None:
    if path.is_symlink():
        raise ValueError(f"symlink paths are not allowed in bundled skills: {label}")


def read_deferred_names(crate_dir: Path) -> list[str]:
    policy = crate_dir / DEFERRED_POLICY
    if not policy.exists():
        return []
    names = []
    for line in policy.read_text(encoding="utf-8").splitlines():
        name = line.strip()
        if not name or name.startswith("#"):
            continue
        names.append(name)
    if len(names) != len(set(names)):
        raise ValueError(f"duplicate entries in {DEFERRED_POLICY}")
    return sorted(names)


def skill_sort_key(path: Path) -> tuple[int, str]:
    name = path.name
    if name in PREFERRED_ORDER:
        return (0, f"{PREFERRED_ORDER.index(name):04d}")
    return (1, name)


def skill_disabled(skill_dir: Path) -> bool:
    raw = (skill_dir / "SKILL.md").read_text(encoding="utf-8")
    return bool(re.search(r"(?m)^disabled:\s*true\s*$", raw))


def iter_skill_dirs(crate_dir: Path, deferred_names: set[str]) -> list[Path]:
    dirs: list[Path] = []
    for child in crate_dir.iterdir():
        if child.name in EXCLUDED_DIRS:
            continue
        reject_symlink(child, child.name)
        if not child.is_dir():
            continue
        if not (child / "SKILL.md").is_file():
            continue
        if child.name in deferred_names or skill_disabled(child):
            continue
        dirs.append(child)
    return sorted(dirs, key=skill_sort_key)


def iter_files(skill_dir: Path) -> list[Path]:
    files: list[Path] = []
    for path in skill_dir.rglob("*"):
        relative = path.relative_to(skill_dir).as_posix()
        reject_symlink(path, f"{skill_dir.name}/{relative}")
        if path.is_file():
            files.append(path)
    files.sort(key=lambda path: path.relative_to(skill_dir).as_posix())
    return files


def validate_skill(skill_dir: Path) -> None:
    reject_symlink(skill_dir, skill_dir.name)
    skill_file = skill_dir / "SKILL.md"
    reject_symlink(skill_file, f"{skill_dir.name}/SKILL.md")
    if not skill_file.is_file():
        raise ValueError(f"missing SKILL.md: {skill_dir}")
    skill_file.read_text(encoding="utf-8")
    for path in iter_files(skill_dir):
        relative = path.relative_to(skill_dir).as_posix()
        if relative.startswith("/") or ".." in Path(relative).parts or "\\" in relative:
            raise ValueError(f"unsafe relative path for {skill_dir.name}: {relative}")


def generate(crate_dir: Path) -> str:
    deferred_names = read_deferred_names(crate_dir)
    skill_dirs = iter_skill_dirs(crate_dir, set(deferred_names))
    seen_names: set[str] = set()
    const_blocks: list[str] = []
    entries: list[str] = []

    for skill_dir in skill_dirs:
        validate_skill(skill_dir)
        skill_name = skill_dir.name
        if skill_name in seen_names:
            raise ValueError(f"duplicate skill directory: {skill_name}")
        seen_names.add(skill_name)

        const = const_name(skill_name)
        file_entries = []
        seen_relative: set[str] = set()
        for path in iter_files(skill_dir):
            relative = path.relative_to(skill_dir).as_posix()
            if relative in seen_relative:
                raise ValueError(f"duplicate file in {skill_name}: {relative}")
            seen_relative.add(relative)
            manifest_path = f"/{skill_name}/{relative}"
            file_entries.append(
                "    BuiltinSkillFile {\n"
                f"        relative_path: {rust_string(relative)},\n"
                "        content: include_bytes!(concat!(\n"
                "            env!(\"CARGO_MANIFEST_DIR\"),\n"
                f"            {rust_string(manifest_path)}\n"
                "        )),\n"
                f"        executable: {str(executable(path)).lower()},\n"
                "    },"
            )
        const_blocks.append(
            f"const {const}: &[BuiltinSkillFile] = &[\n" + "\n".join(file_entries) + "\n];"
        )
        entries.append(
            "    BuiltinSkill {\n"
            f"        name: {rust_string(skill_name)},\n"
            f"        files: {const},\n"
            "    },"
        )

    deferred_entries = ",\n".join(f"    {rust_string(name)}" for name in deferred_names)
    parts = [
        "// This file is @generated by scripts/generate_builtins.py; do not edit by hand.",
        "use super::{BuiltinSkill, BuiltinSkillFile};",
        "",
        f"pub const DEFERRED_BUILTIN_SKILLS: &[&str] = &[\n{deferred_entries}\n];",
        "",
        *const_blocks,
        "",
        "pub(super) const BUILTIN_SKILLS: &[BuiltinSkill] = &[",
        *entries,
        "];",
        "",
    ]
    return "\n".join(parts)


def format_rust(source: str) -> str:
    with tempfile.TemporaryDirectory() as directory:
        path = Path(directory) / "builtins_generated.rs"
        path.write_text(source, encoding="utf-8")
        subprocess.run(["rustfmt", str(path)], check=True)
        return path.read_text(encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--crate-dir", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--output", type=Path, default=None)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()

    crate_dir = args.crate_dir.resolve()
    output = args.output or crate_dir / "src" / "builtins_generated.rs"
    generated = format_rust(generate(crate_dir))
    if args.check:
        existing = output.read_text(encoding="utf-8") if output.exists() else ""
        if existing != generated:
            print(f"{output} is stale; run scripts/generate_builtins.py", flush=True)
            return 1
        return 0
    output.write_text(generated, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
