# PRD 000. cli, tui, and query-command surface

## 목표

이 문서는 `docs/specs/013-user-interfaces-and-session-ux/SPEC.md`의 하위 실행 문서다. CLI, TUI, local API를 같은 상태 의미론 위에 올리고, session flows, approval, progress, inspect, recover를 실제 제품 표면으로 구현하기 위한 실행 계획을 정리한다.

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

- CLI command surface와 human/script output 모드 설계
- TUI focus, session list, progress, approval, recover surface 설계
- local API query/command schema 설계
- 공통 projection builder와 adapter 계층 구현
- create, list, select, resume, cancel, inspect, recover 흐름 구현
- transport 간 의미 일치 자동 테스트 추가

## 범위 제외

- 시각 디자인 고도화
- 모바일 앱
- 멀티클라이언트 협업
- SaaS 운영 포털
- Slack, Discord, Telegram, Email, WhatsApp bridge channel adapter 구현, 단 이 범위는 012 runtime services PRD에서 다룬다.

## 현재 구현 상태

### 이미 반영된 것

- CLI, TUI, local API가 session create/list/inspect/resume/submit/wait/recover/cancel/approval command와 projection 의미를 공유한다.
- approval, progress, error, diagnostics, recovery query surface가 공통 projection과 diagnostics bundle을 소비한다.
- process lifecycle blocker가 있을 때 mutating action이 차단되고, TUI는 inspect/recovery 중심 상태로 유지된다.
- local API는 CLI와 같은 JSON envelope와 conflict/not_found/usage/runtime 의미를 노출한다.
- CLI와 API는 같은 recovery fixture에서 같은 recovery projection 의미를 노출한다.
- CLI/API/TUI의 user-facing error/diagnostics surface는 secret-like text를 `[REDACTED_SECRET]`로 유지한다.
- Spec016 matrix evidence: FullSpec Verified for `InterfaceContract`, `Integration`, `SafetyRedaction`.

### 아직 남은 것

- 시각 디자인 고도화, 모바일 앱, 멀티클라이언트 협업, SaaS 운영 포털은 비범위다.
- 외부 채널 자체의 network bridge 운영은 012 runtime services의 adapter/normalizer 경계 밖에서는 아직 제공하지 않는다.

### 로컬 근거

- `crates/shacs-cli/src/lib.rs`
- `crates/shacs-cli/src/lib.rs` inline tests for parser/session/runtime/API/WebSocket surface
- `crates/shacs-core/tests/runtime_loop.rs`

## TDD 계획

1. `SessionSummaryProjection`, `SessionFocusProjection`, `ApprovalProjection`, `ProgressProjection`, `RecoveryProjection` 단위 테스트를 만든다.
2. CLI, TUI, API가 같은 projection과 command 결과를 공유하는 contract 테스트를 추가한다.
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

## Verification Evidence

- 단위 테스트: projection builders, progress phase entered timestamp, progress pending effect correlation details, recovery late result observation evidence, active permission mode source, redacted inspect summaries, command parsing
- 통합 테스트: create/resume/cancel/recover/approval flow, local API resume recovery projection, CLI/API recovery projection parity
- UX contract 테스트: CLI, TUI, API meaning parity, stable summary projection fields, pending vs completed semantics, recovery-required visibility, resume recovery process blockers, late result rejected metadata visibility
- 안전성 테스트: inspect redaction, API/CLI error projection redaction, TUI redacted error/diagnostics rendering, transport error envelope redaction, approval correlation, stale approval handling
- 문서 증거: query 목록, command 목록, projection 필드 표

## FullSpec Evidence

- `InterfaceContract`: `crates/shacs-cli/src/lib.rs` inline parser/session/runtime/API/WebSocket tests
- `Integration`: `crates/shacs-core/tests/runtime_loop.rs`, `crates/shacs-cli/src/lib.rs` inline API/WebSocket bridge tests
- `SafetyRedaction`: `crates/shacs-cli/src/lib.rs` inline session inspect/export/diagnostics and media/error tests

## Open Risks

- 인터페이스별 출력 포맷 차이가 같은 상태를 다르게 해석하게 만들 수 있다.
- progress surface가 로그 tail처럼 변질되면 공식 phase 의미가 흐려질 수 있다.
- local API가 숨은 privileged mutation을 노출하면 CLI/TUI와 의미 불일치가 생길 수 있다.
- 참고 메모: inspect surface는 014의 공통 진단 모델을 소비하는 구조를 전제로 하므로, 인터페이스별 projection이 inspect 의미를 독자적으로 재정의하지 않도록 주의가 필요하다.

## 종료 기준

- CLI, TUI, local API가 같은 projection과 command 의미를 공유한다.
- create, list, select, resume, cancel, inspect, recover 흐름이 모두 공식 command/query 경계 위에서 동작한다.
- approval, progress, error, recovery surface가 추측이 아니라 공식 상태를 반영한다.
- recovery required 세션은 active처럼 숨겨지지 않는다.
- 013과 016이 요구하는 UX contract, 통합, 안전성 검증 증거가 준비된다.
