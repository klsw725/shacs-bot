# PRD: Planned Workflow Executor M1

> **Spec**: [`../spec.md`](../spec.md)

---

## 문제

executor 전체 범위를 한 번에 구현하면 `wait_until`, `request_approval`, `ask_user`까지 한꺼번에 다뤄야 해서 복잡도가 급격히 커진다.

M1은 executor의 최소 가치 경로를 먼저 고정할 필요가 있다.

## 해결책

M1에서는 `research -> summarize -> send_result`만 지원하는 linear executor를 먼저 도입한다.

## 사용자 영향

| Before | After |
|---|---|
| planner workflow는 실행되더라도 goal 재실행 수준에 머묾 | 핵심 3-step 계획은 명시적으로 실행됨 |
| 현재 어느 step인지 추적 불가 | workflow metadata에서 current step을 볼 수 있음 |

## 기술적 범위

- **변경 파일**: workflow runtime / redispatcher / agent loop(or executor module) / eval assets
- **변경 유형**: 최소 executor MVP
- **의존성**: manual workflow execution path
- **하위 호환성**: 기존 manual workflow 경로 유지, 필요 시 fallback 가능

### 변경 1: current step metadata 정의

- `currentStepIndex`
- `currentStepKind`
- `lastStepResultSummary`

### 변경 2: linear executor 진입점 구현

- manual planned workflow가 executor를 통해 시작
- 현재 step을 실행하고 다음 step으로 전이

### 변경 3: 3-step 지원

- `research`: 정보 수집 또는 agent execution 호출
- `summarize`: 직전 산출물 요약
- `send_result`: 최종 사용자 전달 및 완료 처리

### 변경 4: smoke/eval 검증 추가

- 3-step happy path
- 중간 실패 시 failed 전이
- redispatch 후 current step 기준 재개

## 성공 기준

1. 핵심 3-step path가 executor로 실행된다.
2. current step metadata가 저장된다.
3. 마지막 `send_result`에서 workflow가 완료된다.

---

## 마일스톤

- [x] **M1-1: step metadata 구조 추가**
  current step / last result metadata 정의.

- [x] **M1-2: executor 진입점 연결**
  manual workflow redispatch를 executor로 연결.

- [x] **M1-3: 3-step happy path 구현**
  `research -> summarize -> send_result` 실행.

- [x] **M1-4: smoke 검증**
  happy path / failure path / resume path 확인.

---

## 위험 및 완화

| 위험 | 가능성 | 영향 | 완화 |
|---|---|---|---|
| research/summarize 구현이 다시 goal 재실행으로 흐름 | 중간 | 중간 | 각 step 입력/출력을 metadata에 명시 |
| 초기 executor가 후속 step 확장성을 해침 | 낮음 | 중간 | step contract를 단순하고 선형적으로 유지 |

## Acceptance Criteria

- [x] `research -> summarize -> send_result` 3-step path가 실행된다.
- [x] current step metadata가 기록된다.
- [x] 실패 시 failed, 성공 시 completed로 끝난다.

## 진행 로그

| 날짜 | 상태 | 메모 |
|---|---|---|
| 2026-04-03 | 초안 | executor 전체 범위 중 M1 최소 구현 범위를 별도 PRD로 분리 |
| 2026-04-03 | 구현 | `WorkflowRuntime`에 `update_step_cursor()`, `annotate_step_result()`, `clear_step_cursor()`를 추가하고 `AgentLoop.execute_existing_workflow()`가 stored plan을 읽어 executor 경로로 진입하도록 연결. |
| 2026-04-03 | 구현 | `research`, `summarize`, `send_result` 3-step happy path를 구현하고, plan parse 실패 또는 step 부재 시 기존 goal 재실행 fallback을 유지. |
| 2026-04-03 | 검증 | initial smoke 경로에서 happy path, waiting step 전이, retry_wait 전이를 점검. 이후 executor 범위가 확장되었지만 M1 최소 경로는 그대로 shipped 상태로 유지. |
| 2026-04-04 | 동기화 | pytest 전환 후 `tests/test_step_cursor.py`와 `tests/test_e2e_planner_to_workflow.py`를 기준으로 M1-1~M1-4 완료 상태를 문서에 반영. |
