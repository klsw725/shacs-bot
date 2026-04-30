# subagent runtime 아키텍처 명세

## 문서 목적

이 문서는 `docs/SYSTEM-FOUNDATION.md`, `docs/specs/001-session-kernel/SPEC.md`, `docs/specs/003-provider-runtime/SPEC.md`, `docs/specs/004-tool-runtime/SPEC.md`, `docs/specs/007-main-orchestrator-policy/SPEC.md`, `docs/specs/009-context-assembly-and-compaction-input/SPEC.md`, `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`를 바탕으로 `shacs-bot`의 subagent runtime을 구현 가능한 수준으로 고정한다.

목표는 다음과 같다.

- child task의 identity, lifecycle, spawn envelope, result payload를 정의한다.
- 부모 턴에서 자식 실행으로 어떤 context, policy, budget이 상속되는지 고정한다.
- merge authority와 stale child result 처리 규칙을 명시한다.
- parallelism, cancellation, timeout, synthetic command 재진입 규칙을 결정한다.
- future Rust 구현에서 subagent registry, child task state, merge decision API, 테스트 시나리오를 직접 도출할 수 있게 한다.

이 문서는 "서브에이전트를 나중에 붙일 수 있게 해 두자"는 수준의 방향 메모가 아니다. 구현이 이 문서와 충돌하면 임시 스레드나 임의 비동기 태스크로 대충 대체하지 말고 subagent runtime 계약부터 다시 점검해야 한다.

이 spec의 완료 기준은 병렬 호출 데모나 멀티에이전트 분위기의 POC가 아니라, 이 문서가 정의한 child identity, spawn contract, inherited policy/budget, reentry/merge authority, stale result handling을 충족하는 **완전한 기능 구현과 검증**이다.

---

## 상위 기준과의 관계

이 문서는 다음 상위 기준을 전제로 한다.

- `MainOrchestrator`는 세션 상태를 바꿀 수 있는 유일한 권한자다.
- subagent는 독립적인 상태 소유자가 아니라, 부모 턴이 발행한 외부 실행 경계다.
- subagent 결과는 항상 후보 결과이며, 공식 세션 상태는 오케스트레이터가 병합할 때만 바뀐다.
- 목표는 단일 사용자 self-hosted 런타임의 정확성이지, 분산 멀티에이전트 플랫폼이 아니다.

따라서 이 문서는 독립 노드 간 합의, 여러 사용자의 task ownership, remote team inbox, agent-to-agent 직접 협상 프로토콜을 다루지 않는다.

이 문서가 말하는 subagent는 어디까지나 `MainOrchestrator`가 제한된 목적과 예산을 부여해 잠시 실행시키는 child task다.

---

## 범위

이 문서는 다음을 정의한다.

- child task identity와 lifecycle state
- subagent spawn envelope 필수 필드
- 부모에서 자식으로 상속되는 context, policy, budget
- child result payload 구조
- merge authority와 merge decision 규칙
- parallelism 제약과 child concurrency ceiling
- cancellation, timeout, stale result 처리
- synthetic command 재진입 규칙
- 구현 불변식, 정상 시퀀스, 실패 시퀀스, 금지 패턴, Rust 체크포인트, 테스트 관점

이 문서는 다음을 정의하지 않는다.

- 개별 subagent persona prompt의 세부 문구
- agent marketplace
- 원격 에이전트 호스팅 플랫폼
- agent billing 체계
- 멀티유저 agent 권한 위임 체계

---

## 핵심 정의

### subagent

subagent는 부모 턴이 가진 작업을 보조하기 위해 `MainOrchestrator`가 명시적으로 spawn한 제한된 child executor다. subagent는 독립 세션 진실 원천이 아니라 부모 턴에 종속된 외부 effect executor다.

### child task

child task는 특정 subagent 실행의 수명주기와 상관관계를 나타내는 공식 단위다. child task는 spawn 시점부터 completed, failed, cancelled, timed_out, stale로 닫힐 때까지 추적 가능해야 한다.

