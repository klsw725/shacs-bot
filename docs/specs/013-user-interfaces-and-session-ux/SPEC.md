# user interfaces and session UX 아키텍처 명세

Status: Complete (Scoped)

Implemented scope: 현재 구현은 CLI session commands, `shacs-session` UX projection and query model, local API session query, WebSocket chat and streaming surface, command router, runtime loop command handling, and static web helper를 current interface scope로 지원한다.

Open work moved to: [033 evaluation automation live integration](../033-evaluation-automation-live-integration/SPEC.md), [035 ui projection, diagnostics, and release evidence parity](../035-ui-projection-diagnostics-and-release-evidence-parity/SPEC.md)

Not carried forward: 특정 TUI widget library, HTTP framework or WebSocket frame 선택, branding or theme design, remote multi-client editing UX, mobile app, browser SaaS console, external channel adapter internals는 이 closure에 포함하지 않는다.

## 문서 목적

이 문서는 `docs/SYSTEM-FOUNDATION.md`, `docs/specs/001-session-kernel/SPEC.md`, `docs/specs/006-session-store/SPEC.md`, `docs/specs/007-main-orchestrator-policy/SPEC.md`, `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`, `docs/specs/011-subagent-runtime/SPEC.md`, `docs/specs/012-runtime-services/SPEC.md`를 바탕으로 `shacs-bot`의 사용자 인터페이스 경계와 session UX를 구현 가능한 수준으로 고정한다.

목표는 다음과 같다.

- CLI, future TUI, local API가 무엇을 표시하고 무엇을 표시하지 말아야 하는지 정의한다.
- session create, resume, list, select, cancel, inspect, recover 흐름을 동일한 상태 의미론으로 묶는다.
- approval, progress, error surface가 어떤 공식 상태를 투영해야 하는지 고정한다.
- 인터페이스가 `MainOrchestrator`를 우회하지 않고도 충분히 작업 가능한 제품 경험을 제공하도록 경계를 명시한다.
- future Rust 구현에서 command adapter, projection model, interactive prompt 상태, API schema, 테스트 시나리오를 직접 도출할 수 있게 한다.

이 문서는 화면 아이디어를 적어 두는 제품 메모가 아니다. 구현이 이 문서와 충돌하면 임시 UX 편의나 transport 특성 때문에 오케스트레이터 권한 모델을 약화하지 말고, 인터페이스와 상태 투영 경계부터 다시 점검해야 한다.

이 spec의 최종 완료 기준은 CLI 명령 몇 개가 돌아가는 데모나 TUI mockup이 아니라, 이 문서가 정의한 interface boundary, session flow, approval/progress/error projection, recovery UX, transport 간 의미 일치를 충족하는 **완전한 기능 구현과 검증**이다. 현재 문서는 이 목표가 이미 모두 구현됐다고 판정하지 않고, 구현된 표면과 앞으로 정리할 표면을 함께 구분한다.

---

## 상위 기준과의 관계

이 문서는 다음 상위 기준을 전제로 한다.

- `MainOrchestrator`는 세션 상태와 턴 상태를 바꿀 수 있는 유일한 권한자다.
- session store는 공식 이력과 복구 기준점을 제공하지만 UX를 직접 결정하지 않는다.
- host safety와 approval은 인터페이스 장식이 아니라 공식 정책 결과다.
- 목표는 self-hosted / personal-use 환경에서 사용자가 직접 설치하고 실행하는 단일 사용자 assistant runtime이다.

따라서 이 문서는 관리자 콘솔, 조직 승인 체계, SaaS control plane, 멀티유저 협업 대시보드, 웹 우선 운영 포털을 다루지 않는다.

이 문서가 정의하는 인터페이스는 어디까지나 로컬 사용자의 작업 창구다. 예쁘게 보이는 것이 목적이 아니라, 오케스트레이터가 이미 알고 있는 공식 상태를 왜곡 없이 드러내고 조작 가능하게 만드는 것이 목적이다.

---

## 범위

