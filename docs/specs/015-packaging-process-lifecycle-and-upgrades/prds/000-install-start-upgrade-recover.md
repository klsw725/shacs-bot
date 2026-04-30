# PRD 000. install, start, upgrade, recover

## 목표

이 문서는 `docs/specs/015-packaging-process-lifecycle-and-upgrades/SPEC.md`의 하위 실행 문서다. self-hosted 사용자가 직접 설치, 시작, 중지, 업그레이드, 복구할 수 있는 실제 제품 수명주기를 구현 단위로 내린다.

이번 PRD의 목표는 바이너리 교체가 아니라, runtime root ownership, compatibility 검사, migration gate, interrupted upgrade 방어까지 포함한 전체 lifecycle을 shipping 가능한 수준으로 고정하는 것이다.

## SPEC 입력

- 주관 spec: `docs/specs/015-packaging-process-lifecycle-and-upgrades/SPEC.md`
- 선행 기준:
- `docs/SYSTEM-FOUNDATION.md`
  - `docs/specs/006-session-store/SPEC.md`
  - `docs/specs/008-configuration-profiles-and-runtime-layout/SPEC.md`
  - `docs/specs/012-runtime-services/SPEC.md`
  - `docs/specs/013-user-interfaces-and-session-ux/SPEC.md`
  - `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`
- 교차 의존:
  - `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`
  - `docs/specs/016-verification-matrix-and-release-gates/SPEC.md`

## Dependency Cut

- 008은 runtime root layout, internal directories, config discovery 규칙을 제공한다.
- 006은 session store, checkpoint, replay correctness를 제공하므로 upgrade 중에도 truth 손상이 없어야 한다.
- 012는 service bootstrap, draining, stale worker signal 처리와 연결된다.
- 013은 install/start/inspect/recover를 사용자가 실제로 조작하는 표면을 제공한다.
- 014는 ownership marker, interrupted upgrade marker, migration failure를 진단하는 근거를 제공한다.
- 010은 recover나 migration 중에도 secret handling과 writable boundary가 무너지지 않게 한다.

## 범위

- 패키징 단위, 버전 식별, compatibility metadata 설계
- install, start, stop, restart 절차 구현
- runtime ownership marker와 stale marker 구현
- compatibility 검사와 migration runner 구현
- interrupted upgrade marker와 recover 절차 구현
- inspect-only mode, blocked start, safe stop 테스트 추가

## 범위 제외

- OS 패키지 매니저별 배포 스크립트 확장
- fleet management
- 원격 rolling upgrade
- 웹 installer

## 현재 구현 상태

### 이미 반영된 것

- runtime inspect/start/stop/recover/restart CLI 경로가 runtime root ownership marker와 compatibility guard를 기준으로 동작한다.
- runtime update CLI 경로가 target version을 받아 upgrade marker를 기록하고, 현재 no-op migration completion 경로를 거쳐 `completed_cleanup` 상태를 inspect/start 가능한 상태로 남긴다.
- active ownership과 stale ownership이 구분되며, `runtime recover`는 stale marker만 지우고 active ownership, partial migration, interrupted upgrade는 차단한다.
- session store format compatibility, inspect-only compatible, incompatible data, migration required, partial migration marker가 mutation guard와 recovery projection에 반영된다.
- interrupted upgrade marker details와 process lifecycle blockers가 diagnostics/recovery evidence로 노출된다.
- ownership/upgrade marker write는 temporary file, file sync, atomic rename, parent directory sync를 사용해 interrupted write가 false healthy 상태로 보이는 위험을 줄인다.

### 비범위 / 후속 확장

- OS package manager별 installer, 원격 rolling upgrade, 웹 installer는 비범위다.
- migration rollback/cleanup 전략은 실제 data-transform migration이 추가될 때 확장할 장기 운영 risk다. 현재 FullSpec evidence는 self-hosted local update, no-op migration completion, marker durability, compatibility guard, inspect/recover blocking 범위에서 닫으며, data-transform migration rollback은 현재 제품 범위의 release blocker가 아니다.

### 로컬 근거

- `crates/shacs-core/src/core/lifecycle.rs`
- `crates/shacs-core/tests/process_lifecycle.rs`
- `crates/shacs-cli/src/lib.rs`
- `crates/shacs-cli/tests/runtime_inspect_cli.rs`
- `crates/shacs-cli/tests/release_candidate_smoke.rs`
- `crates/shacs-cli/tests/session_recover_cli.rs`
- `crates/shacs-cli/tests/session_submit_cli.rs`
- `crates/shacs-cli/tests/api_serve.rs`

## TDD 계획

1. compatibility 결과 분류와 start 허용 결정표 단위 테스트를 만든다.
2. ownership marker 생성, active marker 충돌, stale marker 정리 테스트를 추가한다.
3. install 후 first start, safe stop, clean restart 통합 테스트를 추가한다.
4. migration required, interrupted upgrade, partial migration, inspect-only mode 테스트를 추가한다.
5. recover 이후 start 가능 여부와 writable state 전환 테스트를 추가한다.

