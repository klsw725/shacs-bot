# PRD 002. common process envelope and side-effect gate

Status: Planned

## Goal

Spec 030 needs one typed process language for every launch that can create a host side effect. This PRD defines that language and the pre-spawn permission gate for exec, plugin command-backed surfaces, MCP stdio servers, app processes, dependency preparation, and verified entrypoints.

The goal is not to invent a new process manager. The goal is to make every supported side-effect launch prove the same facts before spawn: who owns the adapter, what executable or entrypoint is being requested, what redacted argument and env evidence will be stored, which policy and safety snapshot applies, which secret refs may be resolved just in time, which containment intent applies, which approval lineage is valid, and which redacted receipt will correlate the result.

## Scope

1. Define `ProcessExecutionEnvelope` as the required pre-spawn typed input for supported process launches.
2. Define `ProcessExecutionReceipt` as the required redacted success or failure output after the adapter completes, times out, or is cancelled.
3. Define the adapter matrix for exec, plugin command, plugin command-backed tool, plugin hook callback, MCP stdio server startup, app process start, dependency preparation, and verified entrypoint execution.
4. Define the exact side-effect gate order: normalize, static policy, approval and ceiling, spawn, redacted receipt.
5. Define timeout and cancellation semantics for future implementation, including deterministic resume cases.
6. Define replay non-execution rules. Replay reads recorded envelope refs and receipts only. It must not dispatch a live process or resolve raw secrets.
7. Define TDD order, focused Cargo commands, real process QA commands, closure evidence, and exit criteria.

## Non-Scope

1. AppSupervisor lifecycle, start, stop, restart, recover, stale process handling, and app process state vocabulary remain Spec 032 scope.
2. MCP protocol semantics, JSON-RPC framing details, capability listing, and remote transport behavior remain the MCP runtime owner scope. This PRD only gates stdio process startup before spawn.
3. Dependency installer behavior, package manager choice, package cache layout, native build policy, and runtime prerequisite installation remain Spec 032 and Spec 035 scope.
4. Physical snapshot persistence, runtime layout, migration, immutable snapshot storage, and trust persistence remain Spec 035 scope.
5. Kernel isolation, guaranteed sandboxing on native hosts, fleet policy rollout, organization RBAC, hosted plugin marketplace, and central secret vaults are not closure requirements.
6. This PRD does not make plugin hook output, skill text, tool output, user prompt text, MCP prompt text, or process stdout an authorization source.

## SPEC Inputs

1. Spec 030 `Implementation PRDs`, `Invariants`, `Must Have`, `Must Not Have`, and `Acceptance Criteria` mark containment evidence, process paths, skill trust, dependency preparation, verified entrypoints, and common process envelopes as closure targets, not current baseline requirements.
2. Spec 030 `Stronger Contract Owner Map` assigns the common process envelope to this PRD and keeps AppSupervisor, installer behavior, MCP semantics, and physical snapshot persistence external.
3. Spec 030 `External Dependency Gates` keep 032 and 035 ownership of app lifecycle, trust lifecycle, and execution snapshot persistence.
4. PRD 000 supplies `PolicySafetySnapshotRef` and `policy_safety_digest` as required envelope inputs.
5. PRD 001 supplies `SecretRef` and `RedactionEvidenceRef` as the only allowed secret-bearing inputs.
6. Current Rust process facts come from `tool_execution.rs`, `plugin_runtime.rs`, `agent_loop.rs`, `tools/shell.rs`, `tools/mcp.rs`, `tools/spawn.rs`, and `shacs-app/src/app.rs`.
7. Spec 004 confirms the current tool runtime uses `RuntimeToolCall`, `RuntimeToolExecutor`, `ToolResult`, and checkpoint events, and does not yet have a common process envelope.
8. Spec 025 confirms plugin command-backed tools, hooks, commands, and MCP declarations exist, use bounded command processes, clear env in current plugin paths, and reject live plugin dispatch during replay.
9. Spec 032 confirms app process lifecycle and skill trust lifecycle are external open scope, while 030 may consume their process and trust evidence after they exist.

## Dependency Cut

1. PRD 000 must close before implementation can require `policy_safety_snapshot_ref` on every process envelope.
2. PRD 001 must close before implementation can carry secret refs and redaction evidence in env or argument projections.
3. PRD 003 consumes this envelope to prove containment inheritance and ceiling non-widening before process admission.
4. PRD 005 consumes this envelope for dependency preparation and verified entrypoint authorization, but it owns only permission consumption of active trust provenance, not installer or runner internals.
5. Spec 032 supplies app process producer facts, AppSupervisor lifecycle receipts, skill trust lifecycle states, and verified entrypoint lifecycle status.
6. Spec 035 supplies physical execution snapshots, config refs, trust persistence, and any storage that contains envelope refs.
7. Current runtime remains valid before this PRD is implemented. The implementation must characterize current distributed gates before changing behavior.

## Exact Typed Envelope

The future implementation must define a typed contract equivalent to this Rust shape. Module placement may change, but field meaning must not change.

