# main orchestrator policy 아키텍처 명세

Status: Complete (Scoped)
Implemented scope: `AgentLoop`, `SessionTurnLock`, recovery marker cleanup, `AgentRunner` retry/abort, subagent stale discard, provider selection snapshot, context and spawn snapshot의 current policy boundary를 닫았다.
Open work moved to: [028 formal execution reentry and outcome contracts](../028-formal-execution-reentry-and-outcome-contracts/SPEC.md), [030 trusted agent runtime and operational controls](../030-trusted-agent-runtime-and-operational-controls/SPEC.md)
Not carried forward: 멀티유저 역할 정책, 원격 운영자 console, 외부 policy server, 조직 승인 체계, centralized `PolicyDecisionEngine`, 통계 기반 자동 최적화를 후속 owner 범위로 가져가지 않는다. 030은 trusted runtime profile과 hook ordering만 소유한다.

## 문서 목적

이 문서는 `shacs-bot`의 현재 main orchestrator policy 경계를 코드 구조에 맞춰 정리한다. 이전 초안은 formal `PolicyDecisionEngine`이 이미 완료돼야 하는 구현 계약처럼 읽혔다. 이 문서는 그 관점을 고친다.

현재 완료 판정은 formal policy engine 완료가 아니다. 완료의 의미는 지금 코드에 존재하는 오케스트레이터 중심 정책 경계, recovery marker 정리, runtime retry와 abort 판단, correlation 기반 stale result 처리, 얕은 selection과 snapshot 표면을 Spec 007의 current architecture로 문서화했다는 뜻이다.

목표는 다음과 같다.

- 현재 구현된 정책 판단이 어디에 있고 어디에 있지 않은지 고정한다.
- current architecture 기준 완료 범위와 future policy engine 범위를 분리한다.
- self-hosted, personal-use 단일 사용자 런타임이라는 전제를 유지한다.
- formal `PolicyState`, `PolicySnapshot`, approval matrix, timeout table, executor-facing policy surface는 후속 작업으로 명시한다.

이 문서는 현재 코드를 formal policy layer로 과장하지 않는다. Spec 007은 2026-05-14 기준 현재 아키텍처 매핑으로 닫혔고, 그것이 완전한 정책 엔진 구현을 뜻하진 않는다.

---

## 상위 기준과의 관계

이 문서는 다음 상위 기준을 전제로 한다.

- `MainOrchestrator` 또는 그 현재 구현 표면인 runtime loop가 세션 상태 변경의 중심 경계다.
- provider runtime, tool runtime, session store는 실행 경계 또는 저장 경계이며, 세션의 공식 결과를 임의로 확정하면 안 된다.
- 세션의 단일 턴 정확성과 crash 이후 재진입 가능성이 확장성보다 우선이다.
- 목표는 사용자가 직접 설치하고 운영하는 self-hosted, personal-use 단일 사용자 런타임이다.

따라서 이 문서는 멀티유저 승인 체계, 원격 운영자 콘솔, 조직별 정책 배포 체계, 외부 정책 서버 연동을 현재 범위로 보지 않는다.

---

## 현재 완료 판정

2026-05-14 기준 Spec 007은 current architecture mapping 기준으로 완료로 닫혔다. 완료의 의미는 formal `PolicyDecisionEngine`, `PolicyState`, `PolicySnapshot`, `ApprovalState`, `RetryDecision`, `LateResultDecision` 타입이 모두 구현됐다는 뜻이 아니다.

완료의 의미는 다음이다.

- 새 사용자 턴 진입은 `AgentLoop::process_message`와 `SessionTurnLock` 경계에서 직렬화된다.
- crash 또는 중단 뒤에는 `materialize_recovery_markers`가 `pending_user_turn`, `runtime_checkpoint` marker를 사용자에게 보이는 interrupted assistant message 또는 placeholder message로 정리한다.
- provider 실행의 현재 retry와 abort 판단은 `AgentRunner` 경계에서 처리된다.
- provider retry 판단 타입인 `ProviderRetryDecision`은 `shacs-providers`에 존재하지만, 아직 orchestrator policy 소유로 중앙화되지 않았다.
- subagent 결과는 correlation과 stale discard로 일부 late-result 정책을 적용한다.
- `ProviderSelectionSnapshot`, `StaticProviderSelector`, `ContextBuilder`, `SpawnEnvelope` snapshot은 얕은 selection과 snapshot 증거다.
- `ask_user` interrupt는 사용자의 입력을 기다리는 현재 메커니즘이지만, Spec 007의 완전한 approval과 permission matrix 구현과 같지 않다.

