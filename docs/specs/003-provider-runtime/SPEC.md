# provider runtime 아키텍처 명세

## 문서 목적

이 문서는 현재 코드베이스의 provider runtime 구조를 기준으로, 모델 호출 경계와 세션 상태 확정 경계를 정리한다. Spec 001은 세션 커널을, Spec 002는 Command, Event, Effect 용어를 아키텍처 권한 경계로 정리했다. 이 문서도 같은 기준을 따른다.

현재 구현은 독립적인 공유 Rust enum인 `Effect::InvokeModel` 실행기를 중심으로 하지 않는다. 실제 호출 경계는 `shacs-providers`의 provider 클라이언트 계층과 `shacs-core`의 `AgentRunner`, `AgentLoop` 조합으로 구성된다.

핵심 불변식은 유지한다. provider 출력은 세션에 기록될 후보 데이터일 뿐이며, 세션에 보이는 상태는 runtime 또는 orchestrator 경계가 결정한다. provider 코드는 세션을 직접 변경하면 안 된다.

## 현재 범위

이 문서는 다음을 설명한다.

* `ProviderRequest`, `AgentRunSpec`, `ProviderClient`가 현재 `InvokeModel`에 해당하는 호출 경계를 어떻게 구성하는지
* provider별 응답이 `LlmResponse`, `ToolCallRequest`, `ProviderEvent`로 정규화되는 방식
* streaming, tool call, retry, cancellation, timeout의 현재 구현 위치
* formal late result correlation과 reentry command 모델을 후속 owner 작업으로 분리하는 기준

현재 완료 판정은 다음을 blocker로 보지 않는다. 이 항목들은 구현됐다고 주장하지 않고, 후속 owner 작업으로 남긴다.

* 공유 `InvokeModelEffect` 타입
* 공유 `ProviderInvocationOutcome` 또는 `ModelInvocationOutcome` 타입
* `ModelInvocationCompleted`, `ModelInvocationToolRequested` 같은 별도 재진입 command
* per effect `effect_id` 또는 `correlation_id` 기반 late result 처리
* async provider worker와 late result 수거 프레임워크

## 현재 구현 상태

### 완료 판정

2026-05-13 기준 Spec 003은 완료로 닫는다. 완료의 의미는 formal `InvokeModelEffect`/`ProviderInvocationOutcome`/`ModelInvocation*` reentry command나 per effect `effect_id`/`correlation_id` 기반 late result framework를 구현했다는 뜻이 아니다.

완료의 의미는 현재 코드의 `ProviderClient`/`ProviderRequest`/`ProviderEvent`/`LlmResponse`/`ToolCallRequest` 표면과 `AgentRunner`/`AgentLoop` 경계가 provider runtime의 current architecture 기준을 충족한다고 문서상 확정한다는 뜻이다.

### 이미 반영된 것

* provider 호출 입력은 `AgentRunSpec`과 iteration별 `ProviderRequest`로 구성된다.
* provider별 raw 응답은 `LlmResponse`, `ToolCallRequest`, `ProviderEvent`로 정규화된다.
* `AgentRunner`는 provider 호출, streaming callback, retry, cancellation check, tool loop handoff를 담당한다.
* `AgentLoop`는 `AgentRunResult`를 session visible state로 반영하는 경계다.
* provider raw 응답과 stream chunk가 세션 상태를 직접 바꾸지 않는 불변식은 현재 구조와 테스트 evidence로 고정돼 있다.

### 후속 비목표 / 별도 owner로 넘길 것

* formal `InvokeModelEffect`와 `ProviderInvocationOutcome` 계열 타입 도입은 003 완료 조건이 아니다.
* `ModelInvocationCompleted`, `ModelInvocationToolRequested` 같은 별도 reentry command는 현재 runtime loop를 대체하지 않는다.
* provider 호출별 `effect_id`/`correlation_id` late result correlation과 async worker 수거 framework는 외부 async provider worker가 필요해질 때 설계한다.
* `ProviderStreamBuffer`, `ProviderCallOutput::Final`, `TurnCompleted` 같은 이름은 현재 코드의 evidence로 쓰지 않는다.

