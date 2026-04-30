# runtime services 아키텍처 명세

## 문서 목적

이 문서는 `docs/SYSTEM-FOUNDATION.md`, `docs/specs/001-session-kernel/SPEC.md`, `docs/specs/002-command-event-effect/SPEC.md`, `docs/specs/006-session-store/SPEC.md`, `docs/specs/007-main-orchestrator-policy/SPEC.md`, `docs/specs/011-subagent-runtime/SPEC.md`를 바탕으로 `shacs-bot`의 runtime services 경계를 구현 가능한 수준으로 고정한다.

목표는 다음과 같다.

- queue, scheduler, mailbox, hooks, background worker가 무엇을 소유하고 무엇을 소유하지 않는지 정의한다.
- 각 서비스가 어떤 command를 emit할 수 있고 어떤 command는 emit하면 안 되는지 고정한다.
- 서비스 메타데이터와 session truth의 경계를 명시한다.
- dedupe, retry, wake/resume, failure-safe reentry 규칙을 결정한다.
- future Rust 구현에서 service adapters, wake command, dedupe key, retry state, 테스트 시나리오를 직접 도출할 수 있게 한다.

이 문서는 주변 서비스를 나열하는 개요 문서가 아니다. 구현이 이 문서와 충돌하면 서비스 편의상 오케스트레이터를 우회하지 말고, 서비스 경계와 재진입 계약부터 다시 점검해야 한다.

이 spec의 완료 기준은 크론이나 큐 하나를 붙여보는 POC가 아니라, 이 문서가 정의한 service boundary, command emission rule, metadata/session truth 구분, dedupe/retry/wake/reentry 규칙을 충족하는 **완전한 기능 구현과 검증**이다.

---

## 상위 기준과의 관계

이 문서는 다음 상위 기준을 전제로 한다.

- `MainOrchestrator`는 세션 상태를 바꿀 수 있는 유일한 권한자다.
- queue, scheduler, mailbox, hooks, background worker는 코어 밖 서비스지만 정책 권한자는 아니다.
- 서비스는 외부 자극을 command로 정규화해 재진입시키거나 effect를 집행할 뿐, 세션 truth를 직접 확정하지 않는다.
- 목표는 self-hosted / personal-use 환경에서 단일 사용자 세션을 안정적으로 깨우고 이어서 실행하는 것이다.

따라서 이 문서는 분산 작업 큐, 멀티노드 리더 선출, 조직 단위 inbox, 운영자 대시보드, 멀티테넌트 webhook fan-out을 다루지 않는다.

---

## 범위

이 문서는 다음을 정의한다.

- runtime services의 책임 경계
- queue, scheduler, mailbox, hooks, background worker의 공식 역할
- 각 서비스가 emit 가능한 command 집합
- 서비스 메타데이터와 세션 진실 원천의 구분
- dedupe key, retry semantics, idempotency 기대치
- wake/resume 동작과 recovery 규칙
- 모든 서비스 결과의 failure-safe reentry 원칙
- 구현 불변식, 정상 시퀀스, 실패 시퀀스, 금지 패턴, Rust 체크포인트, 테스트 관점

이 문서는 다음을 정의하지 않는다.

- 특정 큐 제품이나 cron 라이브러리 선택
- 이메일, IMAP, Slack, webhook 벤더 세부 프로토콜
- 관리자 inbox UI
- 멀티유저 작업 분배
- 분산 스케줄러 클러스터 합의

---

## 핵심 정의

### runtime service

runtime service는 세션 코어 밖에서 대기열, 시간 기반 깨움, 외부 메시지 수신, hook 실행, 장기 작업 감시 같은 보조 기능을 수행하는 서비스 컴포넌트다.

### service-owned metadata

service-owned metadata는 서비스가 자기 운영을 위해 들고 있어도 되는 부가 정보다. 예:

- queue delivery attempt count
- scheduler next fire time
- mailbox external message id
- hook registration revision
- worker heartbeat

이 값은 세션 truth가 아니다.

### session truth

session truth는 `SessionState`, `TurnState`, event log, checkpoint replay로 복원되는 공식 상태다. runtime service는 이를 직접 수정하거나 대체하면 안 된다.

### wake command

wake command는 scheduler, queue, mailbox, background worker 등이 "이 세션을 다시 살펴봐야 한다"는 사실을 오케스트레이터에 알리기 위해 발행하는 synthetic command다.

### failure-safe reentry

failure-safe reentry는 서비스가 실패, 중복 전달, 재시작, 지연, 재배달 상황에서도 결국 `MainOrchestrator`를 유일한 확정 경계로 사용하도록 강제하는 구조다.

