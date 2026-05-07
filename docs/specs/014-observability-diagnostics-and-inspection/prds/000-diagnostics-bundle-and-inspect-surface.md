# PRD 000. diagnostics bundle and inspect surface

## 목표

이 문서는 `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`의 하위 실행 문서다. event, operational log, trace, inspect, diagnostics bundle의 역할을 실제 구현 단계로 나누고, redaction과 recovery evidence를 제품 기본선으로 고정한다.

이번 PRD의 목표는 self-hosted 사용자가 별도 운영 콘솔 없이도 로컬에서 현재 상태와 최근 실패 원인을 설명할 수 있게 만드는 것이다. 많이 남기는 것보다, 안전하게 남기고 다시 읽을 수 있게 만드는 것이 우선이다.

## SPEC 입력

- 주관 spec: `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`
- 선행 기준:
  - `docs/specs/001-session-kernel/SPEC.md`
  - `docs/specs/006-session-store/SPEC.md`
  - `docs/specs/007-main-orchestrator-policy/SPEC.md`
  - `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`
  - `docs/specs/012-runtime-services/SPEC.md`
  - `docs/specs/013-user-interfaces-and-session-ux/SPEC.md`
- 교차 의존:
  - `docs/specs/008-configuration-profiles-and-runtime-layout/SPEC.md`
  - `docs/specs/015-packaging-process-lifecycle-and-upgrades/SPEC.md`
  - `docs/specs/016-verification-matrix-and-release-gates/SPEC.md`

## Dependency Cut

- 006의 event log와 replay가 공식 truth를 제공하므로 diagnostics는 이를 대체하지 않는다.
- 010의 secret handling과 redaction 규칙이 inspect, trace, bundle에도 동일하게 적용되어야 한다.
- 012는 service correlation과 stale wake 관측 근거를 제공한다.
- 013은 inspect surface를 CLI, TUI, local API 공통 읽기 모델로 소비한다.
- 015는 crash, stale ownership, interrupted upgrade, recover 흐름의 증거 요구사항을 제공한다.

## 범위

- operational log, trace, diagnostics record 데이터 모델 구현
- inspect surface 읽기 모델과 bundle 생성기 구현
- crash evidence, recovery evidence 수집 규칙 구현
- diagnostics artifact 저장 경로와 redaction gate 구현
- inspect 요약과 deep diagnostics 흐름 구현
- redaction failure, bundle generation failure, late result observability 테스트 추가

## 범위 제외

- 원격 observability SaaS
- 조직용 대시보드
- 시계열 metrics 플랫폼 구축
- 보안 감사 조직 워크플로우

## 현재 구현 상태

### 이미 반영된 것

- inspect snapshot, diagnostics record, diagnostics bundle, redaction status, recovery evidence가 core observability 계층에 구현돼 있다.
- operational log record, trace record, crash evidence가 event truth를 대체하지 않는 보조 증거 모델로 분리돼 있다.
- rejected reentry, aborted turn, service correlation, subagent child task, process lifecycle blocker가 recent events/diagnostics/recovery projection에 반영된다.
- provider/tool failure는 correlation, severity, next action, redaction status를 가진 diagnostics record로 요약할 수 있다.
- interrupted upgrade와 partial migration 상태는 diagnostics bundle과 recovery inspect에서 user-correctable evidence로 노출된다.
- unsafe artifact refs, bundle generation failure, secret-like diagnostics/session summary/log/trace payload는 redaction 또는 rejection evidence로 처리된다.
- CLI/API diagnostics inspect는 같은 diagnostics bundle 의미를 노출하고 read-only query로 event log를 변경하지 않는다.
- Spec016 matrix evidence: FullSpec Verified for `Unit`, `Integration`, `DurabilityRecovery`.

### 아직 남은 것

- remote observability SaaS, 조직용 dashboard, 시계열 metrics 플랫폼은 비범위다.
- triage 근거를 충분히 남기면서도 redaction 누락을 막는 균형은 계속 open risk다.

### 로컬 근거

- `crates/shacs-utils/src/progress_events.rs`
- `crates/shacs-core/src/runtime/runner.rs`
- `crates/shacs-core/src/runtime/agent_loop.rs`
- `crates/shacs-cli/src/lib.rs` inline observability/session diagnostics/runtime inspect tests

## TDD 계획

