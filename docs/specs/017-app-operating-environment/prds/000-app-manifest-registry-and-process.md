# PRD 000. app manifest, registry, and process baseline

## 목표

이 문서는 `docs/specs/017-app-operating-environment/SPEC.md`의 하위 실행 문서다. Spec 017이 정의한 장기 제품 계약을 첫 구현 가능한 baseline으로 낮춰, self-hosted 사용자가 config data dir에 local app bundle을 설치하고 상태를 관찰할 수 있는 최소 app operating environment를 고정한다.

이번 PRD의 목표는 app manifest, app registry, lifecycle state, process snapshot, permission/secret request reference, task ledger receipt의 첫 타입과 저장 의미를 정리하는 것이다. 현재 상태는 local app manifest, registry, process projection, task ledger baseline 구현 완료이며, 이 문서는 full AI OS 완성이 아니라 첫 buildable baseline의 closure를 기록한다.

## SPEC 입력

- 주관 spec: `docs/specs/017-app-operating-environment/SPEC.md`
- 선행 기준:
  - `docs/SYSTEM-FOUNDATION.md`
  - `docs/specs/004-tool-runtime/SPEC.md`
  - `docs/specs/005-skill-system/SPEC.md`
  - `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`
  - `docs/specs/012-runtime-services/SPEC.md`
  - `docs/specs/013-user-interfaces-and-session-ux/SPEC.md`
  - `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`
  - `docs/specs/015-packaging-process-lifecycle-and-upgrades/SPEC.md`
  - `docs/specs/016-verification-matrix-and-release-gates/SPEC.md`

## Dependency Cut

- 005는 skill discovery, precedence, parsing, injection 규칙을 소유한다. 이 PRD는 app manifest가 skill path를 참조할 수 있다는 registry 입력만 다룬다.
- 004는 tool registry와 tool execution envelope를 소유한다. 이 PRD는 app이 요구하는 tool 또는 MCP port reference를 기록하지만 실행 envelope를 재정의하지 않는다.
- 010은 permission, secret, host safety, grant 판단을 소유한다. 이 PRD의 permission/secret declaration은 request reference이며 grant가 아니다.
- 012는 service reentry, wake, process boundary, external message normalization을 소유한다. 이 PRD의 `AppProcessSnapshot`은 해당 실행 경계를 읽는 projection이다.
- 013은 app list, process card, approval, settings 같은 UI projection을 소유한다. 이 PRD는 UI가 읽을 registry와 snapshot 의미만 제공한다.
- 014는 diagnostics, receipts, task ledger projection을 소유한다. 이 PRD는 `runtime/app-ledger/` 아래 redacted execution receipt의 최소 record 의미를 정의한다.
- 015는 host runtime lifecycle, packaging safety, install/update/recover gate를 소유한다. 이 PRD의 app install은 host binary install이나 runtime upgrade가 아니다.
- 016은 release evidence와 gate mapping을 소유한다. 이 PRD의 closure evidence는 repo-relative executable evidence로 016의 matrix에 연결해야 한다.

## 범위

- `AppManifest`, `AppId`, `AppBundlePath` 타입 의미 정의
- `<data-dir>/apps/<app-id>.shacsapp/` local bundle convention 고정
- `AppRegistry`와 `AppRegistryEntry`의 최소 저장 필드 정의
- lifecycle state의 최소 집합 정의: `installed`, `enabled`, `disabled`, `unavailable`, `uninstalling`
- app install 시 manifest parse와 validation, app id/version/digest/resource/permission/secret summary 기록
- install은 app code를 실행하지 않고, permission grant나 secret value 주입도 하지 않는다는 admission rule 고정
- permission declaration과 secret declaration을 grant가 아니라 request reference로 저장
- `AppProcessId`와 `AppProcessSnapshot`을 session truth owner가 아닌 process projection으로 정의
- `TaskLedgerEntry`를 `runtime/app-ledger/` 아래 redacted execution receipt로 정의
- uninstall 시 registry entry와 local bundle 제거 의미를 정하되, ledger와 historical session reference는 보존