이 문서는 다음을 정의한다.

- CLI, TUI, local API의 공식 역할과 비역할
- session 목록, 선택, 생성, 재개, 취소, 검사, 복구 흐름
- approval, progress, error, recovery surface의 의미
- 인터페이스가 소비할 projection과 command 변환 규칙
- transport 간 의미 일치 규칙
- 구현 불변식, 정상 시퀀스, 실패 시퀀스, 금지 패턴, Rust 체크포인트, 테스트 관점

이 문서는 다음을 정의하지 않는다.

- 특정 TUI 위젯 라이브러리 선택
- HTTP 프레임워크나 WebSocket 프레임 선택
- 브랜딩, 색상 테마, 고급 시각 디자인
- 원격 멀티클라이언트 동시 편집 UX
- 모바일 앱이나 브라우저 SaaS 콘솔
- Slack, Discord, Telegram, Email, WhatsApp bridge 같은 외부 채널 adapter 자체, 단 이 범위는 012 runtime services의 mailbox 경계에서 다룬다.

---

## 현재 구현 판정

현재 코드 기준으로 확인되는 구현은 CLI/session command UX, `shacs-session`의 session UX projection/query model, local API session query/WebSocket/chat completion/streaming surface, `shacs-web`의 정적 웹 UI helper와 session/protocol helper, command router, runtime loop command 처리다. 근거는 다음 경로를 기준으로 삼는다.

- `crates/shacs-session/src/lib.rs`
- `crates/shacs-cli/src/lib.rs`
- `crates/shacs-api/src/lib.rs`
- `crates/shacs-command/src/lib.rs`
- `crates/shacs-core/tests/runtime_loop.rs`
- `crates/shacs-core/tests/runtime_agent.rs`
- `crates/shacs-command/tests/router.rs`
- `crates/shacs-web/src/lib.rs`
- `crates/shacs-web/src/sessions.rs`
- `crates/shacs-web/src/protocol.rs`

현재 구현으로 `SessionUxSummary`, `SessionUxDetail`, `SessionUxDiagnostics`, `SessionUxHistory`, `SessionProjectionOptions`가 session store 옆의 공통 읽기 모델로 추가되었고, CLI list/inspect/history/diagnostics 및 local API의 session list/detail/history/diagnostics 조회가 이 의미론을 공유한다. 이 projection은 raw export와 분리되어 있어 기본 list/inspect/diagnostics 표면이 raw messages나 secret-like metadata 원문을 노출하지 않는다는 기존 안전 동작을 유지한다. 단, `SessionUxSummary`는 아직 lifecycle, 열린 턴 여부, recover 필요 여부를 모두 표현하는 최종 summary projection이 아니라 현재 세션 목록 조회에 필요한 최소 필드 중심이다.

아직 완성 구현으로 보지 않는 것은 terminal TUI, approval/progress/recovery 전체를 포괄하는 transport 공통 projection builder, 그리고 `SessionSummaryProjection`, `ApprovalProjection`, `ProgressProjection`, `RecoveryProjection` 같은 이름의 exact shared model이다. 이 이름들은 현재 아키텍처가 수렴해야 할 설계 어휘로 다루며, 코드에 같은 이름의 공통 구현이 있다는 주장으로 읽으면 안 된다.

CLI, local API, WebSocket, web helper가 이미 존재하더라도 이 spec의 transport parity는 현재 완료 판정이 아니라 유지해야 할 아키텍처 목표다. TUI는 별도 구현 근거가 확인되기 전까지 future surface로 분류한다.

---

## 핵심 정의

### interface surface

interface surface는 사용자가 `shacs-bot`과 상호작용하는 공식 진입점이다. 설계 범위는 아래 세 가지다.

- CLI
- TUI
- local API

### projection

projection은 `MainOrchestrator`와 session store가 가진 공식 상태를 인터페이스가 표시하기 좋은 읽기 모델로 변환한 표현이다. projection은 세션 진실 원천이 아니다. 이 문서의 projection 이름은 완성 시점의 공통 모델을 부르는 설계 어휘이며, 현재 모든 표면이 이미 같은 타입을 공유한다는 뜻은 아니다.