```rust
pub struct ProcessExecutionEnvelope {
    pub schema_id: ProcessEnvelopeSchemaId,
    pub envelope_id: ProcessEnvelopeId,
    pub process_identity: ProcessIdentity,
    pub requested_at_unix_ms: u64,
    pub adapter: ProcessAdapterRef,
    pub owner_adapter: OwnerAdapterBoundary,
    pub executable_ref: ExecutableRef,
    pub entrypoint_ref: Option<EntryPointRef>,
    pub argument_projection: RedactedArgumentProjection,
    pub environment_projection: RedactedEnvironmentProjection,
    pub workspace_scope: WorkspaceScope,
    pub cwd_scope: CwdScope,
    pub policy_safety_snapshot_ref: PolicySafetySnapshotRef,
    pub secret_refs: Vec<SecretRef>,
    pub redaction_evidence_refs: Vec<RedactionEvidenceRef>,
    pub containment_intent: ContainmentIntent,
    pub containment_evidence_ref: Option<ContainmentEvidenceRef>,
    pub timeout_policy: ProcessTimeoutPolicy,
    pub cancellation_policy: ProcessCancellationPolicy,
    pub permission_action: PermissionedAction,
    pub approval_lineage: Option<ApprovalLineageRef>,
    pub receipt_correlation: ReceiptCorrelation,
}

pub struct ProcessExecutionReceipt {
    pub schema_id: ProcessReceiptSchemaId,
    pub receipt_id: ProcessReceiptId,
    pub envelope_id: ProcessEnvelopeId,
    pub process_identity: ProcessIdentity,
    pub adapter: ProcessAdapterRef,
    pub outcome: ProcessReceiptOutcome,
    pub started_at_unix_ms: Option<u64>,
    pub completed_at_unix_ms: u64,
    pub elapsed_ms: Option<u64>,
    pub exit_status: Option<RedactedExitStatus>,
    pub stdout_summary: RedactedStreamSummary,
    pub stderr_summary: RedactedStreamSummary,
    pub artifact_refs: Vec<RedactedArtifactRef>,
    pub policy_safety_snapshot_ref: PolicySafetySnapshotRef,
    pub permission_action_id: String,
    pub approval_ref: Option<String>,
    pub containment_evidence_ref: Option<ContainmentEvidenceRef>,
    pub redaction_evidence_refs: Vec<RedactionEvidenceRef>,
    pub failure: Option<ProcessFailure>,
    pub replayable: ProcessReplayability,
}
```

Required enum values and field rules:

| Type or field | Required contract |
|---|---|
| `ProcessEnvelopeSchemaId` | Only `process_envelope.v1` is valid for this PRD. Unknown schema rejects before policy. |
| `ProcessReceiptSchemaId` | Only `process_receipt.v1` is valid for this PRD. Unknown schema rejects during replay and diagnostics. |
| `process_identity` | Opaque process identity made from adapter family, local request id, and digest. It is not an OS handle and must not expose PID as durable truth. |
| `adapter` | One of `exec`, `plugin_command`, `plugin_tool`, `plugin_hook`, `mcp_stdio`, `app_process`, `dependency_preparation`, `verified_entrypoint`. |
| `owner_adapter` | Names the Rust adapter boundary that may spawn after gate approval. It must be exact enough to reject direct `Command::new` bypasses. |
| `executable_ref` | Safe identity for executable or command declaration. It may include basename, plugin-relative path, app-relative path, digest, source owner, and safe summary. It must not store shell-expanded raw command text when a structured argv form exists. |
| `entrypoint_ref` | Optional source identity for app, skill, plugin, dependency, or verified entrypoint. It must include owner source, manifest digest, content digest when available, and declared capability scope. |
| `argument_projection` | Stores argument schema digest, redacted argument digest, safe key summary, and redaction evidence. Raw args must never persist. |
| `environment_projection` | Stores env key refs, `SecretRef` ids, non-secret env key allowlist digest, and redacted env digest. Raw env values and full env maps must never persist. |
| `workspace_scope` | Records allowed workspace root ref, narrowing reason, and scope digest. Absolute host paths are not safe summaries. |
| `cwd_scope` | Records resolved cwd as a workspace-relative or opaque redacted locator. It must reject cwd outside declared scope. |
| `policy_safety_snapshot_ref` | Required. Missing ref rejects before static policy. |
| `secret_refs` | Required when args or env need secrets. Empty means no secret resolution is allowed. |
| `containment_intent` | One of `inherit_parent`, `require_confirmed_non_privileged`, `allow_unknown_with_ask`, `deny_unknown`, `external_owner_supplied`. Unknown intent rejects. |
| `containment_evidence_ref` | Optional evidence ref. Missing evidence is not safe evidence. PRD 003 decides equal-or-narrower proof. |
| `timeout_policy` | Required bounded timeout with max cap. Unbounded launches reject. |
| `cancellation_policy` | Required action for cooperative cancel, force cleanup, late result, and resume. |
| `permission_action` | Required normalized `PermissionedAction` for this process candidate. It must use redacted args and PRD 000 snapshot digest. |
| `approval_lineage` | Required only when policy returned ask and a local user approved. Expired, consumed, mismatched, inspect-only, or wrong-scope approval rejects. |
| `receipt_correlation` | Binds envelope id, action id, action digest, policy safety digest, adapter family, and expected receipt id. |

`ProcessReceiptOutcome` must include `succeeded`, `failed`, `spawn_rejected`, `timed_out`, `cancelled`, `malformed_input`, `policy_denied`, `approval_required`, `approval_rejected`, `ceiling_violation`, `containment_mismatch`, `redaction_failed`, and `replay_rejected`.

`ProcessFailure` must include machine fields: `code`, `retryable`, `owner_adapter`, `safe_message`, `diagnostics_refs`, and optional `redacted_detail_digest`. It must not include raw args, raw env, raw stdout, raw stderr, process handles, absolute host paths, or secret values.

## Adapter Matrix

| Adapter family | Current surface | Envelope owner adapter | Spawn boundary after gate | Raw persistence rule | External owner cut |
|---|---|---|---|---|---|
| `exec` | `ExecTool` in `tools/shell.rs` | `shacs-core::tools::exec` | `/bin/bash -l -c` or Windows command wrapper only after envelope allow | Store command digest, redacted command summary, cwd scope, env key digest. Do not store raw shell text when it contains secret material. | Sandbox wrapper and host guard remain current tool owner facts. |
| `plugin_command` | `PluginCommandDispatcher` in `plugin_runtime.rs` | `shacs-core::runtime::plugin_command_dispatcher` | plugin-root relative command plus args after envelope allow | Store plugin id, command name, manifest digest, command path digest, args digest, redacted stdin digest. No raw stdin persistence. | Plugin discovery and activation remain Spec 025 baseline, app extension lifecycle remains Spec 032. |
| `plugin_tool` | `PluginRuntimeTool` registered in `ToolRegistry` | `shacs-core::runtime::plugin_tool_adapter` | plugin-root relative command with JSON stdin after envelope allow | Store tool name, argument digest, redacted output summary. No raw argument JSON persistence. | Tool registry semantics remain Spec 004 baseline. |
| `plugin_hook` | `ProcessPluginHookCommandExecutor` | `shacs-core::runtime::plugin_hook_adapter` | plugin-root relative hook command after envelope allow | Store event, plugin id, context preview digest, redacted stdout and stderr summaries. Hook output is data only. | Hook behavior and block-only consumption remain Spec 025 baseline. |
| `mcp_stdio` | `StdioMcpConnector` in `tools/mcp.rs` | `shacs-core::tools::mcp_stdio_connector` | command plus args after envelope allow, before JSON-RPC initialize | Store server name, transport kind, command digest, args digest, env ref digest, containment ref. No raw env or protocol payload persistence. | MCP protocol semantics and capability registry remain MCP runtime owner scope. |
| `app_process` | Current `AppProcessSnapshot` is projection only | `shacs-app-supervisor::app_process_adapter` when Spec 032 implements it | AppSupervisor delegates spawn only after envelope allow | Store app id, process id, manifest digest, entrypoint ref, secret ref ids, receipt id. No app raw env. | AppSupervisor lifecycle and app state remain Spec 032. |
| `dependency_preparation` | Future skill trust and dependency path | `shacs-skill-trust::dependency_preparation_adapter` | Package manager or local verifier command only after envelope allow | Store trust record ref, dependency manifest digest, package ref, capability digest, receipt id. No installer stdout raw dump. | Installer behavior and storage layout remain Specs 032 and 035. |
| `verified_entrypoint` | Future verified skill entrypoint path | `shacs-skill-trust::verified_entrypoint_adapter` | Verified entrypoint command only after trust and envelope allow | Store source identity, content digest, dependency digest, entrypoint digest, redacted args and env refs. | Entry point runner lifecycle remains Specs 032 and 035. |

