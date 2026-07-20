# tool runtime 아키텍처 명세

Status: Complete (Scoped)
Implemented scope: `AgentRunner`, `RuntimeToolExecutor`, `ToolRegistry`, `RuntimeToolCall`, `ToolResult`, `RuntimeToolMessage`, `RuntimeToolExecutionReport`, `ToolEvent`, checkpoint callback의 current tool runtime 경계를 닫았다.
Open work moved to: [028 formal execution reentry and outcome contracts](../028-formal-execution-reentry-and-outcome-contracts/SPEC.md), [030 policy, permission, redaction, and containment model](../030-policy-permission-redaction-and-containment-model/SPEC.md), [034 generated media and rich file context expansion](../034-generated-media-and-rich-file-context-expansion/SPEC.md)
Not carried forward: 개별 tool 내부 알고리즘, 원격 plugin marketplace, 모든 tool에 대한 완전한 host sandbox 보장을 004의 후속 owner 범위로 가져가지 않는다.

## 문서 목적

이 문서는 `docs/SYSTEM-FOUNDATION.md`, `docs/specs/001-session-kernel/SPEC.md`, `docs/specs/002-command-event-effect/SPEC.md`, `docs/specs/003-provider-runtime/SPEC.md`를 바탕으로 `shacs-bot`의 tool runtime 경계를 현재 구현 기준으로 고정한다.

목표는 다음과 같다.

- tool registry가 무엇을 보관하고 무엇을 보관하지 않는지 정의한다.
- provider tool 요청이 `AgentRunner`, `RuntimeToolExecutor`, `ToolRegistry`를 거쳐 provider tool message로 돌아가는 현재 경계를 설명한다.
- tool 실행 결과, interrupt, skipped call, checkpoint, observability event가 어디에서 만들어지는지 정리한다.
- formal `RunToolEffect`, effect id, correlation id, permission snapshot guard, 공통 outcome envelope, 명시적 timeout/cancelled 상태, late-result 채택 규칙은 후속 owner gap으로 남긴다.

이 문서는 방향 제안이 아니라 현재 구현을 판정하는 기준이다. 구현이 이 문서와 충돌하면 코드를 먼저 밀어붙이지 않고 문서 판단부터 다시 확인해야 한다.

이 spec의 완료 기준은 formal effect/reentry 모델을 완성했다는 뜻이 아니다. 완료의 의미는 현재 코드의 `AgentRunner` + `RuntimeToolExecutor` + `ToolRegistry` + `RuntimeToolCall` + `ToolResult` + `RuntimeToolMessage` + `RuntimeToolExecutionReport` + `ToolEvent` + `checkpoint_callback` 경계가 tool runtime current architecture 기준을 충족한다고 문서상 확정한다는 뜻이다.

---

## 현재 구현 상태

### 완료 판정

2026-05-13 기준 Spec 004는 current architecture 기준으로 완료로 닫는다. 완료의 의미는 formal `Effect::RunTool`, `RunToolEffect`, `ToolOutcomeRecorded`, `ToolCallCompleted`/`ToolCallFailed`/`ToolCallTimedOut`/`ToolCallCancelled` 재진입 command, permission snapshot guard, full late-result handling을 구현했다는 뜻이 아니다.

완료의 의미는 현재 코드의 `AgentRunner`, `RuntimeToolExecutor`, `ToolRegistry`, `RuntimeToolCall`, `ToolResult`, `RuntimeToolMessage`, `RuntimeToolExecutionReport`, `ToolEvent`, `checkpoint_callback` 표면이 tool runtime의 현재 경계로 문서화됐고, 기존 테스트 증거가 그 범위 안에서 유지된다는 뜻이다.

### 이미 반영된 것