## Command, Event, Effect 용어의 의미

Spec 002의 Command, Event, Effect 용어는 현재 코드에서 권한과 책임을 설명하는 개념어다. 모든 개념이 동일한 이름의 공유 Rust enum으로 존재한다는 뜻이 아니다.

provider 호출은 개념적으로 외부 Effect에 가깝다. 하지만 현재 구현에서 모델 호출은 `AgentRunner::run` 내부 루프가 `ProviderRequest`를 만들고 `ProviderClient`를 호출하는 방식으로 실행된다. 결과 적용은 `AgentRunner`가 만든 `AgentRunResult`를 `AgentLoop`가 세션에 반영하는 경계에서 일어난다.

따라서 이 문서에서 "provider 결과가 재진입한다"는 말은 현재 코드에서는 별도 `ModelInvocation*` command를 뜻하지 않는다. provider 후보 결과가 `AgentRunner`의 루프를 거쳐 `AgentLoop`의 세션 반영 경계로 돌아온다는 뜻이다.

## 현재 주요 구성 요소

### `shacs-providers`

`crates/shacs-providers/src/provider.rs`는 provider 호출의 공통 표면을 둔다.

* `ProviderClient`는 `chat`과 `chat_stream`을 제공한다.
* `ProviderRequest`는 `messages`, `tools`, `model`, `settings`, `tool_choice`를 담는다.
* `ProviderEvent`는 stream 중간 관찰값인 `TextDelta`, `ReasoningDelta`, `ToolCallStart`, `ToolCallDelta`, `ToolCallReady`, `Finish`를 표현한다.

`crates/shacs-providers/src/types.rs`는 정규화된 provider 결과를 둔다.

* `GenerationSettings`는 `temperature`, `max_tokens`, `reasoning_effort`를 담는다.
* `ToolCallRequest`는 provider가 제안한 tool call 후보다.
* `LlmResponse`는 최종 text 후보, tool call 후보, usage, reasoning, error metadata를 담는다.
* `finish_reason`은 현재 닫힌 enum이 아니라 문자열이다. 대표 값은 `stop`, `tool_calls`, `length`, `error`처럼 provider 변환 로직이 넣는 값이다.

`crates/shacs-providers/src/retry.rs`는 retry 정책을 둔다.

* `ProviderRetryMode`는 `Standard`와 `Persistent`를 구분한다.
* retry decision은 provider error와 `finish_reason == "error"` 응답을 보고 내려진다.
* retry 대기는 `ProviderRetryWaiter`를 통해 실행되며, runtime은 이 대기를 callback으로 관찰할 수 있다.

`crates/shacs-providers/src/clients/mod.rs`는 provider resolution과 request 준비를 담당한다.

* `resolve_provider_client`는 `ProviderRegistry`와 사용자 provider config를 보고 실제 `ProviderClient`를 고른다.
* `provider_client_from_config`는 backend별 client를 만든다.
* `validate_provider_config`는 현재 필요한 config 검증을 수행한다.
* `prepare_provider_request`는 resolved model, messages, tools, defaults, optional settings, optional tool choice를 `ProviderRequest`로 만든다.

`crates/shacs-providers/src/registry.rs`는 provider 목록과 matching 규칙을 둔다.

* `ProviderRegistry`는 `ProviderSpec` 목록을 보관한다.
* `ProviderSpec`은 backend, auth 성격, local 여부, model prefix 처리, thinking style 같은 provider metadata를 담는다.
* `match_provider`는 명시 provider, model prefix, keyword, local provider, configured provider 순서로 후보를 고른다.

### provider client 구현

`crates/shacs-providers/src/clients/openai_compatible`, `anthropic`, `codex`, `azure_openai`는 공통 `ProviderClient` 표면을 provider별 HTTP 요청과 응답 파서에 연결한다.

