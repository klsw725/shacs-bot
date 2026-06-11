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

## SPEC 입력

1. 주관 spec은 `docs/specs/024-dynamic-workflows-and-harness-orchestration/SPEC.md`다.
2. skill discovery/read-only contract는 `docs/specs/005-skill-system/SPEC.md`를 소비한다.
3. permission boundary는 010/022를 소비한다.
4. diagnostics and inspect surface는 014를 소비한다.

## Dependency Cut

1. PRD 000 harness plan schema가 선행되어야 한다.
2. Recipe는 plan 생성을 돕는 metadata이며 executable workflow code가 아니다.
3. Skill은 permission을 얻거나 높일 수 없다.
4. remote marketplace 또는 signed public registry는 비범위다.

## 데이터/상태 모델

1. `WorkflowRecipeMetadata`: name, pattern, required inputs, suggested verifier, digest를 가진다.
2. `SkillBackedWorkflowRecipe`: source skill id, body digest, recipe metadata, validation status를 가진다.
3. `WorkflowRecipeConflict`: duplicate name, malformed metadata, incompatible pattern을 구분한다.
4. `SavedWorkflowInspectView`: recipe source, digest, last validation error를 가진다.

## 정상 시퀀스

1. skill registry가 workflow recipe metadata를 발견한다.
2. recipe validator가 pattern과 required fields를 확인한다.
3. valid recipe는 admission helper의 plan candidate로 사용된다.
4. inspect surface는 recipe source와 digest를 보여준다.

## 실패 시퀀스

1. malformed recipe는 blocked diagnostic으로 남고 실행되지 않는다.
2. conflict는 silent override가 아니라 conflict diagnostic이 된다.
3. recipe가 permission grant를 요구하면 거부한다.
4. recipe body는 executable code로 실행되지 않는다.

## 검증 관점

1. malformed recipe가 blocked diagnostic이 되는지 확인한다.
2. duplicate recipe conflict가 silent override되지 않는지 확인한다.
3. recipe가 permission ceiling을 높일 수 없는 regression을 둔다.

## Cargo 검증

1. `cargo fmt --manifest-path crates/shacs-core/Cargo.toml -- --check`
2. `cargo clippy --manifest-path crates/shacs-core/Cargo.toml --all-targets -- -D warnings`
3. `cargo test --manifest-path crates/shacs-core/Cargo.toml workflow_recipe`

## 완료 기준

- recipe id, source ref, prompt template ref가 비어 있으면 malformed다.
- valid recipe는 ready다.
- recipe는 suggested policy만 제공하며 runtime permission policy를 직접 변경하지 않는다.
- skill-backed recipe 설명은 self-hosted/personal-use 관점에서 유지된다.
