# SPEC: 설정 및 인증 관리 통합

> **Prompt**: config.json으로 글로벌 환경변수를 관리할 방법이 없고, OAuth 토큰이 설정 디렉토리와 별도 경로에 저장되어 Docker/백업 시 일관성 문제 발생.

## PRDs

| PRD | 설명 |
|---|---|
| [`global-env-injection.md`](./prds/global-env-injection.md) | config.json 글로벌 환경변수 주입 메커니즘 |
| [`oauth-token-storage-consolidation.md`](./prds/oauth-token-storage-consolidation.md) | OAuth 토큰 저장 경로를 `~/.shacs-bot/` 하위로 통합 |
