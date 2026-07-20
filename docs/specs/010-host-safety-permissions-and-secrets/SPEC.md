# host safety, permissions, and secrets 아키텍처 명세

Status: Complete (Baseline)

Implemented scope: 현재 구현은 workspace filesystem guard, shell command guard, network SSRF guard, auth and diagnostics redaction, oversized tool result redaction, MCP default-deny registration, and bounded subagent tool inheritance의 local host safety baseline을 지원한다.

Open work moved to: [030 policy, permission, redaction, and containment model](../030-policy-permission-redaction-and-containment-model/SPEC.md), [035 configuration runtime layout and execution snapshots](../035-configuration-runtime-layout-and-execution-snapshots/SPEC.md)

Not carried forward: admin approval chain, central secret vault, organization policy engine or distribution, remote operator console은 self-hosted personal-use 범위 밖에 남긴다.

## 문서 목적

이 문서는 `shacs-bot`의 host safety, permission, secret handling을 현재 구현과 앞으로 남은 작업으로 나누어 정리한다. Spec 010은 현재 `self-hosted/local baseline` 기준으로 닫힌 상태다. 이것은 formal `SafetySnapshot`, `PermissionMode`, approval engine, 통합 redaction pipeline이 완성되었다는 뜻이 아니라, 개인용 로컬 런타임에서 요구되는 guard-denied execution, default-deny MCP 등록, oversized tool result redaction 기준이 구현과 검증으로 닫혔다는 뜻이다.

이 문서의 현재 역할은 다음과 같다.

1. 현재 구현된 안전 경계를 정확히 설명한다.
2. 현재 아키텍처 매핑으로 인정할 수 있는 범위를 고정한다.
3. future formal host safety와 permission 작업을 현재 local baseline 완료 범위와 분리한다.

`shacs-bot`은 `self-hosted/personal-use` 성격의, 사용자가 직접 설치하고 운영하는 개인용 런타임을 기본으로 본다. 따라서 사용자의 워크스페이스와 로컬 환경을 보호하는 것이 핵심이며, 관리자 승인 체계나 조직 정책 배포를 기본 가정하지 않는다.

## 상위 기준과의 관계

이 문서는 `docs/SYSTEM-FOUNDATION.md`, `docs/specs/001-session-kernel/SPEC.md`, `docs/specs/004-tool-runtime/SPEC.md`, `docs/specs/007-main-orchestrator-policy/SPEC.md`, `docs/specs/008-configuration-profiles-and-runtime-layout/SPEC.md`, `docs/specs/009-context-assembly-and-compaction-input/SPEC.md`를 전제로 한다.

현재 구현은 하나의 공식 permission engine이 모든 effect를 판정하는 구조가 아니다. 대신 filesystem, shell, network, config, CLI auth, inspect, tool result persistence, subagent runtime에 각각 safety guard가 들어가 있다.

따라서 이 문서는 다음 두 층을 구분한다.

1. 현재 아키텍처 매핑: 이미 존재하는 guard, boundary check, redaction, persistence hardening.
2. future formal model: 아직 도입되지 않은 capability evaluator, approval contract, inherited `SafetySnapshot`, 통합 redaction pipeline.

## 범위

현재 문서에서 다루는 범위는 다음과 같다.

1. filesystem, process, network, secrets의 현재 안전 경계.
2. 현재 구현을 Spec 010 요구사항에 어떻게 매핑할지.
3. 현재 테스트 증거로 확인되는 안전 동작.
4. local baseline 완료 밖의 future formal host safety, permission, redaction 항목.

이 문서는 다음을 현재 기능으로 선언하지 않는다.

1. `SafetyCapability`, `PermissionMode`, `SafetySnapshot`, `SecretRef`, `RedactedValue`, `ApprovalRequest`, `ApprovalDecision` 타입이 완성되어 있다는 주장.
2. `fs_read`, `fs_write`, `proc_exec`, `net_outbound`, `secret_read`를 하나의 formal evaluator가 판정한다는 주장.
3. `plan`, `default`, `auto` permission mode 결정표가 runtime 전체에 적용된다는 주장.
4. Spec 022가 소유하는 formal approval engine까지 이 문서가 완료 선언한다는 주장.
5. context, compaction, provider, tool, event를 모두 통과하는 통합 redaction pipeline이 있다는 주장.

