# PRD 000. command reentry and correlation 완료 정리

## 목표

이 PRD는 `docs/specs/002-command-event-effect/SPEC.md`를 현재 런타임 구현에 맞게 닫기 위한 완료 문서다.

이전 문서는 provider, tool, subagent가 모두 공용 `causation_id`, `correlation_id`, `effect_id`를 보존하고 reentry command로 돌아온다고 설명했다. 현재 코드는 그렇지 않다. 이 PRD의 완료 의미는 false requirement를 제거하고, 실제 구현된 경계와 받아들인 gap을 002의 current architecture 기준으로 확정했다는 뜻이다.

## 제품 관점

`shacs-bot`은 사용자가 직접 설치하고 운영하는 personal, self hosted 성격의 런타임이다. 이 문서는 별도 관리자 조직이나 운영자 workflow를 전제로 하지 않는다. 핵심은 한 사용자의 세션 상태가 외부 adapter나 비동기 결과에 의해 임의로 바뀌지 않도록 경계를 분명히 하는 것이다.

## 범위

이 PRD의 범위는 완료 정리와 문서 정렬이다.

1. 002 spec을 future event sourcing 명세가 아니라 current architecture spec으로 바꾼다.
2. `Command`, `Event`, `Effect`를 공용 Rust 타입이 아니라 개념 어휘와 권한 경계로 설명한다.
3. 현재 runtime 파일과 타입을 정확히 매핑한다.
4. subagent correlation model을 보존한다.
5. provider/tool 영역에 아직 없는 shared identifier와 reentry envelope 주장을 제거한다.
6. event log와 replay는 006 session store 영역으로 돌린다.

## 범위 제외

이번 PRD는 다음을 요구하지 않는다.

1. 공용 `Command`, `Event`, `Effect` enum 또는 trait 추가
2. 공용 `EventId`, `EffectId`, `CorrelationId`, `CausationId` 타입 추가
3. provider/tool 실행 구조의 전면 재작성
4. session JSONL의 formal event log 전환
5. scheduler, mailbox, external worker 설계 추가
6. 기존 `.gitignore`나 코드 파일 수정

## 현재 구현 상태

### 완료 판정

2026-05-12 기준 이 PRD는 완료로 닫는다. 완료의 의미는 공용 `Command`/`Event`/`Effect` Rust boundary, 공용 `EventId`/`EffectId`/`CorrelationId`/`CausationId` 타입, event sourcing store, provider/tool async reentry envelope를 구현했다는 뜻이 아니다.

완료의 의미는 현재 코드에서 command, event, effect에 해당하는 표면을 정확히 매핑하고, 세션 변경 권한이 `AgentLoop`와 runtime orchestrator path에 있다는 기준을 문서로 고정했으며, subagent correlation은 보존하고 provider/tool gap은 accepted gap으로 남겼다는 뜻이다.

### 이미 반영된 것

- slash command와 loop command는 `crates/shacs-command/src/lib.rs`의 concrete 타입으로 매핑했다.
- channel envelope와 bus는 `InboundMessage`, `OutboundMessage`, `MessageBus`로 매핑했다.
- provider/tool loop는 `AgentRunner`, `ProviderEvent`, `ToolEvent`, checkpoint callback, `RuntimeToolCall`, `RuntimeToolMessage`, `RuntimeInterrupt`로 매핑했다.
- subagent reentry와 correlation은 `SpawnEnvelope`, `ChildResultEnvelope`, `MergeDecision`, stale correlation check, synthetic inbound publication 규칙으로 보존했다.
- session JSONL은 formal event log가 아니라 message persistence로 정리했다.

### 후속 비목표 / 별도 owner로 넘길 것

- 공용 `Command`/`Event`/`Effect` enum 또는 trait 도입은 002 완료 조건이 아니다.
- 공용 ID 타입 도입은 현재 gap으로 수용하며, 필요가 생기면 별도 설계로 다룬다.
- provider/tool late result idempotency는 외부 async worker 모델이 생길 때 다시 설계한다.
- event log, replay, checkpoint durability는 006 session store 영역에서 다룬다.
- scheduler, mailbox, external worker 구현 세부는 이 PRD에 추가하지 않는다.

### 로컬 근거

- `crates/shacs-command/src/lib.rs`
- `crates/shacs-channels/src/lib.rs`
- `crates/shacs-bus/src/lib.rs`
- `crates/shacs-core/src/runtime/agent_loop.rs`
- `crates/shacs-core/src/runtime/runner.rs`
- `crates/shacs-core/src/runtime/tool_execution.rs`
- `crates/shacs-core/src/runtime/subagent.rs`
- `crates/shacs-session/src/lib.rs`

## 현재 구현 매핑

### Slash and loop commands

`crates/shacs-command/src/lib.rs`가 현재 slash command와 loop command parsing boundary다.