Every adapter above must have one boundary function that accepts `ProcessExecutionEnvelope` and returns `ProcessExecutionReceipt`. Direct process creation outside that boundary is a bypass and must be rejected by tests or code review gate.

## Required Gate Order

The only valid launch order is:

1. Normalize the adapter candidate into `ProcessExecutionEnvelope` using typed inputs, redacted argument and env projections, `PolicySafetySnapshotRef`, `SecretRef`, containment intent, timeout policy, and owner adapter boundary.
2. Evaluate static policy using the envelope's `PermissionedAction`, protected target rules, proc exec summary, raw credential export rules, containment state, and malformed input state.
3. Evaluate approval and ceiling. Static deny and ceiling violation win over classifier allow or plugin hook text. Ask requires valid approval lineage before spawn. Missing, stale, consumed, wrong-scope, inspect-only, mismatched digest, or expired approval rejects.
4. Spawn only through the owner adapter boundary, after all checks pass. Just-in-time secret resolution may happen inside this boundary only when the envelope carries matching `SecretRef` ids.
5. Write one redacted receipt. The receipt records success, failure, timeout, cancellation, or rejection using safe summaries, digests, refs, and redaction evidence only.

No adapter may combine steps 1 through 3 with spawn in the same untyped helper. The typed envelope must exist before any process handle is created.

## Timeout and Cancellation

1. Every envelope must carry a timeout. `timeout_ms` must be greater than zero and no greater than the adapter cap.
2. Timeout produces `ProcessReceiptOutcome::timed_out`, records redacted stdout and stderr summaries available before cleanup, and records cleanup disposition.
3. Cancellation produces `ProcessReceiptOutcome::cancelled` when the user, turn lock, runtime stop request, or future AppSupervisor cancel token cancels before terminal process status.
4. `cancel_resume` is future behavior. If cancellation happens after spawn and before receipt persistence, recovery must emit either `cancelled` with cleanup evidence or `recovery_needed` for the external owner. It must never assume success from missing receipt.
5. `repeated_interruptions` must have deterministic tests. A second cancel or interrupt for the same envelope id must be idempotent and must not spawn a duplicate process.
6. Late process completion after timeout or cancellation must not overwrite the terminal receipt as success. It may add redacted diagnostic evidence if the external owner supports it.
7. Tests and QA must not use sleeps as proof. They must use a deterministic fake process driver, controllable clock, process completion signal, or bounded probe with explicit timeout.

## Replay Non-Execution

1. Replay must read `ProcessExecutionEnvelope` refs, `ProcessExecutionReceipt` refs, safe mock outcomes, and diagnostics only.
2. Replay must not call `Command::new`, resolve `SecretRef`, start MCP stdio, run plugin commands, start app processes, prepare dependencies, or invoke verified entrypoints.
3. Replay of a missing receipt must produce `replay_rejected` or blocked replay evidence. It must not live-dispatch to fill the gap.
4. Replay may compare schema ids, action digests, policy safety digests, receipt ids, outcome codes, and redacted summary digests.
5. Replay of prompt, plugin, skill, MCP, stdout, or stderr text must treat those values as data only. Text cannot raise permission mode, widen ceiling, or authorize a launch.

## Normal Sequences

### Exec

1. Provider proposes an `exec` tool call.
2. Runtime normalizes it into `PermissionedAction` and `ProcessExecutionEnvelope` with cwd scope, command digest, redacted argument digest, env projection, policy snapshot ref, and timeout.
3. Static policy checks dangerous patterns, proc exec summary, protected targets, containment evidence, and malformed input.
4. Approval and ceiling check pass.
5. `ExecTool` receives the envelope and spawns the command.
6. Runtime stores one redacted receipt with exit status, stdout summary, stderr summary, action id, policy digest, and receipt id.

### Plugin Command or Tool

1. Enabled plugin produces a command, tool, or hook candidate from manifest entrypoint data.
2. Runtime normalizes plugin id, manifest digest, command path digest, args digest, redacted stdin digest, event or tool name, policy snapshot ref, and timeout into an envelope.
3. Static policy rejects disabled, blocked, undeclared, unsafe path, malformed input, and missing policy snapshot cases before spawn.
4. Approval and ceiling pass.
5. Plugin adapter spawns the plugin-root relative executable and writes bounded stdin.
6. Runtime stores a redacted receipt. Hook output remains observer or block data and cannot approve a permission decision.

### MCP Stdio

1. Runtime loads an MCP stdio server declaration.
2. Before startup, it normalizes server name, command digest, args digest, enabled capability summary, env ref digest, containment evidence ref, policy snapshot ref, and timeout into an envelope.
3. Static policy and ceiling evaluate the stdio startup as proc exec. Unknown containment follows ask or deny policy and is not safe evidence.
4. Adapter spawns the server only after allow.
5. JSON-RPC initialize and capability listing happen after spawn and are outside this PRD's authorization semantics.
6. Startup receipt records connected or failed startup with redacted error summaries and registered count refs.

