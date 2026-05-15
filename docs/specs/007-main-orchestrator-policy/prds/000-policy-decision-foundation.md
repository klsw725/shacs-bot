# PRD 000. policy decision foundation

## 목표

이 문서는 `docs/specs/007-main-orchestrator-policy/SPEC.md`의 하위 실행 문서다. 목표는 formal `PolicyDecisionEngine`을 이미 완료된 구현으로 요구하는 것이 아니라, 현재 코드에 존재하는 main orchestrator policy 경계를 current architecture 기준으로 정리하고 2026-05-14 기준 완료로 닫는 것이다.

현재 완료 기준은 다음이다.

- 새 턴 수용과 동시 턴 차단은 `AgentLoop::process_message`와 `SessionTurnLock`로 설명한다.
- recovery cleanup은 `materialize_recovery_markers`, `pending_user_turn`, `runtime_checkpoint`로 설명한다.
- runtime retry와 abort는 `AgentRunner`의 현재 동작으로 설명한다.
- `ProviderRetryDecision`은 `shacs-providers`에 남아 있으며, 아직 orchestrator policy ownership 아래로 중앙화되지 않았다고 명시한다.
- subagent correlation과 stale discard는 late-result policy의 일부 구현으로 설명한다.
- `ProviderSelectionSnapshot`, `StaticProviderSelector`, `ContextBuilder`, `SpawnEnvelope` snapshot은 얕은 selection과 context/policy snapshot 증거로 설명한다.
- `ask_user` interrupt는 full approval과 permission matrix가 아니라고 명시한다.

## SPEC 입력

- 주관 spec: `docs/specs/007-main-orchestrator-policy/SPEC.md`
- 교차 의존:
  - `docs/specs/001-session-kernel/SPEC.md`
  - `docs/specs/002-command-event-effect/SPEC.md`
  - `docs/specs/003-provider-runtime/SPEC.md`
  - `docs/specs/004-tool-runtime/SPEC.md`
  - `docs/specs/006-session-store/SPEC.md`
  - `docs/specs/008-configuration-profiles-and-runtime-layout/SPEC.md`

## Dependency Cut

이 PRD는 현재 아키텍처를 문서화하는 작업이다. 이번 문서 정리는 Rust code, executor 내부 알고리즘, UI 화면을 변경하지 않는다. 완료 근거에는 2026-05-14에 추가된 focused Rust test까지 포함한다. formal policy engine 설계는 후속 owner 작업으로 남긴다.

현재 범위는 self-hosted, personal-use 단일 사용자 런타임에 맞춘다. 멀티유저 승인 체계, 원격 운영자 콘솔, 조직 운영 정책은 범위 밖이다.

## 범위

- current architecture 기준 policy boundary mapping
- 새 턴 수용 gate와 `SessionTurnLock` 설명
- recovery marker cleanup 설명
- `AgentRunner` retry와 abort 설명
- provider retry ownership의 현재 한계 설명
- subagent stale discard와 late-result 부분 구현 설명
- 얕은 selection과 snapshot 증거 설명
- future policy-engine gap 재분류

## 범위 제외

- 이번 PRD 문서 변경 안에서의 Rust code 또는 test 추가 변경
- formal `PolicyDecisionEngine` 구현
- formal approval과 permission matrix 구현
- executor-facing tool schema와 policy surface 구현
- provider retry ownership 재배치
- 멀티유저 역할 기반 정책

## 현재 구현 상태

### 완료 판정

Spec 007은 2026-05-14 기준 current architecture mapping으로 완료돼 닫혔다. 완료의 의미는 정책 관련 현재 구현 경계와 한계를 문서가 정확히 설명한다는 뜻이다. formal `PolicyDecisionEngine`이 완성됐다는 뜻이 아니다.

### 이미 반영된 것

- `AgentLoop::process_message`는 recovery marker 정리와 새 사용자 턴 처리의 현재 진입점이다.
- `SessionTurnLock`은 같은 세션의 새 턴이 동시에 진행되지 않도록 막는 current ingress gate다.
- `materialize_recovery_markers`는 `pending_user_turn`, `runtime_checkpoint` marker를 정리한다. 이전 열린 턴을 자동 성공으로 만들지 않고 interrupted assistant message 또는 placeholder message로 물질화한다.
- `AgentRunner`는 provider 실행 실패 뒤 retry 또는 abort를 결정하는 현재 runtime 경계다.
- `ProviderRetryDecision`은 `shacs-providers`에 존재한다. 이 점은 provider retry 구조의 증거이지만, 아직 orchestrator policy 소유의 중앙 decision API는 아니다.
- subagent correlation과 stale discard는 late result policy의 일부를 구현한다.
- `ProviderSelectionSnapshot`, `StaticProviderSelector`, `ContextBuilder`, `SpawnEnvelope` snapshot은 현재 selection과 context/policy snapshot 경계가 얕게나마 존재함을 보여준다.
- `ask_user` interrupt는 사용자에게 묻는 현재 상호작용 표면이다. full approval과 permission matrix는 아니다.

### 아직 남은 것

- formal `PolicyState`, `PolicySnapshot`, `ApprovalState`
- orchestrator 소유 `RetryDecision`, `LateResultDecision`
- `plan`, `default`, `auto` capability matrix
- approval request state, response correlation, timeout, late approval discard
- provider, tool, subagent별 explicit timeout policy table
- executor-facing tool schema와 policy snapshot surface
- provider retry 판단의 orchestrator policy ownership 중앙화
- durable policy state와 turn-local policy state의 명시적 타입 분리

위 항목은 current architecture 완료의 blocker가 아니다. future policy engine 작업의 범위다.

### 로컬 근거

