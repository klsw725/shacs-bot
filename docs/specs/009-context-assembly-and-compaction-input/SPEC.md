# context assembly and compaction input 아키텍처 명세

## 문서 목적

이 문서는 `docs/SYSTEM-FOUNDATION.md`, `docs/specs/001-session-kernel/SPEC.md`, `docs/specs/003-provider-runtime/SPEC.md`, `docs/specs/004-tool-runtime/SPEC.md`, `docs/specs/005-skill-system/SPEC.md`, `docs/specs/006-session-store/SPEC.md`, `docs/specs/007-main-orchestrator-policy/SPEC.md`, `docs/specs/008-configuration-profiles-and-runtime-layout/SPEC.md`를 바탕으로 `shacs-bot`의 문맥 조립과 compaction 입력 경계를 구현 가능한 수준으로 고정한다.

목표는 다음과 같다.

- provider 호출 직전 어떤 입력이 문맥에 들어가고 어떤 입력은 들어가면 안 되는지 고정한다.
- durable state, skill, tool/subagent 결과, compaction 결과를 어떻게 결정적으로 조립할지 정의한다.
- token budgeting, truncation, snapshot 경계를 명시한다.
- future Rust 구현에서 context builder, compaction planner, provider input snapshot 타입과 테스트 시나리오를 직접 도출할 수 있게 한다.

이 문서는 prompt 작성 팁 문서가 아니다. 구현이 이 문서와 충돌하면 편의상 텍스트를 이어 붙이지 말고 context assembly 의미론부터 다시 점검해야 한다.

이 spec의 완료 기준은 문자열 concat 수준의 POC가 아니라, 이 문서가 정의한 deterministic assembly, token budgeting, truncation, compaction input, provider input snapshot 규칙을 충족하는 **완전한 기능 구현과 검증**이다.

---

## 상위 기준과의 관계

이 문서는 다음 상위 기준을 전제로 한다.

- `MainOrchestrator`는 한 턴의 `context_building`을 통제하는 유일한 권한자다.
- session store는 durable한 사실만 제공하며, replay/resume 결과가 문맥 조립의 진실 원천이다.
- skill은 read-only 지식 팩이며 문맥 보강만 한다.
- provider runtime은 오케스트레이터가 만든 snapshot을 받아 실행할 뿐, 문맥을 독자적으로 바꾸지 않는다.
- compaction은 닫힌 턴 경계에서만 일어나며, 이후에도 핵심 작업 문맥은 유지되어야 한다.

따라서 이 문서의 context assembly는 런타임 메모리의 임시 버퍼, UI 전용 projection, executor 내부 캐시, 미확정 streaming 조각을 진실 원천으로 삼지 않는다.

---

## 범위

이 문서는 다음을 정의한다.

- context assembly의 공식 입력 원천
- assembly 단계와 정규화 규칙
- tool/subagent/provider 결과가 문맥에 들어오는 경계
- compaction input이 무엇이고 무엇이 아닌지
- token budgeting과 truncation 규칙
- provider input snapshot에 포함되어야 할 것과 포함되면 안 되는 것
- 구현 불변식, 정상 시퀀스, 실패 시나리오, 금지 패턴, Rust 체크포인트, 테스트 관점

이 문서는 다음을 정의하지 않는다.

- 특정 provider 프롬프트 포맷 전체
- 고급 prompt compression 알고리즘의 내부 수학
- reasoning display UI
- 멀티세션 검색 랭킹 시스템

---

## 핵심 정의

### context assembly

context assembly는 이번 턴의 provider 호출에 필요한 입력을 공식 상태 원천에서 수집하고, 정규화하고, budget 안에 맞게 잘라낸 뒤, 하나의 provider input snapshot으로 굳히는 절차다.

### provider input snapshot

provider input snapshot은 특정 `Effect::InvokeModel`이 참조하는 완결된 입력 묶음이다. 이 snapshot은 이후 provider runtime이 추가 보정 없이 그대로 실행할 수 있어야 한다.

### durable context source

durable context source는 session store replay 결과, 세션 메타데이터, durable policy state, 확정된 skill selection, 닫힌 턴 결과처럼 resume 후에도 같은 값으로 재구성 가능한 문맥 원천이다.

### ephemeral candidate

ephemeral candidate는 아직 공식 상태로 확정되지 않았거나 현재 턴 안에서만 잠시 존재하는 산출물이다. 예:

- streaming chunk
- 미검증 tool stdout 조각
- provider partial delta
- executor 내부 캐시

ephemeral candidate는 provider input snapshot의 진실 원천이 아니다.

### compaction input

