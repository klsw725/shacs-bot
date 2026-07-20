# session kernel 아키텍처 명세

Status: Complete (Scoped)
Implemented scope: 현재 구현의 `AgentLoop`, `AgentRunner`, `SessionManager`, `SessionTurnLock`, `runtime_checkpoint`, `pending_user_turn` marker를 session kernel과 turn lifecycle의 current architecture로 매핑하고 recovery evidence 경계를 닫았다.
Open work moved to: [029 durable runtime recovery and data migration](../029-durable-runtime-recovery-and-data-migration/SPEC.md), [031 ui projection, diagnostics, and release evidence parity](../031-ui-projection-diagnostics-and-release-evidence-parity/SPEC.md)
Not carried forward: 분산 실행, 멀티유저 동기화, 다중 오케스트레이터 협상, 별도 durable `TurnState`와 formal phase enum은 001 완료 범위에 포함하지 않는다.

## 문서 목적

이 문서는 `docs/SYSTEM-FOUNDATION.md`를 구현 가능한 수준으로 구체화한 하위 명세 문서다.

목표는 다음 두 가지다.

- `shacs-bot`의 session kernel이 무엇을 책임지고 무엇을 책임지지 않는지 고정한다.
- 이후 Rust 구현에서 상태 모델, 턴 루프, 이벤트 기록, 중단 처리에 대한 테스트를 직접 도출할 수 있게 한다.

이 문서는 방향 제안이 아니라 구현 기준이다. 구현이 이 문서와 충돌하면 코드를 우선 밀어붙이지 않고 문서 판단부터 다시 갱신해야 한다.

이 spec의 완료 기준은 session kernel의 POC를 만드는 것이 아니라, 이 문서가 정의한 권한 경계, 상태 모델, 턴 수명주기, 불변식, 실패 처리 규칙을 충족하는 **완전한 기능 구현과 검증을 끝내는 것**이다.

현재 코드베이스에는 이미 `AgentLoop`, `AgentRunner`, `SessionManager`, `SessionTurnLock`, `runtime_checkpoint` 기반의 session kernel 구현이 존재한다. 따라서 001 작업은 새 `SessionState`/`TurnState`/phase enum을 전면 도입하는 재작성 작업이 아니라, 현재 구현을 이 문서의 개념 계약에 매핑하고 부족한 관측/검증/문서 gap만 외과적으로 보강하는 작업이어야 한다.

---

## 상위 기준과의 관계

이 문서는 `docs/SYSTEM-FOUNDATION.md`의 다음 결정을 전제로 한다.

- 강한 `MainOrchestrator`가 모든 상태 전이를 관장한다.
- 시스템 중심은 한 세션에서 한 턴이 어떻게 실행되는가다.
- 바깥 시스템은 서비스일 뿐 정책 권한자가 아니다.
- 목표는 분산 플랫폼이 아니라 self-hosted / personal-use 환경에서의 단일 사용자 세션 정확성이다.

따라서 이 문서는 분산 실행, 멀티유저 동기화, 다중 오케스트레이터 협상 같은 구조를 다루지 않는다.

---

## 현재 구현과의 관계

이 문서의 용어는 제품/아키텍처 계약을 설명하는 개념명이다. current Rust 구현에서 반드시 동일한 이름의 타입을 가져야 한다는 뜻은 아니다.

현재 코드 기준 매핑은 아래와 같다.

| spec 개념 | current code 기준 |
|---|---|
| `MainOrchestrator` | `crates/shacs-core/src/runtime/agent_loop.rs`의 `AgentLoop` |
| session kernel | `AgentLoop`가 세션 로드, 턴 락, context build, runner 호출, checkpoint 저장/정리, outbound publish를 조정하는 경계 |
| `SessionState` | `crates/shacs-session/src/lib.rs`의 `Session` + `SessionManager` + `metadata` + `last_consolidated` |
| `TurnState` | 별도 durable 타입이 아니라 `SessionTurnLock`, `AgentRunSpec`, `AgentRunResult`, `runtime_checkpoint`, `pending_user_turn` marker가 함께 이루는 실행 중 상태 |
| model/tool loop | `crates/shacs-core/src/runtime/runner.rs`의 `AgentRunner`와 `crates/shacs-core/src/runtime/tool_execution.rs`의 `RuntimeToolExecutor` |
| opened turn invariant | `crates/shacs-core/src/runtime/loop_control.rs`의 `SessionTurnLock` |

