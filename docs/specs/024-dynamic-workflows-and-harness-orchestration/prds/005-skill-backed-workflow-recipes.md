# PRD 005: skill-backed workflow recipes

## 목표

Workflow recipe를 skill-backed reusable harness input으로 저장하고 검사할 수 있는 최소 contract를 고정한다. Recipe는 pattern, prompt scaffold, rubric, schema, budget hint, tool scope hint를 제공하지만 permission을 획득하거나 실행 코드를 주입하지 않는다.

## 범위

- workflow recipe metadata schema
- source ref와 prompt template ref required validation
- optional rubric/output schema/budget/tool scope hint
- malformed recipe readiness result
- recipe가 permission grant가 아니라 read-only input이라는 boundary

## 비범위

- skill registry persistence
- recipe conflict resolver
- executable plugin code
- remote marketplace 또는 signed public registry

## 구현 매핑

- `crates/shacs-core/src/runtime/workflow.rs`
  - `WorkflowRecipe`
  - `WorkflowRecipeReadiness`
  - `workflow_recipe_readiness`
- `crates/shacs-core/tests/runtime_workflow.rs`
  - `workflow_recipe_quarantine_and_permission_ceiling_preserve_safety_boundaries`

## 완료 기준

- recipe id, source ref, prompt template ref가 비어 있으면 malformed다.
- valid recipe는 ready다.
- recipe는 suggested policy만 제공하며 runtime permission policy를 직접 변경하지 않는다.
- skill-backed recipe 설명은 self-hosted/personal-use 관점에서 유지된다.
