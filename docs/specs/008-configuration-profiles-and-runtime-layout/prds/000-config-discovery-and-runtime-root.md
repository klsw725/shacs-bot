# PRD 000. config discovery and runtime root

## 목표

이 문서는 `docs/specs/008-configuration-profiles-and-runtime-layout/SPEC.md`의 하위 실행 문서다. 현재 목표는 지금 구현된 configuration discovery와 runtime root 경계를 current architecture에 맞춰 문서화하고, 2026-05-15 기준 Spec 008을 그 범위에서 완료로 닫는 것이다.

- 현재 `.shacs-bot/config.json`과 `.shacs-bot/auth.json` 경계를 정확히 설명한다.
- JSON config loading, env placeholder resolution, auth store, path helper, runtime dir 생성 범위를 current architecture로 고정한다.
- formal TOML layered config, split profile type, formal runtime layout은 future work로 남긴다.
- 이 완료가 formal TOML layered config/profile/runtime layout 구현 완료를 뜻하지 않음을 명시한다.

## SPEC 입력

- 주관 spec: `docs/specs/008-configuration-profiles-and-runtime-layout/SPEC.md`
- 교차 의존:
  - `docs/specs/003-provider-runtime/SPEC.md`
  - `docs/specs/005-skill-system/SPEC.md`
  - `docs/specs/006-session-store/SPEC.md`
  - `docs/specs/007-main-orchestrator-policy/SPEC.md`

## Dependency Cut

이 PRD는 현재 설정 탐색, auth 저장, runtime path helper, runtime dir 생성, skill discovery 증거를 문서화 대상으로 삼는다. GUI config editor, remote config sync, cloud secret manager, multi-user RBAC/admin workflow, SaaS control plane, cluster layout은 범위 밖이다.

기준은 self-hosted, personal-use 사용자가 로컬 파일과 디렉터리만 보고 상태를 이해할 수 있는 것이다.

## 범위

- `.shacs-bot/config.json` 중심의 현재 JSON config 경로
- `.shacs-bot/auth.json` 중심의 현재 auth store 경로
- `load_config_with_env`, `load_config_or_default`의 현재 로딩 경계
- `resolve_config_env_refs`, `interpolate_env_with_source`의 환경 변수 placeholder 해석
- `migrate_config_value`의 legacy key migration
- `ProviderConfig`의 `api_key`, `api_base`, `extra_headers`, `extra_body` 필드
- `AuthStore`, `ProviderAuth` 인증 저장 경계
- `default_config_path`, `shacs_home_dir`, `get_config_path`, `get_data_dir`, `get_runtime_subdir`, `get_media_dir`, `get_cron_dir`, `get_logs_dir`, `ensure_runtime_dirs` path helper
- `workspace`, `history/cli_history`, `bridge`, `sessions`, `media`, `cron`, `logs`, `channels`, `skills` runtime dir 매핑
- `ContextBuilder`와 `shacs-skills` 기반 skill root, context, discovery 증거
- Spec 008 current architecture mapping 기준 완료 선언

## 범위 제외

- `.shacs/config.toml`과 `.shacs/secrets.toml` layered discovery 구현 완료 선언
- `ConfigSnapshot`, `SecretsSnapshot`, `ProviderProfile`, `PermissionProfile`, `RuntimeProfile` split type 구현 완료 선언
- `api_key_ref` secret-reference model 구현 완료 선언
- `schema_version` validator와 future schema rejection 구현 완료 선언
- formal `.shacs/runtime/{artifacts,sessions,checkpoints,app-ledger,logs,cache,tmp}` layout 구현 완료 선언
- deep source-origin provenance 구현 완료 선언
- runtime migration entrypoint 구현 완료 선언
- broad provider/auth-family expansion

## 현재 구현 상태

### 이미 반영된 것

