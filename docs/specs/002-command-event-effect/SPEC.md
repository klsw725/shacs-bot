# 002. Command, Event, Effect 현재 아키텍처 명세

## 목적

이 문서는 현재 `shacs-bot` 런타임에서 `Command`, `Event`, `Effect`라는 말을 어떤 의미로 쓰는지 정리한다.

이 spec은 전체 코드를 공용 Rust enum, trait, event sourcing 구조로 다시 쓰라는 요구가 아니다. 현재 결정은 더 작다. `Command`, `Event`, `Effect`는 지금 구현에서 권한 경계와 메시지 의미를 설명하는 개념 어휘다. 아직 모든 crate가 공유하는 정식 `Command`, `Event`, `Effect` 타입 경계는 없다.

목표는 다음과 같다.

1. 외부 입력, 런타임 판단, 외부 실행의 권한 경계를 설명한다.
2. 세션에 보이는 상태 변경은 `AgentLoop`와 런타임 오케스트레이터 경로에서만 결정한다는 불변식을 고정한다.
3. slash command, channel message, provider loop, tool loop, subagent 결과가 현재 코드에서 어디에 놓이는지 연결한다.
4. 앞으로 공용 envelope나 식별자를 추가하더라도 기존 런타임을 무리하게 갈아엎지 않도록 범위를 제한한다.

## 범위

이 문서는 현재 런타임 아키텍처 명세다. 세션 저장소의 event log, replay, durability 설계는 006 session store 계열 문서가 다룬다. 이 문서는 message JSONL을 formal event log라고 부르지 않는다.

## 핵심 용어

### Command

`Command`는 런타임에 의도를 전달하는 입력이라는 뜻으로 쓴다. 현재 코드에는 두 종류의 실제 표면이 있다.

1. Slash 또는 loop command 파싱과 라우팅, `crates/shacs-command/src/lib.rs`
2. 채널에서 들어오는 사용자 입력 envelope, `crates/shacs-channels/src/lib.rs`의 `InboundMessage`

현재 `CommandId`, `CommandKind`, `ParsedCommand`, `RoutedLoopCommand`, `LoopCommand`는 slash command와 loop command를 식별하고 라우팅하는 타입이다. 이것은 전체 런타임의 모든 입력을 담는 공용 `Command` enum이 아니다.

`InboundMessage`는 채널 경계의 입력 envelope다. 사용자의 자연어 메시지, 시스템이 만든 synthetic inbound, subagent 완료 알림 같은 입력이 이 표면으로 들어올 수 있다. 입력을 세션 상태로 받아들일지, 어떤 메시지를 추가할지, 어떤 실행을 시작할지는 런타임 오케스트레이터가 결정한다.

### Event

`Event`는 런타임이 관찰 가능한 진행 사실을 알리는 말로 쓴다. 현재 구현은 formal event log를 갖고 있지 않다.

현재 event에 가까운 표면은 다음과 같다.

1. `AgentRunner`가 provider와 tool 진행을 알리기 위해 쓰는 `ProviderEvent`, `ToolEvent`
2. session JSONL에 저장되는 message 기록
3. subagent 상태와 완료 결과가 synthetic inbound로 부모 세션에 다시 들어오는 흐름

`ProviderEvent`와 `ToolEvent`는 provider/tool loop 내부 관찰 신호다. 이것들은 공용 `EventId`나 `CorrelationId`를 가진 시스템 event 타입이 아니다. session JSONL 역시 세션 메시지 저장 형식이지 event sourcing 로그가 아니다.

### Effect

`Effect`는 런타임이 바깥 실행 경계에 일을 맡긴다는 의미로 쓴다. 현재 구현에서 effect에 가까운 실행은 다음 위치에 있다.

1. provider 호출, `AgentRunner` 내부 provider loop
2. tool 실행, `RuntimeToolCall`, `RuntimeToolMessage`, `RuntimeInterrupt`
3. subagent spawn과 child result 처리, `SpawnEnvelope`, `ChildResultEnvelope`, `MergeDecision`
4. outbound channel message publication, `OutboundMessage`와 `MessageBus`