## 현재 구현 요약

현재 구현은 분산된 안전 장치들의 조합이다.

1. Filesystem은 `crates/shacs-core/src/tools/filesystem.rs`의 `PathContext`, `resolve_path`, `resolve_creatable_path`를 중심으로 workspace 내부 경로를 확인하고 symlink를 통한 workspace escape를 막는다.
2. Process 실행은 `crates/shacs-core/src/tools/shell.rs`의 `ExecConfig`, `ExecTool`, `resolve_working_dir`, `guard_command`, `default_deny_patterns`를 통해 workspace, timeout, environment allowlist, deny pattern을 적용한다.
3. 현재 exec는 shell string 실행이다. structured argv process envelope가 완성된 것은 아니다.
4. Network와 web 접근은 `crates/shacs-security/src/lib.rs`의 `NetworkGuard`, `validate_url_target`, `contains_internal_url`와 `crates/shacs-core/src/tools/web.rs`의 guard 호출로 SSRF, private, loopback, internal URL 접근을 차단한다.
5. Secret과 auth는 `crates/shacs-config/src/lib.rs`의 `AuthStore`, `ProviderAuth`, env placeholder 처리와 `crates/shacs-cli/src/lib.rs`의 Codex, Copilot token import, login, auth overlay 처리로 구성된다.
6. CLI는 token import와 login 과정에서 config나 output에 raw token을 남기지 않는 방향으로 동작하며, transport, email error redaction과 session diagnostics, export surface를 갖고 있다.
7. Inspect와 self tool은 `crates/shacs-core/src/tools/self_tool.rs`의 `SelfTool`, `SENSITIVE_NAMES`, `redact_object`, `redact_value`, `is_sensitive`, `is_blocked`, `is_read_only`로 민감 field redaction과 차단을 수행한다.
8. Tool result persistence는 `crates/shacs-utils/src/tool_results.rs`에서 oversized result를 별도 저장하기 전에 redaction을 적용하고 symlink hardening을 수행한다.
9. MCP capability 등록은 `crates/shacs-config/src/lib.rs`의 empty `enabledTools` 기본값과 `crates/shacs-core/src/tools/mcp.rs`의 등록 필터로 tools, resources, prompts 모두 default-deny다. 사용자가 `*`, raw capability name, wrapped capability name 중 하나를 명시한 경우에만 등록된다.
10. Subagent 제한은 `crates/shacs-core/src/runtime/subagent.rs`의 `SubagentExecutionConfig`, `build_subagent_tool_registry`로 일부 상속된다. formal inherited `SafetySnapshot`은 아니다.

---

## 현재 아키텍처 매핑 기준

Spec 010의 현재 매핑은 formal permission model이 아니라, 다음 조건을 만족하는 구현 증거로 인정한다.

1. 경로 입력이 workspace boundary와 symlink escape 검사를 통과해야 한다.
2. 생성 대상 path도 parent symlink와 workspace escape를 검사해야 한다.
3. shell 실행은 workspace 내부 working directory, deny pattern, timeout, environment allowlist를 적용해야 한다.
4. network와 web tool은 private, loopback, link local, internal URL, private redirect를 client 호출 전에 차단해야 한다.
5. auth와 secret 처리 surface는 raw token을 config, output, diagnostics에 쉽게 남기지 않아야 한다.
6. inspect와 self tool은 민감 field와 read only path를 숨기거나 차단해야 한다.
7. 큰 tool result persistence는 파일 저장과 반환 reference 전에 redaction을 적용하고 symlink hardening을 가져야 한다.
8. MCP capability 등록은 tools, resources, prompts 모두 명시적으로 enable되지 않으면 등록하지 않아야 한다.
9. subagent 실행은 상위 context와 제한된 tool registry를 따르는 범위 안에서만 현재 매핑으로 본다.

