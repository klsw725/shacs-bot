# tool runtime 아키텍처 명세

## 문서 목적

이 문서는 `docs/SYSTEM-FOUNDATION.md`, `docs/specs/001-session-kernel/SPEC.md`, `docs/specs/002-command-event-effect/SPEC.md`를 바탕으로 `shacs-bot`의 tool runtime 경계를 구현 가능한 수준으로 고정한다.

목표는 다음과 같다.

- tool registry가 무엇을 보관하고 무엇을 보관하지 않는지 정의한다.
- tool 실행이 어떤 envelope를 통해 오케스트레이터 정책 아래로 내려가는지 정의한다.
- permission, timeout, error, cancellation이 tool runtime에서 어떻게 정규화되어 재진입하는지 정의한다.
- future Rust 구현에서 trait, enum, id 타입, 테스트 시나리오를 직접 도출할 수 있게 한다.

이 문서는 방향 제안이 아니라 구현 기준이다. 구현이 이 문서와 충돌하면 코드를 먼저 밀어붙이지 않고 문서 판단부터 다시 확인해야 한다.

이 spec의 완료 기준은 tool 호출 데모 수준의 POC가 아니라, registry, execution envelope, permission integration, 결과 정규화, 재진입, 오류/취소/timeout 규칙까지 포함한 **완전한 기능 구현과 검증**이다.

---

## 상위 기준과의 관계

이 문서는 다음 상위 기준을 전제로 한다.

- `MainOrchestrator`는 세션 상태를 변경할 수 있는 유일한 권한자다.
- tool runtime은 바깥 실행 경계이며, 오케스트레이터 정책 아래에서만 동작한다.
- tool은 오케스트레이터 권한을 우회하지 않는다.
- tool 결과는 직접 상태에 반영되지 않고 항상 재진입 command로 정규화된다.
- permission 판단, retry 판단, abort 판단, late result 채택 여부는 모두 오케스트레이터가 결정한다.

따라서 tool runtime은 "강한 실행기"가 아니라 "제한된 외부 effect executor"다. 실행은 할 수 있지만 확정은 할 수 없다.

---

## 범위

이 문서는 다음을 정의한다.

- tool registry의 책임과 조회 규칙
- tool execution envelope의 필수 필드와 의미
- permission 연동 지점
- timeout, failure, cancellation 처리 규칙
- normalized result 형태
- reentry path와 correlation 규칙
- 구현 불변식, positive example, forbidden pattern

이 문서는 다음을 정의하지 않는다.

- 개별 tool의 내부 구현 알고리즘
- 구체적인 shell sandbox 구현 방식
- plugin marketplace나 원격 설치 프로토콜
- session store의 직렬화 포맷

특히 이 문서는 plugin marketplace 동작을 상정하지 않는다. tool runtime은 오케스트레이터가 이미 알고 있는 tool 정의를 실행하는 경계만 다룬다.

---

## 핵심 정의

### tool registry

tool registry는 오케스트레이터와 tool runtime이 공유하는 정적 또는 반정적 메타데이터 인덱스다. 여기에는 "어떤 tool이 존재하는가"와 "그 tool을 어떤 정책 조건에서 실행할 수 있는가"가 들어간다.

registry는 최소한 다음을 제공해야 한다.

- `tool_name`으로 tool 정의 조회
- 인자 스키마 또는 입력 계약 조회
- capability 분류 조회, 예: read, write, exec, network
- 기본 timeout 정책 조회
- 결과 정규화 방식 조회
- 호출 가능한 executor kind 조회

registry가 해서는 안 되는 일:

- 세션 상태 직접 수정
- 실행 결과 캐시를 공식 상태처럼 보관
- permission 허용 여부 최종 확정
- tool 호출 결과를 conversation 기록에 직접 반영

### tool runtime

tool runtime은 `Effect::RunTool`을 받아 실제 외부 작업을 수행하고, 결과를 normalized reentry command로 돌려주는 실행 경계다.

