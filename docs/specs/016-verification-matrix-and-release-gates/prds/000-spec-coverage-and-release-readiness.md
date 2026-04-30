# PRD 000. spec coverage and release readiness

## 목표

이 문서는 `docs/specs/016-verification-matrix-and-release-gates/SPEC.md`의 하위 실행 문서다. 전체 spec 001~015를 release 가능한 제품 수준까지 검증하기 위한 테스트 체계, coverage 운영 방식, gate 집행 절차를 실제 실행 문서로 정리한다.

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

## Dependency Cut

- 이 PRD는 개별 기능을 직접 구현하지 않고, 각 spec PRD가 남기는 테스트와 증거를 release gate에 연결한다.
- 009~015는 각각의 domain test family를 제공해야 하며, 이 PRD는 이를 하나의 coverage matrix와 gate runner로 묶는다.
- 014와 015의 diagnostics, packaging 증거는 release readiness 판단의 필수 입력이다.
- 010의 safety와 redaction은 waiver 금지 대상이라 별도 blocker lane으로 취급한다.

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

- `scripts/release-gate`가 local Cargo-only shipping/minimum-slice release gate runner로 제공된다.
- `verification_matrix` 테스트가 spec coverage matrix, required family, evidence locator, release readiness decision, full-spec gap 분리를 검증한다.
- `release_candidate_smoke` 테스트가 fresh workspace 기준 session create, input submit, progress inspect, recovery inspect/recover, approval inspect/respond, list 흐름을 release candidate smoke evidence로 검증한다.
- README와 USAGE는 `scripts/release-gate`가 local Cargo-only shipping gate이며, full-spec readiness는 matrix evidence decision으로 별도 판정된다고 명시한다.

### 지속 관리 지점

- full-spec promotion은 각 required family별 `CoverageLevel::FullSpec` + `CoverageStatus::Verified` evidence가 명시되어야 하며, script 통과만으로 승격되지 않는다.
- 새 spec 또는 테스트 파일 변경 시 coverage matrix, evidence locator policy, release gate 대표 테스트가 함께 갱신되지 않으면 traceability drift가 생길 수 있다.

### 로컬 근거

- `scripts/release-gate`
- `crates/shacs-core/tests/verification_matrix.rs`
- `crates/shacs-cli/tests/release_candidate_smoke.rs`
- `README.md`
- `docs/USAGE.md`

## TDD 계획

1. spec별 coverage matrix가 비어 있으면 실패하는 메타 검증을 만든다.
2. blocker 규칙, waiver 금지 규칙, gate 통과 조건을 판정하는 단위 테스트를 만든다.
3. 각 verification family에 최소 하나의 실제 테스트 그룹이 연결되는지 통합 검증을 추가한다.
4. release candidate smoke test가 install, create session, input, approval, inspect, recover 경로를 검증하는 end-to-end 테스트를 추가한다.
5. flaky test, missing evidence, interrupted upgrade 미검증 상태를 release 불가로 판정하는 테스트를 추가한다.

## 구현 웨이브

### Wave 1. Coverage matrix와 traceability 구축

- spec 001~015 각각에 대해 required verification family를 명시한 matrix를 만든다.
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

- 단위 테스트: gate decision table, blocker classification, waiver prohibition, matrix completeness
- 단위 테스트: `missing_release_gate_evaluation_blocks_release`가 필수 release gate 평가 누락을 release blocker로 판정하고, ready 판정 테스트들이 모든 `required_release_gates()`의 `Pass` 증거를 요구함을 검증한다.
- 단위 테스트: `release_gate_script_covers_required_gate_representatives`가 `scripts/release-gate` 단계와 gate 대표 테스트 drift를 검증한다.
- 단위 테스트: `full_spec_verified_evidence_uses_executable_repo_relative_locators`가 FullSpec evidence locator를 repo-relative executable evidence로 제한하고 matrix self-certification을 막는다.
- 단위 테스트: `every_blocker_kind_blocks_release_even_if_waiver_is_marked_allowed`가 blocker waiver가 release를 열지 못함을 검증한다.
- 단위 테스트: `missing_one_spec_full_spec_evidence_reports_only_that_spec`가 missing FullSpec evidence가 정확한 spec gap으로 보고됨을 검증한다.
- 통합 테스트: verification family to spec mapping, release pipeline aggregation, evidence collection flow
- 통합 테스트: `crates/shacs-cli/tests/release_candidate_smoke.rs`가 fresh workspace CLI 흐름을 하나의 release candidate smoke evidence로 검증한다.
- 패키징 및 smoke 테스트: fresh install, create session, input handling, approval surface, inspect, recover
- 내구성 테스트: interrupted upgrade and recovery evidence must be covered before ready state
- 문서 증거: spec coverage matrix, release checklist, blocker taxonomy, waiver template

## Release gate runner

로컬 release gate는 저장소 루트의 `scripts/release-gate`로 실행한다. 이 runner는 self-hosted/personal-use 개발자가 외부 CI 서비스 없이 같은 판정을 반복할 수 있도록 Cargo 하위 명령만 사용한다.

```sh
scripts/release-gate
```

