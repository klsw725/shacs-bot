"""Resolve SHACS_HOME for standalone skill scripts.

Skill scripts may run outside the shacs-bot process, so this module keeps
the home-directory lookup in one place using only the Python standard library.

All scripts under ``google-workspace/scripts/`` should import from here
instead of duplicating the ``SHACS_HOME = Path(os.getenv(...))`` pattern.
"""

from __future__ import annotations

import os
from pathlib import Path

def get_shacs_home() -> Path:
    """Return the shacs-bot home directory (default: ~/.shacs-bot)."""
    val = os.environ.get("SHACS_HOME", "").strip()
    return Path(val) if val else Path.home() / ".shacs-bot"


def display_shacs_home() -> str:
    """Return a user-friendly ``~/``-shortened display string."""
    home = get_shacs_home()
    try:
        return "~/" + str(home.relative_to(Path.home()))
    except ValueError:
        return str(home)
