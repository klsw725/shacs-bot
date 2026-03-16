# PRD: Multi-modal Media Generation

> **Spec**: [`docs/specs/2026-03-16-media-generation.md`](../specs/2026-03-16-media-generation.md)

---

## 문제

채팅 봇에서 "이미지 그려줘", "동영상 만들어줘" 같은 요청이 빈번하지만, shacs-bot에는 미디어 생성 도구가 없다. 현재 `agent/tools/`에는 filesystem, web, shell, history, message, spawn, cron, mcp만 존재하며, 에이전트는 `exec`로 외부 CLI 도구를 호출하는 우회적 방법밖에 없다.

채널 시스템에는 이미 미디어 파일 전송 인프라가 있으므로 (Telegram, Discord 등), 생성 도구만 추가하면 된다.

## 해결책

단일 `media_generate` 도구를 추가한다. `type` 파라미터로 이미지/비디오를 구분하고, Google Gemini (Imagen 4, Veo 3), OpenAI (DALL-E 3), 또는 **OpenRouter** (chat completions + `modalities: ["image"]`)를 백엔드로 사용한다. OpenRouter는 기존 LLM용 API 키 하나로 다양한 이미지 모델(Gemini, GPT-5 Image, FLUX.2 등)에 접근할 수 있어 별도 API 키 발급 없이 사용 가능하다. `media.enabled=false`(기본값)이면 도구가 등록되지 않는다.

## 사용자 영향

| Before | After |
|---|---|
| "이미지 그려줘" → exec로 우회하거나 불가 | `media_generate(type="image", prompt="...")` 직접 호출 |
| 미디어 생성 불가 | 이미지(Imagen 4/DALL-E 3/OpenRouter), 비디오(Veo 3) 생성 |
| 이미지 생성에 별도 API 키 발급 필요 | OpenRouter 키가 이미 있으면 추가 설정 없이 사용 가능 |
| 설정 변경 없음 | `media.enabled=true` + API 키 설정 필요 (opt-in) |

## 기술적 범위

- **변경 파일**: 3개 (1개 신규 + 2개 수정)
- **변경 유형**: 신규 도구 모듈 + config 추가 + 조건부 등록
- **의존성**: `httpx` (이미 존재)
- **하위 호환성**: `media.enabled=false`(기본값)이면 도구 미등록. 기존 동작 무변경
- **이미지 백엔드**: Gemini (Imagen 4), OpenAI (DALL-E 3), OpenRouter (chat completions + modalities)
- **비디오 백엔드**: Gemini (Veo 3) — OpenRouter/OpenAI는 비디오 미지원

### 변경 1: Config 추가 (`config/schema.py`)

`ExecToolConfig` 아래 (line 307 부근)에 추가:

```python
class MediaConfig(Base):
    """미디어 생성 도구 설정."""
    enabled: bool = False
    default_image_backend: str = "gemini"    # "gemini" | "openai" | "openrouter"
    openrouter_model: str = "google/gemini-3.1-flash-image-preview"
    save_dir: str = "~/.shacs-bot/workspace/media"
    image_size: str = "1024x1024"
    video_duration_seconds: int = 8
```

`ToolsConfig` (line 325)에 필드 추가:

```python
media: MediaConfig = Field(default_factory=MediaConfig)
```

### 변경 2: MediaGenerateTool (`agent/tools/media.py` 신규)