### spawn envelope

spawn envelope는 오케스트레이터가 승인한 단일 child task 실행 계약이다. subagent runtime은 envelope 밖의 권한이나 문맥을 추측하면 안 된다.

### inherited context

inherited context는 부모 턴이 자식에게 내려주는 작업 문맥의 축약본이다. 이 문맥은 부모 전체 transcript 복제가 아니라, child task 목적에 필요한 공식 snapshot이어야 한다.

### merge

merge는 child result를 공식 세션 상태나 현재 턴의 문맥으로 채택할지, 폐기할지, 요약만 반영할지를 결정하는 오케스트레이터 정책 단계다.

### stale child result

stale child result는 child가 결과를 돌려주기는 했지만, 부모 턴이 이미 닫혔거나 더 새로운 child/retry가 유효해서 지금은 공식 입력으로 받아들일 수 없는 결과다.

### synthetic command reentry

synthetic command reentry는 외부 사용자 입력이 아니라 runtime 내부 서비스가 공식 command 형태로 오케스트레이터에 결과를 재주입하는 경로다. subagent 결과는 반드시 synthetic command를 통해서만 재진입해야 한다.

---

## subagent runtime의 기본 원칙

1. subagent spawn 여부는 `MainOrchestrator`가 결정한다.
2. 자식은 부모 권한을 상속받지만, 확대 상속은 허용되지 않는다.
3. child result는 후보 결과일 뿐, merge 전에는 공식 상태가 아니다.
4. 같은 부모 상태와 같은 spawn envelope라면 child 실행 의미가 설명 가능해야 한다.
5. 부모 턴이 닫힌 뒤 child 결과가 와도 턴을 되살리면 안 된다.
6. subagent 병렬성은 성능보다 부모 턴 정확성과 budget 보호를 우선해야 한다.

---

## child task identity

child task는 최소한 아래 식별자를 가져야 한다.

- `session_id`
- `parent_turn_id`
- `child_task_id`
- `spawn_effect_id`
- `subagent_kind`
- `spawn_sequence`

### identity 규칙

1. `child_task_id`는 세션 안에서 고유해야 한다.
2. 같은 부모 턴 안에서는 `spawn_sequence`가 단조 증가해야 한다.
3. retry는 같은 child task의 상태 갱신이 아니라, 원칙적으로 새 child effect 또는 새 child task로 설명 가능해야 한다.
4. stale result 판정은 최소한 `session_id`, `parent_turn_id`, `child_task_id`, active 여부를 함께 확인해야 한다.

---

## child task lifecycle

child task는 최소한 아래 state를 가져야 한다.

1. `spawn_requested`
2. `spawned`
3. `running`
4. `awaiting_merge`
5. `completed`
6. `failed`
7. `cancelled`
8. `timed_out`
9. `stale`

### state 의미

#### `spawn_requested`

오케스트레이터가 child task를 만들기로 결정했고, spawn envelope를 확정하는 단계다.

#### `spawned`

subagent runtime에 실행 effect가 전달되었으나, 아직 본격 실행 결과가 들어오기 전 상태다.

#### `running`

child가 실제로 작업 중인 상태다. 내부 streaming이나 intermediate progress가 있더라도 공식 merge 가능 상태는 아니다.

#### `awaiting_merge`

child가 종료 결과를 돌려줬고, 오케스트레이터가 이를 평가해 병합 여부를 결정하는 단계다.

#### `completed`

오케스트레이터가 child result를 채택 또는 요약 반영까지 끝내고 child task를 닫은 상태다.

#### `failed`, `cancelled`, `timed_out`

child 실행이 실패, 취소, 시간 초과로 닫힌 상태다. 이 경우에도 오케스트레이터는 결과를 공식 failure fact로 반영할지, retry할지, 부모 턴을 중단할지 정책 판단을 해야 한다.

#### `stale`

결과는 도착했지만 더 이상 현재 부모 흐름에 유효하지 않은 상태다. stale은 관찰 이벤트가 될 수는 있어도 merge 대상이 아니다.