### App Process

1. Spec 032 AppSupervisor receives an app start request.
2. It asks the process gate to normalize app id, manifest digest, entrypoint ref, workspace scope, secret refs, permission snapshot ref, containment intent, and timeout into an envelope.
3. Static policy, approval, ceiling, and PRD 003 containment proof run before process creation.
4. AppSupervisor owns actual lifecycle, but it may spawn only after an allowed envelope returns from the gate.
5. App process receipt carries the envelope id, app process id, manifest digest, secret ref ids, and redacted outcome. AppSupervisor may add lifecycle receipt fields under Spec 032.

### Dependency Preparation and Verified Entrypoint

1. PRD 005 supplies active trust provenance for source identity, content digest, dependency manifest digest, capability scope, and lifecycle status.
2. Runtime normalizes preparation or entrypoint action into an envelope with trust refs, dependency refs, executable or entrypoint refs, policy snapshot ref, redacted args and env, timeout, and receipt correlation.
3. Static policy and ceiling run before package manager, verifier, or entrypoint command starts.
4. Missing runtime prerequisite, manifest-outside package, lifecycle script, native build, or digest mismatch rejects before spawn.
5. Receipt records preparation or entrypoint result as redacted evidence only. Installer details remain external owner data.

## Failure Sequences

1. Malformed input reaches envelope boundary, such as unknown schema, missing adapter, empty executable ref, unsupported containment intent, invalid cwd scope, raw secret field, raw env map, missing timeout, or unknown owner adapter. Parser returns `malformed_input` and no process starts.
2. Static policy denies raw credential export, protected target access, dangerous proc exec without summary, unknown unsafe containment, disabled plugin, undeclared plugin command, app unavailable state, or trust mismatch. Receipt outcome is `policy_denied` or adapter-specific `spawn_rejected` and no process starts.
3. Approval is required but no valid lineage exists. Receipt outcome is `approval_required`; interactive caller may ask, non-interactive caller denies.
4. Approval exists but action digest, policy safety digest, snapshot id, scope, expiry, consumed state, or secret ref staleness does not match. Receipt outcome is `approval_rejected` and no process starts.
5. Capability request exceeds inherited ceiling. Receipt outcome is `ceiling_violation` and no process starts.
6. `BypassPermissions` is requested while containment proof is missing, unknown, unsafe, or wider than parent. Receipt outcome is `containment_mismatch` or `policy_denied` and no process starts.
7. Adapter spawn fails after allow. Receipt outcome is `failed`, with safe error code and redacted stderr summary.
8. Process exceeds timeout. Receipt outcome is `timed_out`, cleanup evidence is recorded, and late success cannot overwrite it.
9. User or runtime cancels after spawn. Receipt outcome is `cancelled`, and resume must not spawn a second process for the same envelope id.
10. Replay attempts live dispatch. Receipt outcome is `replay_rejected`, and live dispatch count remains zero.

## Bypass Rejection

Implementation must reject these bypasses:

1. Calling `Command::new` or equivalent from exec, plugin, MCP, app, dependency, or entrypoint code without a `ProcessExecutionEnvelope`.
2. Starting an MCP stdio server from config projection or plugin MCP declaration before the envelope gate.
3. Letting `PluginCommandDispatcher`, `PluginRuntimeTool`, or `ProcessPluginHookCommandExecutor` spawn from manifest data without the envelope gate.
4. Letting AppSupervisor own permission truth or spawn without a successful envelope decision.
5. Letting package preparation or verified entrypoints run from skill name, Markdown body, or dependency text without active exact-match trust provenance and envelope allow.
6. Persisting raw args, raw env, full env maps, process handles, raw stdout, raw stderr, absolute host paths, or raw secret values in envelopes, receipts, replay input, diagnostics, or release evidence.

## TDD Sequence

Future implementation must follow this order.

1. Baseline characterization. Add or confirm tests for current exec permission normalization, plugin command/tool/hook bounded process behavior, MCP stdio env clearing, replay live-dispatch rejection, app ledger redaction, and current approval mismatch behavior.
2. Failing-first proof for envelope boundary. Add failing tests named `process_envelope_rejects_malformed_input`, `process_envelope_rejects_raw_args_and_env_persistence`, `process_envelope_requires_policy_safety_ref`, and `process_envelope_requires_owner_adapter`.
3. Failing-first proof for gate order. Add tests named `process_gate_static_deny_runs_before_spawn`, `process_gate_approval_and_ceiling_run_before_spawn`, `process_gate_bypass_permissions_requires_containment_proof`, and `process_gate_never_spawns_on_denied_policy`.
4. Failing-first proof for receipts. Add tests named `process_receipt_success_is_redacted`, `process_receipt_failure_is_redacted`, `process_receipt_timeout_is_terminal`, `process_receipt_cancel_resume_is_idempotent`, and `process_receipt_repeated_interruptions_do_not_duplicate_spawn`.
5. Failing-first proof for replay. Add tests named `process_replay_does_not_dispatch_exec`, `process_replay_does_not_start_mcp_stdio`, `process_replay_does_not_run_plugin_command`, `process_replay_does_not_start_app_process`, and `process_replay_does_not_resolve_secret_ref`.
6. Failing-first proof for prompt injection. Add tests proving untrusted user prompt, plugin output, skill body, MCP prompt text, stdout, and stderr remain data only and cannot approve, raise mode, widen ceiling, or alter owner adapter.
7. Minimal implementation. Add the typed envelope, parser, redacted projections, receipt type, gate service, and adapter boundary shims. Do not add AppSupervisor lifecycle, installer behavior, MCP protocol changes, or physical snapshot storage under Spec 030.
8. Refactor only after focused tests pass. Remove duplicate direct spawn paths or route them through owner adapter boundaries. Keep adapter behavior unchanged except for gate admission and redacted receipt emission.
9. Final regression. Run focused Cargo commands, workspace gates, real process QA, and evidence review before PRD 006 may consume this PRD.

