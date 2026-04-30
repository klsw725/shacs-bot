# Command, Event, Effect 아키텍처 명세

## 문서 목적

이 문서는 `docs/SYSTEM-FOUNDATION.md`를 구체화해 `shacs-bot`의 `Command`, `Event`, `Effect`를 어떻게 정의하고 연결할지 명세한다.

이 문서의 목표는 다음과 같다.

- 메인 오케스트레이터 단일 권한 원칙을 메시지 경계 수준에서 고정한다.
- 외부 시스템이 어떤 입력을 보낼 수 있고 어떤 출력만 받을 수 있는지 분명히 한다.
- future Rust 구현에서 trait, enum, ID 타입, 테스트 케이스를 도출할 수 있을 정도로 동작 규칙을 정리한다.
- tool roundtrip, subagent reentry, 외부 서비스 연동에서 상태 권한이 흔들리지 않도록 금지 패턴과 불변식을 고정한다.

이 문서는 세션 저장 방식, checkpoint 포맷, event log 영속화 내부 구조 같은 `session-store` 세부 구현은 다루지 않는다. 여기서 다루는 것은 메시지 의미와 상태 전이 규칙이다.

이 spec의 완료 기준은 `Command`, `Event`, `Effect`를 개념적으로만 흉내 내는 POC가 아니라, 이 문서가 요구하는 생산자/소비자 경계, 재진입 규칙, 금지 패턴, 불변식을 만족하는 **완전한 기능 구현과 검증**이다.

---

## 상위 기준과 범위

이 문서는 다음 상위 기준을 따른다.

- `MainOrchestrator`는 세션 상태를 변경할 수 있는 유일한 구성요소다.
- 바깥 시스템은 상태를 직접 수정하지 않는다.
- 바깥 시스템은 `Command`를 전달하거나, `Event`를 소비하거나, `Effect`를 실행하는 역할만 가진다.
- subagent와 tool runtime도 별도 권한자가 아니라 오케스트레이터의 하위 실행 경계다.

따라서 이 문서의 핵심 질문은 이것이다.

> 어떤 입력이 상태 전이를 요청할 수 있는가, 어떤 출력이 상태 전이 사실을 설명하는가, 어떤 요청이 외부 실행을 위임하는가?

답은 다음 세 종류로 고정한다.

- `Command`: 상태 전이를 요청하는 입력
- `Event`: 상태 전이가 일어났음을 기록하는 사실
- `Effect`: 외부 실행이 필요하므로 오케스트레이터가 방출하는 위임 요청

---

## 핵심 의미론

### Command

`Command`는 오케스트레이터에게 "이 의도를 검토하고, 허용되면 상태 전이를 수행하라"고 요청하는 입력이다.

특징은 다음과 같다.

- 아직 사실이 아니다.
- 거부될 수 있다.
- 중복 제출될 수 있으므로 재처리 규칙이 필요하다.
- 외부 actor가 만들 수 있지만, 해석과 승인 권한은 오케스트레이터에 있다.
- synthetic command 형태로 오케스트레이터 내부 흐름에서도 다시 만들어질 수 있다.

예시:

- 사용자 입력 제출
- 턴 중단 요청
- tool 결과 재진입 요청
- subagent 완료 결과 재진입 요청
- scheduler가 만든 "이 작업을 지금 시도해 달라" 요청

### Event

`Event`는 오케스트레이터가 판단을 완료한 뒤 "이 사실이 세션의 공식 이력에 포함되었다"고 선언하는 출력이다.

특징은 다음과 같다.

- 이미 일어난 사실이다.
- 외부 consumer는 `Event`를 보고 후속 동작을 할 수 있지만, event 자체를 수정할 수 없다.
- event는 재생 가능해야 하고, 같은 순서로 보면 같은 상태 전이를 설명할 수 있어야 한다.
- 상태를 설명해야지, 외부 실행 절차 자체를 명령해서는 안 된다.

예시:

