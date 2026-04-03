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
  - [Lifecycle Hooks](#lifecycle-hooks)
  - [Workflow Runtime](#workflow-runtime)
  - [Evaluation Harness](#evaluation-harness)
  - [Custom Agents](#custom-agents)
- [CLI 레퍼런스](#cli-레퍼런스)
- [프로젝트 구조](#프로젝트-구조)
- [Docker](#docker)
- [Acknowledgments](#acknowledgments)

## 주요 기능

- **멀티 채널 지원** — Telegram, Slack, DingTalk, Discord, WhatsApp, Feishu, QQ, Email, Matrix, Mochat
- **멀티 LLM 프로바이더** — 네이티브 Anthropic + OpenAI SDK 기반으로 20개+ 프로바이더 지원 (OpenAI, Anthropic, Google, OpenRouter 등)
- **MCP(Model Context Protocol)** — 외부 MCP 서버 연결을 통한 도구 확장
- **내장 스킬** — cron 예약, GitHub 연동, 메모리, 웹 검색, 요약, tmux 등
- **스킬 격리** — 모든 스킬을 서브에이전트에서 실행. workspace 스킬은 3단계 승인 게이트 (규칙→파일쓰기→LLM 분류기) 적용
- **OAuth 인증** — OpenAI Codex, GitHub Copilot OAuth 로그인 지원
- **Failover** — 프로바이더 장애 시 자동 대체 (Circuit Breaker 패턴)
- **Usage Tracking** — 토큰 사용량/비용 추적, 채널 내 `/usage` `/status` 조회
- **Observability** — OpenTelemetry 기반 트레이싱 (OTLP)
- **Heartbeat** — LLM 기반 주기적 백그라운드 작업 실행
- **Planned Workflow** — step 기반 workflow 실행, `ask_user` / `request_approval` / `wait_until` 단계 대기 및 재개
- **서브에이전트** — 비동기 백그라운드 작업 스폰 및 관리
- **Evaluation Harness** — workspace 기반 평가 케이스 실행, baseline 비교, self-eval 정책/상태 관리
- **Custom Agents** — Git 저장소 또는 로컬 디렉터리에서 에이전트/스킬 번들을 설치·업데이트·삭제
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
| `/skill trust auto\|manual\|off` | 스킬 승인 모드 변경 (auto: LLM 판단, manual: 사용자 승인, off: 무승인) |

> 사용량 데이터는 `~/.shacs-bot/usage/{YYYY-MM-DD}.jsonl`에 일별로 저장됩니다. 비용은 내장 가격 테이블로 계산되며, 가격 정보가 없는 모델(Ollama, vLLM 등)은 토큰 수만 기록됩니다.

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

### Lifecycle Hooks

Lifecycle Hooks는 메시지 처리 경계에서 **운영용 관측 로직**이나 **가벼운 후처리 로직**을 붙일 수 있게 하는 in-process hook 시스템입니다. 현재 구현은 "기존 동작을 깨지 않는 것"을 가장 우선으로 두고 있습니다.

- hooks가 꺼져 있으면 `NoOpHookRegistry`를 사용하므로 기존 gateway/chat 경로가 그대로 유지됩니다.
- hook handler는 등록 순서대로 순차 실행됩니다.
- handler가 실패해도 메인 응답 경로는 계속 진행됩니다.
- `before_outbound_send`만 outbound `content`, `media` 수정이 가능하고, 그 외 이벤트는 observer-only입니다.

기본 설정 예시:

```json
{
  "hooks": {
    "enabled": true,
    "redactPayloads": true,
    "outboundMutationEnabled": false
  }
}
```

| 설정 | 기본값 | 설명 |
|------|--------|------|
| `hooks.enabled` | `false` | Lifecycle Hooks 자체를 활성화합니다. |
| `hooks.redactPayloads` | `true` | built-in example hook 로그에서 payload를 축약해서 남깁니다. |
| `hooks.outboundMutationEnabled` | `false` | `before_outbound_send`에서 수정한 `content`, `media`를 실제 전송에 반영합니다. |

`hooks.enabled`를 켜면 built-in example hook도 함께 등록됩니다. 이 예제는 기능을 바꾸기 위한 샘플이 아니라, 어떤 이벤트가 어떤 맥락에서 발생하는지 **loguru 로그로 바로 확인할 수 있게 해 주는 운영용 기준점**입니다.

기본 예제 hook가 관측하는 이벤트:

- `session_loaded`
- `after_tool_execute`
- `approval_resolved`
- `after_outbound_send`
- `heartbeat_decided`
- `background_job_completed`

예를 들어 hook가 활성화된 상태에서 세션이 열리거나 승인/heartbeat 이벤트가 발생하면 아래와 비슷한 로그가 남습니다.

```text
Lifecycle hook example: event=session_loaded session_key=cli:test channel=cli payload={}
Lifecycle hook example: event=approval_resolved session_key=sess-2 channel=dummy payload={'tool': 'exec', 'reason': 'ok'}
```

`redactPayloads`가 `true`면 example hook는 전체 payload를 그대로 찍지 않고, 운영 관점에서 유용한 일부 필드만 남깁니다. 현재 예제는 `tool`, `tier`, `denied`, `reason`, `chat_id`, `action`, `has_tool_calls`, `result_length`, `is_error` 정도만 로그에 포함합니다.

outbound 수정이 필요한 경우에는 `hooks.outboundMutationEnabled`를 명시적으로 켜야 합니다.

```json
{
  "hooks": {
    "enabled": true,
    "outboundMutationEnabled": true
  }
}
```

이 설정이 꺼져 있으면 `before_outbound_send` handler가 `content`나 `media`를 바꿔도 실제 전송 메시지에는 반영되지 않습니다. 즉, 기본값은 **관측만 허용**하고, 실제 메시지 변형은 opt-in입니다.

현재 정의된 주요 이벤트 범주는 다음과 같습니다.

- inbound: `message_received`
- session/context: `session_loaded`, `before_context_build`
- llm: `before_llm_call`, `after_llm_call`
- tool: `before_tool_execute`, `after_tool_execute`
- outbound: `before_outbound_send`, `after_outbound_send`
- approval: `approval_requested`, `approval_resolved`
- heartbeat/background: `heartbeat_decided`, `background_job_completed`

실제 custom handler를 붙이려면 현재는 코드 레벨에서 `HookRegistry.register()`를 사용해야 합니다. built-in example hook는 **"이벤트가 실제로 보이는지 확인하는 기준점"**으로 두고, 이후 운영 로깅/정책/알림 로직을 여기에 맞춰 확장하면 됩니다.

### Workflow Runtime

Workflow Runtime은 heartbeat, planner, 채널 명령이 만든 background workflow를 workspace에 저장하고, step 단위로 재개 가능한 실행 상태를 관리합니다.

- planned workflow는 step 배열을 순차 실행하며 각 step 결과를 workflow 메타데이터에 기록합니다.
- `ask_user` step은 사용자 답변을 기다렸다가 같은 workflow를 이어서 실행합니다.
- `request_approval` step은 승인/거절 결과를 반영한 뒤 다음 step으로 재개합니다.
- `wait_until` step은 지정한 시각까지 대기한 뒤 queued 상태로 다시 스케줄됩니다.
- CLI에서 workflow 상태를 조회하고, 멈춘 workflow를 수동 복구할 수 있습니다.

```bash
uv run shacs-bot workflows status
uv run shacs-bot workflows recover <workflow-id>
```

### Evaluation Harness

Evaluation Harness는 workspace 안에서 **회귀 확인용 케이스 번들**을 관리하고, 동일한 케이스를 여러 runtime variant / provider:model 조합으로 비교 실행할 수 있는 평가 도구입니다.

- `eval run` — JSON 케이스 파일을 실행하고 variant별 요약을 생성합니다.
- `eval extract` — 기존 세션에서 평가 케이스를 추출해 JSON bundle로 저장합니다.
- `eval auto-run` — 기본 케이스 + 세션 추출 케이스를 묶어 baseline 비교와 self-eval을 한 번에 수행합니다.
- `eval status` — 최근 self-eval 상태, 추천 runtime, candidate 점수를 조회합니다.
- `eval policy` — self-eval 트리거, 스케줄, candidate 목록을 조정합니다.

기본 예시:

```bash
# 기본 케이스 실행
uv run shacs-bot eval run

# 특정 variant 비교 실행
uv run shacs-bot eval run --variant default --variant cautious

# 최근 세션에서 평가 케이스 추출
uv run shacs-bot eval extract --session-limit 20 --case-limit 30

# baseline 비교 + 후보 모델 비교
uv run shacs-bot eval auto-run \
  --baseline \
  --candidate openrouter:anthropic/claude-sonnet-4-20250514 \
  --candidate openai:gpt-5

# self-eval 정책 확인/수정
uv run shacs-bot eval status
uv run shacs-bot eval policy --turn-threshold 50 --schedule-kind every --schedule-every-minutes 360
```

평가 결과는 workspace 아래 `evals/` 디렉토리에 저장되며, auto-run 상태 파일에는 baseline run, variant health, candidate score, 다음 트리거 정보가 기록됩니다.

### Custom Agents

Custom Agent 시스템은 Git 저장소에 담긴 `agent.toml` / `agents/*.toml` 정의와 번들 스킬을 workspace에 설치하는 기능입니다.

- `/agent install <git-url|local-dir>` — Git 저장소를 shallow clone하거나 로컬 디렉터리에서 에이전트와 스킬을 설치
- `/agent list` — 설치된 에이전트 목록 확인
- `/agent update [name]` — 설치된 에이전트를 최신 커밋으로 업데이트
- `/agent remove <name>` — 에이전트와 연관 스킬 제거

내장 `agent-builder` 스킬은 커스텀 에이전트 패키지 골격과 스킬 번들을 만드는 가이드를 제공합니다. 설치된 에이전트는 workspace 레벨에 배치되며 ApprovalGate 제약을 그대로 적용받습니다.

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
| `shacs-bot workflows status` | background/planned workflow 상태 조회 |
| `shacs-bot workflows status --all --state waiting_input` | 완료 포함/상태별 workflow 필터 조회 |
| `shacs-bot workflows recover <workflow-id>` | 대기/실패 workflow를 queued 상태로 수동 복구 |
| `shacs-bot provider login openai-codex` | OpenAI Codex OAuth 로그인 |
| `shacs-bot provider login github-copilot` | GitHub Copilot OAuth 로그인 |
| `shacs-bot eval run` | 평가 케이스 실행 |
| `shacs-bot eval extract` | 기존 세션에서 평가 케이스 추출 |
| `shacs-bot eval auto-run` | baseline 비교 + self-eval 자동 실행 |
| `shacs-bot eval status` | self-eval 상태 및 추천 runtime 조회 |
| `shacs-bot eval policy` | self-eval 트리거/스케줄/후보 모델 정책 수정 |

## 프로젝트 구조

```
shacs_bot/
  agent/         # 에이전트 루프, 도구, 세션 관리, 서브에이전트
  bus/           # 메시지 버스 (인바운드/아웃바운드)
  channels/      # 채널 어댑터 (Telegram, Slack, ...)
  cli/           # CLI 커맨드
  config/        # 설정 로딩, 스키마
  evals/         # 평가 케이스, runner, self-eval 상태/정책 관리
  heartbeat/     # Heartbeat 서비스 (주기적 LLM 백그라운드 작업)
  observability/ # OpenTelemetry 트레이싱
  providers/     # LLM 프로바이더 (Anthropic, OpenAI-compatible, OAuth, Failover)
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
