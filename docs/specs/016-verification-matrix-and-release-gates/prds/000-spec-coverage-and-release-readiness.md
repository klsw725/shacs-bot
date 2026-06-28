# PRD 000. spec coverage and release readiness

## 목표

이 문서는 `docs/specs/016-verification-matrix-and-release-gates/SPEC.md`의 하위 실행 문서다. 전체 spec 001~015, 017, 023을 release 가능한 제품 수준까지 검증하기 위한 테스트 체계, coverage 운영 방식, gate 집행 절차를 실제 실행 문서로 정리한다.

이번 PRD의 목표는 "데모가 된다"와 "출시 가능하다"를 코드와 증거로 구분하는 것이다. 각 spec에 대해 어떤 자동 검증이 있어야 하는지, blocker가 무엇인지, release candidate가 어디서 멈춰야 하는지를 구현과 운영 절차 양쪽에서 고정한다.

## SPEC 입력

- 주관 spec: `docs/specs/016-verification-matrix-and-release-gates/SPEC.md`
- 상위 입력:
  - `docs/SYSTEM-FOUNDATION.md`
  - `docs/specs/001-session-kernel/SPEC.md`
  - `docs/specs/002-command-event-effect/SPEC.md`
  - `docs/specs/003-provider-runtime/SPEC.md`
  - `docs/specs/004-tool-runtime/SPEC.md`
  - `docs/specs/005-skill-system/SPEC.md`
  - `docs/specs/006-session-store/SPEC.md`
  - `docs/specs/007-main-orchestrator-policy/SPEC.md`
  - `docs/specs/008-configuration-profiles-and-runtime-layout/SPEC.md`
  - `docs/specs/009-context-assembly-and-compaction-input/SPEC.md`
  - `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`
  - `docs/specs/011-subagent-runtime/SPEC.md`
  - `docs/specs/012-runtime-services/SPEC.md`
  - `docs/specs/013-user-interfaces-and-session-ux/SPEC.md`
  - `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`
  - `docs/specs/015-packaging-process-lifecycle-and-upgrades/SPEC.md`
  - `docs/specs/017-app-operating-environment/SPEC.md`
  - `docs/specs/023-zero-setup-sandbox-execution/SPEC.md`

## Dependency Cut

- 이 PRD는 개별 기능을 직접 구현하지 않고, 각 spec PRD가 남기는 테스트와 증거를 release gate에 연결한다.
- 009~015와 017은 각각의 domain test family를 제공해야 하며, 이 PRD는 이를 하나의 coverage matrix와 gate runner로 묶는다.
- 014와 015의 diagnostics, packaging 증거는 release readiness 판단의 필수 입력이다.
- 010의 safety와 redaction은 waiver 금지 대상이라 별도 blocker lane으로 취급한다.
- 023의 zero-setup sandbox execution 증거는 containment diagnostics, native unknown fallback, unsafe privileged fallback, exec fail-closed 또는 scope narrowing, MCP and subagent inheritance를 실제 Cargo test 이름으로 연결한다. 공식 Docker/Compose runtime containment evidence는 opt-in smoke command `./docs/scripts/spec023-compose-smoke.sh`로 연결한다.

## 범위

- verification family별 테스트 배치와 naming 기준 정리
- spec coverage matrix와 traceability 규칙 정의
- blocker, non-blocker, waiver 집행 규칙 정의
- release gate runner와 release candidate smoke test 절차 정의
- 실패 triage와 evidence collection 절차 정의
- demo behavior 판별 체크리스트를 자동화 우선 기준으로 정리

## 범위 제외

- 특정 CI 서비스 선택
- 브랜치 전략
- 사람 조직 승인 라인 설계
- 마케팅 출시 계획

## 현재 구현 상태

### 이미 반영된 것

- 현재 저장소는 Cargo manifest path 기준 fmt/clippy/test/build와 crate inline tests를 release 후보의 반복 가능한 최소 검증으로 사용한다.
- `crates/shacs-cli/src/lib.rs`의 inline tests가 runtime inspect/update/recover, session commands, API/WebSocket bridge, facade hook 동작을 검증한다.
- README와 USAGE는 source/Cargo 기반 self-hosted install/update/recover 절차를 설명한다.

