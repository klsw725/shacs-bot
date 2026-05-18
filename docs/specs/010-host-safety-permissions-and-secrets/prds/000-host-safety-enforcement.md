# PRD 000. host safety enforcement

## 목표

이 PRD는 `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`의 현재 아키텍처 매핑을 실행 계획으로 정리한다. 목표는 현재 구현된 filesystem, process, network, secret, redaction, MCP capability registration 관련 safety guard를 정확히 문서화하고, Spec 010을 `self-hosted/local baseline` 기준으로 닫는 것이다. 아직 남은 formal host safety 작업은 별도 future work로 고정한다.

이번 PRD는 Spec 010의 local baseline 완료 선언이다. 현재 코드는 guard-denied execution, default-deny MCP 등록, oversized tool result redaction을 포함한 로컬 개인용 baseline을 충족한다. 다만 formal `SafetySnapshot`, `PermissionMode`, approval engine, unified redaction pipeline이 완성된 상태는 아니다.

## SPEC 입력

1. 주관 spec: `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`
2. 선행 기준: `docs/specs/001-session-kernel/SPEC.md`, `docs/specs/004-tool-runtime/SPEC.md`, `docs/specs/007-main-orchestrator-policy/SPEC.md`, `docs/specs/008-configuration-profiles-and-runtime-layout/SPEC.md`, `docs/specs/009-context-assembly-and-compaction-input/SPEC.md`
3. 교차 의존: `docs/specs/011-subagent-runtime/SPEC.md`, `docs/specs/012-runtime-services/SPEC.md`, `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`, `docs/specs/016-verification-matrix-and-release-gates/SPEC.md`

## Dependency Cut

현재 dependency cut은 formal approval gate가 아니라 분산 guard 기준으로 잡는다.

1. 004 tool runtime은 filesystem, shell, web tool 안에서 workspace, symlink, timeout, deny pattern, network guard를 집행한다.
2. 008 config와 CLI auth surface는 provider auth, env placeholder, token import, login, auth overlay를 통해 raw secret 저장을 피한다.
3. 009 context와 compaction 계층에는 아직 universal redaction pipeline이 완성되지 않았다.
4. 011 subagent runtime은 `SubagentExecutionConfig`와 tool registry 구성으로 일부 실행 제한을 상속하지만, inherited `SafetySnapshot`은 아니다.
5. 014 diagnostics와 inspect surface는 SelfTool redaction, transport, email error redaction, session diagnostics/export surface에 의해 일부 보호된다.

## 범위

이번 PRD의 현재 범위는 다음이다.

1. 현재 구현된 filesystem path boundary와 symlink hardening 정리.
2. 현재 구현된 shell exec guard, timeout, workspace, env allowlist 정리.
3. 현재 구현된 network/web SSRF, private, internal URL guard 정리.
4. 현재 구현된 auth store, token import, env placeholder, CLI redaction surface 정리.
5. 현재 구현된 SelfTool redaction과 blocking 정리.
6. 현재 구현된 oversized tool result redaction, persistence, symlink hardening 정리.
7. 현재 구현된 MCP tools/resources/prompts default-deny registration 정리.
8. subagent 실행 제한을 partial inherited restriction으로 정리.

## 범위 제외

다음은 현재 구현 완료로 쓰지 않는다.

1. formal capability evaluator.
2. `plan`, `default`, `auto` permission mode decision table.
3. effect level approval request와 response correlation.
4. stale 또는 expired approval rejection.
5. formal denied execution standard outcome.
6. inherited `SafetySnapshot`.
7. unified redaction pipeline.
8. structured argv process envelope.
9. browser automation 또는 web browsing permission control tests.

다음은 제품 범위 밖이다.

1. 관리자 승인 체계.
2. 중앙 secret vault.
3. 조직 단위 정책 배포.
4. 원격 운영 콘솔.
5. multi user RBAC.
6. distributed trust negotiation.
7. corporate EDR 또는 anti virus integration.

## 현재 구현 상태

### 이미 반영된 것