* OpenAI compatible client는 chat completions 또는 responses 계열 요청을 만들고 JSON 응답과 SSE stream을 `LlmResponse`, `ProviderEvent`로 변환한다.
* Anthropic client는 messages 요청을 만들고 text, thinking, tool use fragment, finish 정보를 공통 타입으로 변환한다.
* Codex client는 responses stream 계열 이벤트를 OpenAI responses parser 경로로 변환한다.
* Azure OpenAI client는 Azure responses request를 만들고 responses SSE 이벤트를 파싱한다. streaming transport가 없을 때는 지원되는 경로에서 단일 응답으로 fallback할 수 있다.

각 client는 provider raw 응답을 세션 기록으로 직접 쓰지 않는다. request builder와 parser는 후보 데이터를 만들 뿐이다.

### `shacs-core` runtime

`crates/shacs-core/src/runtime/runner.rs`는 현재 provider runtime의 중심이다.

* `AgentRunSpec`은 messages, tools, provider client, model, generation settings, retry mode, callbacks, cancellation token, checkpoint callback을 담는다.
* `AgentRunner`는 iteration마다 `ProviderRequest`를 만들고 `ProviderClient`를 호출한다.
* streaming이 필요하면 `chat_stream_with_retry_using_waiter`를 사용하고, 아니면 `chat_with_retry_using_waiter`를 사용한다.
* `ProviderEvent`는 `provider_event_callback`으로 전달된다. `TextDelta`는 streaming을 원하는 `AgentHook`에도 전달될 수 있다.
* callback panic은 runtime을 깨지 않도록 격리된다.
* cancellation token은 provider 호출 전후와 tool 실행 경계에서 확인된다.
* provider retry 대기는 `retry_wait_callback`으로 관찰할 수 있다.
* checkpoint callback은 tool 대기, tool 완료, final response 같은 runtime 중간 상태를 저장하는 데 쓰인다.

`crates/shacs-core/src/runtime/agent_loop.rs`는 session visible state를 확정하는 쪽이다.

* `AgentLoop`는 inbound message를 session history와 context로 바꾸고 `AgentRunSpec`을 구성한다.
* `AgentRunner`가 반환한 `AgentRunResult`의 새 messages를 session에 append한다.
* provider error는 session에 assistant error message로 반영되고 outbound error로 publish된다.
* runtime checkpoint는 session metadata에 저장된다.

이 경계가 중요하다. provider client와 parser는 세션을 모른다. `AgentRunner`도 provider 후보를 runtime loop의 다음 행동으로 해석하지만, session visible state 저장은 `AgentLoop`가 담당한다.

### surface와 adapter

`crates/shacs-api/src/lib.rs`와 `crates/shacs-cli/src/lib.rs`는 provider runtime의 핵심 정의가 아니라 consumer와 adapter 표면이다.

* API 표면은 chat completion 요청을 `ProviderRequest`와 `LlmResponse`에 맞춰 다룬다.
* CLI 표면은 provider resolution, config, runtime loop 구성, streaming 출력 같은 self hosted 사용 경로를 연결한다.

## `Effect::InvokeModel` 대신 현재 호출 매핑

미래형 `Effect::InvokeModel` 필드를 현재 코드에 그대로 대입하면 정확하지 않다. 현재 매핑은 다음과 같다.

| 개념 | 현재 코드 |
| --- | --- |
| 호출 입력 | `AgentRunSpec`과 iteration별 `ProviderRequest` |
| provider 선택 | `ProviderRegistry`, `resolve_provider_client`, `provider_client_from_config` |
| model과 generation 설정 | `AgentRunSpec.model`, `AgentRunSpec.settings`, `ProviderRequest.model`, `ProviderRequest.settings` |
| message snapshot | `AgentRunSpec.initial_messages`, loop 내부 `messages`, `govern_messages_for_model` 결과 |
| tool schema snapshot | `ToolRegistry::definitions()`가 `ProviderRequest.tools`로 전달됨 |
| streaming 관찰 | `ProviderEvent`, `provider_event_callback`, `AgentHook::on_stream` |
| retry | `ProviderRetryMode`, `chat_with_retry_using_waiter`, `chat_stream_with_retry_using_waiter` |
| cancellation | `AgentRunSpec.cancellation_token` checks |
| 결과 후보 | `LlmResponse.content`, `LlmResponse.tool_calls`, reasoning fields, usage, `finish_reason` |
| session 반영 | `AgentLoop`가 `AgentRunResult.messages`를 session에 append |

