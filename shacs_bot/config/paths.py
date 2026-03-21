"""Runtime path helpers derived from the active config context."""

from __future__ import annotations

from pathlib import Path

from shacs_bot.config.loader import get_config_path
from shacs_bot.utils import ensure_dir


def get_data_dir() -> Path:
    """인스턴스 수준의 런타임 데이터 디렉터리를 반환한다."""
    return ensure_dir(get_config_path().parent)


def get_runtime_subdir(name: str) -> Path:
    """인스턴스 데이터 디렉터리 아래의 지정된 이름의 런타임 하위 디렉터리를 반환한다."""
    return ensure_dir(get_data_dir() / name)


def get_media_dir(channel: str | None = None) -> Path:
    """미디어 디렉터리를 반환하며, 필요에 따라 채널별로 분리된 경로를 사용한다.."""
    base: Path = get_runtime_subdir("media")
    return ensure_dir(base / channel) if channel else base


def get_usage_dir() -> Path:
    return get_runtime_subdir("usage")


def get_cron_dir() -> Path:
    """cron 저장 디렉터리를 반환한다."""
    return get_runtime_subdir("cron")


def get_logs_dir() -> Path:
    """로그 디렉토리를 반환한다."""
    return get_runtime_subdir("logs")


def get_workspace_path(workspace: str | None = None) -> Path:
    """에이전트 워크스페이스 경로를 확인하고 필요한 경우 생성하여 반환한다."""
    path = Path(workspace).expanduser() if workspace else Path.home() / ".shacs-bot" / "workspace"
    return ensure_dir(path)


def get_cli_history_path() -> Path:
    """공용 CLI 히스토리 파일 경로를 반환한다."""
    return Path.home() / ".shacs-bot" / "history" / "cli_history"


def get_bridge_install_dir() -> Path:
    """공용 WhatsApp 브리지 설치 디렉터리를 반환한다."""
    return Path.home() / ".shacs-bot" / "bridge"


def get_legacy_sessions_dir() -> Path:
    """마이그레이션 폴백에 사용되는 레거시 전역 세션 디렉터리를 반환한다."""
    return Path.home() / ".shacs-bot" / "sessions"
