"""Utility functions for shacs-bot."""
import re
from datetime import datetime
from importlib.resources import files as pkg_files
from importlib.resources.abc import Traversable
from pathlib import Path

from rich import Console


def ensure_dir(path: Path) -> Path:
    """Ensure a directory exists, creating it if necessary."""
    path.mkdir(parents=True, exist_ok=True)
    return path

def get_data_path() -> Path:
    """Get the shacs-bot data directory (~/.shacs-bot)."""
    return ensure_dir(Path.home() / ".shacs-bot")

def get_workspace_path(workspace: str | None = None) -> Path:
    """
    Get the workspace path.

    Args:
        workspace: Optional workspace path. Defaults to ~/.shacs-bot/workspace.

    Returns:
        Expanded and ensured workspace path.
    """
    if workspace:
        path: Path = Path(workspace).expanduser()
    else:
        path: Path = Path.home() / ".shacs-bot" / "workspace"
    return ensure_dir(path)

def timestamp() -> str:
    """Get current timestamp in ISO format."""
    return datetime.now().isoformat()

_UNSAFE_CHARS = re.compile(r'[<>:"/\\|?*]')

def safe_filename(name: str) -> str:
    """안전하지 않은 경로 문자를 밑줄(_)로 대체합니다."""
    return _UNSAFE_CHARS.sub("_", name).strip()

def sync_workspace_template(workspace: Path, silent: bool = False) -> list[str]:
    """번들된 템플릿을 워크스페이스에 동기화합니다. 존재하지 않는 파일만 생성합니다."""
    try:
        tpl: Traversable = pkg_files("shacs-bot") / "templates"
    except Exception:
        return []

    if not tpl.is_dir():
        return []

    added: list[str] = []

    def _write(src: Traversable | None, dst: Path) -> None:
        if dst.exists():
            return

        dst.parent.mkdir(parents=True, exist_ok=True)
        dst.write_text(src.read_text(encoding="utf-8") if src else "", encoding="utf-8")
        added.append(str(dst.relative_to(workspace)))

    for item in tpl.iterdir():
        if item.name.endswith(".md"):
            _write(item, workspace / item.name)

    _write(tpl / "memory" / "MEMORY.md", workspace / "memory" / "MEMORY.md")
    _write(None, workspace / "memory" / "HISTORY.md")

    (workspace / "skills").mkdir(exist_ok=True)

    if added and not silent:
        for name in added:
            Console().print(f"  [dim]Created {name}[/dim]")

    return added