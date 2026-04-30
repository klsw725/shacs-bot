# session store 아키텍처 명세

## 문서 목적

이 문서는 `docs/SYSTEM-FOUNDATION.md`, `docs/specs/001-session-kernel/SPEC.md`, `docs/specs/002-command-event-effect/SPEC.md`를 바탕으로 `shacs-bot`의 session store를 구현 가능한 수준으로 고정한다.

목표는 다음과 같다.

- event log, checkpoint, replay, resume의 의미를 분명히 한다.
- crash 이후에도 단일 세션 정확성을 유지하는 저장 규칙을 정한다.
- future Rust 구현에서 타입, 파일 단위 책임, 테스트 케이스를 직접 도출할 수 있게 한다.

이 문서는 저장 계층의 기준 문서다. 구현이 이 문서와 충돌하면 코드를 우선 밀어붙이지 않고 저장 의미론부터 다시 점검해야 한다.

이 spec의 완료 기준은 단순 저장 POC가 아니라, event log, checkpoint, replay, resume, recovery 의미론과 금지 패턴까지 충족하는 **완전한 기능 구현과 검증**이다.

---

## 상위 기준과의 관계

이 문서는 다음 상위 결정을 전제로 한다.

- `MainOrchestrator`만 세션 상태를 확정할 수 있다.
- 공식 상태 전이 순서는 event 순서로 정의된다.
- 외부 executor는 상태를 직접 commit하지 못한다.
- 목표는 분산 복구가 아니라 self-hosted / personal-use 환경에서의 로컬 단일 세션 재개 정확성이다.

따라서 session store는 독립 정책 엔진이 아니다. session store는 오케스트레이터가 확정한 사실을 보존하고, 이후 replay와 resume를 위해 복원 가능한 형태로 제공하는 계층이다.

---

## 핵심 정의

### session store

session store는 한 세션의 공식 이력과 복구 기준점을 보관하는 영속 계층이다. 이 계층은 최소한 아래 세 가지를 다룬다.

- append-only event log
- 특정 시점의 checkpoint
- replay / resume에 필요한 메타데이터

### event log

event log는 오케스트레이터가 확정한 `Event`의 순서 있는 영속 이력이다. 같은 세션의 공식 상태 전이 순서는 event log의 append 순서로만 정의한다.

### checkpoint

checkpoint는 특정 event sequence까지 replay를 끝낸 뒤 얻을 수 있는 `SessionState`의 직렬화된 스냅샷이다. checkpoint는 replay를 빠르게 시작하기 위한 기준점이지, 그 자체가 event log를 대체하는 독립 진실 원천은 아니다.

### replay

replay는 checkpoint와 이후 event log를 사용해 `SessionState`를 다시 구성하는 절차다. replay는 effect 재실행이 아니라 공식 event 이력 재적용이다.

### resume

resume는 특정 `session_id`의 마지막 일관된 상태를 복원해 새 입력을 받을 수 있는 안정 상태 또는 중단 사실이 드러나는 상태로 되돌리는 절차다.

---

## 저장 책임과 비책임

### session store가 책임지는 것

- event log append와 읽기
- checkpoint 저장과 읽기
- 마지막으로 일관되게 durably recorded 된 event sequence 식별
- replay 시작점 결정
- crash 이후 복구 시 사용할 resume metadata 제공

### session store가 책임지지 않는 것

- 어떤 event를 만들지 판단하는 일
- 어떤 effect를 발행할지 결정하는 일
- late result 수용 여부를 정책적으로 결정하는 일
- provider, tool, transport 실행 핸들 유지
- UI projection, 채널 전송, 외부 알림 상태 관리

session store는 판단자가 아니라 기록자이자 복원자다.

---

## 저장 모델

한 세션의 영속 데이터는 개념적으로 아래 세 층으로 나뉜다.

1. **identity/meta**
   - `session_id`
   - 현재 저장 형식 버전
   - 마지막 durably committed event sequence
   - 마지막 사용 가능한 checkpoint 메타데이터

2. **event log**
   - append-only `Event` 레코드
   - 각 레코드는 최소한 `event_id`, `session_id`, `sequence`, `causation_id`, `correlation_id`, event payload를 가져야 한다.

3. **checkpoint**
   - 특정 `sequence`까지 반영된 `SessionState` 스냅샷
   - checkpoint가 기반으로 삼은 마지막 event sequence
   - resume 시 검증 가능한 무결성 정보

