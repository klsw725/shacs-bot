# PRD: Planned Workflow Executor

> **Spec**: [`../spec.md`](../spec.md)

---

## 문제

현재 assistant workflow planner는 `planned_workflow`와 `PlanStep` 목록을 생성할 수 있지만, 실제 실행은 여전히 goal 재실행 중심이다.

문제점:

1. planner가 만든 step taxonomy가 runtime에서 구조적으로 실행되지 않는다.
2. manual workflow redispatch가 생겨도 현재는 `goal` 텍스트를 다시 agent loop에 태우는 수준이다.
3. `ask_user`, `wait_until`, `request_approval` 같은 step은 실행 계약이 불명확하다.
4. recovery / resume도 현재 step보다 goal 중심으로 해석돼 계획 실행기의 의미가 약하다.

## 해결책

planned workflow 전용 executor를 추가해 `AssistantPlan.steps`를 순서대로 소비하고, 각 step 결과와 현재 cursor를 workflow metadata에 반영한다.

## 사용자 영향

| Before | After |
|---|---|
| planner가 계획만 만들고 실제 실행은 goal 재실행에 의존 | planner step을 실제 순서대로 실행 |
| `research -> summarize -> send_result`가 문서상 계획에 머묾 | 각 step이 executor에서 명시적으로 소비됨 |
| 대기형 step이 애매하게 처리됨 | 입력/승인/대기 step이 명시적 상태로 연결됨 |

## 기술적 범위

- **변경 파일**: workflow runtime / redispatcher / agent loop(or executor module) / planner / eval assets
- **변경 유형**: workflow execution 계층 추가
- **의존성**: assistant workflow planner, background workflow runtime
- **하위 호환성**: direct answer / clarification 경로 유지

### 변경 1: executor 뼈대 추가

- planned workflow executor 인터페이스 정의
- current step index / last step result metadata 구조 정의
- unsupported step 처리 규칙 정의

### 변경 2: 최소 3-step 실행 경로 구현

- `research`
- `summarize`
- `send_result`

### 변경 3: runtime metadata 확장

- current step 저장
- step result 요약 저장
- resume/recover 시 현재 step 기준 재개

### 변경 4: redispatch 연계 강화

- manual workflow redispatch가 executor 진입점으로 연결
- goal 재실행 대신 step execution path로 이동

### 변경 5: 대기형 step 상태 설계

- `ask_user` → `waiting_input`
- `wait_until` → `retry_wait` 또는 별도 대기 메타데이터
- `request_approval` → approval 대기 상태 또는 명시적 실패

### 변경 6: 검증 시나리오 추가

- step executor end-to-end 시나리오
- step 실패/재개 시나리오

## 성공 기준

1. `planned_workflow` 실행이 step executor를 통해 시작된다.
2. 최소한 `research -> summarize -> send_result` 예제가 end-to-end로 동작한다.
3. workflow는 현재 step 기준으로 재개될 수 있다.
4. 미지원 step은 조용히 무시되지 않는다.

---

## 마일스톤

- [ ] **M1: planned workflow executor 뼈대 추가**
  executor 진입점, current step metadata, 최소 3-step 계약 정의.

- [ ] **M2: 핵심 3-step 실행 경로 구현**
  `research`, `summarize`, `send_result` 실행.

- [ ] **M3: 대기형 step 상태 연계**
  `ask_user`, `wait_until`, `request_approval` 상태기 연동.

- [ ] **M4: recovery / redispatch / eval 검증**
  step resume, queued redispatch, smoke/eval 시나리오 검증.

---

## 위험 및 완화

| 위험 | 가능성 | 영향 | 완화 |
|---|---|---|---|
| executor가 planner 책임까지 침범 | 중간 | 중간 | planner는 계획 생성만, executor는 실행만 담당 |
| step abstraction이 과도하게 무거워짐 | 중간 | 높음 | M1/M2에서 linear 3-step 경로만 먼저 구현 |
| wait/approval 흐름이 runtime과 충돌 | 중간 | 중간 | 기존 workflow state를 재사용 |

## Acceptance Criteria

- [ ] `planned_workflow`가 step executor를 통해 실행된다.
- [ ] `research -> summarize -> send_result` 경로가 동작한다.
- [ ] workflow metadata에 current step 정보가 저장된다.
- [ ] 실패/대기/재개가 step 기준으로 동작한다.

## 진행 로그

| 날짜 | 상태 | 메모 |
|---|---|---|
| 2026-04-03 | 초안 | planned workflow를 goal 재실행이 아닌 step executor 기반으로 전환하는 후속 PRD 추가 |
