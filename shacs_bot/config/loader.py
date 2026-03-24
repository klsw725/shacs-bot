"""Configuration loading utilities."""

import json
import os
from pathlib import Path
from typing import Any

from loguru import logger

from shacs_bot.config.schema import Config


def get_config_path() -> Path:
    """Get the default configuration file path."""
    return Path.home() / ".shacs-bot" / "config.json"


def camel_to_snake(string: str) -> str:
    """Convert camelCase to snake_case"""
    result = []
    for i, char in enumerate(string):
        if char.isupper() and i > 0:
            result.append("_")
        result.append(char.lower())
    return "".join(result)


def convert_to_camel(data: Any) -> Any:
    """Convert camelCase keys to snake_case for Pydantic."""
    if isinstance(data, dict):
        return {camel_to_snake(k): convert_to_camel(v) for k, v in data.items()}
    if isinstance(data, list):
        return [convert_to_camel(item) for item in data]
    return data


def load_config(config_path: Path | None = None) -> Config:
    """
    Load configuration from file or create default.

    Args:
         config_path: Optional path to config file. Uses default if not provided.

    Returns:
        Loaded configuration object.
    """
    config_file: Path = config_path or get_config_path()

    if config_file.exists():
        try:
            with open(config_file, encoding="utf-8") as f:
                data = json.load(f)

            data = _migration_config(data)
            config = Config.model_validate(data)
            _apply_env(config)
            _migrate_workspace_layout(config_file.parent)
            return config
        except (json.JSONDecodeError, ValueError) as e:
            logger.warning("Failed to load config from {}: {}", config_file, e)
            logger.warning("Using default configuration.")

    return Config()


def _apply_env(config: Config) -> None:
    for key, value in config.env.items():
        if value:
            os.environ.setdefault(key, value)


def _migration_config(data):
    """Migrate old config formats to current."""
    # Move tools.exec.restrictToWorkspace -> tools.restrictToWorkspace
    tools: dict[str, dict] = data.get("tools", {})
    exec_cfg = tools.get("exec", {})
    if ("restrictToWorkspace" in exec_cfg) and ("restrictToWorkspace" not in tools):
        tools["restrictToWorkspace"] = exec_cfg.pop("restrictToWorkspace")

    return data


def _migrate_workspace_layout(data_dir: Path) -> None:
    """기존 워크스페이스 레이아웃을 새 구조로 마이그레이션한다. 멱등."""
    workspace = data_dir / "workspace"
    if not workspace.exists():
        return

    moves = [
        (workspace / "sessions", data_dir / "data" / "sessions"),
        (workspace / ".clawhub", data_dir / "data" / "clawhub"),
        (workspace / "cron", data_dir / "data" / "cron"),
        (data_dir / "cron", data_dir / "data" / "cron"),
        (data_dir / "usage", data_dir / "data" / "usage"),
        (data_dir / "sessions", data_dir / "data" / "sessions"),
    ]

    for src, dst in moves:
        if src.exists() and not dst.exists():
            dst.parent.mkdir(parents=True, exist_ok=True)
            src.rename(dst)
            logger.info("마이그레이션: {} → {}", src, dst)


def save_config(config: Config, config_path: Path | None = None) -> None:
    """
    Save Configuration to file.

    Args:
        config: Configuration to save.
        config_path: Optional path to save to. Uses default if not provided.
    """
    path = config_path or get_config_path()
    path.parent.mkdir(parents=True, exist_ok=True)

    # Convert to camelCase format
    data = config.model_dump(by_alias=True)

    with open(path, "w", encoding="utf-8") as f:
        json.dump(data, f, indent=2, ensure_ascii=False)