핵심은 checkpoint가 있더라도 공식 이력은 event log에 남아야 한다는 점이다.

---

## replay 시 진실 원천

### 기본 원칙

replay 시 진실 원천은 **"가장 최신의 유효한 checkpoint" + "그 checkpoint 이후 durably committed 된 event log"** 조합이다.

이를 더 엄밀히 쓰면 아래와 같다.

- checkpoint는 replay 시작 상태의 기준점이다.
- checkpoint 이후 상태 전이는 event log가 설명한다.
- checkpoint와 event log가 모순되면, checkpoint 기준 sequence 이후의 진실 원천은 event log다.
- checkpoint 자체가 손상되었거나 무결성 검증에 실패하면, checkpoint는 버리고 더 이전 checkpoint 또는 event log 처음부터 다시 replay한다.

### 중요한 해석

- event log 없는 checkpoint는 불완전한 캐시일 뿐이다.
- checkpoint 이후 event를 무시하고 checkpoint만 신뢰하면 안 된다.
- event log에 없는 상태는 replay 결과에 포함되면 안 된다.

즉 replay의 공식 입력은 "checkpoint snapshot"과 "공식 event tail"뿐이다. 미완료 effect, 실행 중 프로세스, UI 버퍼, transport 연결 객체는 replay의 입력이 아니다.

---

## checkpoint 의미와 compaction boundary

### checkpoint의 의미

checkpoint는 아래 조건을 만족해야 한다.

- 특정 `last_included_sequence`까지의 replay 결과와 동치여야 한다.
- checkpoint를 읽은 뒤 같은 sequence 이후 event만 재적용하면 동일한 `SessionState`가 나와야 한다.
- 열린 턴 유무, 마지막 확정 응답, 세션 정책 상태, compact 이후 유지할 핵심 작업 문맥이 복원 가능해야 한다.

### compaction boundary

compaction boundary는 "checkpoint가 완전히 대체할 수 있는 가장 마지막 공식 event sequence"다.

이 경계 이전의 event는 논리적으로 checkpoint에 흡수되었다고 볼 수 있다. 하지만 초기 구현에서는 아래 보수 규칙을 따른다.

- compaction은 event log 전체 삭제를 전제로 하지 않는다.
- compaction boundary 이전 event를 실제로 제거하더라도, 현재 checkpoint 하나만으로 동일 상태를 재구성할 수 있다는 검증이 선행돼야 한다.
- boundary는 턴 중간이 아니라 **턴이 `completed` 또는 `aborted`로 닫힌 뒤**에만 잡는다.

즉 미완료 턴 한가운데를 compaction boundary로 삼으면 안 된다.

### 왜 턴 경계만 허용하는가

- 미완료 턴은 외부 결과 대기, 재시도 카운터, 임시 산출물 같은 불안정 정보를 포함한다.
- 턴 중간 지점은 resume semantics를 복잡하게 만든다.
- 개인형 로컬 런타임에서는 빠른 최적화보다 재현 가능성이 더 중요하다.

따라서 초기 Rust 구현은 **닫힌 턴 이후만 checkpoint/compaction 가능**을 기본 규칙으로 삼는다.

---

## resume identity

resume의 기본 식별자는 `session_id`다. 하지만 정확한 재개를 위해 `session_id`만으로는 부족하다. 최소한 아래 의미가 함께 고정돼야 한다.

- 어떤 세션을 여는가: `session_id`
- 어디까지 공식 이력이 반영되었는가: `last_committed_sequence`
- 어떤 checkpoint를 시작점으로 삼는가: `checkpoint_id` 또는 `last_included_sequence`
- 현재 열린 턴이 있는가: `open_turn_id` 또는 없음

### deterministic resume identity

같은 `session_id`와 같은 `last_committed_sequence`를 기준으로 resume하면, 결과 `SessionState`는 항상 같아야 한다.

이 규칙 때문에 다음은 resume identity에 포함되면 안 된다.

- 현재 시각
- 재부팅 후 새로 발급한 프로세스 ID
- provider 연결 상태
- 미확정 streaming 조각
- executor가 임의로 만든 임시 메모리 캐시

resume는 "지금 살아 있는 외부 세계를 이어 붙이는 동작"이 아니라, "공식 기록 기준으로 같은 세션 상태를 다시 세우는 동작"이어야 한다.

---

## 결정적 resume 의미론

resume는 아래 절차를 따른다.