- 사용자 메시지가 세션 입력으로 수용됨
- 모델 호출이 계획됨
- tool 호출이 요청됨
- tool 결과가 수용됨
- assistant 응답이 확정됨
- subagent 작업이 생성됨
- 세션이 abort 상태로 전이됨

### Effect

`Effect`는 오케스트레이터가 직접 실행하지 않는 I/O 또는 외부 작업을 수행해 달라고 바깥 실행 경계에 위임하는 요청이다.

특징은 다음과 같다.

- 상태 사실이 아니라 실행 요청이다.
- 외부 시스템은 effect를 수행할 수 있지만, 수행 결과를 상태에 직접 반영할 수 없다.
- effect 결과는 반드시 다시 `Command` 또는 이에 준하는 오케스트레이터 입력으로 재진입해야 한다.
- effect는 오케스트레이터가 승인한 정책 범위 안에서만 생성된다.

예시:

- LLM provider 호출
- tool runtime 실행
- subagent spawn
- mailbox 전송
- scheduler 등록 또는 해제

---

## 생산자와 소비자

### 1. Command 생산자

`Command`를 만들 수 있는 주체는 다음으로 제한한다.

- 사용자 인터페이스 계층
  - CLI
  - TUI
  - local HTTP API
- runtime service adapter
  - scheduler adapter
  - mailbox adapter
  - queue worker adapter
- effect executor reentry bridge
  - tool result bridge
  - provider result bridge
  - subagent result bridge
- 메인 오케스트레이터 내부 synthetic command 생성기

중요한 제약은 다음과 같다.

- 외부 주체는 command를 제출할 수만 있다.
- 외부 주체는 command를 적용할 수 없다.
- command payload는 상태 변경 의도를 담을 수 있지만 상태 diff를 직접 담아서는 안 된다.

### 2. Command 소비자

`Command`의 공식 소비자는 `MainOrchestrator` 하나뿐이다.

다른 구성요소는 command를 로깅하거나 큐잉할 수는 있어도, 해석해 상태를 바꾸는 소비자가 되어서는 안 된다.

### 3. Event 생산자

`Event`를 생산할 수 있는 주체도 `MainOrchestrator` 하나뿐이다.

tool runtime, provider adapter, subagent runner, scheduler는 event를 직접 쓰지 않는다. 이들은 결과를 재진입시키기 위한 입력만 제공한다.

### 4. Event 소비자

`Event`는 다음 주체가 소비할 수 있다.

- session-store 계층
- UI projection 계층
- logging / tracing 계층
- runtime service adapter
- 디버깅 / 관찰 도구

이 소비는 read-only여야 한다. event consumer가 event를 보고 독자적으로 세션 상태를 고치면 안 된다.

### 5. Effect 생산자

`Effect`는 오케스트레이터만 방출한다.

### 6. Effect 소비자

`Effect`는 effect dispatcher와 그 하위 executor가 소비한다.

예시:

- `ProviderEffect` → provider executor
- `ToolEffect` → tool runtime
- `SubagentEffect` → subagent runner
- `MailboxEffect` → mailbox adapter

effect consumer는 실행 결과를 직접 commit하지 않고, 오직 reentry input을 오케스트레이터에 되돌려준다.

---

## 허용 전이

메시지 흐름의 기본 규칙은 아래와 같다.

```text
external intent -> Command -> MainOrchestrator -> Event*
                                         |
                                         +-> Effect* -> external execution -> reentry Command -> MainOrchestrator
```

여기서 중요한 것은 상태 전이가 항상 오케스트레이터 중앙을 통과해야 한다는 점이다.

### 허용되는 기본 전이

- `Command -> Event`
  - command가 승인되거나 거부되며, 그 판단 결과가 event로 남는다.
- `Command -> Event + Effect`
  - command 처리 결과 외부 실행이 필요하면 event를 남기고 effect를 방출한다.
- `Effect result -> Command`
  - effect 실행 결과는 reentry command로 바뀌어 다시 오케스트레이터에 들어온다.
- `Event -> external reaction`
  - UI 갱신, 로그 기록, 후속 큐잉 같은 반응은 허용된다. 단, 세션 상태는 직접 바꾸지 못한다.