`session_id`, `turn_id`, `effect_id`, `correlation_id`를 가진 공유 invocation envelope는 현재 provider runtime에 없다. 이 식별자 기반 late result 모델은 남은 설계 작업이다.

## 결과 정규화

현재 공통 결과는 `LlmResponse`다.

`content`는 assistant text 후보이며, `tool_calls`는 tool call 후보 목록이다. `usage`는 token 사용량 등 provider가 준 계량값을 담는다. `reasoning_content`와 `thinking_blocks`는 reasoning 계열 정보를 보관한다. error 계열 필드는 provider error 응답을 retry 판단과 진단에 쓰기 위한 metadata다.

`ToolCallRequest`는 실행 명령이 아니다. provider가 제안한 `id`, `name`, `arguments`와 provider별 보존 필드를 담는 후보 데이터다.

`finish_reason`은 문자열이므로 이 문서는 closed enum 값을 요구하지 않는다. 현재 runtime은 `LlmResponse::should_execute_tools`에서 tool call 존재 여부와 `finish_reason` 문자열을 함께 보고 tool loop로 갈지 판단한다.

## streaming 규칙

Streaming parser와 client는 `ProviderEvent`를 emit한다. 이 이벤트는 다음 용도로만 사용된다.

* UI나 CLI에서 진행 중 text delta를 보여주기
* provider event callback으로 관찰하기
* `AgentHook`이 원하는 경우 text stream hook을 받기

Stream chunk는 그 자체로 session truth가 아니다. 최종 session visible state는 `LlmResponse`를 기반으로 `AgentRunner`가 message 후보를 만들고, `AgentLoop`가 session에 반영할 때 결정된다.

현재 streaming 구현은 provider별로 다르다. OpenAI compatible, Codex, Azure OpenAI는 responses 또는 chat SSE 이벤트를 파싱한다. Anthropic은 text, thinking, tool use, finish 이벤트를 공통 event와 `LlmResponse`로 모은다. streaming transport가 없는 경로는 단일 응답 fallback을 쓸 수 있다.

## tool call 규칙

Provider가 tool call을 반환하면 그것은 `ToolCallRequest` 후보다. `AgentRunner`는 이 후보를 `RuntimeToolCall`로 변환하고 runtime tool executor를 호출한다. tool 결과는 다시 messages에 추가되고 다음 model iteration의 입력이 된다.

현재 구현에는 별도 `ModelInvocationToolRequested` 재진입 command가 없다. tool call 후보가 `AgentRunner` 내부 loop에서 처리된다는 점이 현재 구조의 핵심이다.

불변식은 다음과 같다.

1. provider client는 tool runtime을 직접 호출하지 않는다.
2. provider parser는 tool call 후보를 실행 완료 event로 기록하지 않는다.
3. tool call 실행과 session 반영은 runtime loop와 `AgentLoop` 경계를 통과해야 한다.

## retry 규칙

Retry 정책은 `crates/shacs-providers/src/retry.rs`에 있다.

`Standard` 모드는 제한된 backoff를 사용한다. `Persistent` 모드는 더 오래 재시도할 수 있으며 동일 transient error 반복 제한을 둔다. retry 판단은 retryable provider error, `finish_reason == "error"` 응답, retry metadata, image fallback 가능성 등을 고려한다.

`AgentRunner`는 retry 대기를 직접 결정하지 않는다. 대신 `ProviderRetryWaiter`를 `retry_wait_callback`과 연결해 사용자가 실행 중 retry wait를 관찰할 수 있게 한다.

## cancellation, timeout, late result

현재 구현은 cancellation token check를 갖고 있다. `AgentRunner`는 provider 호출 전후, tool 실행 전후에 cancellation을 확인하고 취소 결과를 `AgentRunResult`로 만든다. 이미 진행 중인 HTTP 호출 자체의 강제 abort는 provider transport 수준의 보장으로 일반화돼 있지 않다.