1. `session_id`에 대한 최신 메타데이터를 읽는다.
2. 가장 최신이면서 유효한 checkpoint를 찾는다.
3. checkpoint 이후 `last_committed_sequence`까지 event log를 순서대로 읽는다.
4. replay로 `SessionState`를 복원한다.
5. 복원된 상태에 열린 턴이 없다면, 세션은 바로 다음 입력을 받을 수 있다.
6. 복원된 상태에 열린 턴이 있었다면, 그 턴은 **자동 재실행하지 않고 recovery 규칙에 따라 종료 방향으로 정리**한다.

초기 구현의 결정은 아래처럼 고정한다.

- crash 직전 진행 중이던 effect를 자동으로 다시 붙잡아 이어서 실행하지 않는다.
- 미완료 턴은 resume 시 `aborted` 또는 이에 준하는 recoverable-interrupted 상태로 정리한다.
- 이미 `completed` 또는 `aborted`로 닫힌 턴 결과는 resume가 바뀌게 만들면 안 된다.

즉 deterministic resume의 핵심은 "중간 실행을 자연스럽게 이어붙이는 것"이 아니라, "같은 공식 이력에서 같은 안정 상태를 만든 뒤 필요하면 새 턴으로 다시 시작하게 하는 것"이다.

---

## recovery 규칙

### 기본 복구 원칙

1. **durable한 것만 믿는다.** 메모리에만 있던 상태는 복구 기준이 아니다.
2. **닫힌 턴 결과는 보존한다.** 마지막으로 확정된 응답은 crash 이후에도 바뀌면 안 된다.
3. **열린 턴은 복구 시 자동 성공 처리하지 않는다.** 외부 결과가 일부 있었더라도 공식 event가 아니면 성공으로 간주하지 않는다.
4. **late result는 새 공식 이력을 뒤집지 못한다.** crash 전 발행된 effect 결과가 뒤늦게 와도, 복구 이후 오케스트레이터가 correlation과 턴 상태를 보고 무시 또는 관찰 이벤트로만 남겨야 한다.

### 열린 턴 복구 규칙

resume 시 복원된 `SessionState`에 열린 턴이 있다면 아래 둘 중 하나만 허용한다.

- recovery step에서 해당 턴을 명시적으로 `aborted` 처리한다.
- recovery step에서 "중단됨, 다시 시도 필요" 같은 관찰 가능한 종료 사실을 남긴다.

허용되지 않는 것은 아래다.

- 열린 턴을 아무 흔적 없이 삭제
- 외부 결과가 있었을 것이라고 추정하고 `completed`로 승격
- effect executor의 내부 캐시를 근거로 턴을 계속 이어감

### checkpoint 손상 복구 규칙

- checkpoint 무결성 검증 실패 시 해당 checkpoint는 사용하지 않는다.
- 더 이전 checkpoint가 있으면 그 지점부터 replay한다.
- 없으면 event log 처음부터 replay한다.
- checkpoint 실패만으로 event log까지 폐기하지 않는다.

### event tail 손상 복구 규칙

- `last_committed_sequence` 이후의 부분 기록은 공식 이력으로 채택하지 않는다.
- 중간에 잘린 tail이 발견되면 마지막으로 완전한 committed sequence까지만 사용한다.
- 불완전 tail은 진단 대상일 수 있지만 상태 복원의 진실 원천은 아니다.

---

## append와 durability 규칙

session store는 적어도 의미 수준에서 아래 원칙을 만족해야 한다.

1. event는 세션별 단조 증가 sequence로 append된다.
2. `last_committed_sequence`는 실제로 durable하게 기록된 마지막 event까지만 가리킨다.
3. checkpoint는 그것이 가리키는 `last_included_sequence`보다 앞선 상태를 절대 주장하면 안 된다.
4. checkpoint 메타데이터 갱신이 실패해도 기존 checkpoint와 event log로 복구 가능해야 한다.

구체 파일 쓰기 순서는 구현에서 정할 수 있다. 그러나 외부에 보이는 의미는 아래를 깨면 안 된다.

- "commit됐다고 표시된 event"는 replay 가능해야 한다.
- "사용 가능하다고 표시된 checkpoint"는 그 sequence까지의 상태와 동치여야 한다.

---

## 불변식

아래 불변식은 future Rust 구현에서 타입, assertion, 테스트로 강제 대상이다.

