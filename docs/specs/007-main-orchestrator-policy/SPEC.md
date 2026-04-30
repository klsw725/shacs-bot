# main orchestrator policy 아키텍처 명세

## 문서 목적

이 문서는 `docs/SYSTEM-FOUNDATION.md`, `docs/specs/001-session-kernel/SPEC.md`, `docs/specs/002-command-event-effect/SPEC.md`, `docs/specs/003-provider-runtime/SPEC.md`, `docs/specs/004-tool-runtime/SPEC.md`, `docs/specs/006-session-store/SPEC.md`를 바탕으로 `shacs-bot`의 `MainOrchestrator` 정책 계층을 구현 가능한 수준으로 고정한다.

목표는 다음과 같다.

- 정책 소유권이 어디에 있고 어디에 있지 않은지 고정한다.
- retry, abort, timeout, late result, approval, selection 판단 규칙을 결정표 수준으로 명시한다.
- 어떤 정책 정보가 durable하고 어떤 정보가 turn-local인지 구분한다.
- future Rust 구현에서 policy enum, decision API, snapshot 구조, 테스트 시나리오를 직접 도출할 수 있게 한다.

이 문서는 방향 제안이 아니라 구현 계약이다. 구현이 이 문서와 충돌하면 코드를 우선 밀어붙이지 말고 정책 의미론부터 다시 점검해야 한다.

이 spec의 완료 기준은 단순히 if 문 몇 개로 분기하는 POC가 아니라, 이 문서가 정의한 정책 소유권, 결정 시점, durable/turn-local 경계, late result 처리, 승인 모델, 실패 처리 규칙을 충족하는 **완전한 기능 구현과 검증**이다.

---

## 상위 기준과의 관계

이 문서는 다음 상위 기준을 전제로 한다.

- `MainOrchestrator`는 세션 상태를 변경할 수 있는 유일한 권한자다.
- provider runtime, tool runtime, session store는 실행 경계 또는 저장 경계일 뿐 정책 권한자가 아니다.
- 세션의 단일 턴 정확성과 deterministic resume이 확장성보다 우선이다.
- 목표는 self-hosted / personal-use 환경에서 사용자가 직접 설치하고 운영하는 단일 사용자 런타임이다.

따라서 이 문서의 정책 모델은 멀티유저 승인 워크플로우, 관리자 콘솔 승인 체인, 분산 합의 기반 리더 선출, 조직별 정책 배포 체계를 다루지 않는다.

이 문서가 말하는 정책은 중앙 집중형이다. `MainOrchestrator`는 policy owner이며, 바깥 시스템은 policy input을 제공하거나 policy decision을 집행할 뿐 정책을 확정하지 않는다.

---

## 범위

이 문서는 다음을 정의한다.

- policy ownership과 책임 경계
- policy state의 durable / turn-local 구분
- approval, selection, retry, abort, timeout, late result에 대한 결정 시점
- policy snapshot이 effect에 어떻게 내려가는지
- recovery 이후 정책 재구성 원칙
- 구현 불변식, 정상 시퀀스, 실패 시퀀스, 금지 패턴, Rust 체크포인트, 테스트 관점

이 문서는 다음을 정의하지 않는다.

- 특정 UI에서 승인을 어떻게 보여줄지
- provider별 세부 매개변수 목록
- tool 내부 알고리즘이나 shell sandbox 세부 구현
- 장기 통계 기반 자동 최적화 정책
- 멀티유저 역할 기반 접근 제어

---

## 핵심 정의

### orchestrator policy

orchestrator policy는 `MainOrchestrator`가 한 턴을 진행하면서 내리는 승인과 거절의 기준 집합이다. 이 정책은 단순 설정 값의 모음이 아니라, 아래 질문들에 대한 최종 판단 계약이다.

- 지금 새 턴을 받을 수 있는가
- 이 provider 호출을 retry 해야 하는가
- 이 tool call을 승인할 수 있는가
- 이 결과가 너무 늦게 도착했는가
- 이 턴을 계속 진행할 것인가, 중단할 것인가
- 어떤 provider profile, tool set, skill set, timeout budget을 선택할 것인가