## 범위 제외

- Rust dynamic plugin ABI
- arbitrary in-process third-party code loading
- remote marketplace, marketplace protocol 세부 설계
- remote package execution 또는 install 중 원격 code 실행
- SaaS 운영 포털, 조직 관리자 승인 workflow, fleet 배포
- 멀티유저 app entitlement 시스템
- OS별 GUI shell, desktop shell, macOS visual cloning
- app별 MCP server 내부 구현
- secret vault 저장 backend 세부 구현
- host binary install, update, recover lifecycle 자체 구현

## 현재 구현 상태

### 구현 완료된 local baseline

- Spec 017의 app operating environment 중 local app bundle baseline은 구현되어 closure 상태다.
- 완료 범위는 `AppManifest`, `AppRegistry`, `AppRegistryEntry`, `AppLifecycleState`, `AppProcessSnapshot`, `TaskLedgerEntry`의 core 타입과 저장 의미다.
- 구현 evidence는 `crates/shacs-core/src/app.rs`, `crates/shacs-core/tests/app_environment.rs`, `crates/shacs-cli/src/lib.rs`의 apps command surface에 있다.
- 이 closure는 local app manifest/registry/process projection/task ledger baseline 완료를 뜻한다. remote marketplace, dynamic plugin ABI, 실제 app process 실행, MCP server 내부, secret vault backend, SaaS/admin/fleet workflow, visual UI design 완료를 뜻하지 않는다.

### 현재 구현 계약

- local app bundle 기본 위치는 config data dir의 `apps/<app-id>.shacsapp/`이며, 기본 config 기준으로는 `~/.shacs-bot/apps/<app-id>.shacsapp/`이다. 현재 공개 설치 표면은 이 위치의 bundle만 registry에 등록한다.
- `manifest.json`은 bundle identity의 진실 원천이다.
- install은 manifest를 검증하고 digest를 기록하지만 app process를 만들지 않는다.
- permission과 secret 선언은 request다. 최종 grant와 secret value 주입은 010의 host safety 경계에서 결정한다.
- process snapshot은 session truth가 아니라 projection이다. session truth와 recovery 판단은 기존 session/runtime owner를 따라야 한다.
- ledger receipt는 raw secret value, hidden reasoning, 불필요한 file contents를 저장하지 않는다.

## 회귀 테스트 기준

1. manifest parse validation: 유효한 `manifest.json`은 `AppManifest`로 파싱되고, 필수 id/version/entry 누락과 bundle 밖 path 참조는 validation 실패로 남긴다.
2. id collision과 digest mismatch: 같은 `AppId`가 이미 등록된 경우 충돌을 진단하고, 같은 id/path의 manifest digest가 registry 기록과 다르면 mismatch로 보고한다.
3. install no auto-run: install path는 registry entry와 digest만 기록하고 app process, MCP device, tool execution, secret injection을 만들지 않는다.
4. enable/disable projection: `enabled`와 `disabled` 전환은 intent/skill/tool exposure 후보 projection에 반영되지만 session truth를 직접 바꾸지 않는다.
5. missing secret unavailable status: required secret request가 충족되지 않은 app은 `unavailable`로 보이고, secret value 없이 request key와 reason만 노출한다.
6. denied permission receipt: permission denial은 app crash나 session corruption이 아니라 redacted `TaskLedgerEntry` receipt로 남는다.
7. uninstall preserving ledger references: uninstall은 registry entry와 bundle 제거를 처리하지만 historical ledger/session reference는 설명 가능한 상태로 보존한다.

## 구현된 범위

### Wave 1. Manifest와 local bundle validation

- `AppId`, `AppBundlePath`, `AppManifest`의 최소 타입을 구현했다.
- `<data-dir>/apps/<app-id>.shacsapp/manifest.json`을 기준으로 bundle identity를 읽는다.
- manifest 필수 필드, bundle 내부 resource path, permission/secret declaration schema를 검증한다.
- digest 계산은 manifest와 등록된 static resource summary를 재현 가능한 방식으로 기록한다.