Provider clients는 HTTP timeout 기본값을 갖는다. 예를 들어 OpenAI compatible과 Anthropic 계열은 120초 기본 timeout을, Codex transport는 60초 기본 timeout을 사용한다. 하지만 `AgentRunSpec` 또는 `ProviderRequest`에 per effect `timeout_ms`가 있는 구조는 아니다.

Late result correlation은 current architecture closure의 blocker가 아니라 별도 owner 작업이다. per effect `effect_id`, `correlation_id`를 붙여 async worker가 늦게 온 결과를 stale로 분류하는 framework가 없다. 현재 runner는 동기 호출 루프를 기준으로 동작하므로, 미래형 async provider worker late result 처리와 같은 모델은 gap으로 남는다.

## 현재 정상 흐름

### 최종 assistant 응답

```text
1) AgentLoop가 inbound message와 session history로 initial messages를 만든다.
2) AgentLoop가 AgentRunSpec을 구성하고 AgentRunner를 호출한다.
3) AgentRunner가 ProviderRequest를 만들고 ProviderClient를 호출한다.
4) provider client가 LlmResponse를 반환한다.
5) AgentRunner가 final assistant message 후보를 AgentRunResult.messages에 담는다.
6) AgentLoop가 새 messages를 session에 append하고 outbound 응답을 publish한다.
```

### tool call roundtrip

```text
1) provider client가 ToolCallRequest 후보가 포함된 LlmResponse를 반환한다.
2) AgentRunner가 ToolCallRequest를 RuntimeToolCall로 변환한다.
3) AgentRunner가 runtime tool executor를 호출한다.
4) tool 결과 message가 다음 model iteration의 입력에 추가된다.
5) 최종 assistant 응답이 나오면 AgentLoop가 session visible state로 반영한다.
```

## 불변식

1. provider code는 세션 상태를 직접 변경하지 않는다.
2. provider raw 응답과 stream chunk는 session truth가 아니다.
3. `ToolCallRequest`는 실행 후보이며, 실행 권한은 runtime loop에 있다.
4. `ProviderEvent`는 진행 상황 관찰값이며, 최종 assistant message가 아니다.
5. retry wait는 provider retry 정책에서 결정되고 runtime callback은 이를 관찰한다.
6. cancellation과 timeout은 현재 구현 범위와 gap을 구분해서 설명해야 한다.
7. 문서가 현재 존재하지 않는 shared invocation, outcome, reentry command 타입을 구현 완료처럼 말하면 안 된다.

## 남은 gap과 future work

다음 항목은 2026-05-13 완료 판정에서 accepted gap으로 남긴다. 현재 구현 완료로 주장하지 않지만, Spec 003 closure를 막는 blocker도 아니다.

* `InvokeModelEffect`
* `ProviderInvocationOutcome` 또는 `ModelInvocationOutcome`
* `ModelInvocationCompleted`, `ModelInvocationToolRequested` 같은 provider reentry command
* shared `effect_id` 또는 `correlation_id` 기반 late result correlation
* `ProviderStreamBuffer`
* `ProviderCallOutput::Final`
* `TurnCompleted` evidence

후속 targeted work는 다음이다.

1. 현재 `AgentRunner` 중심 구조를 유지할지, formal provider invocation envelope를 도입할지 결정한다.
2. per effect timeout과 cancellation abort 의미를 코드 수준 계약으로 정한다.
3. async provider worker가 필요해질 경우 `effect_id` 또는 `correlation_id` 기반 stale result 처리 규칙을 설계한다.
4. `finish_reason` 문자열을 계속 둘지, 닫힌 enum으로 승격할지 결정한다.
5. 별도 `ModelInvocation*` reentry command가 필요한지, 현재 runtime loop boundary로 충분한지 검토한다.

위 gap은 별도 owner 작업으로 넘긴다. Spec 003은 current `AgentRunner`/`AgentLoop`와 `shacs-providers` architecture 기준으로 완료 상태다.