- provider tool 요청은 `AgentRunner` 안에서 `RuntimeToolCall { id, name, arguments }`로 변환된다.
- `RuntimeToolExecutor`는 `ToolRegistry`를 거쳐 registry-backed call을 실행하고 `RuntimeToolExecutionReport { messages, interrupt, skipped_tool_calls }`를 반환한다.
- `ToolResult`는 `RuntimeToolMessage`, ask-user interrupt, skipped call report의 입력으로 정리된다.
- `RuntimeToolMessage`는 provider tool message인 `{ role: "tool", tool_call_id, name, content }` 형태로 돌아간다.
- `AgentRunner`는 `awaiting_tools`, `tools_completed`, `final_response` checkpoint를 남기고, `ToolEvent`로 tool 진행을 관측 가능하게 만든다.

### 후속 비목표 / 별도 owner로 넘길 것

- 공유 `Effect::RunTool` 또는 formal `RunToolEffect` 타입
- per tool call `effect_id`와 formal `correlation_id`
- `ToolOutcomeRecorded` event와 `ToolCallCompleted`/`ToolCallFailed`/`ToolCallTimedOut`/`ToolCallCancelled` 재진입 command
- execution envelope 안의 `permission_snapshot`을 다시 대조하는 runtime guard
- 모든 tool 결과를 감싸는 공통 normalized outcome envelope
- `timed_out`과 `cancelled`를 success/failure와 별도 outcome state로 기록하는 모델
- 종료된 턴 뒤 도착한 late result를 채택, 폐기, 관찰 event로 나누는 formal rule

### 로컬 근거

- `crates/shacs-core/src/runtime/runner.rs`
- `crates/shacs-core/src/runtime/tool_execution.rs`
- `crates/shacs-core/src/tools/base.rs`
- `crates/shacs-core/src/tools/registry.rs`
- `crates/shacs-core/tests/runtime.rs`: `RuntimeToolExecutor`와 `RuntimeToolMessage`의 result message mapping, ask-user interrupt, skipped later tools 경계
- `crates/shacs-core/tests/runtime_agent.rs`: runner tool loop, checkpoint, normalized tool result 사용 경계
- `crates/shacs-core/tests/runtime_loop.rs`: runtime checkpoint materialization과 session recovery context 경계
- `crates/shacs-core/tests/tools.rs`: registry validation과 tool safety 경계

---

## 현재 구현과의 관계

Spec 002의 Command, Event, Effect 용어는 현재 코드에서 권한과 책임을 설명하는 개념어다. 모든 개념이 같은 이름의 공유 Rust enum으로 존재한다는 뜻이 아니다.

현재 코드 기준 매핑은 아래와 같다.

| spec 개념 | current code 기준 |
|---|---|
| tool runtime | `crates/shacs-core/src/runtime/runner.rs`의 `AgentRunner`와 `crates/shacs-core/src/runtime/tool_execution.rs`의 `RuntimeToolExecutor` |
| tool call envelope | `RuntimeToolCall { id, name, arguments }` |
| tool registry | `crates/shacs-core/src/tools/registry.rs`의 `ToolRegistry` |
| tool output | `ToolResult::Text`, `ToolResult::Json`, `ToolResult::AskUserInterrupt` |
| provider로 돌아가는 tool message | `RuntimeToolMessage { role: "tool", tool_call_id, name, content }` |
| 실행 보고서 | `RuntimeToolExecutionReport { messages, interrupt, skipped_tool_calls }` |
| 관측 event | `ToolEvent`와 `ToolStatus::{Ok, Error, Waiting, Skipped}` |
| 진행 저장 경계 | `AgentRunner`가 호출하는 `checkpoint_callback`, 예: `awaiting_tools`, `tools_completed`, `final_response` |

따라서 004 완료 판정은 다음을 blocker로 보지 않는다. 이 항목들은 구현됐다고 주장하지 않고, 후속 owner 작업으로 남긴다.