tool runtime은 다음만 할 수 있다.

- registry를 통해 tool 정의 조회
- 실행 envelope를 해석해 실제 executor 호출
- stdout, stderr, 파일 읽기 결과, 구조화 데이터, 오류를 정규화
- timeout 또는 cancellation을 감지해 결과 상태로 보고

tool runtime은 다음을 할 수 없다.

- 실행 승인 자체를 독자적으로 확정
- 세션 기록 append
- 후속 tool 호출 결정
- assistant 메시지 생성 또는 확정
- permission mode 변경

### execution envelope

execution envelope는 오케스트레이터가 승인한 단일 tool effect의 실행 계약이다. tool runtime은 envelope 밖의 맥락을 가정하면 안 된다.

---

## tool registry 명세

future Rust 구현에서 registry 항목은 대략 아래 의미를 가져야 한다.

- `tool_name`
- `version` 또는 이에 준하는 definition revision
- `description`
- `capabilities`, 예: `fs_read`, `fs_write`, `proc_exec`, `net_outbound`
- `input_schema_ref` 또는 validator
- `executor_kind`, 예: filesystem, shell, provider-backed
- `default_timeout_ms`
- `max_timeout_ms`
- `result_kind`, 예: text, bytes, json, file-list
- `permission_profile`

권장 규칙은 다음과 같다.

1. registry는 오케스트레이터 초기화 시 구성되거나 명시적 reload를 통해 갱신된다.
2. 턴 중간에 registry 의미가 바뀌더라도 이미 발행된 `Effect::RunTool`의 해석은 해당 effect 생성 시점 정의를 따른다.
3. registry 조회 실패는 실행기 내부 panic 이유가 아니라 정규화된 실패 결과가 되어야 한다.

### capability 분류 최소 기준

초기 구현은 과도한 세분화보다 010의 host safety 문서와 같은 canonical capability taxonomy를 따른다.

- `fs_read`: 파일 읽기, 메타데이터 조회, 검색
- `fs_write`: 파일 생성, 수정, 삭제
- `proc_exec`: shell 또는 외부 프로세스 실행
- `net_outbound`: 외부 네트워크 요청

`search` 같은 세부 동작은 별도 capability가 아니라 기본적으로 `fs_read`의 하위 사용례로 본다. secret 접근이 필요한 tool은 별도 host safety 판단(`secret_read`)의 영향을 받을 수 있지만, tool registry의 canonical capability 이름은 010의 host safety taxonomy와 충돌하면 안 된다.

이 capability는 실행 자체를 허용하는 플래그가 아니라, 오케스트레이터 permission 정책이 참고할 분류다.

---

## execution envelope 명세

`Effect::RunTool`은 최소한 아래 의미의 필드를 가져야 한다.

- `session_id`
- `turn_id`
- `effect_id`
- `causation_id`
- `correlation_id`
- `tool_call_id`
- `tool_name`
- `args`
- `issued_at`
- `timeout_ms`
- `permission_snapshot`
- `working_directory` 또는 실행 기준 위치
- `resource_limits`, 필요 시 크기 또는 출력 제한

### envelope 규칙

1. envelope는 오케스트레이터가 승인한 실행 사실을 담는다.
2. tool runtime은 envelope에 없는 추가 권한을 가정하면 안 된다.
3. tool runtime은 `timeout_ms`를 초과하는 실행을 성공으로 보고하면 안 된다.
4. tool runtime은 결과에 반드시 `effect_id`, `tool_call_id`, `session_id`, `turn_id`를 다시 붙여야 한다.
5. executor는 envelope를 다른 effect로 임의 변환하면 안 된다.

### Rust 구현 체크포인트

- `RunToolEffect`와 `ToolCallOutcome`는 서로 다른 타입이어야 한다.
- envelope validation은 executor 진입 전에 한 번, executor 내부에서 안전성 확인용으로 한 번 더 할 수 있다.
- validation 실패는 process abort가 아니라 정규화된 `failed` 결과로 돌려야 한다.