1. 같은 세션의 공식 상태 전이 순서는 event log sequence로만 정의된다.
2. checkpoint는 특정 sequence까지 replay한 결과와 동치여야 한다.
3. replay는 effect를 재실행하지 않는다.
4. event log에 없는 상태는 공식 복구 결과에 포함되면 안 된다.
5. `last_committed_sequence`보다 뒤의 부분 기록은 복구 시 채택되면 안 된다.
6. 열린 턴이 있는 세션을 resume해도 그 턴이 암묵적으로 성공 완료되면 안 된다.
7. 닫힌 턴의 마지막 확정 응답은 crash 이후 replay/resume로 바뀌면 안 된다.
8. compaction boundary는 닫힌 턴 뒤에만 놓일 수 있다.
9. resume 결과는 `session_id`와 committed event prefix가 같으면 결정적으로 같아야 한다.
10. session store는 외부 executor의 실행 핸들, 소켓, 프로세스 객체를 진실 원천으로 삼으면 안 된다.

---

## 정상 예시

### 예시 1. 닫힌 턴 뒤 checkpoint 생성

```text
1) sequence 120에서 TurnCompleted 또는 TurnAborted까지 공식 event가 기록된다.
2) 오케스트레이터가 checkpoint 생성을 승인하고, 저장 계층은 sequence 120 기준 SessionState를 직렬화해 기록한다.
3) checkpoint metadata에는 last_included_sequence=120이 기록된다.
4) 이후 replay는 checkpoint@120에서 시작하고, sequence 121 이후 event만 다시 읽는다.
```

이 경우 checkpoint는 완결된 턴 경계에 놓였으므로 다음 resume에서 같은 안정 상태를 재구성할 수 있다.

### 예시 2. checkpoint 이후 tail replay

```text
1) checkpoint가 sequence 200까지 존재한다.
2) event log에는 201, 202, 203이 committed 되어 있다.
3) resume는 checkpoint@200을 읽는다.
4) replay는 201 -> 202 -> 203 순서로 상태를 재적용한다.
5) 결과 SessionState는 sequence 203 기준 상태와 같아야 한다.
```

핵심은 checkpoint만 읽고 끝내지 않는다는 점이다. checkpoint 이후 event tail이 공식 진실 원천이다.

---

## crash recovery 시퀀스 예시

### 예시 3. tool 대기 중 crash

```text
1) sequence 310: Event::TurnStarted
2) sequence 311: Event::ToolCallRequested(effect_id=E-9)
3) 프로세스가 tool 결과 reentry 전에 crash 한다.
4) restart 후 session store는 checkpoint@300과 event 301..311을 읽는다.
5) replay 결과, session에는 open_turn_id가 남아 있고 마지막 공식 사실은 ToolCallRequested다.
6) recovery 단계에서 오케스트레이터는 이 열린 턴을 "실행 중 crash로 중단됨" 사유로 종료 처리하기로 결정한다.
7) 저장 계층은 그 recovery event를 새 committed sequence로 append 한다.
8) 세션은 다음 입력을 받을 수 있는 안정 상태가 된다.
9) 나중에 old tool runtime이 effect_id=E-9 결과를 보내더라도, 오케스트레이터는 닫힌 턴의 late result로 무시하거나 관찰 이벤트로만 남긴다.
```

이 시퀀스에서 중요한 점은, tool이 실제로 성공했을 수도 있어도 공식 event가 없으면 성공으로 간주하지 않는다는 점이다. 또한 recovery 중 어떤 종료 사실을 남길지 결정하는 권한은 저장 계층이 아니라 항상 오케스트레이터에 있다.

---

## 실패 시나리오

### 시나리오 1. checkpoint는 최신인데 event tail을 무시하는 경우

- 잘못된 동작: sequence 400 checkpoint가 있으니 401 이후 event를 읽지 않고 resume 종료
- 왜 실패인가: checkpoint 이후 확정된 assistant 응답, abort 사실, 정책 변경이 사라질 수 있다.

### 시나리오 2. 열린 턴을 자동 성공 처리하는 경우

- 잘못된 동작: crash 직전 provider 응답이 메모리에 있었으니 resume 시 assistant 응답을 commit
- 왜 실패인가: durable한 공식 event가 없으므로 재현 가능성이 깨진다.

### 시나리오 3. tail 일부 기록을 공식 이력으로 채택하는 경우

- 잘못된 동작: event payload 일부만 남았는데도 sequence를 증가시켜 replay에 포함
- 왜 실패인가: 동일 세션을 두 번 복구했을 때 상태가 달라질 수 있다.

