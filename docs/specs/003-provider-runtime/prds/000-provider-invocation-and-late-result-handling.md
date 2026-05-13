# PRD 000. provider invocation and late result handling

## 목표

이 PRD는 Spec 003을 현재 provider runtime 구조에 맞게 정렬하기 위한 실행 문서다. 목표는 없는 타입을 구현 완료처럼 적는 것이 아니라, 현재 `shacs-providers`와 `AgentRunner` 기반 구조에서 이미 동작하는 부분과 아직 남은 late result gap을 분리하는 것이다.

핵심 원칙은 변하지 않는다. provider output은 session visible state가 아니라 후보 데이터다. 세션에 보이는 결과는 runtime 또는 orchestrator 경계가 결정해야 하며, provider code는 session을 직접 수정하면 안 된다.

## SPEC 입력

* 주관 spec: `docs/specs/003-provider-runtime/SPEC.md`
* 교차 의존: `docs/specs/001-session-kernel/SPEC.md`
* 교차 의존: `docs/specs/002-command-event-effect/SPEC.md`
* 교차 의존: `docs/specs/004-tool-runtime/SPEC.md`
* 교차 의존: `docs/specs/007-main-orchestrator-policy/SPEC.md`

## Dependency Cut

이 PRD는 provider invocation과 late result gap 정리에 집중한다. tool runtime 자체, session store 구현, provider auth onboarding, CLI UX는 직접 범위가 아니다. 다만 현재 구현을 설명하기 위해 `AgentRunner`, `AgentLoop`, provider clients, provider registry는 범위 안에 둔다.

## 현재 구현 상태

### 완료 판정

2026-05-13 기준 이 PRD와 Spec 003은 완료로 닫는다. 완료의 의미는 formal `InvokeModelEffect`, `ProviderInvocationOutcome`, `ModelInvocation*` reentry command, per effect `effect_id`/`correlation_id`, async worker late result framework를 구현했다는 뜻이 아니다.

완료의 의미는 현재 `ProviderClient`/`ProviderRequest`/`ProviderEvent`/`LlmResponse`/`ToolCallRequest`와 `AgentRunner`/`AgentLoop` 경계가 003의 current architecture 기준을 충족한다고 확정했다는 뜻이다.

### 이미 반영된 것

* `crates/shacs-providers/src/provider.rs`의 `ProviderClient`, `ProviderRequest`, `ProviderEvent`가 provider 호출 공통 표면이다.
* `crates/shacs-providers/src/types.rs`의 `GenerationSettings`, `ToolCallRequest`, `LlmResponse`가 request 설정과 결과 후보를 표현한다.
* `finish_reason`은 닫힌 enum이 아니라 문자열이다.
* `crates/shacs-providers/src/retry.rs`의 `ProviderRetryMode`, retry decision, `ProviderRetryWaiter`가 retry와 wait를 담당한다.
* `crates/shacs-providers/src/clients/mod.rs`는 provider resolution, config validation, `prepare_provider_request`를 제공한다.
* `crates/shacs-providers/src/registry.rs`는 `ProviderRegistry`, `ProviderSpec`, model/provider matching 규칙을 제공한다.
* OpenAI compatible, Anthropic, Codex, Azure OpenAI clients는 request builder, response parser, 가능한 streaming parser를 통해 `LlmResponse`와 `ProviderEvent`를 만든다.
* `crates/shacs-core/src/runtime/runner.rs`의 `AgentRunSpec`과 `AgentRunner`가 provider request 구성, streaming callback dispatch, retry integration, cancellation check, tool loop handoff, checkpoint callback을 처리한다.
* `crates/shacs-core/src/runtime/agent_loop.rs`가 `AgentRunResult`를 session visible state로 반영하는 orchestration boundary다.
* `crates/shacs-api/src/lib.rs`와 `crates/shacs-cli/src/lib.rs`는 provider runtime의 정의가 아니라 API와 CLI surface adapter로 소비한다.

### 후속 비목표 / 별도 owner로 넘길 것

* 공유 `InvokeModelEffect` 타입은 현재 없다. 이것은 003 완료 blocker가 아니다.
* 공유 `ProviderInvocationOutcome` 또는 `ModelInvocationOutcome` 타입은 현재 없다.
* `ModelInvocationCompleted`, `ModelInvocationToolRequested`, `ModelInvocationFailed`, `ModelInvocationTimedOut`, `ModelInvocationCancelled` reentry command는 현재 없다.
* provider 호출마다 공유 `effect_id` 또는 `correlation_id`를 붙여 late result를 식별하는 framework는 현재 없다.
* async provider worker late result correlation과 stale discard는 별도 owner가 외부 async provider worker 필요성을 먼저 확정한 뒤 설계한다.
* `ProviderStreamBuffer`, `ProviderCallOutput::Final`, `TurnCompleted` evidence는 현재 코드의 근거 이름이 아니다.