1. `crates/shacs-core/src/tools/filesystem.rs`는 `PathContext`, `resolve_path`, `resolve_creatable_path`로 workspace 내부 path를 확인하고 symlink escape를 막는다.
2. `crates/shacs-core/src/tools/shell.rs`는 `ExecConfig`, `ExecTool`, `resolve_working_dir`, `guard_command`, `default_deny_patterns`로 workspace, timeout, env allowlist, deny pattern을 적용한다.
3. 현재 exec는 shell string 실행이다. structured argv envelope는 아직 없다.
4. `crates/shacs-security/src/lib.rs`는 `NetworkGuard`, `validate_url_target`, `contains_internal_url`로 SSRF와 private/internal URL을 막는다.
5. `crates/shacs-core/src/tools/web.rs`는 web fetch/search에서 network guard를 사용한다.
6. `crates/shacs-config/src/lib.rs`는 `AuthStore`, `ProviderAuth`, env placeholder 처리를 제공한다.
7. `crates/shacs-cli/src/lib.rs`는 Codex/Copilot token import, login, auth overlay, transport/email error redaction, session diagnostics/export surface를 제공한다.
8. `crates/shacs-core/src/tools/self_tool.rs`는 `SelfTool`, `SENSITIVE_NAMES`, `redact_object`, `redact_value`, `is_sensitive`, `is_blocked`, `is_read_only`로 inspect/self tool redaction과 blocking을 수행한다.
9. `crates/shacs-utils/src/tool_results.rs`는 oversized tool result를 파일과 반환 reference에 남기기 전에 redaction하고 symlink hardening을 제공한다.
10. `crates/shacs-config/src/lib.rs`와 `crates/shacs-core/src/tools/mcp.rs`는 MCP tools, resources, prompts를 empty default로 두고, `*`, raw capability name, wrapped capability name 중 하나가 명시된 경우에만 등록한다.
11. `crates/shacs-core/src/runtime/subagent.rs`는 `SubagentExecutionConfig`, `build_subagent_tool_registry`로 partial inherited execution restriction을 제공한다.

### 아직 남은 것

1. `SafetyCapability`, `PermissionMode`, `SafetySnapshot`, `SecretRef`, `RedactedValue`, `ApprovalRequest`, `ApprovalDecision` 타입.
2. `fs_read`, `fs_write`, `proc_exec`, `net_outbound`, `secret_read` formal capability evaluator.
3. `plan`, `default`, `auto` permission mode decision table.
4. effect approval correlation, stale approval rejection, denied execution standard outcome.
5. inherited `SafetySnapshot` for subagent/service reentry.
6. context, compaction, provider, tool, event 전역 redaction pipeline.
7. raw secret value와 secret reference의 type level separation.
8. structured argv process envelope.
9. browser automation과 web browsing permission control tests.

### `ask_user` 위치

`ask_user`는 현재 user question과 interruption mechanism이다. later tool skip, button resume, tool result resume의 근거는 있다. 하지만 Spec 010의 formal approval gate가 아니다. approval request id, approval decision, stale approval rejection을 검증하는 체계로 쓰면 안 된다.

## 구현 웨이브

### Wave 1. 현재 guard 정합성 고정

1. filesystem, shell, network, auth, SelfTool, MCP default-deny, tool result redaction/persistence의 현재 safety guard를 Spec 010 local baseline mapping으로 고정한다.
2. local baseline 완료가 formal permission model 완료로 읽히지 않게 문서와 검증 matrix 표현을 정리한다.
3. `ask_user`를 approval gate가 아니라 interruption mechanism으로 분리한다.

### Wave 2. Formal capability model 설계

1. `SafetyCapability`, `PermissionMode`, `SafetySnapshot`의 실제 도입 범위를 설계한다.
2. `fs_read`, `fs_write`, `proc_exec`, `net_outbound`, `secret_read` evaluator를 현재 tool guard 위에 어떻게 연결할지 정한다.
3. `plan`, `default`, `auto` decision table이 기존 shell/web/filesystem 동작과 충돌하지 않는지 검증한다.

### Wave 3. Approval contract 설계

1. `ApprovalRequest`와 `ApprovalDecision`의 effect correlation을 정의한다.
2. stale 또는 expired approval rejection을 session turn lifecycle과 연결한다.
3. denied execution standard outcome을 tool runtime과 interface surface에 맞춘다.

### Wave 4. Secret과 redaction 통합

1. `SecretRef`와 raw secret value의 type level separation을 도입한다.
2. `RedactedValue`와 unified redaction pipeline을 context, compaction, provider, tool, event에 연결한다.
3. oversized tool result sink 밖의 universal pre-persistence secret redaction pass를 추가할지 결정한다.

### Wave 5. Inheritance와 browser/web permission 검증

1. subagent와 service reentry에 inherited `SafetySnapshot`을 연결한다.
2. current partial restrictions가 parent boundary를 넓히지 않도록 formal contract를 세운다.
3. browser automation과 web browsing permission control tests를 추가한다.

