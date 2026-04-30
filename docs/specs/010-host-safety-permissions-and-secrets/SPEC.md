# host safety, permissions, and secrets 아키텍처 명세

## 문서 목적

이 문서는 `docs/SYSTEM-FOUNDATION.md`, `docs/specs/001-session-kernel/SPEC.md`, `docs/specs/004-tool-runtime/SPEC.md`, `docs/specs/007-main-orchestrator-policy/SPEC.md`, `docs/specs/008-configuration-profiles-and-runtime-layout/SPEC.md`, `docs/specs/009-context-assembly-and-compaction-input/SPEC.md`를 바탕으로 `shacs-bot`의 host safety 경계, permission model, secret handling 계약을 구현 가능한 수준으로 고정한다.

목표는 다음과 같다.

- host 상의 filesystem, network, process, secrets 접근 경계를 명시한다.
- permission mode, approval, redaction, inherited safety context의 의미를 고정한다.
- 어떤 정보가 durable policy state로 남고 어떤 정보는 절대 영속화되면 안 되는지 정의한다.
- future Rust 구현에서 permission evaluator, safety snapshot, secret redaction 계층, 테스트 시나리오를 직접 도출할 수 있게 한다.

이 문서는 안전성 철학을 말로만 설명하는 안내문이 아니다. 구현이 이 문서와 충돌하면 우회 로직을 추가하지 말고 host safety 의미론부터 다시 점검해야 한다.

이 spec의 완료 기준은 위험 작업 앞에 확인 문구를 하나 붙이는 POC가 아니라, 이 문서가 정의한 host boundary, permission mode, approval contract, redaction rule, persistence 금지 규칙을 충족하는 **완전한 기능 구현과 검증**이다.

---

## 상위 기준과의 관계

이 문서는 다음 상위 기준을 전제로 한다.

- `MainOrchestrator`는 세션 상태와 정책 확정을 담당하는 유일한 권한자다.
- tool runtime과 provider runtime은 외부 실행 경계이며, host safety 판단을 독자적으로 확정하지 않는다.
- `shacs-bot`은 self-hosted / personal-use 환경을 기본으로 하며, 사용자가 직접 자신의 머신과 워크스페이스에 대해 책임지는 구조다.
- 목표는 데모나 POC가 아니라, 실제 장기 사용 중에도 예측 가능하고 복구 가능한 안전 모델이다.

따라서 이 문서는 멀티유저 역할 분리, 관리자 승인 체계, 중앙 secret vault, 원격 운영 콘솔, 조직 단위 정책 배포를 다루지 않는다.

이 문서의 핵심은 단순하다. host safety는 도구 친화적 편의 기능이 아니라, `MainOrchestrator`가 턴마다 강제해야 하는 공식 정책이다.

---

## 범위

이 문서는 다음을 정의한다.

- filesystem, network, process, secrets에 대한 host safety boundary
- permission mode와 capability 분류
- approval 필요 여부와 승인 응답의 의미
- redaction 규칙과 secret 노출 금지 경계
- effect 실행 시 inherited safety context에 포함되어야 하는 정보
- durable safety state와 turn-local safety state 구분
- 절대 영속화되면 안 되는 값
- 구현 불변식, 결정표, 정상 시퀀스, 실패 시퀀스, 금지 패턴, Rust 체크포인트, 테스트 관점

이 문서는 다음을 정의하지 않는다.

- UI에서 승인 버튼을 어떻게 보여줄지
- OS별 sandbox 커널 기능의 세부 구현
- 외부 비밀관리 서비스 연동
- anti-virus, EDR, corporate policy agent 같은 외부 보안 제품 연동
- 멀티호스트 분산 실행의 trust boundary

---

## 핵심 정의

### host safety

host safety는 `shacs-bot`이 사용자의 로컬 머신 또는 self-hosted 환경에서 파일, 네트워크, 프로세스, secret에 접근할 때 어떤 범위까지 허용되고 어떤 조건에서 거절되는지를 고정하는 정책 계층이다.