### 시나리오 4. 턴 중간 checkpoint를 compaction boundary로 삼는 경우

- 잘못된 동작: `ToolCallRequested` 직후 checkpoint를 만들고 그 이전 event를 지움
- 왜 실패인가: effect 대기 중간 상태가 안정 경계가 아니므로 deterministic resume이 어려워진다.

---

## 금지 패턴

### 금지 패턴 1. checkpoint를 단독 진실 원천으로 취급

예:

- 최신 checkpoint만 읽고 이후 event log를 무시
- checkpoint가 더 최신처럼 보여도 committed event tail보다 우선 적용

왜 금지인가:

- 공식 상태 전이 순서가 event log라는 원칙이 깨진다.
- crash 직전 확정된 사실을 잃을 수 있다.

### 금지 패턴 2. 미완료 effect 상태를 resume 근거로 사용

예:

- provider socket에 아직 연결이 살아 있으니 기존 턴을 그대로 이어감
- tool runtime 임시 캐시를 읽어 assistant 응답을 확정

왜 금지인가:

- session store 바깥 실행기 내부 상태를 공식 이력보다 우선하게 된다.
- 로컬 재시작 후 같은 결과를 결정적으로 재현할 수 없다.

### 금지 패턴 3. 턴 중간 compaction

예:

- `model_pending`, `tool_pending` 상태를 checkpoint로 굳힌 뒤 이전 event를 제거

왜 금지인가:

- 열린 턴의 의미가 불안정하다.
- crash recovery가 실행기 세부사항에 의존하게 된다.

---

## Rust 구현으로 이어질 체크포인트

구체 타입 이름은 바뀔 수 있지만, 아래 질문에는 모두 "예"라고 답할 수 있어야 한다.

- event 레코드에 `session_id`, `event_id`, `sequence`, causation/correlation 정보가 있는가?
- checkpoint 타입이 `last_included_sequence`를 명시하는가?
- replay API가 "checkpoint + event iterator" 모델로 정의되는가?
- resume API가 열린 턴을 자동 성공시키지 않도록 강제하는가?
- corrupted checkpoint fallback 테스트를 만들 수 있는가?
- truncated event tail 무시 테스트를 만들 수 있는가?
- compaction이 닫힌 턴 경계에서만 수행되는지 검증할 수 있는가?

### 최소 테스트 관점

future Rust 구현은 최소한 아래 시나리오를 테스트로 가져가야 한다.

- checkpoint 없이 event log 처음부터 replay해 동일 상태가 나오는지
- checkpoint가 있어도 이후 event tail을 반드시 반영하는지
- 열린 턴이 있는 상태로 crash 후 resume하면 자동 성공이 아니라 중단 정리가 일어나는지
- 같은 committed prefix로 두 번 resume해도 동일 상태가 나오는지
- 손상된 최신 checkpoint를 버리고 이전 checkpoint 또는 전체 replay로 복구하는지
- 중간에 잘린 event tail을 공식 이력으로 채택하지 않는지

---

## 명시적 비범위

이 문서는 다음을 정의하지 않는다.

- 실제 파일 포맷, SQLite 사용 여부, JSONL 여부 같은 저장 매체 선택
- 디렉터리 레이아웃과 파일명 규칙
- checksum 알고리즘 종류
- provider streaming chunk 저장 방식
- UI projection 캐시 구조
- transport, CLI, TUI, API 응답 형식
- 멀티세션 스케줄링 정책
- 멀티유저 동시성 제어와 분산 락

이 항목들은 별도 문서에서 다룬다. 단, 어떤 하위 설계도 이 문서의 핵심 규칙, 특히 replay 진실 원천과 deterministic resume 의미론을 뒤집어서는 안 된다.

---

## 결론

`shacs-bot`의 session store는 단순 저장소가 아니다. 이 계층은 "무엇이 공식 이력인가", "무엇이 복구 기준인가", "crash 뒤 무엇을 버리고 무엇을 남길 것인가"를 고정하는 복구 계약이다.

핵심은 세 가지다.

- 공식 순서는 event log가 정의한다.
- checkpoint는 replay를 빠르게 만드는 기준점이지만 event tail을 대체하지 않는다.
- resume는 미완료 실행을 추측해 이어 붙이지 않고, 공식 기록 기준으로 같은 안정 상태를 다시 세운다.
