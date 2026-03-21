# shacs-bot

**shacs-bot**은 [nanobot](https://github.com/HKUDS/nanobot)을 클론 코딩하여 확장한 개인용 AI 어시스턴트 프레임워크입니다.

nanobot의 경량 에이전트 아키텍처를 기반으로, 멀티 채널 통합 및 도구 확장에 초점을 맞춰 개발하고 있습니다.

> 이 프로젝트는 MIT 라이선스로 배포되는 [HKUDS/nanobot](https://github.com/HKUDS/nanobot)을 기반으로 합니다.

## 목차

- [주요 기능](#주요-기능)
- [설치](#설치)
- [빠른 시작](#빠른-시작)
- [설정](#설정)
  - [프로바이더](#프로바이더)
  - [채널](#채널)
  - [웹 검색](#웹-검색)
  - [미디어 생성](#미디어-생성)
  - [MCP](#mcp-model-context-protocol)
  - [Failover](#failover)
  - [Usage Tracking](#usage-tracking)
  - [Observability](#observability)
- [CLI 레퍼런스](#cli-레퍼런스)
- [프로젝트 구조](#프로젝트-구조)
- [Docker](#docker)
- [Acknowledgments](#acknowledgments)

## 주요 기능

- **멀티 채널 지원** — Telegram, Slack, DingTalk, Discord, WhatsApp, Feishu, QQ, Email, Matrix, Mochat
- **멀티 LLM 프로바이더** — LiteLLM 기반으로 20개+ 프로바이더 지원 (OpenAI, Anthropic, Google, OpenRouter 등)
- **MCP(Model Context Protocol)** — 외부 MCP 서버 연결을 통한 도구 확장
- **내장 스킬** — cron 예약, GitHub 연동, 메모리, 웹 검색, 요약, tmux 등
- **OAuth 인증** — OpenAI Codex, GitHub Copilot OAuth 로그인 지원
- **Failover** — 프로바이더 장애 시 자동 대체 (Circuit Breaker 패턴)
- **Usage Tracking** — 토큰 사용량/비용 추적, 채널 내 `/usage` `/status` 조회
- **Observability** — OpenTelemetry 기반 트레이싱 (OTLP)
- **Heartbeat** — LLM 기반 주기적 백그라운드 작업 실행
- **서브에이전트** — 비동기 백그라운드 작업 스폰 및 관리
- **미디어 생성** — 이미지/비디오 생성 (Gemini Imagen 4, OpenAI-compatible 엔드포인트, 로컬 VLM)
- **실행 상태 모니터** — 도구 반복, 에러 연쇄, 파일 쓰기 폭주 감지

## 설치

```bash
# uv 사용 (권장)
uv sync

# 실행
uv run shacs-bot --help
```

## 빠른 시작

> **API 키 발급**: [OpenRouter](https://openrouter.ai/keys) (글로벌 추천) / 기타 프로바이더는 [프로바이더](#프로바이더) 섹션 참고

**1. 초기 설정**

```bash
uv run shacs-bot onboard
```

**2. 설정** (`~/.shacs-bot/config.json`)

*API 키 설정 (예: OpenRouter):*

```json
{
  "providers": {
    "openrouter": {
      "apiKey": "sk-or-v1-xxx"
    }
  }
}
```

*모델 설정:*

```json
{
  "agents": {
    "defaults": {
      "model": "anthropic/claude-sonnet-4-20250514",
      "provider": "openrouter"
    }
  }
}
```

**3. 대화**

```bash
# 단일 메시지
uv run shacs-bot agent -m "안녕하세요"

# 인터랙티브 모드
uv run shacs-bot agent

# 게이트웨이 모드 (채널 연동)
uv run shacs-bot gateway
```

## 설정

설정 파일: `~/.shacs-bot/config.json` (camelCase)

### 프로바이더

> **Groq**는 Whisper 기반 무료 음성 변환을 제공합니다. 설정 시 Telegram 음성 메시지가 자동으로 텍스트로 변환됩니다.

| 프로바이더 | 용도 | API 키 발급 |
|-----------|------|------------|
| `openrouter` | LLM (추천, 모든 모델 접근) | [openrouter.ai](https://openrouter.ai) |
| `anthropic` | LLM (Claude) | [console.anthropic.com](https://console.anthropic.com) |
| `openai` | LLM (GPT) | [platform.openai.com](https://platform.openai.com) |
| `deepseek` | LLM (DeepSeek) | [platform.deepseek.com](https://platform.deepseek.com) |
| `gemini` | LLM (Gemini) | [aistudio.google.com](https://aistudio.google.com) |
| `groq` | LLM + 음성 변환 (Whisper) | [console.groq.com](https://console.groq.com) |
| `azure_openai` | LLM (Azure OpenAI) | [portal.azure.com](https://portal.azure.com) |
| `dashscope` | LLM (Qwen/통의천문) | [dashscope.console.aliyun.com](https://dashscope.console.aliyun.com) |
| `moonshot` | LLM (Kimi) | [platform.moonshot.cn](https://platform.moonshot.cn) |
| `zhipu` | LLM (GLM) | [open.bigmodel.cn](https://open.bigmodel.cn) |
| `minimax` | LLM (MiniMax) | [platform.minimaxi.com](https://platform.minimaxi.com) |
| `volcengine` | LLM (화산엔진) | [volcengine.com](https://www.volcengine.com) |
| `byteplus` | LLM (BytePlus) | [byteplus.com](https://www.byteplus.com) |
| `siliconflow` | LLM (실리콘플로우) | [siliconflow.cn](https://siliconflow.cn) |
| `aihubmix` | LLM (게이트웨이) | [aihubmix.com](https://aihubmix.com) |
| `ollama` | LLM (로컬) | — |
| `vllm` | LLM (로컬) | — |
| `custom` | OpenAI 호환 엔드포인트 | — |
| `image_gen` | 이미지 생성 전용 | 백엔드에 따라 다름 |
| `openai_codex` | LLM (Codex, OAuth) | `shacs-bot provider login openai-codex` |
| `github_copilot` | LLM (Copilot, OAuth) | `shacs-bot provider login github-copilot` |

<details>
<summary><b>OAuth 로그인 (OpenAI Codex / GitHub Copilot)</b></summary>

OAuth 기반 프로바이더는 API 키 대신 `provider login` 명령으로 인증합니다.

```bash
# OpenAI Codex (ChatGPT Plus/Pro 필요)
uv run shacs-bot provider login openai-codex

# GitHub Copilot (Copilot 플랜 필요)
uv run shacs-bot provider login github-copilot
```

모델 설정:

```json
{
  "agents": {
    "defaults": {
      "model": "openai-codex/gpt-5.1-codex"
    }
  }
}
```

> 자세한 OAuth 설정은 [nanobot README — Providers](https://github.com/HKUDS/nanobot#providers) 참고

</details>

<details>
<summary><b>커스텀 프로바이더 (OpenAI 호환 API)</b></summary>

LM Studio, llama.cpp, Together AI 등 OpenAI 호환 엔드포인트에 직접 연결합니다.

```json
{
  "providers": {
    "custom": {
      "apiKey": "your-api-key",
      "apiBase": "https://api.your-provider.com/v1"
    }
  },
  "agents": {
    "defaults": {
      "model": "your-model-name"
    }
  }
}
```

> 로컬 서버의 경우 `apiKey`에 아무 값이나 설정하세요 (예: `"no-key"`).

</details>

<details>
<summary><b>새 프로바이더 추가하기</b></summary>

`providers/registry.py`에 `ProviderSpec`을 추가하고 `config/schema.py`에 필드를 추가하면 됩니다 (2단계).

> 자세한 가이드는 [nanobot README — Adding a New Provider](https://github.com/HKUDS/nanobot#providers) 참고

</details>

### 채널

> 채널별 상세 설정 가이드는 [nanobot README — Chat Apps](https://github.com/HKUDS/nanobot#-chat-apps) 참고 (설정 형식 동일)

| 채널 | 필요한 것 | 비고 |
|------|----------|------|
| **Telegram** | Bot token ([@BotFather](https://t.me/BotFather)) | 추천, 가장 안정적 |
| **Discord** | Bot token + Message Content intent | 스레드 기반 응답 지원 |
| **WhatsApp** | QR 코드 스캔 (Node.js ≥18 필요) | TypeScript 브리지 사용 |
| **Slack** | Bot token + App-Level token (Socket Mode) | |
| **Feishu (飞书)** | App ID + App Secret | WebSocket, 공인 IP 불필요 |
| **DingTalk (钉钉)** | App Key + App Secret | Stream Mode |
| **QQ** | App ID + App Secret | 개인 메시지만 지원 |
| **Email** | IMAP/SMTP 자격증명 | 이메일 어시스턴트 |
| **Matrix** | Access Token + homeserver | E2EE 지원 |
| **Mochat** | Claw token | WebSocket |

<details>
<summary><b>Telegram 설정 예시</b></summary>

```json
{
  "channels": {
    "telegram": {
      "enabled": true,
      "token": "YOUR_BOT_TOKEN",
      "allowFrom": ["YOUR_USER_ID"]
    }
  }
}
```

```bash
uv run shacs-bot gateway
```

</details>

<details>
<summary><b>WhatsApp 설정</b></summary>

Node.js ≥18이 필요합니다. TypeScript 브리지가 WhatsApp Web과 shacs-bot을 WebSocket으로 연결합니다.

```bash
# 1. QR 코드 로그인
uv run shacs-bot channels login
```

```json
{
  "channels": {
    "whatsapp": {
      "enabled": true,
      "allowFrom": ["+821012345678"]
    }
  }
}
```

```bash
# 2. 실행 (터미널 2개)
uv run shacs-bot channels login   # 터미널 1
uv run shacs-bot gateway          # 터미널 2
```

> 업그레이드 후 브리지를 재빌드하세요: `rm -rf ~/.shacs-bot/bridge && uv run shacs-bot channels login`

</details>

### 웹 검색

```json
{
  "tools": {
    "web": {
      "search": {
        "provider": "brave",
        "apiKey": "BSA..."
      },
      "proxy": "http://127.0.0.1:7890"
    }
  }
}
```

> `proxy`를 설정하면 모든 웹 요청(검색 + 페이지 가져오기)이 프록시를 경유합니다.
>
> 지원 검색 프로바이더: Brave (기본), Tavily, Jina, SearXNG, DuckDuckGo.
> 자세한 옵션은 [nanobot README — Web Search](https://github.com/HKUDS/nanobot#web-search) 참고

### 미디어 생성

`media_generate` 도구로 이미지/비디오를 생성합니다. 기본 비활성화이며, `providers.image_gen`에 API 키를 설정하면 사용할 수 있습니다.

**OpenRouter / OpenAI / 로컬 서버 (OpenAI-compatible):**

```json
{
  "providers": {
    "image_gen": {
      "apiKey": "sk-or-v1-xxx",
      "baseUrl": "https://openrouter.ai/api/v1"
    }
  },
  "tools": {
    "media": {
      "enabled": true,
      "model": "google/gemini-3.1-flash-image-preview"
    }
  }
}
```

**Google Gemini (Imagen 4 / Veo 3):**

```json
{
  "providers": {
    "image_gen": {
      "apiKey": "AIza..."
    }
  },
  "tools": {
    "media": {
      "enabled": true,
      "backend": "gemini"
    }
  }
}
```

| 설정 | 기본값 | 설명 |
|------|--------|------|
| `tools.media.enabled` | `false` | 미디어 생성 도구 활성화 |
| `tools.media.backend` | `"openai-compatible"` | `"gemini"` 또는 `"openai-compatible"` |
| `tools.media.model` | `""` | 이미지 생성 모델명 (OpenAI-compatible 백엔드) |
| `providers.image_gen.apiKey` | `""` | API 키 |
| `providers.image_gen.baseUrl` | — | 엔드포인트 URL (OpenAI-compatible 백엔드) |

> 비디오 생성은 `backend: "gemini"`일 때만 지원됩니다 (Veo 3).
>
> 로컬 VLM 사용 시 `baseUrl`을 로컬 서버로 지정하세요 (예: `http://localhost:8000/v1`).

### MCP (Model Context Protocol)

> Claude Desktop / Cursor의 MCP 설정을 그대로 복사하여 사용할 수 있습니다.

```json
{
  "tools": {
    "mcpServers": {
      "filesystem": {
        "command": "npx",
        "args": ["-y", "@modelcontextprotocol/server-filesystem", "/path/to/dir"]
      },
      "my-remote-mcp": {
        "url": "https://example.com/mcp/",
        "headers": {
          "Authorization": "Bearer xxxxx"
        }
      }
    }
  }
}
```

| 모드 | 설정 | 설명 |
|------|------|------|
| **Stdio** | `command` + `args` | 로컬 프로세스 (`npx` / `uvx`) |
| **HTTP** | `url` + `headers` (선택) | 원격 MCP 엔드포인트 |

> 자세한 MCP 설정은 [nanobot README — MCP](https://github.com/HKUDS/nanobot#mcp-model-context-protocol) 참고

### Failover

프로바이더 장애 시 자동으로 대체 프로바이더로 전환합니다 (Circuit Breaker 패턴).

```json
{
  "failover": {
    "enabled": true,
    "cooldownSeconds": 300,
    "rules": [
      {
        "fromProvider": "anthropic",
        "toProvider": "openrouter",
        "modelMap": {
          "claude-sonnet-4-20250514": "anthropic/claude-sonnet-4-20250514"
        }
      }
    ]
  }
}
```

### Usage Tracking

토큰 사용량과 비용을 자동 추적합니다. 채널 내 슬래시 커맨드로 조회하거나, 응답 footer에 표시할 수 있습니다.

```json
{
  "usage": {
    "enabled": true,
    "footer": "full"
  }
}
```

| 설정 | 값 | 설명 |
|------|---|------|
| `enabled` | `true` (기본) | 사용량 추적 활성화 |
| `footer` | `"off"` (기본) / `"tokens"` / `"full"` | 응답 끝에 사용량 표시 |

**슬래시 커맨드:**

| 명령어 | 설명 |
|--------|------|
| `/usage` | 현재 세션 + 오늘 전체 토큰/비용 요약 |
| `/status` | 현재 모델, 프로바이더, 세션 상태 |

> 사용량 데이터는 `~/.shacs-bot/usage/{YYYY-MM-DD}.jsonl`에 일별로 저장됩니다. 비용은 `litellm`의 모델별 가격 테이블로 계산되며, 가격 정보가 없는 모델(Ollama, vLLM 등)은 토큰 수만 기록됩니다.

### Observability

OpenTelemetry 기반 트레이싱을 지원합니다. `opentelemetry` 패키지가 설치되어 있으면 자동 활성화됩니다.

```json
{
  "observability": {
    "enabled": true,
    "otlpEndpoint": "http://localhost:4317",
    "serviceName": "shacs-bot",
    "sampleRate": 1.0
  }
}
```

## CLI 레퍼런스

| 명령어 | 설명 |
|--------|------|
| `shacs-bot onboard` | 초기 설정 (워크스페이스 + config 생성) |
| `shacs-bot agent` | 에이전트와 대화 (인터랙티브) |
| `shacs-bot agent -m "메시지"` | 단일 메시지 전송 |
| `shacs-bot agent -s SESSION_ID` | 특정 세션 이어서 대화 |
| `shacs-bot gateway` | 게이트웨이 시작 (채널 연동) |
| `shacs-bot gateway -p 8080` | 포트 지정 |
| `shacs-bot status` | 설정/프로바이더 상태 확인 |
| `shacs-bot channels status` | 채널 상태 확인 |
| `shacs-bot channels login` | WhatsApp QR 로그인 |
| `shacs-bot provider login openai-codex` | OpenAI Codex OAuth 로그인 |
| `shacs-bot provider login github-copilot` | GitHub Copilot OAuth 로그인 |

## 프로젝트 구조

```
shacs_bot/
  agent/         # 에이전트 루프, 도구, 세션 관리, 서브에이전트
  bus/           # 메시지 버스 (인바운드/아웃바운드)
  channels/      # 채널 어댑터 (Telegram, Slack, ...)
  cli/           # CLI 커맨드
  config/        # 설정 로딩, 스키마
  heartbeat/     # Heartbeat 서비스 (주기적 LLM 백그라운드 작업)
  observability/ # OpenTelemetry 트레이싱
  providers/     # LLM 프로바이더 (LiteLLM, Custom, OAuth, Failover)
  skills/        # 내장 스킬
  utils/         # 유틸리티
bridge/          # WhatsApp 브리지 (TypeScript)
```

## Docker

```bash
# 게이트웨이 모드
docker compose up -d shacs-bot-gateway

# CLI 모드 (인터랙티브)
docker compose run --rm shacs-bot-cli
```

> `~/.shacs-bot` 디렉토리가 볼륨으로 마운트됩니다. WhatsApp 사용 시 Node.js 20이 컨테이너에 포함되어 있습니다.

## Acknowledgments

이 프로젝트는 [HKUDS/nanobot](https://github.com/HKUDS/nanobot) (MIT License)을 기반으로 합니다.
