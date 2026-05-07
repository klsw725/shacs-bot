# PRD 000. provider invocation and late result handling

## 목표

이 문서는 `docs/specs/003-provider-runtime/SPEC.md`의 하위 실행 문서다. 목표는 `Effect::InvokeModel` 실행 계약, provider 결과 정규화, timeout과 cancellation 이후의 late result 처리까지 포함한 provider runtime 구현 계획을 고정하는 것이다.

- provider 호출을 오케스트레이터 바깥 effect executor로 분리한다.
- provider 결과를 최종 상태가 아닌 후보 결과 envelope로 정규화한다.
- timeout, cancellation, retry 이후 늦게 도착한 provider 결과가 공식 응답으로 승격되지 못하게 한다.

## SPEC 입력

- 주관 spec: `docs/specs/003-provider-runtime/SPEC.md`
- 교차 의존:
  - `docs/specs/001-session-kernel/SPEC.md`
  - `docs/specs/002-command-event-effect/SPEC.md`
  - `docs/specs/004-tool-runtime/SPEC.md`
  - `docs/specs/007-main-orchestrator-policy/SPEC.md`

## Dependency Cut

이 PRD는 provider 실행 경계와 결과 정규화에 집중한다. tool 실행기 구현, 세션 영속화, config discovery는 별도 PRD의 범위다. 여기서는 provider 요청 envelope와 재진입 command까지를 완성 범위로 본다.

## 범위

- `InvokeModel` effect와 provider adapter trait 정의
- 요청 envelope 필수 필드와 검증 규칙
- streaming 내부 처리와 최종 결과 정규화
- assistant 후보와 tool call 후보 구분
- timeout, cancellation, failure, late result 처리
- provider 결과의 재진입 command 변환

## 범위 제외

- 특정 벤더 API의 전체 옵션 노출
- OpenAI-compatible, Anthropic auth, Codex auth(OpenAI auth style) 바깥의 추가 provider/auth family 구현
- 비용 집계 및 과금 리포팅
- UI에서 streaming chunk를 보여주는 방식
- tool runtime 자체 구현

## 현재 구현 상태

### 이미 반영된 것

- provider client/adapter 타입은 `crates/shacs-providers/src/`에 있고, runtime provider invocation은 `crates/shacs-core/src/runtime/runner.rs`와 `agent_loop.rs` 경계에서 처리된다.
- provider 성공을 assistant output과 tool call 후보로 분기하고, 실패/timeout/cancelled를 runtime progress/error 경로로 다루는 처리가 구현돼 있다.
- provider 성공은 유효한 outcome을 반드시 가져야 하고, retry 후 old effect 결과는 late result로 폐기되는 테스트가 있다.
- OpenAI-compatible 및 Codex auth adapter는 SSE-style `data:` streaming chunk를 `ProviderStreamBuffer`에만 모은 뒤 `ProviderCallOutput::Final` outcome으로 정규화하며, `delta.tool_calls[]` fragment를 `index` 기준으로 병합해 단일 tool 후보로 만든다.
- Anthropic auth adapter는 final JSON 응답과 SSE `data:` streaming 응답을 모두 공통 provider outcome으로 정규화하며, text delta와 `tool_use`/`input_json_delta` fragment를 최종 후보 결과로만 승격한다.
- surface 통합 테스트는 buffered streaming provider의 partial chunk가 store event로 기록되지 않고 최종 `TurnCompleted` output만 남는 경계를 검증한다.

### 로컬 근거

- `crates/shacs-providers/src/provider.rs`
- `crates/shacs-providers/src/types.rs`
- `crates/shacs-providers/src/clients/`
- `crates/shacs-core/src/runtime/runner.rs`
- `crates/shacs-core/src/runtime/agent_loop.rs`
- `crates/shacs-core/tests/runtime_agent.rs`
- `crates/shacs-providers/tests/providers.rs`

## TDD 계획

1. 유효한 `InvokeModel` envelope만 실행되는 테스트를 작성한다.
2. streaming chunk가 직접 공식 상태가 되지 않고 최종 envelope로 모이는 테스트를 작성한다.
3. assistant 후보 결과와 tool call 후보 결과가 서로 다른 reentry command로 정규화되는 테스트를 작성한다.
4. timeout 이후 늦게 도착한 결과가 late result로 분류되는 테스트를 작성한다.
5. 취소된 호출 결과가 닫힌 턴을 되살리지 못하는 테스트를 작성한다.

