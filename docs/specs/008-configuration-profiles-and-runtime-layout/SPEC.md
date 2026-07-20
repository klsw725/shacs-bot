# configuration profiles and runtime layout 아키텍처 명세

Status: Complete (Scoped)
Implemented scope: `.shacs-bot/config.json`, `.shacs-bot/auth.json`, JSON config loader, env placeholder resolution, auth store, path helper, current runtime dirs, skill discovery 연결을 current configuration/runtime layout으로 닫았다.
Open work moved to: [035 configuration runtime layout and execution snapshots](../035-configuration-runtime-layout-and-execution-snapshots/SPEC.md)
Not carried forward: GUI config editor, remote config sync, cloud secret manager, multi-user RBAC, cluster runtime layout, marketplace profile distribution을 후속 owner 범위로 가져가지 않는다.

## 문서 목적

이 문서는 `shacs-bot`의 현재 설정, 인증 저장, runtime 디렉터리, skill discovery 경계를 코드 구조에 맞춰 정리한다. 이전 초안은 `.shacs/config.toml`, `.shacs/secrets.toml`, formal profile 타입, formal runtime layout이 이미 구현 계약인 것처럼 읽혔다. 이 문서는 그 관점을 고친다.

2026-05-15 기준 Spec 008은 current architecture mapping 범위에서 완료로 닫는다. 이 문서의 목적은 지금 구현된 `.shacs-bot/config.json`, `.shacs-bot/auth.json`, JSON 설정 로딩, 환경 변수 placeholder 해석, auth store, path helper, runtime dir 생성, skill root discovery를 완료 증거로 묶고, TOML layered config/profile/runtime-layout 시스템은 future work로 분리하는 것이다.

목표는 다음과 같다.

- 현재 구현된 설정 경로와 runtime 경계를 과장 없이 고정한다.
- current architecture mapping으로 완료 인정할 수 있는 범위를 정의한다.
- future config/profile/runtime-layout 작업을 현재 완료 blocker처럼 다루지 않는다.
- self-hosted, personal-use 단일 사용자 런타임이라는 전제를 유지한다.

이 문서는 사용자가 직접 설치, 설정, 실행, 복구하는 로컬 assistant runtime을 기준으로 한다. 조직 운영자 console, SaaS control plane, cluster layout은 현재 범위가 아니다.

---

## 현재 상태 판정

2026-05-15 기준 Spec 008은 current architecture mapping 범위에서 완료로 닫는다. 이 완료는 지금 구현된 JSON config, auth store, path helper, runtime dirs, skill discovery 경계에 대한 완료다. formal TOML layered config, formal profile split, formal runtime layout, deep provenance, runtime migration entrypoint 구현 완료를 뜻하지 않는다.

현재 구현으로 인정하는 범위는 다음이다.

- `crates/shacs-config/src/lib.rs`는 기본 설정을 `.shacs-bot/config.json`에서 읽고 인증 저장을 `.shacs-bot/auth.json`에서 다룬다.
- 현재 config 로딩은 JSON 기반이며 `.shacs/config.toml`, `.shacs/secrets.toml` layered discovery가 아니다.
- `default_config_path`, `shacs_home_dir`, `get_config_path`, `get_data_dir`, `get_runtime_subdir`, `get_media_dir`, `get_cron_dir`, `get_logs_dir`, `ensure_runtime_dirs`가 현재 path helper와 runtime dir 생성 경계다.
- `load_config_with_env`, `load_config_or_default`, `resolve_config_env_refs`, `interpolate_env_with_source`, `migrate_config_value`가 현재 JSON config 로딩, 환경 변수 placeholder 해석, legacy key migration 경계다.
- `AuthStore`, `ProviderAuth`가 현재 인증 저장 경계다.
- 현재 provider 설정은 `ProviderConfig`의 `api_key`, `api_base`, `extra_headers`, `extra_body`를 사용한다. `api_key_ref` secret-reference model은 구현됐다고 말하지 않는다.
- 현재 runtime layout 증거는 `workspace`, `history/cli_history`, `bridge`, `sessions`, `media`, `cron`, `logs`, `channels`, `skills` 경계다.
- `ContextBuilder`와 `shacs-skills`는 현재 skill root, context building, skill discovery의 증거다.

이 범위가 current architecture mapping이다. 이것은 formal configuration profile system 완성이 아니다.

---

## 현재 범위

