# PRD: Operator Console (CLI/TUI)

## 목표

웹 UI 없이도 로컬/운영 환경에서 assistant 상태를 점검할 수 있는 최소 operator console을 제공한다.

## Deliverables

1. admin sessions / approvals / workflows / usage 명령
2. Rich 기반 표 출력
3. inspection helper 추가

## 비목표

- 웹 admin panel
- destructive 운영 액션

## Acceptance Criteria

- session/approval/workflow/usage를 CLI에서 확인 가능
- 비어 있는 상태에서도 안전하게 실행됨
- gateway 미실행 상태에서도 파일 기반 inspection 가능