이 범위가 현재 Spec 007 완료 기준이다. formal policy engine은 후속 작업이다.

---

## 현재 범위

이 문서는 다음을 설명한다.

- 현재 새 턴 수용 gate와 session turn lock 경계
- recovery marker를 세션 visible state로 정리하는 현재 정책
- provider runtime retry와 abort의 현재 위치
- provider retry ownership이 아직 완전히 orchestrator로 모이지 않은 상태
- subagent stale discard가 담당하는 late-result 정책 일부
- provider selection, context building, spawn envelope snapshot의 현재 얕은 형태
- 현재 완료로 인정하는 범위와 future policy engine gap

이 문서는 다음을 현재 구현 완료로 주장하지 않는다.

- formal `PolicyDecisionEngine`
- formal `PolicyState`, `PolicySnapshot`, `ApprovalState`
- orchestrator 소유 `RetryDecision`, `LateResultDecision`
- `plan`, `default`, `auto` capability matrix
- 명시적 timeout policy table
- executor-facing tool schema와 policy surface
- 전체 approval과 permission matrix

---

## 현재 구현 매핑

### 새 턴 진입 정책

현재 새 사용자 턴은 `AgentLoop::process_message`를 통해 들어온다. 이 경계는 세션을 읽고, recovery marker를 먼저 정리하고, 새 턴 처리로 넘어간다. `SessionTurnLock`은 같은 세션의 새 턴이 동시에 진행되지 않도록 막는 현재 ingress gate다.

따라서 Spec 007 current architecture에서 새 턴 수용 정책은 formal decision enum이 아니라 `AgentLoop::process_message`와 `SessionTurnLock` 조합으로 표현된다. 이 조합은 single-user 로컬 실행에서 한 세션의 열린 턴을 하나로 유지하기 위한 현재 완료 범위다.

### recovery 정리 정책

현재 recovery 정책은 `materialize_recovery_markers`가 맡는다. 세션 metadata에 `pending_user_turn`이 남아 있으면 이전 열린 턴을 성공으로 간주하지 않고 interrupted assistant message로 물질화한 뒤 marker를 지운다. `runtime_checkpoint`가 남아 있으면 checkpoint 안의 assistant message와 pending tool call을 placeholder message로 물질화하고 marker를 지운다.

이 동작은 formal event replay engine이 아니다. 다만 current architecture에서는 crash 이후 열린 턴을 자동 성공으로 만들지 않고, 사용자가 inspect할 수 있는 세션 기록으로 정리하는 정책 증거다.

### runtime retry와 abort 정책

현재 provider 실행의 retry와 abort는 `AgentRunner` 경계에 있다. 실패가 retry 가능한지, retry ceiling에 닿았는지, abort로 닫아야 하는지는 현재 runtime loop의 실행 경로에서 판단된다.

이것은 formal orchestrator-owned `RetryDecision` 타입이 있다는 뜻이 아니다. current architecture의 완료 의미는 retry와 abort가 무한 재시도나 executor의 임의 성공 처리로 흩어지지 않고, runtime의 한 경계에서 일관되게 처리된다는 점이다.

### provider retry ownership

`ProviderRetryDecision`은 `shacs-providers`에 존재한다. 그래서 provider retry 판단의 일부 구조는 이미 타입으로 드러나 있다. 하지만 이 타입은 아직 `MainOrchestrator`가 소유하는 중앙 policy API 아래로 이동하지 않았다.

current architecture에서는 이 상태를 완료 범위 안의 현실로 인정한다. future policy engine에서는 provider retry 판단을 orchestrator policy ownership 아래로 모을지, provider crate가 policy input만 제공하도록 할지 다시 정해야 한다.