이 매핑은 Spec 010의 self-hosted/local baseline 완료 선언이다. future formal model을 완료했다는 선언은 아니며, local baseline 밖의 formal permission model은 별도 future work다.

## Capability와 permission의 현재 상태

Spec 010이 원하는 future capability는 다음이다.

1. `fs_read`
2. `fs_write`
3. `proc_exec`
4. `net_outbound`
5. `secret_read`

현재 코드는 이 capability들을 공식 `SafetyCapability` 타입과 evaluator로 묶지 않는다. 대신 각 tool과 runtime surface가 자기 입력 범위에서 guard를 수행한다.

현재 상태를 이렇게 해석한다.

1. `fs_read`, `fs_write` 매핑은 filesystem tool의 path resolution과 symlink escape 방지다.
2. `proc_exec` 매핑은 exec tool의 command guard, working directory 제한, timeout, env allowlist다.
3. `net_outbound` 매핑은 `NetworkGuard`와 web fetch/search guard다.
4. `secret_read` 매핑은 config/auth store의 env placeholder와 CLI auth overlay, SelfTool redaction, diagnostics redaction이다.

future work는 이 분산 매핑을 하나의 capability evaluator로 연결하는 것이다.

## Permission mode와 approval의 현재 상태

`plan`, `default`, `auto` permission mode와 approval correlation의 formal 계약은 Spec 022가 소유한다. Spec 010은 host safety/local baseline 문서로 남으며, permission engine 완료 여부를 이 문서에서 중복 판정하지 않는다.

현재 구현에서 확인되는 것은 다음이다.

1. filesystem, shell, network, secret surface가 각자 위험 입력을 차단한다.
2. `ask_user`는 사용자에게 질문하고 실행을 중단하거나 재개하는 interruption mechanism이다.
3. `ask_user`는 formal effect approval gate가 아니며, formal permission approval은 `PermissionApproval` interrupt와 Spec 022의 approval correlation 계약이 담당한다.
4. `ask_user` 테스트는 interruption과 later tool skip, button resume 동작의 근거이며, approval request와 decision correlation의 근거는 Spec 022 구현/테스트에서 확인한다.

Spec 010 관점의 남은 permission work는 host safety baseline을 유지하면서 Spec 022의 permission engine을 소비하는 경계를 문서 간 일관성 있게 유지하는 것이다.

## Redaction과 secret handling의 현재 상태

현재 구현에는 여러 redaction 지점이 있다.

1. `AuthStore`와 `ProviderAuth`는 OAuth와 token 정보를 config 구조에서 다룬다.
2. env placeholder는 recursive resolution과 missing env reporting을 지원한다.
3. Codex와 Copilot token import, login flow는 raw token을 config나 output에 남기지 않는 방향으로 검증된다.
4. CLI auth overlay와 transport, email error handling은 민감 정보를 가리는 surface를 갖고 있다.
5. `SelfTool`은 민감 이름, 민감 값, 차단 path, read only path를 기준으로 inspect 결과를 redaction하거나 차단한다.
6. session diagnostics와 export surface는 session 관리 명령 안에서 노출 경계를 가진다.

아직 없는 것은 하나의 `RedactedValue` 타입과 통합 redaction pipeline이다. oversized tool result persistence sink는 파일 저장과 반환 reference 전에 redaction을 적용한다. 다만 모든 provider, context, compaction, tool, event persistence 직전에 raw secret을 통과시키지 않는 universal pre-persistence secret redaction pass는 아직 없다.

future work는 다음이다.

1. raw secret value와 secret reference의 type level separation.
2. `SecretRef`와 `RedactedValue` 중심의 persistence contract.
3. context, compaction, provider, tool, event를 모두 지나는 redaction pipeline.
4. redaction 실패 시 raw persistence를 금지하는 표준 outcome.

## Inherited safety의 현재 상태