### policy owner

policy owner는 정책 판단을 확정하고 그 결과를 세션 상태와 event로 남길 수 있는 주체다. `shacs-bot`에서 policy owner는 `MainOrchestrator` 하나뿐이다.

### policy input

policy input은 정책 판단에 참고되는 정보다. 예:

- 세션의 durable policy mode
- 현재 turn phase와 retry count
- tool capability와 args
- provider/tool effect의 correlation 정보
- timeout 예산
- 사용자의 명시적 승인 또는 취소 요청

policy input은 판단 재료일 뿐이다. 그 자체가 정책 결정은 아니다.

### policy snapshot

policy snapshot은 오케스트레이터가 특정 effect를 발행하는 시점에 확정한 정책 판단의 복사본이다. 예:

- tool execution envelope 안의 `permission_snapshot`
- provider invocation envelope 안의 `provider_profile`, `timeout_ms`, `tool_schema_snapshot`

snapshot은 실행기가 준수해야 할 계약이지만, snapshot이 policy owner를 대체하지는 않는다.

### durable policy state

durable policy state는 턴이 끝난 뒤에도 다음 턴 해석과 resume correctness에 직접 필요한 정책 상태다.

### turn-local policy state

turn-local policy state는 현재 열린 턴을 닫는 동안만 의미가 있는 임시 판단 상태다. 턴이 `completed` 또는 `aborted`로 닫히면 다음 턴으로 그대로 승계되면 안 된다.

---

## 정책 소유권과 책임 경계

### `MainOrchestrator`가 반드시 소유해야 하는 정책 판단

`MainOrchestrator`는 최소한 다음 정책 판단의 유일한 권한자여야 한다.

- 새 턴 수용 여부
- provider profile 선택 여부
- 모델 재호출 여부와 retry 여부
- tool 실행 승인 또는 거절
- subagent spawn 승인 또는 거절
- approval 대기 여부와 승인 재진입 처리
- timeout 이후 재시도, 중단, late result 폐기 여부
- 이미 닫힌 턴으로 들어온 결과의 무시 또는 관찰 이벤트화 여부
- compact 필요성 판단
- recovery 시 열린 턴 정리 방향

### 바깥 구성요소가 할 수 있는 정책 관련 행동

바깥 구성요소는 다음만 할 수 있다.

- 설정 값을 읽어 policy input으로 제공
- effect를 snapshot대로 실행
- 승인 요청 UI를 표시하고 사용자 의사를 `Command`로 되돌림
- timeout, failure, cancellation, late arrival 사실을 결과로 보고

### 바깥 구성요소가 해서는 안 되는 행동

- tool runtime이 위험도가 낮다고 판단해 승인 없이 실행
- provider runtime이 이전 실패를 근거로 독자적으로 model fallback 수행
- session store가 recovery 중 열린 턴을 자동 성공 처리
- UI 계층이 승인 응답을 event처럼 조작해 세션에 직접 반영

정책이 여러 계층으로 흩어지면 재현 가능성과 설명 가능성이 무너진다. 따라서 모든 확정 판단은 오케스트레이터 중심으로 수렴해야 한다.

---

## 정책 상태 모델

### durable로 유지해야 하는 정책 상태

다음 정보는 `SessionState` 또는 session store replay 결과로 복원 가능한 durable policy state여야 한다.

- 세션 수준 permission mode, 예: `default`, `auto`, `plan`
- 현재 활성 provider profile 또는 기본 provider selection 규칙
- 세션에 적용되는 기본 timeout policy
- 기본 retry ceiling과 retry 허용 범주
- 사용자가 명시적으로 선택한 skill set 또는 skill selection policy
- late result를 어떤 수준까지 관찰 이벤트로 남길지에 대한 세션 정책
- compact 후에도 보존해야 하는 작업 목적과 정책 관련 핵심 메타데이터

이 정보는 다음 턴에서도 의미를 가지며, resume 이후에도 같은 판단 기준을 재구성해야 한다.

### turn-local로만 유지해야 하는 정책 상태

다음 정보는 `TurnState` 또는 effect correlation 영역에만 존재해야 한다.