### session UX

session UX는 사용자가 세션의 현재 상태를 이해하고, 적절한 command를 보내고, 중단과 재개를 다루고, approval과 오류를 해석할 수 있게 만드는 전체 상호작용 계약이다.

### selection

selection은 사용자가 현재 집중할 세션 하나를 명시적으로 고르는 행위다. selection은 UI 상태일 수는 있어도 세션 truth 자체는 아니다.

### inspect surface

inspect surface는 진행 상태, 최근 event, pending approval, 열린 턴, recovery 필요 여부 같은 설명용 정보를 조회하는 읽기 전용 인터페이스다.

### app/process projection

app/process projection은 설치된 app, enabled 상태, 필요한 secret/permission, device 상태, 실행 중 app process, task receipt를 사용자가 이해할 수 있게 보여주는 읽기 모델이다. projection은 app registry나 session truth를 대체하지 않는다.

### recover flow

recover flow는 crash, interrupted upgrade, late result, 열린 턴 잔재 같은 상황에서 사용자가 세션을 안전한 상태로 되돌리고 다시 작업을 시작하게 만드는 UX 흐름이다.

---

## 인터페이스 계층의 기본 원칙

1. 인터페이스는 command를 만들고 projection을 보여줄 수 있어도 세션 상태를 직접 수정하면 안 된다.
2. 같은 세션 사실은 최종적으로 CLI, TUI, local API에서 같은 의미로 보여야 한다.
3. progress, approval, error는 추측으로 합성하면 안 되고 공식 상태나 event에 기반해야 한다.
4. selection과 포커스는 UI 로컬 상태일 수 있지만, cancel, resume, recover 같은 공식 동작은 반드시 command를 통해 오케스트레이터에 재진입해야 한다.
5. recovery UX는 손상된 실행을 감추는 것이 아니라, 무엇이 durable하고 무엇이 중단되었는지 드러내야 한다.
6. 인터페이스는 사용자를 대신해 "이미 끝난 것처럼" 보이게 만들면 안 된다.

---

## 인터페이스별 공식 역할

### 1. CLI

CLI는 가장 기본적인 제어면이다.

CLI가 해야 하는 일:

- 명시적 command 발행
- 단발성 조회와 조작, 예: list, create, resume, cancel, inspect
- interactive approval prompt 제공
- 스크립트 친화적 출력 모드와 사람 친화적 출력 모드 제공

CLI가 해서는 안 되는 일:

- 내부 state file을 직접 수정해 빠른 우회 제공
- 진행 중 effect를 로컬 캐시만 보고 성공 처리
- API 없이 세션 파일을 직접 편집해 recover 완료처럼 표시

### 2. TUI

TUI는 같은 공식 상태를 더 연속적인 작업 화면으로 보여주는 future interactive shell이다. 현재 문서는 terminal TUI가 이미 구현됐다고 보지 않는다.

TUI가 해야 하는 일:

- session 목록과 현재 포커스 세션 표시
- 열린 턴, progress, approval, error, inspect 정보를 지속적으로 projection
- create, switch, cancel, recover 같은 command 발행을 단축 동작으로 제공
- 로그 tail이 아니라 공식 progress/inspection projection을 중심으로 보여주기

TUI가 해서는 안 되는 일:

- transport latency를 숨기기 위해 가짜 완료 상태 표시
- 승인 버튼 클릭만으로 공식 승인 사실을 로컬 확정
- recovery 전 세션을 정상 active처럼 노출

### 3. local API

local API는 같은 런타임을 다른 로컬 도구가 호출할 수 있게 하는 machine-friendly surface다.

local API가 해야 하는 일:

- CLI와 future TUI가 따라야 할 command와 projection 의미 제공
- request/response 또는 stream 형태로 session projection 노출
- idempotent 조회와 상관관계 있는 mutation 요청 제공