---

## permission 연동

permission은 tool runtime의 부가 기능이 아니라 오케스트레이터 정책의 일부다.

### 권한 판단 위치

최종 허용/거절 판단은 `MainOrchestrator`가 한다.

tool runtime은 다음만 할 수 있다.

- 전달받은 `permission_snapshot`을 실행 가드로 확인
- snapshot과 실제 요청이 모순될 때 실행을 거부하고 실패로 반환

tool runtime은 다음을 할 수 없다.

- 위험도를 다시 계산해 독자적으로 허용으로 뒤집기
- 사용자 확인을 자체 UI로 받아 승인 처리하기
- 세션 permission mode를 변경하기

### permission bridge 규칙

오케스트레이터는 `tool_name`, canonical capability, args, 경로 범위, 세션 mode를 바탕으로 permission을 평가한 뒤, 그 결과를 execution envelope에 snapshot으로 남긴다.

이 snapshot은 최소한 아래 의미를 담을 수 있어야 한다.

- 평가 시점 mode, 예: `default`, `auto`, `plan`
- 허용된 capability 범위
- 허용된 경로 또는 작업 범위
- 추가 확인이 필요했는지 여부

`plan` 같은 분석 전용 모드에서는 read/search는 허용될 수 있지만 write/exec는 effect 생성 이전에 거절되어야 한다.

### 핵심 원칙

tool이 오케스트레이터 권한을 우회하는 경로는 존재하면 안 된다. permission은 tool runtime 안으로 "위임"되는 것이 아니라, 오케스트레이터가 이미 결정한 정책을 runtime이 준수하는 구조여야 한다.

---

## timeout, error, cancellation 동작

tool runtime은 실행 결과를 최소한 다음 네 상태 중 하나로 정규화해야 한다.

- `completed`
- `failed`
- `timed_out`
- `cancelled`

### timeout

- timeout 기준 시각은 effect 발행 이후 executor가 수락한 시점부터 계산할 수 있다.
- timeout 발생 시 executor는 가능한 범위에서 외부 작업을 중지한다.
- 중지가 완전히 보장되지 않더라도 오케스트레이터에는 `timed_out`으로 보고해야 한다.
- timeout 이후 늦게 도착한 실제 외부 완료 신호는 late result로 취급될 수 있으며, 오케스트레이터가 별도 무시 판단을 한다.

### failure

`failed`는 최소한 아래 범주를 표현할 수 있어야 한다.

- 입력 검증 실패
- registry 조회 실패
- executor 초기화 실패
- 실행 중 I/O 실패
- permission snapshot 불일치
- 비정상 종료 코드
- 출력 정규화 실패

오류 정보는 사용자가 이해할 수 있는 요약과 디버깅 가능한 구조화 필드를 함께 가질 수 있어야 한다. 단, 내부 핸들이나 복구 불가능한 런타임 객체를 세션 상태에 흘려보내면 안 된다.

### cancellation

- cancellation은 사용자 abort 또는 상위 턴 중단 결정의 결과다.
- tool runtime은 취소 신호를 받으면 best-effort로 실행을 멈추고 `cancelled`를 보고한다.
- 이미 종료된 turn에 대한 취소 결과가 뒤늦게 오더라도 상태를 되살리면 안 된다.

---

## normalized result 명세

tool runtime의 출력은 개별 tool 구현마다 제멋대로여서는 안 되고, 공통 envelope 위에 실려야 한다.

### 공통 결과 필드

- `session_id`
- `turn_id`
- `effect_id`
- `tool_call_id`
- `tool_name`
- `status`
- `started_at`
- `finished_at`
- `duration_ms`
- `output`
- `error`, 실패 시
- `observations`, 선택 사항

### output 정규화 원칙

output은 최소한 다음 중 하나로 표현 가능해야 한다.

- `text`
- `structured_json`
- `binary_ref`, 실제 바이트 대신 runtime-managed artifact 참조
- `artifact_list`, runtime-managed artifact 참조 목록
- `empty`