- 현재 턴의 provider retry count
- 현재 턴의 tool retry count
- 승인 대기 중인 specific effect id와 approval request id
- 현재 턴에서만 유효한 timeout deadline
- 이전 effect를 late result로 판정하기 위한 active correlation set
- 이번 턴의 selection rationale, 예: 왜 특정 profile이나 스킬이 선택되었는지에 대한 임시 설명 메모

이 정보는 현재 턴을 닫는 데만 필요하다. 턴 종료 후 그대로 durable state로 승격하면 안 된다.

### 경계 판단 규칙

어떤 정책 필드가 durable인지 turn-local인지 애매하면 아래 질문으로 판단한다.

> 이 값이 턴이 닫힌 뒤에도 다음 턴의 승인 기준 또는 deterministic resume에 직접 필요한가?

- 그렇다 → durable policy state
- 아니다, 현재 턴의 특정 effect나 retry 흐름을 닫는 데만 필요하다 → turn-local policy state

---

## 정책 판단 시점

정책은 아무 때나 평가하면 안 된다. 각 판단은 고정된 시점에서 일어나야 한다.

### 새 턴 수용 정책

- 평가 시점: `Command`를 받아 `accepted`로 들어가기 직전
- 참고 입력: 세션 lifecycle 상태, 열린 턴 유무, recovery 중 여부, command 종류
- 출력: 수용, 거절, 지연

> 참고 메모: 새 턴 수용 판단은 user input, approval response, service wake, late reentry가 같은 세션에 경쟁적으로 들어오는 ingress arbitration과 직접 맞물린다.
> 이 문서는 owner 역할을 전제로 하지만, 구체 우선순위와 직렬화 규칙은 교차 문서 관점에서 더 명시될 여지가 있다.

### selection 정책

- 평가 시점: `context_building` 진입 시점
- 참고 입력: durable policy state, 사용자 요청 종류, skill registry snapshot, provider profile registry
- 출력: 이번 턴의 provider profile, tool schema 범위, skill set, token budget 초안

### approval 정책

- 평가 시점: 모델이 tool call 또는 위험 작업 후보를 제안했을 때, effect 발행 직전
- 참고 입력: permission mode, tool capability, args, path scope, 현재 턴 상태
- 출력: 승인, 승인 필요, 즉시 거절

### retry / abort 정책

- 평가 시점: provider/tool/subagent 결과가 실패, timeout, cancellation, parse error, policy rejection으로 재진입했을 때
- 참고 입력: 결과 상태, retry count, effect kind, failure kind, 턴 phase, 사용자 취소 여부
- 출력: retry, abort, alternate selection, 관찰 이벤트만 기록

### late result 정책

- 평가 시점: 외부 결과가 재진입했을 때
- 참고 입력: `session_id`, `turn_id`, `effect_id`, 현재 열린 턴, active correlation set, 이미 닫힌 여부
- 출력: 수용 가능, late result로 폐기, 관찰 이벤트 기록 후 폐기

### compact 정책

- 평가 시점: 턴이 닫힌 직후 또는 다음 턴 시작 전 유지보수 단계
- 참고 입력: 세션 기록 길이, token pressure, 최근 checkpoint 유무, 열린 턴 없음 여부
- 출력: compact 실행, checkpoint만 생성, 아무 것도 안 함

---

## 결정표

### 1. 새 턴 수용 결정표

| 조건 | 결정 | 비고 |
| --- | --- | --- |
| 세션이 active이고 열린 턴이 없음 | 수용 | 새 `TurnState` 생성 가능 |
| 열린 턴이 있음 | 거절 또는 대기 | 기본 구현은 거절 또는 명시적 대기, 병행 수용 금지 |
| 세션이 recovery 중이고 열린 턴 정리 전 | 거절 | recovery가 먼저 끝나야 함 |
| 세션이 aborted/finalized 상태 | 거절 | 새 세션 또는 명시적 resume 정책 필요 |

### 2. approval 결정표