local API가 해서는 안 되는 일:

- privileged 내부 command를 공개 mutation으로 노출
- approval 우회용 hidden parameter 제공
- session truth를 transport 응답 캐시로 대체

---

## 공유 읽기 모델과 query/command 경계

### projection 설계 범주

아래 이름은 공통 projection model로 수렴하기 위한 설계 범주다. 현재 exact shared model 구현이 확인된다는 뜻이 아니며, 구현이 성숙하면 모든 인터페이스는 최소한 아래 의미를 읽을 수 있어야 한다.

- `SessionSummaryProjection`
  - `session_id`
  - 제목 또는 요약 라벨
  - lifecycle state
  - 마지막 activity 시각
  - 열린 턴 유무
  - recover 필요 여부
- `SessionFocusProjection`
  - 현재 선택 세션의 대화 요약
  - 열린 턴 phase
  - pending effect 요약
  - active permission mode
  - 최근 assistant 결과와 사용자 입력 요약
- `ApprovalProjection`
  - approval request id
  - capability 종류
  - 대상 path 또는 command 요약
  - 승인 필요 이유
  - 허용 가능한 응답 집합
- `ProgressProjection`
  - 현재 phase
  - phase 진입 시각
  - 최근 progress event 목록
  - retry count 요약
  - 대기 중 외부 effect 요약
- `ErrorProjection`
  - 최근 오류 분류
  - 현재 turn abort reason 또는 recovery 필요 사유
  - 사용자에게 보여도 되는 진단 요약
- `RecoveryProjection`
  - 열린 턴 잔재 여부
  - 마지막 durable event sequence
  - late result 관찰 여부
  - recover 가능한 action 집합
- `AppListProjection`
  - app id, name, version, enabled state
  - install path와 manifest digest 요약
  - missing secret 또는 denied grant 요약
- `AppProcessProjection`
  - app process id와 originating intent
  - status, active grants, device status
  - tool/MCP call 요약과 artifact/receipt 참조

### query 경계

인터페이스는 아래 읽기 동작을 projection query로 수행할 수 있어야 한다.

- `ListSessions`
- `InspectSession`
- 현재 포커스 세션의 `SessionFocusProjection` 조회
- approval / progress / recovery projection 조회
- app list / app process / app permission projection 조회

이 query들은 세션 truth를 바꾸지 않는다. query는 command와 달리 상태 전이를 요청하지 않고, 공식 projection을 읽기 전용으로 가져오는 경계다.

### command 경계

인터페이스는 최소한 아래 command 집합을 발행할 수 있어야 한다.

- `CreateSession`
- `SubmitUserInput`
- `ResumeSession`
- `CancelTurn`
- `RecoverSession`
- `RespondToApproval`
- `InstallApp`, `EnableApp`, `DisableApp`, `OpenApp`, `UninstallApp`

`SelectSession`은 기본적으로 UI 로컬 포커스 상태다. selection 자체가 세션 truth를 바꾸지 않는 한, command가 아니라 인터페이스 내부 상태로 다뤄야 한다.

app 관련 command도 내부 registry 파일을 직접 수정하지 않고 `MainOrchestrator`가 소유한 공식 command 경계로 재진입해야 한다. app supervisor는 lifecycle/effect 실행을 보조할 수 있지만 command owner가 되면 안 된다.

인터페이스는 아래를 직접 확정하면 안 된다.

- assistant message finalized
- approval granted without validation
- recovery completed
- turn completed

---

## session 흐름 명세

### 1. create flow

create flow의 목적은 새 세션 identity를 만들고, 사용자가 즉시 입력을 시작할 수 있는 안정 상태를 제공하는 것이다.

규칙:

1. 새 세션 생성은 반드시 `CreateSession` command를 통해 시작해야 한다.
2. 인터페이스는 제목, workspace context, 기본 profile 같은 입력을 받을 수 있지만, 세션 공식 기본값은 오케스트레이터와 config snapshot이 결정한다.
3. 세션이 생성되면 projection에 새 `session_id`와 lifecycle `active`가 보여야 한다.
4. 생성 직후 열린 턴이 없는 상태가 기본이다.