### permission

permission은 특정 작업 후보를 지금 이 턴에서 effect로 내려도 되는지에 대한 오케스트레이터의 공식 승인 결과다. permission은 tool runtime의 재량이 아니라 `MainOrchestrator`의 결정이다.

### safety context

safety context는 특정 effect가 어떤 권한 경계 아래에서 발행되었는지를 설명하는 snapshot이다. 예:

- 현재 permission mode
- 허용된 workspace root 집합
- 허용된 capability 범위
- network 허용 대상 범주
- redaction 요구사항
- secret access scope

### secret

secret은 API key, access token, bearer token, local credential, session cookie, private key처럼 원문 노출이 로그, 세션 기록, provider 입력, artifact에 남으면 안 되는 민감 값이다.

### redaction

redaction은 secret 또는 민감 경로, 민감 출력이 공식 상태나 사용자 출력, provider 문맥, trace에 남지 않도록 원문을 제거하거나 대체 표기로 치환하는 규칙이다.

### inherited safety context

inherited safety context는 상위 턴 또는 상위 실행 경계에서 이미 확정한 안전 제약을 하위 effect, subagent, background reentry가 그대로 물려받는 규칙이다.

---

## host safety의 기본 원칙

이 시스템은 사용자의 로컬 환경에서 실제 작업을 수행할 수 있어야 한다. 그렇다고 해서 실행기가 host 전체에 무제한 권한을 가지면 안 된다. 초기 구현은 아래 원칙을 만족해야 한다.

1. host safety 판단은 `MainOrchestrator`가 한다.
2. safety snapshot은 effect 발행 전에 고정되어야 한다.
3. runtime은 snapshot을 좁게 집행할 수는 있어도 넓힐 수는 없다.
4. secret 원문은 세션 진실 원천, provider input snapshot, compaction input, 일반 로그에 남으면 안 된다.
5. approval은 위험 작업을 effect 생성 이전에 멈추는 정책 게이트다.
6. 한 턴의 승인 결과가 다음 턴의 무제한 권한으로 암묵 승격되면 안 된다.
7. self-hosted 환경이라도 host boundary는 workspace 중심으로 설명 가능해야 한다.

---

## capability와 host boundary

초기 구현은 host safety 관점에서 최소한 아래 capability 분류를 가져야 한다.

- `fs_read`: 파일 읽기, 디렉터리 열람, 메타데이터 조회
- `fs_write`: 파일 생성, 수정, 삭제, rename
- `proc_exec`: 프로세스 실행, shell command 실행, 하위 프로세스 생성
- `net_outbound`: 외부 네트워크 요청, remote API 호출
- `secret_read`: secret reference를 실제 값으로 해석

이 capability는 권한 자체가 아니라 정책 평가 대상이다.

### 1. filesystem boundary

filesystem 안전 경계는 최소한 다음을 구분해야 한다.

- 현재 workspace 루트
- 명시적으로 허용된 추가 경로
- runtime이 자체적으로 소유하는 내부 디렉터리, 예: `.shacs/runtime/`
- 금지 경로, 예: 홈 디렉터리 전체, SSH 키 디렉터리, 시스템 디렉터리, workspace 밖 임의 경로

기본 원칙은 다음과 같다.

1. path 해석은 canonical path 기준으로 해야 한다.
2. symbolic link를 따라간 최종 경로가 허용 범위 밖이면 거절해야 한다.
3. 상대 경로 허용 여부는 `working_directory`와 workspace root 기준으로 결정해야 한다.
4. 경로 패턴 허용은 설명 가능해야 하며, 단순 문자열 prefix 비교에만 의존하면 안 된다.

### 2. network boundary

network 안전 경계는 최소한 다음을 구분해야 한다.