| permission mode | capability | 추가 조건 | 결정 |
| --- | --- | --- | --- |
| `plan` | `fs_read` | 경로 범위 허용 | 승인 가능 |
| `plan` | `fs_write`, `proc_exec`, `net_outbound`, `secret_read` | 무관 | 즉시 거절 |
| `default` | `fs_read` | 경로 범위 허용 | 승인 가능 |
| `default` | `fs_write`, `proc_exec`, `net_outbound`, `secret_read` | 사용자 승인 없음 | approval 필요 |
| `default` | `fs_write`, `proc_exec`, `net_outbound`, `secret_read` | 명시적 승인 수신 | 승인 가능 |
| `auto` | 모든 capability | 정책 범위 안 | 승인 가능 |
| 모든 mode | 모든 capability | 경로/리소스 범위 초과 | 즉시 거절 |

### 3. provider 결과 후 retry 결정표

| 결과 상태 | 조건 | 결정 |
| --- | --- | --- |
| `failed` | retryable failure, retry ceiling 미도달 | retry |
| `failed` | 인증/구성 오류 | abort |
| `timed_out` | timeout retry 허용, retry ceiling 미도달 | retry 또는 더 낮은 budget/profile로 재선택 |
| `cancelled` | 사용자 취소 | abort |
| `tool_requested` | approval 필요 | approval 대기 또는 거절 |
| `completed` | assistant candidate 유효 | result applying |

### 4. tool 결과 후 retry 결정표

| 결과 상태 | 조건 | 결정 |
| --- | --- | --- |
| `completed` | 결과 payload 유효 | context rebuilding 또는 result applying |
| `failed` | 입력/권한 불일치 | abort |
| `failed` | 일시적 I/O 실패, retry 허용 | retry |
| `timed_out` | retry ceiling 미도달 | retry 또는 abort |
| `cancelled` | 사용자 취소 또는 상위 abort | abort |

### 5. late result 결정표

| 재진입 결과 | 현재 상태 | 결정 |
| --- | --- | --- |
| `effect_id`가 active correlation과 일치 | 열린 동일 턴 존재 | 정상 평가 가능 |
| 같은 턴이지만 이미 새 retry effect가 active | late result 폐기 |
| 턴이 이미 `completed` 또는 `aborted` | late result 폐기 |
| `session_id` 불일치 또는 `turn_id` 불일치 | 즉시 거절 |
| recovery 후 과거 effect 결과 도착 | 관찰 이벤트 optional 후 폐기 |

### 6. approval 응답 결정표

| 승인 응답 | 상관관계 유효성 | 결정 |
| --- | --- | --- |
| 승인 | approval request id 유효 | 해당 effect 발행 |
| 거절 | approval request id 유효 | 턴 abort 또는 다른 계획으로 축소 |
| 응답 없음 | deadline 경과 | approval timeout으로 abort 또는 거절 |
| 승인/거절 | 이미 턴 종료 | late approval로 폐기 |

---

## selection 정책

selection은 단순 추천이 아니라, 이번 턴 실행에 들어갈 공식 입력 집합을 고르는 정책 단계다.

### selection 대상

- provider profile
- model id 또는 model tier
- 사용할 skill set
- 노출할 tool schema 집합
- timeout budget
- max output token budget
- compaction 필요 여부

### selection 원칙

1. selection 결과는 결정적이어야 한다.
2. 같은 durable state와 같은 turn input이면 같은 selection 결과가 나와야 한다.
3. selection은 바깥 executor가 아니라 오케스트레이터가 수행한다.
4. selection 결과는 effect 발행 전에 snapshot으로 굳어야 한다.
5. selection 결과는 현재 턴에만 적용되는 부분과 세션 기본 정책을 구분해야 한다.

### selection에서 허용되지 않는 것

- provider runtime이 벤더 오류를 이유로 자체 fallback profile을 선택
- tool runtime이 실제 실행 시점에 노출 tool schema를 임의 확장
- skill loader가 스킬 본문을 읽다가 session mode를 자동 변경

---

## 승인 모델

### approval의 의미

approval은 사용자의 확인이 필요한 작업 후보를 오케스트레이터가 보류 상태로 두고, 그 결과를 기다리는 정책 단계다.

