# PRD 000. child task spawn and merge

## 목표

이 문서는 `docs/specs/011-subagent-runtime/SPEC.md`의 하위 실행 문서다. SPEC이 정의한 child task identity, spawn envelope, inherited context, synthetic command reentry, merge authority를 실제 런타임 구현 단계로 정리한다.

이번 PRD의 목표는 subagent를 독립 세션 소유자가 아니라 부모 턴이 발행한 제한된 child executor로 구현하는 것이다. 결과는 언제나 후보 결과로만 돌아오고, 공식 상태 변경은 오직 부모 오케스트레이터의 merge 판단을 통해서만 일어나야 한다.

## SPEC 입력

- 주관 spec: `docs/specs/011-subagent-runtime/SPEC.md`
- 선행 기준:
  - `docs/specs/001-session-kernel/SPEC.md`
  - `docs/specs/003-provider-runtime/SPEC.md`
  - `docs/specs/004-tool-runtime/SPEC.md`
  - `docs/specs/007-main-orchestrator-policy/SPEC.md`
  - `docs/specs/009-context-assembly-and-compaction-input/SPEC.md`
  - `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`
- 교차 의존:
  - `docs/specs/012-runtime-services/SPEC.md`
  - `docs/specs/013-user-interfaces-and-session-ux/SPEC.md`
  - `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`
  - `docs/specs/016-verification-matrix-and-release-gates/SPEC.md`

## Dependency Cut

- 007에서 spawn 결정, merge 채택, retry, stale classification 정책을 받는다.
- 009는 child에 내려갈 inherited context snapshot을 만들고, 부모 전체 transcript 무제한 복제를 금지한다.
- 010은 child가 상속받는 safety snapshot이 부모보다 넓어지지 않게 만든다.
- 012는 child 결과가 synthetic command로만 재진입하도록 service 경계를 제공한다.
- 013과 014는 child lifecycle과 merge 상태를 사용자 projection과 trace로 드러내야 한다.

## 범위

- child task identity와 lifecycle 모델 구현
- spawn envelope와 inherited snapshot 타입 구현
- child concurrency ceiling과 timeout, cancel 규칙 구현
- child result payload와 synthetic command reentry 경로 구현
- merge authority와 stale result 분류기 구현
- parent turn closed, retry, late result 회귀 테스트 추가

## 범위 제외

- agent persona 프롬프트 설계
- 원격 agent marketplace
- 멀티유저 task ownership
- 분산 합의형 agent orchestration

## TDD 계획

1. child lifecycle 전이와 terminal state 규칙을 단위 테스트로 고정한다.
2. spawn envelope에 필수 필드가 빠지면 거절되는 validation 테스트를 추가한다.
3. spawn, running, awaiting_merge, completed 흐름 통합 테스트를 만든다.
4. cancel, timeout, parent turn closed before child return, duplicate child result 테스트를 추가한다.
5. merge accept, summarize-only, discard stale 분기 테스트를 추가한다.

## 현재 구현 상태

- `crates/shacs-core/src/core/subagent.rs`는 child identity, lifecycle, spawn envelope, result envelope, `SubagentReentry`를 정의한다.
- `crates/shacs-core/tests/subagent_runtime.rs`는 spawn envelope/budget, active child ceiling, merge decision table, completed/failed/timed_out/cancelled result, duplicate, inactive-effect stale result, parent-close stale rejection, child identity mismatch rejection을 검증한다.
- `crates/shacs-runtime-adapters/src/subagent.rs`는 `SubagentAdapter`와 `SubagentRuntime`을 제공하며, `SpawnSubagentEffect` 식별자를 보존해 terminal `SubagentReentry`로 정규화한다.
- `crates/shacs-surface/src/session_queries.rs`의 runtime effect executor는 `Effect::SpawnSubagent`를 `ReentrySource::Subagent` command로 변환할 수 있다.
- merge 판단은 `classify_subagent_merge`가 terminal reentry shape을 summary-only accept, terminal fact, parent abort decision으로 분류하고, `MainOrchestrator`가 synthetic reentry 경로에서 이를 상태 전이와 merge/abort 처리로 적용한다.

## 구현 웨이브

### Wave 1. Child task 모델과 spawn 계약 구현

- `child_task_id`, `spawn_effect_id`, `spawn_sequence`를 포함한 identity 타입을 만든다.
- lifecycle state와 전이 표를 코드로 고정한다.
- spawn envelope validation과 child concurrency ceiling 검사를 구현한다.

