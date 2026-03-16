# SPEC: Multi-modal Media Generation

> **Prompt**: HKUDS/nanobot, OpenClaw 분석 후 shacs-bot에 추가할 기능 — Multi-modal Media Generation

## PRDs

| PRD | 설명 |
|---|---|
| [`docs/prds/media-generation.md`](../prds/media-generation.md) | MediaGenerateTool (Imagen 4 / DALL-E 3 / Veo 3) + 조건부 등록 |

## TL;DR

> **목적**: "이미지 그려줘", "동영상 만들어줘", "음악 만들어줘" 같은 요청에 대응하는 미디어 생성 도구를 추가한다.
>
> **Deliverables**:
> - `agent/tools/media.py` — `media_generate` 도구 구현
> - `config/schema.py` — `MediaConfig` 추가
>
> **Estimated Effort**: Medium (3-4시간)

## 현재 상태 분석

- 미디어 생성 도구 없음. `agent/tools/`에 filesystem, web, shell, history, message, spawn, cron, mcp만 존재
- 채널 시스템에 이미지/파일 전송 인프라는 존재 (Telegram, Discord 등에서 미디어 첨부 지원)
- 에이전트가 파일을 생성하면 `message` 도구로 사용자에게 전달 가능

### nanobot 참조 구현

nanobot은 Google의 Gemini API를 통해 Imagen 4 (이미지), Veo 3.1 (비디오), Lyria (음악)을 지원한다. 단일 `media_generate` 도구로 통합.

## 설계

### 단일 도구, 다중 모달리티

```
media_generate(type="image", prompt="...", ...)
media_generate(type="video", prompt="...", ...)
media_generate(type="music", prompt="...", ...)
```

### 지원 백엔드

| 모달리티 | API | 모델 | 필요 키 |
|---------|-----|------|---------|
| 이미지 | Google Gemini (Imagen) | `imagen-4.0-generate-preview-05-20` | `GEMINI_API_KEY` |
| 이미지 | OpenAI DALL-E | `dall-e-3` | `OPENAI_API_KEY` |
| 비디오 | Google Gemini (Veo) | `veo-3.0-generate-preview` | `GEMINI_API_KEY` |
| 음악 | Google Gemini (Lyria) | `lyria-002` | `GEMINI_API_KEY` |

### 변경 사항

#### 1. Config 추가 (`config/schema.py`)

```python
class MediaConfig(Base):
    """미디어 생성 도구 설정."""
    enabled: bool = False
    default_image_backend: str = "gemini"   # "gemini" | "openai"
    save_dir: str = "~/.shacs-bot/workspace/media"
    image_size: str = "1024x1024"
    video_duration_seconds: int = 8
```

#### 2. MediaGenerateTool (`agent/tools/media.py`)