### 지속 관리 지점

- full-spec promotion은 각 required family별 실행 가능한 evidence locator가 명시되어야 하며, script 통과만으로 승격되지 않는다.
- 새 spec 또는 테스트 파일 변경 시 coverage matrix, evidence locator policy, release gate 대표 테스트가 함께 갱신되지 않으면 traceability drift가 생길 수 있다.

### 로컬 근거

- `crates/shacs-cli/src/lib.rs` inline tests
- `crates/shacs-core/tests/runtime_loop.rs`
- `crates/shacs-core/tests/runtime_agent.rs`
- `README.md`
- `docs/USAGE.md`
- Spec023 containment and permission evidence:
  - `crates/shacs-cli/src/lib.rs` inline tests: `runtime_containment_classifier_reports_native_unknown`, `runtime_containment_snapshot_ref_preserves_unknown_state`, `runtime_containment_classifier_reports_official_container_marker`, `runtime_containment_classifier_reports_recognized_container_evidence`, `runtime_containment_classifier_reports_unsafe_privileged_evidence`, `bypass_permissions_falls_back_for_native_unknown_containment`, `bypass_permissions_falls_back_for_unsafe_privileged`
  - `crates/shacs-core/tests/tools.rs`: `exec_tool_bwrap_sandbox_setup_failure_does_not_execute_original_command`, `exec_tool_native_unknown_without_backend_enforces_workspace_scope`, `exec_tool_unknown_sandbox_backend_does_not_execute_command`, `mcp_runtime_connects_registers_and_closes_servers`, `mcp_default_deny_excludes_disabled_capabilities_from_tool_search_bridge`
  - `crates/shacs-core/tests/runtime_loop.rs`: `subagent_permissioned_action_context_inherits_snapshots_and_origin`

## TDD 계획

1. spec별 coverage matrix가 비어 있으면 실패하는 메타 검증을 만든다.
2. blocker 규칙, waiver 금지 규칙, gate 통과 조건을 판정하는 단위 테스트를 만든다.
3. 각 verification family에 최소 하나의 실제 테스트 그룹이 연결되는지 통합 검증을 추가한다.
4. release candidate smoke test가 install, create session, input, approval, inspect, recover 경로를 검증하는 end-to-end 테스트를 추가한다.
5. flaky test, missing evidence, interrupted upgrade 미검증 상태를 release 불가로 판정하는 테스트를 추가한다.

## 구현 웨이브

### Wave 1. Coverage matrix와 traceability 구축

- spec 001~015와 017 각각에 대해 required verification family를 명시한 matrix를 만든다.
- 테스트 케이스와 spec 조항을 연결하는 traceability 메타데이터를 도입한다.
- 미연결 spec 항목이 있으면 보고되도록 한다.

### Wave 2. Gate 판정 규칙 구현

- 정적 검증, 핵심 계약, recovery, safety, interface, packaging, smoke test gate를 코드나 스크립트 수준에서 판정 가능하게 만든다.
- blocker와 non-blocker를 구분하고 waiver 금지 항목을 강제한다.
- missing evidence를 경고가 아니라 gate 실패로 처리한다.

### Wave 3. Release candidate 실행 경로 구축

- 새 설치부터 recover까지 포함한 release candidate smoke flow를 자동화한다.
- diagnostics bundle, inspect output, package version metadata 같은 증거 수집 경로를 연결한다.
- 실패 시 triage에 필요한 최소 artifact 목록을 고정한다.

### Wave 4. 지속 검증과 완료 정의 고정

- 개별 spec PRD가 추가되거나 수정될 때 matrix 갱신이 빠지지 않도록 검증한다.
- demo behavior 징후를 자동 탐지 가능한 항목부터 gate에 편입한다.
- release readiness 보고 형식을 고정해 blocker와 잔여 non-blocker를 분리 표시한다.

## Verification Evidence