- `crates/shacs-config/src/lib.rs`는 `.shacs-bot/config.json`과 `.shacs-bot/auth.json`을 현재 기본 경계로 사용한다.
- 현재 config 로딩은 JSON 기반이다.
- 환경 변수 placeholder는 `resolve_config_env_refs`와 `interpolate_env_with_source` 경계에서 재귀적으로 해석되고, 빠진 환경 변수는 진단된다.
- legacy config key는 `migrate_config_value` 경계에서 현재 shape로 보정된다.
- path helper는 active config context에 따라 config path, data dir, runtime subdir, media dir, cron dir, logs dir을 계산한다.
- `ensure_runtime_dirs`는 현재 runtime directory set을 생성한다.
- 현재 runtime dir 증거는 `workspace`, `history/cli_history`, `bridge`, `sessions`, `media`, `cron`, `logs`, `channels`, `skills`다.
- 현재 provider config는 `ProviderConfig`의 `api_key`, `api_base`, `extra_headers`, `extra_body`를 사용한다.
- 현재 auth 저장은 `AuthStore`, `ProviderAuth` 경계로 설명한다.
- `ContextBuilder`와 `shacs-skills`는 skill root, context, discovery 증거를 제공한다.

### 아직 남은 것

- `.shacs/config.toml`과 `.shacs/secrets.toml`의 layered discovery는 future work다.
- formal `ConfigSnapshot`, `SecretsSnapshot`, `ProviderProfile`, `PermissionProfile`, `RuntimeProfile` 분리는 future work다.
- `api_key_ref` secret-reference model은 future work다.
- `schema_version` validator와 unsupported future schema rejection은 future work다.
- formal `.shacs/runtime/{artifacts,sessions,checkpoints,app-ledger,logs,cache,tmp}` layout은 future work다.
- deep source-origin provenance와 runtime migration entrypoint는 future work다.

위 항목은 Spec 008의 current architecture mapping 기준 완료를 막지 않는다. 다만 formal TOML/profile/runtime layout 구현 완료를 주장하려면 별도 구현과 검증 범위를 다시 잡아야 한다.

### 로컬 근거

- `crates/shacs-config/src/lib.rs`
- `crates/shacs-core`의 runtime context와 provider adapter 관련 테스트
- `crates/shacs-core`의 runtime recovery와 session management 관련 테스트
- `ContextBuilder`
- `shacs-skills`

## TDD 계획

이 PRD의 현재 단계는 새 구현을 요구하지 않는다. 기존 테스트 증거와 `ensure_runtime_dirs_creates_current_layout_contract`를 current architecture mapping 완료 증거에 연결한다.

1. JSON 기본 경로와 provider 기본값이 `.shacs-bot` 규약을 따르는지 확인한다.
2. config 저장, auth 저장, runtime context roundtrip이 깨지지 않는지 확인한다.
3. public path helper가 active config context를 따르는지 확인한다.
4. 환경 변수 placeholder가 재귀 해석되고 missing env를 진단하는지 확인한다.
5. legacy migration writeback이 env template과 workspace override를 잘못 덮어쓰지 않는지 확인한다.
6. provider adapter가 resolved model과 config defaults를 사용하는지 확인한다.
7. status 표면이 config workspace와 provider fields를 보고하는지 확인한다.
8. runtime recovery와 session management command가 현재 runtime 경계와 충돌하지 않는지 확인한다.
9. `ensure_runtime_dirs`가 현재 runtime directory layout 계약을 생성하는지 확인한다.

## 구현 웨이브

### Wave 1. 현재 경로와 파일 형식 문서화

- `.shacs-bot/config.json`과 `.shacs-bot/auth.json`을 현재 구현 경계로 고정한다.
- `.shacs/config.toml`과 `.shacs/secrets.toml`은 future work로 내린다.
- JSON config loading과 auth store를 current architecture로 설명한다.

### Wave 2. 현재 config loading과 migration 매핑

- `load_config_with_env`, `load_config_or_default`를 현재 loading 경계로 연결한다.
- `resolve_config_env_refs`, `interpolate_env_with_source`를 env placeholder 해석 증거로 연결한다.
- `migrate_config_value`를 legacy migration 증거로 연결하되, formal schema migration으로 과장하지 않는다.

### Wave 3. provider/auth shape 정리

- 현재 provider config 필드를 `api_key`, `api_base`, `extra_headers`, `extra_body`로 적는다.
- `AuthStore`, `ProviderAuth`를 현재 auth 저장 경계로 적는다.
- `api_key_ref`는 future secret-reference model로 둔다.

### Wave 4. runtime dir와 skill discovery 매핑