### 로컬 근거

* `crates/shacs-providers/src/provider.rs`
* `crates/shacs-providers/src/types.rs`
* `crates/shacs-providers/src/retry.rs`
* `crates/shacs-providers/src/clients/mod.rs`
* `crates/shacs-providers/src/registry.rs`
* `crates/shacs-core/src/runtime/runner.rs`
* `crates/shacs-core/src/runtime/agent_loop.rs`

## 요구사항 정렬

### Invocation mapping

현재 `InvokeModel`에 해당하는 실행 단위는 formal effect enum이 아니라 `AgentRunSpec`과 iteration별 `ProviderRequest`다.

Acceptance:

* 문서는 `Effect::InvokeModel` 구현을 완료 조건처럼 요구하지 않는다.
* `ProviderRequest`의 실제 필드인 `messages`, `tools`, `model`, `settings`, `tool_choice`를 기준으로 설명한다.
* provider 선택은 `ProviderRegistry`, `resolve_provider_client`, `provider_client_from_config`를 기준으로 설명한다.
* `prepare_provider_request`는 API, CLI, adapter surface에서 request를 구성하는 helper로 설명한다.

### Result normalization

현재 정규화 결과는 `LlmResponse`다.

Acceptance:

* assistant text 후보는 `LlmResponse.content`로 설명한다.
* tool call 후보는 `LlmResponse.tool_calls`와 `ToolCallRequest`로 설명한다.
* usage, reasoning, thinking, error metadata는 `LlmResponse`의 field로 설명한다.
* `finish_reason`은 문자열이라고 명시한다.
* closed stop reason enum을 현재 구현으로 주장하지 않는다.

### Streaming

Streaming은 provider parser와 client가 `ProviderEvent`를 emit하고, `AgentRunner`가 callback과 hook으로 전달하는 관찰 경로다.

Acceptance:

* `ProviderEvent`는 progress observation으로 설명한다.
* stream chunk를 session truth로 설명하지 않는다.
* 최종 session visible state는 `LlmResponse`, `AgentRunResult`, `AgentLoop` 반영 경계를 통해 결정된다고 설명한다.
* provider별 SSE parser는 현재 테스트 이름과 연결해 설명한다.

### Tool call loop

Provider가 반환한 `ToolCallRequest`는 tool execution candidate다. 현재 `AgentRunner`가 이를 `RuntimeToolCall`로 바꾸고 runtime tool executor를 호출한다.

Acceptance:

* 별도 `ModelInvocationToolRequested` command가 있다고 쓰지 않는다.
* provider client가 tool runtime을 직접 호출한다고 쓰지 않는다.
* tool call 후보가 runtime loop 안에서 처리된다고 설명한다.
* session visible result는 `AgentLoop` 반영 경계를 통과한다고 설명한다.

### Retry, cancellation, timeout, late result

Retry는 구현되어 있다. Cancellation check와 HTTP timeout default도 구현되어 있다. Late result correlation은 current architecture의 blocker가 아니라 후속 owner 작업이다.

Acceptance:

* retry policy는 `crates/shacs-providers/src/retry.rs`에 있다고 설명한다.
* runtime retry wait observation은 `retry_wait_callback`으로 설명한다.
* cancellation은 `AgentRunSpec.cancellation_token` check로 설명한다.
* provider client HTTP timeout default는 언급하되, per effect `timeout_ms`로 일반화하지 않는다.
* async worker late result handling과 `effect_id` stale check는 future work로 남긴다.

## TDD 계획 결과

1. Provider invocation mapping은 `AgentRunSpec`, `ProviderRequest`, `ProviderClient` 근거로 확인한다.
2. Result normalization은 `LlmResponse`, `ToolCallRequest`, string `finish_reason` 근거로 확인한다.
3. Streaming은 `ProviderEvent`, provider별 parser 테스트, `AgentRunner` callback 경로로 확인한다.
4. Tool call loop는 `AgentRunner`가 tool call 후보를 runtime tool execution으로 넘기는 테스트로 확인한다.
5. Retry와 wait observation은 provider retry tests와 `runtime_runner_retry_wait_callback_observes_provider_retry`로 확인한다.
6. Provider stream delta와 provider error가 session visible state를 직접 오염시키지 않는 경계는 `runtime_loop` 집중 테스트로 확인한다.
7. Formal `InvokeModelEffect`, `ProviderInvocationOutcome`, `ModelInvocation*` reentry command, per effect `effect_id`/`correlation_id`는 현재 목표가 아니므로 테스트 계획에 넣지 않는다.

결과: 완료. 003은 새 formal provider runtime rewrite가 아니라 current architecture mapping과 accepted gap 문서화로 닫는다.

## 구현 웨이브 결과

### Wave 1. 문서 정렬