### 허용되지 않는 전이

- `external actor -> SessionState mutate`
- `Effect executor -> Event append`
- `Effect executor -> SessionState mutate`
- `Event consumer -> SessionState mutate`
- `Command producer -> Event fabricate`
- `Effect -> Effect` 직접 연쇄

마지막 항목은 특히 중요하다. effect executor가 자기 판단으로 또 다른 effect를 발행하면 상태 추적이 깨진다. 추가 실행이 필요하면 결과를 reentry command로 올리고, 오케스트레이터가 새 effect를 방출해야 한다.

---

## 재진입 규칙

재진입은 외부 실행 결과가 다시 메인 세션 흐름으로 들어오는 경계다. 이 경계가 느슨하면 단일 권한 원칙이 무너진다.

### 공통 재진입 원칙

- 모든 재진입 결과는 command로 정규화된다.
- 재진입 command는 원래 effect 또는 원래 turn과의 상관관계를 담아야 한다.
- 재진입 command는 성공, 실패, 취소, 타임아웃 중 어떤 결과인지 명시해야 한다.
- 이미 종료되었거나 더 이상 결과를 받을 수 없는 컨텍스트에 대한 재진입은 오케스트레이터가 안전하게 무시 또는 거부할 수 있어야 한다.
- 재진입은 상태 patch가 아니라 결과 보고여야 한다.

### tool result 재진입

tool runtime은 tool을 실행한 뒤 아래 같은 의미의 command만 반환할 수 있다.

- `ToolCallCompleted`
- `ToolCallFailed`
- `ToolCallTimedOut`
- `ToolCallCancelled`

이 command는 최소한 다음 정보를 가져야 한다.

- `session_id`
- `turn_id`
- `effect_id`
- `tool_call_id`
- `tool_name`
- 결과 상태
- 결과 payload 또는 오류 정보

tool runtime은 다음을 할 수 없다.

- assistant 메시지 본문을 직접 세션에 추가
- tool 결과를 보고 다음 tool을 독자적으로 실행
- permission 상태를 직접 변경

tool 결과를 받은 뒤 무엇을 할지는 오케스트레이터가 결정한다. 예를 들어 같은 턴에서 모델을 한 번 더 호출할지, 추가 tool 호출을 만들지, 실패로 턴을 종료할지는 모두 오케스트레이터 책임이다.

### subagent result 재진입

subagent runner는 child execution을 마친 뒤 아래 같은 의미의 command만 반환할 수 있다.

- `SubagentCompleted`
- `SubagentFailed`
- `SubagentTimedOut`
- `SubagentCancelled`

이 command는 최소한 다음 정보를 가져야 한다.

- `session_id`
- `parent_turn_id`
- `effect_id`
- `subagent_id`
- `task_id` 또는 이에 준하는 child work identity
- 결과 상태
- summary, artifacts, structured output 같은 결과 payload

subagent는 다음을 할 수 없다.

- parent session state 직접 수정
- parent turn을 완료 상태로 직접 전이
- 자기 결과를 event로 확정
- sibling subagent 결과와 독자적으로 병합 결정

subagent 결과의 병합은 항상 메인 오케스트레이터가 한다.

---

## ordering과 correlation 규칙

future Rust 구현은 모든 메시지에 대해 ordering과 correlation을 구분해서 다뤄야 한다.

### ordering 기대치

- 같은 세션 안의 공식 상태 전이 순서는 event 순서로 정의한다.
- 같은 turn 안에서는 오케스트레이터가 승인한 순서만이 공식 순서다.
- effect 실행 완료 순서는 외부 세계에서 뒤바뀔 수 있다.
- 뒤늦게 도착한 결과라도 오케스트레이터가 correlation을 확인한 뒤 채택 또는 폐기한다.

즉, 외부 완료 순서는 신뢰 대상이 아니고, 공식 순서는 오케스트레이터가 event로 확정한 순서다.

### correlation 기대치

최소한 다음 수준의 식별 연결이 가능해야 한다.

