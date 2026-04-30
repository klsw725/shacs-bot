# PRD 000. host safety enforcement

## 목표

이 문서는 `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`의 하위 실행 문서다. SPEC의 정책 경계를 실제 runtime enforcement, approval gate, redaction pipeline, persistence 금지 규칙으로 내려 구현 계획을 고정한다.

이번 PRD의 목표는 filesystem, process, network, secret 접근이 모두 `MainOrchestrator`의 safety snapshot 아래에서만 실행되도록 만드는 것이다. 사용자의 self-hosted 환경을 전제로 하되, workspace 바깥 접근과 secret 누출은 편의상 허용하지 않는다.

## SPEC 입력

- 주관 spec: `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`
- 선행 기준:
  - `docs/specs/001-session-kernel/SPEC.md`
  - `docs/specs/004-tool-runtime/SPEC.md`
  - `docs/specs/007-main-orchestrator-policy/SPEC.md`
  - `docs/specs/008-configuration-profiles-and-runtime-layout/SPEC.md`
  - `docs/specs/009-context-assembly-and-compaction-input/SPEC.md`
- 교차 의존:
  - `docs/specs/011-subagent-runtime/SPEC.md`
  - `docs/specs/012-runtime-services/SPEC.md`
  - `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`
  - `docs/specs/016-verification-matrix-and-release-gates/SPEC.md`

## Dependency Cut

- 007에서 approval, deny, abort 정책 결정을 받는다.
- 004는 실제 effect 실행기지만 권한 확정자는 아니며, 이 PRD가 만든 safety snapshot만 집행한다.
- 009는 provider snapshot, compaction input, logs에 secret 원문이 들어가지 않도록 연결된다.
- 011과 012는 inherited safety context와 service reentry가 상위 snapshot을 넓히지 못하게 해야 한다.
- 014는 redaction과 diagnostics 노출 규칙을 같은 안전 규약으로 사용해야 한다.

## 범위

- capability evaluator와 permission mode 해석기 구현
- filesystem, process, network, secret 경계 검증기 구현
- approval request 생성과 응답 반영 규칙 구현
- secret reference와 secret value 타입 분리
- redaction pipeline과 persistence 금지 필터 구현
- subagent, service reentry에 대한 inherited safety snapshot 전파 구현

## 범위 제외

- 관리자 승인 체계
- 중앙 secret vault 연동
- 조직 단위 정책 배포
- 원격 운영 콘솔

## 현재 구현 상태

### 이미 반영된 것

- permission mode, capability, path boundary, network scope, secret scope 평가가 host safety 계층에 구현돼 있다.
- approval request 생성/응답, stale approval 거절, denied execution 경로가 오케스트레이터와 interface surface에서 검증된다.
- filesystem read와 proc_exec executor는 safety snapshot, working directory, timeout, structured argv boundary를 따른다.
- empty `allowed_paths`는 host-wide access로 해석되지 않고, 기본 permission profile은 runtime working directory를 allowed path로 고정한다.
- tool outcome은 event/state persistence 전에 redaction되고, provider context, diagnostics, inspect, tool payload에서 secret-like 값 redaction이 검증된다.
- Spec016 matrix에서 Unit, Integration, SafetyRedaction이 FullSpec verified evidence로 승격돼 있다.

### 아직 남은 것

- OS별 path canonicalization 차이와 sandbox 강화는 계속 open risk다.
- 중앙 secret vault, 조직 정책 배포, 원격 운영 콘솔은 명시적 비범위다.

### 로컬 근거

- `crates/shacs-core/src/core/host_safety.rs`
- `crates/shacs-core/src/core/orchestrator.rs`
- `crates/shacs-core/src/core/observability.rs`
- `crates/shacs-core/tests/host_safety.rs`
- `crates/shacs-core/tests/tool_runtime.rs`
- `crates/shacs-core/tests/command_event_effect.rs`
- `crates/shacs-core/tests/observability.rs`
- `crates/shacs-runtime-adapters/tests/filesystem_tool_executor.rs`
- `crates/shacs-runtime-adapters/tests/proc_exec_tool_executor.rs`

## TDD 계획

