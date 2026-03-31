# SPEC: Operator Console (CLI/TUI)

> **Prompt**: 웹 대시보드 없이도 shacs-bot을 운영할 수 있도록 session/workflow/approval/usage를 점검하는 operator CLI/TUI를 추가한다.

## PRDs

| PRD | 설명 |
|---|---|
| [`operator-console.md`](./prds/operator-console.md) | Rich 기반 admin commands + local inspection workflow |

## TL;DR

> **목적**: 장시간 상주형 assistant 운영에서 필요한 최소 운영 표면을 웹 UI 없이 제공한다.
>
> **Deliverables**:
> - `cli/commands.py` — `admin sessions`, `admin approvals`, `admin workflows`, `admin usage` 명령
> - `agent/session/manager.py` — 상세 조회 helper
> - `workflow/store.py` — 상태 조회 helper
> - `agent/approval.py` — pending approval 조회 helper 활용
>
> **Estimated Effort**: Short (2-4시간)

## 현재 상태 분석

- 현재 CLI에는 `status`, `channels status`, provider login, gateway 실행 등이 있다.
- approval은 내부 `_pending_approvals` 레지스트리에만 존재한다.
- session/usage/workflow 상태를 운영자가 바로 보기 위한 공통 콘솔은 없다.

assistant product는 코드보다 운영 문제가 먼저 보이는 경우가 많다.

- 어느 세션이 최근에 업데이트됐는가
- 승인 대기 중인 요청은 무엇인가
- 어떤 background workflow가 실패/정지했는가
- 오늘 usage hotspot은 어디인가

## 설계

### 명령 구조

```text
shacs-bot admin sessions [--limit N] [--show-meta]
shacs-bot admin approvals
shacs-bot admin workflows [--state running]
shacs-bot admin usage [--date YYYY-MM-DD] [--session KEY]
```

### 출력 원칙

- `rich` table 중심
- destructive action은 이번 범위에서 제외, 먼저 inspect-only
- session key, channel, updated_at, state, cost를 핵심 열로 노출

### 1단계 범위

1. sessions list / inspect
2. approvals list
3. workflows list
4. usage summary / per-session 조회

### 비목표

- 웹 admin panel
- 실시간 streaming dashboard
- multi-user RBAC UI

## 파일 변경 목록

| 파일 | 변경 | 설명 |
|------|:---:|------|
| `shacs_bot/cli/commands.py` | 수정 | admin 명령 추가 |
| `shacs_bot/agent/session/manager.py` | 수정 | inspect helper 추가 |
| `shacs_bot/agent/approval.py` | 수정 | pending approval list helper 추가 |
| `shacs_bot/workflow/store.py` | 수정 | workflow list/inspect helper 추가 |
| `shacs_bot/agent/usage.py` | 수정 | aggregate 결과를 CLI-friendly shape로 확장 |

## 검증 기준

- [ ] gateway 실행 없이도 local 상태 파일 inspection 가능
- [ ] session/approval/workflow/usage 정보를 CLI에서 읽을 수 있음
- [ ] destructive mutation 없이 inspect-only로 시작함
- [ ] rich 출력이 비어 있는 상태에서도 오류 없이 동작함

## Must NOT

- operator console 때문에 별도 웹 인프라를 요구하지 않는다.
- inspect-only 1단계에서 승인 강제처리/세션삭제 같은 파괴 액션을 넣지 않는다.
