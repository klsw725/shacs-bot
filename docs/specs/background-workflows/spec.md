# SPEC: Lightweight Background Workflows

> **Prompt**: cron/heartbeat/subagent를 유지하면서, 별도 인프라 없이 실행 상태를 저장하는 경량 background workflow 레이어를 추가한다.

## PRDs

| PRD | 설명 |
|---|---|
| [`background-workflows.md`](./prds/background-workflows.md) | file-backed workflow state + retry/resume + notify integration |

## TL;DR

> **목적**: shacs-bot을 "지금 답하는 assistant"에서 "작업을 이어서 수행하는 assistant"로 확장하되, Redis/Celery 없이 현재 앱 내부에서 운영 가능한 workflow 상태 레이어를 만든다.
>
> **Deliverables**:
> - `workflow/store.py` — JSONL/file-backed workflow state store
> - `workflow/runtime.py` — queued/running/waiting/retry/completed 상태기계
> - `agent/tools/cron/*`, `heartbeat/service.py`, `agent/subagent.py` 연동
> - `bus/events.py` — workflow completion/notification metadata
>
> **Estimated Effort**: Medium (5-7시간)

## 현재 상태 분석

- `heartbeat/service.py`는 `skip/run` 결정 후 `on_execute`, `on_notify` 콜백만 제공한다.
- cron은 예약 실행에 강하지만 다단계 상태 추적은 없다.
- subagent/background task는 실행 중/완료 상태를 제공하지만, 장기 assistant workflow 관점의 공통 상태 모델은 없다.
- `MessageBus`는 in-memory queue라 프로세스 재시작 시 작업 상태를 보존하지 않는다.

즉, 지금은 background capability는 있으나, "실행 중인 assistant 일"을 공통 구조로 다루는 계층이 없다.

## 설계

### 상태 모델

```text
queued → running → waiting_input
running → retry_wait
running → completed
running → failed
```

### 경량 범위

- file-backed store (`workspace/workflows/*.json`)
- runtime 재시작 시 incomplete workflow 복구
- retry policy는 단순 attempt + next_run_at
- notification target은 기존 channel/chat_id/session_key 재사용

### 포함 시나리오

1. heartbeat가 찾은 할 일 실행
2. planner가 만든 후속 작업 예약/재개
3. 승인 대기 후 이어서 실행
4. subagent 결과를 나중에 사용자 채널로 전달

### 비목표

- Temporal/Airflow/n8n 급 DAG 엔진
- 분산 worker
- 정확히 한 번(exactly-once) 보장

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
