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
- 조직 또는 관리자용 대시보드
- 시계열 metrics 플랫폼 구축
- SOC 또는 보안 감사 포털
- 멀티테넌트 APM
- 분산 trace aggregation
- 장기 성능 분석 제품 전략

## 현재 구현 상태

이 PRD의 self-hosted/local diagnostics bundle and inspect surface 범위는 구현 근거를 갖춘 상태다. SaaS/admin/APM/Web UI/distributed trace aggregation 항목은 계속 명시적 비범위다.

### 현재 코드에서 확인되는 것

- CLI는 `status`, `runtime inspect`, `runtime diagnostics`, `runtime update`, `runtime recover`, session diagnostics 조회 surface를 제공한다.
- session 계층은 `SessionUxDetail`, `SessionUxDiagnostics`로 세션 요약, checkpoint, recovery marker, redacted diagnostics metadata 값을 projection한다.
- API는 `/health`, `/v1/diagnostics`, 로컬 session diagnostics query를 제공한다. `/health`는 alive check이며 readiness, dependency, degraded 상태 모델은 아니다.
- provider/tool progress callback plumbing과 `ToolProgressEvent`/payload helper가 있다.
- runtime checkpoint와 `pending_user_turn` marker 성격의 복구 근거가 있다.
- shared diagnostics redaction은 diagnostics serialization과 bundle writing 전에 적용된다. 기존 scoped redaction은 self-tool 출력, email error, URL 또는 token-like text, session diagnostics metadata 값에서 유지된다.

### 부분 구현 또는 future gap

- `OperationalLogRecord`, `TraceRecord`, `DiagnosticsRecord`, `CrashEvidence`, `RecoveryEvidence` formal model은 `shacs-utils`의 local JSON evidence model로 구현되어 있다.
- diagnostics bundle과 artifact generator는 `runtime diagnostics --bundle <path>` 로컬 zip artifact로 구현되어 있다.
- durable trace/log store와 event replay 기반 inspection은 snapshot/evidence field 수준만 완료되어 있으며, 장기 저장 제품은 아니다.
- readiness/dependency health와 degraded health state는 `/health`의 현재 의미 밖이다.
- provider/tool/subagent progress inspection은 snapshot field와 기존 callback/progress payload 재사용 수준까지 완료되어 있으며, 별도 장기 저장/조회 제품은 아니다.
- event stream reconnect/backpressure/dropped-event accounting은 별도 future gap이다.
- Web UI diagnostics/status surface와 multi-session observability ordering/isolation은 현재 구현 완료로 분류하지 않는다.

### 명시적 비범위

- remote observability SaaS
- organization/admin dashboard
- time-series metrics platform
- SOC/audit portal
- multi-tenant APM
- distributed trace aggregation
- long-term performance analytics product strategy

### 아직 남은 위험

- triage 근거를 충분히 남기면서도 redaction 누락을 막는 균형은 계속 open risk다.

### 로컬 근거

- `crates/shacs-utils/src/progress_events.rs`
- `crates/shacs-core/src/runtime/runner.rs`
- `crates/shacs-core/src/runtime/agent_loop.rs`
- `crates/shacs-core/src/tools/self_tool.rs`
- `crates/shacs-core/tests/runtime_loop.rs`
- `crates/shacs-core/tests/runtime_agent.rs`
- `crates/shacs-core/tests/tools.rs`
- `crates/shacs-cli/src/lib.rs`
- `crates/shacs-api/src/lib.rs`
- `crates/shacs-session/src/lib.rs`
- `crates/shacs-config/src/lib.rs`
- `crates/shacs-cron/src/lib.rs`

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

## 필요한 검증 증거

- 단위 테스트: diagnostics classification, provider/tool failure diagnostics, redaction pass, bundle field filtering, rejected artifact reference filtering, bundle generation failure diagnostics
- 통합 테스트: inspect projection, diagnostics bundle generation, diagnostics bundle generation rejection evidence, trace correlation, operational log correlation, recent subagent event `child_task_id` correlation, recent service event `service_correlation_id` correlation, event-derived diagnostics from rejected reentry and aborted turn events with `recorded_at_ms`, CLI/API diagnostics read-only parity
- 내구성 테스트: crash evidence, recovery evidence, late result observability with rejected reentry metadata and recovery projection evidence, interrupted upgrade diagnostics record, process lifecycle blockers in recovery evidence
- 안전성 테스트: secret leakage prevention in logs, traces, inspect, bundle, embedded session summaries, transport-consumed diagnostics outputs
- 문서 증거: event/log/trace/inspect 역할표, redaction 적용 지점 표

## 현재 확인된 검증 근거

- 016 기준의 전체 완료 증거로 보지 않는다.
- 현재 근거는 `crates/shacs-utils/src/progress_events.rs`, `crates/shacs-core/tests/runtime_loop.rs`, `crates/shacs-core/tests/runtime_agent.rs`, `crates/shacs-core/tests/tools.rs`, `crates/shacs-cli/src/lib.rs`, `crates/shacs-api/src/lib.rs`, `crates/shacs-session/src/lib.rs`, `crates/shacs-config/src/lib.rs`, `crates/shacs-cron/src/lib.rs`의 부분 기능 검증이다.

## Open Risks

- 과도한 기록 제한은 triage 근거를 약하게 만들 수 있다.
- 반대로 redaction 적용 지점이 빠지면 민감 값이 diagnostics artifact에 남을 수 있다.
- inspect surface가 편의를 위해 추론을 섞기 시작하면 truth와 진단 출력이 어긋날 수 있다.

## self-hosted/local baseline 종료 기준

- inspect surface가 현재 상태와 최근 실패 원인을 읽기 전용으로 설명할 수 있다.
- diagnostics bundle은 redaction을 통과한 구조화 출력만 노출한다.
- crash와 recovery evidence가 로컬에서 재현 가능한 형태로 남는다.
- log, trace, diagnostics가 event truth를 대체하지 않는다.
- 014의 self-hosted/local baseline에 필요한 단위, 통합, 안전성 검증 증거가 확보된다.

016 전체 matrix 기준의 장기 저장, replay, Web UI, 분산/운영 제품 범위는 이 PRD의 종료 기준이 아니라 future 또는 비범위 항목이다.