따라서 새 001 작업은 다음을 목표로 삼는다.

1. 기존 `AgentLoop`/`AgentRunner`/`SessionManager` 구조를 보존한다.
2. `SessionState`와 `TurnState`라는 개념 경계를 코드의 실제 타입/metadata/checkpoint와 명시적으로 연결한다.
3. 명시 phase enum을 바로 도입하지 않는다. 필요하면 inspect/read-model 또는 checkpoint schema부터 보강한다.
4. 전면 리팩터링보다 현재 구현의 불변식, recovery evidence, 테스트 이름, 문서 근거를 맞춘다.

---

## 핵심 정의

### session kernel

session kernel은 한 세션의 입력을 받아, 필요한 문맥을 구성하고, 모델 및 툴 왕복을 조정하고, 결과를 세션 상태에 반영하고, 다음 턴으로 이어질 수 있는 안정된 상태를 남기는 코어 실행부다.

### 턴

턴은 하나의 외부 입력이 세션에 받아들여진 시점부터, 그 입력에 대한 최종 결과가 `completed` 또는 `aborted`로 닫힐 때까지의 원자적 실행 단위다.

여기서 외부 입력은 보통 사용자 입력이지만, 향후 서브에이전트 결과나 내부 재진입용 synthetic command도 같은 턴 시작 입력으로 취급할 수 있다. 단, 이 문서의 기본 시퀀스는 사용자 입력 기준으로 정의한다.

### 상태 전이

상태 전이는 `SessionState` 또는 `TurnState`에 해당하는 관측 가능한 사실이 변경되는 모든 경우를 뜻한다. current code에서는 `Session`/`SessionManager` 저장 내용, runtime metadata marker, checkpoint, runner result, turn lock 상태가 이 관측 가능한 사실에 해당한다. 상태 전이는 오직 `MainOrchestrator` 역할을 하는 `AgentLoop`의 결정에 의해 일어나야 한다.

---

## 권한 모델

### `MainOrchestrator` 역할의 단일 권한

`MainOrchestrator`는 제품 계약상의 권한자 이름이다. current code에서 이 역할은 `AgentLoop`가 수행한다.

이 역할은 다음에 대한 유일한 권한자다.

- 새 턴 시작 승인
- `SessionState`에 해당하는 durable session 사실 변경
- `TurnState`에 해당하는 active turn marker, checkpoint, runner result 생성, 변경, 종료
- `Effect` 발행 순서 결정
- 외부 결과를 상태에 반영할지 폐기할지 결정
- retry, abort, compact, resume 판단

다른 구성요소는 위 권한을 가지지 않는다.

### 바깥 구성요소가 할 수 있는 일

오케스트레이터 바깥 구성요소는 다음만 할 수 있다.

- `Command`를 전달한다.
- `Effect` 실행 결과를 돌려준다.
- `Event`를 소비한다.

바깥 구성요소는 다음을 해서는 안 된다.

- `SessionState`를 직접 수정
- 턴 상태를 임의 종료
- tool 결과를 자체 판단으로 대화 기록에 반영
- 여러 외부 결과를 자체 병합한 뒤 확정 상태처럼 주입

### effect 실행자의 위치

LLM provider, tool runtime, session store, queue, scheduler, mailbox, 외부 채널 어댑터는 모두 effect 실행자 또는 event 소비자다. 이들은 실행을 담당하지만 정책을 확정하지 않는다.

---

## 상태 경계

### `SessionState`의 책임 경계

`SessionState`는 턴을 넘어 유지되어야 하는 세션의 진실 원천이다. current code에서는 별도 `SessionState` 타입이 아니라 `Session`과 `SessionManager`가 이 역할을 한다.