Tests must assert typed fields, enum values, digest equality or inequality, spawn counters, receipt outcomes, and absence of raw fixture values. Tests must not assert natural-language prompt prose.

## Focused Cargo Targets and Commands

Future implementation must use the workspace manifest explicitly.

```sh
cargo fmt --manifest-path crates/Cargo.toml --all -- --check
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core process_envelope
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core process_gate
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core plugin_runtime
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core mcp
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-app app_environment
cargo clippy --manifest-path crates/Cargo.toml --locked -p shacs-core --all-targets -- -D warnings
```

Before closure, run the workspace gates from `AGENTS.md`:

```sh
cargo fmt --manifest-path crates/Cargo.toml --all -- --check
cargo clippy --manifest-path crates/Cargo.toml --locked --workspace --all-targets -- -D warnings
cargo test --manifest-path crates/Cargo.toml --locked --workspace
```

No authoring worker for this documentation task should run Cargo. The commands above are future implementation gates.

## Agent-Executed Real Process QA

The implementation worker must use isolated temp workspaces and literal commands. Each probe must have a bounded timeout and deterministic completion signal. Hung commands must fail by timeout evidence, not by unbounded waiting. Flaky tests must not use sleeps as synchronization. Every QA item below must append a cleanup receipt to `docs/specs/030-policy-permission-redaction-and-containment-model/evidence/prd002/process-envelope-cleanup-ledger.jsonl` with `qa_id`, `fixture_owner`, `workspace_removed`, `artifact_refs`, and `raw_value_scan_passed`.

Common setup for current in-repo adapter families:

```sh
qa_root="$(mktemp -d /tmp/shacs-prd002-qa.XXXXXX)"
workspace="$qa_root/workspace"
evidence_dir="$qa_root/evidence"
mkdir -p "$workspace" "$evidence_dir"
```

Common cleanup command after each current-family probe:

```sh
rm -rf "$qa_root"
```

1. Exec adapter happy path.

Fixture owner: PRD 002 implementation worker. Fixture locator: `crates/shacs-core/tests/fixtures/process_envelope/exec_pwd.json`. Deterministic creation command if the fixture is missing:

```sh
mkdir -p crates/shacs-core/tests/fixtures/process_envelope
printf '{"qa_id":"prd002-exec-pwd","command":"pwd","timeout_ms":5000,"cwd":"workspace"}\n' > crates/shacs-core/tests/fixtures/process_envelope/exec_pwd.json
```

Setup command:

```sh
printf 'fixture=prd002-exec-pwd\n' > "$workspace/README.txt"
```

Invocation:

```sh
cargo run --manifest-path crates/Cargo.toml --locked -p shacs-cli -- ask "Use the exec tool to run pwd once for PRD002 fixture prd002-exec-pwd." --workspace "$workspace"
```

Expected artifact: `$evidence_dir/prd002-exec-pwd-receipt.json` containing `qa_id=prd002-exec-pwd`, `adapter=exec`, `outcome=succeeded`, one envelope id, one receipt id, one action id, one policy safety digest, one cwd scope digest, one stdout summary digest, and no raw env map.

PASS/FAIL: PASS only if the receipt exists, outcome is `succeeded`, `raw_value_scan_passed=true`, and cleanup ledger contains a matching `qa_id`. FAIL if the command output alone says success but the receipt is missing, raw env appears, or cleanup ledger is missing.

Cleanup receipt linkage: append `cleanup_for=prd002-exec-pwd` to `process-envelope-cleanup-ledger.jsonl` before deleting `qa_root`.

2. Exec adapter timeout path.

Fixture owner: PRD 002 implementation worker. Fixture locator: `crates/shacs-core/tests/fixtures/process_envelope/exec_timeout.json`. Deterministic creation command if the fixture is missing:

```sh
mkdir -p crates/shacs-core/tests/fixtures/process_envelope
printf '{"qa_id":"prd002-exec-timeout","command":"sleep 2","timeout_ms":50,"cwd":"workspace"}\n' > crates/shacs-core/tests/fixtures/process_envelope/exec_timeout.json
```

Setup command:

```sh
printf 'fixture=prd002-exec-timeout\n' > "$workspace/README.txt"
```

Invocation:

```sh
cargo run --manifest-path crates/Cargo.toml --locked -p shacs-cli -- ask "Use the exec tool to run the bounded timeout fixture prd002-exec-timeout." --workspace "$workspace"
```

Expected artifact: `$evidence_dir/prd002-exec-timeout-receipt.json` containing `adapter=exec`, `outcome=timed_out`, cleanup disposition, elapsed bound evidence, and `late_success_overwrite=false`.

PASS/FAIL: PASS only if terminal outcome is `timed_out` and no later success receipt for the same envelope id exists. FAIL on missing receipt, success overwrite, unbounded wait, or sleep-based test synchronization.

Cleanup receipt linkage: append `cleanup_for=prd002-exec-timeout` to `process-envelope-cleanup-ledger.jsonl`.

3. Plugin command adapter.

Fixture owner: Spec 025 plugin runtime baseline plus PRD 002 gate implementation. Fixture locator: `crates/shacs-core/tests/fixtures/process_envelope/plugin_command/prd002-command-plugin/plugin.json`. Deterministic creation command if the fixture is missing:

```sh
mkdir -p crates/shacs-core/tests/fixtures/process_envelope/plugin_command/prd002-command-plugin
printf '#!/bin/sh\nprintf "{\\"ok\\":true,\\"adapter\\":\\"plugin_command\\"}\\n"\n' > crates/shacs-core/tests/fixtures/process_envelope/plugin_command/prd002-command-plugin/run-command.sh
chmod 755 crates/shacs-core/tests/fixtures/process_envelope/plugin_command/prd002-command-plugin/run-command.sh
printf '{"name":"prd002-command-plugin","version":"0.1.0","description":"PRD002 command fixture","surfaces":{"commands":["prd002_cmd"]},"entrypoints":{"commands":{"prd002_cmd":{"command":"run-command.sh","args":[],"timeoutMs":1000}}}}\n' > crates/shacs-core/tests/fixtures/process_envelope/plugin_command/prd002-command-plugin/plugin.json
```

Setup command:

```sh
mkdir -p "$workspace/.shacs-bot/plugins"
cp -R crates/shacs-core/tests/fixtures/process_envelope/plugin_command/prd002-command-plugin "$workspace/.shacs-bot/plugins/prd002-command-plugin"
cargo run --manifest-path crates/Cargo.toml --locked -p shacs-cli -- plugins enable prd002-command-plugin --workspace "$workspace"
```

Invocation:

```sh
cargo run --manifest-path crates/Cargo.toml --locked -p shacs-cli -- ask "/prd002_cmd" --workspace "$workspace"
```

Expected artifact: `$evidence_dir/prd002-plugin-command-receipt.json` containing `adapter=plugin_command`, `plugin_id=prd002-command-plugin`, manifest digest, command path digest, args digest, redacted stdin digest, envelope id, receipt id, and cleanup ref.

PASS/FAIL: PASS only if the command-backed process spawned through the owner adapter boundary and no raw stdin payload persists. FAIL if builtin command routing is overridden, the plugin command spawns without envelope id, or raw stdin appears in any artifact.

Cleanup receipt linkage: append `cleanup_for=prd002-plugin-command` to `process-envelope-cleanup-ledger.jsonl`.

4. Plugin command-backed tool adapter.

Fixture owner: Spec 025 plugin runtime baseline plus PRD 002 gate implementation. Fixture locator: `crates/shacs-core/tests/fixtures/process_envelope/plugin_tool/prd002-tool-plugin/plugin.json`. Deterministic creation command if the fixture is missing:

```sh
mkdir -p crates/shacs-core/tests/fixtures/process_envelope/plugin_tool/prd002-tool-plugin
printf '#!/bin/sh\ncat >/dev/null\nprintf "{\\"content\\":[{\\"type\\":\\"text\\",\\"text\\":\\"tool-ok\\"}]}\\n"\n' > crates/shacs-core/tests/fixtures/process_envelope/plugin_tool/prd002-tool-plugin/run-tool.sh
chmod 755 crates/shacs-core/tests/fixtures/process_envelope/plugin_tool/prd002-tool-plugin/run-tool.sh
printf '{"name":"prd002-tool-plugin","version":"0.1.0","description":"PRD002 tool fixture","surfaces":{"tools":["prd002_plugin_tool"]},"entrypoints":{"tools":{"prd002_plugin_tool":{"command":"run-tool.sh","args":[],"timeoutMs":1000,"parameters":{"type":"object","properties":{"message":{"type":"string"}},"required":["message"]}}}}}\n' > crates/shacs-core/tests/fixtures/process_envelope/plugin_tool/prd002-tool-plugin/plugin.json
```

Setup command:

```sh
mkdir -p "$workspace/.shacs-bot/plugins"
cp -R crates/shacs-core/tests/fixtures/process_envelope/plugin_tool/prd002-tool-plugin "$workspace/.shacs-bot/plugins/prd002-tool-plugin"
cargo run --manifest-path crates/Cargo.toml --locked -p shacs-cli -- plugins enable prd002-tool-plugin --workspace "$workspace"
```

Invocation:

```sh
cargo run --manifest-path crates/Cargo.toml --locked -p shacs-cli -- ask "Call tool prd002_plugin_tool with message prd002-tool." --workspace "$workspace"
```

Expected artifact: `$evidence_dir/prd002-plugin-tool-receipt.json` containing `adapter=plugin_tool`, `tool_name=prd002_plugin_tool`, argument digest, redacted stdin digest, redacted output summary digest, envelope id, and receipt id.

PASS/FAIL: PASS only if the tool goes through `ToolRegistry`, process envelope id exists before spawn, and raw argument JSON is absent from persisted artifacts. FAIL if the plugin tool bypasses tool runtime, lacks owner adapter boundary, or persists raw arguments.

Cleanup receipt linkage: append `cleanup_for=prd002-plugin-tool` to `process-envelope-cleanup-ledger.jsonl`.

5. Plugin hook adapter.

Fixture owner: Spec 025 plugin hook baseline plus PRD 002 gate implementation. Fixture locator: `crates/shacs-core/tests/fixtures/process_envelope/plugin_hook/prd002-hook-plugin/plugin.json`. Deterministic creation command if the fixture is missing:

```sh
mkdir -p crates/shacs-core/tests/fixtures/process_envelope/plugin_hook/prd002-hook-plugin
printf '#!/bin/sh\ncat >/dev/null\nprintf "{\\"decision\\":\\"block\\",\\"reason\\":\\"prd002 hook fixture\\"}\\n"\n' > crates/shacs-core/tests/fixtures/process_envelope/plugin_hook/prd002-hook-plugin/run-hook.sh
chmod 755 crates/shacs-core/tests/fixtures/process_envelope/plugin_hook/prd002-hook-plugin/run-hook.sh
printf '{"name":"prd002-hook-plugin","version":"0.1.0","description":"PRD002 hook fixture","surfaces":{"hooks":["tool:before"]},"entrypoints":{"hooks":{"tool:before":{"command":"run-hook.sh","args":[],"timeoutMs":1000}}}}\n' > crates/shacs-core/tests/fixtures/process_envelope/plugin_hook/prd002-hook-plugin/plugin.json
```

Setup command:

```sh
mkdir -p "$workspace/.shacs-bot/plugins"
cp -R crates/shacs-core/tests/fixtures/process_envelope/plugin_hook/prd002-hook-plugin "$workspace/.shacs-bot/plugins/prd002-hook-plugin"
cargo run --manifest-path crates/Cargo.toml --locked -p shacs-cli -- plugins enable prd002-hook-plugin --workspace "$workspace"
```

Invocation:

```sh
cargo run --manifest-path crates/Cargo.toml --locked -p shacs-cli -- ask "Ask for a harmless read-only tool call so tool:before hook prd002-hook-plugin runs." --workspace "$workspace"
```

Expected artifact: `$evidence_dir/prd002-plugin-hook-receipt.json` containing `adapter=plugin_hook`, `event=tool:before`, `plugin_id=prd002-hook-plugin`, envelope id, receipt id, block effect evidence, and redacted stdout summary digest.

PASS/FAIL: PASS only if hook output is recorded as data and cannot create approval, allow, grant, mode raise, or ceiling widening. FAIL if hook text changes authorization or replay live-dispatches the hook.

