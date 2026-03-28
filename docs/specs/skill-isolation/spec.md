# SPEC: 스킬 격리 실행

> **Prompt**: 스킬은 신뢰할 수 없는 외부 코드다. 모든 스킬을 서브에이전트로 격리 실행하고, workspace 스킬에는 출처 기반 승인 게이트(auto/manual/off)를 적용하여 보안 경계를 확보한다.

## PRDs

| PRD | 설명 |
|---|---|
| [`skill-isolation.md`](./prds/skill-isolation.md) | 스킬 격리 실행 v3 — 전체 스킬 서브에이전트, `/skill trust auto\|manual\|off` 모드 전환, 세션 맥락 LLM 판단 + 사용자 직접 승인 |