현재 코드는 공용 `EffectId` 타입을 쓰지 않는다. provider와 tool 결과도 shared `EffectId`, `CorrelationId`, reentry command envelope로 오가지 않는다. provider/tool 실행은 `AgentRunner` 안에서 `RuntimeToolCall`, `RuntimeToolMessage`, `ToolEvent`, `ProviderEvent`, checkpoint callback을 통해 관리된다.

## 현재 런타임 구성

### Command parsing boundary

`crates/shacs-command/src/lib.rs`는 slash command와 loop command의 현재 권위다.

1. `CommandId`는 알려진 slash command를 식별한다.
2. `CommandKind`는 priority, exact, prefix, intercept 라우팅 성격을 나타낸다.
3. `ParsedCommand`는 raw input, matched command, args를 담는다.
4. `RoutedLoopCommand`는 파싱된 command와 실제 `LoopCommand`를 연결한다.
5. `LoopCommand`는 runtime loop가 처리할 명령 의미를 담는다.

이 crate는 현재 slash command 라우터다. 모든 런타임 입력을 대표하는 공용 command bus가 아니다.

### Channel boundary

`crates/shacs-channels/src/lib.rs`의 `InboundMessage`와 `OutboundMessage`는 채널 경계 envelope다.

1. `InboundMessage`는 channel, sender, chat, content, media, metadata, optional session key override를 담는다.
2. `OutboundMessage`는 channel, chat, content, reply target, media, metadata, buttons를 담는다.

외부 adapter는 이 envelope를 통해 입력을 넣거나 출력을 소비할 수 있다. 세션을 직접 수정하면 안 된다.

### Message bus

`crates/shacs-bus/src/lib.rs`의 `MessageBus`는 inbound와 outbound queue를 제공한다.

1. `publish_inbound`와 `consume_inbound` 계열은 runtime으로 들어오는 메시지를 다룬다.
2. `publish_outbound`와 `consume_outbound` 계열은 채널로 나갈 메시지를 다룬다.
3. bus는 queue다. 상태 전이 정책이나 session mutation 권한을 갖지 않는다.

### Runtime orchestrator

`crates/shacs-core/src/runtime/agent_loop.rs`의 `AgentLoop`가 현재 오케스트레이터다.

`AgentLoop`는 inbound message를 받아 session을 로드하고, slash command를 처리하고, agent run을 시작하고, assistant 결과와 channel delivery를 session에 반영한다. 이 경로가 현재 session visible state change의 권한자다.

핵심 불변식은 다음과 같다.

1. 외부 adapter는 session 파일이나 session 객체를 직접 바꾸지 않는다.
2. 사용자에게 보이는 session message 추가, 중단, 재시작, subagent 결과 반영은 `AgentLoop` 또는 그 런타임 하위 경로에서 결정한다.
3. bus, channel, provider, tool, subagent 실행자는 입력과 결과를 전달할 수 있지만 session state의 최종 승인자가 아니다.

### Provider and tool runner

`crates/shacs-core/src/runtime/runner.rs`의 `AgentRunner`는 provider/tool loop를 실행한다.

1. provider 호출은 `ProviderClient`와 `ProviderEvent` callback을 통해 관찰된다.
2. tool 호출은 `RuntimeToolCall`로 표현되고, 결과는 `RuntimeToolMessage`로 모델 대화에 다시 들어간다.
3. tool 진행은 `ToolEvent`로 관찰된다.
4. runner는 checkpoint callback으로 `awaiting_tools`, `tools_completed` 같은 진행 상태를 남길 수 있다.
5. `RuntimeInterrupt`는 현재 ask user 같은 tool 중단 표면을 표현한다.

이 경계는 지금 `AgentRunner` 안에 있다. provider/tool late result idempotency를 위한 공용 `EffectId`, `CorrelationId`, reentry command envelope는 아직 없다. provider/tool 실행이 앞으로 비동기 외부 실행으로 분리될 때 그 식별자 설계를 다시 판단한다.