## Verification Evidence

현재 증거는 다음으로 본다.

Filesystem/path:

1. `recursive_tools_skip_symlinks_that_escape_workspace`
2. `write_file_rejects_allowed_dir_escape`
3. `write_and_edit_reject_symlink_input_paths`
4. `write_file_rejects_symlink_parent_component`
5. `runtime_context_rejects_media_symlink_to_outside_workspace`

Exec/process:

1. `exec_tool_blocks_dangerous_and_non_allowlisted_commands`
2. `exec_tool_restricts_working_dir_and_paths_to_workspace`
3. `exec_tool_times_out_long_running_command`
4. `exec_tool_blocks_internal_urls_and_invalid_deny_regex`

Network/web:

1. `blocks_private_loopback_link_local_cgnat_and_mapped_addresses`
2. `ssrf_whitelist_allows_specific_cidrs_and_ignores_invalid_entries`
3. `web_fetch_blocks_internal_url_before_client_call`
4. `web_fetch_blocks_private_redirect_and_truncates_text`
5. `web_search_blocks_private_searxng_base_url`

Secrets/auth/redaction:

1. `auth_store_roundtrips_open_code_style_oauth_entries`
2. `resolves_env_refs_recursively_and_reports_missing`
3. `migration_writeback_preserves_env_templates_and_does_not_persist_workspace_override`
4. `codex_import_token_writes_auth_without_leaking_token_to_config_or_output`
5. `copilot_import_token_writes_auth_without_leaking_token_to_config_or_output`
6. `codex_login_success_saves_oauth_session_without_config_secret`
7. `self_tool_checks_summary_paths_and_redacts_sensitive_fields`
8. `self_tool_blocks_sensitive_and_read_only_paths`
9. `redacts_oversized_string_tool_result_in_file_and_reference`
10. `redacts_oversized_text_block_json_in_file_and_reference`
11. `session_management_commands_cover_history_export_clear_diagnostics_and_compact`

MCP default-deny:

1. `mcp_enabled_tools_defaults_to_empty_default_deny`
2. `missing_mcp_enabled_tools_deserializes_to_empty_default_deny`
3. `mcp_empty_enabled_tools_registers_no_tools`
4. `mcp_registers_tools_resources_prompts_and_filters_enabled_tools`
5. `mcp_wrappers_retry_transient_errors_once`

Ask/interruption/inheritance:

1. `runtime_preserves_ask_user_interrupt_and_skips_later_tools`
2. `runtime_runner_stops_on_ask_user_without_later_tools`
3. `loop_ask_user_interrupt_publishes_buttons_and_resumes_as_tool_result`
4. `spawn_tool_uses_context_and_delegates_to_spawner`
5. `subagent_stale_inbound_is_not_persisted_as_session_content`

이 증거는 Spec 010 self-hosted/local baseline 완료의 근거다. formal permission/approval engine 완료 증거로 승격하지 않는다.

## Open Risks

1. 현재 safety guard가 여러 모듈에 흩어져 있어 정책 설명과 실제 집행이 어긋날 수 있다.
2. shell exec가 structured argv envelope가 아니므로 command string 해석 경계가 formal model보다 약하다.
3. oversized tool result persistence sink는 redaction과 symlink hardening을 갖지만, 모든 persistence surface를 지나는 universal pre-persistence secret redaction pass는 없다.
4. `ask_user`를 approval gate로 오해하면 stale approval과 denied execution 검증이 빠진다.
5. subagent restrictions는 partial mapping이며 inherited `SafetySnapshot`이 아니다.

## 종료 기준

현재 local baseline 종료 기준은 다음이다.

1. Spec 010과 이 PRD가 현재 구현을 self-hosted/local baseline complete로 표시하되 formal permission engine 완료로 주장하지 않는다.
2. filesystem, shell, network, auth, SelfTool, MCP default-deny, oversized tool result redaction/persistence, subagent 제한이 local baseline mapping으로 설명된다.
3. `ask_user`가 approval gate가 아니라 interruption mechanism으로 분리된다.
4. future gaps가 local baseline blocker가 아니라 남은 formal host safety work로 분리된다.
5. `self-hosted/personal-use` product framing을 유지한다.

Spec 010의 formal permission model 완료 기준은 별도다. formal capability evaluator, permission mode, approval engine, inherited `SafetySnapshot`, unified redaction pipeline, structured argv envelope, browser/web permission tests가 구현되고 검증되어야 그 범위를 완료로 볼 수 있다.