approval은 effect 자체가 아니다. approval은 effect 생성 전의 정책 게이트다.

### approval lifecycle

1. 모델이 tool call 또는 위험 작업 후보를 제안한다.
2. 오케스트레이터가 approval 필요성을 판정한다.
3. 필요하면 `ApprovalRequested`에 준하는 event를 남기고 turn-local approval state를 생성한다.
4. 인터페이스 계층은 이를 보여주고 사용자의 응답을 `Command`로 재진입시킨다.
5. 오케스트레이터는 응답과 correlation을 검증한다.
6. 승인되면 effect를 발행하고, 거절되면 턴을 abort 또는 축소 계획으로 전환한다.

### approval state에 durable로 남기면 안 되는 것

- UI 핸들
- OS 알림 토큰
- transport connection id
- 화면 렌더링 위치 정보

approval state에 durable로 남길 수 있는 것은 "이 턴이 승인 거절로 중단되었다" 같은 결과 사실뿐이다.

---

## retry, abort, timeout 정책

### retry 기본 원칙

- retry는 자동 반사 동작이 아니라 정책 판단이다.
- retry는 effect kind별 ceiling을 가져야 한다.
- retry는 동일 입력을 무한 반복하면 안 된다.
- retry 전에는 이전 effect가 더 이상 active하지 않도록 correlation 집합을 갱신해야 한다.

### retry 가능한 실패 예시

- provider 네트워크 실패
- provider 일시적 timeout
- tool의 일시적 I/O 오류
- rate limit 성격의 재시도 가능 오류

### retry 불가능한 실패 예시

- malformed tool args
- permission 거절
- config/profile 미발견
- 사용자 명시적 취소
- 불변식 위반

### abort 기본 원칙

다음 중 하나면 기본 정책은 abort다.

- retry ceiling 소진
- 승인 거절
- 세션 정책상 금지된 capability 요청
- 회복 불가능한 config/provider/tool registry 오류
- correlation 위반 또는 상태 불일치

### timeout 정책

- timeout은 executor가 보고하는 사실이지만, timeout 이후 retry/abort 선택은 오케스트레이터가 한다.
- timeout은 즉시 성공으로 승격될 수 없다.
- timeout 이후 도착한 결과는 active correlation과 대조해 late result 여부를 판정해야 한다.

---

## late result 정책

late result는 "실행이 끝났는가"가 아니라 "그 결과를 지금도 공식 입력으로 받아들일 수 있는가"의 문제다.

### late result 판정 기준

아래 중 하나라도 참이면 late result다.

- 해당 `effect_id`가 더 이상 active set에 없다.
- 해당 `turn_id`가 이미 닫혔다.
- 같은 logical step에 대해 더 새로운 retry effect가 active 또는 완료되었다.
- recovery 이후 이전 프로세스의 잔여 결과가 도착했다.

### late result 처리 규칙

1. late result는 공식 assistant 응답 또는 공식 tool result로 승격되면 안 된다.
2. late result는 필요하면 observability용 event로 남길 수 있다.
3. late result가 있더라도 이미 닫힌 턴 결과는 바뀌면 안 된다.
4. late result는 retry count를 되돌리거나 approval 상태를 되살리면 안 된다.

---

## 정상 시퀀스 예시

### 예시 1. approval이 필요한 write tool 요청

```text
1) 사용자가 파일 수정 요청을 보낸다.
2) MainOrchestrator는 세션이 active이고 열린 턴이 없음을 확인한 뒤 턴을 accepted로 연다.
3) context_building에서 provider profile과 skill set을 선택한다.
4) provider effect를 발행한다.
5) provider가 `fs_write` capability를 요구하는 tool call 후보를 반환한다.
6) 오케스트레이터는 permission mode=default, capability=`fs_write`를 확인하고 approval 필요로 판정한다.
7) 오케스트레이터는 approval state를 turn-local로 기록하고 승인 요청 event를 남긴다.
8) 인터페이스가 사용자 승인을 받아 ApprovalGranted command를 되돌린다.
9) 오케스트레이터는 approval request id와 turn correlation을 검증한다.
10) 검증이 통과하면 tool effect를 발행한다.
11) tool runtime이 결과를 돌려주면 오케스트레이터가 이를 반영해 턴을 완료한다.
```