- 세션 수준: `session_id`
- 턴 수준: `turn_id`
- command 수준: `command_id`
- event 수준: `event_id`
- effect 수준: `effect_id`
- 외부 실행 수준: `tool_call_id`, `provider_call_id`, `subagent_id` 등 구체 ID
- 인과 연결: `causation_id`
- 연관 묶음: `correlation_id`

정확한 필드 이름은 구현에서 조정할 수 있지만, 의미는 유지해야 한다.

권장 규칙:

- 하나의 command를 처리하며 나온 event와 effect는 원 command를 `causation`으로 가리킨다.
- effect 결과 재진입 command는 원 effect를 `causation`으로 가리킨다.
- 같은 사용자 요청에서 파생된 전체 흐름은 같은 `correlation_id`를 공유할 수 있다.

> 참고 메모: 공통 correlation/envelope 계약의 정리 기준은 문서 끝 `부록 A. 공통 correlation 계약`을 따른다.
> 003, 004, 011, 012, 013, 014는 그 shared identifier와 처리 원칙을 각 실행 경계에서 운반하거나 노출하되, 의미를 독자 재정의하지 않는다.

### out-of-order 결과 처리 원칙

- 이미 superseded 된 effect 결과는 재진입돼도 stateful success로 채택하지 않는다.
- 이미 abort 된 turn에 대한 late result는 보관할 수는 있어도 활성 상태를 되살리면 안 된다.
- 같은 `effect_id`에 대해 중복 completion이 오면 오케스트레이터는 idempotent하게 처리해야 한다.

---

## 구현 지향 분류 기준

future Rust 구현에서 enum 또는 trait 경계를 설계할 때 아래 분류를 기준으로 삼는다.

### Command는 의도 중심이어야 한다

좋은 예:

- `SubmitUserInput`
- `ResumeSession`
- `AbortTurn`
- `ApplyToolResult`
- `ApplySubagentResult`

나쁜 예:

- `SetPendingTools(Vec<ToolResult>)`
- `WriteAssistantMessage(String)`
- `MarkSessionCompacted(bool)`

나쁜 예는 상태 patch에 가깝다. command는 "무엇을 시도할지"를 말해야지 "상태를 이렇게 바꿔라"를 직접 말하면 안 된다.

### Event는 사실 중심이어야 한다

좋은 예:

- `UserInputAccepted`
- `TurnStarted`
- `ToolCallRequested`
- `ToolResultAccepted`
- `AssistantResponseCommitted`
- `SubagentSpawned`
- `TurnAborted`

나쁜 예:

- `CallToolNow`
- `AskModelAgain`
- `MaybeRetryLater`

나쁜 예는 사실이 아니라 실행 의도다. 그런 것은 effect 또는 오케스트레이터 내부 정책 판단이어야 한다.

### Effect는 실행 위임 중심이어야 한다

좋은 예:

- `InvokeModel`
- `RunTool`
- `SpawnSubagent`
- `SendMailboxMessage`

나쁜 예:

- `AppendEventLog`
- `UpdateSessionState`
- `CommitCheckpoint`

마지막 항목들은 session-store 내부 구현 문제일 수는 있어도, 이 문서의 `Effect` 의미론에서는 외부 실행 위임으로 취급하지 않는다.

---

## positive sequence 예시

아래 시퀀스는 허용되는 흐름의 기준 예시다.

### 예시 1. 사용자 입력에서 assistant 응답까지, tool 없음

```text
1) CLI -> Command::SubmitUserInput
2) MainOrchestrator -> Event::UserInputAccepted
3) MainOrchestrator -> Event::TurnStarted
4) MainOrchestrator -> Effect::InvokeModel
5) Provider executor -> Command::ModelInvocationCompleted
6) MainOrchestrator -> Event::ModelOutputAccepted
7) MainOrchestrator -> Event::AssistantResponseCommitted
8) UI / logger / store consumer가 event를 소비
```

이 흐름에서 provider executor는 assistant 응답을 직접 쓰지 않는다. 모델 출력은 먼저 재진입 command가 되고, 응답 확정은 오케스트레이터가 event로 남긴다.