### late result와 stale discard

subagent 경로는 correlation과 stale discard를 통해 늦게 도착한 일부 결과를 버린다. 이것은 Spec 007 late-result 정책의 일부를 이미 충족한다.

다만 현재 구현은 모든 effect 종류에 대해 독립 `LateResultDecision` API를 제공하지 않는다. provider, tool, subagent 전부를 같은 결정표로 다루는 formal late-result layer는 future work다.

### selection과 snapshot

현재 selection과 snapshot 증거는 얕지만 존재한다.

- `ProviderSelectionSnapshot`은 provider 선택 결과를 snapshot으로 남기는 현재 표면이다.
- `StaticProviderSelector`는 현재 provider 선택이 별도 selection 경계로 분리돼 있음을 보여준다.
- `ContextBuilder`는 provider 호출 전에 대화 맥락과 선택 입력을 구성한다.
- `SpawnEnvelope` snapshots는 subagent spawn 시점의 얕은 context와 policy snapshot 입력을 고정한다.

이것은 완전한 policy snapshot 모델이 아니다. 현재 완료 범위는 provider, context, spawn 입력이 실행 시점에 아무렇게나 재계산되지 않도록 얕은 snapshot 표면을 갖췄다는 점이다.

### ask_user interrupt와 approval의 차이

`ask_user` interrupt는 실행 중 사용자 입력을 기다리는 현재 메커니즘이다. 이것은 사용자가 직접 운영하는 personal-use 런타임에 맞는 현재 상호작용 표면이다.

하지만 `ask_user` interrupt는 Spec 007이 미래 work로 남기는 전체 approval과 permission matrix가 아니다. `plan`, `default`, `auto` 모드별 capability 결정표, approval request state, deadline, late approval discard, executor-facing tool policy schema는 아직 formal layer로 구현됐다고 말할 수 없다.

---

## current architecture 정책 원칙

현재 구현을 기준으로 유지해야 하는 원칙은 다음과 같다.

1. 같은 세션에 새 사용자 턴이 동시에 열리면 안 된다.
2. recovery marker는 새 턴 처리 전에 정리돼야 한다.
3. recovery cleanup은 이전 열린 턴을 자동 성공으로 만들면 안 된다.
4. provider retry와 abort는 runtime 경계에서 제어돼야 하며 무한 재시도하면 안 된다.
5. provider retry 정책이 아직 provider crate에 남아 있다는 사실을 숨기면 안 된다.
6. stale subagent 결과는 현재 활성 correlation과 맞지 않으면 공식 결과로 반영되면 안 된다.
7. selection과 spawn 입력은 현재 snapshot 표면이 제공하는 범위에서 고정돼야 한다.
8. `ask_user` interrupt를 full approval matrix로 부르면 안 된다.

---

## future policy engine gap

다음 항목은 useful future-gap 정보이며 삭제하지 않는다. 다만 current architecture 완료의 blocker로 보지 않는다.

### formal state와 decision 타입

후속 formal policy engine은 최소한 아래 경계를 타입 수준에서 드러내야 한다.

- `PolicyState`
- `PolicySnapshot`
- `ApprovalState`
- `RetryDecision`
- `LateResultDecision`

현재 코드는 이 경계를 모두 독립 타입으로 제공하지 않는다. future work에서는 durable policy state와 turn-local policy state를 분리하고, recovery 이후 어떤 판단을 재구성할지 명시해야 한다.

### capability와 approval matrix

`plan`, `default`, `auto` 모드별 capability matrix는 아직 formal current behavior가 아니다. future work에서는 다음을 결정표로 고정해야 한다.

- 어떤 capability가 자동 허용되는가
- 어떤 capability가 사용자 확인을 요구하는가
- 어떤 capability가 즉시 거절되는가
- approval request와 response correlation을 어떻게 검증하는가
- approval timeout과 late approval을 어떻게 처리하는가

이 matrix는 self-hosted personal-use 사용자가 직접 제어하는 로컬 정책이어야 한다. 조직 운영이나 관리자 승인 체계로 확장하지 않는다.

### timeout policy table

