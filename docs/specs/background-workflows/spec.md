# SPEC: Lightweight Background Workflows

> **Prompt**: cron/heartbeat/subagent를 유지하면서, 별도 인프라 없이 실행 상태를 저장하는 경량 background workflow 레이어를 추가한다.

## PRDs

| PRD | 설명 |
|---|---|
| [`background-workflows.md`](./prds/background-workflows.md) | workflow 상태 저장, retry/resume, cron/heartbeat/subagent 연결을 구현 태스크로 분해 |

## TL;DR

> **목적**: shacs-bot을 "지금 답하는 assistant"에서 "작업을 이어서 수행하는 assistant"로 확장하되, 현재 앱 내부에서 운영 가능한 workflow 상태 레이어를 제공한다.
>
> **Deliverables**:
> - `shacs_bot/workflow/store.py` — workflow 상태 저장
> - `shacs_bot/workflow/runtime.py` — queued/running/waiting/retry/completed 상태기계
> - `shacs_bot/heartbeat/service.py`, `shacs_bot/agent/subagent.py`, `shacs_bot/agent/tools/cron/service.py` — workflow 연계
> - `docs/specs/background-workflows/checklists/requirements.md` — 스펙 품질 체크리스트
>
> **Estimated Effort**: Medium (5-7시간)

## User Scenarios & Testing

### Scenario 1 - assistant가 나중에 끝나는 일을 이어서 처리한다

사용자는 즉시 끝나지 않는 요청을 맡기고, 완료되면 같은 채널에서 결과를 받아야 한다.

**테스트**: background 실행이 완료된 뒤 지정된 채널/세션으로 결과가 전달되는지 확인한다.

### Scenario 2 - 프로세스가 재시작되어도 미완료 작업 상태를 잃지 않는다

운영자는 게이트웨이 재시작 후에도 진행 중이던 assistant 작업을 추적할 수 있어야 한다.

**테스트**: 재시작 후 incomplete workflow가 복구되고 재개 가능한지 확인한다.

## Functional Requirements

- **FR-001**: 시스템은 assistant background 작업의 상태를 queued, running, waiting, retry, completed, failed 중 하나로 저장해야 한다.
- **FR-002**: background workflow는 프로세스 재시작 후에도 미완료 상태를 복구할 수 있어야 한다.
- **FR-003**: workflow는 cron, heartbeat, subagent 결과와 연결될 수 있어야 한다.
- **FR-004**: retry가 필요한 작업은 다음 실행 시점과 시도 횟수를 저장해야 한다.
- **FR-005**: workflow 완료 시 기존 outbound 경로로 결과를 전달할 수 있어야 한다.

## Key Entities

- **Workflow Record**: goal, state, retries, notify target을 보관하는 상태 객체
- **Workflow Runtime**: 상태 전이와 resume/retry를 담당하는 실행기
- **Notify Target**: channel, chat_id, session_key 등 완료 통지 대상 정보

## Success Criteria

- 미완료 background 작업이 재시작 후에도 조회 가능하다.
- cron/heartbeat/subagent에서 발생한 작업이 공통 workflow 상태로 관리된다.
- 완료된 workflow는 사용자가 지정한 채널에서 결과를 받을 수 있다.
- 별도 큐 인프라 없이도 기본 retry/resume 동작이 가능하다.

## Assumptions

- 1단계는 단일 프로세스/파일 기반 저장만 다룬다.
- 분산 worker, exactly-once 보장은 범위 밖이다.
- planner는 workflow를 생성할 수 있지만 상태기는 planner와 별도 책임을 가진다.

## 현재 상태 분석

- `heartbeat/service.py`는 `skip/run` 결정 후 `on_execute`, `on_notify` 콜백만 제공한다.
- cron은 예약 실행에 강하지만 다단계 상태 추적은 없다.
- subagent/background task는 실행 중/완료 상태를 제공하지만, 장기 assistant workflow 관점의 공통 상태 모델은 없다.
- `MessageBus`는 in-memory queue라 프로세스 재시작 시 작업 상태를 보존하지 않는다.

## 설계

### 상태 모델

```text
queued → running → waiting_input
running → retry_wait
running → completed
running → failed
```

### 범위

- file-backed store
- runtime 재시작 시 incomplete workflow 복구
- retry 시도 횟수와 next_run_at 저장
- notify target 재사용

## 파일 변경 목록

| 파일 | 변경 | 설명 |
|------|:---:|------|
| `shacs_bot/workflow/store.py` | 신규 | workflow persistence |
| `shacs_bot/workflow/runtime.py` | 신규 | 상태 전이 및 retry/resume |
| `shacs_bot/heartbeat/service.py` | 수정 | workflow runtime 경유 실행 |
| `shacs_bot/agent/subagent.py` | 수정 | background task 결과를 workflow state로 연결 |
| `shacs_bot/agent/tools/cron/service.py` | 수정 | 예약 작업을 workflow runtime과 통합 |
| `shacs_bot/bus/events.py` | 수정 | workflow metadata 추가 |

## 검증 기준

- [ ] workflow store 없이도 기존 cron/heartbeat 동작 regression 없음
- [ ] 프로세스 재시작 후 incomplete workflow가 재개 가능
- [ ] retry_wait / waiting_input / completed 상태가 저장됨
- [ ] 완료 시 기존 outbound 경로로 notify 가능

## Must NOT

- Redis/Celery 같은 별도 인프라를 전제로 설계하지 않는다.
- planner 로직과 runtime 상태기를 한 파일에 섞지 않는다.