### 2. resume flow

resume flow의 목적은 기존 세션을 현재 작업 대상으로 다시 여는 것이다.

규칙:

1. resume는 `session_id` 기준으로 시작한다.
2. session store replay와 recovery 판단이 먼저다.
3. recovery가 필요한 세션이면 인터페이스는 이를 active처럼 숨기면 안 되고 `RecoveryProjection`을 우선 보여야 한다.
4. 열린 턴이 없이 안정 복원되면 사용자는 즉시 새 입력을 보낼 수 있어야 한다.

### 3. list flow

list flow는 사용 가능한 세션 집합을 읽기 전용으로 탐색하는 흐름이다.

규칙:

1. 목록은 session summary projection만으로 구성해야 한다.
2. list 결과는 최근 activity, lifecycle, recover 필요 여부, 열린 턴 여부를 포함해야 한다.
3. list는 세션 내용을 암묵적으로 resume하지 않는다.

### 4. select flow

select flow는 여러 세션 중 하나를 현재 포커스로 고르는 흐름이다.

규칙:

1. selection은 UI 포커스 상태일 수 있다.
2. selection 자체가 session lifecycle을 바꾸면 안 된다.
3. select 이후 상세 projection 로드가 이어질 수 있지만, 이것이 곧 새 턴 수용을 뜻하지는 않는다.

### 5. cancel flow

cancel flow는 현재 열린 턴이나 대기 중 approval을 중단하는 흐름이다.

규칙:

1. cancel은 반드시 `CancelTurn` 또는 이에 준하는 command로 재진입해야 한다.
2. 인터페이스는 "취소 요청됨"과 "공식 취소 완료"를 구분해야 한다.
3. cancel 이후 턴이 `aborted`로 닫히기 전까지는 진행 중 상태가 유지될 수 있다.
4. 대기 중 외부 effect가 늦게 돌아오더라도 인터페이스는 stale result가 채택된 것처럼 보이면 안 된다.

### 6. inspect flow

inspect flow는 세션 진행 상황과 원인 정보를 깊게 보는 흐름이다.

규칙:

1. inspect는 읽기 전용이다.
2. inspect는 최근 event, 열린 턴 phase, pending effect, approval 대기, recovery 힌트, 오류 요약을 포함할 수 있어야 한다.
3. inspect는 raw secret, 미허용 경로, redaction 전 출력 원문을 노출하면 안 된다.

### 7. recover flow

recover flow는 interrupted 상태를 안정 상태로 정리하고 다시 작업을 시작하게 하는 흐름이다.

규칙:

1. recover는 `RecoverSession` command로 시작해야 한다.
2. recover 실행 전 인터페이스는 왜 recover가 필요한지, 예: 열린 턴 잔재, interrupted upgrade, replay mismatch, stale external result를 설명해야 한다.
3. recover 완료는 공식 recovery event 또는 recovery projection 갱신으로만 확인해야 한다.
4. recover는 중간 실행을 마술처럼 이어 붙이는 동작이 아니라, durable한 사실 기준으로 안정 상태를 다시 세우는 동작이어야 한다.

---

## approval surface

approval은 인터페이스 편의 기능이 아니라 정책 게이트의 시각화다.

### approval surface에 반드시 포함할 정보

- approval request id
- 현재 세션과 턴 식별자
- 요청 capability, 예: `fs_write`, `proc_exec`, `net_outbound`, `secret_read`
- 대상 요약, 예: path, command, host, secret scope
- 왜 approval이 필요한지에 대한 설명
- 허용 가능한 응답, 예: approve once, deny, cancel turn

### approval 응답 규칙

1. 인터페이스는 approval 응답을 로컬 상태로만 처리하면 안 된다.
2. 응답은 반드시 `RespondToApproval` command로 전달되어야 한다.
3. 이미 stale한 approval request면 거절 또는 무효 응답으로 보여야 한다.
4. 승인 이후에도 effect 완료 여부는 별도 progress로 추적해야 한다.

