# PRD: Multi-modal Media Generation

> **Spec**: [`docs/specs/2026-03-16-media-generation.md`](../spec.md)

---

## 문제

채팅 봇에서 "이미지 그려줘", "동영상 만들어줘" 같은 요청이 빈번하지만, shacs-bot에는 미디어 생성 도구가 없다. 현재 `agent/tools/`에는 filesystem, web, shell, history, message, spawn, cron, mcp만 존재하며, 에이전트는 `exec`로 외부 CLI 도구를 호출하는 우회적 방법밖에 없다.

채널 시스템에는 이미 미디어 파일 전송 인프라가 있으므로 (Telegram, Discord 등), 생성 도구만 추가하면 된다.

## 해결책

단일 `media_generate` 도구를 추가한다. `type` 파라미터로 이미지/비디오를 구분하고, `default_image_backend`에 지정한 프로바이더를 통해 이미지를 생성한다. `config.providers`에 등록된 **아무 프로바이더**를 백엔드로 사용할 수 있다.

API 호출 방식은 2가지:
- **gemini**: Google Imagen 4 / Veo 3 전용 prediction API
- **그 외 전부**: OpenAI-compatible chat completions + `modalities: ["image"]` — openai, openrouter, vllm, ollama, custom 등 모든 OpenAI-compatible 엔드포인트에서 동작

API 키와 base_url은 `config.providers.{default_image_backend}`에서 자동으로 가져온다. `media.enabled=false`(기본값)이면 도구가 등록되지 않는다.

## 사용자 영향

| Before | After |
|---|---|
| "이미지 그려줘" → exec로 우회하거나 불가 | `media_generate(type="image", prompt="...")` 직접 호출 |
| 미디어 생성 불가 | 이미지 생성 (gemini/openai/openrouter/vllm/ollama/custom) |
| 별도 API 키 발급 필요 | 기존 `config.providers`의 키를 그대로 사용 |
| 로컬 VLM 이미지 생성 불가 | vllm/ollama 등 로컬 모델로 이미지 생성 가능 |
| 설정 변경 없음 | `media.enabled=true` + backend 지정 (opt-in) |

## 기술적 범위

- **변경 파일**: 3개 (1개 신규 + 2개 수정)
- **변경 유형**: 신규 도구 모듈 + config 추가 + 조건부 등록
- **의존성**: `httpx` (이미 존재)
- **하위 호환성**: `media.enabled=false`(기본값)이면 도구 미등록. 기존 동작 무변경
- **이미지 백엔드**: gemini (Imagen 4 전용 API) 또는 아무 OpenAI-compatible 프로바이더 (chat completions + `modalities: ["image"]`)
- **비디오 백엔드**: gemini (Veo 3) 전용 — 다른 프로바이더는 비디오 미지원

### 변경 1: Config 추가 (`config/schema.py`)

`ExecToolConfig` 아래 (line 307 부근)에 추가:

`ProvidersConfig`에 이미지 생성 전용 프로바이더 추가:

```python
class ProvidersConfig(BaseModel):
    ...
    image_gen: ProviderConfig = Field(default_factory=ProviderConfig)  # 이미지 생성 전용
```

`MediaConfig`:

```python
class MediaConfig(Base):
    enabled: bool = False
    backend: str = "openai-compatible"  # "gemini" | "openai-compatible"
    model: str = ""
    save_dir: str = "~/.shacs-bot/workspace/media"
    video_duration_seconds: int = 8
```

`ToolsConfig`에 필드 추가:

```python
media: MediaConfig = Field(default_factory=MediaConfig)
```

`ToolsConfig` (line 325)에 필드 추가:

```python
media: MediaConfig = Field(default_factory=MediaConfig)
```

### 변경 2: MediaGenerateTool (`agent/tools/media.py` 신규)

API 호출 방식 2가지:
- **`gemini`**: Imagen 4 / Veo 3 전용 prediction API (`x-goog-api-key` 헤더)
- **그 외 전부**: OpenAI-compatible chat completions + `modalities: ["image"]` (`Authorization: Bearer` 헤더, `base_url` 사용)

```python
def __init__(self, config: MediaConfig, api_key: str = "", base_url: str = ""):
    self._config = config
    self._api_key = api_key
    self._base_url = base_url  # OpenAI-compatible 엔드포인트의 base URL

async def _generate_image(self, prompt: str) -> str:
    if not self._api_key:
        return f"Error: No API key for {self._config.default_image_backend} provider."
    if self._config.default_image_backend == "gemini":
        return await self._imagen(prompt)
    return await self._openai_compatible_image(prompt)

async def _openai_compatible_image(self, prompt: str) -> str:
    """OpenAI-compatible chat completions + modalities: ["image"]."""
    url = f"{self._base_url}/chat/completions"
    payload = {
        "messages": [{"role": "user", "content": prompt}],
        "modalities": ["image", "text"],
    }
    if self._config.model:
        payload["model"] = self._config.model
    # ... POST → base64 이미지 파싱 → 파일 저장
```

**지원 프로바이더 예시:**

| `defaultImageBackend` | `base_url` (자동) | 설명 |
|---|---|---|
| `gemini` | — | Imagen 4 전용 API |
| `openai` | `https://api.openai.com/v1` | GPT-4o 이미지 생성 |
| `openrouter` | `https://openrouter.ai/api/v1` | FLUX, Gemini 등 다양한 모델 |
| `vllm` | `http://localhost:8000/v1` | 로컬 VLM |
| `ollama` | `http://localhost:11434/v1` | 로컬 Ollama |
| `custom` | 사용자 지정 | 아무 OpenAI-compatible 서버 |