compaction input은 긴 세션 기록을 줄이기 위해 요약 또는 핵심 상태 추출에 사용할 공식 입력 집합이다. compaction input 역시 durable한 사실만 포함해야 하며, 턴 중간 임시 조각을 포함하면 안 된다.

### token budget

token budget은 provider 호출에 사용할 수 있는 전체 입력/출력 예산 중, 입력 문맥에 배정된 한도다.

### truncation

truncation은 budget을 넘는 문맥 후보에서 어떤 부분을 유지하고 어떤 부분을 줄일지 결정하는 과정이다.

---

## context assembly의 공식 입력 원천

### 반드시 포함 가능한 원천

다음은 문맥 조립에 사용할 수 있는 공식 원천이다.

- replay 또는 checkpoint+event tail로 복원된 `SessionState`
- 닫힌 턴의 확정된 conversation history 또는 정규화된 대화 표현
- durable policy state
- 현재 턴의 시작 command
- 현재 턴 이전에 확정된 tool 결과 요약 또는 구조화 결과
- 현재 턴 이전에 확정된 subagent 결과 요약 또는 구조화 결과
- 오케스트레이터가 선택한 skill 본문 snapshot
- compaction으로 생성된 durable summary block
- config에서 선택된 provider/tool schema snapshot

### 포함하면 안 되는 원천

- 아직 닫히지 않은 현재 턴의 partial provider chunk
- 아직 승인되지 않은 tool call 후보
- executor가 가지고 있는 미공식 캐시
- transport connection 상태
- UI projection 전용 필드
- late result로 폐기된 산출물
- secret 원문

---

## deterministic assembly 원칙

context assembly는 아래 원칙을 만족해야 한다.

1. 같은 durable state와 같은 turn input이면 같은 snapshot이 만들어져야 한다.
2. assembly는 명시적 단계와 우선순위를 가져야 한다.
3. assembly 중간에 파일 시스템 재스캔이나 외부 캐시 갱신으로 동일 턴의 입력이 흔들리면 안 된다.
4. truncation과 compaction 역시 결정적이어야 한다.
5. provider runtime은 이 snapshot을 수정하지 않고 실행해야 한다.

### 결정성을 깨는 요소 예시

- 랜덤 순서의 message 정렬
- 같은 턴 안에서 서로 다른 skill registry snapshot 사용
- 현재 시각에 따라 과거 메시지 선택 범위가 달라짐
- provider adapter가 자체적으로 system prompt를 더함

---

## context assembly 단계

### 1. source snapshot 고정

오케스트레이터는 `context_building` 진입 시 아래 스냅샷을 먼저 고정해야 한다.

- session replay 결과
- skill registry snapshot과 선택된 skill set
- provider profile snapshot
- tool schema snapshot
- 현재 turn input

이 단계가 끝나면 같은 턴 안에서 source가 흔들리면 안 된다.

### 2. semantic blocks 구성

assembly는 무조건 평문 문자열 concat부터 시작하지 말고, 의미 단위 블록을 먼저 만든다. 예:

- system/policy block
- compacted memory block
- recent conversation block
- tool result block
- subagent result block
- skill block
- current turn request block

### 3. block 정규화

각 block은 provider에 전달 가능한 공통 표현으로 정규화해야 한다.

예:

- 메시지 역할 구분
- 너무 큰 binary/text payload의 요약화
- tool 결과의 제목, 출처, 핵심 본문 정리
- subagent 결과의 상태, 결론, 근거 분리

### 4. token estimation

정규화된 block들에 대해 대략적 token estimate를 계산한다.

### 5. budgeting과 truncation

budget을 넘으면 block 우선순위 규칙에 따라 줄인다.

### 6. provider input snapshot 확정

최종 block 순서와 내용을 확정해 `InvokeModel` effect의 snapshot 필드로 저장한다.

---

## semantic block 규칙

### system/policy block

포함 가능 정보:

- session mode
- current tool availability summary
- provider/tool interaction 규칙
- durable safety policy 요약

포함 금지 정보:

- secret 원문
- UI 전용 문자열
- executor 내부 핸들

### compacted memory block

포함 가능 정보:

- 닫힌 턴 경계까지의 durable summary
- 장기 작업 목표
- 아직 유효한 사용자 선호나 결정 사항

포함 금지 정보:

- 이미 폐기된 시도 세부 로그 전체
- partial assistant draft

### recent conversation block

포함 가능 정보:

- 최근 닫힌 턴의 user/assistant/tool 결과 요약
- 현재 턴 시작 input

포함 금지 정보:

- 아직 미완료인 현재 턴의 내부 상태

### tool result block

포함 가능 정보:

- 오케스트레이터가 수용한 tool 결과
- tool 이름, 주요 출력, 실패 요약, correlation 가능한 최소 메타데이터

포함 금지 정보:

- raw stdout 전체를 무제한 첨부
- late result로 폐기된 tool 결과

### subagent result block

포함 가능 정보:

- 완료된 subagent의 핵심 결론
- 실패 여부와 재시도 필요성
- 필요한 경우 제한된 supporting details

포함 금지 정보:

- subagent 내부의 전체 transcript를 무제한 주입
- 아직 병합 승인되지 않은 가설 결과

### skill block

포함 가능 정보:

- 선택된 `SKILL.md` 본문 전체 또는 발췌
- 스킬 출처와 표시 이름에 대한 최소 메타데이터

포함 금지 정보:

- 충돌 상태거나 malformed인 스킬 본문
- 턴 도중 새로 발견된 다른 버전의 스킬 본문

---

## compaction input 규칙

compaction은 문맥을 줄이기 위한 별도 절차이지만, 그 입력 자체도 엄격해야 한다.

### compaction input에 포함되어야 하는 것

- 닫힌 턴의 공식 대화 기록
- durable policy state 변화 중 다음 턴 해석에 필요한 것
- 수용된 tool 결과의 핵심 사실
- 수용된 subagent 결과의 핵심 사실
- 장기 작업 목표, 보류 중 제약, 명시적 사용자 선호

### compaction input에 포함되면 안 되는 것

- 열린 턴의 미완료 산출물
- late result
- approval 대기 중이던 미실행 후보
- executor 내부 재시도 로그 전체
- raw secret

### compaction boundary 규칙

- compaction input은 닫힌 턴 경계까지만 수집한다.
- 현재 열린 턴이 있으면 그 턴은 compaction input에 포함하지 않는다.
- compaction 결과는 durable summary block으로 세션 상태에 편입될 수 있지만, 원래 event truth를 대체한다고 해석하면 안 된다.

---

## token budgeting 규칙

### 기본 원칙

1. token budget은 effect 발행 전에 오케스트레이터가 계산한다.
2. 입력 budget과 출력 budget은 구분되어야 한다.
3. budget 초과 시 무작정 최근 문자열을 잘라내지 말고 block 우선순위를 따라야 한다.
4. secret, corrupted text, binary dump는 budget을 차지하는 공식 문맥이 되면 안 된다.

### 권장 block 우선순위

높은 우선순위부터 유지한다.

1. current turn request block
2. system/policy block
3. 최근 닫힌 턴의 핵심 conversation block
4. compacted memory block
5. 현재 턴에 직접 관련된 tool/subagent result block
6. skill block
7. 더 오래된 부가 세부사항

### truncation 규칙

- 먼저 오래된 세부 블록을 줄인다.
- 그다음 block 내부에서 세부 payload를 요약한다.
- 그래도 넘치면 더 오래된 conversation을 summary reference로 대체한다.
- current turn request와 필수 policy block은 마지막까지 보존한다.

### 절대 잘라내면 안 되는 것

- 현재 턴의 사용자 요청 핵심 내용
- 현재 턴에서 허용된 tool schema에 대한 최소 설명
- 안전 정책상 필수 제약
- compaction 이후에도 유지하기로 약속된 장기 목표 핵심 요약

---

## provider input snapshot 명세

`InvokeModel` effect에 들어가는 snapshot은 최소한 아래 의미의 필드를 가져야 한다.

- `session_id`
- `turn_id`
- `effect_id`
- `provider_profile_snapshot`
- `messages_snapshot` 또는 구조화된 block list
- `system_context_snapshot`
- `tool_schema_snapshot`
- `skill_context_snapshot` optional
- `compacted_memory_snapshot` optional
- `token_budget_snapshot`
- `assembly_metadata`, 예: source sequences, compaction boundary, truncation markers

### snapshot에 포함되어야 하는 것

- 어떤 durable sources가 사용되었는지 설명 가능한 최소 메타데이터
- truncation 또는 summary 대체가 있었다는 사실
- 이 호출 시점의 tool availability 범위

### snapshot에 포함되면 안 되는 것

- provider adapter 내부 커넥션 상태
- API key 원문
- executor 재시도 핸들
- late result 후보
- 현재 턴의 partial delta

### snapshot 불변성

effect가 발행된 뒤 provider runtime은 snapshot을 수정해선 안 된다. 재시도가 필요하면 새 snapshot과 새 effect가 만들어져야 한다.

---

## tool/subagent 결과의 문맥 편입 규칙

### tool 결과