- 공유 `Effect::RunTool` 또는 formal `RunToolEffect` 타입
- per tool call `effect_id`와 formal `correlation_id`
- `ToolOutcomeRecorded` event와 `ToolCallCompleted`/`ToolCallFailed`/`ToolCallTimedOut`/`ToolCallCancelled` 재진입 command
- execution envelope 안의 `permission_snapshot`을 다시 대조하는 runtime guard
- 모든 tool 결과를 감싸는 공통 normalized outcome envelope
- `timed_out`과 `cancelled`를 success/failure와 별도 outcome state로 기록하는 모델
- 종료된 턴 뒤 도착한 late result를 채택, 폐기, 관찰 event로 나누는 formal rule

---

## 상위 기준과의 관계

이 문서는 다음 상위 기준을 전제로 한다.

- 세션에 보이는 상태 변경은 런타임 오케스트레이터 경계가 결정한다.
- tool runtime은 바깥 실행 경계이며, runner가 허용한 provider tool 요청 아래에서만 동작한다.
- tool은 런타임 권한을 우회하지 않는다.
- tool 결과는 provider 재호출 문맥에 넣을 `RuntimeToolMessage` 또는 사용자 interrupt로 정리되며, tool executor가 세션 기록을 직접 확정하지 않는다.
- permission 판단, retry 판단, abort 판단, late result 채택 여부를 formal policy로 분리하는 일은 후속 owner gap이다.

따라서 tool runtime은 "강한 실행기"가 아니라 "제한된 외부 실행 경계"다. 실행은 할 수 있지만 세션 상태 확정은 할 수 없다.

---

## 범위

이 문서는 다음을 정의한다.

- `ToolRegistry`의 책임과 조회 규칙
- `RuntimeToolCall`과 `RuntimeToolExecutor`의 현재 실행 경계
- `ToolResult`, `RuntimeToolMessage`, `RuntimeToolExecutionReport`의 결과 표현
- `ToolEvent`, `ToolStatus`, `checkpoint_callback`을 통한 관측과 진행 저장
- 현재 구현이 일부만 가진 timeout, failure, cancellation 처리 위치
- 후속 owner로 남길 formal effect, outcome, reentry, late-result gap

이 문서는 다음을 정의하지 않는다.

- 개별 tool의 내부 구현 알고리즘
- 구체적인 shell sandbox 구현 방식
- plugin marketplace나 원격 설치 프로토콜
- session store의 직렬화 포맷

특히 이 문서는 plugin marketplace 동작을 상정하지 않는다. tool runtime은 현재 사용자가 실행하는 로컬 런타임이 이미 알고 있는 tool 정의를 실행하는 경계만 다룬다.

---

## 핵심 정의

### tool registry

`ToolRegistry`는 런타임과 tool executor가 공유하는 tool 정의 인덱스다. 현재 구현에서는 tool 이름으로 executor를 찾고, 입력 parameter를 검증하며, unknown tool이나 invalid arguments를 provider에 돌려줄 error text로 정규화한다.

registry는 최소한 다음을 제공한다.

- `tool_name`으로 tool 정의 조회
- 입력 parameter 검증
- executor 호출 경계
- tool별 설명과 schema를 provider에 노출할 수 있는 정의

registry가 해서는 안 되는 일:

- 세션 상태 직접 수정
- 실행 결과 캐시를 공식 상태처럼 보관
- permission 허용 여부 최종 확정
- tool 호출 결과를 conversation 기록에 직접 반영

### tool runtime

현재 tool runtime은 `AgentRunner`가 provider의 `ToolCallRequest`를 `RuntimeToolCall { id, name, arguments }`로 바꾸고, `RuntimeToolExecutor`가 registry-backed call을 실행하는 경계다.

tool runtime은 다음만 할 수 있다.

- registry를 통해 tool 정의 조회
- `RuntimeToolCall`의 `id`, `name`, `arguments`를 바탕으로 executor 호출
- `ToolResult::Text`, `ToolResult::Json`, `ToolResult::AskUserInterrupt`를 `RuntimeToolMessage` 또는 interrupt로 변환
- `RuntimeToolExecutionReport`에 `messages`, `interrupt`, `skipped_tool_calls`를 모아 runner에 반환
- `ToolEvent`와 checkpoint로 진행 상황을 알림