Cleanup receipt linkage: append `cleanup_for=prd002-plugin-hook` to `process-envelope-cleanup-ledger.jsonl`.

6. MCP stdio adapter.

Fixture owner: MCP runtime adapter plus PRD 002 gate implementation. Fixture locator: `crates/shacs-core/tests/fixtures/process_envelope/mcp_stdio/prd002-mcp-server.sh`. Deterministic creation command if the fixture is missing:

```sh
mkdir -p crates/shacs-core/tests/fixtures/process_envelope/mcp_stdio
printf '#!/bin/sh\nwhile IFS= read -r line; do case "$line" in Content-Length:*) break;; esac; done\nprintf "Content-Length: 77\\r\\n\\r\\n{\\"jsonrpc\\":\\"2.0\\",\\"id\\":1,\\"result\\":{\\"protocolVersion\\":\\"2024-11-05\\",\\"capabilities\\":{}}}"\n' > crates/shacs-core/tests/fixtures/process_envelope/mcp_stdio/prd002-mcp-server.sh
chmod 755 crates/shacs-core/tests/fixtures/process_envelope/mcp_stdio/prd002-mcp-server.sh
```

Setup command:

```sh
mkdir -p "$workspace/.shacs-bot"
printf '{"tools":{"mcpServers":{"prd002_stdio":{"type":"stdio","command":"%s","args":[],"env":{"SHACS_ALLOWED":"1"},"clearEnv":true,"enabledTools":["*"]}}}}\n' "$(pwd)/crates/shacs-core/tests/fixtures/process_envelope/mcp_stdio/prd002-mcp-server.sh" > "$workspace/.shacs-bot/config.json"
```

Invocation:

```sh
cargo run --manifest-path crates/Cargo.toml --locked -p shacs-cli -- runtime inspect --workspace "$workspace"
```

Expected artifact: `$evidence_dir/prd002-mcp-stdio-startup-receipt.json` containing `adapter=mcp_stdio`, `server_name=prd002_stdio`, command digest, args digest, env ref digest, startup envelope id, startup receipt id, and registered capability count or safe startup error.

PASS/FAIL: PASS only if stdio startup is gated before JSON-RPC initialize and raw env values are absent. FAIL if MCP starts during inspect-only plugin or hooks management, starts without envelope id, or persists raw env/protocol payload.

Cleanup receipt linkage: append `cleanup_for=prd002-mcp-stdio` to `process-envelope-cleanup-ledger.jsonl`.

7. App process adapter external dependency.

Fixture owner: Spec 032 AppSupervisor implementation, consumed by PRD 002. Fixture locator: `docs/specs/032-app-maker-runtime-and-extension-lifecycle/evidence/prd002/app-process-envelope-qa.json`. Setup command after Spec 032 provides the fixture:

```sh
test -f docs/specs/032-app-maker-runtime-and-extension-lifecycle/evidence/prd002/app-process-envelope-qa.json
```

Invocation: no current CLI invocation is authorized by this PRD. The future Spec 032 implementation evidence must name its real command in the locator above and must include `app_id=prd002.local.echo`, `manifest_digest`, `appsupervisor_receipt_id`, `process_envelope_id`, and `process_receipt_id`.

Expected artifact: `docs/specs/032-app-maker-runtime-and-extension-lifecycle/evidence/prd002/app-process-envelope-qa.json` plus PRD 002 receipt projection at `docs/specs/030-policy-permission-redaction-and-containment-model/evidence/prd002/app-process-receipt.json`.

PASS/FAIL: PASS only if the Spec 032 artifact exists, names the actual app start invocation, proves AppSupervisor consumed an allowed PRD 002 envelope before spawn, and includes a redacted app process receipt. BLOCKED if the Spec 032 artifact is missing or lacks `process_envelope_id`. FAIL if AppSupervisor owns permission truth, starts without envelope allow, or persists raw app env.

Cleanup receipt linkage: Spec 032 artifact must include `cleanup_receipt_ref=docs/specs/030-policy-permission-redaction-and-containment-model/evidence/prd002/process-envelope-cleanup-ledger.jsonl#prd002-app-process`.

8. Dependency preparation adapter external dependency.

Fixture owner: PRD 005 trust provenance implementation with Spec 032 lifecycle and Spec 035 persistence evidence. Fixture locator: `docs/specs/030-policy-permission-redaction-and-containment-model/evidence/prd005/dependency-preparation-prd002-handoff.json`. Setup command after PRD 005 provides the fixture:

```sh
test -f docs/specs/030-policy-permission-redaction-and-containment-model/evidence/prd005/dependency-preparation-prd002-handoff.json
```

Invocation: no current CLI invocation is authorized by this PRD. The PRD 005 handoff artifact must name the real dependency preparation command and must include `skill_ref=skill://prd002/local-prep`, active trust record ref, source digest, dependency manifest digest, capability digest, envelope id, receipt id, and Spec 035 persistence ref.

Expected artifact: PRD 005 handoff plus PRD 002 receipt projection at `docs/specs/030-policy-permission-redaction-and-containment-model/evidence/prd002/dependency-preparation-receipt.json`.

PASS/FAIL: PASS only if exact active trust provenance is present and the dependency preparation process is gated by PRD 002 before spawn. BLOCKED if PRD 005 or Spec 035 evidence is absent. FAIL if skill name/body authorizes install, a manifest-outside dependency runs, runtime prerequisite installation is inferred from package trust, or installer stdout persists raw.

Cleanup receipt linkage: PRD 005 handoff must include `cleanup_receipt_ref=docs/specs/030-policy-permission-redaction-and-containment-model/evidence/prd002/process-envelope-cleanup-ledger.jsonl#prd002-dependency-preparation`.

9. Verified entrypoint adapter external dependency.

Fixture owner: PRD 005 verified entrypoint implementation with Spec 032 lifecycle and Spec 035 persistence evidence. Fixture locator: `docs/specs/030-policy-permission-redaction-and-containment-model/evidence/prd005/verified-entrypoint-prd002-handoff.json`. Setup command after PRD 005 provides the fixture:

```sh
test -f docs/specs/030-policy-permission-redaction-and-containment-model/evidence/prd005/verified-entrypoint-prd002-handoff.json
```