- tool 결과는 오케스트레이터가 수용한 뒤에만 다음 provider 호출 문맥에 편입될 수 있다.
- 편입 시 raw result 전체를 항상 넣지 말고, 구조화된 요약 또는 필요한 본문만 포함해야 한다.
- 실패한 tool 결과도 다음 판단에 필요하면 실패 사실과 핵심 오류만 포함할 수 있다.

### subagent 결과

- subagent 결과 역시 오케스트레이터가 병합을 승인한 뒤에만 편입할 수 있다.
- 병합 전 가설이나 초안은 provider input snapshot에 포함되면 안 된다.
- subagent transcript 전체는 기본적으로 제외하고, 결론과 필요한 근거만 넣는다.

---

## 정상 시퀀스 예시

### 예시 1. compacted memory와 recent tool 결과를 포함한 정상 assembly

```text
1) 세션에는 닫힌 턴 기준 compacted summary가 이미 durable state로 존재한다.
2) 새 턴이 accepted 된다.
3) context_building에서 오케스트레이터는 session replay 결과, selected skills, tool schema snapshot, current turn input을 고정한다.
4) semantic blocks를 만든다. system/policy, compacted memory, recent conversation, recent tool result, current request, selected skill block.
5) token estimate 결과 budget을 약간 초과한다.
6) 오케스트레이터는 오래된 conversation 세부를 줄이고 compacted summary는 유지한다.
7) 최종 provider input snapshot을 확정한다.
8) provider runtime은 그 snapshot 그대로 호출한다.
```

### 예시 2. tool 결과를 재호출 문맥에 편입

```text
1) provider가 read tool call을 제안한다.
2) 오케스트레이터가 이를 승인한다.
3) tool runtime이 결과를 반환한다.
4) 오케스트레이터는 결과를 수용하고 current turn의 공식 intermediate fact로 정규화한다.
5) 다음 model invocation을 위해 tool result block을 만든다.
6) raw stdout 전체 대신 핵심 결과와 출처 메타데이터만 snapshot에 포함한다.
```

---

## 실패 시나리오

### 시나리오 1. late tool result가 문맥에 섞이는 경우

- 잘못된 동작: retry 후 늦게 도착한 이전 tool 결과를 다음 provider 호출 문맥에 포함
- 왜 실패인가: 현재 활성 턴 흐름과 무관한 결과가 섞여 deterministic assembly가 깨진다.

### 시나리오 2. partial streaming delta를 durable context처럼 사용

- 잘못된 동작: 아직 완료되지 않은 provider chunk를 현재 턴 재호출 문맥에 포함
- 왜 실패인가: 미확정 산출물을 공식 입력으로 승격하게 된다.

### 시나리오 3. budget 초과 시 필수 policy block 제거

- 잘못된 동작: token을 맞추기 위해 permission/tool 규칙 설명 블록을 먼저 삭제
- 왜 실패인가: 모델이 현재 턴의 제약을 잃는다.

### 시나리오 4. secret을 snapshot에 포함

- 잘못된 동작: provider input snapshot debug dump에 API key가 포함
- 왜 실패인가: snapshot은 추적 가능한 기록이므로 secret 노출이 된다.

---

## 구현 불변식

아래 불변식은 future Rust 구현에서 타입, assertion, 테스트로 강제 대상이다.

1. context assembly는 durable source snapshot을 먼저 고정한 뒤 진행해야 한다.
2. 같은 durable state와 같은 turn input은 같은 provider input snapshot을 만들어야 한다.
3. late result와 미확정 partial 산출물은 공식 문맥 source가 될 수 없다.
4. compaction input은 닫힌 턴 경계까지만 포함해야 한다.
5. token budgeting은 block 우선순위 기반으로 동작해야 한다.
6. 현재 턴 요청과 필수 policy block은 truncation의 마지막까지 보존되어야 한다.
7. tool/subagent 결과는 오케스트레이터 수용 후에만 문맥에 편입될 수 있다.
8. provider input snapshot은 effect 발행 후 불변이어야 한다.
9. secret 원문은 provider input snapshot과 trace에 포함되면 안 된다.
10. compaction summary는 원래 event truth를 대체하는 독립 진실 원천으로 취급되면 안 된다.

---

## 금지 패턴

### 1. 문자열 concat 중심의 무차별 조립

금지 예:

- 모든 기록과 출력과 스킬을 순서 없이 이어 붙인 뒤 token limit만 맞춤

왜 금지인가:

- block 의미와 우선순위가 사라진다.
- truncation 결과가 설명 불가능해진다.

### 2. executor 내부 캐시를 공식 문맥 원천으로 사용

금지 예:

- provider adapter 내부에 남은 최근 요청 버퍼를 다음 호출 문맥으로 재사용
- tool runtime 캐시 stdout를 세션 기록보다 우선 반영

왜 금지인가:

- replay/resume와 같은 문맥을 재현할 수 없다.
- 오케스트레이터 단일 권한 원칙이 깨진다.

### 3. compaction에 열린 턴 포함

금지 예:

- `tool_pending` 상태의 임시 결과를 summary에 섞음

왜 금지인가:

- 미완료 사실을 durable summary로 굳히게 된다.
- recovery semantics가 흐려진다.

### 4. budget 초과 시 최근 요청보다 오래된 부가 정보 우선 유지

금지 예:

- 현재 요청을 잘라내고 오래된 transcript를 남김

왜 금지인가:

- 모델이 이번 턴의 핵심을 잃는다.
- 현재 턴 정확성이 깨진다.

### 5. snapshot에 secret 또는 실행 핸들 포함

금지 예:

- API key, bearer token, file descriptor, process id를 input snapshot에 저장

왜 금지인가:

- 보안과 재현성 모두 깨진다.

---

## Rust 구현으로 이어질 체크포인트

구체 타입 이름은 바뀔 수 있지만, 아래 질문에는 모두 "예"라고 답할 수 있어야 한다.

- `ContextSourceSnapshot`, `SemanticBlock`, `TokenBudget`, `TruncationPlan`, `ProviderInputSnapshot` 같은 타입 경계가 분리되는가?
- block 우선순위와 truncation 규칙을 독립 함수로 테스트할 수 있는가?
- compaction input collector가 닫힌 턴까지만 수집하도록 강제되는가?
- tool/subagent 결과 편입이 "수용된 공식 결과"만 대상으로 제한되는가?
- provider input snapshot에 source sequence나 compaction boundary 같은 assembly metadata를 남길 수 있는가?
- secret redaction 또는 secret exclusion 검사가 snapshot 생성 단계에 존재하는가?
- 같은 source snapshot으로 두 번 assembly 했을 때 동일 결과가 나오는가?

---

## 테스트 관점에서 꼭 검증할 시나리오

future Rust 구현은 최소한 아래 시나리오를 테스트로 가져갈 수 있어야 한다.

- 같은 session replay 결과와 같은 turn input에서 동일한 provider input snapshot이 나오는가
- late tool result가 다음 model invocation 문맥에 포함되지 않는가
- malformed 또는 conflicted skill이 skill block에 들어가지 않는가
- budget 초과 시 오래된 conversation 세부가 먼저 줄어들고 current turn request는 유지되는가
- compaction input collector가 열린 턴 내용을 제외하는가
- compacted summary와 recent conversation이 함께 있을 때 block 우선순위가 문서대로 적용되는가
- raw tool stdout가 너무 클 때 구조화 요약으로 대체되는가
- secret reference가 실제 secret 원문으로 snapshot에 새겨지지 않는가

---

## 명시적 비범위

이 문서는 다음을 정의하지 않는다.

- 벤더별 prompt wire format 전체 명세
- 모델 품질을 높이기 위한 고급 프롬프트 엔지니어링 규칙집
- semantic retrieval 시스템 전체
- 멀티에이전트 전역 공유 메모리

이 항목들은 필요가 생기면 별도 문서에서 다룬다. 단, 어떤 확장도 "공식 문맥은 durable한 사실에서 결정적으로 조립되어야 하며, provider input snapshot은 설명 가능하고 재현 가능해야 한다"는 원칙을 뒤집어서는 안 된다.

---

## 결론

`shacs-bot`의 context assembly and compaction input은 단순한 prompt 생성기가 아니다. 이 계층은 어떤 정보가 공식 문맥이 될 수 있는지, 긴 세션을 어떻게 줄여도 핵심 작업 상태를 잃지 않는지, provider에 정확히 무엇을 넘길지를 고정하는 실행 계약이다.

핵심은 네 가지다.

- 문맥은 durable한 공식 원천에서만 조립되어야 한다.
- tool/subagent 결과는 오케스트레이터 수용 후에만 문맥에 편입될 수 있다.
- token budgeting과 truncation은 block 우선순위에 따라 결정적으로 동작해야 한다.
- provider input snapshot은 나중에 다시 봐도 왜 그 입력이 선택됐는지 설명 가능해야 한다.

이 구조가 지켜져야 `shacs-bot`은 긴 세션과 compaction이 있어도 흔들리지 않고, 같은 상태에서 같은 입력을 다시 만들 수 있는 self-hosted assistant runtime으로 유지될 수 있다.