* Spec 003을 current architecture spec으로 재작성한다.
* `Effect::InvokeModel` 중심 문장을 `ProviderRequest`, `AgentRunSpec`, `ProviderClient` 매핑으로 바꾼다.
* Command, Event, Effect 용어를 Spec 002와 같은 conceptual vocabulary로 맞춘다.

결과: 완료. SPEC와 PRD가 2026-05-13 current provider runtime architecture 기준으로 정렬됐다.

### Wave 2. 구현 근거 연결

* provider core files와 runtime files를 명시한다.
* response normalization을 `LlmResponse`, `ToolCallRequest`, string `finish_reason` 중심으로 고친다.
* streaming 설명을 `ProviderEvent`, callback, hook, final `LlmResponse` 중심으로 고친다.
* retry 설명을 `ProviderRetryMode`, retry decision, wait behavior 중심으로 고친다.

결과: 완료. current code의 concrete 타입과 테스트 evidence가 문서에 연결됐다.

### Wave 3. accepted gap과 후속 owner 분리

* `InvokeModelEffect`와 `ProviderInvocationOutcome` 계열을 current implementation claim에서 제거한다.
* `ModelInvocation*` reentry command를 후속 owner 작업으로 남긴다.
* `effect_id`, `correlation_id` late result framework를 후속 owner 작업으로 남긴다.
* `ProviderStreamBuffer`, `ProviderCallOutput::Final`, `TurnCompleted` claim을 제거한다.

결과: 완료. accepted gap은 blocker가 아니라 future owner work로 분리됐다.

## Verification Evidence

현재 PRD는 코드 변경 PRD가 아니라 문서 정렬 PRD다. verification evidence는 문서가 참조할 수 있는 실제 구현 근거와 테스트 이름으로 제한한다.

Runtime runner evidence:

* `runtime_runner_executes_tool_loop_and_accumulates_usage`
* `runtime_runner_retry_wait_callback_observes_provider_retry`
* `runtime_runner_isolates_callback_panics`
* `runtime_runner_checkpoint_uses_normalized_tool_results`

Provider runtime loop evidence:

* `loop_does_not_persist_provider_stream_delta_as_session_content`
* `loop_provider_error_publishes_error_and_clears_runtime_markers`
* `cargo test --manifest-path crates/shacs-core/Cargo.toml --test runtime_loop loop_does_not_persist_provider_stream_delta_as_session_content --locked`
* `cargo test --manifest-path crates/shacs-core/Cargo.toml --test runtime_loop loop_provider_error_publishes_error_and_clears_runtime_markers --locked`

Provider streaming evidence:

* `azure_openai_streaming_uses_responses_sse_events`
* `codex_stream_parser_maps_responses_events`
* `codex_stream_parser_preserves_raw_malformed_tool_arguments`
* `anthropic_stream_parser_maps_text_thinking_tools_and_finish`
* `openai_client_chat_stream_uses_native_sse_when_transport_supports_it`
* `openai_client_stream_falls_back_to_single_text_delta`

Retry evidence:

* retry behavior tests in `crates/shacs-providers/tests/retry.rs`
* Standard retry and Persistent retry coverage in the provider retry test area
* retry wait behavior observed through `ProviderRetryWaiter` and `retry_wait_callback`

## Residual Risks

이번 완료 판정은 다음 gap을 의도적으로 받아들인다.

1. `finish_reason`이 문자열이라 provider별 의미가 느슨하게 유지된다.
2. Streaming callback이 관찰 경로라는 점을 문서와 adapter가 계속 지켜야 한다.
3. 현재 tool call loop는 `AgentRunner` 내부에서 처리되므로, formal reentry command를 도입할 경우 책임 경계를 다시 나눠야 한다.
4. async provider worker를 도입하면 late result correlation과 stale result discard 규칙을 새로 설계해야 한다.
5. 공유 `InvokeModelEffect`, `ProviderInvocationOutcome`, `ModelInvocation*` reentry command, per effect `effect_id`/`correlation_id`는 현재 구현된 타입이나 framework가 아니다.

## 종료 기준

1. Spec 003이 current provider runtime architecture를 정확히 설명한다.
2. PRD가 없는 타입과 없는 테스트를 구현 완료처럼 주장하지 않는다.
3. Current architecture 기준으로 `ProviderClient`/`ProviderRequest`/`ProviderEvent`/`LlmResponse`/`ToolCallRequest`와 `AgentRunner`/`AgentLoop` 경계가 provider runtime 완료 기준을 충족한다고 명시한다.
4. Accepted gap은 blocker가 아니라 future owner work로 남긴다.
5. Verification evidence가 기존 실제 evidence 이름과 두 runtime loop test command를 포함한다.

위 기준은 2026-05-13 current architecture 기준으로 충족된 것으로 판정한다. 이 PRD와 Spec 003은 완료 상태이며, 이후 변경은 새 003 wave가 아니라 관련 owner spec의 좁은 보강 PRD로 추가한다.
