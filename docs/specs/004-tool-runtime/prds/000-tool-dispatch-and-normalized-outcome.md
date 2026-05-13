# PRD 000. tool dispatch and normalized outcome

## 목표

이 문서는 `docs/specs/004-tool-runtime/SPEC.md`의 하위 실행 문서다. 목표는 현재 구현된 tool dispatch 경계를 `AgentRunner`, `RuntimeToolExecutor`, `ToolRegistry`, `RuntimeToolCall`, `ToolResult`, `RuntimeToolMessage`, `RuntimeToolExecutionReport`, `ToolEvent`, `checkpoint_callback` 기준으로 고정하는 것이다.

- provider가 요청한 tool call이 `RuntimeToolCall { id, name, arguments }`로 변환되어 registry-backed executor로 실행된다.
- tool 결과는 현재 `ToolResult`에서 `RuntimeToolMessage` 또는 interrupt로 정리된다.
- checkpoint와 `ToolEvent`는 진행을 관측 가능하게 만든다.
- formal `RunToolEffect`, permission snapshot guard, 공통 outcome envelope, explicit timeout/cancelled outcome, full late-result handling은 후속 owner gap으로 남긴다.

## SPEC 입력

- 주관 spec: `docs/specs/004-tool-runtime/SPEC.md`
- 교차 의존:
  - `docs/specs/001-session-kernel/SPEC.md`
  - `docs/specs/002-command-event-effect/SPEC.md`
  - `docs/specs/003-provider-runtime/SPEC.md`
  - `docs/specs/007-main-orchestrator-policy/SPEC.md`
  - `docs/specs/008-configuration-profiles-and-runtime-layout/SPEC.md`

## Dependency Cut

이 PRD는 현재 tool runtime 경계와 result/message 변환에 집중한다. 개별 tool 알고리즘, sandbox 강화, host safety taxonomy의 전체 문서는 별도 범위다. 여기서는 formal effect/reentry 모델을 완성했다고 주장하지 않는다.

## 범위

- provider `ToolCallRequest`에서 `RuntimeToolCall`로 넘어가는 dispatch entrypoint
- `ToolRegistry` 조회와 parameter validation
- `RuntimeToolExecutor` 실행과 `RuntimeToolExecutionReport` 반환
- `ToolResult::Text`, `ToolResult::Json`, `ToolResult::AskUserInterrupt` 처리
- `RuntimeToolMessage` 생성과 provider 재호출 context 연결
- `ToolEvent`와 checkpoint 기반 진행 관측
- 후속 owner로 남길 normalized outcome gap 정리

## 범위 제외

- 개별 tool 구현 세부 알고리즘
- 원격 plugin marketplace
- shell sandbox의 완전한 보안 설계
- 멀티세션 리소스 스케줄링
- formal `RunToolEffect`와 reentry command 구현

## 현재 구현 상태

### 완료 판정

2026-05-13 기준 Spec 004는 current architecture 기준으로 완료로 닫는다. 완료의 의미는 formal `Effect::RunTool`, `RunToolEffect`, `ToolOutcomeRecorded`, `ToolCallCompleted`/`ToolCallFailed`/`ToolCallTimedOut`/`ToolCallCancelled` 재진입 command, permission snapshot guard, full late-result handling을 구현했다는 뜻이 아니다.

완료의 의미는 현재 코드의 `AgentRunner`/`RuntimeToolExecutor`/`ToolRegistry`/`RuntimeToolCall`/`ToolResult`/`RuntimeToolMessage`/`RuntimeToolExecutionReport`/`ToolEvent`/`checkpoint_callback` 표면이 tool runtime의 현재 경계로 문서화됐고, 기존 테스트 증거가 그 범위 안에서 유지된다는 뜻이다.

### 이미 반영된 것