1. `CommandId`는 알려진 slash command 목록이다.
2. `CommandKind`는 priority, exact, prefix, intercept 라우팅 종류다.
3. `ParsedCommand`는 raw input, matched command, args를 담는다.
4. `RoutedLoopCommand`는 parsed command와 `LoopCommand`를 연결한다.
5. `LoopCommand`는 runtime loop가 처리하는 command 의미다.

이 타입들은 현재 command vocabulary의 concrete한 일부지만, 전체 런타임 공용 `Command` 타입은 아니다.

### Channel envelopes and bus

`crates/shacs-channels/src/lib.rs`의 `InboundMessage`와 `OutboundMessage`는 channel boundary envelope다.

`crates/shacs-bus/src/lib.rs`의 `MessageBus`는 inbound와 outbound queue를 제공한다. bus는 메시지를 보관하고 전달하지만 session mutation을 승인하지 않는다.

### Agent loop authority

`crates/shacs-core/src/runtime/agent_loop.rs`의 `AgentLoop`가 현재 orchestration authority다.

`AgentLoop`는 inbound message를 처리하고, session을 로드하고, slash command를 해석하고, runner 결과를 session visible message로 반영한다. 따라서 외부 adapter가 session을 직접 수정하면 안 된다. 세션에 보이는 상태 변경은 `AgentLoop` 또는 그 runtime 하위 경로에서 결정되어야 한다.

### Provider and tool loop

`crates/shacs-core/src/runtime/runner.rs`의 `AgentRunner`가 provider/tool loop를 수행한다.

현재 provider/tool 경계는 다음 타입과 callback으로 표현된다.

1. `ProviderEvent`
2. `ToolEvent`
3. checkpoint callback
4. `RuntimeToolCall`
5. `RuntimeToolMessage`
6. `RuntimeInterrupt`

이 영역은 현재 shared `EffectId`, `CorrelationId`, `CausationId`를 쓰지 않는다. provider/tool late result idempotency도 공용 reentry command envelope로 처리하지 않는다. runner 안에서 provider 응답과 tool result를 model messages로 이어 붙이고, 상위 `AgentLoop`가 session 반영을 결정한다.

### Tool execution messages

`crates/shacs-core/src/runtime/tool_execution.rs`가 tool loop 내부 message 형식을 정의한다.

1. `RuntimeToolCall`은 tool id, name, arguments를 담는다.
2. `RuntimeToolMessage`는 tool_call_id, name, content를 담고 provider 대화 형식으로 변환된다.
3. `RuntimeInterrupt`는 ask user 같은 중단 표면을 담는다.

이것은 현재 tool execution contract다. 공용 effect envelope가 아니다.

### Subagent reentry and correlation

`crates/shacs-core/src/runtime/subagent.rs`는 현재 가장 명확한 reentry model을 갖고 있다.

1. `SpawnEnvelope`는 `session_id`, `parent_turn_id`, `child_task_id`, `spawn_effect_id`를 포함한다.
2. `ChildResultEnvelope`는 같은 네 식별자를 포함한다.
3. `MergeDecision`은 결과를 accept, retry, stale discard, abort 계열로 분류한다.
4. `correlation_decision`은 네 식별자 mismatch를 stale로 분류한다.
5. identifier mismatch로 stale 처리된 result는 synthetic inbound로 publish하지 않는다.
6. identifier mismatch로 stale 처리된 result는 active child를 정상 완료처럼 닫으면 안 된다.
7. accept 가능한 child result만 `InboundMessage`로 만들어져 parent session 경로에 다시 들어간다.

이 모델은 002에 남길 가치가 있다. 다만 이것도 공용 `EffectId`나 `CorrelationId` type system의 증거는 아니다.

### Session persistence

`crates/shacs-session/src/lib.rs`는 session message JSONL persistence다.

`Session`은 message list와 metadata를 갖고, `SessionManager`는 JSONL 파일 load, save, list, delete, clear를 담당한다. 이 저장 형식은 formal event log나 replay source가 아니다. event log와 replay를 다룬다면 006 session store 문서에서 다뤄야 한다.

## TDD 계획 결과

1. command parsing과 routing은 current code의 `shacs-command` 타입 매핑으로 확인한다.
2. session mutation authority는 `AgentLoop` runtime loop 테스트와 duplicate active turn 차단 테스트로 확인한다.
3. provider/tool loop는 runner tool loop 테스트로 확인한다.
4. subagent stale inbound 방어는 correlation mismatch가 session content로 저장되지 않는 테스트로 확인한다.
5. event sourcing이나 공용 ID 타입은 현재 목표가 아니므로 테스트 계획에 넣지 않는다.

결과: 완료. 002는 새 공유 타입 구현이 아니라 current architecture mapping과 accepted gap 문서화로 닫는다.

## 구현 웨이브 결과

### Wave 1. false requirement 제거