### Tool execution messages

`crates/shacs-core/src/runtime/tool_execution.rs`는 런타임 tool message 형식을 정의한다.

1. `RuntimeToolCall`은 provider가 요청한 tool name, id, arguments를 담는다.
2. `RuntimeToolMessage`는 tool result를 model message로 되돌리는 형식이다.
3. `RuntimeInterrupt`는 tool 실행이 사용자 입력을 기다리는 경우를 나타낸다.

이 타입들은 tool loop 내부의 현재 계약이다. 공용 effect envelope는 아니다.

### Subagent runtime

`crates/shacs-core/src/runtime/subagent.rs`는 subagent spawn과 result merge를 관리한다.

현재 subagent correlation model은 유용하므로 보존한다.

1. `SpawnEnvelope`는 `session_id`, `parent_turn_id`, `child_task_id`, `spawn_effect_id`를 포함한다.
2. `ChildResultEnvelope`는 같은 네 식별자를 되돌려준다.
3. `MergeDecision`은 완료 결과를 full accept, summary accept, failure fact accept, retry, stale discard, parent abort 중 하나로 분류한다.
4. `correlation_decision`은 `child_task_id`, `session_id`, `parent_turn_id`, `spawn_effect_id` mismatch를 stale로 분류한다.
5. identifier mismatch로 stale 처리된 result는 inbound로 publish하지 않고 active child를 닫지 않아야 한다.
6. accept 가능한 결과만 synthetic `InboundMessage`로 publish되어 부모 세션 경로에 다시 들어간다.

이 흐름은 현재 002에서 가장 concrete한 command reentry 모델이다. 다만 이것도 공용 `CausationId`, `CorrelationId`, `EffectId` 타입을 쓰는 일반화된 effect system은 아니다.

### Session persistence

`crates/shacs-session/src/lib.rs`는 session을 message JSONL 파일로 저장한다.

1. `Session`은 key, messages, timestamps, metadata, consolidation marker를 가진다.
2. `SessionManager`는 session 파일 경로, load, save, list, delete, clear를 관리한다.
3. 저장 파일은 metadata line과 message line들로 구성된다.

이 저장소는 현재 session message persistence다. formal event log, replay engine, event sourcing store라고 부르지 않는다.

## 권한 규칙

현재 구현에서 지켜야 할 규칙은 다음과 같다.

1. 세션에 보이는 상태 변경은 `AgentLoop`와 runtime orchestrator path가 결정한다.
2. 외부 adapter는 `InboundMessage`를 넣고 `OutboundMessage`를 소비한다.
3. `MessageBus`는 queue 역할만 한다.
4. slash command parser는 command text를 해석하지만 session mutation을 직접 승인하지 않는다.
5. `AgentRunner`는 provider/tool loop를 수행하고 결과 messages와 events를 돌려준다. 세션 반영은 상위 runtime 경로가 한다.
6. subagent 결과는 correlation check를 통과해야 synthetic inbound로 부모 세션에 들어갈 수 있다.
7. identifier mismatch로 stale 처리된 subagent result는 관찰 가능한 discard decision일 수 있지만 부모 세션 입력으로 publish되면 안 되고 active child를 닫으면 안 된다.

## 현재 구현 상태

### 완료 판정

2026-05-12 기준 이 spec은 완료로 닫는다. 완료의 의미는 공용 `Command`/`Event`/`Effect` Rust 경계나 공용 `EventId`/`EffectId`/`CorrelationId`/`CausationId` 타입을 새로 구현했다는 뜻이 아니라, current architecture에서 쓰는 command, event, effect 개념과 권한 경계가 현재 코드와 문서에 맞게 매핑됐다는 뜻이다.

이 spec은 event sourcing 구현 완료를 주장하지 않는다. session JSONL은 message persistence이며, formal event log와 replay engine은 006 session store 영역의 후속 주제다. provider/tool async reentry envelope도 현재 002의 요구가 아니다.

### 이미 반영된 것