- 현재 path helper와 `ensure_runtime_dirs`를 runtime dir 증거로 연결한다.
- `workspace`, `history/cli_history`, `bridge`, `sessions`, `media`, `cron`, `logs`, `channels`, `skills`를 현재 layout 이름으로 적는다.
- `ensure_runtime_dirs_creates_current_layout_contract`를 data dir, workspace override, media, cron, logs, channels, `channels/worker-metadata`, skills 생성 증거로 적는다.
- `ContextBuilder`와 `shacs-skills`를 skill root, context, discovery 증거로 연결한다.

### Wave 5. future work와 exit criteria 분리

- formal profile split, TOML layered discovery, formal runtime layout, schema validator, deep provenance, runtime migration entrypoint를 future work로 유지한다.
- current architecture mapping 기준으로 Spec 008을 완료로 닫는다.
- 이 완료가 formal TOML/profile/runtime layout 구현 완료가 아님을 명시한다.
- out-of-scope 항목이 self-hosted, personal-use framing을 벗어나지 않게 정리한다.

## Verification Evidence

- `defaults_use_shacs_paths_and_nanobot_provider_values`
- `load_save_refresh_and_runtime_context_roundtrip`
- `public_path_helpers_follow_active_config_context`
- `ensure_runtime_dirs_creates_current_layout_contract`
- `resolves_env_refs_recursively_and_reports_missing`
- `migration_writeback_preserves_env_templates_and_does_not_persist_workspace_override`
- `runtime_config_migration_writeback_preserves_env_placeholders`
- `migrates_legacy_keys_without_overriding_new_values`
- `provider_adapter_uses_resolved_model_and_config_defaults`
- `status_reports_config_workspace_and_provider_fields`
- `runtime_recover_blocks_partial_migration_marker`
- `session_management_commands_cover_history_export_clear_diagnostics_and_compact`

이 증거는 현재 JSON config, env placeholder, path helper, auth/runtime context, provider defaults, status reporting, recovery, session command 표면을 뒷받침한다. `ensure_runtime_dirs_creates_current_layout_contract`는 현재 runtime directory layout의 생성 계약을 뒷받침한다. TOML layered discovery, `api_key_ref`, split profile type, formal runtime layout 검증으로 해석하지 않는다.

## Open Risks

- 문서가 다시 TOML layered config를 현재 완료처럼 쓰면 실제 코드와 기대가 어긋난다.
- Spec 008 완료를 formal TOML/profile/runtime layout 구현 완료로 읽으면 남은 future work 범위가 가려진다.
- `api_key_ref`를 현재 구현처럼 쓰면 provider config와 auth store의 실제 경계가 흐려진다.
- 현재 runtime dir와 formal `.shacs/runtime` layout을 섞어 쓰면 사용자 복구 경로가 불명확해진다.
- deep provenance와 schema validator가 없다는 점을 감추면 future migration 작업의 범위를 잘못 잡을 수 있다.

## 종료 기준

- SPEC과 PRD가 현재 경로를 `.shacs-bot/config.json`, `.shacs-bot/auth.json`으로 설명한다.
- SPEC과 PRD가 current provider config를 `api_key`, `api_base`, `extra_headers`, `extra_body`로 설명한다.
- SPEC과 PRD가 현재 runtime dir를 `workspace`, `history/cli_history`, `bridge`, `sessions`, `media`, `cron`, `logs`, `channels`, `skills`로 설명한다.
- SPEC과 PRD가 `ensure_runtime_dirs_creates_current_layout_contract`를 현재 runtime dir 생성 증거로 포함한다.
- SPEC과 PRD가 `ContextBuilder`와 `shacs-skills`를 현재 skill discovery 증거로 연결한다.
- SPEC과 PRD가 TOML layered discovery, split profile type, `api_key_ref`, schema validator, formal runtime layout, deep provenance, runtime migration entrypoint를 future work로 둔다.
- SPEC과 PRD가 Spec 008을 current architecture mapping 기준 완료로 닫는다.
- SPEC과 PRD가 formal TOML layered config/profile/runtime layout 구현 완료를 주장하지 않는다.
- 문서는 self-hosted, personal-use framing을 유지하고 admin/operator organization workflows를 도입하지 않는다.
