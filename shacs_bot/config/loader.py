"""Configuration loading utilities."""

import json
from pathlib import Path
from typing import Any

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
            return Config.model_validate(data)
        except (json.JSONDecodeError, ValueError) as e:
            print(f"Warning: Failed to load config from {config_file}: {e}")
            print("Using default configuration.")

    return Config()


def _migration_config(data):
    """Migrate old config formats to current."""
    # Move tools.exec.restrictToWorkspace -> tools.restrictToWorkspace
    tools: dict[str, dict] = data.get("tools", {})
    exec_cfg = tools.get("exec", {})
    if ("restrictToWorkspace" in exec_cfg) and ("restrictToWorkspace" not in tools):
        tools["restrictToWorkspace"] = exec_cfg.pop("restrictToWorkspace")

    return data


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