---

## spawn envelope 명세

subagent spawn effect는 최소한 아래 의미의 필드를 가져야 한다.

- `session_id`
- `parent_turn_id`
- `child_task_id`
- `spawn_effect_id`
- `subagent_kind`
- `task_goal`
- `task_scope`
- `inherited_context_snapshot`
- `inherited_policy_snapshot`
- `inherited_safety_snapshot`
- `input_budget_snapshot`
- `output_budget_snapshot`
- `timeout_ms`
- `parallelism_group`
- `issued_at`

### envelope 규칙

1. child는 envelope에 없는 추가 목표를 임의로 받으면 안 된다.
2. `task_scope`는 부모 작업을 얼마나 잘라서 맡기는지 설명 가능해야 한다.
3. context snapshot은 child가 수행에 필요한 최소 공식 문맥만 포함해야 한다.
4. policy/safety snapshot은 부모보다 넓어지면 안 된다.
5. timeout과 budget은 child 전용 한도로 해석되어야 한다.

---

## inherited context, policy, budget

### inherited context

자식에게 내려갈 수 있는 문맥은 최소한 아래 범주로 제한해야 한다.

- 현재 부모 턴 목표의 축약본
- child가 담당할 하위 문제 정의
- 필요한 최근 대화 요약
- 필요한 tool/subagent 결과 요약
- 관련 skill snapshot

### 포함하면 안 되는 부모 문맥

- 부모의 전체 raw transcript 무제한 복제
- merge 전 가설 결과
- expired approval state
- raw secret value
- 현재 턴의 unrelated intermediate buffers

### inherited policy

자식은 최소한 아래 정책을 상속받아야 한다.

- 부모 permission mode ceiling
- 허용 capability 범위
- 허용 tool schema 범위
- network/path/secret boundary
- late result 처리와 cancellation 정책의 상위 제한

### inherited budget

자식은 최소한 아래 예산을 상속 또는 재할당받아야 한다.

- token input budget
- token output budget
- wall clock timeout
- 최대 child spawn depth
- 부모 턴 전체 parallelism ceiling 안에서의 slot 수

### 상속 원칙

1. 자식은 부모보다 넓은 policy를 가지면 안 된다.
2. 자식 예산은 부모 예산의 부분집합이어야 한다.
3. 자식에게 내려간 context snapshot은 effect 발행 시점 기준으로 고정되어야 한다.
4. 같은 부모 입력에서 같은 spawn selection이면 같은 envelope가 재현 가능해야 한다.

---

## child result payload

child 결과는 최소한 아래 의미의 공통 필드를 가져야 한다.

- `session_id`
- `parent_turn_id`
- `child_task_id`
- `spawn_effect_id`
- `subagent_kind`
- `status`
- `started_at`
- `finished_at`
- `duration_ms`
- `summary`
- `structured_result` optional
- `error` optional
- `observations` optional
- `budget_usage` optional

### 결과 payload 원칙

1. child transcript 전체를 기본 payload로 삼으면 안 된다.
2. `summary`는 부모가 다음 결정을 내릴 수 있을 정도의 핵심 결론이어야 한다.
3. `structured_result`는 merge 판단과 context 편입에 필요한 최소 구조여야 한다.
4. raw secret, executor handle, 내부 runtime cache는 payload에 포함되면 안 된다.

---

## merge authority

### 병합 권한자

child result를 현재 턴 문맥이나 세션 상태에 채택할 수 있는 유일한 권한자는 `MainOrchestrator`다.

subagent runtime은 다음을 할 수 없다.

- child 결론을 assistant 메시지처럼 세션 기록에 append
- child가 요청한 tool을 자동 실행
- child 결과끼리 자체 병합 후 확정 상태처럼 재주입

### merge 결정 종류

오케스트레이터는 child result를 보고 최소한 아래 중 하나를 결정할 수 있어야 한다.

- `accept_full`
- `accept_summary_only`
- `accept_failure_fact`
- `retry_child`
- `discard_as_stale`
- `abort_parent_turn`

