# PRD: Assistant Workflow Planner

> **Spec**: [`../spec.md`](../spec.md)

---

## 문제

현재 AgentLoop는 요청을 받아 곧바로 응답/도구 실행/서브에이전트 호출로 이동하며, assistant식 후속조치 계획을 명시적으로 남기지 않는다.

문제점:

1. 단순 질의와 복합 후속조치 요청을 구분하는 규칙이 약하다.
2. 계획이 필요한 요청도 세션에 step 구조가 남지 않는다.
3. clarification이 필요한 요청과 바로 실행 가능한 요청의 경계가 불명확하다.

## 해결책

assistant 전용 planner를 추가해 요청을 direct answer / clarification / planned workflow로 분류하고, 계획이 필요한 경우 step 목록과 notify 조건을 생성한다.

## 사용자 영향

| Before | After |
|---|---|
| 복합 요청이 즉흥적으로 처리됨 | assistant가 단계 계획을 세워 이어서 처리 |
| clarification 여부가 일관되지 않음 | 중요한 경우에만 추가 질문 |
| 후속조치 계획이 세션에 남지 않음 | current plan metadata 추적 가능 |

## 기술적 범위

- **변경 파일**: 3개 수정 + 1개 신규
- **변경 유형**: planning 계층 추가
- **의존성**: 없음
- **하위 호환성**: direct answer 경로 유지

### 변경 1: planner 모델 추가 (`shacs_bot/agent/planner.py`)

- `AssistantPlan`, `PlanStep`, clarification 결과 모델
- step type: ask_user, research, summarize, wait_until, request_approval, send_result

### 변경 2: AgentLoop planning 분기 (`shacs_bot/agent/loop.py`)

- direct answer bypass
- 복합 요청 planning 경로
- workflow runtime handoff 조건 정의

### 변경 3: 세션 메타데이터 저장 (`shacs_bot/agent/session/manager.py`)

- current plan 저장
- 마지막 planning 결과 추적

### 변경 4: workflow 연결 (`shacs_bot/workflow/runtime.py`)

- planner 산출물 consume
- notify target / wait step 연계

## 성공 기준

1. 단순 질문은 planner를 우회하고 즉답한다.
2. 복합 요청은 구조화된 step 목록을 생성한다.
3. clarification은 필요한 경우에만 발생한다.
4. background 실행이 필요한 계획은 workflow runtime으로 이어진다.

---

## 마일스톤

- [ ] **M1: assistant plan 모델 정의**
  planner 데이터 모델과 step taxonomy 구현.

- [ ] **M2: direct answer vs planning 분기 추가**
  AgentLoop에서 planning 진입 규칙 구현.

- [ ] **M3: session metadata / workflow handoff 연결**
  계획 상태 저장과 runtime handoff 구현.

- [ ] **M4: direct/clarification/planned 시나리오 검증**
  세 가지 분기 모두 검증.

---

## 위험 및 완화

| 위험 | 가능성 | 영향 | 완화 |
|---|---|---|---|
| planner가 모든 요청을 과도하게 구조화 | 중간 | 중간 | direct answer bypass를 우선 규칙으로 둠 |
| clarification 질문이 과도하게 늘어남 | 중간 | 중간 | 필요한 경우만 질문하도록 heuristics 명시 |
| workflow runtime과 planner 책임 중복 | 중간 | 중간 | planner는 계획만, runtime은 실행 상태만 담당 |

## Acceptance Criteria

- [ ] direct answer 경로가 유지된다.
- [ ] 복합 요청은 step 기반 계획을 남긴다.
- [ ] clarification은 필요한 경우에만 발생한다.
- [ ] background 실행 계획이 workflow runtime으로 전달된다.
