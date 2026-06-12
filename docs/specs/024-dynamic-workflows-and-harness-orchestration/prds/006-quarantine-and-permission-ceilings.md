# PRD 006: quarantine and permission ceilings

## 목표

Untrusted input을 읽은 workflow child와 privileged action 수행자를 분리하는 safety contract를 고정한다. Workflow는 permission ceiling을 낮출 수는 있어도 높일 수 없으며, denied capability와 approval-required privileged step을 우회할 수 없다.

## 범위

- workflow step privilege classification
- quarantine policy decision
- read-only untrusted child의 privileged action block
- privileged actor separated policy의 sanitized handoff requirement
- denied capability fail-closed decision
- privileged step approval required decision

## 비범위

- actual prompt-injection detector
- tool executor implementation
- approval dialog UI
- secret scanner implementation

## 구현 매핑

- `crates/shacs-workflow/src/lib.rs`
  - `WorkflowStepPrivilege`
  - `WorkflowQuarantineDecision`
  - `workflow_quarantine_decision`
  - `WorkflowPermissionCeilingDecision`
  - `workflow_permission_ceiling_decision`
- `crates/shacs-workflow/tests/workflow.rs`
  - `workflow_recipe_quarantine_and_permission_ceiling_preserve_safety_boundaries`

## SPEC 입력

1. 주관 spec은 `docs/specs/024-dynamic-workflows-and-harness-orchestration/SPEC.md`다.
2. permission ceilings and approval are consumed from 010/022.
3. Tool Search child scope is consumed from 020.
4. subagent runtime boundary is consumed from 011.

## Dependency Cut

1. PRD 000 harness plan must include child permission ceiling.
2. PRD 001 child graph must identify untrusted-reader and privileged-actor roles.
3. Child cannot receive a broader registry than its ceiling allows.
4. Quarantine is not a replacement for approval or protected target rules.

## 데이터/상태 모델

1. `ChildPermissionCeiling`: inherited capabilities, explicit denies, approval requirement를 가진다.
2. `WorkflowQuarantineRole`: untrusted reader, sanitizer, privileged actor를 구분한다.
3. `ChildToolScopeSnapshot`: registry digest, Tool Search catalog digest, denied tools를 가진다.
4. `QuarantineBoundaryEvidence`: source child, sanitized output digest, allowed handoff target을 가진다.

## 정상 시퀀스

1. workflow plan이 child별 permission ceiling을 만든다.
2. untrusted reader child는 read-only/safe registry만 받는다.
3. privileged actor는 sanitized evidence만 입력으로 받는다.
4. Tool Search catalog는 child registry scope 안에서만 만들어진다.

## 실패 시퀀스

1. child가 parent-only tool을 search/describe/call하려 하면 scope violation이다.
2. untrusted reader output이 privileged action으로 직접 연결되면 blocked된다.
3. approval 없이 ceiling을 높이는 recipe/skill은 거부된다.
4. denied tool이 catalog에 보이면 release gate 실패다.

## 검증 관점

1. child registry-only Tool Search regression을 둔다.
2. untrusted reader to privileged actor direct path가 blocked되는지 확인한다.
3. permission ceiling snapshot digest를 diagnostics에서 확인한다.

## Cargo 검증

1. `cargo fmt --manifest-path crates/shacs-workflow/Cargo.toml -- --check`
2. `cargo clippy --manifest-path crates/shacs-workflow/Cargo.toml --all-targets -- -D warnings`
3. `cargo test --manifest-path crates/shacs-workflow/Cargo.toml workflow_permission`

## 완료 기준

- `ReadOnlyUntrusted` child가 privileged action을 요청하면 blocked다.
- `PrivilegedActorSeparated` policy에서 privileged action은 sanitized handoff를 요구한다.
- denied capability 요청은 approval 여부와 무관하게 blocked다.
- privileged step approval policy가 true면 approval required decision을 반환한다.
- quarantine과 permission ceiling은 workflow success보다 우선한다.