- 문서 증거: gate decision table, blocker classification, waiver prohibition, matrix completeness는 이 PRD와 SPEC의 release gate 규칙으로 유지한다.
- 문서 증거: release gate representative drift는 별도 runner가 추가될 때 해당 runner path와 함께 검증한다. 현재 slice는 manifest-path Cargo command 목록과 실제 inline/integration test names를 문서 증거로 유지한다.
- 문서 증거: FullSpec evidence locator는 repo-relative executable evidence를 목표로 하며, 현재 문서는 존재하지 않는 runner/test file path를 evidence로 쓰지 않는다.
- 통합 증거: verification family to spec mapping, release pipeline aggregation, evidence collection flow는 실제 Cargo command와 docs locator 일치성으로 관리한다.
- 통합 테스트: 현재 slice에서는 `crates/shacs-cli/src/lib.rs` inline tests와 manifest-path cargo commands가 fresh workspace CLI 흐름을 검증한다. Docker/Compose runtime containment는 opt-in smoke script를 evidence locator로 쓴다.
- Spec023 release evidence lane: 현재 lane은 실제 존재하는 Cargo test filter와 real Compose smoke command만 근거로 쓰며, Spec 023 closed status를 현재 evidence scope에서 뒷받침한다. App process supervisor는 아직 app process를 시작하지 않으므로 active inheritance evidence로 과장하지 않는다. Compose smoke는 official-container runtime evidence와 기본 Compose static safety를 담당한다. Provider credential 없이 full MCP child execution smoke를 반복 가능하게 만들 수 없는 동안 MCP containment inheritance는 core test snapshot/default-deny evidence가 담당한다.
  - `./docs/scripts/spec023-compose-smoke.sh`
  - `cargo test --manifest-path crates/Cargo.toml -p shacs-cli runtime_containment_classifier_reports_native_unknown`
  - `cargo test --manifest-path crates/Cargo.toml -p shacs-cli runtime_containment_snapshot_ref_preserves_unknown_state`
  - `cargo test --manifest-path crates/Cargo.toml -p shacs-cli runtime_containment_classifier_reports_official_container_marker`
  - `cargo test --manifest-path crates/Cargo.toml -p shacs-cli runtime_containment_classifier_reports_recognized_container_evidence`
  - `cargo test --manifest-path crates/Cargo.toml -p shacs-cli runtime_containment_classifier_reports_unsafe_privileged_evidence`
  - `cargo test --manifest-path crates/Cargo.toml -p shacs-cli bypass_permissions_falls_back_for_native_unknown_containment`
  - `cargo test --manifest-path crates/Cargo.toml -p shacs-cli bypass_permissions_falls_back_for_unsafe_privileged`
  - `cargo test --manifest-path crates/Cargo.toml -p shacs-core exec_tool_bwrap_sandbox_setup_failure_does_not_execute_original_command`
  - `cargo test --manifest-path crates/Cargo.toml -p shacs-core exec_tool_native_unknown_without_backend_enforces_workspace_scope`
  - `cargo test --manifest-path crates/Cargo.toml -p shacs-core exec_tool_unknown_sandbox_backend_does_not_execute_command`
  - `cargo test --manifest-path crates/Cargo.toml -p shacs-core mcp_runtime_connects_registers_and_closes_servers`
  - `cargo test --manifest-path crates/Cargo.toml -p shacs-core mcp_default_deny_excludes_disabled_capabilities_from_tool_search_bridge`
  - `cargo test --manifest-path crates/Cargo.toml -p shacs-core subagent_permissioned_action_context_inherits_snapshots_and_origin`
- 패키징 및 smoke 테스트: fresh install, create session, input handling, approval surface, inspect, recover
- 내구성 테스트: interrupted upgrade and recovery evidence must be covered before ready state
- 문서 증거: spec coverage matrix, release checklist, blocker taxonomy, waiver template

## Release gate runner

로컬 release gate는 현재 slice에서 Cargo 하위 명령을 `crates/Cargo.toml` Rust workspace manifest 기준으로 직접 실행한다. 별도 runner script가 추가되면 이 표를 그 실행 경로와 동기화한다.

대표 명령은 첫 실패에서 중단해 실행하며, 각 단계는 구현된 제품 범위의 shipping release gate 증거를 대표한다. 이 명령들의 통과는 full-spec completion 판정 그 자체가 아니며, 문서화된 evidence locator와 함께 해석한다.