`SessionState`에는 최소한 다음 종류의 정보가 포함되어야 한다.

- 세션 식별자
- 세션 수명주기 상태, 예: active, compacting, aborted, finished
- 현재 열린 턴의 식별자 또는 열린 턴이 없다는 사실
- 누적 대화 기록 또는 그에 준하는 영속 문맥 표현
- compact 이후에도 보존해야 하는 핵심 작업 문맥
- 세션 수준 정책 상태, 예: permission mode, 선택된 스킬, resume 메타데이터
- append 가능한 이벤트 이력과 연결될 수 있는 버전 정보 또는 순서 정보

현재 구현에서 이미 대응되는 필드는 다음과 같다.

- 세션 식별자: `Session.key`
- 누적 대화 기록: `Session.messages`
- durable metadata: `Session.metadata`
- compaction 경계: `Session.last_consolidated`
- 저장/조회/삭제/list 경계: `SessionManager`

`SessionState`에 두면 안 되는 것:

- 일시적 provider 요청 핸들
- 실행 중 shell 프로세스 핸들
- 아직 확정되지 않은 중간 tool stdout 조각
- 한 턴 안에서만 의미가 있는 재시도 카운터
- 외부 런타임이 들고 있어야 할 transport 연결 객체

즉 `SessionState`는 재개와 설명 가능성을 위해 남겨야 하는 사실만 가진다.

### `TurnState`의 책임 경계

`TurnState`는 현재 진행 중인 단일 턴의 실행 컨트롤 블록이다. current code에서는 아직 별도 durable `TurnState` 타입이 없다. 대신 active turn lock, runner input/result, cancellation token, checkpoint metadata가 합쳐져 이 역할을 한다.

`TurnState`에는 최소한 다음 종류의 정보가 포함되어야 한다.

- turn id
- 이 턴을 시작한 입력의 정체
- 현재 phase
- phase 진입 시각과 마지막 갱신 시각
- 이 턴에서만 유효한 재시도 횟수
- 대기 중인 effect의 종류와 상관관계 정보
- 아직 최종 반영되지 않은 임시 산출물, 예: 모델 초안 응답, tool result envelope
- abort 사유 또는 failure 원인, 아직 닫히지 않았다면 진행 중 원인

현재 구현에서 이 개념은 아래 요소로 분산되어 있다.

- 열린 턴 존재 여부: `SessionTurnLock`
- 턴 입력과 실행 config: `AgentRunSpec`
- 턴 결과와 stop reason: `AgentRunResult` / `AgentLoopTurnResult`
- 중단 또는 복구 evidence: `runtime_checkpoint`, `pending_user_turn` metadata
- 취소 상태: `CancellationToken`과 `LoopTaskRegistry`

따라서 001 후속 작업에서 별도 `TurnState` 타입을 만들기보다, 먼저 이 분산 상태를 inspect 가능한 read model 또는 명시 checkpoint schema로 설명 가능하게 만드는 것을 우선한다.

`TurnState`에 두면 안 되는 것:

- 세션 전체 기록의 최종 원본
- 다음 턴에도 그대로 유지될 장기 정책 상태
- 독립적인 스케줄러 큐 상태
- 다른 세션과 공유되는 전역 상태

즉 `TurnState`는 현재 턴을 닫기 위해 필요한 실행 중 상태만 가진다.

### 경계 판단 규칙

어떤 필드가 `SessionState`에 있어야 하는지 `TurnState`에 있어야 하는지 애매하면 다음 질문으로 판단한다.

> 이 값이 턴이 `completed` 또는 `aborted`로 닫힌 뒤에도 다음 턴의 해석에 직접 필요하거나 resume correctness에 필요하나?

- 그렇다 → `SessionState`
- 아니다, 현재 턴을 끝내는 동안만 필요하다 → `TurnState`

---

## 턴 수명주기

한 턴은 개념적으로 아래 phase를 순서대로 지난다. current code가 이 phase들을 동일한 enum으로 저장해야 한다는 뜻은 아니다. 구현에서 내부 세부 phase가 더 늘어나거나 runner/checkpoint 이름이 다를 수는 있지만, 외부적으로는 아래 단계 의미를 깨면 안 된다.