tool runtime은 stdout 전체를 무제한으로 세션에 밀어 넣으면 안 된다. 큰 결과는 요약, 참조, 잘린 출력 표시 같은 방식으로 정규화되어야 한다.

`binary_ref`와 `artifact_list`는 008에서 정의한 runtime-managed artifact 루트를 가리키는 안정된 참조여야 한다. 기본적으로 이 참조는 workspace 임의 경로나 executor 내부 임시 핸들을 그대로 노출하면 안 되며, redaction과 수명주기 규칙을 만족하는 저장 위치로만 승격되어야 한다.

### 오류 정규화 원칙

오류는 최소한 다음 의미를 가져야 한다.

- `code`, 예: `timeout`, `permission_mismatch`, `io_error`
- `message`, 사용자와 개발자가 모두 이해할 수 있는 짧은 설명
- `retryable`, 오케스트레이터 판단을 돕는 힌트
- `details`, 선택적 구조화 정보

`retryable`은 참고 정보일 뿐이다. retry 여부를 확정하는 것은 여전히 오케스트레이터다.

---

## reentry path

tool runtime 결과는 반드시 재진입 command로 돌아와야 한다. 직접 event append나 session mutation 경로는 없다.

### 허용되는 재진입 command

- `ToolCallCompleted`
- `ToolCallFailed`
- `ToolCallTimedOut`
- `ToolCallCancelled`

### 재진입 규칙

1. 모든 재진입 결과는 `session_id`, `turn_id`, `effect_id`, `tool_call_id`, `tool_name`을 포함해야 한다.
2. 오케스트레이터는 상관관계가 맞는 경우에만 결과를 현재 턴의 임시 산출물로 수용할 수 있다.
3. 수용 이후에도 바로 세션 최종 기록에 append되는 것은 아니다.
4. 오케스트레이터는 결과를 바탕으로 다시 `context_building`, `model_pending`, `result_applying`, `aborted` 중 하나를 선택한다.
5. 종료된 turn 또는 superseded effect에 대한 결과는 late result로 무시하거나 관찰 이벤트만 남길 수 있다.

### 상태 반영 규칙

tool 결과는 먼저 `TurnState`의 임시 산출물로 머무른다. 그 뒤에만 다음 중 하나가 가능하다.

- 모델 재호출 문맥에 포함
- 사용자에게 설명 가능한 실패 사유로 반영
- 턴 abort 근거로 반영
- 최종 assistant 응답 생성의 참고 재료로 사용

tool 결과 그 자체가 세션의 공식 assistant 응답은 아니다.

---

## 전체 roundtrip 예시

아래는 `read` tool이 한 번 호출되는 전체 정상 시퀀스다.

```text
1) CLI -> Command::SubmitUserInput("docs/SYSTEM-FOUNDATION.md 핵심을 요약해줘")
2) MainOrchestrator -> Event::UserInputAccepted
3) MainOrchestrator -> Event::TurnStarted
4) MainOrchestrator -> Effect::InvokeModel
5) Provider executor -> Command::ModelInvocationToolRequested(tool proposal: read)
6) MainOrchestrator가 tool proposal을 검토한다.
7) MainOrchestrator -> Event::ToolCallRequested(tool_name=read)
8) MainOrchestrator가 permission과 경로 제약을 확인한 뒤 Effect::RunTool를 만든다.
9) Tool runtime이 registry에서 read 정의를 조회한다.
10) Tool runtime이 execution envelope에 따라 파일 읽기를 수행한다.
11) Tool runtime -> Command::ToolCallCompleted(
       session_id=S1,
       turn_id=T1,
       effect_id=E-read-1,
       tool_call_id=TC-1,
       tool_name=read,
       payload=text(...)
    )
12) MainOrchestrator -> Event::ToolResultAccepted(tool_call_id=TC-1)
13) MainOrchestrator -> Effect::InvokeModel(tool result 포함)
14) Provider executor -> Command::ModelInvocationCompleted(final assistant draft)
15) MainOrchestrator -> Event::AssistantResponseCommitted
16) MainOrchestrator -> Event::TurnCompleted
```