1. `cargo fmt --manifest-path crates/Cargo.toml --all -- --check`
   - 정적 형식 검증. 포맷 drift가 있으면 release 후보가 아니다.
2. `cargo check --manifest-path crates/Cargo.toml -p shacs-cli --all-targets --locked`
   - CLI crate가 의존하는 runtime/API/channel/config 경계의 compile/type boundary를 검증한다.
3. `cargo clippy --manifest-path crates/Cargo.toml -p shacs-cli --all-targets --locked -- -D warnings`
   - warning-free lint gate. 경고를 release blocker로 취급한다.
4. `cargo test --manifest-path crates/Cargo.toml -p shacs-cli runtime_update`
   - packaging/update/recover marker 대표 테스트.
5. `cargo test --manifest-path crates/Cargo.toml -p shacs-cli programmatic_facade`
   - SDK facade lifecycle/observability 대표 테스트.
6. `cargo test --manifest-path crates/Cargo.toml -p shacs-core --test runtime_loop`
   - core runtime loop/callback regression suite.
7. `cargo test --manifest-path crates/Cargo.toml -p shacs-cli --locked`
   - CLI와 그 manifest dependency graph의 regression suite를 실행한다.
8. `cargo test --manifest-path crates/Cargo.toml -p shacs-cli bypass_permissions_falls_back_for_unsafe_privileged`
   - Spec023 unsafe privileged containment evidence가 permissive permission mode를 유지하지 못하게 하는 대표 안전 회귀 테스트다.

별도 verification matrix crate/test가 추가되면 matrix 행, required family, release readiness decision, spec 문서 존재, repo-relative executable evidence locator, release-gate script representatives를 함께 확인해야 한다. 현재 문서는 존재하지 않는 test file path를 evidence로 쓰지 않고, 실제 Cargo로 실행 가능한 inline/integration tests만 근거로 삼는다.

## Open Risks

- spec와 테스트 이름이 느슨하게 연결되면 coverage 공백이 숨어 있을 수 있다. 현재 locator policy는 repo-relative executable evidence를 강제한다.
- smoke test가 너무 얕으면 demo behavior를 full implementation처럼 오인할 수 있다. 현재 release gate는 smoke를 독립 단계로 두고 workspace regression과 matrix를 함께 실행한다.
- waiver 운영이 느슨해지면 blocker가 문서상으로만 사라질 수 있다. 현재 모든 `BlockerKind`는 waiver 표시와 관계없이 release를 차단한다.
- 참고 메모: SPEC와 PRD 사이의 용어와 템플릿 형식이 부분적으로 섞여 있어, traceability가 약하면 같은 결함이 coverage matrix에서 다른 이름으로 중복되거나 누락될 수 있다.

## 종료 기준

- spec 001~015, 017, 023 모두에 required verification family가 매핑된다.
- release gate 1~8이 자동 또는 반복 가능한 절차로 실행된다.
- blocker와 waiver 금지 대상이 명시적 규칙과 테스트로 강제된다.
- release candidate smoke test가 self-hosted 사용자 최소 흐름을 검증한다.
- "full implementation"과 "demo behavior" 구분이 문구가 아니라 증거 체계로 작동한다.

## FullSpec 승격 상태

- 상태: manifest-path release gate evidence ready.
- Spec016은 product spec matrix에 자기 자신을 product spec row로 추가하지 않는다.
- product spec matrix 대상은 문서 계약대로 Spec001~Spec015이며, post-015 evidence lane은 Spec017과 Spec023의 실제 Cargo evidence locator를 별도 연결한다. Spec016은 release gate/evidence locator/readiness separation을 검증하는 meta-verification layer로 닫는다.
- FullSpec evidence:
  - `crates/shacs-cli/src/lib.rs` inline tests
  - `crates/shacs-core/tests/runtime_loop.rs`
  - `crates/shacs-core/tests/runtime_agent.rs`
  - `README.md`
  - `docs/USAGE.md`
- 비범위로 남는 항목: 특정 CI 서비스, 브랜치 전략, 사람 조직 승인 라인, 마케팅 출시 계획, SaaS 운영 SLA.
