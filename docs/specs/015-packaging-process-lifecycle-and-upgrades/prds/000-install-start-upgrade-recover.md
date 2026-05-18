# PRD 000. install, start, upgrade, recover

## 목표

이 문서는 `docs/specs/015-packaging-process-lifecycle-and-upgrades/SPEC.md`의 하위 실행 문서다. self-hosted 사용자가 직접 설치, 시작, 중지, 업그레이드, 복구할 수 있는 실제 제품 수명주기를 구현 단위로 내린다.

이번 PRD의 목표는 바이너리 교체가 아니라, runtime root ownership, compatibility 검사, migration gate, interrupted upgrade 방어까지 포함한 self-hosted/local lifecycle baseline을 shipping 가능한 수준으로 고정하는 것이다. 현재 범위는 로컬 수명주기와 admission guard이며, 완전한 stored-data transform migration 제품 표면까지 포함하지 않는다.

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
- SaaS updater

## 현재 구현 상태

### 이미 반영된 self-hosted/local lifecycle baseline

- Cargo 기반 build/install-equivalent 경로와 Dockerfile, Docker Compose 기반 로컬 실행 표면이 있다.
- 공식 `runtime start`, `runtime stop`, `runtime restart`가 있으며 `runtime start`는 기존 channel runtime foreground 경로를 lifecycle admission/ownership 이후 실행한다.
- `run`, `serve`, `runtime start`는 active ownership marker와 heartbeat를 기록하고, stop/restart request marker를 관찰해 종료한다.
- `runtime inspect`는 binary version, data schema version, compatibility classification, ownership status, stop request marker, update marker, capabilities, sessions를 보고한다.
- `runtime update`는 source-install workflow에서 target version이 실행 중인 binary version과 일치해야 하며, update marker를 `in_progress`에서 `completed_cleanup`으로 남긴다.
- 현재 compatibility/admission은 `RUNTIME_DATA_SCHEMA_VERSION` 같은 binary constants와 marker state를 근거로 한다. migration evidence는 current schema에서 `migration_required=false`인 no-op completion이다. 이는 현재 schema에서 runtime data transform이 필요 없다는 증거이며, 저장된 데이터를 실제 변환하는 migration framework가 검증됐다는 뜻은 아니다.
- migration-required/inspect-only/incompatible/partial marker 상태는 running/mutation admission을 차단한다.
- `runtime recover`는 partial migration과 active ownership을 차단하고, partial migration이 아닌 update marker와 stale ownership marker를 정리한다.
- diagnostics/recovery evidence는 update marker와 recovery 결과를 설명하는 수준까지 확인된다.

### 비범위

- OS package manager별 installer, 원격 rolling upgrade, 웹 installer, fleet management, SaaS updater

### 로컬 근거

- `crates/shacs-cli/src/lib.rs`의 runtime parser, inspect/update/recover, ownership active/stale, active conflict, stale recover cleanup, stop/restart request, compatibility/admission 단위 테스트
- `crates/shacs-config/src/lib.rs`의 config context/runtime directory/migration 단위 테스트
- `docs/USAGE.md`와 `README.md`의 source/Cargo 기반 install-update-recover 사용자 절차

## TDD 상태와 남은 coverage

1. compatibility 결과 분류와 start 허용 결정표 단위 테스트는 현재 admission guard evidence에 포함된다.
2. ownership marker 생성, active marker 충돌, active/stale classification, stale marker 정리 테스트는 현재 evidence에 포함된다.
3. `runtime stop`/`runtime restart` request marker 기록과 장기 실행 shutdown predicate 관찰은 현재 evidence에 포함된다.
4. migration-required, interrupted update marker, partial migration, inspect-only, incompatible 상태의 admission block은 현재 evidence에 포함된다.
5. install 후 first start, safe stop, clean restart, 실제 stored-data transform migration, recover 이후 start 가능 여부의 end-to-end coverage는 남은 coverage다.

## 구현 웨이브

### Wave 1. Packaging metadata와 bootstrap guard 구현

- 현재 구현은 binary version과 runtime data schema version을 식별 가능한 형식으로 제공한다.
- start 진입 전 runtime root, config, secrets, ownership marker, compatibility 상태를 검사한다.
- active ownership marker가 유효하면 중복 start를 막는다.

### Wave 2. Lifecycle 제어 구현

