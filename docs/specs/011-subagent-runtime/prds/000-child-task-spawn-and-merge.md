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

- `crates/shacs-core/src/runtime/subagent.rs`는 child identity, lifecycle, spawn envelope, result envelope를 정의한다.
- `crates/shacs-core/tests/runtime_loop.rs`는 spawn envelope, active child ceiling, completed result, stale result, parent/child identity mismatch, cancellation cleanup을 검증한다.
- `crates/shacs-core/src/tools/spawn.rs`는 spawn tool이 runtime subagent 경계로 위임되는 경로를 제공한다.
- parent runtime loop는 synthetic inbound message로 child result를 병합하거나 stale result를 폐기한다.

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

- 단위/통합 테스트: `crates/shacs-core/tests/runtime_loop.rs`의 `subagent_result_with_wrong_child_id_is_stale`, `subagent_result_with_matching_parent_and_child_accepts_summary`, `subagent_spawn_registers_active_task_and_cancels_by_session`, `subagent_finish_publishes_synthetic_inbound_and_closes_active_task`, `subagent_stale_result_does_not_publish_or_close_active_child`, `subagent_stale_inbound_is_not_persisted_as_session_content`, `subagent_parallelism_limit_rejects_excess_children`, `spawn_tool_can_delegate_to_subagent_runtime`가 subagent runtime 경계를 검증한다.
- 도구 테스트: `crates/shacs-core/tests/tools.rs`의 spawn tool tests가 context propagation과 spawner delegation을 검증한다.
- 내구성 테스트: `crates/shacs-core/tests/runtime_loop.rs`와 `runtime_agent.rs`의 checkpoint/session persistence tests가 child result가 parent session state를 되살리지 않는 runtime boundary를 검증한다.
- 안전성 테스트: inherited safety and budget do not widen from parent
- 현 slice matrix는 별도 contracts crate가 아니라 실제 runtime tests와 문서 locator를 기준으로 유지한다.

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