현재 runtime에는 timeout과 retry, abort 처리 흐름이 있지만 Spec 007 수준의 명시적 timeout policy table은 없다. future work에서는 provider, tool, subagent별 timeout budget, retry 가능 여부, abort 사유, late result 처리를 표로 고정해야 한다.

### executor-facing policy surface

현재 snapshot 표면은 얕다. future work에서는 executor가 볼 수 있는 tool schema와 policy snapshot을 명시해야 한다. 단 executor가 policy owner가 되면 안 된다. executor는 snapshot을 집행하고 결과를 보고할 뿐, 승인이나 retry 결정을 최종 확정하지 않는다.

### centralized ownership

`ProviderRetryDecision`이 provider crate에 있는 현재 상태는 documented reality다. future work에서는 이 판단을 orchestrator policy API 아래로 중앙화할지, provider crate가 retryability input만 제공하고 orchestrator가 최종 결정을 내리도록 바꿀지 정해야 한다.

---

## 검증 관점

current architecture 완료는 다음 증거로 판단한다.

- `AgentLoop::process_message`가 recovery marker 정리와 새 턴 처리의 진입점인지 확인한다.
- `SessionTurnLock`이 같은 세션의 동시 새 턴을 막는지 확인한다.
- `materialize_recovery_markers`가 `pending_user_turn`, `runtime_checkpoint`를 성공이 아닌 visible cleanup으로 바꾸는지 확인한다.
- `AgentRunner`가 provider 실패 후 retry 또는 abort를 제어하는지 확인한다.
- `ProviderRetryDecision`이 provider crate에 남아 있음을 확인하고, 이를 formal orchestrator policy 완료로 주장하지 않는다.
- subagent correlation과 stale discard가 늦은 결과 일부를 버리는지 확인한다.
- `ProviderSelectionSnapshot`, `StaticProviderSelector`, `ContextBuilder`, `SpawnEnvelope` snapshot이 얕은 selection과 snapshot 증거인지 확인한다.
- `ask_user` interrupt를 full approval matrix로 부르지 않는다.

2026-05-14 closure 증거로 다음 Rust 테스트를 함께 둔다.

- `session_turn_lock_rejects_duplicate_active_session`
- `loop_pending_user_turn_recovery_closes_interrupted_prior_turn`
- `loop_runtime_checkpoint_materializes_placeholders_and_clears_metadata`
- `static_provider_selector_rejects_hot_swap_without_mutating_current_turn`
- `subagent_spawn_inherits_snapshot_contract`
- `runtime_runner_stops_on_ask_user_without_later_tools`
- subagent stale result 계열 테스트

future policy engine 검증은 별도다. 그때는 formal decision 타입, approval matrix, timeout table, executor-facing policy schema, recovery 이후 durable과 turn-local state 경계를 테스트해야 한다.

---

## 명시적 비범위

이 문서는 다음을 현재 범위로 보지 않는다.

- 멀티유저 역할 기반 정책
- 원격 운영자 콘솔
- 외부 정책 서버 연동
- 조직별 승인 체계
- 통계 기반 자동 최적화
- provider 벤더별 상세 튜닝 UI

필요가 생기면 별도 문서에서 다룬다. 단 어떤 확장도 self-hosted personal-use 기본 전제를 뒤집으면 안 된다.

---

## 결론

Spec 007은 2026-05-14 기준 current architecture mapping으로 완료돼 닫혔다. 이 완료 기준은 formal policy engine을 이미 갖췄다는 선언이 아니다. 현재 구현은 `AgentLoop`, `SessionTurnLock`, recovery marker cleanup, `AgentRunner`, provider retry 판단, subagent stale discard, provider selection snapshot, context building, spawn envelope snapshot으로 main orchestrator policy의 현재 아키텍처를 구성한다.

후속 작업은 이 현실 위에서 formal policy engine을 세우는 것이다. 그때 다룰 항목은 `PolicyState`, `PolicySnapshot`, `ApprovalState`, `RetryDecision`, `LateResultDecision`, capability matrix, timeout table, executor-facing policy surface다. 현재 문서는 그 차이를 분명히 남긴다.