- `crates/shacs-core/src/runtime/agent_loop.rs`
- `crates/shacs-core/src/runtime/loop_control.rs`
- `crates/shacs-core/src/runtime/runner.rs`
- `crates/shacs-core/src/runtime/subagent.rs`
- `crates/shacs-core/src/runtime/lifecycle.rs`
- `crates/shacs-core/src/runtime/context.rs`
- `crates/shacs-providers`
- 관련 runtime, recovery, subagent, provider selection 테스트

## 구현 웨이브

### Wave 1. current architecture inventory

- 새 턴 gate를 `AgentLoop::process_message`와 `SessionTurnLock`로 매핑한다.
- recovery cleanup을 `materialize_recovery_markers`, `pending_user_turn`, `runtime_checkpoint`로 매핑한다.
- retry와 abort의 현재 위치를 `AgentRunner`로 매핑한다.
- subagent stale discard와 selection snapshot 표면을 현재 증거로 정리한다.

### Wave 2. completion scope 재정의

- Spec 007 완료 기준을 formal policy engine에서 current architecture mapping으로 바꾼다.
- `ProviderRetryDecision`이 provider crate에 남아 있다는 점을 완료 범위의 한계로 명시한다.
- `ask_user` interrupt를 full approval matrix로 과장하지 않는다.
- self-hosted, personal-use framing을 유지한다.

### Wave 3. future policy engine gap 보존

- formal `PolicyState`, `PolicySnapshot`, `ApprovalState`, `RetryDecision`, `LateResultDecision`을 future gap으로 남긴다.
- `plan`, `default`, `auto` capability matrix를 future work로 남긴다.
- explicit timeout policy table과 executor-facing policy surface를 future work로 남긴다.
- provider retry ownership 중앙화를 future work로 남긴다.

### Future Wave. formal policy engine

이 wave는 현재 PRD의 완료 범위가 아니다. 후속 작업에서 필요하면 다음을 구현한다.

- durable policy state와 turn-local policy state의 명시적 타입 분리
- orchestrator-owned decision API
- approval과 permission matrix
- timeout, retry, abort, late-result 결정표
- executor-facing tool schema와 policy snapshot
- provider retry 판단의 중앙화 또는 input/decision 경계 분리

## Verification Evidence

현재 Spec 007 closure는 문서 정합성과 `runtime_loop`에 추가된 focused Rust test evidence로 검증한다. production Rust code, lockfile, 다른 spec은 변경하지 않는다.

- `SPEC.md`가 formal policy engine 구현 완료를 주장하지 않는다.
- `SPEC.md`가 current architecture evidence를 빠짐없이 반영한다.
- `SPEC.md`가 future policy engine gap을 삭제하지 않고 후속 작업으로 재분류한다.
- 이 PRD가 current state, implementation waves, verification evidence, risks, exit criteria를 current architecture 기준으로 설명한다.
- 두 문서 모두 self-hosted, personal-use framing을 유지한다.
- 두 문서 모두 운영자 조직이나 멀티유저 승인 workflow를 현재 범위로 들여오지 않는다.
- focused runtime_loop test인 `subagent_spawn_inherits_snapshot_contract`가 `SpawnEnvelope`의 얕은 context/policy snapshot 상속 계약을 고정한다.

2026-05-14 closure 증거로 다음 Rust 테스트를 둔다.

- `session_turn_lock_rejects_duplicate_active_session`
- `loop_pending_user_turn_recovery_closes_interrupted_prior_turn`
- `loop_runtime_checkpoint_materializes_placeholders_and_clears_metadata`
- `static_provider_selector_rejects_hot_swap_without_mutating_current_turn`
- `subagent_spawn_inherits_snapshot_contract`
- `runtime_runner_stops_on_ask_user_without_later_tools`
- subagent stale result 계열 테스트

코드 근거로는 다음 표면을 확인 대상으로 둔다.

- `AgentLoop::process_message`
- `SessionTurnLock`
- `materialize_recovery_markers`
- `pending_user_turn`
- `runtime_checkpoint`
- `AgentRunner`
- `ProviderRetryDecision`
- subagent correlation과 stale discard
- `ProviderSelectionSnapshot`
- `StaticProviderSelector`
- `ContextBuilder`
- `SpawnEnvelope`, 특히 `subagent_spawn_inherits_snapshot_contract`가 고정한 spawn 시점의 얕은 context/policy snapshot
- `ask_user` interrupt

## Open Risks

- 문서가 formal policy engine을 current implementation처럼 말하면 현재 코드보다 앞서간 계약이 된다.
- `ProviderRetryDecision` 위치를 숨기면 retry ownership이 이미 중앙화된 것처럼 오해될 수 있다.
- `ask_user` interrupt를 approval matrix로 부르면 permission policy가 구현된 것처럼 보일 수 있다.
- selection snapshot 증거를 과장하면 executor-facing policy surface가 이미 완성된 것처럼 보일 수 있다.
- future gap을 삭제하면 후속 formal policy engine 작업의 범위가 사라진다.

## 종료 기준

- `docs/specs/007-main-orchestrator-policy/SPEC.md`가 2026-05-14 기준 current architecture mapping으로 완료돼 닫혔음을 설명한다.
- `docs/specs/007-main-orchestrator-policy/SPEC.md`가 formal policy engine gap을 future work로 남긴다.
- 이 PRD가 현재 상태, 구현 웨이브, 검증 증거, 위험, 종료 기준을 current architecture 기준으로 갱신한다.
- 두 문서가 full formal policy engine 구현 완료를 주장하지 않는다.
- 두 문서가 self-hosted, personal-use 단일 사용자 framing을 유지한다.
- Spec 007 closure는 focused runtime_loop test `subagent_spawn_inherits_snapshot_contract`와 문서 갱신으로 구성되며, production Rust code, lockfile, 다른 spec은 변경하지 않는다.