핵심은 11단계 이후다. 읽은 파일 내용은 tool runtime이 곧바로 대화 기록에 쓰지 않는다. 오케스트레이터가 그 결과를 채택한 뒤에만 다음 모델 호출 문맥으로 들어간다.

---

## denied execution path 예시

아래는 `write` tool이 permission 정책에 막혀 실행조차 되지 않는 예시다.

```text
1) CLI -> Command::SubmitUserInput("README를 수정해줘")
2) MainOrchestrator -> Event::UserInputAccepted
3) MainOrchestrator -> Event::TurnStarted
4) MainOrchestrator -> Effect::InvokeModel
5) Provider executor -> Command::ModelInvocationToolRequested(tool proposal: write)
6) MainOrchestrator가 현재 세션 mode가 `plan`임을 확인한다.
7) MainOrchestrator는 write capability가 이 mode에서 금지된다고 판단한다.
8) MainOrchestrator는 Effect::RunTool를 발행하지 않는다.
9) MainOrchestrator -> Event::ToolCallDenied(tool_name=write, reason=permission_denied)
10) MainOrchestrator는 해당 거절 사실을 현재 turn의 실패 근거로 유지한다.
11) MainOrchestrator는 필요하면 모델에 거절 사실을 설명하도록 재호출하거나, 곧바로 TurnAborted를 확정한다.
```

이 경로에서 중요한 점은 tool runtime이 아예 호출되지 않을 수 있다는 점이다. 권한 거절은 먼저 오케스트레이터에서 확정된다.

---

## failed execution path 예시

아래는 허용된 `exec` tool이 timeout으로 실패하는 예시다.

```text
1) MainOrchestrator -> Event::ToolCallRequested(tool_name=shell)
2) MainOrchestrator -> Effect::RunTool(timeout_ms=5000)
3) Tool runtime이 envelope와 registry를 확인하고 프로세스를 시작한다.
4) 5초 안에 실행이 끝나지 않는다.
5) Tool runtime은 가능한 범위에서 프로세스를 중지한다.
6) Tool runtime -> Command::ToolCallTimedOut(
       effect_id=E-shell-7,
       tool_call_id=TC-7,
       tool_name=shell,
       error={ code: "timeout", retryable: false }
    )
7) MainOrchestrator는 상관관계를 검증한다.
8) MainOrchestrator -> Event::ToolResultAccepted(status=timed_out)
9) MainOrchestrator는 retry 여부 또는 TurnAborted 여부를 결정한다.
10) 나중에 외부 프로세스의 늦은 종료 신호가 오더라도 현재 turn 상태를 뒤집지 않는다.
```

timeout은 tool runtime이 감지할 수 있지만, 그것을 어떻게 해석해 턴을 계속할지 중단할지는 오케스트레이터가 결정한다.

---

## 구현 불변식

아래 불변식은 future Rust 구현에서 타입 경계, 테스트, assertion으로 강제 대상이다.

1. tool runtime은 세션 상태를 직접 수정할 수 없다.
2. tool runtime은 event를 직접 append할 수 없다.
3. 모든 tool 실행은 오케스트레이터가 만든 `Effect::RunTool`에서만 시작된다.
4. 모든 tool 결과는 재진입 command로만 돌아와야 한다.
5. `effect_id`, `tool_call_id`, `session_id`, `turn_id` 없는 tool 결과는 채택되면 안 된다.
6. permission 거절은 tool runtime의 우회 경로를 만들면 안 된다.
7. timeout, failure, cancellation은 서로 구분되는 결과 상태로 표현되어야 한다.
8. 종료된 turn에 대한 late result는 세션의 공식 결과를 되살리면 안 된다.
9. tool 결과는 오케스트레이터가 채택하기 전까지 공식 대화 기록이 아니다.
10. tool runtime은 결과를 보고 다음 tool 또는 provider 호출을 독자적으로 시작하면 안 된다.
11. registry는 tool 존재와 실행 계약을 설명할 수 있어야 하지만 상태 권한을 가지면 안 된다.
12. tool이 아무리 강력해도 오케스트레이터 권한을 우회하는 직접 경로를 가져서는 안 된다.