### 예시 2. tool result 재진입

```text
1) CLI -> Command::SubmitUserInput("README 요약해줘")
2) MainOrchestrator -> Event::UserInputAccepted
3) MainOrchestrator -> Event::TurnStarted
4) MainOrchestrator -> Effect::InvokeModel
5) Provider executor -> Command::ModelInvocationToolRequested(tool call proposal 포함)
6) MainOrchestrator -> Event::ModelOutputAccepted
7) MainOrchestrator -> Event::ToolCallRequested(tool_name=read)
8) MainOrchestrator -> Effect::RunTool(tool_name=read, args=...)
9) Tool runtime -> Command::ToolCallCompleted(effect_id=E-42, tool_call_id=T-9, payload=...)
10) MainOrchestrator -> Event::ToolResultAccepted(tool_call_id=T-9)
11) MainOrchestrator -> Effect::InvokeModel(tool result 포함)
12) Provider executor -> Command::ModelInvocationCompleted
13) MainOrchestrator -> Event::AssistantResponseCommitted
```

핵심은 9단계다. tool runtime은 결과 payload를 직접 conversation state에 쓰지 않고, `ToolCallCompleted` command로 재진입한다. 그 다음 단계에서만 오케스트레이터가 해당 결과를 세션의 공식 맥락으로 받아들일 수 있다.

### 예시 3. subagent result 재진입

```text
1) CLI -> Command::SubmitUserInput("코드베이스에서 인증 관련 구조 조사해줘")
2) MainOrchestrator -> Event::UserInputAccepted
3) MainOrchestrator -> Event::TurnStarted
4) MainOrchestrator -> Event::SubagentSpawnRequested
5) MainOrchestrator -> Effect::SpawnSubagent(task="auth structure survey")
6) Subagent runner -> Command::SubagentCompleted(subagent_id=S-3, summary=..., artifacts=...)
7) MainOrchestrator -> Event::SubagentResultAccepted(subagent_id=S-3)
8) MainOrchestrator -> Effect::InvokeModel(subagent summary 포함)
9) Provider executor -> Command::ModelInvocationCompleted
10) MainOrchestrator -> Event::AssistantResponseCommitted
```

이 흐름에서도 subagent는 조사 결과를 parent session에 직접 써 넣지 않는다. subagent 결과는 참고 가능한 입력일 뿐이고, 병합 여부와 사용 방식은 메인 오케스트레이터가 결정한다.

### 예시 4. late result 무시

```text
1) MainOrchestrator -> Effect::RunTool(effect_id=E-50)
2) 사용자 abort 요청 -> Command::AbortTurn
3) MainOrchestrator -> Event::TurnAborted
4) 나중에 Tool runtime -> Command::ToolCallCompleted(effect_id=E-50)
5) MainOrchestrator -> Event::LateResultIgnored(effect_id=E-50)
```

late result는 도착할 수 있다. 하지만 종료된 turn을 다시 활성화하면 안 된다.

---

## 금지 패턴

다음 패턴은 구현에서 명시적으로 금지한다.

### 1. 외부 actor의 직접 상태 변경

금지 예:

- CLI handler가 `SessionState`를 직접 수정
- tool runtime이 assistant message를 직접 append
- subagent runner가 parent task 상태를 직접 완료 처리

이 패턴은 메인 오케스트레이터 단일 권한 원칙을 깨뜨린다.

### 2. event를 명령처럼 사용

금지 예:

- `Event::CallToolNow`
- `Event::RetryProvider`

event는 일어난 사실이어야 한다. 실행 지시는 effect로 내려야 한다.

### 3. effect executor의 독자적 후속 실행

금지 예:

- tool runtime이 결과를 보고 다음 tool을 바로 실행
- provider adapter가 모델 결과를 보고 subagent를 스폰

후속 실행은 재진입 후 오케스트레이터가 결정해야 한다.

### 4. 외부에서 조립한 state patch 재진입

금지 예:

- `Command::ApplyStatePatch { json_patch: ... }`
- `Command::OverwriteConversation { messages: ... }`