### Wave 2. Registry와 lifecycle state

- `AppRegistry`와 `AppRegistryEntry`를 runtime root 아래 local index로 둔다.
- entry에는 app id, version, digest, bundle path, lifecycle state, permission request summary, secret request summary, grant reference placeholder를 기록한다.
- install, enable, disable, unavailable, uninstalling 상태 전이를 단위 테스트로 고정한다.
- install 성공 후 기본 상태는 실행이 아니라 등록 상태이며, 자동 enable 여부가 필요하면 명시적 정책으로 분리한다.

### Wave 3. Process snapshot projection

- `AppProcessId`와 `AppProcessSnapshot`을 session/runtime truth에서 파생되는 읽기 모델로 둔다.
- snapshot에는 app id, originating intent, workspace scope, active grant reference, secret handle reference, device/port in-flight reference, status, artifact reference를 담는다.
- process snapshot은 existing session owner를 대체하지 않고, reload나 disable 중에도 이미 시작된 작업을 설명하는 projection으로만 사용한다.

### Wave 4. Redacted task ledger receipt

- `TaskLedgerEntry`를 `runtime/app-ledger/` 아래 redacted execution receipt로 기록한다.
- receipt는 app id, process id, device reference, port reference, grant reference, artifact reference, decision result를 저장한다.
- raw secret value, hidden reasoning, 필요 이상의 file contents는 저장하지 않는다.
- permission denial, unavailable app, uninstall 후 historical reference 조회를 회귀 테스트로 묶는다.

## Verification Evidence

- 구현 evidence는 `crates/shacs-core/src/app.rs`의 core module, `crates/shacs-core/tests/app_environment.rs`의 app environment tests, `crates/shacs-cli/src/lib.rs`의 apps command parser와 command surface다.
- executable evidence는 존재하지 않는 경로가 아니라 Cargo로 실행 가능한 test와 command여야 한다.
- 최소 command evidence는 `cargo test --manifest-path crates/shacs-core/Cargo.toml --test app_environment`와 `cargo test --manifest-path crates/shacs-cli/Cargo.toml`로 연결한다.
- 이 evidence는 local baseline closure만 뒷받침한다. FullSpec 승격이나 전체 AI Operating System 완성 evidence로 쓰지 않는다.

## Open Risks

- registry가 grant owner처럼 커지면 010의 permission/secret 경계가 흐려질 수 있다.
- process snapshot이 session truth로 오해되면 reload, disable, recovery 중 상태 불일치가 생길 수 있다.
- digest 범위를 너무 넓히면 작은 asset 변경이 불필요한 reinstall처럼 보일 수 있고, 너무 좁히면 manifest와 실제 bundle drift를 놓칠 수 있다.
- ledger receipt에 편의를 위해 raw secret이나 hidden reasoning을 넣으면 010과 014의 redaction 계약을 깨뜨린다.

## 종료 상태

아래 기준은 local app manifest/registry/process projection/task ledger baseline의 closure 기준이다. 현재 구현은 이 PRD를 닫지만, Spec 017의 장기 제품 전체를 닫지는 않는다.

- 사용자가 data-dir-local `apps/<app-id>.shacsapp/` bundle을 등록할 때 manifest validation과 digest recording이 실행된다.
- registry entry가 app id, version, digest, bundle path, lifecycle state, request summary, grant reference를 저장한다.
- install은 app code 실행, MCP/device start, tool execution, secret injection, permission grant를 만들지 않는다.
- enabled, disabled, unavailable, uninstalling 상태가 CLI command surface와 diagnostics/read projection에서 같은 의미로 읽힌다.
- process snapshot은 session truth owner가 아니라 app/process 읽기 projection으로만 쓰인다.
- task ledger receipt는 `runtime/app-ledger/` 아래 redacted form으로 남고, uninstall 후에도 historical reference를 설명할 수 있다.
- 016에 연결 가능한 repo-relative executable evidence가 준비된다.
