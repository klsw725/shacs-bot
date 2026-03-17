# shacs-bot

**shacs-bot**은 [nanobot](https://github.com/HKUDS/nanobot)을 클론 코딩하여 확장한 개인용 AI 어시스턴트 프레임워크입니다.

nanobot의 경량 에이전트 아키텍처를 기반으로, 멀티 채널 통합 및 도구 확장에 초점을 맞춰 개발하고 있습니다.

> 이 프로젝트는 MIT 라이선스로 배포되는 [HKUDS/nanobot](https://github.com/HKUDS/nanobot)을 기반으로 합니다.

## 주요 기능

- **멀티 채널 지원** - Telegram, Slack, DingTalk, Discord, WhatsApp, Feishu, QQ, Email, Matrix
- **멀티 LLM 프로바이더** - LiteLLM 기반으로 OpenAI, Anthropic, Google, OpenRouter 등 다양한 모델 사용 가능
- **MCP(Model Context Protocol)** - 외부 MCP 서버 연결을 통한 도구 확장
- **내장 스킬** - cron 예약, GitHub 연동, 메모리, 웹 검색, 요약, tmux 등
- **OAuth 인증** - OpenAI Codex, GitHub Copilot 지원
- **Failover** - 프로바이더 장애 시 자동 대체
- **Observability** - OpenTelemetry 기반 트레이싱

## 설치

```bash
# uv 사용 (권장)
uv sync

# 실행
uv run shacs-bot --help
```

## 빠른 시작

```bash
# 1. 초기 설정
uv run shacs-bot onboard

# 2. ~/.shacs-bot/config.json에 API 키 설정

# 3. CLI로 대화
uv run shacs-bot agent -m "안녕하세요"

# 4. 인터랙티브 모드
uv run shacs-bot agent

# 5. 게이트웨이 모드 (채널 연동)
uv run shacs-bot gateway
```

## 프로젝트 구조

```
shacs_bot/
  agent/       # 에이전트 루프, 도구, 세션 관리
  bus/         # 메시지 버스 (인바운드/아웃바운드)
  channels/    # 채널 어댑터 (Telegram, Slack, ...)
  cli/         # CLI 커맨드
  config/      # 설정 로딩, 스키마
  providers/   # LLM 프로바이더 (LiteLLM, Custom, OAuth)
  skills/      # 내장 스킬
  utils/       # 유틸리티
bridge/        # WhatsApp 브리지 (TypeScript)
```

## Docker

```bash
docker compose up -d shacs-bot-gateway
```

## Acknowledgments

이 프로젝트는 [HKUDS/nanobot](https://github.com/HKUDS/nanobot) (MIT License)을 기반으로 합니다.