### 변경 3: 조건부 도구 등록 (`agent/loop.py`)

**loop.py** (line 156, `_register_default_tools` 끝) — 추가:

```python
# Media generation (opt-in)
# config 객체가 필요하므로, AgentLoop 초기화 시 config를 전달받아야 함
# 또는 외부에서 등록: agent_loop.tools.register(MediaGenerateTool(...))
```

`commands.py`에서 `providers.image_gen`으로 직접 매핑:

```python
agent_loop = AgentLoop(
    ...
    media_config=config.tools.media,
    media_api_key=config.providers.image_gen.api_key or None,
    media_base_url=config.providers.image_gen.base_url,
)
```

`loop.py`의 `_register_default_tools`에서 조건부 등록:

```python
if self._media_config and self._media_config.enabled:
    from shacs_bot.agent.tools.media import MediaGenerateTool
    self._tools.register(MediaGenerateTool(
        config=self._media_config,
        api_key=self._media_api_key or "",
        base_url=self._media_base_url or "",
    ))
```

## 성공 기준

1. `media.enabled=false`(기본값) — `media_generate` 도구가 등록되지 않음
2. `backend: "gemini"` — Imagen 4 전용 API로 이미지 생성
3. `backend: "openai-compatible"` — chat completions + `modalities: ["image"]`로 이미지 생성 (OpenRouter, 로컬 서버 등)
4. `providers.image_gen`에서 `apiKey` + `baseUrl` 중앙 관리
5. API 키가 없을 때 — 명확한 에러 메시지 반환
6. 생성된 파일이 `save_dir`에 저장됨
7. 비디오 생성은 gemini backend일 때만 사용 가능

---

## 마일스톤

- [x] **M1: Config + MediaGenerateTool 구현**
  `ProvidersConfig.image_gen` 추가. `MediaConfig.backend` ("gemini" | "openai-compatible"). `agent/tools/media.py` — gemini 전용 API (`_imagen`) + OpenAI-compatible 통합 메서드 (`_openai_compatible_image`).

- [x] **M2: 조건부 등록 + 검증**
  `commands.py`에서 `providers.image_gen` 직접 참조 (`_resolve_media_key`/`_resolve_media_base_url`). `loop.py`에서 `media_config` + `media_api_key` + `media_base_url`로 조건부 등록. LSP: 신규 에러 0건.

---

## 위험 및 완화

| 위험 | 가능성 | 영향 | 완화 |
|---|---|---|---|
| Imagen 4 / Veo 3 API가 preview 상태 | 확실 | 중간 | API 변경 시 URL/payload만 수정. 핵심 로직은 동일 |
| 비디오 생성이 5분 타임아웃 | 중간 | 낮음 | 폴링 루프에 최대 60회(5분) 제한. 타임아웃 시 에러 반환 |
| 생성된 파일이 디스크 공간 소비 | 중간 | 낮음 | `save_dir` 설정으로 경로 분리. 정리는 사용자 책임 (추후 자동 정리 고려) |
| 채널에서 파일 전송 미지원 | 낮음 | 중간 | 경로만 반환. 채널별 파일 전송은 기존 message 도구의 첨부 기능에 의존 |
| OpenAI-compatible 이미지 응답 형식이 프로바이더마다 다를 수 있음 | 중간 | 중간 | data URL (`data:image/...;base64,...`) 형식은 사실상 표준. 파싱 실패 시 에러 반환 |
| 로컬 VLM이 `modalities: ["image"]`를 지원하지 않을 수 있음 | 중간 | 낮음 | 에러 메시지에 backend 이름 포함. 사용자가 지원 모델로 변경 |

---

## 진행 로그

| 날짜 | 내용 |
|---|---|
| 2026-03-16 | PRD 초안 작성 |
| 2026-03-16 | OpenRouter 이미지 생성 백엔드 추가. chat completions + `modalities: ["image"]` 방식. `openrouter_model` config 필드, `_openrouter_image()` 메서드, fallback 체인 확장. 위험 및 성공 기준 업데이트. |
| 2026-03-21 | M1+M2 초기 구현. 3개 API 키 fallback 방식. |
| 2026-03-21 | 설계 변경 1: API 키 단일화. `default_image_backend`에 따라 `config.providers.{backend}.api_key`에서 1개만 가져옴. Fallback 제거. |
| 2026-03-21 | 설계 변경 2: OpenAI-compatible 통합. `_dalle`+`_openrouter_image` → `_openai_compatible_image` 단일 메서드. `base_url` 파라미터 추가. `config.providers`에 등록된 아무 프로바이더(vllm, ollama, custom 등)를 backend로 사용 가능. `openrouter_model`/`image_size` 필드 → `model` 단일 필드로 통합. |
| 2026-03-21 | 설계 변경 3 + 구현 완료 (v4). `ProvidersConfig.image_gen` 전용 프로바이더 도입. `default_image_backend` → `backend` ("gemini" \| "openai-compatible"). `_resolve` 헬퍼 단순화 — `providers.image_gen` 직접 참조. `media.py` 참조 일괄 변경. |