## 구현 웨이브

### Wave 1. 호출 계약과 adapter 표면

- `InvokeModelEffect`와 `ProviderInvocationOutcome` 타입을 분리한다.
- provider adapter trait과 envelope validator를 정의한다.
- `session_id`, `turn_id`, `effect_id`, `correlation_id`의 재부착 규칙을 고정한다.
- OpenCode의 provider/auth 구조를 참고해 OpenAI-compatible, Anthropic auth, Codex auth(OpenAI auth style) 세 family만 adapter 범위로 고정한다.

### Wave 2. 결과 정규화와 stop reason 매핑

- 벤더 raw 응답을 공통 `status`, `stop_reason`, `usage`, 후보 본문으로 변환한다.
- assistant 후보와 tool call 후보를 동시에 구분 가능한 결과 타입으로 만든다.
- streaming은 내부 버퍼와 최종 결과로만 관리하고 chunk를 공식 상태로 올리지 않는다.

### Wave 3. timeout, cancellation, failure, late result

- `timeout_ms` 기반 중단과 `timed_out` 결과 정규화를 구현한다.
- best-effort cancellation과 `cancelled` 결과 정규화를 구현한다.
- retry 후 이전 `effect_id` 결과가 도착하면 late result로 분류하는 검증을 추가한다.

### Wave 4. 재진입 연결과 정책 통합

- `ModelInvocationCompleted`, `ModelInvocationToolRequested`, `ModelInvocationFailed`, `ModelInvocationTimedOut`, `ModelInvocationCancelled` 재진입 command를 연결한다.
- 오케스트레이터가 승인 전까지 후보 결과를 세션 기록으로 승격할 수 없게 한다.
- tool 요청 후보가 자동 실행되지 않도록 통합 검증을 추가한다.

## Verification Evidence

- envelope validation 테스트
- stop reason 정규화 테스트
- assistant 후보와 tool 후보 재진입 테스트
- timeout 및 late result 무시 테스트
- 닫힌 턴에 대한 provider 결과 방어 테스트
- `adapter_normalizes_streaming_chunks_into_final_assistant_message`
- `openai_streaming_tool_call_fragments_merge_into_one_candidate`
- `openai_streaming_error_redacts_reflected_api_key`
- `openai_streaming_tool_call_with_malformed_arguments_fails`
- `openai_streaming_tool_finish_without_candidate_fails`
- `openai_streaming_tool_arguments_over_limit_fails`
- `codex_streaming_tool_call_fragments_use_openai_normalization`
- `codex_streaming_error_redacts_reflected_api_key`
- `anthropic_streaming_text_deltas_normalize_into_assistant_message`
- `anthropic_streaming_error_redacts_reflected_api_key`
- `anthropic_streaming_tool_use_fragments_normalize_into_one_candidate`
- `anthropic_streaming_tool_use_with_malformed_arguments_fails`
- `anthropic_streaming_tool_finish_without_candidate_fails`
- `anthropic_streaming_text_over_limit_fails`
- `anthropic_streaming_usage_metadata_over_limit_fails`
- `valid_old_provider_completed_after_retry_is_rejected_without_committing_output`
- `valid_old_provider_tool_request_after_retry_is_rejected_without_running_tool`
- `provider_max_tokens_assistant_candidate_completes_turn_with_candidate_output`
- `submit_user_input_persists_only_final_output_from_buffered_streaming_provider`

## Open Risks

- 벤더별 stop reason을 너무 일찍 일반화하면 디버깅 정보가 사라질 수 있다.
- streaming 내부 버퍼와 최종 결과의 경계가 흐리면 상태 전이 규칙이 깨질 수 있다.
- retry와 late result 분리가 약하면 오래된 결과가 현재 턴에 섞일 수 있다.
- 실제 in-flight HTTP request abort는 best-effort cancellation 범위로 남기며, 이 PRD의 완료 기준은 늦은 provider 결과 방어와 cancelled outcome 정규화다.

## 종료 기준

- provider runtime은 effect executor로만 동작한다.
- 모든 provider 결과는 정규화된 후보 결과와 재진입 command로 변환된다.
- timeout, cancellation, late result가 명시적으로 구분된다.
- `docs/specs/003-provider-runtime/SPEC.md`의 금지 패턴, 특히 자동 tool 실행 금지가 코드와 테스트로 보장된다.
