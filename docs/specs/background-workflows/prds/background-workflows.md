# PRD: Lightweight Background Workflows

## 목표

cron/heartbeat/subagent/background completion을 하나의 assistant workflow runtime으로 묶어, 재시작 후에도 일부 상태를 이어갈 수 있게 한다.

## Deliverables

1. file-backed workflow state store
2. queued/running/waiting/retry/completed 상태기계
3. cron/heartbeat/subagent 연동
4. notify target 유지

## 비목표

- 분산 작업 큐
- 대형 DAG 엔진

## Acceptance Criteria

- background 작업이 공통 상태 모델로 관리됨
- 앱 재시작 후 미완료 작업 재개 가능
- 사용자에게 completion/failure 알림 전달 가능
