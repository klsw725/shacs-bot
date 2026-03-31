# PRD: Lifecycle Hooks

## 목표

assistant 제품 운영에 필요한 정책/감사/후처리 로직을 core 코드에 직접 주입하지 않도록, 주요 실행 경계마다 내부 hook emit 포인트를 추가한다.

## Deliverables

1. HookRegistry / HookContext 구현
2. inbound / llm / tool / outbound / approval / heartbeat emit 연결
3. no-op fallback과 예외 격리
4. 기본 config on/off

## 비목표

- 외부 webhook SaaS 연동
- 사용자 스크립트 실행 환경
- 복잡한 hook dependency graph

## Acceptance Criteria

- Hook 미등록 상태에서 기존 동작 완전 동일
- 등록된 hook이 구조화된 context를 수신
- hook 실패는 로그에만 남고 메인 응답은 유지
- 승인/백그라운드 완료 이벤트까지 emit됨