```python
"""Multi-modal media generation tool."""

import asyncio
import base64
from datetime import datetime
from pathlib import Path
from typing import Any

import httpx
from loguru import logger

from shacs_bot.agent.tools.base import Tool


class MediaGenerateTool(Tool):

    @property
    def name(self) -> str:
        return "media_generate"

    @property
    def description(self) -> str:
        return "Generate media content (image, video) from a text prompt. Returns the file path of the generated media."

    @property
    def parameters(self) -> dict[str, Any]:
        return {
            "type": "object",
            "properties": {
                "type": {
                    "type": "string",
                    "enum": ["image", "video"],
                    "description": "Type of media to generate.",
                },
                "prompt": {
                    "type": "string",
                    "description": "Text description of the media to generate.",
                },
            },
            "required": ["type", "prompt"],
        }

    def __init__(self, config, gemini_api_key: str = "", openai_api_key: str = "", openrouter_api_key: str = ""):
        self._config = config
        self._gemini_key: str = gemini_api_key
        self._openai_key: str = openai_api_key
        self._openrouter_key: str = openrouter_api_key
        self._save_dir: Path = Path(config.save_dir).expanduser()
        self._save_dir.mkdir(parents=True, exist_ok=True)

    async def execute(self, type: str, prompt: str, **kwargs: Any) -> str:
        if type == "image":
            return await self._generate_image(prompt)
        elif type == "video":
            return await self._generate_video(prompt)
        return f"Error: Unsupported media type: {type}"

    async def _generate_image(self, prompt: str) -> str:
        backend: str = self._config.default_image_backend
        # 1. 선호 백엔드 시도
        if backend == "gemini" and self._gemini_key:
            return await self._imagen(prompt)
        elif backend == "openai" and self._openai_key:
            return await self._dalle(prompt)
        elif backend == "openrouter" and self._openrouter_key:
            return await self._openrouter_image(prompt)
        # 2. Fallback: 키가 있는 백엔드 자동 선택
        elif self._gemini_key:
            return await self._imagen(prompt)
        elif self._openai_key:
            return await self._dalle(prompt)
        elif self._openrouter_key:
            return await self._openrouter_image(prompt)
        return "Error: No API key configured for image generation. Set gemini, openai, or openrouter API key."

    async def _imagen(self, prompt: str) -> str:
        """Google Imagen 4 API."""
        url: str = "https://generativelanguage.googleapis.com/v1beta/models/imagen-4.0-generate-preview-05-20:predict"
        async with httpx.AsyncClient(timeout=60) as client:
            resp = await client.post(url, headers={"x-goog-api-key": self._gemini_key}, json={
                "instances": [{"prompt": prompt}],
                "parameters": {"sampleCount": 1, "aspectRatio": "1:1"},
            })
            resp.raise_for_status()

        image_bytes: bytes = base64.b64decode(resp.json()["predictions"][0]["bytesBase64Encoded"])
        path: Path = self._save_dir / f"image_{self._ts()}.png"
        path.write_bytes(image_bytes)
        return f"Image generated: {path}"

    async def _dalle(self, prompt: str) -> str:
        """OpenAI DALL-E 3 API."""
        async with httpx.AsyncClient(timeout=60) as client:
            resp = await client.post(
                "https://api.openai.com/v1/images/generations",
                headers={"Authorization": f"Bearer {self._openai_key}"},
                json={"model": "dall-e-3", "prompt": prompt, "n": 1, "size": self._config.image_size},
            )
            resp.raise_for_status()
            image_url: str = resp.json()["data"][0]["url"]
            img = await client.get(image_url)

        path: Path = self._save_dir / f"image_{self._ts()}.png"
        path.write_bytes(img.content)
        return f"Image generated: {path}"

    async def _openrouter_image(self, prompt: str) -> str:
        """OpenRouter chat completions with image modality."""
        async with httpx.AsyncClient(timeout=60) as client:
            resp = await client.post(
                "https://openrouter.ai/api/v1/chat/completions",
                headers={"Authorization": f"Bearer {self._openrouter_key}"},
                json={
                    "model": self._config.openrouter_model,
                    "messages": [{"role": "user", "content": prompt}],
                    "modalities": ["image", "text"],
                },
            )
            resp.raise_for_status()

        data: dict = resp.json()
        images: list = data["choices"][0]["message"].get("images", [])
        if not images:
            return "Error: No image returned from OpenRouter."

        # data:image/png;base64,iVBOR... → base64 디코딩
        data_url: str = images[0]["image_url"]["url"]
        base64_data: str = data_url.split(",", 1)[1]
        image_bytes: bytes = base64.b64decode(base64_data)
        path: Path = self._save_dir / f"image_{self._ts()}.png"
        path.write_bytes(image_bytes)
        return f"Image generated: {path}"

    async def _generate_video(self, prompt: str) -> str:
        """Google Veo 3 API — 비동기 폴링."""
        if not self._gemini_key:
            return "Error: Gemini API key required for video generation."

        url: str = "https://generativelanguage.googleapis.com/v1beta/models/veo-3.0-generate-preview:predictLongRunning"
        headers: dict[str, str] = {"x-goog-api-key": self._gemini_key}

        async with httpx.AsyncClient(timeout=300) as client:
            resp = await client.post(url, headers=headers, json={
                "instances": [{"prompt": prompt}],
                "parameters": {"durationSeconds": self._config.video_duration_seconds, "sampleCount": 1},
            })
            resp.raise_for_status()
            op_name: str = resp.json()["name"]

            for _ in range(60):
                await asyncio.sleep(5)
                poll = await client.get(
                    f"https://generativelanguage.googleapis.com/v1beta/{op_name}",
                    headers=headers,
                )
                data: dict = poll.json()
                if data.get("done"):
                    video_bytes: bytes = base64.b64decode(data["response"]["predictions"][0]["bytesBase64Encoded"])
                    path: Path = self._save_dir / f"video_{self._ts()}.mp4"
                    path.write_bytes(video_bytes)
                    return f"Video generated: {path}"

        return "Error: Video generation timed out (5 minutes)."

    def _ts(self) -> str:
        return datetime.now().strftime("%Y%m%d_%H%M%S")
```