외부는 결과와 의도만 전달해야 한다. 최종 상태 계산은 오케스트레이터가 한다.

### 5. 상관관계 없는 결과 수용

금지 예:

- `session_id`가 다른 tool 결과를 현재 turn에 반영
- 이미 종료된 `effect_id` 결과를 정상 완료로 채택
- 어떤 command나 effect에서 왔는지 추적 불가능한 subagent 결과 수용

---

## 구현 불변식

아래 불변식은 future Rust 구현에서 trait 계약, 테스트, 디버그 assertion으로 옮길 수 있어야 한다.

1. 세션 상태 변경은 오직 오케스트레이터를 통해서만 일어난다.
2. 모든 event는 하나의 오케스트레이터 판단 결과여야 한다.
3. 모든 effect는 하나 이상의 선행 command 또는 event와 인과 관계가 있어야 한다.
4. 모든 reentry command는 원 effect 또는 원 외부 입력과 correlation이 가능해야 한다.
5. effect executor는 상태 권한이 없다.
6. event consumer는 상태 권한이 없다.
7. 종료된 turn에 late result가 오더라도 turn이 암묵적으로 재개되면 안 된다.
8. 같은 effect 결과가 중복 도착해도 세션 상태는 중복 반영되지 않아야 한다.
9. subagent 결과 병합은 오케스트레이터만 결정한다.
10. tool 결과 채택 여부는 오케스트레이터만 결정한다.
11. external actor는 direct state mutation API를 가지면 안 된다.
12. command, event, effect는 의미가 겹치지 않도록 타입 수준에서 구분되어야 한다.

---

## trait / type 설계로 이어질 때의 체크포인트

구현 세부 타입 이름은 바뀔 수 있지만, 아래 질문에 모두 "예"라고 답할 수 있어야 한다.

- `Command`, `Event`, `Effect`가 서로 다른 enum 또는 trait 경계로 분리되어 있는가?
- 외부 executor가 받는 입력이 `Effect`로 제한되어 있는가?
- 외부 executor의 출력이 재진입 command로 제한되어 있는가?
- 모든 재진입 입력이 `session_id`, `turn_id`, `effect_id` 같은 correlation 정보를 포함하는가?
- idempotency와 late result 처리를 검증하는 테스트를 작성할 수 있는가?
- subagent 결과와 tool 결과가 동일한 패턴으로 "외부 실행 후 재진입" 모델을 따르는가?

이 질문 중 하나라도 "아니오"라면, 설계가 이 문서의 원칙에서 벗어났을 가능성이 높다.

---

## 테스트 관점에서 꼭 검증할 시나리오

future Rust 구현은 최소한 아래 시나리오를 테스트로 가져가야 한다.

- 정상 사용자 입력이 command를 거쳐 event와 effect로 이어지는가
- tool 완료 결과가 reentry command를 거쳐서만 세션에 반영되는가
- subagent 완료 결과가 reentry command를 거쳐서만 세션에 반영되는가
- 중복 tool completion이 한 번만 반영되는가
- abort 뒤 늦게 도착한 tool 결과가 활성 상태를 되살리지 않는가
- 상관관계가 맞지 않는 외부 결과가 거부되는가
- event consumer가 상태를 바꾸지 못하는 구조가 타입 또는 모듈 경계에서 보장되는가

---

## 문서 경계

이 문서는 다음을 정의한다.

- `Command`, `Event`, `Effect`의 의미
- 생산자와 소비자 경계
- 허용 전이와 금지 전이
- reentry 규칙
- ordering과 correlation 기대치
- 구현 불변식

이 문서는 다음을 정의하지 않는다.

- event log 파일 포맷
- checkpoint 저장 구조
- resume metadata 직렬화 방식
- queue / mailbox / scheduler 내부 자료구조
- provider 또는 tool runtime의 transport 세부 구현

그 항목들은 별도 하위 문서에서 정의한다. 특히 영속화 세부는 `session-store` 문서의 책임이다.

---

## 부록 A. 공통 correlation 계약