- provider 호출처럼 시스템이 공식적으로 소유한 네트워크 effect
- tool이 요청하는 일반 outbound network
- localhost 또는 loopback 대상
- private network 또는 사용자 지정 allowlist 대상
- 완전 비허용 상태

기본 원칙은 다음과 같다.

1. `net_outbound`는 기본적으로 read/write와 별도 capability로 취급해야 한다.
2. provider runtime의 공식 호출과 임의 tool network 호출은 같은 범주로 뭉개면 안 된다.
3. 네트워크 허용 범위가 host safety snapshot에 명시되지 않았다면 tool runtime은 자체 추측으로 outbound를 허용하면 안 된다.
4. `net_outbound` capability가 있어도 요청 network scope가 비어 있거나 허용 scope에 포함되지 않으면 거절해야 한다.

### 3. process boundary

process 안전 경계는 최소한 다음을 구분해야 한다.

- 단순 명령 실행
- shell을 통한 복합 실행
- 하위 프로세스 spawning
- 장시간 살아 있는 background process
- interactive TUI 또는 persistent shell 핸들 생성

기본 원칙은 다음과 같다.

1. `proc_exec`는 항상 위험 capability로 본다.
2. 실행 가능 명령, 작업 디렉터리, timeout, environment scope는 snapshot에 묶여야 한다.
3. runtime은 승인된 command envelope를 다른 프로세스 계획으로 임의 확장하면 안 된다.
4. 종료 후에도 살아남는 프로세스는 별도 허용 규칙 없이는 만들면 안 된다.

### 4. secrets boundary

secret 안전 경계는 최소한 다음을 구분해야 한다.

- secret reference만 필요한 경우
- secret 원문이 외부 executor에 주입되어야 하는 경우
- provider/tool 호출 시 일부 필드만 secret을 참조하는 경우
- 어떤 경로에도 원문이 남으면 안 되는 경우

기본 원칙은 다음과 같다.

1. secret은 가능한 한 늦게 resolve해야 한다.
2. secret reference와 secret value는 다른 타입으로 취급해야 한다.
3. secret value는 state, event, trace, compaction input의 공식 원천이 되면 안 된다.
4. secret이 필요한 effect라도 결과 payload에는 원문이 다시 실리면 안 된다.
5. `secret_read` capability가 있어도 요청 secret scope가 비어 있거나 허용 scope에 포함되지 않으면 거절해야 한다.

---

## permission mode

초기 구현은 최소한 아래 세 mode를 가져야 한다.

- `plan`
- `default`
- `auto`

### `plan`

`plan`은 분석과 계획 중심 모드다.

- 허용 가능: 안전 범위 안의 `fs_read`, 제한적 검색, 비파괴 메타데이터 조회
- 기본 거절: `fs_write`, `proc_exec`, 일반 `net_outbound`, `secret_read`
- 특징: 승인으로도 장기 실행 권한으로 승격되지 않고, 턴 단위 거절이 기본이다.

### `default`

`default`는 안전한 읽기는 자동 허용하되, host를 바꾸거나 외부로 나가는 동작은 승인 기반으로 허용하는 모드다.

- 허용 가능: 안전 범위 안의 `fs_read`
- approval 필요: `fs_write`, `proc_exec`, `net_outbound`, `secret_read`
- 즉시 거절: 허용 범위 밖 경로, 미정의 secret scope, policy snapshot과 모순되는 요청

### `auto`

`auto`는 사용자가 넓은 자동 실행을 허용한 모드지만, 무제한 모드는 아니다.

- 허용 가능: snapshot 범위 안의 capability
- approval 생략 가능: `fs_write`, `proc_exec`, `net_outbound`, `secret_read`
- 즉시 거절: boundary 밖 접근, 금지 경로 접근, redaction 불가능한 결과를 공식 기록으로 남기게 되는 요청

### mode 해석 원칙