핵심은 approval UI가 아니라 오케스트레이터가 승인 필요성을 확정하고, 승인 응답도 다시 정책 검증을 거친다는 점이다.

### 예시 2. provider timeout 후 retry

```text
1) provider effect가 발행된다.
2) provider runtime이 timeout 결과를 재진입시킨다.
3) 오케스트레이터는 현재 retry count와 timeout policy를 확인한다.
4) retry ceiling 미도달이며 timeout retry 허용 상태이므로 retry를 선택한다.
5) 이전 effect_id는 inactive로 마킹한다.
6) 새 provider effect를 발행한다.
7) 이후 이전 effect 결과가 늦게 도착하면 late result로 폐기한다.
```

---

## 실패 및 중단 시퀀스 예시

### 예시 3. approval timeout

```text
1) write tool 후보가 생성된다.
2) 오케스트레이터는 approval request를 만든다.
3) 지정된 approval deadline 안에 사용자 응답이 오지 않는다.
4) 오케스트레이터는 approval timeout을 감지한다.
5) 정책상 자동 승인으로 전환하지 않는다.
6) 턴은 approval timeout 사유로 aborted 된다.
7) 이후 늦게 도착한 ApprovalGranted command는 late approval로 폐기된다.
```

### 예시 4. retry 이후 이전 결과 도착

```text
1) provider effect P1이 timeout 된다.
2) 오케스트레이터는 retry를 선택해 P2를 발행한다.
3) P2는 정상 결과를 반환해 턴이 completed 된다.
4) 그 뒤 P1의 응답이 도착한다.
5) 오케스트레이터는 P1의 effect_id가 inactive이고 turn이 이미 닫혔음을 확인한다.
6) P1 결과는 late result로 폐기되며 세션 기록을 바꾸지 않는다.
```

---

## 구현 불변식

아래 불변식은 future Rust 구현에서 타입, assertion, 테스트로 강제 대상이다.

1. 정책 확정 권한은 `MainOrchestrator` 하나뿐이다.
2. durable policy state와 turn-local policy state는 구분 가능해야 한다.
3. approval이 필요한 작업은 승인 없이 effect로 내려가면 안 된다.
4. retry 여부는 executor가 아니라 오케스트레이터가 결정한다.
5. late result는 이미 닫힌 턴의 공식 결과를 바꾸면 안 된다.
6. retry 후 이전 effect는 active correlation set에서 제거되어야 한다.
7. approval 응답은 approval request id와 turn correlation이 맞을 때만 유효하다.
8. timeout은 성공 결과로 암묵 승격되면 안 된다.
9. selection 결과는 effect 발행 전에 snapshot으로 고정되어야 한다.
10. recovery 이후 열린 턴 정리 방향 역시 정책 판단으로 명시되어야 한다.

---

## 금지 패턴

### 1. executor가 정책 fallback을 확정

금지 예:

- provider runtime이 기본 profile 실패 후 자체적으로 다른 모델로 재시도
- tool runtime이 approval 없이 low-risk write를 허용

왜 금지인가:

- policy ownership이 분산된다.
- replay와 observability가 깨진다.

### 2. turn-local policy state를 durable state로 승격

금지 예:

- 특정 턴의 retry count를 세션 영속 정책에 그대로 저장
- 만료된 approval request id를 다음 턴까지 유지

왜 금지인가:

- 다음 턴 해석이 오염된다.
- resume 후 의미가 달라진다.

### 3. late result를 부분 성공처럼 반영

금지 예:

- 이미 완료된 턴 뒤에 도착한 tool 결과를 conversation에 덧붙임
- retry 이전 결과의 usage 정보를 최신 성공 호출 통계에 섞음

왜 금지인가:

- 닫힌 턴의 공식 기록이 변형된다.
- 어떤 결과가 실제로 채택되었는지 설명할 수 없어진다.

### 4. approval을 transport 상태에 묶기

금지 예:

- 특정 WebSocket 연결이 살아 있어야만 승인 응답을 인식하는 구조
- UI modal 객체 포인터를 turn state에 저장

왜 금지인가:

- self-hosted 로컬 복구성이 떨어진다.
- 인터페이스 구현이 정책 계층을 오염시킨다.

### 5. selection을 비결정적 휴리스틱에 숨기기

금지 예:

- 같은 세션 상태인데 현재 시각이나 랜덤 값에 따라 provider profile이 바뀜
- 파일 시스템 재스캔 타이밍에 따라 같은 턴의 skill set이 바뀜

왜 금지인가:

- 같은 입력에서 같은 정책 판단이 나오지 않는다.
- 테스트 가능성과 재현성이 떨어진다.

---

## Rust 구현으로 이어질 체크포인트

구체 타입 이름은 바뀔 수 있지만, 아래 질문에는 모두 "예"라고 답할 수 있어야 한다.

- `PolicyState`, `PolicySnapshot`, `ApprovalState`, `RetryDecision`, `LateResultDecision` 같은 경계가 타입 수준에서 구분되는가?
- durable policy state와 turn-local state를 별도 필드 또는 별도 타입으로 나눌 수 있는가?
- effect 발행 전 selection과 approval 판단이 명시적 함수 호출로 드러나는가?
- retry ceiling과 failure kind를 기준으로 `RetryDecision`을 테스트할 수 있는가?
- approval request id와 effect/turn correlation 검증 로직이 있는가?
- late result 판정을 독립 함수나 모듈로 분리해 테스트할 수 있는가?
- recovery 시 열린 턴 정리 정책이 하드코딩된 부수효과가 아니라 명시적 결정으로 표현되는가?

이 질문 중 하나라도 "아니오"라면, 정책 계층이 오케스트레이터 밖으로 새고 있을 가능성이 높다.

---

## 테스트 관점에서 꼭 검증할 시나리오

future Rust 구현은 최소한 아래 시나리오를 테스트로 가져갈 수 있어야 한다.

- `plan` mode에서 write/exec/network tool 후보가 즉시 거절되는가
- `default` mode에서 write tool 후보가 approval 없이는 effect로 내려가지 않는가
- approval 승인 후에만 해당 tool effect가 발행되는가
- approval deadline 경과 시 자동 승인 없이 턴이 중단되는가
- provider timeout 후 retry ceiling 미도달이면 retry가 선택되는가
- retry 후 이전 effect 결과가 late result로 폐기되는가
- turn-local retry count가 턴 종료 후 durable 세션 상태로 남지 않는가
- recovery 이후 crash 전 effect 결과가 도착해도 닫힌 턴을 되살리지 않는가
- 동일한 durable state와 동일한 입력에서 동일한 selection 결과가 나오는가

---

## 명시적 비범위

이 문서는 다음을 정의하지 않는다.

- 승인 UI 화면 배치나 인터랙션 디자인
- 머신러닝 기반 정책 최적화
- 멀티유저 승인 체계와 역할 관리
- 외부 정책 서버 연동
- 원격 운영자 콘솔

이 항목들은 필요가 생기면 별도 문서에서 다룬다. 단, 어떤 확장도 `MainOrchestrator` 단일 권한 원칙과 durable/turn-local 경계를 뒤집어서는 안 된다.

---

## 결론

`shacs-bot`의 main orchestrator policy는 설정 값의 모음이 아니라, 한 턴 안에서 무엇을 승인하고 무엇을 중단하며 무엇을 늦은 결과로 버릴지를 일관되게 결정하는 중앙 정책 계층이다.

핵심은 세 가지다.

- 정책 확정 권한은 끝까지 `MainOrchestrator`에 남아 있어야 한다.
- durable policy state와 turn-local decision state는 엄격히 분리되어야 한다.
- retry, abort, timeout, approval, late result, selection은 모두 설명 가능한 결정표와 correlation 규칙 위에서만 동작해야 한다.

이 구조가 지켜져야 `shacs-bot`은 self-hosted 단일 사용자 런타임으로서 예측 가능하고, 복구 가능하고, 왜 그런 판단이 나왔는지 끝까지 설명할 수 있다.