tool runtime은 다음을 할 수 없다.

- 실행 승인 자체를 독자적으로 확정
- 세션 기록 append
- 후속 tool 호출 결정
- assistant 메시지 생성 또는 확정
- permission mode 변경

### current call boundary

현재 구현의 실행 입력은 formal execution envelope가 아니라 `RuntimeToolCall`이다. 이 타입은 `id`, `name`, `arguments`만 가진다. `session_id`, `turn_id`, `effect_id`, `correlation_id`, `permission_snapshot`은 현재 tool call envelope에 들어 있지 않다.

후속 owner가 formal `RunToolEffect`를 도입한다면 이 문서의 권한 경계와 같은 방향이어야 한다. 단, 그 모델을 현재 구현된 것으로 말하면 안 된다.

---

## tool registry 명세

현재 registry 항목은 tool 이름, 설명, 입력 schema 또는 parameter validator, executor 연결을 중심으로 구성된다. capability taxonomy, default timeout, permission profile 같은 풍부한 metadata는 후속 정책 owner가 필요할 때 확장할 수 있다.

권장 규칙은 다음과 같다.

1. registry는 런타임 초기화 시 구성되거나 명시적 reload를 통해 갱신된다.
2. registry 조회 실패는 실행기 내부 panic 이유가 아니라 provider로 전달 가능한 tool error message가 되어야 한다.
3. 입력 검증 실패도 executor 내부 panic이 아니라 normalized error text로 반환되어야 한다.
4. registry는 tool 존재와 실행 계약을 설명할 수 있어야 하지만 상태 권한을 가지면 안 된다.

### capability 분류의 현재 위치

초기 구현은 과도한 capability 세분화보다 tool별 validator와 executor 경계를 우선한다. `fs_read`, `fs_write`, `proc_exec`, `net_outbound` 같은 canonical capability taxonomy와 permission snapshot guard는 010 host safety 문서와 함께 후속 owner가 정리할 영역이다.

현재 문서는 capability를 실행 자체를 허용하는 플래그로 보지 않는다. 실행 가능 여부를 최종 판단하는 권한은 tool registry가 아니라 runner와 상위 런타임 정책 경계에 남아야 한다.

---

## runtime execution 명세

현재 실행 흐름은 아래 표면을 기준으로 한다.

- provider가 `ToolCallRequest`를 반환한다.
- `AgentRunner`가 이를 `RuntimeToolCall { id, name, arguments }`로 변환한다.
- `RuntimeToolExecutor`가 `ToolRegistry`를 조회하고 tool을 실행한다.
- tool은 `ToolResult`를 반환한다.
- executor는 결과를 `RuntimeToolMessage` 또는 `RuntimeInterrupt`로 바꿔 `RuntimeToolExecutionReport`에 담는다.
- runner는 `awaiting_tools`, `tools_completed`, `final_response` 같은 checkpoint를 남기고 provider 재호출 또는 interrupt/fatal error 처리를 결정한다.

### 현재 execution 규칙

1. provider tool request의 `id`는 provider에 돌려줄 `tool_call_id`로 유지되어야 한다.
2. unknown tool과 invalid arguments는 panic이 아니라 provider가 이해할 수 있는 tool error content로 반환되어야 한다.
3. `ToolResult::AskUserInterrupt`는 일반 tool message가 아니라 사용자 입력이 필요한 interrupt로 올라가야 한다.
4. `RuntimeToolExecutionReport.skipped_tool_calls`는 실행되지 않은 tool call을 관측 가능하게 남긴다.
5. executor는 세션 상태나 assistant 최종 응답을 직접 확정하면 안 된다.

### 후속 owner gap

- formal `RunToolEffect`와 `ToolCallOutcome` 타입 분리
- envelope validation을 executor 진입 전에 수행하는 공통 계층
- `effect_id`와 `correlation_id` 기반 idempotency
- `permission_snapshot`과 실제 요청을 대조하는 runtime guard

---

## permission 연동