1. mode는 durable policy input이지만, 개별 effect 허용 여부는 현재 args와 boundary를 함께 봐야 한다.
2. `auto`라도 snapshot 밖 권한은 허용되지 않는다.
3. `plan`에서 위험 capability를 approval로 승격하는 우회 경로를 기본 동작으로 두면 안 된다.

---

## approval 규칙

approval은 위험 작업의 실행 계약을 확정하기 전 마지막 정책 게이트다.

### approval이 필요한 조건

기본 구현은 아래 중 하나면 approval 후보로 취급해야 한다.

- `fs_write`
- `proc_exec`
- 일반 `net_outbound`
- `secret_read`
- workspace 밖 경계에 닿을 수 있는 경로 접근
- redaction 실패 시 민감 데이터 노출 위험이 있는 작업

### approval이 아닌 즉시 거절 조건

아래는 approval로 넘기지 말고 거절해야 한다.

- 금지 경로 접근
- 허용되지 않은 secret scope 요청
- snapshot에 없는 capability 상승 시도
- 이미 닫힌 턴의 오래된 승인 응답
- redaction 불가능하거나 원문 secret을 공식 상태에 남기게 되는 구조

### approval 응답의 의미

승인은 다음을 뜻해야 한다.

- 특정 `turn_id`, `effect candidate`, `approval_request_id`에 대한 제한된 허용
- 현재 snapshot 범위 안에서만 유효한 허용
- 같은 종류의 다음 작업 전체를 자동 허용한다는 뜻이 아님

거절은 다음을 뜻해야 한다.

- 해당 후보 effect는 실행되지 않는다.
- 오케스트레이터는 abort 또는 더 좁은 계획으로 전환을 판단한다.

---

## redaction 규칙

redaction은 결과 표시 품질이 아니라 공식 진실 원천 보호를 위한 강제 규칙이다.

### redaction 대상

- secret 원문
- secret value가 포함된 URL, header, command arg
- 홈 디렉터리 절대경로처럼 불필요하게 민감한 경로
- provider/tool 결과 안의 credential 조각
- approval payload 안의 민감 본문

### redaction 적용 지점

최소한 아래 지점에서 redaction 검사가 있어야 한다.

- tool result 정규화 직전
- provider input snapshot 생성 직전
- event append 직전
- 사용자 표시용 응답 생성 직전
- observability trace 직전
- compaction input collector 진입 직전

### redaction 원칙

1. 원문을 먼저 저장한 뒤 나중에 가리는 구조를 기본으로 두면 안 된다.
2. redaction 실패 시 기본 동작은 raw persistence 금지가 되어야 한다.
3. redaction marker는 원문을 복구할 수 없는 형태여야 한다.
4. debug 목적이라도 secret raw dump를 state나 일반 로그에 남기면 안 된다.

### redaction 결과 표현

초기 구현은 아래 정도의 표현이면 충분하다.

- `[REDACTED_SECRET]`
- `[REDACTED_PATH]`
- `[REDACTED_TOKEN]`

중요한 것은 표시 문자열의 예쁨이 아니라, 공식 기록에서 원문이 사라졌다는 사실이다.

---

## inherited safety context

안전 제약은 하위 실행으로 내려갈수록 좁아질 수는 있어도 넓어지면 안 된다.

### 하위 effect에 반드시 내려가야 하는 정보

- `session_id`
- `turn_id`
- `effect_id`
- 현재 `permission_mode`
- 허용 capability 집합
- 허용 경로 범위
- network 허용 범위
- secret access scope
- redaction requirement
- timeout 및 resource limit

### subagent 또는 synthetic reentry에 승계되어야 하는 정보

- 부모 턴의 permission ceiling
- 부모 턴의 workspace boundary
- 부모 턴의 secret visibility 규칙
- 부모 턴의 approval 결과 중 현재 자식 실행에 필요한 제한적 사실

### 승계되면 안 되는 정보

