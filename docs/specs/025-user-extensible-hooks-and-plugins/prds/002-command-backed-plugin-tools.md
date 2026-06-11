# PRD 002: command-backed plugin tools

## 목표

Plugin-provided tool을 core tool 추가 없이 등록하고 실행하는 최소 안전 경계를 고정한다. 초기 tool handler는 command-backed process 또는 MCP declaration처럼 runtime이 boundary를 통제할 수 있는 형태를 우선한다.

## 범위

- plugin tool schema registration
- command-backed handler declaration
- MCP-backed tool declaration handoff
- permission ceiling and capability request
- Tool Search deferrable classification
- output limit, timeout, diagnostics

## 비범위

- in-process arbitrary code execution
- public tool marketplace
- provider-native tool search beta
- plugin tool을 core/builtin tool로 자동 승격하는 정책

## 구현 요구사항

1. Plugin tool은 `ToolRegistry`에 등록되더라도 source kind가 plugin임을 inspect할 수 있어야 한다.
2. Plugin tool schema는 validation 실패 시 load-blocked diagnostic으로 남아야 한다.
3. Command-backed handler는 JSON args를 받고 bounded JSON/text result를 반환해야 한다.
4. Handler timeout, exit status, stderr summary는 redacted diagnostics에 남아야 한다.
5. Plugin tool은 permission request를 할 수 있지만 permission approval을 스스로 만들 수 없다.
6. Plugin tool은 기본적으로 Tool Search deferrable candidate다.
7. Disabled/blocked plugin의 tool은 provider-visible surface와 deferred catalog 모두에 없어야 한다.

## SPEC 입력

1. 주관 spec은 `docs/specs/025-user-extensible-hooks-and-plugins/SPEC.md`다.
2. tool execution boundary는 `docs/specs/004-tool-runtime/SPEC.md`를 소비한다.
3. permission/approval은 `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`와 `docs/specs/022-auto-approval-permissions/SPEC.md`를 소비한다.
4. Tool Search deferrable integration은 `docs/specs/020-tool-search-and-provider-tool-surface/SPEC.md`를 소비한다.
5. diagnostics/replay는 `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`와 `docs/specs/018-evaluation-automation-and-self-improvement/SPEC.md`를 소비한다.

## Dependency Cut

1. PRD 000의 enabled plugin state가 선행되어야 한다.
2. Plugin tool schema registration은 plugin command execution과 분리되어야 한다.
3. Plugin tool은 core/builtin tool로 승격되지 않는다.
4. Handler execution은 command-backed or MCP-backed boundary를 통과해야 한다.
5. public marketplace, in-process arbitrary code execution, provider-native Tool Search beta는 비범위다.

## 구현 매핑

| Requirement | Likely crate/module | Test perspective |
|---|---|---|
| plugin tool schema registration | `crates/shacs-core/src/tools/mod.rs`, `runtime/plugin.rs` | source kind is plugin |
| command-backed handler | `crates/shacs-core/src/tools/exec.rs` 또는 plugin adapter | timeout and bounded output |
| Tool Search deferrable integration | `crates/shacs-core/src/tools/tool_search.rs` | enabled plugin appears in deferred catalog |
| permission boundary | `crates/shacs-config/src/permissions.rs`, `crates/shacs-core/src/runtime/runner.rs` | plugin cannot self-approve |

## 데이터/상태 모델

1. `PluginToolDefinition`: tool name, schema, source plugin, schema digest, handler kind, required permission metadata를 가진다.
2. `PluginToolHandler`: command-backed process 또는 MCP-backed declaration을 구분한다.
3. `PluginToolExecutionRecord`: args digest, timeout, exit status, result digest, redaction status를 가진다.
4. `PluginToolVisibility`: `visible`, `deferrable`, `excluded_disabled`, `excluded_blocked`, `excluded_not_enabled`를 구분한다.
5. `PluginToolPermissionRequest`: request metadata일 뿐 approval decision이 아니다.

## 정상 시퀀스

1. enabled plugin이 valid tool schema와 command-backed handler를 선언한다.
2. registry가 source kind plugin으로 tool definition을 등록한다.
3. Tool Search assembler가 plugin tool을 deferrable candidate로 분류한다.
4. provider가 bridge `tool_call`을 요청하면 underlying plugin tool call로 unwrap한다.
5. runtime executor가 validation, permission, timeout, result normalization을 수행한다.

## 실패 시퀀스

1. disabled plugin의 tool은 registry와 deferred catalog에 나타나지 않는다.
2. schema validation 실패는 plugin blocked diagnostic으로 남는다.
3. handler timeout 또는 non-zero exit은 normalized tool error가 된다.
4. plugin tool이 approval을 직접 반환해도 permission approval로 소비하지 않는다.
5. replay는 command-backed handler를 live-dispatch하지 않고 recorded evidence를 사용한다.

## 검증 관점

1. 첫 failing test는 enabled plugin tool이 source kind plugin으로 등록되는지 확인한다.
2. disabled/blocked/not-enabled plugin tool이 provider-visible surface와 deferred catalog에서 제외되는지 확인한다.
3. `tool_call` unwrap 이후에도 existing executor validation과 permission gate가 호출되는지 확인한다.
4. timeout/non-zero exit/redaction diagnostics를 snapshot으로 고정한다.

## Cargo 검증

1. `cargo fmt --manifest-path crates/shacs-core/Cargo.toml -- --check`
2. `cargo clippy --manifest-path crates/shacs-core/Cargo.toml --all-targets -- -D warnings`
3. `cargo test --manifest-path crates/shacs-core/Cargo.toml plugin_tool`
4. Tool Search를 건드렸다면 `cargo test --manifest-path crates/shacs-core/Cargo.toml tool_search`

## 완료 기준

- Plugin tool execution은 004 tool runtime과 010/022 permission 경계를 우회하지 않는다.
- Tool Search는 enabled plugin tool만 deferrable catalog에 포함한다.
- Replay는 command-backed destructive handler를 live-dispatch하지 않는다.
