# provider runtime 아키텍처 명세

## 문서 목적

이 문서는 `docs/SYSTEM-FOUNDATION.md`, `docs/specs/001-session-kernel/SPEC.md`, `docs/specs/002-command-event-effect/SPEC.md`를 바탕으로 `shacs-bot`의 provider runtime 경계를 구현 가능한 수준으로 고정한다.

목표는 다음과 같다.

- 모델 호출이 오케스트레이터 바깥에서 어떻게 실행되는지 정의한다.
- streaming, stop reason, tool call 요청, timeout, cancellation, late result를 어떤 의미로 정규화할지 고정한다.
- future Rust 구현에서 provider adapter trait, 결과 enum, correlation 타입, 테스트 시나리오를 직접 도출할 수 있게 한다.

이 문서는 구체 provider API 사양이나 개별 벤더별 옵션 전체를 다루지 않는다. 이 문서가 다루는 것은 `MainOrchestrator`와 provider runtime 사이의 실행 계약이다.

이 spec의 완료 기준은 단순히 모델을 한 번 호출해 보는 POC가 아니라, 이 문서가 정의한 invocation contract, 결과 정규화, tool request 처리, cancellation/timeout/late result 규칙을 충족하는 **완전한 기능 구현과 검증**이다.

---

## 상위 기준과의 관계

이 문서는 다음 상위 기준을 전제로 한다.

- `MainOrchestrator`는 세션 상태를 변경할 수 있는 유일한 권한자다.
- provider runtime은 tool runtime과 마찬가지로 오케스트레이터 바깥의 effect executor다.
- provider 결과는 직접 상태에 반영되지 않고 항상 오케스트레이터 재진입 경계를 통과해야 한다.
- 모델은 "무엇을 할지" 제안할 수는 있지만, 실행 승인과 상태 확정 권한은 오케스트레이터에 있다.

따라서 provider runtime은 지능 공급자이지 정책 확정자가 아니다.

---

## 범위

이 문서는 다음을 정의한다.

- `Effect::InvokeModel`의 실행 계약
- provider 요청 envelope의 필수 필드
- streaming 결과의 정규화 방식
- tool call 요청과 최종 응답의 의미 구분
- timeout, abort, cancellation, late result 규칙
- provider 결과의 reentry path
- 구현 불변식, 정상 예시, 실패 예시, 금지 패턴

이 문서는 다음을 정의하지 않는다.

- 특정 벤더 API의 전체 파라미터 목록
- 과금 집계 및 billing 정책
- reasoning token UI 표현 방식
- prompt compression 알고리즘 세부 구현
- provider 선택 UX나 profile 저장 포맷
- 어떤 provider/auth family를 제품 범위로 채택할지의 목록, 단 현재 범위 결정은 008을 따른다.

---

## 핵심 정의

### provider runtime

provider runtime은 `Effect::InvokeModel`을 받아 실제 모델 호출을 수행하고, 그 결과를 오케스트레이터가 이해할 수 있는 정규화된 구조로 반환하는 실행 경계다.

provider runtime은 다음만 할 수 있다.

- 요청 envelope 검증
- 모델 스트림 수신
- 스트림을 정규화된 결과 구조로 변환
- timeout / cancellation / provider failure를 결과 상태로 보고

provider runtime은 다음을 할 수 없다.

- assistant 응답을 세션 기록에 직접 추가
- tool call을 자동 실행
- provider가 제안한 행동을 승인된 effect로 승격
- 세션 policy, permission mode, skill selection 변경

### model invocation

model invocation은 특정 턴의 현재 문맥을 바탕으로 모델에게 다음 행동 후보를 생성하게 하는 effect다.

결과는 크게 다음 네 가지 중 하나다.

- 최종 assistant 응답 후보
- tool call 요청 후보
- provider 실패
- timeout / cancellation

### stop reason

stop reason은 provider가 왜 이번 호출을 멈췄는지를 설명하는 정규화된 종료 이유다.