---

## runtime services의 기본 원칙

1. 서비스는 command를 emit할 수 있어도 세션 truth를 직접 바꾸면 안 된다.
2. 모든 서비스 입력과 출력은 재진입 가능한 envelope를 가져야 한다.
3. dedupe와 idempotency는 서비스와 오케스트레이터가 함께 보장해야 한다.
4. service-owned metadata는 복구 보조물이지 공식 결과가 아니다.
5. wake/resume은 새 턴을 강제로 여는 것이 아니라, 오케스트레이터가 판단할 기회를 제공하는 것이다.
6. 서비스 failure는 가능하면 command 수준에서 관찰되고, 최종 abort/retry 판단은 오케스트레이터가 한다.

---

## 서비스별 책임 경계

### 1. queue

queue는 대기 중인 작업 단위를 저장하고 전달하는 서비스다.

queue가 할 수 있는 일:

- 미전달 작업 보관
- delivery attempt 추적
- next delivery scheduling 보조
- ack/nack 같은 전달 상태 관리

queue가 할 수 없는 일:

- 세션 상태 직접 수정
- 작업 성공 여부를 자체 확정
- 같은 세션에 새 턴을 강제 개시

### 2. scheduler

scheduler는 시간 기반 wake 신호를 생성하는 서비스다.

scheduler가 할 수 있는 일:

- 특정 시각 또는 간격 기준 wake 예약
- last fired / next fire metadata 유지
- clock drift나 missed fire 감지

scheduler가 할 수 없는 일:

- 스스로 세션 결과를 append
- timeout을 성공으로 승격
- 닫힌 세션을 자기 판단으로 재활성화

### 3. mailbox

mailbox는 외부 메시지, inbox, webhook 비슷한 외부 자극을 수집해 정규화하는 서비스다.

현재 제품 범위에서 필요한 외부 채널은 아래 네 가지면 충분하다.

- Slack
- Discord
- Telegram
- Email

Email mailbox 지원은 이미 추출된 메시지 필드(`source_id`, `external_message_id`, `from`, plain-text `text`, optional `subject`)를 정규화하는 adapter 경계까지만 포함하며, IMAP polling, SMTP sending, MIME parsing, OAuth, provider-specific mail API는 이 spec의 벤더 세부 프로토콜 제외 범위에 남긴다.

mailbox가 할 수 있는 일:

- 외부 메시지 수신
- external message id와 수신 시각 기록
- payload 검증과 정규화
- dedupe 후보 판정용 metadata 제공

mailbox가 할 수 없는 일:

- 외부 메시지를 바로 conversation history에 append
- 승인 응답을 검증 없이 반영
- 세션이 존재하지 않는데 임의 생성

### 4. hooks

hooks는 특정 event나 lifecycle에 연결된 부가 실행기다.

hooks가 할 수 있는 일:

- event를 구독
- read-only 관찰 또는 별도 effect 요청 후보 생성
- 결과를 command로 재진입

hooks가 할 수 없는 일:

- hook callback 안에서 세션 상태 직접 수정
- 메인 턴 흐름을 우회한 side-effect 확정
- 정책 fallback 확정

### 5. background worker

background worker는 장기 실행 중인 외부 작업, 재시도 대기 작업, service adapter를 감시하는 실행기다.

background worker가 할 수 있는 일:

- 외부 job 상태 polling
- timeout, cancellation, completion 사실 보고
- wake command 생성

background worker가 할 수 없는 일:

- job 결과를 공식 상태로 병합
- turn lifecycle을 직접 닫음
- late result를 독자적으로 채택

---

## emit 가능한 command 범위

runtime service는 임의 command를 만들면 안 된다. 서비스 종류별로 emit 가능한 command 범위를 가져야 한다.

### queue가 emit 가능한 command 예시

- `QueuedWorkReady`
- `QueuedWorkDeliveryFailed`
- `QueuedWorkCancelled`

### scheduler가 emit 가능한 command 예시

- `ScheduledWakeTriggered`
- `ScheduledWakeMissed`
- `ScheduledWakeCancelled`

### mailbox가 emit 가능한 command 예시

- `MailboxMessageReceived`
- `MailboxMessageRejected`
- `MailboxApprovalResponseReceived`

### hooks가 emit 가능한 command 예시

- `HookCompleted`
- `HookFailed`
- `HookObservationProduced`

### background worker가 emit 가능한 command 예시