이 문서는 다음을 설명한다.

- `.shacs-bot/config.json`과 `.shacs-bot/auth.json` 중심의 현재 설정 저장 경계
- JSON config 로딩, 환경 변수 placeholder 해석, legacy key migration
- 현재 provider config shape와 auth store 경계
- 현재 runtime path helper와 runtime dir 생성 범위
- 현재 skill root, context, discovery 증거
- current architecture mapping 기준 완료와 future work의 분리

이 문서는 다음을 현재 구현 완료로 주장하지 않는다.

- `.shacs/config.toml`과 `.shacs/secrets.toml` layered discovery
- `ConfigSnapshot`, `SecretsSnapshot`, `ProviderProfile`, `PermissionProfile`, `RuntimeProfile` split type
- `api_key_ref` secret-reference model
- `schema_version` validator와 future schema rejection
- formal `.shacs/runtime/{artifacts,sessions,checkpoints,app-ledger,logs,cache,tmp}` layout
- deep source-origin provenance
- runtime migration entrypoint

---

## 상위 기준과의 관계

이 문서는 다음 상위 기준을 전제로 한다.

- `shacs-bot`은 self-hosted, personal-use 성격의 Rust assistant runtime이다.
- 사용자는 직접 설정 파일과 런타임 데이터를 확인하고 복구할 수 있어야 한다.
- provider runtime, session store, skill system, orchestrator policy는 각자 현재 구현 경계를 가진다.
- 설정은 실행 상태와 섞이면 안 된다.

따라서 current architecture mapping은 enterprise config platform이 아니다. 기본 주체는 사용자 본인이다.

---

## 현재 구현 매핑

### 설정 저장 경계

현재 기본 설정 경로는 `.shacs-bot/config.json`이다. `default_config_path`와 `shacs_home_dir`는 이 경로 규약의 현재 증거다. `get_config_path`는 active config context에서 설정 파일 위치를 제공한다.

현재 config는 JSON 값으로 로드된다. `load_config_or_default`는 파일이 없을 때 기본값으로 진행하는 경계고, `load_config_with_env`는 환경 변수 해석을 포함한 로딩 경계다. `resolve_config_env_refs`와 `interpolate_env_with_source`는 `${ENV}` 형태의 placeholder를 실제 환경에서 해석하고, 빠진 값은 진단 가능한 오류로 다룬다.

`migrate_config_value`는 legacy key를 현재 JSON shape로 보정하는 현재 migration 경계다. 이것은 formal `schema_version` validator나 future schema rejection 구현과 같지 않다.

### 인증 저장 경계

현재 인증 저장 경로는 `.shacs-bot/auth.json`이다. `AuthStore`와 `ProviderAuth`가 provider 인증 material의 현재 저장 경계다.

현재 문서는 secrets가 일반 config와 분리돼야 한다는 방향을 유지한다. 다만 현재 구현은 `.shacs/secrets.toml` snapshot을 읽는 구조가 아니다. `api_key_ref`로 secret을 참조한다고 쓰면 현재 코드보다 앞서간 표현이다.

### provider config shape

현재 provider 설정은 `ProviderConfig`에 모인다. 현재 필드는 `api_key`, `api_base`, `extra_headers`, `extra_body`다.

따라서 current architecture에서 provider secret 처리는 다음처럼 표현한다.

- 현재 코드에는 직접 `api_key` 필드가 있다.
- auth material은 `AuthStore`와 `ProviderAuth` 경계가 있다.
- 환경 변수 placeholder를 통해 config 파일에 원문 secret을 덜 남기는 사용 경로가 있다.
- `api_key_ref`는 future secret-reference model이다.

### runtime path와 디렉터리

현재 runtime 경계는 formal `.shacs/runtime/{artifacts,sessions,checkpoints,app-ledger,logs,cache,tmp}`가 아니다. 현재 증거는 path helper와 실제 디렉터리 이름이다.

- `get_data_dir`
- `get_runtime_subdir`
- `get_media_dir`
- `get_cron_dir`
- `get_logs_dir`
- `ensure_runtime_dirs`

현재 runtime layout로 매핑할 수 있는 이름은 다음이다.

- `workspace`
- `history/cli_history`
- `bridge`
- `sessions`
- `media`
- `cron`
- `logs`
- `channels`
- `skills`