1. `accepted`
2. `context_building`
3. `model_pending`
4. `tool_pending` 또는 `result_applying`
5. `completed` 또는 `aborted`

## phase 정의

current code 기준으로는 `AgentLoop::process_message`가 `accepted`와 `context_building`에 해당하는 작업을 수행하고, `AgentRunner::run`이 `model_pending`/`tool_pending`/`result_applying`에 해당하는 내부 루프를 수행한다. `runtime_checkpoint`의 `phase` 값은 이 개념 phase를 관찰하기 위한 evidence이지, 현재로서는 공식 durable phase enum이 아니다.

### 1. `accepted`

오케스트레이터가 새 입력을 받아 이 입력을 새 턴으로 처리하기로 확정한 상태다.

진입 조건:

- 세션이 새 턴을 받을 수 있는 상태다.
- 다른 열린 턴이 없다.
- 입력이 정책상 수용 가능하다.

이 단계에서 해야 하는 일:

- current implementation 기준으로는 `SessionTurnLock`을 획득하고, `Session`에 user turn과 필요한 runtime marker를 기록한다.
- 입력을 턴 시작 원인으로 기록
- 턴 시작 이벤트 append 준비

이 단계에서 하면 안 되는 일:

- 아직 문맥이 구성되지 않았는데 provider 호출 시작
- 입력을 최종 assistant 응답처럼 기록

### 2. `context_building`

세션 기록, 정책, 스킬, compact 결과 등 이미 확정된 정보를 바탕으로 이번 턴의 실행 문맥을 구성하는 단계다.

진입 조건:

- 턴이 `accepted` 상태다.

이 단계에서 해야 하는 일:

- 세션 기록 조회
- 필요한 스킬 또는 정책 문맥 결합
- provider 요청 초안 생성

이 단계에서 하면 안 되는 일:

- 외부 결과를 먼저 가정하고 상태 반영
- tool runtime에 직접 작업을 발사

### 3. `model_pending`

LLM provider 호출 effect가 발행되었고, 오케스트레이터가 그 결과를 기다리는 단계다.

진입 조건:

- 모델 호출에 필요한 입력이 준비되었다.

이 단계의 규칙:

- 대기 중인 provider effect는 현재 턴과 상관관계가 묶여야 한다.
- provider 결과가 돌아와도 오케스트레이터가 검증하기 전까지는 세션 기록에 반영되지 않는다.

가능한 결과:

- assistant 최종 응답 초안 수신 → `result_applying`
- tool call 요청 수신 → `tool_pending`
- provider 실패 또는 정책상 거절 → retry 판단 또는 `aborted`

### 4. `tool_pending`

모델이 요청한 tool call을 오케스트레이터가 승인하고, 해당 tool effect 결과를 기다리는 단계다.

진입 조건:

- 모델 출력이 tool roundtrip을 요구한다.
- 오케스트레이터 정책이 해당 tool 실행을 허용한다.

이 단계의 규칙:

- 여러 tool call을 지원하더라도 상태 반영 순서는 오케스트레이터가 결정한다.
- tool runtime은 결과를 반환만 할 뿐, 기록 반영 여부를 결정하지 않는다.
- tool 결과는 턴에 연결된 임시 산출물로 먼저 저장되고, 이후 모델 재호출 또는 최종 적용으로 이어진다.

가능한 결과:

- tool 성공 → `context_building` 또는 `model_pending`으로 재진입
- tool 실패 → retry 판단 또는 `aborted`
- 사용자 취소 또는 정책 중단 → `aborted`

### 5. `result_applying`

이번 턴의 최종 산출물을 세션에 반영하는 단계다.

진입 조건:

- 오케스트레이터가 이번 턴의 종료 산출물을 확정할 수 있다.

이 단계에서 해야 하는 일:

- assistant 응답, tool 결과 요약, 상태 메타데이터를 세션 기록에 append
- 필요한 이벤트 기록
- 열린 턴 포인터 정리

