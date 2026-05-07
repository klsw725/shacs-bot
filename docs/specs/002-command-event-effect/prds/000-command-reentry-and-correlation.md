# PRD 000. command reentry and correlation

## 목표

이 문서는 `docs/specs/002-command-event-effect/SPEC.md`의 하위 실행 문서다. 목표는 외부 실행 결과가 command로 재진입하는 경계를 구현 가능한 수준으로 고정하고, effect와 turn 사이의 correlation 규칙을 코드와 테스트로 닫는 것이다.

- 모든 외부 결과가 직접 상태 patch가 아니라 재진입 command로만 들어오게 만든다.
- `session_id`, `turn_id`, `effect_id`, `correlation_id` 기반의 상관관계 규칙을 공통 타입으로 고정한다.
- 중복 재진입, 닫힌 턴 재진입, 오래된 effect 결과를 안전하게 걸러낸다.

## SPEC 입력

- 주관 spec: `docs/specs/002-command-event-effect/SPEC.md`
- 교차 의존:
  - `docs/specs/001-session-kernel/SPEC.md`
  - `docs/specs/003-provider-runtime/SPEC.md`
  - `docs/specs/004-tool-runtime/SPEC.md`
  - `docs/specs/007-main-orchestrator-policy/SPEC.md`

## Dependency Cut

이 PRD는 메시지 의미론과 재진입 경계만 구현 대상으로 삼는다. provider 결과 본문, tool 결과 본문, session store 영속화는 각 하위 spec가 책임진다. 여기서는 어떤 결과든 오케스트레이터로 다시 들어오는 표준 command 계약과 correlation 검증만 만든다.

## 범위

- `Command`, `Event`, `Effect`의 Rust 타입 경계 정립
- 재진입 command 공통 envelope 정의
- correlation id, causation id, effect id, turn id 연결 규칙
- 중복 재진입 감지 규칙
- 이미 닫힌 턴 또는 만료된 effect에 대한 거부 규칙
- synthetic command와 외부 재진입 command의 공통 처리 표면

## 범위 제외

- 개별 provider/tool 결과 payload 상세 스키마
- UI projection 형식
- event log 저장 포맷
- queue, mailbox, scheduler의 실제 I/O 구현

## 현재 구현 상태

### 이미 반영된 것

- runtime message, provider/tool progress, session command 경계가 `crates/shacs-core/src/runtime/agent_loop.rs`, `runner.rs`, `loop_control.rs`에 구현돼 있다.
- session/turn/effect 성격의 재진입과 duplicate active turn 거절은 runtime loop와 bus 경계에서 검증된다.
- synthetic/external 재진입이 같은 검증 경로를 타는 테스트가 있다.
- provider/tool/subagent/service 결과는 직접 state patch가 아니라 typed reentry command 또는 service command envelope로만 오케스트레이터에 되돌아온다.
- provider/tool/subagent effect는 `causation_id`, `correlation_id`, `effect_id`를 보존하고, effect queue/retire event와 연결된다.
- reducer는 session/turn/order mismatch event를 거절하고 실패 시 state를 변경하지 않는다.

### 아직 남은 것

- 개별 provider/tool/subagent 결과 payload의 세부 스키마와 실행 의미는 003/004/011 하위 spec 소유로 남긴다.
- event log 저장 포맷, append-only 파일 처리, checkpoint/replay durability는 006 하위 spec 소유로 남긴다.

### 로컬 근거

- `crates/shacs-core/src/runtime/agent_loop.rs`
- `crates/shacs-core/src/runtime/runner.rs`
- `crates/shacs-core/src/runtime/loop_control.rs`
- `crates/shacs-core/tests/runtime_agent.rs`
- `crates/shacs-core/tests/runtime_loop.rs`

## TDD 계획

1. 외부 결과가 command가 아닌 형태로 들어오면 거절되는 테스트를 작성한다.
2. 재진입 command가 필수 식별자 없이 들어오면 거절되는 테스트를 작성한다.
3. 활성 correlation set에 없는 effect 결과가 late result로 분류되는 테스트를 작성한다.
4. 동일 재진입 command가 두 번 들어와도 상태가 한 번만 전이되는 테스트를 작성한다.
5. synthetic command와 외부 재진입 command가 같은 검증 경로를 통과하는 테스트를 작성한다.

## 구현 웨이브

### Wave 1. 메시지 공통 식별자 타입 고정

- `CommandId`, `EventId`, `EffectId`, `CorrelationId`, `CausationId` 타입을 도입한다.
- 재진입 command 공통 envelope와 kind별 payload 경계를 분리한다.
- 외부 actor가 상태 diff를 직접 담지 못하도록 타입 레벨 제약을 둔다.

### Wave 2. 재진입 정규화와 검증 계층

- provider, tool, subagent 결과를 공통 `ReentryCommand` 계층으로 받는다.
- 필수 식별자 검증, turn/effect 연계 검증, 상태 patch 금지 검증을 구현한다.
- 검증 실패를 panic이 아니라 명시적 거절 결과로 내린다.

### Wave 3. 중복, 만료, 닫힌 턴 처리

- 같은 `effect_id`와 결과 fingerprint 조합의 중복 재진입을 판별한다.
- 닫힌 턴 또는 현재 활성 correlation 집합 밖 결과를 late result로 분류한다.
- late result를 공식 상태 전이에서 배제하는 규칙을 연결한다.

### Wave 4. 오케스트레이터 통합

- `MainOrchestrator`가 모든 command를 단일 진입점에서 처리하도록 연결한다.
- synthetic command, 외부 command, 재진입 command가 동일한 권한 모델을 따르도록 정리한다.
- event/effect 방출이 이 경계 바깥에서 일어나지 않도록 검증한다.

## Verification Evidence

- Unit/integration evidence: `crates/shacs-core/tests/runtime_agent.rs` covers runtime bus serialization, session persistence, provider retry callbacks, tool-loop checkpointing, and callback panic isolation.
- Integration evidence: `crates/shacs-core/tests/runtime_loop.rs` covers duplicate active session/turn rejection, priority command bypass, provider/tool progress forwarding, channel context preservation, and subagent synthetic inbound handling.
- Static boundary evidence is maintained by the typed runtime modules in `crates/shacs-core/src/runtime/` and the compile-checked integration tests above.

## Open Risks

- kind별 payload를 Spec002에서 과도하게 통합하면 provider/tool/subagent 특성 차이를 숨길 수 있다. 세부 payload 의미는 각 owner spec에 둔다.
- append-only 저장소와 deterministic replay까지 Spec002 단독 보장으로 주장하면 Spec006 경계와 충돌할 수 있다.
- 참고 메모: shared correlation/envelope 계약이 003, 004, 012, 013, 014에도 걸쳐 있어, field 집합과 stale/duplicate 판정 기준이 문서별로 조금씩 드리프트할 위험이 있다.

## 종료 기준

- 외부 결과는 command로만 재진입한다.
- 모든 재진입 command는 상관관계 식별자를 가진다.
- 닫힌 턴과 만료된 effect 결과는 공식 상태를 바꾸지 못한다.
- `docs/specs/002-command-event-effect/SPEC.md`의 허용 전이와 금지 전이가 테스트로 증명된다.