1. capability별 허용, 승인 필요, 즉시 거절 결정표 테스트를 먼저 만든다.
2. canonical path, symlink 탈출, workspace boundary 위반 테스트를 추가한다.
3. `proc_exec`, `net_outbound`, `secret_read` approval 경로와 deny 경로 통합 테스트를 추가한다.
4. secret 원문이 state, event, snapshot, diagnostics에 남지 않는 redaction 테스트를 추가한다.
5. subagent와 runtime service reentry가 safety context를 확대하지 못하는 테스트를 추가한다.

## 구현 웨이브

### Wave 1. Safety snapshot과 evaluator 도입

- permission mode, workspace roots, capability bounds, network scope, secret scope를 가진 safety snapshot 타입을 만든다.
- effect 후보를 snapshot과 비교하는 evaluator를 구현한다.
- plan, default, auto 모드별 기본 허용 표를 고정한다.

### Wave 2. Boundary enforcement 연결

- filesystem path canonicalization과 허용 경로 판정을 구현한다.
- process envelope, working directory, timeout, environment scope 검증을 구현한다.
- provider network와 tool network를 구분하고 임의 outbound를 막는다.
- secret resolve를 늦게 수행하고 secret value의 역직렬화 저장을 금지한다.

### Wave 3. Approval과 redaction 파이프라인 완성

- approval request 생성, correlation, stale approval 거절 흐름을 구현한다.
- 실행 전 redaction requirement를 snapshot에 포함한다.
- 실행 결과, diagnostics, inspect, provider input 생성 전에 redaction pass를 연결한다.

### Wave 4. 하위 실행 경계 전파와 회귀 검증

- subagent spawn envelope와 runtime service command envelope에 inherited safety snapshot을 연결한다.
- late result, stale approval, interrupted process 상황에서도 boundary가 유지되는지 검증한다.
- deny, cancel, recover 시나리오를 포함한 회귀 테스트를 고정한다.

## Verification Evidence

- 단위 테스트: capability evaluation, permission mode 해석, path boundary, network/secret scope boundary, redaction rules
- path boundary 테스트: `host_safety`와 `tool_runtime`에서 canonical child 허용, string-prefix sibling 거절, Unix symlink escape 거절을 검증한다.
- capability 결정표 테스트: `plan_mode_denies_each_risky_capability`, `default_mode_requires_approval_for_scoped_risky_capabilities`, `empty_allowed_paths_does_not_grant_host_wide_access`가 risky capability와 empty path boundary를 고정한다.
- process boundary 테스트: concrete `proc_exec` executor가 structured `argv`만 실행하고 relative executable, invalid argv, timeout을 정규화된 실패/timeout으로 반환한다.
- 통합 테스트: approval flow, denied execution, secret-dependent effect, inherited safety context propagation
- secret-dependent effect 테스트: `secret_read_tool_request_requires_approval_and_preserves_requested_scope`와 `secret_read_tool_request_is_denied_when_scope_misses_profile`가 secret scope approval/deny 경계를 검증한다.
- 안전성 테스트: secret leakage prevention, symlink escape 차단, network boundary enforcement
- persistence redaction 테스트: `tool_outcome_is_redacted_before_event_and_state_persistence`가 raw secret-like tool output/error/observation이 event/state에 저장되기 전에 redaction되는지 검증한다.
- 내구성 테스트: stale approval, late result, service reentry after restart
- 문서 증거: capability별 정책표와 저장 금지 데이터 목록

## Open Risks

- OS별 path canonicalization 차이가 boundary 판정에 영향을 줄 수 있다.
- tool 결과가 예상보다 큰 경우 redaction 누락 지점이 생길 수 있다.
- approval surface와 실제 effect envelope가 어긋나면 사용자 기대와 정책 집행이 달라질 수 있다.

## 종료 기준

- 모든 host effect가 safety snapshot 없이는 실행되지 않는다.
- approval, deny, auto-allow 판단이 mode와 boundary 규칙에 따라 자동 검증된다.
- secret 원문이 state, logs, diagnostics, provider context에 남지 않는다.
- subagent와 service reentry가 상위 safety 제약을 넓히지 못한다.
- 010과 016이 요구하는 단위, 통합, 안전성 검증 증거가 준비된다.