- provider tool 요청은 `AgentRunner` 안에서 `RuntimeToolCall { id, name, arguments }`로 변환된다.
- `RuntimeToolExecutor`는 registry-backed call을 실행하고 `RuntimeToolExecutionReport { messages, interrupt, skipped_tool_calls }`를 반환한다.
- `ToolRegistry`는 tool lookup과 parameter validation을 수행하고, unknown tool 또는 invalid arguments를 provider-visible error text로 정규화한다.
- `ToolResult`는 현재 `Text(String)`, `Json(Value)`, `AskUserInterrupt { question, options }`를 표현한다.
- `RuntimeToolMessage`는 provider tool message인 `{ role: "tool", tool_call_id, name, content }` 형태로 돌아간다.
- `AgentRunner`는 `awaiting_tools`, `tools_completed`, `final_response` checkpoint를 남기고, `ToolEvent`를 `Ok`, `Error`, `Waiting`, `Skipped` 상태로 emit한다.
- runner boundary에는 cancellation token support가 있다. 단, 이것은 formal cancelled outcome reentry가 아니다.
- 큰 text tool result는 `maybe_persist_text_tool_result`와 `max_tool_result_chars` 경계에서 저장 또는 truncation 처리를 받을 수 있다. 단, full `artifact_ref` outcome envelope는 아니다.

### 후속 비목표 / 별도 owner로 넘길 것

- formal `RunToolEffect`, effect id, correlation id 도입은 후속 owner 작업이다.
- `permission_snapshot`을 execution envelope에 넣고 runtime에서 다시 대조하는 guard는 현재 구현에 없다.
- 모든 tool 결과를 감싸는 공통 normalized outcome envelope는 현재 구현에 없다.
- `timed_out`과 `cancelled`를 별도 outcome state로 기록하는 모델은 현재 구현에 없다.
- timeout 또는 취소 후 뒤늦게 온 완료 신호를 채택, 폐기, 관찰 event로 나누는 late-result adoption rule은 현재 구현에 없다.
- shell write, 원격 network tool, secret read 같은 추가 tool family는 현재 기본 runtime path가 아니다.
- proc 실행 sandbox와 out-of-band process cancellation은 self-hosted 최소 범위 밖의 별도 확장 범위다.

### 로컬 근거

- `crates/shacs-core/src/runtime/tool_execution.rs`
- `crates/shacs-core/src/runtime/runner.rs`
- `crates/shacs-core/src/tools/base.rs`
- `crates/shacs-core/src/tools/registry.rs`
- `crates/shacs-core/tests/runtime.rs`
- `crates/shacs-core/tests/runtime_agent.rs`
- `crates/shacs-core/tests/runtime_loop.rs`
- `crates/shacs-core/tests/tools.rs`

## TDD 계획

현재 완료 범위에서 유지할 테스트 관점은 다음과 같다.

1. registry에 없는 tool 실행 요청이 panic이 아니라 provider-visible error text로 반환되는지 확인한다.
2. invalid arguments가 executor panic이 아니라 normalized error content로 반환되는지 확인한다.
3. `ToolResult::Text`와 `ToolResult::Json`이 `RuntimeToolMessage`로 변환되는지 확인한다.
4. `ToolResult::AskUserInterrupt`가 일반 tool message가 아니라 interrupt로 올라가는지 확인한다.
5. tool 실행 중 `awaiting_tools`와 `tools_completed` checkpoint가 남는지 확인한다.
6. 큰 text result가 configured limit 아래에서 truncation 또는 persistence 경계를 지키는지 확인한다.

후속 owner 테스트로 남길 항목은 다음과 같다.

1. permission snapshot과 실제 요청이 모순될 때 실행이 거부되는 테스트.
2. 공통 outcome envelope가 text, JSON, artifact reference, error를 같은 shape로 담는 테스트.
3. timeout과 cancellation이 별도 outcome state로 반환되는 테스트.
4. 이미 닫힌 턴에 대한 tool 결과가 formal late result rule에 따라 처리되는 테스트.
5. effect id와 correlation id가 duplicate result를 idempotent하게 막는 테스트.

## 구현 웨이브

### Wave 1. 현재 registry와 runtime call 경계 고정

- `RuntimeToolCall { id, name, arguments }`를 현재 tool call envelope로 문서화한다.
- `ToolRegistry` 조회와 parameter validation 실패가 provider-visible error text로 이어지는 경계를 유지한다.
- registry가 세션 상태 권한을 갖지 않는다는 점을 테스트와 문서 근거로 남긴다.