- 이전 effect의 raw secret value
- 만료된 approval request id
- runtime executor 핸들
- 과거 턴의 임시 allowlist 확장 흔적

---

## durable safety state와 turn-local safety state

### durable safety state

다음은 `SessionState` 또는 replay 결과로 복원 가능한 durable safety state여야 한다.

- 현재 session permission mode
- 기본 workspace root와 명시적 허용 경로 집합
- 기본 network policy
- secret reference namespace 또는 사용 가능한 secret scope 규칙
- redaction 기본 정책
- late result와 민감 결과 관찰 이벤트 정책

### turn-local safety state

다음은 현재 턴에만 남아야 하는 turn-local safety state다.

- approval request id
- 특정 effect candidate에 대한 safety rationale
- 현재 턴에서만 확정된 temporary narrowed path scope
- redaction failure로 인해 폐기 대기 중인 임시 결과
- timeout과 연결된 현재 effect safety deadline

### 경계 판단 질문

> 이 safety 정보가 다음 턴의 기본 해석과 resume correctness에 직접 필요한가?

- 그렇다 → durable safety state
- 아니다, 현재 effect나 approval 흐름을 닫는 데만 필요하다 → turn-local safety state

---

## 절대 영속화되면 안 되는 것

아래 값은 어떤 이유로도 session truth, event log, compaction summary, provider input snapshot에 raw 형태로 남으면 안 된다.

- secret 원문 전체
- bearer token, API key, private key, session cookie
- secret이 포함된 command line raw string
- approval 전 원문 위험 payload 전체
- executor environment 전체 dump
- process handle, file descriptor, OS credential handle
- redaction 전 stdout/stderr raw buffer
- runtime이 메모리에서만 들고 있는 decrypted secret cache

이 규칙은 디버깅 편의보다 우선한다.

---

## 결정표

### 1. capability별 기본 결정표

| permission mode | capability | boundary 충족 | 결정 |
| --- | --- | --- | --- |
| `plan` | `fs_read` | 예 | 허용 가능 |
| `plan` | `fs_write` | 무관 | 즉시 거절 |
| `plan` | `proc_exec` | 무관 | 즉시 거절 |
| `plan` | `net_outbound` | 무관 | 즉시 거절 |
| `plan` | `secret_read` | 무관 | 즉시 거절 |
| `default` | `fs_read` | 예 | 허용 가능 |
| `default` | `fs_write` | 예 | approval 필요 |
| `default` | `proc_exec` | 예 | approval 필요 |
| `default` | `net_outbound` | 예 | approval 필요 |
| `default` | `secret_read` | 예 | approval 필요 |
| `auto` | 모든 capability | 예 | 허용 가능 |
| 모든 mode | 모든 capability | 아니오 | 즉시 거절 |

### 2. filesystem 결정표

| 요청 경로 | canonical path 결과 | 결정 |
| --- | --- | --- |
| workspace 내부 | 허용 범위 안 | 평가 계속 |
| symlink 경유 | 최종 경로가 허용 범위 안 | 평가 계속 |
| symlink 경유 | 최종 경로가 허용 범위 밖 | 즉시 거절 |
| 상대 경로 | 기준 작업 디렉터리에서 workspace 밖 | 즉시 거절 |
| 절대 경로 | 금지 경로 집합에 포함 | 즉시 거절 |

### 3. secret 처리 결정표

| 상황 | 결정 |
| --- | --- |
| secret reference만 필요하고 raw persistence 없음 | 허용 가능 |
| secret 원문이 provider input snapshot에 들어가려 함 | 즉시 거절 또는 redaction 후 대체 |
| result payload에 secret 원문이 포함됨 | redaction 필수, raw persistence 금지 |
| secret scope가 정의되지 않음 | 즉시 거절 |
| expired approval로 secret_read 재진입 | late approval로 폐기 |

---

## 정상 시퀀스 예시