runner는 첫 실패에서 중단하며, 각 단계는 구현된 제품 범위의 shipping release gate 증거를 대표한다. 이 runner의 통과는 full-spec completion 판정 그 자체가 아니며, `verification_matrix`가 full-spec readiness와 evidence rules를 별도로 판정한다.

1. `cargo fmt --check --all`
   - 정적 형식 검증. 포맷 drift가 있으면 release 후보가 아니다.
2. `cargo check --workspace --all-targets --locked`
   - workspace 전체 compile/type boundary 검증. 루트 `default-members`가 일부 crate만 가리키는 것을 피하기 위해 `--workspace`를 고정한다.
3. `cargo clippy --workspace --all-targets --locked -- -D warnings`
   - warning-free lint gate. 경고를 release blocker로 취급한다.
4. `cargo test -p shacs-bot --test command_event_effect --locked`
   - core command/event/effect contract gate 대표 테스트.
5. `cargo test -p shacs-bot --test session_store_replay --locked`
   - recovery/durability gate 대표 테스트.
6. `cargo test -p shacs-bot --test host_safety --locked`
   - safety/redaction gate 대표 테스트.
7. `cargo test -p shacs-cli --test api_serve --locked`
   - interface contract gate 대표 테스트.
8. `cargo test -p shacs-cli --test runtime_inspect_cli --locked`
   - packaging/upgrade gate 대표 테스트.
9. `cargo test -p shacs-cli --test release_candidate_smoke --locked`
   - release candidate smoke gate. fresh workspace에서 runtime inspect/update/start/stop, session create, input submit, progress inspect, recovery inspect/recover, approval inspect/respond, list 흐름을 하나의 반복 가능한 CLI smoke로 검증한다.
10. `cargo test -p shacs-bot --test verification_matrix --locked`
   - spec 001~015 coverage matrix, FullSpec evidence locator policy, release gate script drift, blocker/waiver, readiness decision을 검증한다.
11. `cargo test --workspace --locked`
   - core/contracts/runtime adapters/surface/CLI 전체 regression suite를 실행한다.

`verification_matrix` 테스트는 matrix 행, required family, release readiness decision, spec 문서 존재, repo-relative executable evidence locator, release-gate script representatives를 함께 확인한다. full-spec promotion은 `full_spec_level` 플래그만으로는 불가능하며, 각 required family별 `CoverageLevel::FullSpec` + `CoverageStatus::Verified` evidence가 명시되어야 한다. product spec matrix는 Spec001~Spec015를 대상으로 유지하며, Spec016은 이 matrix를 자기 자신만으로 증명하지 않도록 release-gate/evidence-locator/readiness-separation meta tests로 검증한다. `release_candidate_smoke` 테스트는 Gate 7을 workspace 전체 테스트에 묻히지 않는 독립 증거로 남긴다. 따라서 runner가 통과하려면 새 spec 또는 테스트 파일 변경이 coverage matrix, release gate representative, smoke gate와 함께 갱신되어야 한다.

## Open Risks

- spec와 테스트 이름이 느슨하게 연결되면 coverage 공백이 숨어 있을 수 있다. 현재 locator policy는 repo-relative executable evidence를 강제한다.
- smoke test가 너무 얕으면 demo behavior를 full implementation처럼 오인할 수 있다. 현재 release gate는 smoke를 독립 단계로 두고 workspace regression과 matrix를 함께 실행한다.
- waiver 운영이 느슨해지면 blocker가 문서상으로만 사라질 수 있다. 현재 모든 `BlockerKind`는 waiver 표시와 관계없이 release를 차단한다.
- 참고 메모: SPEC와 PRD 사이의 용어와 템플릿 형식이 부분적으로 섞여 있어, traceability가 약하면 같은 결함이 coverage matrix에서 다른 이름으로 중복되거나 누락될 수 있다.

## 종료 기준

- spec 001~015 모두에 required verification family가 매핑된다.
- release gate 1~7이 자동 또는 반복 가능한 절차로 실행된다.
- blocker와 waiver 금지 대상이 명시적 규칙과 테스트로 강제된다.
- release candidate smoke test가 self-hosted 사용자 최소 흐름을 검증한다.
- "full implementation"과 "demo behavior" 구분이 문구가 아니라 증거 체계로 작동한다.

## FullSpec 승격 상태

- 상태: FullSpec evidence ready.
- Spec016은 product spec matrix에 `SpecId::Spec016`으로 자기 자신을 추가하지 않는다.
- product spec matrix 대상은 문서 계약대로 Spec001~Spec015이며, Spec016은 release gate/evidence locator/readiness separation을 검증하는 meta-verification layer로 닫는다.
- FullSpec evidence:
  - `crates/shacs-core/tests/verification_matrix.rs`
  - `scripts/release-gate`
  - `crates/shacs-cli/tests/release_candidate_smoke.rs`
  - `README.md`
  - `docs/USAGE.md`
- 비범위로 남는 항목: 특정 CI 서비스, 브랜치 전략, 사람 조직 승인 라인, 마케팅 출시 계획, SaaS 운영 SLA.