### merge 판단 기준

- child correlation 유효성
- 부모 턴이 아직 열려 있는지
- 더 새로운 sibling 또는 retry child가 이미 채택되었는지
- result가 inherited policy를 위반하지 않았는지
- budget 소진 상태

---

## parallelism 제약

subagent 병렬성은 무제한 fan-out 구조가 되어서는 안 된다.

### 기본 제약

1. 한 부모 턴에는 명시적 parallelism ceiling이 있어야 한다.
2. sibling child 수가 ceiling을 넘으면 즉시 spawn하지 말고 대기 또는 거절해야 한다.
3. 동일 목적을 가진 child를 중복 spawn하면 dedupe 또는 명시적 rationale가 있어야 한다.
4. 자식 안에서 다시 자식을 낳는 구조가 있다면 depth ceiling이 있어야 한다.

### 병렬 merge 규칙

여러 child 결과가 동시에 와도 병합 순서와 채택 여부는 오케스트레이터가 결정한다.

- 더 먼저 도착했다고 자동 채택하면 안 된다.
- 같은 scope를 맡은 sibling 둘을 동시에 full merge하면 안 된다.
- 서로 다른 scope라면 둘 다 summary-only로 편입할 수 있다.

---

## cancellation, timeout, stale child result

### cancellation

- 사용자가 턴 취소를 요청하거나 부모 턴이 abort되면 활성 child는 best-effort 취소 대상이 된다.
- child runtime은 취소 결과를 synthetic command로 보고해야 한다.
- 부모 턴이 이미 닫혔다면 늦게 온 cancellation acknowledgement는 stale 관찰 이벤트일 뿐이다.

### timeout

- child는 envelope의 `timeout_ms`를 넘기면 `timed_out`으로 정규화되어야 한다.
- timeout 이후 retry 여부는 오케스트레이터가 결정한다.
- timeout 뒤 실제 child 결과가 늦게 오면 stale 판정을 해야 한다.

### stale child result 판정 기준

아래 중 하나라도 참이면 stale child result다.

- 부모 턴이 이미 `completed` 또는 `aborted`
- 해당 `child_task_id`가 더 이상 active set에 없음
- 같은 scope에 대한 더 새로운 retry child가 이미 채택됨
- recovery 이후 이전 프로세스의 잔여 child 결과가 도착함

### stale 처리 규칙

1. stale result는 공식 문맥이나 세션 기록의 결과 본문으로 승격되면 안 된다.
2. 필요하면 observability 이벤트로만 남길 수 있다.
3. stale result가 부모 턴을 다시 열거나 retry count를 되돌리면 안 된다.

---

## synthetic command 재진입

child result는 반드시 synthetic command로 재진입해야 한다.

초기 구현은 최소한 아래 의미의 command를 가질 수 있어야 한다.

- `SubagentCompleted`
- `SubagentFailed`
- `SubagentTimedOut`
- `SubagentCancelled`
- `SubagentProgressObserved` optional

### 재진입 규칙

1. 모든 command는 `session_id`, `parent_turn_id`, `child_task_id`, `spawn_effect_id`를 포함해야 한다.
2. 오케스트레이터는 command 수신 시 correlation과 active child set을 검증해야 한다.
3. synthetic command도 외부 사용자 command와 같은 공식 진입점으로 처리되어야 한다.
4. subagent runtime이 세션 상태를 직접 건너뛰어 변경하면 안 된다.

---

## 결정표

### 1. child spawn 결정표

| 조건 | 결정 | 비고 |
| --- | --- | --- |
| 부모 턴 활성, budget 여유, parallelism slot 있음 | spawn 허용 | 새 child task 생성 |
| 부모 턴 활성, slot 없음 | 지연 또는 거절 | 무제한 병렬 금지 |
| 부모 턴이 approval 대기 중 | 기본적으로 spawn 금지 | 승인 전 fan-out 금지 |
| 부모 턴이 종료됨 | 거절 | child 생성 불가 |
| inherited safety snapshot이 불완전 | 거절 | unsafe spawn 금지 |