### 예시 1. `default` 모드에서 workspace 내부 파일 수정 승인

```text
1) 모델이 workspace 내부 파일 수정 tool 후보를 제안한다.
2) MainOrchestrator는 capability=fs_write, mode=default를 확인한다.
3) path canonicalization 결과가 workspace 내부임을 확인한다.
4) 오케스트레이터는 approval 필요로 판정하고 approval request를 만든다.
5) 사용자가 승인 command를 보낸다.
6) 오케스트레이터는 approval_request_id와 turn correlation을 검증한다.
7) safety snapshot에 허용 경로와 redaction 규칙을 고정한 뒤 effect를 발행한다.
8) tool runtime은 snapshot 범위 안에서만 실행한다.
9) 결과는 redaction 검사 후에만 공식 상태로 편입된다.
```

### 예시 2. `auto` 모드에서 secret reference 사용

```text
1) provider profile이 secret reference를 필요로 한다.
2) MainOrchestrator는 현재 session의 secret scope를 확인한다.
3) effect 발행 직전 secret reference를 제한된 executor 입력으로 해석한다.
4) 호출은 성공하지만 usage trace와 event에는 secret reference 정보만 남기고 raw value는 남기지 않는다.
5) 이후 compaction input collector는 raw secret을 전혀 보지 못한다.
```

---

## 실패 시나리오

### 시나리오 1. workspace 밖 symlink 쓰기

- 잘못된 동작: 상대 경로는 workspace 내부처럼 보이지만 symlink가 홈 디렉터리 밖 민감 경로를 가리키고, runtime이 이를 그대로 수정
- 올바른 동작: canonical path 확인 후 즉시 거절

### 시나리오 2. tool stdout에 포함된 token이 event log에 저장

- 잘못된 동작: 실행기는 stdout raw를 그대로 normalized result에 넣고 세션 기록에 append
- 올바른 동작: redaction 검사 후 원문 제거, 필요 시 `[REDACTED_TOKEN]`으로 치환

### 시나리오 3. `auto` 모드를 무제한 허용으로 해석

- 잘못된 동작: mode가 auto라는 이유만으로 workspace 밖 경로와 임의 네트워크 요청을 모두 허용
- 올바른 동작: auto도 snapshot boundary 안에서만 허용

---

## 구현 불변식

아래 불변식은 future Rust 구현에서 타입, assertion, 테스트로 강제 대상이다.

1. host safety 최종 판단은 `MainOrchestrator`가 한다.
2. effect 발행 전 safety snapshot이 고정되어야 한다.
3. runtime은 snapshot보다 넓은 권한을 행사하면 안 된다.
4. secret raw value는 durable state, provider input snapshot, compaction input에 포함되면 안 된다.
5. approval은 특정 후보 effect에만 유효해야 하며, 범용 권한 승격으로 해석되면 안 된다.
6. canonical path 기준 boundary 검사가 존재해야 한다.
7. redaction 실패 시 raw persistence 금지가 기본 동작이어야 한다.
8. inherited safety context는 자식 실행으로 갈수록 넓어지면 안 된다.
9. `plan` 모드가 분석 전용이라는 의미가 런타임에서도 깨지면 안 된다.
10. late approval 또는 late result는 이미 닫힌 턴의 공식 상태를 바꾸면 안 된다.

---

## 금지 패턴

### 1. runtime이 위험도 재판단으로 허용을 뒤집기

금지 예:

- tool runtime이 "이번 write는 사소하니 승인 없이 진행"이라고 판단
- secret resolver가 scope 불일치를 무시하고 raw value 제공

왜 금지인가:

- 정책 권한이 분산된다.
- replay와 설명 가능성이 깨진다.

### 2. secret 원문을 로그나 snapshot에 남기기

금지 예:

- provider input debug dump에 API key 포함
- tool stderr 원문을 그대로 event log에 append

왜 금지인가:

