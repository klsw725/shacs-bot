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

- `crates/shacs-core/src/runtime/workflow.rs`
  - `WorkflowStepPrivilege`
  - `WorkflowQuarantineDecision`
  - `workflow_quarantine_decision`
  - `WorkflowPermissionCeilingDecision`
  - `workflow_permission_ceiling_decision`
- `crates/shacs-core/tests/runtime_workflow.rs`
  - `workflow_recipe_quarantine_and_permission_ceiling_preserve_safety_boundaries`

## 완료 기준

- `ReadOnlyUntrusted` child가 privileged action을 요청하면 blocked다.
- `PrivilegedActorSeparated` policy에서 privileged action은 sanitized handoff를 요구한다.
- denied capability 요청은 approval 여부와 무관하게 blocked다.
- privileged step approval policy가 true면 approval required decision을 반환한다.
- quarantine과 permission ceiling은 workflow success보다 우선한다.
