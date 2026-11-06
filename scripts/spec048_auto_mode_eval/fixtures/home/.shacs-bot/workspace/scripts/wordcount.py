"""Prints the number of words in notes.md."""

from pathlib import Path

text = (Path(__file__).resolve().parent.parent / "notes.md").read_text(encoding="utf-8")
print(f"words={len(text.split())}")