### 2. child result merge 결정표

| child status | correlation 유효 | 부모 턴 상태 | 결정 |
| --- | --- | --- | --- |
| `completed` | 예 | 활성 | merge 평가 |
| `failed` | 예 | 활성 | failure fact 반영 또는 retry |
| `timed_out` | 예 | 활성 | retry 또는 abort 평가 |
| 모든 status | 아니오 | 무관 | stale 또는 즉시 폐기 |
| 모든 status | 예 | 부모 턴 종료 | stale |

---

## 정상 시퀀스 예시

### 예시 1. read-only 조사 child를 병렬로 둘 생성

```text
1) 부모 턴이 context_building 중 두 개의 독립 조사 범위를 식별한다.
2) MainOrchestrator는 parallelism ceiling 안에서 child A, child B를 spawn한다.
3) 각 child는 축약된 inherited context와 read-only safety snapshot만 받는다.
4) child A가 먼저 완료되지만, 오케스트레이터는 즉시 세션 확정 대신 awaiting_merge로 둔다.
5) child B도 완료된다.
6) 오케스트레이터는 두 결과를 평가해 각각 summary-only로 부모 문맥에 편입한다.
7) 이후 부모는 새 model invocation을 진행한다.
```

### 예시 2. child 결과의 synthetic command 재진입

```text
1) child task가 완료된다.
2) subagent runtime은 SubagentCompleted command를 생성한다.
3) command에는 session_id, parent_turn_id, child_task_id, spawn_effect_id가 포함된다.
4) MainOrchestrator는 active child set과 correlation을 검증한다.
5) 검증이 통과하면 merge decision을 수행한다.
6) 그 뒤에만 child 결과가 공식 문맥에 편입된다.
```

---

## 실패 시나리오

### 시나리오 1. 부모 턴 종료 후 늦게 도착한 child 결과

- 잘못된 동작: 이미 완료된 부모 턴 뒤에 도착한 child 결과를 conversation에 추가
- 올바른 동작: stale로 폐기하고 필요 시 관찰 이벤트만 남김

### 시나리오 2. 자식이 부모보다 넓은 권한 사용

- 잘못된 동작: 부모는 read-only였는데 child가 write/exec를 수행
- 올바른 동작: spawn envelope 또는 runtime guard에서 거절

### 시나리오 3. child transcript 전체를 문맥에 무제한 편입

- 잘못된 동작: child 내부 사고 과정과 raw 로그 전체를 다음 provider 호출 문맥에 넣음
- 올바른 동작: summary와 필요한 structured_result만 편입

---

## 구현 불변식

아래 불변식은 future Rust 구현에서 타입, assertion, 테스트로 강제 대상이다.

1. child spawn 여부는 `MainOrchestrator`가 결정한다.
2. child task identity는 `session_id`, `parent_turn_id`, `child_task_id`로 추적 가능해야 한다.
3. child는 부모보다 넓은 policy, safety, budget을 가지면 안 된다.
4. child result는 merge 전까지 공식 상태가 아니다.
5. merge authority는 끝까지 `MainOrchestrator` 하나뿐이다.
6. stale child result는 이미 닫힌 턴의 결과를 바꾸면 안 된다.
7. synthetic command 재진입은 correlation 검증 없이 수용되면 안 된다.
8. 병렬 child 수는 explicit ceiling을 가져야 한다.
9. timeout과 cancellation은 child runtime의 사실 보고이지만, 후속 결정은 오케스트레이터가 해야 한다.
10. child transcript 전체가 기본 문맥 원천이 되면 안 된다.

---

## 금지 패턴

### 1. child를 독립 상태 소유자로 취급

금지 예:

- child가 세션 기록을 직접 수정
- child가 자신의 결과를 공식 assistant 답변처럼 저장

왜 금지인가:

- 메인 오케스트레이터 단일 권한 원칙이 깨진다.

