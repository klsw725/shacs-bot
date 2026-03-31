# PRD: Policy / Approval / Trust Model

## 목표

ApprovalGate를 workspace skill 보호 장치에서 assistant 전반의 policy engine으로 확장한다.

## Deliverables

1. user/channel/action-aware policy evaluator
2. quota-aware approval flow
3. planner/workflow에도 재사용 가능한 정책 진입점
4. config 기반 trust rules

## 비목표

- 외부 IAM 시스템 연동
- 조직/멀티테넌시 RBAC 전체 구현

## Acceptance Criteria

- tool 실행뿐 아니라 planner/workflow 단계에도 정책 적용
- trusted actor는 불필요한 승인 감소
- quota 초과 시 예측 가능한 차단/강등 동작