이 단계에서 하면 안 되는 일:

- 아직 검증되지 않은 외부 결과를 확정 기록으로 남김
- 세션 기록 반영 이후 다시 동일 턴을 열린 상태로 되돌림

### 6. `completed`

이번 턴이 정상 종료되었고, 다음 입력을 받을 수 있는 안정 상태다.

완료 조건:

- 열린 턴이 없다.
- 이번 턴의 최종 결과가 세션에 반영되었다.
- 이후 resume 시 동일한 결과를 재구성할 수 있다.

### 7. `aborted`

이번 턴이 정상 종료되지 못했고, 오케스트레이터가 중단을 확정한 상태다.

중단 조건:

- retry 정책을 소진했다.
- 사용자 취소가 접수되었다.
- permission 거절 또는 불가역적 외부 실패가 발생했다.
- 내부 불변식 위반이 감지되었다.

중단 후 상태 규칙:

- 열린 턴은 닫혀야 한다.
- 중단 사유는 관측 가능해야 한다.
- 미확정 임시 산출물은 세션의 최종 응답으로 승격되면 안 된다.

---

## 불변식

아래 불변식은 구현과 테스트에서 강제 대상이다.

1. 한 세션에는 동시에 하나의 열린 턴만 존재한다.
2. `SessionState`에 해당하는 durable session 변경은 오직 `MainOrchestrator` 역할을 하는 `AgentLoop`를 통해서만 일어난다.
3. 외부 effect 결과는 턴 상관관계가 일치할 때만 반영 가능하다.
4. `completed` 턴은 최종 결과가 세션 기록에 반영된 뒤에만 닫힐 수 있다.
5. `aborted` 턴의 미완료 산출물은 최종 assistant 응답처럼 기록되면 안 된다.
6. 이미 닫힌 턴에 늦게 도착한 외부 결과는 무시되거나 별도 관찰 이벤트로만 남아야 하며, 세션 기록을 뒤집으면 안 된다.
7. 새 턴 수용 전에는 이전 열린 턴이 없어야 한다.
8. tool 실행 여부는 모델이 아니라 오케스트레이터 정책이 최종 결정한다.
9. compact, resume, retry 판단은 바깥 런타임이 아니라 오케스트레이터가 한다.
10. 세션이 재개된 뒤에도 마지막 확정 턴 결과와 열린 턴 유무는 모순 없이 복원되어야 한다.

---

## 정상 시퀀스 예시

아래는 tool 호출이 한 번 포함된 일반적인 정상 턴 시퀀스다.

1. 사용자가 새 요청을 보낸다.
2. 인터페이스는 이를 `Command` 또는 `InboundMessage`로 변환해 `AgentLoop`에 전달한다.
3. `AgentLoop`는 `SessionTurnLock`으로 세션에 열린 턴이 없음을 확인하고 턴 처리를 시작한다.
4. `AgentLoop`는 세션 기록과 스킬 문맥을 조합해 `AgentRunSpec`을 만든다.
5. `AgentRunner`는 provider 호출을 수행하며 개념상 `model_pending`으로 진입한다.
6. provider가 tool call 요청을 반환한다.
7. `AgentRunner`/tool execution 경계는 요청된 tool이 실행 가능한지 확인한다. host safety 정책 확정은 007/010의 오케스트레이터 정책 경계와 충돌하면 안 된다.
8. 허용되면 tool 실행으로 들어가고 개념상 `tool_pending` 상태가 된다.
9. tool runtime이 결과를 반환한다.
10. `AgentRunner`는 tool 결과를 messages에 연결하고 새 provider 호출로 이어간다.
11. tool 결과를 포함한 새 provider 호출이 수행되며 개념상 `model_pending`으로 재진입한다.
12. provider가 최종 assistant 응답 초안을 반환한다.
13. `AgentLoop`는 `AgentRunResult`를 받아 새 runner message를 세션 기록에 append하고 runtime marker를 정리한다.
14. `SessionTurnGuard`가 해제되며 열린 턴이 닫히고, 결과는 `completed` stop reason 또는 이에 준하는 terminal outcome으로 남는다.
15. 세션은 다음 입력을 받을 수 있는 안정 상태가 된다.

