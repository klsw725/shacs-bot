# PRD 001. pre-tool hooks and user confirmation

Status: Planned

## Goal

Tool 실행 직전 extension-defined veto와 ephemeral user confirmation을 표준 확장 경계로 만든다.

## Scope

1. Spec 025의 `tool:before` event, tool name, call id, validated input을 승계한다.
2. Handler가 `block`과 user-visible reason을 반환하는 계약을 정의한다.
3. Handler 순회와 첫 block short-circuit를 정의한다.
4. UI `confirm`, `select`, `notify`와 headless fallback을 정의한다.
5. Hook panic, timeout, invalid output은 해당 hook만 실패 처리하고 diagnostics를 남긴 뒤 runtime과 후속 handler를 계속하는 fail-open 정책을 사용한다.

## Required behavior

1. Hook은 실제 tool adapter 호출 전에 실행한다.
2. Block reason은 tool failure와 사용자 surface에 전달한다.
3. Headless confirmation은 자동 allow하지 않는다.
4. Confirmation 결과는 현재 호출에만 적용한다.
5. Extension handler 등록 순서는 diagnostics에서 확인할 수 있어야 한다.
6. 기존 command-backed Spec 025 hook과 trusted in-process hook은 같은 deterministic ordering key를 사용한다.

## Explicit non-guarantees

- Approval request persistence
- Action·snapshot digest correlation
- Expiry·consumed state·exact retry
- Remembered allow/deny rule
- Static deny precedence
- Parent-child permission ceiling

## Acceptance Criteria

1. 위험 명령 example hook이 allow, deny, headless deny를 실제 bash call에서 검증한다.
2. 첫 block 이후 tool과 후속 handler가 실행되지 않는다.
3. Block reason과 call id가 model context와 UI에 나타난다.
4. Panic, timeout, invalid output이 diagnostics를 남기고 runtime을 계속하는지 테스트로 고정된다.
5. 문서가 confirmation을 durable approval로 표현하지 않는다.
