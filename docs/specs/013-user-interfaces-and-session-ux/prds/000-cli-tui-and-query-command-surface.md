# PRD 000. cli, tui, and query-command surface

## 목표

이 문서는 `docs/specs/013-user-interfaces-and-session-ux/SPEC.md`의 하위 실행 문서다. 현재 구현된 CLI/session command UX와 local API/WebSocket/web helper 표면을 기준으로 삼고, future TUI까지 같은 상태 의미론 위에 올리기 위한 실행 계획을 정리한다.

이번 PRD의 목표는 화면 종류와 transport가 달라도 사용자가 같은 세션 사실을 같은 의미로 볼 수 있게 만드는 것이다. 인터페이스는 상태를 꾸미는 층이 아니라 command를 재진입시키고 projection을 읽는 층이어야 한다.

## SPEC 입력

- 주관 spec: `docs/specs/013-user-interfaces-and-session-ux/SPEC.md`
- 선행 기준:
  - `docs/specs/001-session-kernel/SPEC.md`
  - `docs/specs/006-session-store/SPEC.md`
  - `docs/specs/007-main-orchestrator-policy/SPEC.md`
  - `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`
  - `docs/specs/011-subagent-runtime/SPEC.md`
  - `docs/specs/012-runtime-services/SPEC.md`
- 교차 의존:
  - `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`
  - `docs/specs/015-packaging-process-lifecycle-and-upgrades/SPEC.md`
  - `docs/specs/016-verification-matrix-and-release-gates/SPEC.md`

## Dependency Cut

- 001과 006은 session lifecycle, open turn, recovery 필요 여부를 projection의 진실 원천으로 제공한다.
- 007은 cancel, approval response, recover, resume 같은 command 의미를 고정한다.
- 010은 approval surface와 inspect redaction 규칙을 제공한다.
- 011과 012는 progress projection에 child task, background service 상태를 노출할 근거를 제공한다.
- 014는 inspect와 diagnostics bundle surface의 읽기 모델을 제공한다.
- 015는 install/start/recover 이후 인터페이스가 보여야 하는 lifecycle 상태를 제공한다.

## 범위

- CLI command surface와 human/script output 모드 정리
- terminal TUI focus, session list, progress, approval, recover surface 설계
- local API, WebSocket, chat completion/streaming surface 정리
- 공통 projection builder와 adapter 계층 설계 및 future 구현
- create, list, select, resume, cancel, inspect, recover 흐름의 command/query 경계 정리
- transport 간 의미 일치 자동 테스트 추가

## 범위 제외

- 시각 디자인 고도화
- 모바일 앱
- 멀티클라이언트 협업
- SaaS 운영 포털
- Slack, Discord, Telegram, Email, WhatsApp bridge channel adapter 구현, 단 이 범위는 012 runtime services PRD에서 다룬다.

## 현재 구현 상태

### 이미 반영된 것

- `crates/shacs-session/src/lib.rs`에 session list/detail/diagnostics/history용 `SessionUx*` 공통 읽기 모델이 있다.
- `crates/shacs-cli/src/lib.rs`에 CLI/session command UX가 있다.
- `crates/shacs-api/src/lib.rs`에 local API session query route, WebSocket, chat completion, streaming surface가 있다.
- `crates/shacs-command/src/lib.rs`와 `crates/shacs-command/tests/router.rs`에 command router와 routing 테스트가 있다.
- `crates/shacs-core/tests/runtime_loop.rs`와 `crates/shacs-core/tests/runtime_agent.rs`에 runtime loop command 처리와 agent runtime 흐름 테스트가 있다.
- `crates/shacs-web/src/lib.rs`, `crates/shacs-web/src/sessions.rs`, `crates/shacs-web/src/protocol.rs`에 static web UI helper, session helper, protocol helper가 있다.
- CLI session list/inspect/history/diagnostics와 local API session list/detail/history/diagnostics는 raw export와 분리된 session UX projection 의미를 공유한다.
- approval, progress, error, recovery 의미는 여러 표면에서 다뤄지고 있지만, 하나의 공통 projection model로 완전히 수렴했다고 보지는 않는다.

### 부분 구현 또는 future work

- terminal TUI는 구현 완료로 보지 않는다. render-mode나 문서 언급만으로 TUI parity를 주장하지 않는다.
- `SessionSummaryProjection`, `SessionFocusProjection`, `ApprovalProjection`, `ProgressProjection`, `RecoveryProjection`은 공통 모델로 수렴하기 위한 설계 어휘다. current exact shared model 구현으로 주장하지 않는다.
- CLI, local API, WebSocket, web helper 간 command/query 의미를 계속 맞춰야 한다. full CLI/TUI/local API projection parity는 future consolidation이다.
- process lifecycle blocker, recovery required, stale approval 같은 UX contract는 현재 근거를 바탕으로 더 명확한 contract test가 필요하다.

### 명시적 비범위

- 시각 디자인 고도화, 모바일 앱, 멀티클라이언트 협업, SaaS 운영 포털은 비범위다.
- 관리자 콘솔, 조직 승인 체계, 원격 control plane, multi-user dashboard는 이 PRD의 제품 전제가 아니다.
- 외부 채널 자체의 network bridge 운영은 012 runtime services의 adapter/normalizer 경계 밖에서는 제공하지 않는다.