permission은 tool runtime의 부가 기능이 아니라 런타임 정책의 일부다.

### 현재 권한 판단 위치

현재 구현에서 `ToolRegistry`는 parameter validation과 tool별 안전 제한을 수행한다. `AgentRunner`는 provider tool call을 runner loop 안에서만 실행하고, tool 결과를 세션에 직접 쓰지 않는다. `AskUserInterrupt`는 사용자 확인이 필요한 상황을 runner 경계로 올리는 현재 표면이다.

현재 구현은 execution envelope에 `permission_snapshot`을 담아 runtime에서 다시 대조하는 모델을 갖고 있지 않다.

### 후속 permission bridge 규칙

후속 owner가 formal permission bridge를 도입한다면 오케스트레이터 또는 런타임 정책 계층은 `tool_name`, canonical capability, args, 경로 범위, 세션 mode를 바탕으로 permission을 평가한 뒤 그 결과를 execution envelope에 snapshot으로 남길 수 있다.

그 snapshot은 최소한 아래 의미를 담을 수 있어야 한다.

- 평가 시점 mode, 예: `default`, `auto`, `plan`
- 허용된 capability 범위
- 허용된 경로 또는 작업 범위
- 추가 확인이 필요했는지 여부

`plan` 같은 분석 전용 모드에서 write/exec를 effect 생성 이전에 거절하는 규칙은 후속 policy owner가 formal effect 모델과 함께 고정해야 한다.

### 핵심 원칙

tool이 런타임 권한을 우회하는 경로는 존재하면 안 된다. permission은 tool runtime 안으로 완전히 위임되는 것이 아니라, runner와 상위 정책 경계가 이미 결정한 실행 범위를 runtime이 벗어나지 않는 구조여야 한다.

---

## timeout, error, cancellation 동작

현재 구현은 모든 tool 결과를 공통 `completed`/`failed`/`timed_out`/`cancelled` outcome envelope로 기록하지 않는다. 관측 표면은 `ToolStatus::{Ok, Error, Waiting, Skipped}`, `RuntimeToolExecutionReport`, fatal tool error, ask-user interrupt, runner boundary의 cancellation token이다.

### timeout

- tool별 timeout이나 executor 내부 제한은 tool 구현과 registry-backed executor 경계에서 다룰 수 있다.
- 현재 spec 완료는 `ToolCallTimedOut` 재진입 command나 `timed_out` outcome state 구현을 뜻하지 않는다.
- timeout 이후 늦게 도착한 외부 완료 신호를 formal late result로 처리하는 규칙은 후속 owner gap이다.

### failure

현재 failure는 주로 아래 방식으로 드러난다.

- registry 조회 실패
- 입력 검증 실패
- executor 실행 오류
- fatal tool error
- provider에 돌려줄 tool error content

오류 정보는 사용자가 이해할 수 있는 요약을 가져야 한다. 단, 내부 핸들이나 복구 불가능한 런타임 객체를 세션 상태에 흘려보내면 안 된다.

### cancellation

- 현재 runner 경계에는 cancellation token support가 있다.
- 이 support는 formal `ToolCallCancelled` 재진입 모델을 구현했다는 뜻이 아니다.
- 실행 중인 외부 작업의 out-of-band cancellation과 취소 후 late result 채택 규칙은 후속 owner gap이다.

---

## result 정규화 명세

현재 tool runtime의 출력은 다음 표면으로 정리된다.

- `ToolResult::Text(String)`
- `ToolResult::Json(Value)`
- `ToolResult::AskUserInterrupt { question, options }`
- provider tool message인 `RuntimeToolMessage { role: "tool", tool_call_id, name, content }`
- runner로 돌아가는 `RuntimeToolExecutionReport { messages, interrupt, skipped_tool_calls }`

### output 정규화 원칙

현재 구현은 text와 JSON 결과를 provider가 받을 수 있는 content로 바꾸고, ask-user 결과는 interrupt로 분리한다. 큰 text tool result는 `maybe_persist_text_tool_result`와 `max_tool_result_chars` 경계를 통해 저장 또는 truncation 처리를 받을 수 있다.