초기 구현은 최소한 아래 의미를 구분할 수 있어야 한다.

- `completed`
- `tool_requested`
- `max_tokens`
- `cancelled`
- `timed_out`
- `failed`

---

## `InvokeModel` effect 명세

`Effect::InvokeModel`은 최소한 아래 필드를 가져야 한다.

- `session_id`
- `turn_id`
- `effect_id`
- `causation_id`
- `correlation_id`
- `provider_profile`
- `model_id`
- `messages_snapshot` 또는 이에 준하는 요청 문맥 참조
- `system_context_snapshot`
- `tool_schema_snapshot`
- `max_output_tokens`
- `timeout_ms`
- `issued_at`

### envelope 규칙

1. envelope는 오케스트레이터가 승인한 단일 모델 호출을 나타낸다.
2. provider runtime은 envelope에 없는 tool schema나 정책을 임의로 추가하면 안 된다.
3. provider runtime은 결과에 반드시 `effect_id`, `session_id`, `turn_id`, `correlation_id`를 다시 붙여야 한다.
4. 동일 effect에 대한 결과는 적어도 오케스트레이터가 중복 감지할 수 있는 식별자를 가져야 한다.

---

## provider 결과 정규화

provider runtime의 출력은 벤더별 raw 응답이 아니라 공통 결과 envelope여야 한다.

### 공통 결과 필드

- `session_id`
- `turn_id`
- `effect_id`
- `provider_profile`
- `model_id`
- `status`
- `stop_reason`
- `started_at`
- `finished_at`
- `duration_ms`
- `usage` optional
- `raw_provider_metadata` optional

### 본문 결과 필드

정규화된 본문은 최소한 아래 중 하나를 담을 수 있어야 한다.

- `assistant_message_candidate`
- `tool_call_candidates[]`
- `error`

여기서 중요한 점은, 이 값들이 아직 **공식 세션 상태가 아니라 후보 결과**라는 것이다.

### streaming 처리 규칙

streaming은 provider runtime 내부에서는 상세 이벤트로 다룰 수 있다. 그러나 오케스트레이터 재진입 경계에서는 최소한 아래 두 단계로 정리돼야 한다.

- 진행 중 관찰 이벤트 또는 내부 버퍼
- 최종 정규화 결과

즉 stream chunk 자체를 공식 상태 전이로 취급하면 안 된다.

---

## tool call 요청 처리 규칙

provider가 tool call을 제안할 수는 있지만, 실행 승인 권한은 provider runtime에 없다.

### 원칙

- provider 결과에 tool call 후보가 있더라도, 그것은 아직 실행 명령이 아니다.
- tool call 후보는 reentry command로 오케스트레이터에 전달된다.
- 오케스트레이터는 permission, policy, 현재 turn 상태를 보고 실제 `Effect::RunTool`을 발행할지 결정한다.

### 금지

- provider runtime이 tool call 후보를 보고 tool runtime을 직접 호출
- provider runtime이 tool call 후보를 자동 승인된 event처럼 기록
- provider runtime이 tool call 후보에 없는 추가 argument를 주입

---

## 재진입 규칙

provider 결과는 반드시 command로 정규화되어 재진입한다.

초기 구현은 최소한 아래 의미의 command를 가질 수 있어야 한다.

- `ModelInvocationCompleted`
- `ModelInvocationToolRequested`
- `ModelInvocationFailed`
- `ModelInvocationTimedOut`
- `ModelInvocationCancelled`

각 command는 최소한 다음을 포함해야 한다.

- `session_id`
- `turn_id`
- `effect_id`
- `correlation_id`
- 결과 상태
- assistant 후보 본문 또는 tool call 후보
- provider 메타데이터 optional

오케스트레이터는 이 재진입 command를 보고 다음 중 하나를 결정한다.