이 시퀀스에서 중요한 점은 provider와 tool runtime이 실제 작업은 했지만, 무엇을 durable session 기록으로 남길지는 `AgentLoop` 경계에서만 결정된다는 점이다.

---

## 실패 및 중단 시퀀스 예시

아래는 tool 실행 중 permission 거절 또는 interrupt로 턴이 중단되는 예시다.

1. 사용자가 파일 수정 성격의 요청을 보낸다.
2. `AgentLoop`는 새 턴 처리를 시작하고 세션 turn lock을 획득한다.
3. 문맥을 구성한 뒤 provider 호출을 발행하고 `model_pending`으로 전이한다.
4. provider가 쓰기 성격 tool call 요청을 반환한다.
5. 오케스트레이터 정책 경계는 현재 세션 permission mode를 검사한다.
6. 현재 정책상 해당 tool은 사용자 확인 없이는 허용되지 않으며, 이 턴에서는 즉시 승인이 불가능하다고 판단한다.
7. 오케스트레이터는 tool effect를 발행하지 않는다.
8. `AgentRunner`/`AgentLoop`는 거절, tool error, cancellation, ask_user interrupt 같은 terminal 또는 paused outcome을 관측 가능하게 남긴다.
9. 오케스트레이터는 미실행 tool call 초안을 세션의 최종 수행 사실처럼 기록하지 않는다.
10. `AgentLoop`는 runtime marker를 정리하거나 recovery marker를 남기고 열린 turn lock을 해제한다.
11. 세션은 중단 사유를 가진 안정 상태로 돌아간다.

이 경우 금지되는 동작은 다음과 같다.

- tool runtime이 승인 여부를 스스로 판단해 실행하는 것
- 모델이 요청했다는 이유만으로 세션 기록에 "파일 수정 완료" 같은 확정 결과를 남기는 것
- 중단된 턴을 열린 상태로 남겨 다음 입력과 섞이게 만드는 것

---

## 금지 패턴

아래 패턴은 session kernel 설계 위반으로 간주한다.

### 1. 바깥에서 상태 직접 수정

예: tool runtime이 성공 결과를 session store에 바로 쓰는 경우

왜 금지인가:

- 상태 전이 추적이 깨진다.
- 재현 불가능한 숨은 side effect가 생긴다.

### 2. 열린 턴이 있는데 새 턴 수용

예: provider 응답 대기 중인데 다음 사용자 입력을 즉시 같은 세션에 병행 삽입하는 경우

왜 금지인가:

- 단일 세션 정확성이 깨진다.
- 어떤 응답이 어떤 입력에 대응하는지 흐려진다.

### 3. 외부 결과의 무조건 반영

예: 닫힌 턴에 늦게 도착한 tool 결과를 그대로 대화 기록에 넣는 경우

왜 금지인가:

- 세션 기록이 뒤늦게 변형된다.
- resume 시 동일 결과를 보장하기 어렵다.

### 4. `TurnState` 역할 상태에 장기 정책 보관

예: permission mode 변경을 턴 종료와 함께 사라지는 임시 필드에만 두는 경우

왜 금지인가:

- 다음 턴 해석이 달라질 수 있다.
- resume 이후 정책 일관성이 깨진다.

### 5. `SessionState` 역할 상태에 실행 핸들 보관

예: 살아 있는 프로세스 핸들, 소켓 연결 객체, transport 세부 객체를 그대로 저장하는 경우

왜 금지인가:

- 재직렬화와 복구 가능성이 떨어진다.
- 세션 상태가 실행기 구현 세부사항에 오염된다.

### 6. provider 또는 tool runtime을 정책 권한자로 취급

예: tool runtime이 위험도를 계산한 뒤 스스로 실행 허용 여부를 확정하는 경우

왜 금지인가:

- 오케스트레이터 단일 권한 원칙이 깨진다.
- 정책 판단 위치가 분산된다.

---

## 테스트 관점에서 꼭 검증할 시나리오