이 동작은 full artifact outcome envelope를 구현했다는 뜻이 아니다. `binary_ref`, `artifact_list`, 공통 `artifact_ref` safety envelope, runtime artifact root 채택 규칙은 후속 owner gap이다.

### 오류 정규화 원칙

unknown tool과 invalid arguments는 provider에 전달 가능한 normalized error text로 돌아가야 한다. 현재 이 증거는 `ToolRegistry`와 `RuntimeToolExecutor` 경계, 그리고 `crates/shacs-core/tests/runtime.rs`와 `crates/shacs-core/tests/tools.rs`의 validation 테스트에서 확인한다.

후속 owner가 공통 outcome envelope를 도입한다면 오류는 최소한 아래 의미를 가져야 한다.

- `code`, 예: `invalid_arguments`, `unknown_tool`, `timeout`
- `message`, 사용자와 개발자가 모두 이해할 수 있는 짧은 설명
- `retryable`, 오케스트레이터 판단을 돕는 힌트
- `details`, 선택적 구조화 정보

`retryable`은 참고 정보일 뿐이다. retry 여부를 확정하는 것은 여전히 runner 또는 상위 런타임 정책이다.

---

## reentry path의 현재 경계

현재 tool 결과는 formal reentry command로 돌아오지 않는다. `AgentRunner` 안에서 `RuntimeToolMessage`가 provider 재호출 context에 들어가고, `RuntimeToolExecutionReport`가 interrupt 또는 fatal error 판단의 입력이 된다.

### 현재 허용되는 반환 표면

- provider tool message 목록
- 사용자 입력이 필요한 interrupt
- skipped tool call 목록
- fatal tool error
- checkpoint와 `ToolEvent` 관측 신호

### 후속 reentry gap

후속 owner가 formal reentry path를 도입한다면 아래 모델은 별도 작업으로 다뤄야 한다.

- `ToolCallCompleted`
- `ToolCallFailed`
- `ToolCallTimedOut`
- `ToolCallCancelled`
- `ToolOutcomeRecorded`
- 종료된 turn 또는 superseded effect에 대한 late result adoption rule

현재 spec 완료는 위 command나 event가 구현됐다는 뜻이 아니다.

---

## 전체 roundtrip 예시

아래는 현재 구현 기준으로 `read` 성격의 tool이 한 번 호출되는 정상 시퀀스다.

```text
1) 사용자 입력이 AgentLoop를 통해 AgentRunner 실행으로 이어진다.
2) AgentRunner가 provider request를 보낸다.
3) provider가 ToolCallRequest(tool proposal: read)를 반환한다.
4) AgentRunner가 RuntimeToolCall { id, name, arguments }를 만든다.
5) AgentRunner가 checkpoint_callback으로 awaiting_tools를 남긴다.
6) RuntimeToolExecutor가 ToolRegistry에서 read 정의를 조회한다.
7) RuntimeToolExecutor가 registry-backed executor를 호출한다.
8) executor가 ToolResult::Text(...) 또는 ToolResult::Json(...)을 반환한다.
9) RuntimeToolExecutor가 RuntimeToolMessage { role: "tool", tool_call_id, name, content }를 만든다.
10) RuntimeToolExecutor가 RuntimeToolExecutionReport { messages, interrupt: None, skipped_tool_calls }를 반환한다.
11) AgentRunner가 ToolEvent(status=Ok 또는 Error)를 emit한다.
12) AgentRunner가 checkpoint_callback으로 tools_completed를 남긴다.
13) AgentRunner가 tool message를 포함해 provider를 다시 호출한다.
14) final assistant response가 나오면 AgentRunner가 final_response checkpoint를 남기고 AgentLoop가 세션에 보이는 결과 반영을 조정한다.
```

