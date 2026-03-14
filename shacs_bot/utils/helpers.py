"""Utility functions for shacs-bot."""
import json
import re
from datetime import datetime
from importlib.resources import files as pkg_files
from importlib.resources.abc import Traversable
from pathlib import Path
from typing import Any

import tiktoken
from rich.console import Console
from tiktoken import Encoding


def detect_image_mime(data: bytes) -> str | None:
    """파일 확장자를 무시하고 매직 바이트(magic bytes)를 이용해 이미지 MIME 타입을 감지한다."""
    if data[:8] == b"\x89PNG\r\n\x1a\n":
        return "image/png"
    elif data[:3] == b"\xff\xd8\xff":
        return "image/jpeg"
    elif data[:6] in (b"GIF87a", b"GIF89a"):
        return "image/gif"
    elif data[:4] == b"RIFF" and data[8:12] == b"WEBP":
        return "image/webp"

    return None


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

def split_message(content: str, max_len: int = 2000) -> list[str]:
    """
    max_len 길이 이내로 내용을 여러 조각으로 나눕니다. 가능한 경우 줄바꿈 위치를 우선적으로 사용합니다.

    Args:
        content: 분할할 텍스트 내용.
        max_len: 각 조각의 최대 길이 (기본값 2000, Discord 호환성을 위한 값).

    Returns:
        max_len을 넘지 않는 메시지 조각들의 리스트.
    """
    if not content:
        return []

    chunks: list[str] = []
    in_codeblock: bool = False

    while len(content) > max_len:
        cut: str = content[:max_len]

        # 먼저 줄바꿈(\n) 위치에서 끊어보고, 없으면 공백에서 끊고, 그래도 없으면 강제로 자른다
        pos: int = cut.rfind('\n')
        if pos <= 0:
            pos = cut.rfind(' ')
        if pos <= 0:
            pos = max_len

        part = content[:pos]

        # codeblock 상태 추적
        if part.count("```") % 2 == 1:
            in_codeblock = True
            part += "\n```"

        chunks.append(part)

        content = content[pos:].lstrip()

        if in_codeblock:
            content = "```\n" + content

    chunks.append(content)
    return chunks

def build_assistant_message(
        content: str | None,
        tool_calls: list[dict[str, Any]] | None = None,
        reasoning_content: str | None = None,
        thinking_blocks: list[dict] | None = None,
) -> dict[str, Any]:
    """선택적인 추론(reasoning) 필드를 포함할 수 있는, 제공자(provider) 호환 안전한 assistant 메시지를 생성합니다."""
    msg: dict[str, Any] = {
        "role": "assistant",
        "content": content
    }
    if tool_calls:
        msg["tool_calls"] = tool_calls
    if reasoning_content is not None:
        msg["reasoning_content"] = reasoning_content
    if thinking_blocks:
        msg["thinking_blocks"] = thinking_blocks

    return msg

def estimate_prompt_tokens(
        messages: list[dict[str, Any]],
        tools: list[dict[str, Any]] | None = None,
) -> int:
    """tiktoken으로 프롬프트 토큰 추정"""
    try:
        enc: Encoding = tiktoken.get_encoding("cl100k_base")
        parts: list[str] = []

        for msg in messages:
            content = msg.get("content")
            if isinstance(content, str):
                parts.append(content)
            elif isinstance(content, list):
                for part in content:
                    if isinstance(part, dict) and part.get("type") == "text":
                        txt: str = part.get("text", "")
                        if txt:
                            parts.append(txt)

        if tools:
            parts.append(json.dumps(tools, ensure_ascii=False))

        return len(enc.encode("\n".join(parts)))
    except Exception:
        return 0

def estimate_message_tokens(message: dict[str, Any]) -> int:
    """하나의 저장된(persisted) 메시지가 프롬프트에 기여하는 토큰 수를 추정합니다."""
    parts: list[str] = []

    content: Any = message.get("content")
    if isinstance(content, str):
        parts.append(content)
    elif isinstance(content, list):
        for part in content:
            if isinstance(part, dict) and part.get("type") == "text":
                text: str = part.get("text", "")
                if text:
                    parts.append(text)
            else:
                parts.append(json.dumps(part, ensure_ascii=False))
    elif content is not None:
        parts.append(json.dumps(content, ensure_ascii=False))

    for key in ("name", "tool_call_id"):
        value: Any = message.get(key)
        if isinstance(value, str) and value:
            parts.append(value)

    if message.get("tool_calls"):
        parts.append(json.dumps(message["tool_calls"], ensure_ascii=False))

    payload: str = "\n".join(parts)
    if not payload:
        return 1

    try:
        enc: Encoding = tiktoken.get_encoding("cl100k_base")
        return max(1, len(enc.encode(payload)))
    except Exception:
        return max(1, len(payload) // 4)


def estimate_prompt_tokens_chain(
        provider: Any,
        model: str | None,
        messages: list[dict[str, Any]],
        tools: list[dict[str, Any]] | None = None,
) -> tuple[int, str]:
    """먼저 provider의 토큰 카운터로 프롬프트 토큰을 추정하고, 실패하면 tiktoken을 대체 수단으로 사용합니다."""
    provider_counter = getattr(provider, "estimate_prompt_tokens", None)
    if callable(provider_counter):
        try:
            tokens, source = provider_counter(messages, tools, model)
            if isinstance(tokens, (int, float)) and tokens > 0:
                return int(tokens), str(source or "provider_counter")
        except Exception:
            pass

    estimated: int = estimate_prompt_tokens(messages, tools)
    if estimated > 0:
        return int(estimated), "tiktoken"

    return 0, "none"

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
            Console().print(f"  [dim]{name} 생성 됨[/dim]")

    return added