- 현재 구현은 `runtime start`, `runtime stop`, `runtime restart` 절차를 명시적 CLI 표면으로 제공한다.
- stop/restart는 active ownership marker를 직접 삭제하지 않고 stop-request marker를 남기며, active owner 또는 stale owner 상태를 보고한다.
- CLI와 inspect surface에서 현재 lifecycle 상태를 읽을 수 있게 한다.

### Wave 3. Compatibility와 migration gate 구현

- 현재 구현은 binary constants와 marker state 기반으로 fully compatible, migration required, inspect-only, incompatible admission을 분류한다.
- current schema에서는 `migration_required=false` no-op completion과 partial marker 차단을 기록한다.
- 실제 stored-data transform migration은 별도 제품 표면으로 남아 있으며, migration 완료 전 running 상태 진입 금지는 admission guard로 유지한다.

### Wave 4. Interrupted upgrade와 recover 상태

- update marker, partial migration marker, stale ownership marker 감지는 현재 evidence에 포함된다.
- inspect/recover surface는 update marker와 recovery 결과를 설명하고, `completed_cleanup` 상태를 남긴다.
- interrupted update 또는 partial migration 이후 mutation admission은 막는다.
- install, update, recover, restart 경로를 묶는 end-to-end 회귀 테스트는 남은 coverage다.

## Verification Evidence

### 현재 확인된 evidence

- `runtime inspect`가 binary version, data schema version, update marker, capabilities, sessions를 보고하는 테스트
- `runtime diagnostics`와 `runtime recover`가 update marker와 recovery 결과를 설명하는 테스트
- `runtime update --target-version ...`이 실행 중인 binary version과 같은 target version만 허용하고, `in_progress`에서 `completed_cleanup` marker로 이어지는 테스트
- partial migration marker가 `runtime recover`에서 차단되는 테스트
- no-op migration completion이 `migration_required=false` baseline으로 남는 테스트
- Dockerfile release CLI binary build와 Docker Compose `shacs-gateway`, `shacs-api`, one-shot `shacs-cli` 서비스 구성

### Baseline evidence

- CLI `runtime start` parser와 lifecycle admission/ownership 경로
- active ownership conflict, active/stale ownership classification, stale-only `runtime recover` cleanup
- `runtime stop`/`runtime restart` request marker write와 장기 실행 shutdown predicate 관찰
- partial migration, migration-required, inspect-only, incompatible 상태의 start/mutation admission block
- current schema no-op migration completion과 interrupted update marker recovery. 이는 `migration_required=false` current schema 증거이며, 실제 stored-data transform migration 증거는 아니다.

## Open Risks

- marker 정리 순서가 잘못되면 중단 후 false healthy 상태로 보일 수 있다. 현재 구현은 active ownership을 stop 명령에서 직접 삭제하지 않고 owner/recover가 정리하도록 제한한다.
- migration이 in-place overwrite에 기대면 rollback과 recovery 설명이 어려워질 수 있다. 현재 schema는 data-transform migration이 필요 없어 no-op completion만 제공한다.

## 종료 기준

### self-hosted/personal-use local lifecycle baseline 종료 상태

- 사용자가 로컬에서 Cargo build/install-equivalent artifact와 Docker 기반 실행 표면을 확인할 수 있다.
- 공식 `runtime start`, `runtime stop`, `runtime restart`와 기존 `run`, `serve`가 ownership/admission guard 아래 동작한다.
- `runtime inspect`, `runtime diagnostics`, `runtime update`, `runtime recover`가 compatibility, ownership, update marker와 recovery evidence를 다룬다.
- update marker flow는 no-op migration completion 기준으로 `completed_cleanup`까지 확인된다.

## Spec 015 승격 상태

- 상태: self-hosted/personal-use local lifecycle baseline complete. 전체 FullSpec의 stored-data transform migration과 더 넓은 upgrade 제품 표면은 complete로 주장하지 않는다.
- 대상 families: `PackagingUpgrade`, `DurabilityRecovery`, `Integration`.
- 현재 evidence:
  - `crates/shacs-cli/src/lib.rs` inline runtime lifecycle, ownership, stop/restart, compatibility, inspect/update/recover tests
  - `crates/shacs-config/src/lib.rs` inline runtime layout/config migration tests
  - Dockerfile, Docker Compose 로컬 실행 구성
- 남은 coverage: install/update/recover/restart end-to-end 회귀, 실제 stored-data transform migration 경로.
- 남은 비범위 항목: OS package manager별 installer, remote rolling upgrade, web installer, fleet management, SaaS update service.