### Wave 2. executor report와 provider message 경계 고정

- `RuntimeToolExecutor`가 `RuntimeToolExecutionReport { messages, interrupt, skipped_tool_calls }`를 반환하는 구조를 유지한다.
- `ToolResult::Text`와 `ToolResult::Json`은 `RuntimeToolMessage` content로 변환한다.
- `ToolResult::AskUserInterrupt`는 `RuntimeInterrupt` 경계로 올리고 일반 tool text로 숨기지 않는다.

### Wave 3. observability와 checkpoint 경계 고정

- `AgentRunner`가 provider tool request를 실행하기 전후로 checkpoint를 남기는 경계를 유지한다.
- `ToolEvent`의 `Ok`, `Error`, `Waiting`, `Skipped` 상태가 진행 관측을 설명하도록 둔다.
- fatal tool error와 skipped tool call이 세션 상태 직접 patch로 흐르지 않게 한다.

### Wave 4. 후속 outcome owner에게 넘길 gap 명시

- formal `RunToolEffect`, effect id, correlation id 설계는 현재 runtime loop를 전면 대체하지 않는 별도 작업으로 둔다.
- permission snapshot guard는 current completion 조건이 아니라 host safety와 policy owner가 함께 다룰 작업으로 둔다.
- common normalized outcome envelope, explicit `timed_out`/`cancelled` states, late-result adoption rule은 후속 PRD에서 다룬다.

## Verification Evidence

- 통합 증거: `crates/shacs-core/tests/runtime.rs`, `crates/shacs-core/tests/runtime_agent.rs`, `crates/shacs-core/tests/runtime_loop.rs`는 현재 runner 구조 안에서 tool call 실행, result message mapping, ask-user interrupt, skipped later tools, checkpoint 기반 tool progress, throttled result handling, provider/tool progress forwarding을 다룬다.
- 안전성 증거: `crates/shacs-core/tests/tools.rs`는 registry validation, filesystem/exec path 제한, SSRF allowlist, symlink escape rejection, output truncation, 민감한 self-tool path 차단을 다룬다.
- 내구성 증거: `crates/shacs-core/tests/runtime.rs`, `crates/shacs-core/tests/runtime_agent.rs`, `crates/shacs-core/tests/runtime_loop.rs`는 tool 실행 중 checkpoint persistence, runtime checkpoint materialization, session recovery context 경계를 다룬다.

이 증거는 현재 runtime boundary를 뒷받침한다. `ToolOutcomeRecorded`, `ToolCallTimedOut`, `ToolCallCancelled`, permission snapshot guard, full late-result adoption을 증명하는 근거로 쓰면 안 된다.

## Open Risks

- registry 메타데이터와 실제 executor 능력이 어긋나면 permission 의미가 흔들릴 수 있다.
- 공통 outcome envelope가 없는 동안 tool별 디버깅 정보와 provider-visible content shape의 경계가 좁게 유지된다.
- runtime root와 artifact 참조 규칙이 약하면 외부 경로 노출이 섞일 수 있다.
- OS별 강한 sandbox와 out-of-band process cancellation은 아직 product 최소 범위 밖이므로, 이를 요구하는 새 tool family를 추가할 때 별도 spec update가 필요하다.

## 종료 기준

- provider tool call이 `RuntimeToolCall`로 변환되고 registry-backed executor를 통해 실행된다.
- registry lookup과 parameter validation 실패가 provider-visible error text로 반환된다.
- `ToolResult`가 `RuntimeToolMessage`, interrupt, skipped call report로 정리된다.
- `AgentRunner`가 `ToolEvent`와 checkpoint를 통해 tool 진행을 관측 가능하게 남긴다.
- 후속 owner gap인 formal `RunToolEffect`, effect id, correlation id, permission snapshot guard, common normalized outcome envelope, explicit timeout/cancelled outcome, late-result adoption rule을 현재 구현으로 주장하지 않는다.