### 로컬 근거

- `crates/shacs-cli/src/lib.rs`
- `crates/shacs-api/src/lib.rs`
- `crates/shacs-session/src/lib.rs`
- `crates/shacs-session/tests/session_manager.rs`
- `crates/shacs-command/src/lib.rs`
- `crates/shacs-core/tests/runtime_loop.rs`
- `crates/shacs-core/tests/runtime_agent.rs`
- `crates/shacs-command/tests/router.rs`
- `crates/shacs-web/src/lib.rs`
- `crates/shacs-web/src/sessions.rs`
- `crates/shacs-web/src/protocol.rs`

## TDD 계획

1. session list/detail/diagnostics/history의 `SessionUx*` 읽기 모델이 raw export와 분리되는지 단위 테스트로 유지한다.
2. CLI, future TUI, API가 같은 projection과 command 결과를 공유하는 contract 테스트를 확장한다.
3. create, resume, cancel, recover 흐름 통합 테스트를 추가한다.
4. approval stale 응답, inspect redaction, late result 표시 테스트를 추가한다.
5. interrupted upgrade 또는 recovery required 상태가 active처럼 보이지 않는 UX contract 테스트를 추가한다.

## 구현 웨이브

### Wave 1. 공통 projection과 command adapter 고정

- 인터페이스 공통 읽기 모델을 정의하고 projection builder를 한 군데로 모은다.
- query와 command 경계를 코드에서 분리한다.
- selection은 UI 로컬 상태로 남기고 세션 truth와 분리한다.

### Wave 2. CLI surface 구현

- create, list, inspect, resume, cancel, recover, approval response 명령을 구현한다.
- 사람 친화적 출력과 기계 친화적 출력 모드를 같은 projection에서 파생시킨다.
- inspect와 recover 흐름에서 recovery required 이유를 숨기지 않고 보여준다.

### Wave 3. TUI와 local API 정렬

- TUI가 공식 projection 중심으로 session list, focus, progress, approval, recovery를 표시하게 만든다.
- local API에 같은 query/command 의미를 노출한다.
- transport별 캐시나 낙관적 완료 표시로 공식 상태를 왜곡하지 않게 막는다.

### Wave 4. 교차 surface 회귀 검증

- 같은 세션 사실이 CLI, TUI, API에서 같은 의미로 보이는지 contract 테스트를 묶는다.
- stale approval, cancel requested vs completed, recover completed, late result ignored 상황을 검증한다.
- install 후 첫 create, interrupted 상태 inspect, recover 후 resume 경로를 end-to-end로 확인한다.

## Verification Targets

- 단위 테스트 목표: projection builders, progress phase entered timestamp, progress pending effect correlation details, recovery late result observation evidence, active permission mode source, redacted inspect summaries, command parsing
- 통합 테스트 목표: create/resume/cancel/recover/approval flow, local API resume recovery projection, CLI/API recovery projection parity
- UX contract 테스트 목표: CLI, future TUI, API meaning parity, stable summary projection fields, pending vs completed semantics, recovery-required visibility, resume recovery process blockers, late result rejected metadata visibility
- 안전성 테스트 목표: inspect redaction, API/CLI error projection redaction, future TUI redacted error/diagnostics rendering, transport error envelope redaction, approval correlation, stale approval handling
- 문서 증거 목표: query 목록, command 목록, projection 필드 표

## Current Evidence And Gaps

- 현재 근거: `crates/shacs-cli/src/lib.rs`, `crates/shacs-api/src/lib.rs`, `crates/shacs-command/src/lib.rs`, `crates/shacs-core/tests/runtime_loop.rs`, `crates/shacs-core/tests/runtime_agent.rs`, `crates/shacs-command/tests/router.rs`, `crates/shacs-web/src/lib.rs`, `crates/shacs-web/src/sessions.rs`, `crates/shacs-web/src/protocol.rs`.
- 검증 공백: terminal TUI 구현 근거와 TUI parity 테스트는 아직 완료 근거로 쓰지 않는다.
- 검증 공백: Spec016 `FullSpec Verified` 주장은 이 문서에서 하지 않는다. 현재 문서가 확인한 것은 관련 로컬 근거와 future verification target이다.

## Open Risks

- 인터페이스별 출력 포맷 차이가 같은 상태를 다르게 해석하게 만들 수 있다.
- progress surface가 로그 tail처럼 변질되면 공식 phase 의미가 흐려질 수 있다.
- local API가 숨은 privileged mutation을 노출하면 CLI/future TUI와 의미 불일치가 생길 수 있다.
- 참고 메모: inspect surface는 014의 공통 진단 모델을 소비하는 구조를 전제로 하므로, 인터페이스별 projection이 inspect 의미를 독자적으로 재정의하지 않도록 주의가 필요하다.

## 종료 기준

- CLI, future TUI, local API가 같은 projection과 command 의미를 공유한다.
- create, list, select, resume, cancel, inspect, recover 흐름이 모두 공식 command/query 경계 위에서 동작한다.
- approval, progress, error, recovery surface가 추측이 아니라 공식 상태를 반영한다.
- recovery required 세션은 active처럼 숨겨지지 않는다.
- 013과 016이 요구하는 UX contract, 통합, 안전성 검증 증거가 준비된다.