- `BackgroundJobCompleted`
- `BackgroundJobFailed`
- `BackgroundJobTimedOut`
- `BackgroundJobCancelled`
- `BackgroundWakeRequested`

### emit 금지 규칙

아래 종류는 서비스가 직접 emit하면 안 된다.

- 이미 확정된 assistant message append를 의미하는 command
- 세션 상태를 직접 변경하는 내부 전용 command
- 승인 검증을 건너뛴 privileged command
- 현재 열리지도 않은 turn에 결과를 강제 적용하는 command

서비스는 사실을 보고해야지, 확정을 선언하면 안 된다.

---

## service-owned metadata vs session truth

### 서비스가 소유할 수 있는 메타데이터

- queue delivery receipt
- retry backoff state
- scheduler next fire timestamp
- mailbox external sender id, external message id
- hook subscription revision
- worker heartbeat, lease timestamp

### 서비스가 소유하면 안 되는 공식 상태

- 현재 열린 `TurnState`
- 공식 assistant 응답 본문
- approval 결과의 최종 진실
- tool/subagent/provider 결과의 채택 여부
- 세션 lifecycle state

### 경계 원칙

1. service metadata는 유실되거나 중복돼도 세션 truth 재구성이 가능해야 한다.
2. 세션 truth는 session store replay로 복원되어야 하며, 서비스 DB를 진실 원천으로 참조하면 안 된다.
3. 서비스가 가진 외부 id는 dedupe와 관찰에 쓰일 수 있지만, 공식 대화 사실을 대체하면 안 된다.

---

## dedupe와 retry semantics

### dedupe 기본 원칙

중복 전달은 예외가 아니라 정상 가능성으로 간주해야 한다.

각 서비스는 최소한 아래 수준의 dedupe key를 가져야 한다.

- queue: `queue_item_id` 또는 전달용 고유 id
- scheduler: `schedule_id + fire_sequence`
- mailbox: `source_id + external_message_id`
- hooks: `hook_run_id`
- background worker: `job_id + attempt_sequence`

### dedupe 처리 규칙

1. 서비스는 같은 key의 중복 command를 여러 번 보낼 수 있다.
2. 오케스트레이터는 correlation과 이미 처리한 delivery marker를 기준으로 idempotent하게 처리해야 한다.
3. 중복 command가 와도 이미 닫힌 턴을 다시 열면 안 된다.

### retry semantics

서비스 retry는 전달 재시도일 뿐, 세션 정책 retry와 동일하지 않다.

- queue retry는 메시지 전달 재시도다.
- scheduler retry는 missed fire 보정이다.
- mailbox retry는 외부 poll 또는 fetch 재시도다.
- worker retry는 status poll 재시도다.

오케스트레이터가 보는 retry 판단은 항상 별도여야 한다.

---

## wake / resume 동작

runtime services는 세션을 깨울 수는 있어도, 세션을 어떻게 이어갈지는 결정하지 않는다.

### wake 규칙

1. wake command는 "확인 필요" 사실만 전달해야 한다.
2. wake command가 왔다고 해서 반드시 새 턴이 열리는 것은 아니다.
3. 오케스트레이터는 현재 세션 lifecycle, 열린 턴, pending effect, stale 여부를 확인한 뒤만 resume 또는 ignore를 결정한다.

### resume 규칙

1. 세션 replay와 열린 턴 복원이 먼저다.
2. 서비스가 들고 있던 metadata만으로 resume correctness를 확정하면 안 된다.
3. 이전 프로세스의 잔여 서비스 신호는 stale wake가 될 수 있다.

### wake source 예시

- 스케줄된 알림 시간 도래
- background job 완료
- mailbox 승인 응답 수신
- queue에 재처리 대상 작업 준비

---

## failure-safe reentry

모든 서비스는 실패하더라도 오케스트레이터 재진입 경계를 우회하면 안 된다.

### 기본 규칙

1. 서비스 결과는 command envelope로 정규화되어야 한다.
2. command에는 최소한 `session_id`, 관련 `turn_id` 또는 wake target, service correlation id가 포함되어야 한다.
3. 오케스트레이터는 command 수신 시 correlation, dedupe, stale 여부를 검증해야 한다.
4. 검증 실패 시 세션 truth를 바꾸지 않고 거절 또는 관찰 이벤트만 남긴다.

### 서비스 재시작 이후 규칙

- 서비스가 재시작되면 이전 미확인 delivery를 다시 보낼 수 있다.
- 오케스트레이터는 replay된 세션 상태 기준으로 이미 처리된 결과인지 판정해야 한다.
- 서비스 재시작이 세션 상태를 자동 복구하는 것처럼 해석되면 안 된다.