- assistant 결과를 세션에 반영할지
- tool roundtrip으로 넘어갈지
- retry 할지
- abort 할지

provider runtime은 이 결정을 대신하면 안 된다.

---

## timeout, abort, cancellation, late result

### timeout

- provider 호출이 `timeout_ms`를 넘기면 `timed_out` 결과로 정규화한다.
- timeout 이후 뒤늦게 온 provider 응답은 late result가 될 수 있다.
- late result 채택 여부는 오케스트레이터가 correlation과 turn 상태를 기준으로 판단한다.

### cancellation

- 사용자가 턴 중단을 요청하거나 오케스트레이터가 abort를 결정하면 provider runtime은 best-effort로 호출을 취소한다.
- 취소 후 결과는 `cancelled`로 보고한다.
- 이미 닫힌 turn에 대한 취소 결과는 상태를 되살리면 안 된다.

### failure

`failed`는 최소한 아래 범주를 표현할 수 있어야 한다.

- 인증/권한 실패
- 네트워크 실패
- provider protocol 위반
- 응답 파싱 실패
- tool schema 불일치
- provider 내부 오류

### late result

late result는 원래 effect가 더 이상 유효하지 않은 뒤 도착한 결과다.

예:

- turn이 이미 `aborted` 또는 `completed` 되었는데 provider 결과가 늦게 옴
- retry 후 새 effect가 발행된 뒤 이전 effect 결과가 도착함

late result는 관찰 이벤트로 남길 수는 있어도 공식 assistant 응답이나 tool call로 승격하면 안 된다.

---

## 정상 시퀀스 예시

### 예시 1. 최종 assistant 응답

```text
1) Command::SubmitUserInput 가 들어온다.
2) MainOrchestrator 는 context_building 을 마치고 Effect::InvokeModel(effect_id=M-1) 을 발행한다.
3) provider runtime 은 모델을 호출하고 assistant_message_candidate 를 포함한 결과를 반환한다.
4) 결과는 Command::ModelInvocationCompleted(effect_id=M-1) 로 재진입한다.
5) MainOrchestrator 는 결과를 검증하고 assistant 응답을 공식 event 와 세션 기록에 반영한다.
6) turn 은 completed 로 닫힌다.
```

### 예시 2. tool call 제안 후 roundtrip

```text
1) Effect::InvokeModel(effect_id=M-9) 이 발행된다.
2) provider runtime 은 tool_call_candidates 를 포함한 결과를 반환한다.
3) 결과는 Command::ModelInvocationToolRequested(effect_id=M-9) 로 재진입한다.
4) MainOrchestrator 는 permission 과 policy 를 확인한다.
5) 허용되면 Effect::RunTool(effect_id=T-4) 를 발행한다.
6) tool 결과가 다시 재진입한 뒤, 오케스트레이터는 새 InvokeModel 을 발행할지 최종 응답을 확정할지 결정한다.
```

---

## 실패 시퀀스 예시

### 예시 3. provider timeout

```text
1) Effect::InvokeModel(effect_id=M-13, timeout_ms=30000) 이 발행된다.
2) provider runtime 은 30초 내 최종 결과를 만들지 못한다.
3) provider runtime 은 Command::ModelInvocationTimedOut(effect_id=M-13) 로 결과를 재진입시킨다.
4) MainOrchestrator 는 retry 정책을 평가한다.
5) retry 하지 않으면 turn 을 aborted 로 닫고 관련 event 를 기록한다.
```

### 예시 4. 늦게 도착한 이전 호출 결과

```text
1) Effect::InvokeModel(effect_id=M-20) 이 발행된다.
2) timeout 으로 인해 오케스트레이터는 새 Effect::InvokeModel(effect_id=M-21) 을 발행한다.
3) 나중에 M-20 의 결과가 늦게 도착한다.
4) provider bridge 는 M-20 결과를 재진입시키지만, MainOrchestrator 는 해당 effect 가 stale 임을 확인한다.
5) 결과는 무시되거나 관찰 이벤트로만 남는다.
6) 공식 assistant 응답은 오직 현재 유효한 호출 결과만으로 확정된다.
```