핵심은 9단계 이후다. 읽은 파일 내용은 tool executor가 곧바로 대화 기록에 쓰지 않는다. runner와 loop 경계가 그 결과를 provider context와 최종 응답 반영에 어떻게 쓸지 결정한다.

---

## skipped 또는 invalid execution path 예시

아래는 registry에 없거나 입력이 맞지 않는 tool 요청의 현재 경계다.

```text
1) provider가 ToolCallRequest(tool proposal: unknown_or_invalid)를 반환한다.
2) AgentRunner가 RuntimeToolCall { id, name, arguments }를 만든다.
3) RuntimeToolExecutor가 ToolRegistry 조회 또는 parameter validation을 수행한다.
4) 조회 또는 검증이 실패한다.
5) RuntimeToolExecutor는 panic하지 않고 provider가 받을 수 있는 tool error content를 만든다.
6) AgentRunner는 ToolEvent(status=Error 또는 Skipped)를 emit한다.
7) AgentRunner는 report의 messages 또는 skipped_tool_calls를 바탕으로 다음 provider 호출, interrupt, fatal error 처리를 결정한다.
```

이 경로에서 중요한 점은 registry validation 실패가 세션 상태를 직접 고치지 않는다는 점이다.

---

## 구현 불변식

아래 불변식은 현재 Rust 구현과 후속 owner 작업 모두에서 유지해야 한다.

1. tool executor는 세션 상태를 직접 수정할 수 없다.
2. tool executor는 session event log를 직접 append할 수 없다.
3. provider tool request는 `AgentRunner` 경계에서 `RuntimeToolCall`로 변환되어야 한다.
4. `RuntimeToolMessage.tool_call_id`는 provider가 준 tool call id와 연결되어야 한다.
5. unknown tool과 invalid arguments는 panic이 아니라 normalized error text로 반환되어야 한다.
6. `AskUserInterrupt`는 일반 text result로 숨기지 않고 interrupt로 올라가야 한다.
7. 큰 text result는 무제한으로 provider context와 세션 기록을 오염시키면 안 된다.
8. tool runtime은 결과를 보고 다음 tool 또는 provider 호출을 독자적으로 시작하면 안 된다. 다음 호출 판단은 `AgentRunner` loop에 남는다.
9. registry는 tool 존재와 실행 계약을 설명할 수 있어야 하지만 상태 권한을 가지면 안 된다.
10. future formal effect를 도입하더라도 permission, timeout, cancellation, late result 채택 권한은 executor 내부로 흩어지면 안 된다.

---

## forbidden patterns

### 1. tool executor의 직접 상태 반영

금지 예:

- read 결과를 곧바로 conversation history에 append
- write 성공 후 session store에 "파일 수정 완료"를 직접 기록

왜 금지인가:

- 런타임 오케스트레이터 단일 권한 원칙이 깨진다.
- 중복 결과와 취소 경계를 안전하게 설명할 수 없다.

### 2. runtime 내부의 독자적 permission 승격

금지 예:

- 분석 전용 모드인데 runtime이 "이 정도는 안전하다"고 판단해 write 실행
- shell tool이 자체 확인 프롬프트를 띄워 실행 허용

왜 금지인가:

- 정책 진실 원천이 분산된다.
- 같은 세션을 replay해도 동일 결과를 보장하기 어렵다.

### 3. runner 경계 없는 tool 실행

금지 예:

- CLI helper가 runner를 거치지 않고 read tool 직접 호출
- provider adapter가 모델 출력에 따라 tool executor를 바로 부름

왜 금지인가:

- tool call id와 provider context 연결이 끊긴다.
- checkpoint와 `ToolEvent`로 진행을 설명할 수 없게 된다.

### 4. 외부 결과의 state patch 재진입

금지 예:

- `Command::ApplyToolStatePatch { ... }`
- tool runtime이 "assistant reply" 필드를 채워 넣은 결과 반환

왜 금지인가:

- 외부 실행기가 최종 상태 계산을 가로채게 된다.
- session kernel 문서의 상태 경계가 무너진다.