### Wave 2. Inherited snapshot과 runtime 실행 연결

- inherited context, policy, safety, input/output budget snapshot을 분리한다.
- subagent runtime adapter가 envelope 밖 목표를 받지 못하게 만든다.
- child timeout, cancel, parallelism group 제약을 연결한다.

### Wave 3. Result payload와 synthetic reentry 구현

- child result payload에 status, summary, evidence, error, correlation 필드를 고정한다.
- child 종료 결과를 synthetic command로 정규화해 부모 오케스트레이터에 재진입시킨다.
- duplicate delivery와 stale result를 분리 판정한다.

### Wave 4. Merge authority와 회귀 검증 완성

- 부모 오케스트레이터가 merge accept, summarize-only, discard, retry를 결정하는 API를 구현한다.
- 부모 턴 종료 후 도착한 결과를 stale로만 기록하고 상태를 되살리지 않게 한다.
- child retry와 newer child precedence 규칙을 회귀 테스트로 고정한다.

## Verification Evidence

- 단위 테스트: `crates/shacs-core/tests/subagent_runtime.rs`의 `subagent_merge_decision_table_matches_terminal_reentry_shape`가 merge decision table을 고정한다.
- 단위 테스트: `spawn_summary_subagent_emits_effect_and_tracks_child`가 spawn envelope identity, inherited policy/safety, inherited budget, timeout, pending effect를 검증한다.
- 단위 테스트: `subagent_reentry_with_child_identity_mismatches_is_rejected`와 `old_child_result_with_new_fingerprint_is_rejected_after_effect_retired`가 identity mismatch와 inactive-effect stale classification을 검증한다.
- 통합 테스트: `crates/shacs-core/tests/subagent_runtime.rs`가 spawn, active child ceiling, completed summary merge, failed/timed_out/cancelled child result의 `spawned -> awaiting_merge -> terminal` lifecycle 기록, parent abort 전 failure fact 보존, duplicate child result rejection, parent closed stale rejection을 검증한다.
- adapter 테스트: `crates/shacs-runtime-adapters/tests/subagent_runtime.rs`가 `summary_subagent_runtime_normalizes_spawn_effect_to_subagent_completed_reentry`, `subagent_runtime_preserves_terminal_timeout_and_cancelled_identity`, `subagent_runtime_coerces_non_terminal_adapter_status_to_failed_reentry`로 envelope identity 보존과 terminal reentry 정규화를 검증한다.
- surface 테스트: `runtime_effect_executor_converts_spawn_subagent_effect_to_subagent_reentry_command`가 `Effect::SpawnSubagent`가 무시되지 않고 synthetic subagent reentry command로 변환되는 경계를 검증한다.
- 내구성 테스트: `crates/shacs-core/tests/session_store_replay.rs`의 `resumed_open_child_result_is_rejected_as_recovery_residual`, `replayed_terminal_close_discards_temporary_turn_artifacts`, `replayed_aborted_close_discards_temporary_turn_artifacts`가 restart/replay 뒤 child result가 부모 턴을 되살리거나 temporary artifact를 유지하지 않음을 검증한다.
- 안전성 테스트: inherited safety and budget do not widen from parent
- Spec016 matrix 증거: `crates/shacs-contracts/src/verification.rs`가 Spec011 `Unit`, `Integration`, `DurabilityRecovery`를 `CoverageLevel::FullSpec` / `CoverageStatus::Verified`로 선언하고, `crates/shacs-core/tests/verification_matrix.rs`의 `spec011_full_spec_evidence_covers_required_families`가 이를 검증한다.

## Open Risks

- child result 요약 수준이 낮으면 merge 판단 근거가 부족할 수 있다.
- synthetic command correlation이 약하면 duplicate와 stale 구분이 흔들릴 수 있다.
- child concurrency ceiling이 없거나 느슨하면 부모 턴 budget이 무너질 수 있다.

## 종료 기준

- 모든 child task가 고유 identity와 lifecycle을 가진다.
- child는 envelope 밖 권한, 문맥, budget을 얻지 못한다.
- child result는 synthetic command로만 재진입하고 merge 전에는 공식 상태가 아니다.
- stale result가 부모 턴을 되살리거나 truth를 덮어쓰지 않는다.
- 011과 016이 요구하는 단위, 통합, 내구성 검증이 모두 통과 가능한 상태가 된다.