---

## 불변식

1. provider runtime 은 세션 상태를 직접 수정하면 안 된다.
2. provider raw 응답은 정규화되지 않은 채 세션 진실 원천이 되면 안 된다.
3. tool call 후보는 오케스트레이터 승인 전까지 실행 effect 가 아니다.
4. stream chunk 는 공식 event 가 아니다.
5. 같은 `session_id`, `turn_id`, `effect_id` 조합의 결과는 중복 또는 stale 판정 가능해야 한다.
6. 닫힌 turn 이후 도착한 provider 결과는 상태를 되살리면 안 된다.
7. provider 실패와 timeout 은 성공 응답처럼 세션 기록에 append 되면 안 된다.

---

## 금지 패턴

### 금지 패턴 1. provider runtime 이 assistant 응답을 직접 commit

왜 금지인가:

- 단일 권한 원칙이 깨진다.
- retry, abort, late result 판정 지점이 사라진다.

### 금지 패턴 2. provider runtime 이 tool call 후보를 자동 실행

왜 금지인가:

- permission 과 policy 경계를 우회한다.
- 모델 제안을 실행 승인으로 오인하게 만든다.

### 금지 패턴 3. stream chunk 를 공식 세션 이력으로 취급

왜 금지인가:

- provider 별 streaming 차이가 세션 의미론을 오염시킨다.
- crash / resume 시 결정성이 깨진다.

---

## Rust 구현으로 이어질 체크포인트

- `InvokeModelEffect` 와 `ModelInvocationOutcome` 가 분리된 타입인가?
- provider adapter trait 이 request envelope 과 normalized outcome 을 중심으로 설계되는가?
- timeout / cancelled / failed / completed / tool_requested 상태가 enum 으로 구분되는가?
- stale effect 결과를 correlation 기준으로 무시하는 테스트를 만들 수 있는가?
- stream chunk 와 final outcome 을 분리하는 버퍼링 계층이 있는가?

### 최소 테스트 관점

- provider 최종 응답이 공식 assistant 응답으로 반영되는 정상 경로
- tool call 후보가 재진입 후에만 tool effect 로 승격되는지
- timeout 이후 late result 가 무시되는지
- cancellation 이후 닫힌 turn 이 되살아나지 않는지
- malformed provider 응답이 failed 로 정규화되는지

---

## 명시적 비범위

이 문서는 다음을 정의하지 않는다.

- Anthropic/OpenAI/로컬 모델별 상세 파라미터 차이
- billing / usage 저장 포맷
- provider profile CLI UX
- prompt engineering 세부 규칙
- compact 직전 어떤 메시지를 얼마나 자를지의 알고리즘

이 항목들은 별도 문서에서 다룬다. 단, 어떤 하위 설계도 이 문서의 핵심 규칙, 특히 "provider 결과는 후보일 뿐이고 오케스트레이터를 거쳐야 공식 상태가 된다"는 원칙을 뒤집어서는 안 된다.

---

## 결론

`shacs-bot`의 provider runtime 은 모델을 붙여주는 편의 계층이 아니라, 오케스트레이터 바깥에서 동작하는 엄격한 실행 경계다.

현재 제품 범위에서는 OpenAI-compatible, Anthropic auth, Codex auth(OpenAI auth style)만 지원 대상으로 본다. provider runtime 구현은 OpenCode의 provider/auth 구조를 참고하되, 이 문서는 그 지원 목록을 소비하는 실행 계약만 다룬다.

핵심은 세 가지다.

- 모델 결과는 후보이며, 공식 상태가 아니다.
- tool call 제안은 실행 승인이 아니다.
- timeout, cancellation, late result 처리는 끝까지 오케스트레이터 중심으로 정리되어야 한다.