### 변경 3: 조건부 도구 등록 (`agent/loop.py`)

**loop.py** (line 156, `_register_default_tools` 끝) — 추가:

```python
# Media generation (opt-in)
# config 객체가 필요하므로, AgentLoop 초기화 시 config를 전달받아야 함
# 또는 외부에서 등록: agent_loop.tools.register(MediaGenerateTool(...))
```

실제 등록은 `gateway.py` 등 AgentLoop 생성 시점에서:

```python
if config.tools.media.enabled:
    from shacs_bot.agent.tools.media import MediaGenerateTool
    agent_loop.tools.register(MediaGenerateTool(
        config=config.tools.media,
        gemini_api_key=config.providers.gemini.api_key,
        openai_api_key=config.providers.openai.api_key,
        openrouter_api_key=config.providers.openrouter.api_key,
    ))
```

## 성공 기준

1. `media.enabled=false`(기본값) — `media_generate` 도구가 등록되지 않음
2. `media.enabled=true` + Gemini API 키 — `media_generate(type="image", prompt="a cat")` → PNG 파일 생성 → 경로 반환
3. Gemini 키 없고 OpenAI 키만 있을 때 — DALL-E 3로 fallback
4. Gemini/OpenAI 키 없고 OpenRouter 키만 있을 때 — OpenRouter chat completions (`modalities: ["image"]`)로 fallback
5. `default_image_backend: "openrouter"` 설정 시 — OpenRouter를 우선 사용
6. 모든 키가 없을 때 — 명확한 에러 메시지 반환
7. 생성된 파일이 `~/.shacs-bot/workspace/media/`에 저장됨
8. `message` 도구로 생성된 파일 경로를 사용자에게 전송 가능

---

## 마일스톤

- [ ] **M1: Config + MediaGenerateTool 구현**
  `MediaConfig` 스키마 추가 (`openrouter_model` 필드 포함). `agent/tools/media.py` — Imagen 4, DALL-E 3, OpenRouter 이미지 생성, Veo 3 비디오 생성. OpenRouter는 chat completions + `modalities: ["image"]` 방식.

- [ ] **M2: 조건부 등록 + 검증**
  `loop.py` 또는 `gateway.py`에서 `media.enabled` 체크 후 등록 (gemini/openai/openrouter 3개 키 전달). fallback 체인 동작 확인: preferred → gemini → openai → openrouter.

---

## 위험 및 완화

| 위험 | 가능성 | 영향 | 완화 |
|---|---|---|---|
| Imagen 4 / Veo 3 API가 preview 상태 | 확실 | 중간 | API 변경 시 URL/payload만 수정. 핵심 로직은 동일 |
| 비디오 생성이 5분 타임아웃 | 중간 | 낮음 | 폴링 루프에 최대 60회(5분) 제한. 타임아웃 시 에러 반환 |
| 생성된 파일이 디스크 공간 소비 | 중간 | 낮음 | `save_dir` 설정으로 경로 분리. 정리는 사용자 책임 (추후 자동 정리 고려) |
| 채널에서 파일 전송 미지원 | 낮음 | 중간 | 경로만 반환. 채널별 파일 전송은 기존 message 도구의 첨부 기능에 의존 |
| httpx timeout 값 하드코딩 | 낮음 | 낮음 | 이미지 60초, 비디오 300초로 충분. 필요 시 config 이동 |
| OpenRouter 이미지 응답 형식 변경 | 중간 | 중간 | `images[0].image_url.url` 파싱 실패 시 에러 반환. data URL 포맷은 표준 |
| OpenRouter 이미지 모델 가용성 변동 | 중간 | 낮음 | `openrouter_model` config로 모델 변경 가능. 기본값은 안정적 모델 선택 |

---

## 진행 로그

| 날짜 | 내용 |
|---|---|
| 2026-03-16 | PRD 초안 작성 |
| 2026-03-16 | OpenRouter 이미지 생성 백엔드 추가. chat completions + `modalities: ["image"]` 방식. `openrouter_model` config 필드, `_openrouter_image()` 메서드, fallback 체인 확장. 위험 및 성공 기준 업데이트. |
