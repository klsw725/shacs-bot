# PRD 000. trusted runtime profile and boundary

Status: Planned

## Goal

Prime 방식의 trusted-input 실행 모델을 하나의 runtime profile과 disclosure 계약으로 고정한다.

## Scope

1. `TrustedRuntimeProfile`의 enabled state와 source를 정의한다.
2. Workspace, instruction, skill, extension, Python kernel, native command가 trusted input임을 표시한다.
3. Daemon·worker·kernel lifecycle isolation과 security sandbox 비보장을 구분한다.
4. CLI, TUI, API가 같은 profile status와 safe summary를 표시하도록 Spec 031 input을 제공한다.

## Contract

```text
profile: trusted_local_agent
execution_authority: current_os_user
workspace_trust: user_asserted
resource_trust: explicitly_activated_or_trusted_workspace
default_containment: none
optional_sandbox: adapter_scoped
```

Profile은 권한 grant가 아니라 실행 가정의 disclosure다. Missing profile은 sandboxed 또는 safe로 추론하지 않는다.

## Acceptance Criteria

1. CLI/TUI/API에서 현재 trusted profile과 OS-user execution 경고를 확인할 수 있다.
2. Daemon·worker·kernel을 lifecycle isolation으로 표시한다.
3. Sandbox 미활성 상태를 `none` 또는 `disabled`로 표시한다.
4. Untrusted repository 사용 시 외부 sandbox 또는 별도 OS account 권고가 문서와 inspect surface에 나타난다.
5. Profile 상태가 기존 permission snapshot이나 capability ceiling 존재를 암시하지 않는다.

## Non-goals

- 중앙 permission engine
- 조직 RBAC
- kernel isolation 증명
- prompt injection 방지