### 금지되는 approval UX

- 승인 클릭 즉시 "파일 수정 완료" 같은 결과를 표시
- request id 없이 가장 최근 요청에 암묵 응답
- TUI에서는 가능한데 API에서는 불가능한 별도 승인 semantics 제공

---

## progress surface

progress는 로그 줄 흘려보내기가 아니라, 현재 공식 실행 위치를 설명하는 surface다.

### progress에 포함해야 하는 최소 정보

- 현재 turn phase
- 최근 phase 변경 시각
- pending provider/tool/subagent/service effect 요약
- retry 또는 backoff 사실
- 사용자가 지금 할 수 있는 다음 action, 예: 기다리기, cancel, inspect

### progress 표시에 대한 규칙

1. progress는 공식 phase와 correlation 정보에 기반해야 한다.
2. 외부 executor가 임의로 내보낸 비공식 로그를 progress 완료 근거로 삼으면 안 된다.
3. 같은 세션의 progress 의미는 CLI, TUI, local API에서 일치해야 한다.
4. progress 부재는 곧 정상이 아닐 수 있으므로, heartbeat 없음이나 stalled 가능성도 표시 가능해야 한다.

---

## error surface

error surface는 사용자가 무엇이 실패했는지, 다시 시도 가능한지, recover가 필요한지 이해하게 만드는 표면이다.

### error 범주

- user-correctable error
  - 잘못된 session id
  - 잘못된 approval 응답
  - 허용되지 않은 명령 인자
- runtime failure
  - provider timeout
  - tool failure
  - session store replay 실패
  - projection 생성 실패
- recovery-required condition
  - interrupted turn
  - interrupted upgrade
  - event/checkpoint mismatch
  - stale process residue

### error 표시 규칙

1. 사용자 입력 오류와 내부 런타임 오류를 구분해 보여야 한다.
2. 내부 오류라도 노출 가능한 원인 요약은 제공해야 한다.
3. raw stack trace, secret, 민감 경로, redaction 전 payload는 기본 출력에 노출하면 안 된다.
4. recover 가능한 오류면 recover action을 함께 제시해야 한다.

---

## CLI, TUI, local API의 의미 일치 규칙

같은 런타임에서 transport마다 다른 제품이 되면 안 된다.

### 의미 일치 원칙

1. 같은 command는 transport마다 같은 상태 전이를 유발해야 한다.
2. 같은 세션 요약은 transport마다 같은 핵심 필드를 가져야 한다.
3. approval request의 응답 종류와 실패 semantics는 같아야 한다.
4. cancel, recover, inspect 결과는 표시 형식은 달라도 공식 의미는 같아야 한다.

### 허용되는 차이

- CLI는 한 번에 한 결과를 렌더링할 수 있다.
- TUI는 지속 갱신과 키보드 중심 조작을 가질 수 있다.
- local API는 구조화 JSON이나 stream envelope를 사용할 수 있다.

허용되지 않는 차이:

- CLI에서만 recovery 가능하고 API에서는 불가능한 구조
- TUI만 비공식 progress 채널을 읽는 구조
- local API만 privileged mutation을 받는 구조

---

## 결정표

### 1. session 선택 및 재개 결정표

| 조건 | 인터페이스 결정 | 비고 |
| --- | --- | --- |
| 세션 존재, replay 성공, recovery 불필요 | 상세 projection 표시 | 새 입력 가능 |
| 세션 존재, replay 성공, recovery 필요 | recovery surface 우선 표시 | active처럼 위장 금지 |
| 세션 없음 | user-correctable error | 생성 또는 다른 세션 선택 |
| session id malformed | user-correctable error | 런타임 오류로 감추지 않음 |

### 2. approval 응답 처리 결정표