1. diagnostics record 분류, severity, correlation 필드 단위 테스트를 만든다.
2. secret, path, env 값 redaction 테스트를 먼저 추가한다.
3. inspect projection이 열린 턴, pending effect, abort reason, recovery required를 올바르게 표시하는 통합 테스트를 추가한다.
4. crash evidence와 recovery evidence 생성 테스트를 추가한다.
5. bundle 생성 실패도 진단 기록으로 남는 테스트를 추가한다.

## 구현 웨이브

### Wave 1. 기본 관측 모델 구축

- event와 별개인 operational log, trace, diagnostics record 스키마를 만든다.
- 최소 correlation 필드 집합을 모든 기록 경로에 연결한다.
- 공식 event를 기준으로 inspect용 요약 모델을 빌드한다.

### Wave 2. Redaction-first 기록 파이프라인 구현

- 기록 전 redaction pass를 공통 계층으로 넣는다.
- secret value, 민감 path, provider/tool 민감 payload, 환경 변수 값을 치환하거나 기록 거절한다.
- redaction 여부와 실패 사실을 diagnostics record에 남긴다.

### Wave 3. Inspect surface와 diagnostics bundle 구현

- inspect surface에 lifecycle, open turn, last durable sequence, pending approval/effect, recent diagnostics를 노출한다.
- `inspect --diagnostics` 수준의 bundle 생성기를 구현한다.
- 필요한 경우 runtime-managed artifact 경로 아래에 redacted artifact를 저장한다.

### Wave 4. Crash/recovery evidence와 회귀 검증

- bootstrap, crash, stale ownership, interrupted upgrade, recover 흐름의 evidence 수집을 연결한다.
- late result, stale wake, replay mismatch가 trace와 diagnostics에 설명 가능하게 남는지 검증한다.
- inspect와 bundle의 정보 깊이가 다르되 의미는 일치하는지 회귀 테스트를 묶는다.

## Verification Evidence

- 단위 테스트: diagnostics classification, provider/tool failure diagnostics, redaction pass, bundle field filtering, rejected artifact reference filtering, bundle generation failure diagnostics
- 통합 테스트: inspect projection, diagnostics bundle generation, diagnostics bundle generation rejection evidence, trace correlation, operational log correlation, recent subagent event `child_task_id` correlation, recent service event `service_correlation_id` correlation, event-derived diagnostics from rejected reentry and aborted turn events with `recorded_at_ms`, CLI/API diagnostics read-only parity
- 내구성 테스트: crash evidence, recovery evidence, late result observability with rejected reentry metadata and recovery projection evidence, interrupted upgrade diagnostics record, process lifecycle blockers in recovery evidence
- 안전성 테스트: secret leakage prevention in logs, traces, inspect, bundle, embedded session summaries, transport-consumed diagnostics outputs
- 문서 증거: event/log/trace/inspect 역할표, redaction 적용 지점 표

## FullSpec Evidence

- `Unit`: `crates/shacs-utils/src/progress_events.rs` inline payload tests, `crates/shacs-cli/src/lib.rs` inline facade observability tests
- `Integration`: `crates/shacs-core/tests/runtime_loop.rs`, `crates/shacs-core/tests/runtime_agent.rs`, `crates/shacs-cli/src/lib.rs` inline API/WebSocket/session diagnostics tests
- `DurabilityRecovery`: `crates/shacs-cron/src/lib.rs` durable store/log tests, `crates/shacs-cli/src/lib.rs` inline runtime update/recover marker tests

## Open Risks

- 과도한 기록 제한은 triage 근거를 약하게 만들 수 있다.
- 반대로 redaction 적용 지점이 빠지면 민감 값이 diagnostics artifact에 남을 수 있다.
- inspect surface가 편의를 위해 추론을 섞기 시작하면 truth와 진단 출력이 어긋날 수 있다.

## 종료 기준

- inspect surface가 현재 상태와 최근 실패 원인을 읽기 전용으로 설명할 수 있다.
- diagnostics bundle은 redaction을 통과한 구조화 출력만 노출한다.
- crash와 recovery evidence가 로컬에서 재현 가능한 형태로 남는다.
- log, trace, diagnostics가 event truth를 대체하지 않는다.
- 014와 016이 요구하는 단위, 통합, 내구성, 안전성 검증 증거가 확보된다.