### 5. 정규화되지 않은 결과 전달

금지 예:

- 어떤 tool은 문자열, 어떤 tool은 임의 map, 어떤 tool은 프로세스 핸들을 그대로 반환

왜 금지인가:

- Rust 타입 경계가 흐려진다.
- 재시도, 로깅, 테스트, resume 규칙을 일관되게 적용할 수 없다.

### 6. timeout 뒤 성공으로 덮어쓰기

금지 예:

- timeout 또는 cancellation 판단 뒤 늦게 온 외부 완료를 정상 성공처럼 세션 결과로 덮어쓰기

왜 금지인가:

- 종료된 판단을 뒤집어 세션 재현성이 깨진다.

현재 구현은 이 late-result adoption rule을 formal하게 갖고 있지 않다. 이 금지 패턴은 후속 owner가 formal timeout/cancelled outcome을 도입할 때 지켜야 할 기준이다.

---

## Rust 구현으로 이어질 체크포인트

현재 구현에 대해서는 아래 질문에 "예"라고 답할 수 있어야 한다.

- `RuntimeToolCall`, `RuntimeToolExecutor`, `ToolRegistry`, `ToolResult`, `RuntimeToolMessage`, `RuntimeToolExecutionReport`의 책임이 분리되어 있는가?
- provider tool call id가 `RuntimeToolMessage.tool_call_id`로 보존되는가?
- registry 조회 실패와 입력 검증 실패가 panic이 아니라 provider-visible error content로 돌아가는가?
- `AskUserInterrupt`가 일반 tool text로 섞이지 않고 interrupt로 올라가는가?
- checkpoint와 `ToolEvent`가 tool 진행을 관측 가능하게 만드는가?
- 큰 text result가 `maybe_persist_text_tool_result`와 `max_tool_result_chars` 경계로 제한되는가?

후속 owner 작업에서는 아래 질문을 별도로 다뤄야 한다.

- formal `RunToolEffect`, effect id, correlation id를 도입할 것인가?
- permission snapshot guard를 어디에서 강제할 것인가?
- 공통 normalized outcome envelope가 필요한가?
- `timed_out`과 `cancelled`를 별도 outcome state로 기록할 것인가?
- late result와 duplicate result를 idempotent하게 무시하거나 관찰 event로 남기는 규칙을 어떻게 둘 것인가?

---

## 테스트 관점에서 꼭 검증할 시나리오

현재 증거로 삼을 수 있는 테스트 범위는 아래와 같다.

- `crates/shacs-core/tests/runtime_agent.rs`: provider tool call이 runtime tool execution과 provider tool message로 이어지는 경계, checkpoint 기반 진행 저장
- `crates/shacs-core/tests/runtime.rs`: `RuntimeToolExecutor` result message mapping, ask-user interrupt, skipped later tools 경계
- `crates/shacs-core/tests/runtime_loop.rs`: runtime checkpoint materialization과 session recovery context 경계
- `crates/shacs-core/tests/tools.rs`: registry validation, filesystem/exec 제한, 출력 truncation, 민감한 self-tool path 차단

이 증거는 current architecture를 뒷받침한다. 다만 formal `ToolOutcomeRecorded`, `ToolCallTimedOut`, `ToolCallCancelled`, full late-result handling을 검증한다는 뜻은 아니다.

---

## 결론

`shacs-bot`의 현재 tool runtime은 `AgentRunner`와 `RuntimeToolExecutor`가 provider tool 요청을 registry-backed 실행으로 연결하고, 그 결과를 `RuntimeToolMessage`, interrupt, skipped call, checkpoint, `ToolEvent`로 정리하는 경계다.

즉 이 문서의 핵심은 하나다. tool runtime은 바깥 실행 경계이고, 세션에 보이는 상태 권한은 끝까지 runner와 상위 런타임 오케스트레이터에 남는다. formal effect, outcome, permission snapshot, timeout/cancelled state, late-result adoption은 현재 구현이 아니라 후속 owner 작업이다.