---

## forbidden patterns

### 1. tool runtime의 직접 상태 반영

금지 예:

- read 결과를 곧바로 conversation history에 append
- write 성공 후 session store에 "파일 수정 완료"를 직접 기록

왜 금지인가:

- 메인 오케스트레이터 단일 권한 원칙이 깨진다.
- late result와 중복 결과를 안전하게 처리할 수 없다.

### 2. runtime 내부의 독자적 permission 승격

금지 예:

- `plan` mode인데 runtime이 "이 정도는 안전하다"고 판단해 write 실행
- shell tool이 자체 확인 프롬프트를 띄워 실행 허용

왜 금지인가:

- 정책 진실 원천이 분산된다.
- 같은 세션을 replay해도 동일 결과를 보장하기 어렵다.

### 3. effect 없는 tool 실행

금지 예:

- CLI helper가 오케스트레이터를 거치지 않고 read tool 직접 호출
- provider adapter가 모델 출력에 따라 tool runtime을 바로 부름

왜 금지인가:

- correlation과 causation이 끊긴다.
- event log로 상태 전이를 설명할 수 없게 된다.

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

- 먼저 `ToolCallTimedOut`을 보냈다가 뒤늦은 실제 완료를 정상 성공으로 다시 채택

왜 금지인가:

- 종료된 판단을 뒤집어 세션 재현성이 깨진다.

---

## Rust 구현으로 이어질 체크포인트

아래 질문에 모두 "예"라고 답할 수 있어야 한다.

- `ToolDefinition`, `RunToolEffect`, `ToolCallOutcome`, `ToolReentryCommand`가 분리된 타입인가?
- tool registry 조회와 실제 실행이 분리된 책임인가?
- permission 판단은 오케스트레이터 계층에서 먼저 일어나는가?
- 모든 결과가 `completed`, `failed`, `timed_out`, `cancelled` 중 하나로 정규화되는가?
- late result와 duplicate result를 idempotent하게 무시하는 테스트를 작성할 수 있는가?
- 큰 출력이 세션 상태를 오염시키지 않도록 참조 또는 요약 규칙이 있는가?
- 어떤 tool도 오케스트레이터 승인 없이 직접 실행될 수 없는가?

하나라도 "아니오"라면 tool runtime 경계가 흐려졌을 가능성이 높다.

---

## 테스트 관점에서 꼭 검증할 시나리오

future Rust 구현은 최소한 아래를 테스트로 가져가야 한다.

- 허용된 read tool이 effect를 거쳐 reentry command로만 반영되는가
- `plan` mode에서 write tool 제안이 effect 생성 이전에 거절되는가
- timeout 결과가 `timed_out`으로 정규화되는가
- 중복 `ToolCallCompleted`가 한 번만 채택되는가
- 종료된 turn에 늦게 도착한 tool 결과가 무시되는가
- registry에 없는 tool 요청이 panic이 아니라 정규화된 실패로 돌아오는가
- tool runtime이 세션 상태 수정 API에 접근할 수 없도록 모듈 경계가 막혀 있는가

---

## 결론

`shacs-bot`의 tool runtime은 강한 실행기처럼 보여도 권한자는 아니다. tool은 오케스트레이터가 발행한 effect 안에서만 실행되고, 결과는 정규화된 재진입 command로만 돌아오며, 어떤 경우에도 오케스트레이터 권한을 우회해 상태를 바꾸지 못해야 한다.

즉 이 문서의 핵심은 하나다. tool runtime은 바깥 실행 경계이고, 상태 권한은 끝까지 `MainOrchestrator`에 남는다.
