# SPEC: Operator Console (CLI/TUI)

> **Prompt**: 웹 대시보드 없이도 shacs-bot을 운영할 수 있도록 session/workflow/approval/usage를 점검하는 operator CLI/TUI를 추가한다.

## PRDs

| PRD | 설명 |
|---|---|
| [`operator-console.md`](./prds/operator-console.md) | inspect-only admin 명령, 표 출력, 조회 helper를 구현 태스크로 분해 |

## TL;DR

> **목적**: 장시간 상주형 assistant 운영에서 필요한 최소 운영 표면을 웹 UI 없이 제공한다.
>
> **Deliverables**:
> - `shacs_bot/cli/commands.py` — `admin sessions`, `admin approvals`, `admin workflows`, `admin usage`
> - `shacs_bot/agent/session/manager.py` — 상세 조회 helper
> - `shacs_bot/workflow/store.py` — workflow 상태 조회 helper
> - `shacs_bot/agent/approval.py`, `shacs_bot/agent/usage.py` — CLI 조회용 helper 보강
> - `docs/specs/operator-console/checklists/requirements.md` — 스펙 품질 체크리스트
>
> **Estimated Effort**: Short (2-4시간)

## User Scenarios & Testing

### Scenario 1 - 운영자는 최근 세션 상태를 즉시 확인할 수 있다

운영자는 gateway를 붙여 디버깅하지 않고도 로컬 상태 파일만으로 최근 세션/작업 상황을 파악할 수 있어야 한다.

**테스트**: admin sessions 명령으로 최근 세션 목록과 메타데이터가 표시되는지 확인한다.

### Scenario 2 - 승인 대기와 background 작업을 한 곳에서 본다

운영자는 승인 대기 요청과 실패한 workflow를 빠르게 찾아야 한다.

**테스트**: admin approvals, admin workflows 명령이 비어 있거나 값이 있을 때 모두 정상 동작하는지 확인한다.

## Functional Requirements

- **FR-001**: 시스템은 세션, 승인 대기, workflow, usage 상태를 CLI에서 조회할 수 있어야 한다.
- **FR-002**: 조회 명령은 gateway 미실행 상태에서도 로컬 파일/메모리 상태를 읽어야 한다.
- **FR-003**: 출력은 빈 상태에서도 오류 없이 동작해야 한다.
- **FR-004**: 1단계 operator console은 inspect-only여야 하며 파괴적 변경을 포함하면 안 된다.
- **FR-005**: 운영자는 필터/범위 옵션으로 필요한 상태만 좁혀 볼 수 있어야 한다.

## Key Entities

- **Session Summary**: key, updated_at, message count, metadata를 요약한 조회 단위
- **Pending Approval**: 해결 전 상태의 approval 요청
- **Workflow Summary**: state, retries, notify target을 요약한 background 작업 레코드

## Success Criteria

- 운영자가 CLI만으로 주요 상태(session, approval, workflow, usage)를 조회할 수 있다.
- 상태가 비어 있어도 명령이 실패하지 않는다.
- inspect-only 범위에서 파괴적 조작 없이 운영 가시성을 확보한다.
- 로컬 상태 기반 운영 점검 시간이 줄어든다.

## Assumptions

- 1단계는 Rich 기반 CLI/TUI 수준까지만 다룬다.
- 웹 UI, 실시간 대시보드, RBAC는 범위 밖이다.
- background workflow store가 존재한다는 전제를 두되, 없을 때도 graceful fallback 한다.

## 현재 상태 분석

- 현재 CLI에는 `status`, `channels status`, provider login, gateway 실행 등이 있다.
- approval은 내부 `_pending_approvals` 레지스트리에만 존재한다.
- session/usage/workflow 상태를 운영자가 바로 보기 위한 공통 콘솔은 없다.

## 설계

### 명령 구조

```text
shacs-bot admin sessions [--limit N] [--show-meta]
shacs-bot admin approvals
shacs-bot admin workflows [--state running]
shacs-bot admin usage [--date YYYY-MM-DD] [--session KEY]
```

### 범위

- inspect-only admin 명령
- rich table 출력
- local helper 재사용

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