- slash command와 loop command 경계는 `crates/shacs-command/src/lib.rs`의 concrete 타입으로 설명돼 있다.
- channel input/output 경계는 `InboundMessage`, `OutboundMessage`, `MessageBus`로 설명돼 있다.
- 세션에 보이는 상태 변경 권한은 `AgentLoop`와 runtime orchestrator path에 남아 있다.
- provider/tool 진행은 `AgentRunner`, `ProviderEvent`, `ToolEvent`, `RuntimeToolCall`, `RuntimeToolMessage`, checkpoint callback으로 매핑돼 있다.
- subagent reentry는 `SpawnEnvelope`, `ChildResultEnvelope`, `MergeDecision`, stale correlation check, synthetic inbound publication 규칙으로 문서화돼 있다.
- session JSONL은 event sourcing store가 아니라 message persistence로 정리돼 있다.

### 후속 비목표 / 별도 owner로 넘길 것

- 공용 `Command`/`Event`/`Effect` 타입 도입은 현재 002 완료 조건이 아니다.
- 공용 `EventId`/`EffectId`/`CorrelationId`/`CausationId` 타입 도입은 필요가 입증될 때 별도 설계로 다룬다.
- provider/tool 실행이 외부 async worker로 분리될 때만 late result idempotency와 reentry envelope를 다시 검토한다.
- event log, replay, checkpoint durability는 006 session store 영역에서 다룬다.
- scheduler나 mailbox 구현 세부는 002에 추가하지 않는다.

### 로컬 근거

- `crates/shacs-command/src/lib.rs`
- `crates/shacs-channels/src/lib.rs`
- `crates/shacs-bus/src/lib.rs`
- `crates/shacs-core/src/runtime/agent_loop.rs`
- `crates/shacs-core/src/runtime/runner.rs`
- `crates/shacs-core/src/runtime/tool_execution.rs`
- `crates/shacs-core/src/runtime/subagent.rs`
- `crates/shacs-session/src/lib.rs`

## 후속 owner 작업

현재 결정은 full rewrite를 피하고, 필요한 경계만 점진적으로 명확히 하는 것이다. 후속 owner 작업은 다음 정도로 제한한다.

1. 경계별 optional shared identifier 또는 envelope 추가 검토, 예를 들어 subagent에는 이미 네 식별자가 있으므로 이를 먼저 문서화하고, provider/tool이 외부 비동기 실행이 될 때만 확장한다.
2. provider/tool 실행이 async 또는 외부 worker로 분리될 경우 late result idempotency와 correlation 검증 추가.
3. event log, replay, checkpoint durability는 006 session store 영역에서 다룬다.
4. 현재 개념 용어와 실제 타입 이름이 헷갈리지 않도록 문서와 테스트 이름을 계속 맞춘다.

## 비목표

다음은 이 spec의 요구가 아니다.

1. 공용 `Command`, `Event`, `Effect` enum 또는 trait를 즉시 추가하는 것
2. 공용 `EventId`, `EffectId`, `CorrelationId`, `CausationId` 타입이 이미 있다고 주장하는 것
3. session JSONL을 formal event log로 재정의하는 것
4. provider, tool, subagent를 모두 같은 reentry command envelope로 강제하는 것
5. scheduler나 mailbox 구현 세부를 002에 추가하는 것

## 완료 기준

이 spec은 현재 런타임을 있는 그대로 설명할 때 완료된 것으로 본다. 2026-05-12 기준 아래 기준은 current architecture 기준으로 충족됐고, 002는 이 기준으로 닫는다.

1. 문서가 현재 타입과 파일 위치를 정확히 가리킨다.
2. `Command`, `Event`, `Effect`가 개념 어휘이며 아직 공용 Rust 타입 경계가 아니라고 분명히 말한다.
3. `AgentLoop` 중심 session mutation 권한을 유지한다.
4. subagent correlation invariant를 보존한다.
5. provider/tool 영역의 현재 한계를 과장하지 않는다.

이후 변경은 002 재오픈이 아니라 관련 owner spec의 좁은 보강 PRD로 추가한다.