---

## 결정표

### 1. wake command 처리 결정표

| 조건 | 결정 | 비고 |
| --- | --- | --- |
| 세션 active, 열린 턴 없음, wake target 유효 | 새 턴 수용 검토 | command 종류별 정책 적용 |
| 세션 active, 열린 턴 있음 | 기존 턴과 correlation 검사 | 보통은 현재 턴으로 재진입 또는 stale |
| 세션 aborted/finalized | 기본 거절 | 명시적 resume 정책 필요 |
| dedupe key 이미 처리됨 | 무시 또는 관찰 이벤트 | 중복 전달 |

### 2. 서비스 결과 처리 결정표

| 서비스 결과 | correlation 유효 | 현재 상태 | 결정 |
| --- | --- | --- | --- |
| 완료 사실 | 예 | 열린 동일 턴 존재 | 공식 정책 평가 |
| 실패 사실 | 예 | 열린 동일 턴 존재 | retry/abort 평가 |
| 어떤 결과든 | 아니오 | 무관 | 거절 또는 stale |
| 어떤 결과든 | 예 | 턴 종료됨 | stale |

---

## 정상 시퀀스 예시

### 예시 1. scheduler wake 후 메인 오케스트레이터 재진입

```text
1) scheduler는 schedule_id=S1의 next fire에 도달한다.
2) scheduler는 ScheduledWakeTriggered command를 생성한다.
3) command에는 session_id, schedule_id, fire_sequence가 포함된다.
4) MainOrchestrator는 세션 replay와 dedupe 검사를 수행한다.
5) wake가 아직 처리되지 않았고 세션이 active이면 새 턴 수용 여부를 판단한다.
6) 턴이 열리면 그 이후 상태 전이는 모두 오케스트레이터가 담당한다.
```

### 예시 2. background job 완료 후 결과 재진입

```text
1) background worker가 외부 job 완료를 감지한다.
2) worker는 BackgroundJobCompleted command를 생성한다.
3) command에는 session_id, parent_turn_id, job_id, attempt_sequence가 포함된다.
4) MainOrchestrator는 현재 turn correlation과 stale 여부를 확인한다.
5) 유효하면 결과를 현재 턴의 공식 입력 후보로 평가한다.
6) 유효하지 않으면 stale로 폐기한다.
```

---

## 실패 시나리오

### 시나리오 1. mailbox가 외부 메시지를 conversation에 직접 추가

- 잘못된 동작: mailbox adapter가 메시지를 받은 즉시 세션 history 파일에 append
- 올바른 동작: MailboxMessageReceived command로만 재진입

### 시나리오 2. queue 재배달이 같은 턴을 두 번 완료시킴

- 잘못된 동작: dedupe 없이 같은 완료 command를 두 번 처리해 중복 상태 전이 발생
- 올바른 동작: dedupe key와 correlation 검사로 두 번째 전달은 무시

### 시나리오 3. scheduler missed fire를 자동 성공으로 처리

- 잘못된 동작: 예약 시간을 놓쳤으니 작업이 끝난 것으로 간주
- 올바른 동작: ScheduledWakeMissed 사실만 보고하고 후속 판단은 오케스트레이터가 수행

---

## 구현 불변식

아래 불변식은 future Rust 구현에서 타입, assertion, 테스트로 강제 대상이다.

1. runtime services는 세션 truth를 직접 수정할 수 없다.
2. 서비스가 emit하는 것은 사실 보고용 command여야 하며 확정 command여서는 안 된다.
3. service-owned metadata와 session truth는 저장 경계가 구분되어야 한다.
4. wake command는 새 턴 개시를 보장하지 않는다.
5. dedupe key 없는 서비스 command는 허용되면 안 된다.
6. 서비스 retry와 오케스트레이터 policy retry는 다른 개념으로 유지되어야 한다.
7. 서비스 재시작 후 중복 전달이 와도 세션 상태는 idempotent하게 유지되어야 한다.
8. 이미 닫힌 턴에 대한 늦은 서비스 결과는 stale로 처리되어야 한다.
9. 모든 서비스 재진입은 `MainOrchestrator`를 통과해야 한다.
10. 서비스 failure가 있어도 세션 truth가 손상된 성공 상태로 암묵 승격되면 안 된다.

---

## 금지 패턴

### 1. 서비스 DB를 세션 진실 원천으로 사용

금지 예:

- queue 테이블의 상태를 보고 세션이 이미 완료되었다고 간주
- scheduler metadata를 replay 없이 turn truth처럼 사용