Invocation: no current CLI invocation is authorized by this PRD. The PRD 005 handoff artifact must name the real verified entrypoint command and must include `skill_ref=skill://prd002/local-entrypoint`, active trust record ref, source digest, content digest, dependency digest, entrypoint digest, envelope id, receipt id, and Spec 035 persistence ref.

Expected artifact: PRD 005 handoff plus PRD 002 receipt projection at `docs/specs/030-policy-permission-redaction-and-containment-model/evidence/prd002/verified-entrypoint-receipt.json`.

PASS/FAIL: PASS only if exact active trust provenance is present and stale, revoked, removed, or digest-mismatched trust is rejected before spawn. BLOCKED if PRD 005 or Spec 035 evidence is absent. FAIL if skill text, MCP prompt text, stdout, or stderr changes authorization, owner adapter, permission mode, or ceiling.

Cleanup receipt linkage: PRD 005 handoff must include `cleanup_receipt_ref=docs/specs/030-policy-permission-redaction-and-containment-model/evidence/prd002/process-envelope-cleanup-ledger.jsonl#prd002-verified-entrypoint`.

## Agent-Generated Artifact-Backed Read Audit

Before PRD 006 may count this PRD as closed, an agent must generate `docs/specs/030-policy-permission-redaction-and-containment-model/evidence/prd002/process-envelope-read-audit.json` by reading every artifact named in the QA section above.

The audit must contain one row per `qa_id` with these fields: `artifact_locator`, `artifact_read`, `envelope_id_present`, `receipt_id_present`, `owner_adapter_present`, `policy_safety_ref_present`, `raw_args_absent`, `raw_env_absent`, `raw_stdout_absent`, `raw_stderr_absent`, `raw_secret_absent`, `replay_live_dispatch_count`, `appsupervisor_external`, `external_dependency_state`, `cleanup_receipt_ref`, and `verdict`.

Binary criteria:

1. PASS if every current-family row has `artifact_read=true`, envelope and receipt ids present, owner adapter present, policy safety ref present, all raw-value absence fields true, `replay_live_dispatch_count=0`, and cleanup receipt ref present.
2. PASS for external rows only when the named 032, 035, or PRD 005 evidence exists and `external_dependency_state=ready` while preserving ownership boundaries.
3. BLOCKED if an external owner artifact is missing, incomplete, or does not name its real invocation. Blocked external rows must prevent PRD 006 closure but must not be reported as PRD 002 success.
4. FAIL if any artifact contains raw args, raw env, raw stdout, raw stderr, raw secret values, process handles, absolute host paths, direct spawn bypass evidence, replay live dispatch, AppSupervisor permission ownership, or authorization from prompt/plugin/skill/MCP/stdout/stderr text.

## Adversarial Requirements

| Scenario | Required handling |
|---|---|
| `malformed_input` | Applies at envelope parse boundary. Unknown schema, missing required field, raw arg/env field, invalid cwd, missing timeout, unknown adapter, or unsupported containment intent must reject before static policy or spawn. |
| `cancel_resume` | Applies to future timeout and cancellation contract. Recovery after cancel must produce terminal cancelled evidence or recovery-needed evidence, never assumed success or duplicate spawn. |
| `repeated_interruptions` | Applies to future timeout and cancellation contract. Repeated cancel or interrupt for the same envelope id must be idempotent. |
| `stale_state` | Current authoring and future implementation must re-read current specs and Rust surfaces before claiming behavior. Stale snapshot or stale approval rejects. |
| `dirty_worktree` | Current authoring must verify only requested files changed. Future implementation must isolate fixture workspaces and avoid unrelated source edits. |
| `misleading_success_output` | Process stdout saying success is not proof. Receipt outcome depends on adapter status, policy evidence, and terminal receipt fields. |
| `hung_commands` | Future real process QA must use bounded probes and timeout receipts. No unbounded wait is allowed. |
| `flaky_tests` | Tests must use deterministic fake drivers, controllable clocks, completion signals, or bounded process probes. Sleeps are not proof. |
| `prompt_injection` | User prompt, plugin text, skill body, MCP prompt, stdout, and stderr are untrusted data. They cannot authorize, raise mode, widen ceiling, or select owner adapter. |

## Closure Evidence

PRD 006 may count this PRD as closed only when implementation evidence includes:

1. Baseline characterization test output for current exec, plugin, MCP, replay, approval, and app receipt boundaries.
2. Failing-first history for envelope malformed input, raw persistence rejection, missing policy snapshot ref, gate ordering, bypass rejection, timeout, cancellation, replay non-execution, and prompt injection data-only behavior.
3. Typed Rust contracts for `ProcessExecutionEnvelope`, `ProcessExecutionReceipt`, adapter refs, owner adapter boundary, redacted projections, timeout policy, cancellation policy, and receipt outcomes.
4. Adapter matrix implementation evidence proving each supported process family enters through one envelope gate before spawn.
5. Focused Cargo commands and workspace gates passing with `--manifest-path crates/Cargo.toml`.
6. Real process QA artifacts for exec, plugin, MCP, app process, dependency preparation, and verified entrypoint families, with unavailable future adapters marked blocked on 032 or PRD 005 evidence rather than skipped as success.
7. A structured audit proving raw args, raw env, raw stdout, raw stderr, process handles, absolute host paths, and raw secret values do not persist.
8. Owner handoff evidence showing AppSupervisor lifecycle, MCP semantics, installer behavior, and physical snapshot persistence remain external.

## Exit Criteria

1. Every supported side-effect launch has a typed envelope before spawn and a redacted receipt after terminal outcome or rejection.
2. Gate order is enforced exactly as normalize, static policy, approval and ceiling, spawn, redacted receipt.
3. Static deny, ceiling violation, malformed input, stale approval, missing containment proof for bypass, raw persistence, and replay live dispatch all reject before spawn.
4. Timeout and cancellation are terminal receipt outcomes with deterministic resume behavior.
5. Raw args and raw env never persist.
6. AppSupervisor lifecycle, MCP protocol semantics, dependency installer behavior, and physical snapshot persistence are not owned by Spec 030.
7. PRD 003, PRD 005, and PRD 006 can consume envelope ids, policy snapshot refs, containment refs, secret refs, and receipt ids without redefining process admission.