## 구현 웨이브

### Wave 1. Packaging metadata와 bootstrap guard 구현

- binary version, schema compatibility 범위, bundled resource metadata를 식별 가능한 형식으로 제공한다.
- start 진입 전 runtime root, config, secrets, ownership marker, compatibility 상태를 검사한다.
- active ownership marker가 유효하면 중복 start를 막는다.

### Wave 2. Lifecycle 제어 구현

- install, start, stop, restart 절차를 명시적 단계로 구현한다.
- stop 시 새 입력 수용 중지, draining, shutdown reason 기록, marker 정리를 연결한다.
- CLI와 inspect surface에서 현재 lifecycle 상태를 읽을 수 있게 한다.

### Wave 3. Compatibility와 migration gate 구현

- fully compatible, migration required, inspect-only, incompatible 분류를 구현한다.
- migration runner가 대상 버전, 결과 버전, partial marker를 기록하게 만든다.
- migration 완료 전 running 상태 진입을 금지한다.

### Wave 4. Interrupted upgrade와 recover 완성

- upgrade marker, partial migration marker, stale ownership marker를 감지한다.
- inspect/recover surface가 upgrade marker의 from/target version, phase, partial migration 여부와 무엇이 아직 막는지 기록하게 만든다.
- interrupted upgrade 이후 inspect-only는 허용하되 mutation은 막는다.
- install, update, recover, restart 경로를 end-to-end 회귀 테스트로 묶는다.

## Verification Evidence

- 단위 테스트: `classify_data_compatibility`, `evaluate_start_admission_with_data_compatibility`, marker parsing, no-op migration runner phase/result marker, migration state machine
- 통합 테스트: install/start/stop/restart, migration-required start, recover flow
- 통합 테스트: CLI `runtime start` ownership marker write, second start active ownership conflict, `runtime stop` marker cleanup, stale-only `runtime recover` marker cleanup, active/partial-migration/interrupted-upgrade recover blocking, `runtime restart` marker rewrite
- 통합 테스트: CLI `runtime update --target-version ...` upgrade marker completion, active ownership update blocking, partial migration update blocking, update 후 clean start
- 통합 테스트: bootstrap이 session store format version을 관측해 inspect-only compatible과 incompatible data를 mutation guard와 recovery projection에 반영
- 내구성 테스트: interrupted upgrade marker details, partial migration, stale ownership, atomic marker write/cleanup, crash during upgrade
- 패키징 테스트: `runtime inspect` version metadata exposure, bundled resource discovery, inspect-only mode
- 문서 증거: lifecycle 단계표, fully compatible / migration required / inspect-only compatible / incompatible compatibility 결과표, recover 결정표
- matrix 증거: `SpecId::Spec015` FullSpec evidence가 `PackagingUpgrade`, `DurabilityRecovery`, `Integration`을 모두 Verified로 가리킨다.

## Open Risks

- marker 정리 순서가 잘못되면 중단 후 false healthy 상태로 보일 수 있다. 현재 marker write/delete 경로는 atomic rename과 directory sync로 방어한다.
- migration이 in-place overwrite에 기대면 rollback과 recovery 설명이 어려워질 수 있다. 현재 구현은 data-transform migration을 수행하지 않고 marker 기반 no-op completion만 제공한다.
- inspect-only와 writable mode 경계가 약하면 손상 상태에서도 mutation이 열릴 수 있다. 현재 CLI/API/session mutation guard가 partial migration, inspect-only compatibility, incompatible data를 차단한다.
- 참고 메모: ownership/stale/interrupted-upgrade marker의 위치와 cleanup 순서는 008, 014와 함께 정리되어야 하므로, 본 PRD만으로 runtime-managed file lifecycle이 완결되지는 않는다.

## 종료 기준

- 사용자가 로컬에서 build/install-equivalent Cargo artifact, start, stop, inspect, update marker flow, recover를 수행할 수 있다.
- active ownership과 stale ownership이 구분되며 중복 주 인스턴스 실행이 막힌다.
- compatibility mismatch와 interrupted upgrade가 감지되면 running 진입이 차단된다.
- migration required는 migration 전 running이 차단되고, inspect-only compatible은 mutation이 차단되며, incompatible data는 start가 차단된다.
- migration 완료 전 성공 실행처럼 보이지 않는다.
- 015와 016이 요구하는 패키징, 통합, 내구성 검증 증거가 준비된다.

## FullSpec 승격 상태

- 상태: FullSpec evidence ready.
- Required families: `PackagingUpgrade`, `DurabilityRecovery`, `Integration`.
- FullSpec evidence:
  - `crates/shacs-core/tests/process_lifecycle.rs`
  - `crates/shacs-cli/tests/runtime_inspect_cli.rs`
  - `crates/shacs-cli/tests/release_candidate_smoke.rs`
  - `crates/shacs-core/tests/verification_matrix.rs`
- 비범위로 남는 항목: OS package manager별 installer, remote rolling upgrade, web installer, fleet management, SaaS update service.
