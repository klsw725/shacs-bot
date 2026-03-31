# PRD: Assistant Workflow Planner

## 목표

assistant 요청을 direct answer / clarification / planned workflow로 구분하고, 계획이 필요한 경우 최소한의 step structure를 생성한다.

## Deliverables

1. assistant-specific plan model
2. planner 진입 규칙
3. clarification heuristics
4. workflow runtime 연결

## 비목표

- 코딩 작업 플래너
- 대형 multi-agent orchestration planner

## Acceptance Criteria

- 단순 질문은 즉답 유지
- 계획이 필요한 요청은 구조화된 steps 생성
- 승인/대기/알림 단계가 모델에 표현됨