현재 subagent runtime에는 일부 상속 제한이 있다. `SubagentExecutionConfig`와 `build_subagent_tool_registry`는 context와 tool registry를 제한된 형태로 구성한다.

이것은 current architecture mapping으로 인정할 수 있다. 하지만 formal inherited `SafetySnapshot`은 아니다.

future work는 다음이다.

1. 부모 effect의 capability ceiling을 child execution에 타입으로 전달한다.
2. subagent와 service reentry가 parent safety boundary를 넓히지 못하게 한다.
3. stale inbound나 late result가 닫힌 session content를 바꾸지 못하게 formal contract로 고정한다.

## 현재 검증 증거

현재 구현을 뒷받침하는 테스트 증거는 다음 이름들로 정리한다.

Filesystem과 path boundary:

1. `recursive_tools_skip_symlinks_that_escape_workspace`
2. `write_file_rejects_allowed_dir_escape`
3. `write_and_edit_reject_symlink_input_paths`
4. `write_file_rejects_symlink_parent_component`
5. `runtime_context_rejects_media_symlink_to_outside_workspace`

Exec와 process boundary:

1. `exec_tool_blocks_dangerous_and_non_allowlisted_commands`
2. `exec_tool_restricts_working_dir_and_paths_to_workspace`
3. `exec_tool_times_out_long_running_command`
4. `exec_tool_blocks_internal_urls_and_invalid_deny_regex`

Network와 web boundary:

1. `blocks_private_loopback_link_local_cgnat_and_mapped_addresses`
2. `ssrf_whitelist_allows_specific_cidrs_and_ignores_invalid_entries`
3. `web_fetch_blocks_internal_url_before_client_call`
4. `web_fetch_blocks_private_redirect_and_truncates_text`
5. `web_search_blocks_private_searxng_base_url`

Secrets, auth, redaction:

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

Ask, interruption, inheritance:

1. `runtime_preserves_ask_user_interrupt_and_skips_later_tools`
2. `runtime_runner_stops_on_ask_user_without_later_tools`
3. `loop_ask_user_interrupt_publishes_buttons_and_resumes_as_tool_result`
4. `spawn_tool_uses_context_and_delegates_to_spawner`
5. `subagent_stale_inbound_is_not_persisted_as_session_content`

위 테스트들은 Spec 010 self-hosted/local baseline 완료의 근거다. formal approval engine이나 formal `SafetySnapshot` 완료 증거로 읽으면 안 된다.

## Future gaps

다음 항목은 현재 local baseline blocker가 아니라 future formal host safety와 permission work다.

1. `SafetySnapshot`, `SecretRef`, `RedactedValue` type level separation.
2. `fs_read`, `fs_write`, `proc_exec`, `net_outbound`, `secret_read`에 대한 formal capability evaluator의 Spec 010 소비 경계 정리.
3. subagent와 service reentry를 위한 inherited `SafetySnapshot`.
4. unified context, compaction, provider, tool, event redaction pipeline.
5. raw secret value와 secret reference의 type level separation.
6. structured argv process envelope.
7. browser automation과 web browsing permission control tests.

## 명시적 비범위

다음은 Spec 010의 현재 범위가 아니다.

1. admin approval chain.
2. central secret vault.
3. organization policy engine 또는 distribution.
4. remote operator console.
5. multi user RBAC.
6. distributed trust negotiation.
7. corporate EDR 또는 anti virus integration.

필요가 생기면 별도 문서에서 다룬다. 지금의 기본 주체는 사용자가 직접 운영하는 개인용 런타임이다.

## 결론

Spec 010은 self-hosted/local baseline 기준으로 완료되었다. 현재 상태는 filesystem, shell, network, auth, self inspection, MCP capability registration, result persistence, subagent runtime에 흩어진 safety guard로 로컬 개인용 런타임의 baseline을 닫는다.

다음 단계의 핵심은 이 local baseline을 유지하면서, formal capability evaluator, permission mode, approval gate, inherited safety snapshot, unified redaction pipeline을 별도 future work로 완성하는 것이다.