왜 금지인가:

- session store와 truth ownership이 무너진다.

### 2. 서비스가 privileged command를 직접 생성

금지 예:

- mailbox가 ApprovalGranted를 검증 없이 생성
- worker가 AssistantMessageFinalized 같은 내부 확정 command를 생성

왜 금지인가:

- 정책 검증이 우회된다.

### 3. dedupe를 서비스 한쪽에만 맡기기

금지 예:

- queue만 dedupe하고 오케스트레이터는 중복 재진입을 그대로 처리

왜 금지인가:

- 재시작, 재배달, race 상황에서 안전하지 않다.

### 4. wake를 곧바로 새 턴 생성으로 해석

금지 예:

- scheduler fire가 오면 무조건 새 turn 생성

왜 금지인가:

- 열린 턴, stale signal, 세션 종료 상태를 무시하게 된다.

---

## Rust 구현으로 이어질 체크포인트

구체 타입 이름은 바뀔 수 있지만, 아래 질문에는 모두 "예"라고 답할 수 있어야 한다.

- `ServiceCommandEnvelope`, `WakeCommand`, `ServiceCorrelationId`, `DedupeKey`, `ServiceMetadata` 같은 타입 경계가 분리되는가?
- queue, scheduler, mailbox, hooks, worker가 emit 가능한 command 종류를 enum 수준으로 제한할 수 있는가?
- service-owned metadata 저장소와 session store가 분리되는가?
- wake 처리에서 replay, dedupe, stale 검사를 독립 단계로 테스트할 수 있는가?
- 서비스 retry와 policy retry를 다른 필드와 로직으로 유지할 수 있는가?
- 서비스 재시작 뒤 중복 command 재진입을 idempotent하게 처리할 수 있는가?

---

## 테스트 관점에서 꼭 검증할 시나리오

future Rust 구현은 최소한 아래 시나리오를 테스트로 가져갈 수 있어야 한다.

- scheduler가 같은 fire_sequence를 두 번 보내도 한 번만 처리되는가
- mailbox가 받은 외부 메시지가 오케스트레이터 재진입 전에는 conversation에 반영되지 않는가
- background job 완료가 닫힌 턴으로 오면 stale로 폐기되는가
- queue delivery retry와 policy retry가 별도 카운터로 유지되는가
- service metadata가 유실되어도 session replay만으로 공식 상태를 복원할 수 있는가
- hook 실패가 직접 세션 상태를 바꾸지 않고 command로만 관찰되는가
- wake command가 왔어도 세션이 finalized면 새 턴이 열리지 않는가
- service 재시작 후 중복 재진입이 와도 세션 상태가 두 번 변하지 않는가

---

## 명시적 비범위

이 문서는 다음을 정의하지 않는다.

- 외부 메시징 벤더 프로토콜 전체
- Slack, Discord, Telegram, Email 바깥의 추가 채널 지원
- distributed queue consensus
- 멀티유저 inbox ownership
- 관리자 운영 콘솔
- 조직 단위 스케줄 정책 관리

이 항목들은 필요가 생기면 별도 문서에서 다룬다. 단, 어떤 확장도 runtime services는 보조 서비스일 뿐이며, 상태 확정과 재진입 수용 권한은 끝까지 `MainOrchestrator`에 남아 있어야 한다는 원칙을 뒤집어서는 안 된다.

---

## 결론

`shacs-bot`의 runtime services는 코어 밖 보조 모듈이지만, 그렇다고 임의로 동작해도 되는 느슨한 주변부는 아니다. 이 계층은 큐, 스케줄러, 메일박스, 훅, 백그라운드 워커가 어떤 사실을 어떻게 보고하고, 어떤 경계까지 책임질 수 있는지를 고정하는 실행 계약이다.

핵심은 네 가지다.

- 서비스는 command를 emit할 수 있어도 세션 truth를 직접 바꾸면 안 된다.
- service-owned metadata와 session truth는 엄격히 분리되어야 한다.
- dedupe, retry, wake/resume은 중복과 재시작을 정상 상황으로 가정한 채 설계되어야 한다.
- 모든 서비스 결과는 failure-safe reentry를 통해 다시 `MainOrchestrator` 아래로 들어와야 한다.

이 구조가 지켜져야 `shacs-bot`은 주변 서비스가 늘어나도 코어 권한 모델이 흐트러지지 않고, self-hosted 단일 사용자 런타임으로서 예측 가능하고 복구 가능한 상태 전이를 유지할 수 있다.