Rust 구현은 최소한 다음 성격의 테스트를 만들 수 있어야 한다.

- 열린 턴이 있을 때 새 입력을 거절하거나 대기시키는지 확인하는 테스트
- tool 결과가 잘못된 turn correlation으로 들어오면 반영되지 않는지 확인하는 테스트
- permission 거절 시 tool effect가 발행되지 않고 턴이 `aborted`로 닫히는지 확인하는 테스트
- 정상 턴 종료 시 `SessionState`에 최종 응답이 남고 열린 턴이 제거되는지 확인하는 테스트
- 늦게 도착한 provider 또는 tool 결과가 이미 닫힌 턴의 결과를 덮어쓰지 않는지 확인하는 테스트
- compact 또는 resume 이후에도 마지막 확정 결과와 다음 턴 가능 여부가 유지되는지 확인하는 테스트

current code 기준으로는 다음 테스트 성격도 함께 유지해야 한다.

- `AgentLoop::process_message`가 세션별 turn lock을 획득하고 중복 active turn을 거절하는지
- `runtime_checkpoint`와 `pending_user_turn` marker가 crash/recovery evidence로 남고 성공처럼 오인되지 않는지
- `AgentRunner`의 tool loop가 checkpoint와 stop reason을 일관되게 남기는지
- `SessionManager`가 JSONL 저장, history repair, orphan tool result filtering, compaction metadata를 보존하는지

이 문서의 목적은 테스트 이름을 나열하는 것이 아니라, 어떤 테스트가 반드시 가능해야 하는지 고정하는 데 있다.

---

## 명시적 비범위

이 문서는 다음을 정의하지 않는다.

- session store의 구체 파일 포맷, 디렉터리 레이아웃, 직렬화 방식
- tool runtime의 프로세스 관리 세부 구현
- provider streaming 프로토콜 세부사항
- queue, scheduler, mailbox의 내부 동작 모델
- 멀티유저 세션 격리 모델
- 분산 락, 클러스터 리더 선출, 원격 워커 토폴로지
- 웹 API, TUI, 외부 채널 transport 사양
- `AgentLoop`/`AgentRunner`를 새 이름의 `MainOrchestrator` 타입으로 단순 rename하는 작업
- 기존 runtime loop를 버리고 새 `SessionState`/`TurnState` 저장소로 전면 교체하는 작업

이 항목들은 별도 하위 문서에서 다룬다. 단, 어떤 하위 문서도 이 문서의 권한 경계를 뒤집어서는 안 된다.

---

## 결론

`shacs-bot`의 session kernel은 강한 `MainOrchestrator` 역할이 `SessionState`와 `TurnState` 개념 경계를 엄격히 나누고, 한 번에 하나의 턴만 통제하며, 모든 외부 실행 결과를 검증 후 반영하는 구조여야 한다.

현재 코드에서는 이 역할을 `AgentLoop`/`AgentRunner`/`SessionManager`/`SessionTurnLock` 조합이 이미 상당 부분 수행한다. 따라서 001의 다음 작업은 전면 재구현이 아니라, current architecture를 기준으로 불변식과 recovery evidence를 더 명시적으로 문서화하고 필요한 inspect/checkpoint/test gap만 보강하는 것이다.

2026-05-12 기준으로 001은 current architecture mapping 기준 완료로 닫는다. 완료 판정은 `AgentLoop`/`AgentRunner`/`SessionManager`/`SessionTurnLock` 구조를 공식 구현 경로로 인정하고, 별도 durable `TurnState` 타입이나 phase enum 전면 도입을 비목표로 둔다는 뜻이다. 이후 변경은 001을 다시 여는 것이 아니라, 관련 owner spec에서 inspect projection, checkpoint schema, recovery evidence를 좁게 보강하는 방식으로 진행한다.

핵심은 기능 수를 늘리는 것이 아니라, 한 세션의 한 턴이 언제 시작되고 언제 닫히며, 어떤 결과가 기록될 수 있고 어떤 결과는 버려져야 하는지를 일관되게 설명할 수 있게 만드는 것이다.