이 부록은 002가 의미 차원에서 소유하는 shared correlation 계약을 한 곳에 모아 적는다. 003, 004, 010, 011, 012, 013, 014는 아래 식별자와 처리 원칙을 각 실행 경계에서 운반하거나 노출할 수 있지만, 그 의미를 독자적으로 다시 정의하지 않는다.

### A.1 shared identifier set

- `session_id`: 어떤 세션에 속한 흐름인지 식별한다.
- `turn_id`: 어떤 열린 턴 또는 닫힌 턴에 속한 흐름인지 식별한다.
- `command_id`: 특정 입력 command 단위를 식별한다.
- `event_id`: 공식 상태 전이 fact를 식별한다.
- `effect_id`: 오케스트레이터가 승인해 발행한 외부 실행 단위를 식별한다.
- `causation_id`: 바로 직전 원인이 된 command, event, effect를 가리킨다.
- `correlation_id`: 하나의 사용자 요청 또는 그에 준하는 상위 흐름에서 파생된 관련 작업 묶음을 연결한다.

아래 식별자는 source-specific attached id 예시다.

- `tool_call_id`: tool runtime이 특정 tool 실행 단위를 식별할 때 사용한다.
- `provider_call_id`: provider runtime이 특정 모델 호출 단위를 식별할 때 사용한다.
- `subagent_id` 또는 `child_task_id`: subagent runtime이 특정 child 실행 단위를 식별할 때 사용한다.
- `approval_request_id`: approval surface가 특정 승인 요청과 응답을 연결할 때 사용한다.
- `service_correlation_id`: runtime service가 자기 내부 전달/재시도 흐름을 연결할 때 선택적으로 사용할 수 있다.

### A.2 공통 처리 원칙

- 외부 결과의 도착 순서는 공식 순서가 아니다. 공식 순서는 오케스트레이터가 event로 확정한 순서다.
- 중복 reentry는 idempotent하게 처리되어야 하며, 같은 결과가 다시 왔다고 해서 공식 상태가 두 번 적용되면 안 된다.
- 이미 닫힌 turn이나 superseded된 effect에 대한 stale 또는 late result는 관찰 가능하게 남길 수는 있어도 활성 상태를 되살리면 안 된다.
- out-of-order 결과의 수용/거절은 도착 시각이 아니라 active correlation, 현재 열린 turn, 관련 effect의 유효성 기준으로 판단해야 한다.
- stale, duplicate, accept, reject에 대한 최종 정책 판단은 `MainOrchestrator`가 한다. runtime과 service는 사실을 보고할 수는 있어도 그 판단을 대신하지 않는다.

### A.3 소유권 경계

- 002는 shared identifier의 의미 축과 causation/correlation 관계 규칙을 소유한다.
- 007은 stale/duplicate/late result를 실제로 수용할지 폐기할지에 대한 정책 판단을 소유한다.
- 003, 004, 011, 012는 provider/tool/subagent/service 경계에서 shared identifier와 그에 대응하는 specialized identifier를 포함한 envelope와 reentry payload를 운반한다.
- 010, 013, 014는 approval, inspect, diagnostics 같은 읽기 모델에서 이 식별자를 노출할 수 있지만, 그 의미를 안전성/UX/관측 계층 사정에 맞게 바꾸면 안 된다.
- 하위 spec은 exact field name을 동일하게 맞출 필요는 없지만, shared identifier와 specialized identifier 사이의 semantic mapping은 설명 가능해야 한다.

---

## 결론

`shacs-bot`에서 `Command`, `Event`, `Effect`는 단순한 메시지 이름이 아니라 권한 경계를 고정하는 핵심 장치다.

- `Command`는 상태 전이를 요청한다.
- `Event`는 상태 전이가 일어난 사실을 선언한다.
- `Effect`는 외부 실행을 위임한다.

이 세 경계가 유지되어야 메인 오케스트레이터가 유일한 상태 권한자로 남을 수 있고, tool roundtrip과 subagent reentry가 늘어나도 시스템이 여전히 설명 가능하고 재현 가능한 구조를 유지할 수 있다.