- provider, tool, subagent가 모두 같은 공용 causation/correlation/effect id를 보존한다는 설명을 제거한다.
- session JSONL을 formal event log라고 부르지 않는다.
- provider/tool async reentry envelope 요구를 현재 002 범위에서 제외한다.

결과: 완료. SPEC와 이 PRD가 current architecture 기준으로 다시 정렬됐다.

### Wave 2. 현재 구현 매핑 고정

- command, channel, bus, agent loop, runner, tool execution, subagent, session persistence 경계를 현재 파일과 타입에 연결한다.
- `Command`, `Event`, `Effect`를 공용 Rust boundary가 아니라 개념 어휘와 권한 경계로 설명한다.
- 세션에 보이는 변경은 `AgentLoop`와 runtime orchestrator path가 결정한다는 규칙을 유지한다.

결과: 완료. 현재 architecture criteria가 문서에 반영됐다.

### Wave 3. accepted gap과 후속 owner 분리

- 공용 ID 타입 부재를 blocker가 아니라 accepted gap으로 둔다.
- provider/tool late result idempotency는 외부 async worker가 생길 때의 후속 작업으로 남긴다.
- event log, replay, checkpoint durability는 006 session store 영역으로 넘긴다.

결과: 완료. 002는 current architecture 기준으로 닫고, 남은 항목은 future owner work로 분리한다.

## Residual Risks

이번 완료 판정은 다음 gap을 의도적으로 받아들인다.

1. 공용 `Command`, `Event`, `Effect` Rust boundary는 아직 없다.
2. provider/tool은 shared reentry envelope를 쓰지 않는다.
3. provider/tool result에 대한 late result idempotency는 현재 외부 async 실행 모델이 아니므로 002에서 강제하지 않는다.
4. subagent correlation은 구현돼 있지만 전체 runtime의 일반 correlation framework는 아니다.
5. session JSONL은 message persistence이며 event sourcing store가 아니다.

## 후속 owner 작업

후속 owner 작업은 전면 재작성이 아니라 경계별 점진 정리다.

1. 필요할 때 boundary별 optional shared identifier 또는 envelope를 추가한다.
2. provider/tool 실행이 async worker나 external process로 분리되면 late result idempotency와 correlation을 설계한다.
3. session event log, replay, checkpoint durability는 006 session store 쪽에서 다룬다.
4. 002 문서는 current runtime vocabulary와 authority boundary를 유지한다.

## Verification Evidence

완료 판정에 포함한 현재 코드 근거는 다음과 같다.

1. `crates/shacs-command/src/lib.rs`, slash command와 loop command parsing/routing 타입 확인
2. `crates/shacs-channels/src/lib.rs`, `InboundMessage`, `OutboundMessage` 확인
3. `crates/shacs-bus/src/lib.rs`, `MessageBus` inbound/outbound queue 확인
4. `crates/shacs-core/src/runtime/agent_loop.rs`, `AgentLoop` runtime orchestration 경계 확인
5. `crates/shacs-core/src/runtime/runner.rs`, `AgentRunner`, `ProviderEvent`, `ToolEvent`, checkpoint callback 확인
6. `crates/shacs-core/src/runtime/tool_execution.rs`, `RuntimeToolCall`, `RuntimeToolMessage`, `RuntimeInterrupt` 확인
7. `crates/shacs-core/src/runtime/subagent.rs`, `SpawnEnvelope`, `ChildResultEnvelope`, `MergeDecision`, stale correlation check, synthetic inbound publication 확인
8. `crates/shacs-session/src/lib.rs`, message JSONL session persistence 확인

완료 판정에 포함한 test evidence는 다음 명령이다.

- `cargo test --manifest-path crates/shacs-core/Cargo.toml --test runtime_loop loop_rejects_duplicate_active_turn_for_same_session --locked`
- `cargo test --manifest-path crates/shacs-core/Cargo.toml --test runtime_loop subagent_stale_inbound_is_not_persisted_as_session_content --locked`
- `cargo test --manifest-path crates/shacs-core/Cargo.toml --test runtime_agent runtime_runner_executes_tool_loop_and_accumulates_usage --locked`

## 종료 기준

1. 002 spec이 current architecture spec으로 읽힌다.
2. 공용 `Command`, `Event`, `Effect` 타입이 현재 있다고 말하지 않는다.
3. `EventId`, `EffectId`, `CorrelationId`, `CausationId` 공용 타입이 현재 있다고 말하지 않는다.
4. provider/tool/subagent가 모두 같은 correlation identifier를 보존한다고 말하지 않는다.
5. subagent의 실제 네 식별자 correlation rule은 보존한다.
6. session JSONL을 formal event log라고 말하지 않는다.
7. 변경 파일은 이 PRD와 상위 spec 두 개뿐이다.

위 기준은 2026-05-12 current architecture 기준으로 충족된 것으로 판정한다. 이 PRD는 완료 상태이며, 이후 변경은 새 002 wave가 아니라 관련 owner spec의 좁은 보강 PRD로 추가한다.