- self-hosted 환경에서도 복구 기록이 곧 노출 경로가 된다.

### 3. approval을 세션 전역 권한 승격처럼 취급

금지 예:

- 한 번 승인한 write를 다음 턴의 모든 write에 자동 적용

왜 금지인가:

- 턴 단위 정책이 무너진다.
- 사용자가 무엇을 승인했는지 설명할 수 없어진다.

### 4. filesystem boundary를 문자열 prefix로만 판단

금지 예:

- `/repo2`가 `/repo` prefix를 가진다는 이유로 허용
- symlink 최종 경로를 확인하지 않음

왜 금지인가:

- 경로 우회가 가능해진다.

---

## Rust 구현으로 이어질 체크포인트

구체 타입 이름은 바뀔 수 있지만, 아래 질문에는 모두 "예"라고 답할 수 있어야 한다.

- `SafetyCapability`, `PermissionMode`, `SafetySnapshot`, `SecretRef`, `RedactedValue` 같은 타입 경계가 분리되는가?
- canonical path boundary 검사를 독립 함수로 테스트할 수 있는가?
- secret reference와 raw secret value를 타입 수준에서 구분할 수 있는가?
- result normalization 직전에 redaction pass가 존재하는가?
- approval request와 approval response의 correlation 검증 로직이 있는가?
- inherited safety context를 child effect나 subagent spawn envelope로 내려보낼 수 있는가?
- 어떤 필드가 durable safety state이고 어떤 필드가 turn-local인지 구조상 분리되는가?

---

## 테스트 관점에서 꼭 검증할 시나리오

future Rust 구현은 최소한 아래 시나리오를 테스트로 가져갈 수 있어야 한다.

- `plan` 모드에서 `fs_write`, `proc_exec`, `net_outbound`, `secret_read`가 즉시 거절되는가
- `default` 모드에서 workspace 내부 `fs_write`가 approval 없이는 effect로 내려가지 않는가
- symlink가 허용 범위를 벗어나면 거절되는가
- tool 결과에 secret 원문이 있어도 공식 기록에는 redacted 값만 남는가
- `auto` 모드에서도 허용 범위 밖 경로와 네트워크는 거절되는가
- secret scope가 없는 effect가 즉시 실패하는가
- expired approval response가 late approval로 폐기되는가
- provider input snapshot 생성 단계에서 raw secret이 제외되는가

---

## 명시적 비범위

이 문서는 다음을 정의하지 않는다.

- 관리자 승인 체인
- 중앙 secret vault 연동
- 조직 정책 엔진
- 멀티유저 role 기반 접근 제어
- 분산 실행 환경의 host trust negotiation

이 항목들은 필요가 생기면 별도 문서에서 다룬다. 단, 어떤 확장도 `MainOrchestrator` 단일 권한 원칙, secret raw persistence 금지, 턴 단위 approval 모델을 뒤집어서는 안 된다.

---

## 결론

`shacs-bot`의 host safety, permissions, and secrets 계층은 단순한 확인 팝업 규칙이 아니다. 이 계층은 로컬 host에 대한 실제 권한 행사와 민감 정보 노출을 턴 단위 정책으로 고정하는 핵심 계약이다.

핵심은 네 가지다.

- host boundary 판단과 permission 확정은 끝까지 `MainOrchestrator`에 남아 있어야 한다.
- filesystem, network, process, secrets는 별도 capability와 boundary로 취급되어야 한다.
- approval과 inherited safety context는 effect 발행 전 snapshot으로 굳어야 한다.
- secret 원문과 redaction 전 민감 출력은 공식 상태와 기록에 남으면 안 된다.

이 구조가 지켜져야 `shacs-bot`은 self-hosted 단일 사용자 런타임으로서 실제 작업을 수행하면서도, 무엇이 왜 허용되었고 무엇이 왜 거절되었는지 끝까지 설명할 수 있다.