```python
"""Multi-modal media generation tool."""

import asyncio
import base64
import httpx
from pathlib import Path
from typing import Any

from loguru import logger

from shacs_bot.agent.tools.base import Tool


class MediaGenerateTool(Tool):
    """이미지, 비디오, 음악을 생성하는 도구."""

    @property
    def name(self) -> str:
        return "media_generate"

    @property
    def description(self) -> str:
        return (
            "Generate media content (image, video, music) from a text prompt. "
            "Returns the file path of the generated media."
        )

    @property
    def parameters(self) -> dict[str, Any]:
        return {
            "type": "object",
            "properties": {
                "type": {
                    "type": "string",
                    "enum": ["image", "video", "music"],
                    "description": "Type of media to generate.",
                },
                "prompt": {
                    "type": "string",
                    "description": "Text description of the media to generate.",
                },
            },
            "required": ["type", "prompt"],
        }

    def __init__(self, config, gemini_api_key: str = "", openai_api_key: str = ""):
        self._config = config
        self._gemini_key = gemini_api_key
        self._openai_key = openai_api_key
        self._save_dir = Path(config.save_dir).expanduser()
        self._save_dir.mkdir(parents=True, exist_ok=True)

    async def execute(self, type: str, prompt: str, **kwargs) -> str:
        if type == "image":
            return await self._generate_image(prompt)
        elif type == "video":
            return await self._generate_video(prompt)
        elif type == "music":
            return await self._generate_music(prompt)
        return f"Error: Unsupported media type: {type}"

    async def _generate_image(self, prompt: str) -> str:
        """Gemini Imagen 또는 OpenAI DALL-E로 이미지 생성."""
        backend = self._config.default_image_backend

        if backend == "gemini" and self._gemini_key:
            return await self._imagen_generate(prompt)
        elif backend == "openai" and self._openai_key:
            return await self._dalle_generate(prompt)
        elif self._gemini_key:
            return await self._imagen_generate(prompt)
        elif self._openai_key:
            return await self._dalle_generate(prompt)
        else:
            return "Error: No API key configured for image generation. Set gemini or openai API key."

    async def _imagen_generate(self, prompt: str) -> str:
        """Google Imagen 4 API를 통한 이미지 생성."""
        url = f"https://generativelanguage.googleapis.com/v1beta/models/imagen-4.0-generate-preview-05-20:predict"
        async with httpx.AsyncClient(timeout=60) as client:
            response = await client.post(
                url,
                headers={"x-goog-api-key": self._gemini_key},
                json={
                    "instances": [{"prompt": prompt}],
                    "parameters": {"sampleCount": 1, "aspectRatio": "1:1"},
                },
            )
            response.raise_for_status()
            data = response.json()

        image_bytes = base64.b64decode(data["predictions"][0]["bytesBase64Encoded"])
        file_path = self._save_dir / f"image_{self._timestamp()}.png"
        file_path.write_bytes(image_bytes)
        return f"Image generated: {file_path}"

    async def _dalle_generate(self, prompt: str) -> str:
        """OpenAI DALL-E 3 API를 통한 이미지 생성."""
        url = "https://api.openai.com/v1/images/generations"
        async with httpx.AsyncClient(timeout=60) as client:
            response = await client.post(
                url,
                headers={"Authorization": f"Bearer {self._openai_key}"},
                json={"model": "dall-e-3", "prompt": prompt, "n": 1, "size": self._config.image_size},
            )
            response.raise_for_status()
            data = response.json()

        image_url = data["data"][0]["url"]
        async with httpx.AsyncClient(timeout=60) as client:
            img_response = await client.get(image_url)
            file_path = self._save_dir / f"image_{self._timestamp()}.png"
            file_path.write_bytes(img_response.content)

        return f"Image generated: {file_path}"

    async def _generate_video(self, prompt: str) -> str:
        """Google Veo API를 통한 비디오 생성 (비동기 폴링)."""
        if not self._gemini_key:
            return "Error: Gemini API key required for video generation."

        url = "https://generativelanguage.googleapis.com/v1beta/models/veo-3.0-generate-preview:predictLongRunning"
        async with httpx.AsyncClient(timeout=300) as client:
            # 작업 시작
            response = await client.post(
                url,
                headers={"x-goog-api-key": self._gemini_key},
                json={
                    "instances": [{"prompt": prompt}],
                    "parameters": {
                        "durationSeconds": self._config.video_duration_seconds,
                        "sampleCount": 1,
                    },
                },
            )
            response.raise_for_status()
            operation = response.json()

            # 폴링
            op_name = operation["name"]
            for _ in range(60):  # 최대 5분
                await asyncio.sleep(5)
                poll = await client.get(
                    f"https://generativelanguage.googleapis.com/v1beta/{op_name}",
                    headers={"x-goog-api-key": self._gemini_key},
                )
                poll_data = poll.json()
                if poll_data.get("done"):
                    video_bytes = base64.b64decode(
                        poll_data["response"]["predictions"][0]["bytesBase64Encoded"]
                    )
                    file_path = self._save_dir / f"video_{self._timestamp()}.mp4"
                    file_path.write_bytes(video_bytes)
                    return f"Video generated: {file_path}"

        return "Error: Video generation timed out."

    async def _generate_music(self, prompt: str) -> str:
        """Google Lyria API를 통한 음악 생성."""
        if not self._gemini_key:
            return "Error: Gemini API key required for music generation."
        # Lyria API는 Imagen과 유사한 패턴
        # 정식 API 공개 시 구현 확정
        return "Error: Music generation is not yet available (waiting for Lyria API GA)."

    def _timestamp(self) -> str:
        from datetime import datetime
        return datetime.now().strftime("%Y%m%d_%H%M%S")
```

#### 3. 도구 등록 (`agent/loop.py`)

```python
# _register_default_tools 내부
if config.tools.media.enabled:
    from shacs_bot.agent.tools.media import MediaGenerateTool
    gemini_key = config.providers.gemini.api_key if hasattr(config.providers, "gemini") else ""
    openai_key = config.providers.openai.api_key if hasattr(config.providers, "openai") else ""
    self._tools.register(MediaGenerateTool(config.tools.media, gemini_key, openai_key))
```

## 파일 변경 목록

| 파일 | 변경 | 설명 |
|------|:---:|------|
| `agent/tools/media.py` | 신규 | `MediaGenerateTool` 구현 |
| `config/schema.py` | 수정 | `MediaConfig` 추가, `ToolsConfig`에 `media` 필드 |
| `agent/loop.py` | 수정 | 조건부 도구 등록 |

## 검증 기준

- [ ] `media.enabled=false`일 때 `media_generate` 도구가 등록되지 않음 확인
- [ ] Gemini API 키로 이미지 생성 → 파일 저장 → 경로 반환 확인
- [ ] OpenAI API 키로 이미지 생성 fallback 동작 확인
- [ ] API 키 미설정 시 명확한 에러 메시지 반환 확인
- [ ] 채널(Telegram/Discord)에서 생성된 미디어 파일 전송 가능 확인
