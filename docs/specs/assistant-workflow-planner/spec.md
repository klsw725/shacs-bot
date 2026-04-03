# SPEC: Assistant Workflow Planner

> **Prompt**: shacs-bot이 장기 assistant 작업을 계획·질문·후속조치까지 이어갈 수 있도록, 코딩 에이전트용 플래너가 아닌 assistant workflow planner를 추가한다.

## PRDs

| PRD | 설명 |
|---|---|
| [`assistant-workflow-planner.md`](./prds/assistant-workflow-planner.md) | direct answer 판별, clarification, step planning, workflow handoff를 구현 태스크로 분해 |

## TL;DR

> **목적**: 사용자의 요청을 즉답, 추가 확인, 계획 수립, 후속 실행으로 구분해 assistant가 더 일관되게 일을 이어가도록 한다.
>
> **Deliverables**:
> - `shacs_bot/agent/planner.py` — plan/step/clarification 모델
> - `shacs_bot/agent/loop.py` — planning 진입과 bypass 규칙
> - `shacs_bot/workflow/runtime.py` — planner 산출물 소비 연계
> - `shacs_bot/evals/models.py`, `shacs_bot/evals/runner.py` — planner 시나리오 판정 확장
> - `shacs_bot/templates/evals/cases/planner-scenarios.json` — planner 분기 검증 케이스
> - `docs/specs/assistant-workflow-planner/checklists/requirements.md` — 스펙 품질 체크리스트
>
> **Estimated Effort**: Medium (4-6시간)

## User Scenarios & Testing

### Scenario 1 - 단순 질문은 planner를 거치지 않고 즉답한다

사용자는 간단한 질문에서 planning 오버헤드 없이 바로 답을 받아야 한다.

**테스트**: 단순 질의가 direct answer 경로로 처리되는지 확인한다.

### Scenario 2 - 복합 요청은 단계 계획으로 바뀐다

사용자는 "찾아보고 나중에 알려줘" 같은 요청에서 assistant가 필요한 단계를 스스로 정리하길 기대한다.

**테스트**: 복합 요청이 plan step 목록으로 구조화되는지 확인한다.

### Scenario 3 - 불명확한 요청은 필요한 것만 확인한다

assistant는 모든 요청에 질문을 던지지 말고, 정말 필요한 경우에만 clarification 해야 한다.

**테스트**: 모호한 요청에서 최소 수의 clarification 질문만 생성되는지 확인한다.

## Functional Requirements

- **FR-001**: 시스템은 요청을 direct answer, clarification, planned workflow 중 하나로 분류해야 한다.
- **FR-002**: planner는 복합 요청에 대해 순서 있는 step 목록을 생성해야 한다.
- **FR-003**: clarification은 계획/결과에 중대한 영향을 주는 경우에만 발생해야 한다.
- **FR-004**: background 실행이 필요한 계획은 workflow runtime으로 넘겨질 수 있어야 한다.
- **FR-005**: 현재 계획 상태는 세션 메타데이터에서 추적 가능해야 한다.

## Key Entities

- **Assistant Plan**: goal, step 목록, clarification 여부, notify target을 포함한 계획 객체
- **Plan Step**: research, summarize, wait, approval, send_result 같은 단계 단위
- **Clarification Prompt**: 추가 정보가 없으면 계획 확정이 어려운 경우 생성되는 질문

## Success Criteria

- 단순 질문은 planning 오버헤드 없이 처리된다.
- 복합 assistant 요청은 구조화된 step 목록을 남긴다.
- clarification은 필요한 경우에만 발생한다.
- background가 필요한 계획은 workflow runtime과 연결된다.

## Assumptions

- planner는 코딩 에이전트용 todo 분해가 아니라 assistant 후속조치 계획에 집중한다.
- 1단계는 하나의 현재 계획만 세션에 유지한다.
- planner는 실행기가 아니라 계획기이며, 실제 상태 전이는 workflow runtime이 맡는다.

## 현재 상태 분석

- 현재는 AgentLoop가 메시지를 받아 바로 응답/도구/서브에이전트 호출로 흘러간다.
- 복합 요청도 session metadata에 명시적 plan 구조를 남기지 않는다.
- cron/heartbeat가 있어도 "이 요청을 어떤 단계로 풀어야 하는가"를 구조화하는 planner는 없다.

## 설계

### Planner 역할

1. direct answer 가능 여부 판정
2. clarification 필요 여부 판정
3. step list 생성
4. background workflow 필요 여부 판정
5. notify target 결정

### 범위

- assistant-specific plan model
- lightweight planning heuristics
- workflow runtime handoff
- session metadata 저장

## 파일 변경 목록

| 파일 | 변경 | 설명 |
|------|:---:|------|
| `shacs_bot/agent/planner.py` | 신규 | AssistantPlan, PlanStep, planner entry |
| `shacs_bot/agent/loop.py` | 수정 | direct answer vs planning 분기 |
| `shacs_bot/workflow/runtime.py` | 수정 | planner output 소비 |
| `shacs_bot/evals/models.py` | 수정 | response pattern 기반 시나리오 판정 필드 |
| `shacs_bot/evals/runner.py` | 수정 | eval 응답 패턴 검증 경로 |
| `shacs_bot/templates/evals/cases/planner-scenarios.json` | 신규 | direct / clarification / planned_workflow 검증 케이스 |

## 검증 기준

- [x] 단순 질의는 planner를 우회하고 즉답한다
- [x] 복합 assistant 요청은 구조화된 step list를 만든다
- [x] clarification이 필요한 경우에만 추가 질문을 한다
- [x] planner 산출물이 background workflow runtime으로 연결된다

## Must NOT

- planner가 코딩 에이전트식 todo orchestration으로 변질되지 않는다.
- 단순 대화까지 과도하게 구조화하지 않는다.
