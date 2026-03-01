"""Utility functions for shacs-bot."""
from datetime import datetime
from pathlib import Path


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

def get_sessions_path() -> Path:
    """Get the sessions storage directory."""
    return ensure_dir(get_data_path() / "sessions")

def get_memory_path(workspace: Path | None = None) -> Path:
    """Get the memory directory within the workspace."""
    ws: Path = workspace or get_workspace_path()
    return ensure_dir(ws / "memory")

def get_skills_path(workspace: Path | None = None) -> Path:
    """Get the skills directory within the workspace."""
    ws: Path = workspace or get_workspace_path()
    return ensure_dir(ws / "skills")

def today_date() -> str:
    """get today's date in YYYY-MM-DD format."""
    return datetime.now().strftime("%Y-%m-%d")

def timestamp() -> str:
    """Get current timestamp in ISO format."""
    return datetime.now().isoformat()

def truncate_string(s: str, max_len: int = 100, suffix: str = "...") -> str:
    """Truncate a string to max length, adding suffix if truncated."""
    if len(s) <= max_len:
        return s
    return s[:(max_len - len(suffix))] + suffix

def safe_filename(name: str) -> str:
    """문자열을 safe filename으로 바꿉니다."""
    # 대체해야 할 안전하지 않은 문자열
    unsafe: str = '<>:"/\\|?*'
    for char in unsafe:
        name = name.replace(char, "_")

    return name.strip()

def parse_session_key(key: str) -> tuple[str, str]:
    """
    세션 키를 channel과 chat_id로 분리합니다.

    Args:
        key: "channel:chat_id" 형식의 세션 키

    Returns:
        Tuple of (channel, chat_id)
    """
    parts: list[str] = key.split(":", 1)
    if len(parts) != 2:
        raise ValueError(f"유효하지 않은 세션 키: {key}")

    return parts[0], parts[1]
