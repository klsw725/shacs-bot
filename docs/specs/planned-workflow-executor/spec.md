# SPEC: Planned Workflow Executor

> **Prompt**: assistant workflow planner가 만든 `planned_workflow`를 goal 재실행이 아니라 step executor 기반으로 실행하도록 확장한다.

## PRDs

| PRD | 설명 |
|---|---|
| [`planned-workflow-executor.md`](./prds/planned-workflow-executor.md) | step 기반 executor, runtime 연계, recovery/resume 방향을 전체 구현 태스크로 분해 |
| [`planned-workflow-executor-m1.md`](./prds/planned-workflow-executor-m1.md) | executor 뼈대와 `research -> summarize -> send_result` 최소 경로를 첫 구현 단위로 고정 |

## TL;DR

> **목적**: planner가 만든 `PlanStep` 목록을 실제 실행 가능한 workflow 단계로 바꿔, `planned_workflow`가 단순 queue 등록이나 goal 재실행이 아니라 step-by-step 실행으로 이어지게 한다.
>
> **Deliverables**:
> - `shacs_bot/workflow/runtime.py` — current step / step result metadata 연계
> - `shacs_bot/workflow/redispatcher.py` — manual workflow를 step executor 진입점으로 연결
> - `shacs_bot/agent/loop.py` 또는 executor 전용 모듈 — planned workflow 실행 진입점
> - `shacs_bot/agent/planner.py` — step taxonomy 소비 계약 정리(필요 시)
> - planner/eval 관련 문서 및 검증 케이스 — step execution 시나리오 추가
> - `docs/specs/planned-workflow-executor/checklists/requirements.md` — 스펙 품질 체크리스트
>
> **Estimated Effort**: Medium (6-10시간)

## User Scenarios & Testing

### Scenario 1 - planner가 만든 기본 후속조치를 실제 단계로 실행한다

사용자는 "먼저 조사하고, 그 다음 요약해서 알려줘" 같은 요청을 맡겼을 때 assistant가 step 목록을 실제로 순서대로 수행하길 기대한다.

**테스트**: `research -> summarize -> send_result` 계획이 step executor를 통해 end-to-end로 실행되는지 확인한다.

### Scenario 2 - 실행 중간 상태를 workflow가 기억하고 재개한다

사용자는 workflow가 중간에 멈추더라도 현재 어느 step인지 잃지 않고 이어서 처리되길 기대한다.

**테스트**: current step metadata가 저장되고 queued redispatch 또는 recover 시 현재 step 기준으로 재개되는지 확인한다.

### Scenario 3 - 대기형 step은 조용히 무시되지 않는다

사용자는 `ask_user`, `request_approval`, `wait_until` 같은 step이 지원되지 않더라도 침묵 실패 대신 명시적인 상태 전이 또는 안내를 받아야 한다.

**테스트**: 미지원/대기형 step이 명시적으로 `waiting_input` 또는 실패 상태로 반영되는지 확인한다.

## Functional Requirements

- **FR-001**: 시스템은 `planned_workflow`의 `steps`를 순서대로 소비하는 실행 경로를 제공해야 한다.
- **FR-002**: 최소 구현은 `research`, `summarize`, `send_result` step을 지원해야 한다.
- **FR-003**: workflow는 현재 실행 중인 step index와 step 결과 요약을 저장해야 한다.
- **FR-004**: manual workflow redispatch는 goal 재실행이 아니라 step executor 진입으로 연결되어야 한다.
- **FR-005**: `ask_user`, `wait_until`, `request_approval` 같은 대기형 step은 명시적 상태 전이 또는 오류로 처리되어야 한다.

## Key Entities

- **Planned Workflow Executor**: `PlanStep`를 해석하고 다음 step으로 전이시키는 실행 계층
- **Execution Cursor**: 현재 step index, 최근 step 결과, 다음 재개 지점을 보관하는 상태
- **Step Result**: 각 step의 산출물 요약 또는 실패 이유
- **Wait State**: 입력/승인/시간 대기를 runtime state에 반영한 상태

## Success Criteria

- `planned_workflow`가 더 이상 goal 재실행에만 의존하지 않는다.
- `research -> summarize -> send_result` 경로가 step 기반으로 동작한다.
- workflow가 현재 step 기준으로 재개될 수 있다.
- 미지원 step이 침묵 실패하지 않고 명시적 상태를 남긴다.

## Assumptions

- planner는 step 생성만 담당하고, executor가 실행 책임을 가진다.
- 1단계는 범용 DAG executor가 아니라 linear step sequence에 집중한다.
- 기존 workflow runtime 상태(`queued`, `running`, `waiting_input`, `retry_wait`, `completed`, `failed`)를 재사용한다.

## 현재 상태 분석

- assistant workflow planner는 `PlanStep` taxonomy와 `step_meta`를 생성한다.
- manual planned workflow는 `WorkflowRedispatcher`를 통해 `AgentLoop.execute_existing_workflow()`로 진입한다.
- planned workflow executor는 `research`, `summarize`, `send_result`를 선형 실행하고, `ask_user`, `request_approval`, `wait_until`은 runtime 상태(`waiting_input`, `retry_wait`)와 연결된다.
- workflow metadata에는 `currentStepIndex`, `currentStepKind`, `lastStepResultSummary`가 저장되며, `ask_user` / `request_approval` / `wait_until` 재개 경로가 현재 step cursor 기준으로 이어진다.
- plan 파싱 실패 또는 step 부재 시에는 기존 goal 재실행 fallback이 유지된다.

## 설계

### Executor 역할

1. workflow metadata에서 plan/step cursor를 읽는다.
2. 현재 step을 실행한다.
3. step 결과를 저장하고 다음 step으로 전이한다.
4. 마지막 step이면 사용자에게 결과를 전달하고 workflow를 완료한다.

### 최소 범위

- linear planned workflow execution
- current step / step result metadata
- `research`, `summarize`, `send_result` 지원
- 대기형 step의 명시적 pending/fail 처리

### 제외 범위

- 범용 분산 workflow engine
- multi-instance lease/locking
- 모든 step type의 완전 자동화

## 파일 변경 목록

| 파일 | 변경 | 설명 |
|------|:---:|------|
| `shacs_bot/workflow/runtime.py` | 수정 | current step / step result metadata 및 대기 상태 연계 |
| `shacs_bot/workflow/redispatcher.py` | 수정 | manual workflow를 step executor로 연결 |
| `shacs_bot/agent/loop.py` 또는 executor 모듈 | 수정/신규 | step 기반 planned workflow 실행 진입점 |
| `shacs_bot/agent/planner.py` | 수정 | step contract 정합성 보완(필요 시) |
| 관련 eval 케이스/문서 | 수정 | step executor 검증 |

## 검증 기준

- [x] `research -> summarize -> send_result` step 경로가 end-to-end로 동작한다
- [x] workflow metadata에 current step 정보가 저장된다
- [x] queued redispatch 후 현재 step 기준으로 재개된다
- [x] 대기형 step이 명시적 상태(`waiting_input`, 실패 등)로 반영된다

## Must NOT

- planner 로직과 executor 책임을 한 계층에 섞지 않는다.
- 모든 step type을 한 번에 지원하려고 과도하게 추상화하지 않는다.
- 기존 direct answer / clarification 경로를 회귀시키지 않는다.