### 2. child 결과 자동 병합

금지 예:

- `completed` 결과가 도착하는 즉시 오케스트레이터 검증 없이 conversation에 편입

왜 금지인가:

- stale result와 policy 위반 결과를 구분할 수 없게 된다.

### 3. 무제한 fan-out

금지 예:

- 한 턴이 수십 개 child를 제한 없이 생성

왜 금지인가:

- budget 고갈과 merge 혼란이 생긴다.
- self-hosted 환경에서 복구와 설명 가능성이 무너진다.

### 4. synthetic command를 우회한 직접 callback 병합

금지 예:

- child runtime이 오케스트레이터 내부 메서드를 직접 호출해 결과를 주입

왜 금지인가:

- 공식 command/event/effect 경계가 흐려진다.

---

## Rust 구현으로 이어질 체크포인트

구체 타입 이름은 바뀔 수 있지만, 아래 질문에는 모두 "예"라고 답할 수 있어야 한다.

- `ChildTaskId`, `SpawnEnvelope`, `InheritedContextSnapshot`, `InheritedPolicySnapshot`, `ChildResultEnvelope`, `MergeDecision` 같은 타입 경계가 분리되는가?
- active child set과 stale 판정을 독립 로직으로 테스트할 수 있는가?
- parallelism ceiling과 depth ceiling을 오케스트레이터 정책으로 표현할 수 있는가?
- child result synthetic command를 일반 command 처리 경로로 재진입시킬 수 있는가?
- child 결과의 summary-only merge와 full merge를 구분할 수 있는가?
- timeout/cancellation 이후 late child result를 stale로 판정하는 로직이 있는가?

---

## 테스트 관점에서 꼭 검증할 시나리오

future Rust 구현은 최소한 아래 시나리오를 테스트로 가져갈 수 있어야 한다.

- 부모 read-only safety snapshot이 child에도 그대로 적용되는가
- parallelism ceiling 초과 시 새 child spawn이 지연 또는 거절되는가
- child completion이 와도 correlation이 틀리면 stale로 폐기되는가
- 부모 턴 종료 후 도착한 child 결과가 merge되지 않는가
- child timeout 후 오케스트레이터가 retry 또는 abort를 명시적으로 결정하는가
- child transcript 전체가 아니라 summary만 다음 문맥에 편입되는가
- synthetic command에 필요한 식별자가 빠지면 거절되는가
- 동일 scope의 retry child가 채택된 뒤 이전 child 결과가 stale 처리되는가

---

## 명시적 비범위

이 문서는 다음을 정의하지 않는다.

- 원격 agent fleet 관리
- 멀티유저 shared task board
- agent billing이나 과금 분배
- 중앙 coordinator 없는 agent 간 직접 합의

이 항목들은 필요가 생기면 별도 문서에서 다룬다. 단, 어떤 확장도 child result는 후보 결과일 뿐이며 공식 병합 권한은 `MainOrchestrator`에 있다는 원칙을 뒤집어서는 안 된다.

---

## 결론

`shacs-bot`의 subagent runtime은 멀티에이전트 연출용 부가 기능이 아니다. 이 계층은 부모 턴이 제한된 하위 작업을 안전하게 병렬화하고, 그 결과를 다시 메인 오케스트레이터 아래로 회수하는 실행 계약이다.

핵심은 네 가지다.

- child task는 명확한 identity와 lifecycle을 가져야 한다.
- spawn envelope는 context, policy, safety, budget을 좁은 snapshot으로 고정해야 한다.
- child result는 synthetic command로만 재진입하고, merge authority는 오직 `MainOrchestrator`가 가진다.
- stale, timeout, cancellation, parallelism 제약을 명시적으로 다뤄야 부모 턴 정확성이 유지된다.

이 구조가 지켜져야 `shacs-bot`은 subagent를 도입해도 단일 권한 오케스트레이션을 유지하면서, 왜 어떤 child 결과를 채택했고 어떤 결과를 버렸는지 끝까지 설명할 수 있다.