이 디렉터리들은 현재 self-hosted 단일 사용자 실행에서 세션, 로그, media, cron, channel, skill 관련 데이터를 예측 가능한 위치에 두기 위한 경계다. formal artifact, checkpoint, app-ledger, cache, tmp 분리는 future work다.

### skill discovery와 context

`ContextBuilder`와 `shacs-skills`는 현재 skill root와 discovery의 증거다. current architecture mapping에서는 skill path가 config와 runtime context에 연결된다는 점까지 인정한다.

다만 marketplace profile distribution, remote skill registry, 조직 단위 profile 배포는 현재 범위가 아니다.

---

## current architecture mapping 기준

Spec 008에서 current architecture mapping으로 인정하는 조건은 다음이다.

- 문서가 현재 파일 형식을 JSON으로 말한다.
- 문서가 현재 경로를 `.shacs-bot/config.json`과 `.shacs-bot/auth.json`으로 말한다.
- 문서가 현재 runtime dir 이름을 실제 path helper와 맞춰 말한다.
- 문서가 `ProviderConfig`의 현재 필드를 `api_key`, `api_base`, `extra_headers`, `extra_body`로 말한다.
- 문서가 환경 변수 placeholder 해석과 legacy migration을 현재 구현으로 말한다.
- 문서가 `api_key_ref`, split profile type, formal TOML layered discovery를 future work로 둔다.
- 문서가 current tests를 현재 증거로 연결한다.

이 기준을 충족했으므로 Spec 008은 current architecture mapping 범위에서 완료로 닫는다. future work의 formal config/profile/runtime-layout은 별도 구현 범위가 잡힐 때 다시 판단한다.

---

## future config/profile/runtime-layout work

아래 항목은 future work다. 현재 완료 blocker로 쓰지 않는다.

- `.shacs/config.toml`과 `.shacs/secrets.toml` layered discovery
- built-in, user-global, workspace-local, explicit override를 formal layer로 조립하는 `ConfigSnapshot`과 `SecretsSnapshot`
- `ProviderProfile`, `PermissionProfile`, `RuntimeProfile` split type
- `api_key_ref` secret-reference model
- provider invocation 직전 secret resolution과 redaction된 snapshot
- `schema_version` validator
- unsupported future schema rejection
- formal `.shacs/runtime/{artifacts,sessions,checkpoints,app-ledger,logs,cache,tmp}` layout
- artifact, checkpoint, app-ledger, cache, tmp 책임 분리
- deep source-origin provenance
- runtime migration entrypoint

이 항목들은 여전히 유용한 방향이다. 다만 현재 코드가 이미 이 구조를 완성했다고 쓰면 안 된다.

---

## 현재 검증 증거

현재 구현 매핑은 아래 테스트 이름들과 연결된다.

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

이 테스트들은 현재 JSON config, auth/runtime context, path helper, env placeholder, migration writeback, provider defaults, status reporting, runtime recovery, session management 표면의 증거다. `ensure_runtime_dirs_creates_current_layout_contract`는 data dir, workspace override, media, cron, logs, channels, `channels/worker-metadata`, skills 생성 계약을 현재 runtime layout 증거로 고정한다. TOML layered config와 formal profile split을 검증하는 증거로 해석하지 않는다.

---

## 명시적 비범위

이 문서는 다음을 정의하지 않는다.

- GUI config editor
- remote config sync
- cloud secret manager
- multi-user RBAC와 admin workflow
- SaaS control plane
- cluster 또는 multi-node shared layout
- marketplace profile distribution
- broad provider/auth-family expansion

이 항목들은 self-hosted, personal-use 기본 전제를 흐리기 쉽다. 필요하면 별도 문서에서 다룬다.

---

## 결론

Spec 008은 2026-05-15 기준 current architecture mapping 범위에서 완료로 닫는다. 이 완료는 `.shacs-bot/config.json`, `.shacs-bot/auth.json`, JSON config loader, auth store, path helper, runtime dirs, skill discovery를 정확히 설명하고 검증 증거와 연결했다는 뜻이다. 미래 TOML/profile/runtime layout을 이미 끝난 구현 계약으로 요구하지 않는다.

남은 formal config/profile/runtime-layout 작업은 future work다. 다음 구현 작업은 현재 구조를 과장하지 않고, 사용자가 직접 운영하는 로컬 assistant runtime의 설정과 상태 경계를 이 완료선 위에서 발전시킨다.