| 조건 | 결정 | 비고 |
| --- | --- | --- |
| request id 유효, pending 상태 | command 전송 | 결과는 별도 progress로 확인 |
| request id 유효, 이미 stale | 무효 응답 표시 | 승인 완료처럼 보이면 안 됨 |
| request id 없음 | 거절 | 암묵 응답 금지 |
| transport 권한 부족 | 거절 | local-only 범위 유지 |

### 3. recover surface 표시 결정표

| 조건 | 기본 표시 | 사용자 action |
| --- | --- | --- |
| 열린 턴 잔재 감지 | interrupted session | inspect 또는 recover |
| replay mismatch | recovery blocked | 상세 진단 확인 |
| interrupted upgrade marker 존재 | upgrade recovery required | recover 또는 rollback 지침 |
| stale late result만 존재 | recover optional | inspect 후 진행 가능 |

---

## 정상 시퀀스 예시

### 예시 1. CLI에서 새 세션 생성 후 첫 입력 제출

1. 사용자가 `create` 성격의 CLI 명령을 호출한다.
2. CLI는 `CreateSession` command를 보낸다.
3. 오케스트레이터가 새 세션 identity를 만들고 session summary projection이 생성된다.
4. CLI는 새 `session_id`와 기본 상태를 표시한다.
5. 사용자가 첫 입력을 제출한다.
6. CLI는 `SubmitUserInput` command를 보낸다.
7. 이후 진행 상태는 progress projection으로 표시된다.

### 예시 2. TUI에서 목록 조회 후 recover 필요한 세션 선택

1. TUI가 session summary 목록을 읽는다.
2. 특정 세션에 `recover_required=true`가 표시된다.
3. 사용자가 해당 세션을 선택한다.
4. TUI는 대화 입력창 대신 recovery surface를 우선 표시한다.
5. 사용자가 inspect를 열어 interrupted turn과 마지막 durable sequence를 확인한다.
6. 사용자가 recover action을 실행한다.
7. recovery projection이 정상 상태로 갱신되면 그때 입력창이 다시 활성화된다.

---

## 실패 시나리오

### 시나리오 1. 인터페이스가 approval을 로컬 성공으로 처리

- 잘못된 동작: 버튼 클릭 즉시 "승인 및 실행 완료" 표시
- 올바른 동작: `RespondToApproval` command 전송 후, progress surface에서 effect 결과를 별도로 추적

### 시나리오 2. resume가 recovery 필요 세션을 바로 active 채팅창으로 연다

- 잘못된 동작: interrupted turn이 있는 세션을 일반 active 세션처럼 렌더링
- 올바른 동작: recovery requirement를 먼저 표시하고, recover 또는 inspect를 거치게 함

### 시나리오 3. local API가 privileged mutation을 노출한다

- 잘못된 동작: 내부 전용 finalize endpoint 공개
- 올바른 동작: 공식 command 집합만 노출하고, 최종 확정은 오케스트레이터 내부 정책에 맡김

---

## 구현 불변식

1. 인터페이스는 세션 상태를 직접 수정할 수 없다.
2. create, resume, cancel, recover, approval 응답은 모두 공식 command 재진입을 거쳐야 한다.
3. progress는 공식 phase와 correlation에 기반해야 한다.
4. recovery 필요 세션은 정상 active 상태처럼 위장되면 안 된다.
5. selection UI 상태는 세션 truth와 분리되어야 한다.
6. inspect surface는 읽기 전용이어야 한다.
7. raw secret과 redaction 전 민감 출력은 어떤 인터페이스 기본 출력에도 노출되면 안 된다.
8. CLI, future TUI, local API는 같은 command와 projection 의미를 공유해야 한다.
9. approval request는 request id 없이 응답되면 안 된다.
10. cancel requested와 cancel completed는 구분되어 표시되어야 한다.

---

## 금지 패턴

### 1. 인터페이스가 session file을 직접 수정

왜 금지인가:

- `MainOrchestrator` 단일 권한 원칙이 깨진다.

### 2. transport별 다른 상태 의미

왜 금지인가:

- 사용자가 같은 런타임을 서로 다른 제품처럼 겪게 된다.

### 3. progress를 비공식 로그 tail로만 구성

왜 금지인가:

- 현재 공식 phase와 pending effect를 설명할 수 없게 된다.

### 4. recovery 필요 상태를 숨김

왜 금지인가:

- interrupted execution이 정상 완료처럼 오해된다.

### 5. approval과 completion을 한 화면에서 혼동

왜 금지인가:

- 정책 승인과 실제 effect 결과가 섞여 잘못된 성공 인상을 만든다.

---

## Rust 구현으로 이어질 체크포인트

구체 타입 이름은 바뀔 수 있지만, 최종 구현은 아래 질문에 모두 "예"라고 답할 수 있어야 한다. 현재 코드에 같은 이름의 exact shared model이 있다는 뜻은 아니다. 현재 구현은 session list/detail/history/diagnostics용 `SessionUx*` 읽기 모델과 local API query route를 제공하지만, TUI와 approval/progress 전체 projection은 남아 있다.

- `SessionSummaryProjection`, `SessionFocusProjection`, `ApprovalProjection`, `RecoveryProjection` 같은 읽기 모델이 분리되는가?
- CLI, future TUI, local API가 공통 command schema 또는 공통 내부 command 타입에 매핑되는가?
- selection 같은 UI 로컬 상태와 session truth가 타입 경계로 구분되는가?
- recovery-required 세션을 projection 수준에서 식별할 수 있는가?
- approval request id와 response command correlation을 강제할 수 있는가?
- progress rendering이 raw executor log 없이도 공식 상태만으로 가능하도록 모델링되는가?

---

## 테스트 관점에서 꼭 검증할 시나리오

Rust 구현은 최소한 다음 성격의 테스트를 만들 수 있어야 한다.

- 새 세션 생성 후 summary projection이 안정적으로 반환되는지 확인하는 테스트. Staged 구현에서는 `shacs-session` projection 테스트와 local API session query 테스트가 이 일부를 검증한다.
- recovery-required 세션이 resume 시 일반 active session projection으로 보이지 않는지 확인하는 테스트
- approval request id 없는 응답이 거절되는지 확인하는 테스트
- cancel requested 이후 cancel completed 전까지 상태가 구분되는지 확인하는 테스트
- CLI, future TUI, local API가 같은 session summary 의미를 공유하는지 확인하는 contract test. Staged 구현에서는 CLI와 local API가 `shacs-session`의 session UX projection 의미를 공유하며, future TUI parity는 남아 있다.
- inspect surface가 secret 원문이나 redaction 전 payload를 노출하지 않는지 확인하는 테스트
- stale approval 또는 stale recover action이 공식 상태를 바꾸지 않는지 확인하는 테스트

---

## 명시적 비범위

이 문서는 다음을 정의하지 않는다.

- 시각 디자인 시스템
- 웹 기반 원격 운영 콘솔
- 멀티유저 presence 표시
- 협업 댓글, 공유 세션 링크
- 고급 텔레메트리 대시보드 UI

이 항목들은 별도 문서로 다룰 수 있다. 단, 어떤 확장도 이 문서가 고정한 인터페이스 경계와 `MainOrchestrator` 단일 권한 모델을 약화하면 안 된다.

---

## 결론

`shacs-bot`의 사용자 인터페이스는 현재 구현된 CLI, local API, WebSocket, web helper 표면에서 시작해, future TUI까지 같은 공식 상태를 다른 형태로 보여주는 구조로 수렴해야 한다. create, resume, list, select, cancel, inspect, recover는 모두 command와 projection의 명시적 계약으로 설명되어야 하며, approval, progress, error surface는 오케스트레이터가 확정한 사실만 드러내야 한다.

핵심은 인터페이스가 똑똑한 척 상태를 대신 확정하는 것이 아니라, 사용자가 현재 세션이 어디까지 진행되었고 무엇을 할 수 있으며 왜 멈췄는지를 정확히 이해하게 만드는 데 있다.
