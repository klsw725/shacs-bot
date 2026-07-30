# PRD 003. containment inheritance and permission ceiling proof

Status: Planned

## Goal

This PRD defines the proof contract that decides whether a child boundary may execute under parent containment and permission limits. The proof must show that the child is equal to or narrower than the parent before a process envelope is admitted.

The proof is evidence for policy decisions. It is not kernel isolation. Docker, Compose, native detection, app state, plugin metadata, MCP metadata, and diagnostics output can supply evidence, but none of them can create permission or widen a ceiling by themselves.

Unknown or unsafe containment is never safe. It must become ask or deny according to the caller, permission mode, static rules, and process envelope intent.

## Scope

1. Define typed parent-child containment lineage, snapshot linkage, and proof refs consumed by PRD 002 process envelopes.
2. Define non-widening comparison for containment state, workspace scope, permission mode, and capability ceiling.
3. Define confirmed, unknown, and unsafe containment states and their admission effects.
4. Define stale, mismatch, missing evidence, malformed input, and prompt injection rejection semantics.
5. Define process-envelope admission rules for subagent, MCP stdio, app process, plugin command/tool/hook, dependency preparation, verified entrypoint, and deferred bridge boundaries.
6. Define diagnostics projection input and handoff to Spec 031 without owning any UI, release, or adapter rendering.
7. Define TDD order, focused Cargo commands, workspace gates, and real-surface QA for the future implementation.

## Non-Scope

1. This PRD does not claim kernel isolation, complete sandboxing, host escape prevention, or container security proof.
2. This PRD does not own Dockerfile, Compose, bwrap, native host detector, or packaged sandbox implementation. Spec 023 provides the current containment evidence baseline.
3. This PRD does not own UI projection, diagnostics rendering, release artifact rendering, or parity adapters. Spec 031 owns those outputs.
4. This PRD does not own `ProcessExecutionEnvelope` or spawn receipt shape. PRD 002 owns that process contract and consumes this proof.
5. This PRD does not own AppSupervisor lifecycle, app start, app stop, app recover, trust lifecycle, dependency installer, verified entrypoint runner, or physical snapshot storage. Specs 032 and 035 own those surfaces.
6. This PRD does not make user prompt text, child metadata, process stdout, plugin hook output, MCP prompt text, app manifest prose, or skill body text an authorization source.
7. This PRD does not require organization policy, fleet rollout, hosted approval service, or a central vault.

## SPEC Inputs

1. Spec 030 `소유하는 open scope` requires containment evidence and unknown containment handling as Spec 030 owned policy meaning.
2. Spec 030 `Implementation PRDs` assigns PRD 003 as the owner for containment inheritance and permission-ceiling proof, while PRD 000 and PRD 002 supply snapshot and process inputs.
3. Spec 030 `Stronger Contract Owner Map` requires parent-child containment evidence and ceiling comparison to prove equal-or-narrower execution before admission, with UI and diagnostics projection left to Spec 031.
4. Spec 030 `External Dependency Gates` keeps Spec 031 projection ownership external.
5. Spec 030 `Baseline Conformance vs Final Closure` makes final closure stricter than current distributed baseline and keeps unknown containment from becoming safe evidence.
6. PRD 000 supplies `PolicySafetySnapshotRef`, `policy_safety_digest`, `capability_ceiling`, and `containment` refs.
7. PRD 002 supplies `ProcessExecutionEnvelope`, `ProcessAdapterRef`, `workspace_scope`, `cwd_scope`, `containment_intent`, `containment_evidence_ref`, `permission_action`, `approval_lineage`, and receipt correlation.
8. Current Rust baseline supplies `PermissionCeilingSnapshot`, `InheritedPermissionContext`, `BoundaryPermissionViolation`, `RuntimeBoundaryOrigin`, `DockerContainmentSnapshot`, `PermissionRuleInput`, and static containment rules.
9. Spec 022 PRD 005 supplies the implemented inherited permission ceiling baseline for subagent, app task, deferred MCP, cron, local API, and channel boundaries.
10. Spec 023 supplies Docker and Compose evidence, native unknown fallback, exec workspace narrowing, MCP and subagent snapshot inheritance, and the explicit non-claim of kernel isolation.
11. Spec 031 owns projection schema, CLI/TUI/API/channel adapters, diagnostics parity, and release evidence rendering that will consume this proof.

## Dependency Cut

1. PRD 000 must close first. This PRD requires a known `PolicySafetySnapshotRef` and digest before any containment proof can be admitted.
2. PRD 002 must close first. This PRD proves admission for a typed process envelope, not for raw process spawn helpers.
3. Spec 023 remains the current source for containment evidence classification and current zero-setup baseline. This PRD tightens proof consumption, not physical isolation.
4. Spec 031 consumes `ContainmentPermissionProofProjectionInput` and renders it. Spec 030 owns the input meaning only.
5. Spec 032 supplies future app lifecycle and trust lifecycle facts. This PRD only decides whether those facts fit the parent proof.
6. Spec 035 supplies physical execution snapshots, runtime layout, and persisted refs. This PRD must not invent storage paths or migration families.
7. PRD 006 may count this PRD only after typed proof, stale rejection, widening rejection, diagnostics input, focused tests, workspace gates, and real-surface QA evidence exist.

## Exact Typed Contract

The future implementation must define a typed contract equivalent to the following Rust shape. Module placement may change, but field meaning must stay fixed.

```rust
pub struct ContainmentPermissionLineage {
    pub schema_id: ContainmentLineageSchemaId,
    pub lineage_id: ContainmentLineageId,
    pub parent: ContainmentBoundaryRef,
    pub child: ContainmentBoundaryRef,
    pub policy_safety_snapshot_ref: PolicySafetySnapshotRef,
    pub process_envelope_id: ProcessEnvelopeId,
    pub requested_at_unix_ms: u64,
    pub expires_at_unix_ms: Option<u64>,
    pub source_refs: Vec<ContainmentEvidenceSourceRef>,
    pub redacted_summary: RedactedContainmentLineageSummary,
}

pub struct ContainmentBoundaryRef {
    pub boundary_id: ContainmentBoundaryId,
    pub boundary_kind: RuntimeBoundaryKind,
    pub origin: RuntimeBoundaryOrigin,
    pub containment_state: ContainmentEvidenceState,
    pub containment_evidence_ref: Option<ContainmentEvidenceRef>,
    pub containment_digest: Option<ContainmentDigest>,
    pub workspace_scope: WorkspaceScopeProof,
    pub permission_ceiling: PermissionCeilingProofInput,
    pub created_at_unix_ms: u64,
}

pub struct ContainmentPermissionProof {
    pub schema_id: ContainmentProofSchemaId,
    pub proof_id: ContainmentProofId,
    pub lineage_id: ContainmentLineageId,
    pub policy_safety_digest: PolicySafetyDigest,
    pub envelope_id: ProcessEnvelopeId,
    pub containment_outcome: ContainmentComparisonOutcome,
    pub workspace_outcome: WorkspaceComparisonOutcome,
    pub ceiling_outcome: PermissionCeilingComparisonOutcome,
    pub admission: ProcessEnvelopeAdmission,
    pub violations: Vec<ContainmentProofViolation>,
    pub diagnostics_input: ContainmentPermissionProofProjectionInput,
}
```

Required enum values and field rules:

| Type or field | Required contract |
|---|---|
| `ContainmentLineageSchemaId` | Only `containment_lineage.v1` is valid. Unknown schema is malformed input and rejects. |
| `ContainmentProofSchemaId` | Only `containment_permission_proof.v1` is valid. Unknown schema is malformed input and rejects. |
| `RuntimeBoundaryKind` | `user_turn`, `subagent`, `mcp_stdio`, `app_process`, `plugin_command`, `plugin_tool`, `plugin_hook`, `dependency_preparation`, `verified_entrypoint`, `deferred_bridge`, `local_api`, `channel_inbound`, `cron_wake`. |
| `ContainmentEvidenceState` | `confirmed_non_privileged`, `confirmed_equivalent`, `narrower_hardened`, `native_unknown`, `evidence_missing`, `unsafe_privileged`, `mismatched`, `stale`, `malformed`. |
| `WorkspaceScopeProof` | Contains parent workspace root ref, child workspace root ref, child relative cwd ref, scope digest, and narrowing reason. It must not expose absolute host paths as durable truth. |
| `PermissionCeilingProofInput` | Contains parent mode, requested child mode, parent capability ceiling, requested child capabilities, approved scope refs, per-action evaluation flag, and origin. |
| `policy_safety_snapshot_ref` | Required and must match PRD 000 digest. Missing or mismatched snapshot rejects before process admission. |
| `process_envelope_id` | Required and must match the PRD 002 envelope being admitted. A proof for one envelope cannot admit another. |
| `source_refs` | Safe refs only. Raw process metadata, raw child prompt, raw stdout, raw env, and raw host paths are not legal source refs. |
| `redacted_summary` | Safe for diagnostics and approval. It cannot contain raw secrets, full env, raw command text with secrets, process handles, or absolute host paths. |

## Evidence Is Not Isolation

Containment evidence answers one question: what did the runtime observe or inherit about this boundary. It does not prove the kernel blocked every escape, and it does not replace permission policy.

The proof must preserve these distinctions:

| Evidence state | Meaning | Admission effect |
|---|---|---|
| `confirmed_non_privileged` | Evidence says the boundary runs in a recognized container or equivalent non-privileged context. | May pass containment comparison if snapshot, workspace, and ceiling also pass. |
| `confirmed_equivalent` | Child evidence digest and state equal the parent. | May pass as equal only when workspace and ceiling do not widen. |
| `narrower_hardened` | Child has extra narrowing such as smaller workspace or stricter wrapper. | May pass as narrower only when source refs are known and non-stale. |
| `native_unknown` | Native host or child state lacks known containment evidence. | Never safe. Proc exec and process spawn must ask or deny. `BypassPermissions` must deny. |
| `evidence_missing` | Required ref is absent. | Reject or ask according to envelope intent. It must not default safe. |
| `unsafe_privileged` | Root, privileged container, Docker socket, host root mount, or similar unsafe signal. | Deny for bypass and side-effect process admission. Ask is allowed only when current policy explicitly supports it for a narrower non-bypass case. |
| `mismatched` | Child evidence does not match the parent lineage or expected digest. | Reject before spawn. |
| `stale` | Evidence expired or belongs to an old snapshot or envelope. | Reject before spawn. |
| `malformed` | Unknown schema, illegal state, raw unsafe field, or invalid workspace shape. | Reject before static policy or spawn. |

## Parent-Child Reference Model

Every proof must bind one parent boundary to one child boundary.

| Parent boundary | Child boundary | Required parent refs | Required child refs | Non-widening rule |
|---|---|---|---|---|
| `user_turn` | `subagent` | policy snapshot, parent containment ref, ceiling snapshot | child origin, inherited containment ref, child ceiling input | Child mode and capability set must be equal-or-narrower and per-action evaluation remains required. |
| `user_turn` or `runtime` | `mcp_stdio` | policy snapshot, envelope id, parent containment ref | server declaration ref, command digest, env ref digest, inherited containment ref | Stdio startup cannot claim better containment than parent unless it has confirmed narrower evidence. MCP capability default-deny remains separate. |
| `user_turn` or `runtime` | `app_process` | policy snapshot, envelope id, parent containment ref, approved scope refs | app id, manifest digest, AppSupervisor lifecycle ref, app workspace ref | App declaration is not approval. App workspace and capabilities must be within approved scope. |
| `runtime` | `plugin_command` | policy snapshot, envelope id, plugin manifest activation ref | plugin id, manifest digest, command path digest, inherited containment ref | Plugin command can run only inside the envelope boundary and cannot widen workspace, env, or capability. |
| `runtime` | `plugin_tool` | policy snapshot, envelope id, tool registry ref | plugin tool name, manifest digest, argument digest, inherited containment ref | Tool registration is not permission. Requested capabilities must fit the parent ceiling. |
| `runtime` | `plugin_hook` | policy snapshot, envelope id, hook event ref | hook command ref, redacted context digest, inherited containment ref | Hook output is data only and cannot approve, widen mode, or replace proof. |
| `runtime` | `dependency_preparation` | policy snapshot, envelope id, active trust ref from PRD 005 | dependency manifest digest, package ref, inherited containment ref | Trust provenance can only bound the requested preparation. Installer behavior remains external. |
| `runtime` | `verified_entrypoint` | policy snapshot, envelope id, active trust ref from PRD 005 | entrypoint digest, dependency digest, inherited containment ref | Entrypoint execution must match exact active trust and fit parent ceiling. |
| `parent_process` | `deferred_bridge` | policy snapshot, bridge scope digest, approved scope refs | deferred bridge name, underlying tool action, inherited containment ref | Provider-visible bridge cannot bypass the underlying same-gate evaluation. |

If any required ref is missing, stale, mismatched, or malformed, the proof cannot be `admit`. Missing evidence is not weaker proof. It is no proof.

## Snapshot Linkage

The proof must bind to PRD 000 snapshot fields.

1. `policy_safety_snapshot_ref.schema_id` must be known.
2. `policy_safety_snapshot_ref.policy_safety_digest` must equal `ContainmentPermissionProof.policy_safety_digest`.
3. The parent and child `containment_digest` values must match the snapshot containment ref or be explicitly recorded as child-narrower evidence.
4. The proof `expires_at_unix_ms` cannot outlive the policy safety snapshot expiry.
5. A proof created for one `ProcessExecutionEnvelope.envelope_id` cannot admit a different envelope.
6. A proof created before cancellation, timeout, AppSupervisor restart, runtime recover, plugin reload, trust revoke, MCP server reconnect, or deferred bridge resume must be invalidated unless the external owner supplies fresh matching evidence.

## Workspace Narrowing

Workspace comparison must be structural, not prose based.

`WorkspaceScopeProof` must compare these fields:

1. Parent workspace root ref.
2. Child workspace root ref.
3. Child cwd as a workspace-relative ref or opaque redacted locator.
4. Allowed read scope digest.
5. Allowed write scope digest.
6. Host mount summary digest when available.
7. Narrowing reason, such as `same_workspace`, `subdirectory`, `read_only`, `temp_workspace`, `external_owner_supplied`.

Allowed outcomes:

| Outcome | Meaning | Admission effect |
|---|---|---|
| `same_scope` | Child scope equals parent scope by digest. | May pass if containment and ceiling pass. |
| `narrower_scope` | Child scope is a strict subset or read-only subset. | May pass if containment and ceiling pass. |
| `unknown_scope` | Scope cannot be compared. | Ask or deny. Never default safe. |
| `wider_scope` | Child reads, writes, cwd, or host mount reach outside parent scope. | Reject before spawn. |
| `mismatched_scope_ref` | Scope digest or ref does not match snapshot or envelope. | Reject before spawn. |
| `malformed_scope` | Absolute host path, traversal, empty required ref, or illegal raw field appears. | Reject before static policy or spawn. |

## Ceiling Comparison Outcomes

Permission ceiling comparison must extend the current `evaluate_inherited_ceiling` baseline without weakening it.

| Outcome | Required condition | Admission effect |
|---|---|---|
| `equal_ceiling` | Requested mode rank equals parent mode rank, requested capabilities are a subset, approved scope refs match, and per-action evaluation is required. | May pass if containment and workspace pass. |
| `narrower_ceiling` | Requested mode rank is lower than parent or requested capabilities are a strict subset, with per-action evaluation required. | May pass if containment and workspace pass. |
| `mode_widening` | Requested mode rank is greater than parent. | Reject. |
| `capability_widening` | Any requested capability is outside parent capability ceiling. | Reject. |
| `scope_widening` | Approved scope refs, workspace refs, or source refs add reach not present in the parent. | Reject. |
| `missing_approval_ref` | Boundary origin requires approval and none is present. | Reject for non-interactive, ask for interactive only when static policy permits. |
| `app_declaration_only` | App manifest declaration is the only grant source. | Reject. |
| `deferred_gate_bypass` | Deferred or bridge path lacks per-action evaluation or approved scope refs. | Reject. |
| `stale_decision_reuse` | Decision digest, snapshot digest, turn id, or proof ref belongs to an old state. | Reject. |
| `malformed_ceiling` | Unknown mode, unknown capability, illegal origin, or missing required ceiling field. | Reject before policy. |

`BypassPermissions` is the widest current mode. A child cannot request it unless the parent already has it, the capability ceiling allows the requested action, containment is confirmed non-privileged or narrower, and static policy has no denial. Unknown or unsafe containment must deny bypass.

## Process-Envelope Admission Matrix

`ProcessEnvelopeAdmission` must have these outcomes:

| Admission | Meaning | Spawn allowed |
|---|---|---|
| `admit` | Containment, workspace, ceiling, snapshot, and envelope refs are equal-or-narrower and current. | Yes, through PRD 002 owner adapter only. |
| `ask_required` | Evidence is unknown or incomplete and current permission mode allows local user ask. | No until a fresh matching approval is correlated. |
| `deny` | Static policy, unsafe containment, widening, stale proof, mismatch, malformed input, or non-interactive unknown blocks. | No. |
| `reject_malformed` | Schema, required fields, raw unsafe fields, or shape are invalid. | No. |
| `reject_stale` | Proof, snapshot, approval, trust, app state, plugin state, MCP connection, or deferred bridge state is stale. | No. |
| `reject_mismatch` | Snapshot digest, containment digest, workspace digest, ceiling refs, envelope id, or source refs differ. | No. |

Admission must run before process spawn. Process stdout saying success, plugin hook text saying approved, app manifest prose, MCP prompt metadata, or child model output cannot change an admission result.

## Boundary Rules

### Subagent

1. Parent context supplies policy snapshot ref, containment ref, and permission ceiling.
2. Child context receives inherited refs and requested mode and capability list.
3. Proof admits only if mode, capabilities, workspace, and containment are equal-or-narrower.
4. Child prompt, child system text, child output, and subagent metadata are untrusted data and cannot raise mode or create approval.

### MCP Stdio

1. Stdio server startup is a process envelope boundary.
2. MCP server metadata, prompts, tools, resources, and JSON-RPC output are not proof inputs except as safe refs and digests.
3. Parent containment must be inherited or a confirmed narrower child evidence ref must exist.
4. Deferred MCP bridge calls must re-enter the same permission and containment proof before underlying tool execution.

### App Process

1. AppSupervisor owns lifecycle and may spawn only after PRD 002 envelope admission and this proof admit.
2. App manifest permission declaration is input to requested capability classification, not approval.
3. App workspace must be equal to or narrower than parent approved scope.
4. Cancel, stop, recover, or stale AppSupervisor state invalidates reusable proof unless fresh matching evidence exists.

### Plugin Command, Tool, and Hook

1. Plugin manifest, command path, tool registration, and hook registration supply refs only.
2. Hook output and plugin stdout are data only.
3. Plugin command, tool, and hook process candidates must keep the same or narrower workspace, env refs, capabilities, and containment.
4. Plugin reload, disable, manifest digest change, command digest change, or hook event mismatch invalidates proof.

### Dependency Preparation and Verified Entrypoint

1. PRD 005 supplies active exact-match trust provenance before these boundaries may ask for admission.
2. This PRD compares containment, workspace, and ceiling for the process envelope only.
3. Installer behavior, package verification details, trust registry transitions, and runner lifecycle remain external.
4. Revoked, stale, missing, or digest-mismatched trust provenance causes stale or mismatch rejection before proof comparison.

### Deferred Bridge

1. Provider-visible bridge visibility is not execution permission.
2. The underlying tool or process action must use the current policy snapshot and proof.
3. Closed turn, superseded turn, stale decision digest, or cancellation invalidates admission state.
4. Resume after cancellation must ask again or deny if fresh proof cannot be built.

## Normal Sequence

1. Runtime receives a PRD 002 `ProcessExecutionEnvelope` with policy snapshot ref, workspace scope, containment intent, containment evidence ref, permission action, and approval lineage.
2. Runtime builds `ContainmentPermissionLineage` from the parent boundary, requested child boundary, policy snapshot ref, envelope id, and safe source refs.
3. Runtime validates schema ids, snapshot digest, envelope id, source refs, expiry, and redacted shape.
4. Runtime compares containment state. Equal or narrower may continue. Unknown, missing, unsafe, stale, malformed, or mismatched states become ask or deny as defined above.
5. Runtime compares workspace scope. Equal or narrower may continue. Wider, unknown, malformed, stale, or mismatched scope rejects or asks according to admission rules.
6. Runtime compares permission ceiling using the current inherited ceiling baseline plus this PRD's outcome vocabulary.
7. Runtime emits one `ContainmentPermissionProof` with diagnostics projection input.
8. PRD 002 process gate may spawn only when admission is `admit`, static policy permits, and approval lineage is valid when required.
9. Runtime stores or hands off redacted proof refs according to the external owner, without raw args, raw env, raw host paths, or process handles.

## Failure Sequences

1. Child boundary has no containment evidence ref and the envelope asks for confirmed non-privileged containment. Proof returns `reject_mismatch` or `ask_required` according to mode and no process starts.
2. Child requests `ProcExec` under `native_unknown` containment. Static policy and proof return ask or deny. Bypass denies.
3. Child workspace root digest includes a host mount outside the parent root. Proof returns `wider_scope` and `deny` before spawn.
4. Child asks for `ProcExec` when parent capability ceiling contains only `FsRead`. Proof returns `capability_widening` and `deny`.
5. Approval lineage matches action digest but not policy safety digest. Proof returns stale or mismatch and no process starts.
6. App process proof was created before cancellation and AppSupervisor resumes later. Proof is invalid. Resume must build fresh proof or deny.
7. Plugin stdout says the command is safe. That output is ignored for proof. Admission remains based on typed refs and comparison outcomes.
8. MCP prompt metadata asks to ignore restrictions. The metadata is data only and cannot change containment, workspace, or ceiling comparison.

## Stale and Mismatch Rejection

Reject before process spawn when any of these are true:

1. Unknown lineage or proof schema id.
2. Missing policy safety snapshot ref.
3. Policy safety digest mismatch.
4. Envelope id mismatch.
5. Parent or child containment digest mismatch.
6. Expired lineage or proof.
7. Snapshot expiry is older than proof expiry.
8. Workspace digest mismatch.
9. Child workspace outside parent scope.
10. Requested mode or capability widens parent ceiling.
11. Approval ref is missing where required.
12. Approval, trust, plugin, MCP, app, deferred bridge, or process state is stale.
13. Required source ref is missing or belongs to a different boundary.
14. Raw secret, raw env, raw prompt, raw command with secret material, process handle, or absolute host path appears in a proof field.

## Diagnostics Projection Input and Spec 031 Handoff

This PRD owns only the machine input that projection adapters consume.

```rust
pub struct ContainmentPermissionProofProjectionInput {
    pub proof_id: ContainmentProofId,
    pub lineage_id: ContainmentLineageId,
    pub envelope_id: ProcessEnvelopeId,
    pub policy_safety_snapshot_id: PolicySafetySnapshotId,
    pub policy_safety_digest: PolicySafetyDigest,
    pub parent_boundary_kind: RuntimeBoundaryKind,
    pub child_boundary_kind: RuntimeBoundaryKind,
    pub containment_outcome: ContainmentComparisonOutcome,
    pub workspace_outcome: WorkspaceComparisonOutcome,
    pub ceiling_outcome: PermissionCeilingComparisonOutcome,
    pub admission: ProcessEnvelopeAdmission,
    pub violation_codes: Vec<ContainmentProofViolation>,
    pub redacted_summary: RedactedContainmentLineageSummary,
    pub evidence_refs: Vec<ContainmentEvidenceSourceRef>,
}
```

Spec 031 owns:

1. CLI, TUI, local API, WebSocket, channel, diagnostics bundle, and release artifact adapters.
2. Status names as displayed to users.
3. Projection parity smoke tests.
4. Release runner artifact layout and human-readable summaries.

Spec 030 owns:

1. The typed input fields above.
2. The rule that unknown and unsafe are not safe.
3. The comparison outcomes.
4. The admission result.
5. The redaction rule for proof input.

If Spec 031 projection is missing, PRD 006 may not count UI parity as done. This PRD can still be implemented if the typed projection input exists and is covered by tests.

## TDD Sequence

Future implementation must follow this order.

1. Baseline characterization. Add or confirm tests for current `evaluate_inherited_ceiling`, unknown containment proc exec handling, bypass containment denial, app declaration-only rejection, deferred gate bypass rejection, late stale decision rejection, audit diagnostics containment warning, and current subagent or MCP inheritance cases.
2. Failing matrix for typed lineage. Add failing tests named `containment_lineage_rejects_unknown_schema`, `containment_lineage_rejects_missing_policy_snapshot_ref`, `containment_lineage_rejects_envelope_mismatch`, and `containment_lineage_rejects_raw_untrusted_metadata`.
3. Failing matrix for containment states. Add failing tests named `containment_proof_rejects_missing_evidence_default_safe`, `containment_proof_unknown_state_requires_ask_or_deny`, `containment_proof_unsafe_privileged_denies_bypass`, `containment_proof_rejects_stale_evidence`, and `containment_proof_rejects_digest_mismatch`.
4. Failing matrix for workspace and ceiling. Add failing tests named `containment_proof_allows_equal_workspace_and_ceiling`, `containment_proof_allows_narrower_workspace_and_ceiling`, `containment_proof_rejects_workspace_widening`, `containment_proof_rejects_capability_widening`, `containment_proof_rejects_mode_widening`, `containment_proof_rejects_app_declaration_only`, and `containment_proof_rejects_deferred_gate_bypass`.
5. Failing matrix for boundary adapters. Add failing tests named `subagent_boundary_consumes_parent_lineage`, `mcp_stdio_boundary_requires_equal_or_narrower_proof`, `app_process_boundary_requires_fresh_proof`, `plugin_command_boundary_ignores_stdout_authorization`, `plugin_tool_boundary_requires_equal_or_narrower_proof`, `plugin_hook_boundary_cannot_widen_ceiling`, and `deferred_bridge_boundary_invalidates_closed_turn_proof`.
6. Failing matrix for PRD 005-dependent boundaries. Add failing tests named `dependency_preparation_boundary_blocks_without_prd005_trust_evidence`, `dependency_preparation_boundary_rejects_stale_trust_evidence`, `verified_entrypoint_boundary_blocks_without_prd005_trust_evidence`, and `verified_entrypoint_boundary_rejects_digest_mismatch`. Until PRD 005 is implemented, these tests must assert blocked-on-PRD005 evidence rather than skip or success. PRD 006 may consume these boundaries only when PRD 005 evidence and this proof evidence both exist.
7. Failing matrix for cancellation and repeated interruption. Add failing tests named `containment_proof_cancel_resume_invalidates_reusable_admission`, `containment_proof_cancel_resume_requires_fresh_matching_evidence`, `containment_proof_repeated_interruptions_preserve_terminal_denial`, and `containment_proof_repeated_interruptions_do_not_reopen_denied_admission`.
8. Failing matrix for prompt injection. Add tests proving untrusted child prompt, process metadata, plugin stdout, MCP prompt, app manifest prose, and deferred bridge metadata cannot alter containment outcome, workspace outcome, ceiling outcome, or admission.
9. Minimal implementation. Add typed lineage, proof, comparison functions, parser rejection, diagnostics projection input, and PRD 002 admission hook. Do not add Spec 031 adapters, AppSupervisor lifecycle, physical storage, installer behavior, or new isolation backend under Spec 030.
10. Refactor only after focused tests pass. Remove duplicate containment or ceiling comparisons only when the typed proof is the shared seam and behavior stays locked.
11. Final regression. Run focused Cargo commands, workspace gates, real-surface QA, and evidence review before PRD 006 may consume this PRD.

Tests must assert typed enum values, proof fields, digest equality or inequality, admission outcomes, violation codes, and absence of raw fixture values. Tests must not assert natural-language prompt prose.

## Focused Cargo Targets and Commands

Future implementation must use the workspace manifest explicitly.

```sh
cargo fmt --manifest-path crates/Cargo.toml --all -- --check
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core permission_policy
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core containment_permission
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core process_gate
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core runtime_loop
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core plugin_runtime
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core mcp
cargo clippy --manifest-path crates/Cargo.toml --locked -p shacs-core --all-targets -- -D warnings
```

Before closure, run the workspace gates from `AGENTS.md`:

```sh
cargo fmt --manifest-path crates/Cargo.toml --all -- --check
cargo clippy --manifest-path crates/Cargo.toml --locked --workspace --all-targets -- -D warnings
cargo test --manifest-path crates/Cargo.toml --locked --workspace
```

`cargo fmt` does not consume dependency resolution; every final or CI Cargo gate that resolves packages must use `--locked`.

No authoring worker for this documentation task should run Cargo. The commands above are future implementation gates.

## Agent-Executed Real Surface QA

The future implementation worker must create deterministic fixtures and save redacted machine evidence at the exact locators below. The setup owner is the `shacs-core` containment proof test fixture pack, with fixture source rooted at `crates/shacs-core/tests/fixtures/spec030_prd003_containment/`. The setup command is:

```sh
cargo run --manifest-path crates/Cargo.toml --locked -p shacs-cli -- qa-fixture spec030-prd003 setup --workspace /tmp/shacs-spec030-prd003-qa --fixture-root crates/shacs-core/tests/fixtures/spec030_prd003_containment --evidence-root .omo/evidence/spec030/prd003
```

The cleanup command is linked from every artifact as `cleanup.command` and must be run after PASS or FAIL evidence is saved:

```sh
cargo run --manifest-path crates/Cargo.toml --locked -p shacs-cli -- qa-fixture spec030-prd003 cleanup --workspace /tmp/shacs-spec030-prd003-qa --evidence-root .omo/evidence/spec030/prd003
```

| Boundary | Fixture owner and locator | Invocation | Artifact | PASS criteria | FAIL criteria | Cleanup linkage |
|---|---|---|---|---|---|---|
| `subagent` | `shacs-core::tests::fixtures::spec030_prd003_containment::subagent`; `crates/shacs-core/tests/fixtures/spec030_prd003_containment/subagent/parent-default-child-read.json` | `cargo run --manifest-path crates/Cargo.toml --locked -p shacs-cli -- qa-fixture spec030-prd003 run subagent --workspace /tmp/shacs-spec030-prd003-qa --evidence-root .omo/evidence/spec030/prd003` | `.omo/evidence/spec030/prd003/subagent-proof.json` | `status=pass`, `boundary=subagent`, `parent_ref`, `child_ref`, `policy_safety_digest`, `envelope_id`, `containment_outcome` in `confirmed_equivalent,narrower_hardened`, `workspace_outcome` in `same_scope,narrower_scope`, `ceiling_outcome` in `equal_ceiling,narrower_ceiling`, `admission=admit`. | Any missing required field, widening outcome, `admission=admit` with unknown/unsafe containment, raw prompt/env/path/process handle, or absent cleanup command. | Artifact field `cleanup.command` equals the cleanup command above. |
| `mcp_stdio` | `shacs-core::tests::fixtures::spec030_prd003_containment::mcp`; `crates/shacs-core/tests/fixtures/spec030_prd003_containment/mcp/stdio-default-deny-server.json` | `cargo run --manifest-path crates/Cargo.toml --locked -p shacs-cli -- qa-fixture spec030-prd003 run mcp-stdio --workspace /tmp/shacs-spec030-prd003-qa --evidence-root .omo/evidence/spec030/prd003` | `.omo/evidence/spec030/prd003/mcp-stdio-proof.json` | `status=pass`, `boundary=mcp_stdio`, server ref and command digest present, default-deny capability projection present, proof admits only equal-or-narrower containment/workspace/ceiling. | MCP prompt, tool metadata, or JSON-RPC output changes permission mode, ceiling, proof outcome, or admission; missing inherited containment digest; raw env persists. | Artifact field `cleanup.command` equals the cleanup command above. |
| `app_process` | Spec 032 AppSupervisor fixture owner; blocked locator `.omo/evidence/spec030/prd003/blocked/spec032-app-process.json`; active locator after Spec 032 closure `crates/shacs-core/tests/fixtures/spec030_prd003_containment/app/spec032-app-process-contained.json` | Blocked check: `cargo run --manifest-path crates/Cargo.toml --locked -p shacs-cli -- qa-fixture spec030-prd003 blocked app-process --evidence-root .omo/evidence/spec030/prd003`; active check after Spec 032 evidence exists: `cargo run --manifest-path crates/Cargo.toml --locked -p shacs-cli -- qa-fixture spec030-prd003 run app-process --workspace /tmp/shacs-spec030-prd003-qa --evidence-root .omo/evidence/spec030/prd003` | Blocked: `.omo/evidence/spec030/prd003/blocked/spec032-app-process.json`; active: `.omo/evidence/spec030/prd003/app-process-proof.json` | Before Spec 032 closure: `status=blocked`, `blocked_on=spec032_app_supervisor_lifecycle`, and no success claim. After Spec 032 closure: lifecycle ref, app manifest digest, envelope id, proof id, equal-or-narrower outcomes, and app declaration-only rejection are present. | Missing blocked artifact before Spec 032, blocked artifact marked pass, active run without Spec 032 lifecycle ref, app declaration-only grant, or stale AppSupervisor proof reuse. | Both artifacts include the cleanup command above or `cleanup.state=not_started` for blocked evidence. |
| `plugin_command` | `shacs-core::tests::fixtures::spec030_prd003_containment::plugin`; `crates/shacs-core/tests/fixtures/spec030_prd003_containment/plugin/command-safe-stdout-injection.json` | `cargo run --manifest-path crates/Cargo.toml --locked -p shacs-cli -- qa-fixture spec030-prd003 run plugin-command --workspace /tmp/shacs-spec030-prd003-qa --evidence-root .omo/evidence/spec030/prd003` | `.omo/evidence/spec030/prd003/plugin-command-proof.json` | Plugin id, manifest digest, command digest, envelope id, proof id, equal-or-narrower outcomes, and stdout data-only disposition are present. | Plugin stdout authorizes, widens ceiling, changes admission, raw stdin/stdout persists, or manifest digest mismatch admits. | Artifact field `cleanup.command` equals the cleanup command above. |
| `plugin_tool` | `shacs-core::tests::fixtures::spec030_prd003_containment::plugin`; `crates/shacs-core/tests/fixtures/spec030_prd003_containment/plugin/tool-fs-read-narrow.json` | `cargo run --manifest-path crates/Cargo.toml --locked -p shacs-cli -- qa-fixture spec030-prd003 run plugin-tool --workspace /tmp/shacs-spec030-prd003-qa --evidence-root .omo/evidence/spec030/prd003` | `.omo/evidence/spec030/prd003/plugin-tool-proof.json` | Tool registry ref, plugin tool name, manifest digest, argument digest, inherited containment ref, and `plugin_tool_boundary_requires_equal_or_narrower_proof` evidence are present. | Tool registration alone grants permission, requested capability exceeds parent ceiling, plugin tool metadata changes proof, or raw argument JSON persists. | Artifact field `cleanup.command` equals the cleanup command above. |
| `plugin_hook` | `shacs-core::tests::fixtures::spec030_prd003_containment::plugin`; `crates/shacs-core/tests/fixtures/spec030_prd003_containment/plugin/hook-before-attempted-allow.json` | `cargo run --manifest-path crates/Cargo.toml --locked -p shacs-cli -- qa-fixture spec030-prd003 run plugin-hook --workspace /tmp/shacs-spec030-prd003-qa --evidence-root .omo/evidence/spec030/prd003` | `.omo/evidence/spec030/prd003/plugin-hook-proof.json` | Hook event ref, redacted context digest, hook command ref, proof id, and data-only hook output disposition are present; ceiling remains equal-or-narrower. | Hook output approves, widens mode/ceiling, replaces proof, or raw event context persists. | Artifact field `cleanup.command` equals the cleanup command above. |
| `deferred_bridge` | `shacs-core::tests::fixtures::spec030_prd003_containment::deferred`; `crates/shacs-core/tests/fixtures/spec030_prd003_containment/deferred/bridge-read-closed-turn.json` | `cargo run --manifest-path crates/Cargo.toml --locked -p shacs-cli -- qa-fixture spec030-prd003 run deferred-bridge --workspace /tmp/shacs-spec030-prd003-qa --evidence-root .omo/evidence/spec030/prd003` | `.omo/evidence/spec030/prd003/deferred-bridge-proof.json` | Bridge name, bridge scope digest, underlying action id, inherited containment ref, same-gate evaluation, and closed-turn invalidation are present. | Provider-visible bridge bypasses proof, closed or superseded turn admits, stale decision digest admits, or cancellation resumes without fresh proof. | Artifact field `cleanup.command` equals the cleanup command above. |
| `dependency_preparation` | PRD 005 trust fixture owner; blocked locator `.omo/evidence/spec030/prd003/blocked/prd005-dependency-preparation.json`; active locator after PRD 005 closure `crates/shacs-core/tests/fixtures/spec030_prd003_containment/prd005/dependency-preparation-active-trust.json` | Blocked check: `cargo run --manifest-path crates/Cargo.toml --locked -p shacs-cli -- qa-fixture spec030-prd003 blocked dependency-preparation --evidence-root .omo/evidence/spec030/prd003`; active check after PRD 005 evidence exists: `cargo run --manifest-path crates/Cargo.toml --locked -p shacs-cli -- qa-fixture spec030-prd003 run dependency-preparation --workspace /tmp/shacs-spec030-prd003-qa --evidence-root .omo/evidence/spec030/prd003` | Blocked: `.omo/evidence/spec030/prd003/blocked/prd005-dependency-preparation.json`; active: `.omo/evidence/spec030/prd003/dependency-preparation-proof.json` | Before PRD 005 closure: `status=blocked`, `blocked_on=prd005_active_trust_provenance`, and no success claim. After PRD 005 closure: active trust ref, dependency manifest digest, package ref, stale trust rejection, and equal-or-narrower proof are present. | Missing blocked artifact before PRD 005, blocked artifact marked pass, installer behavior claimed by PRD 003, stale/revoked/missing trust admits, or trust digest mismatch admits. | Both artifacts include the cleanup command above or `cleanup.state=not_started` for blocked evidence. |
| `verified_entrypoint` | PRD 005 trust fixture owner; blocked locator `.omo/evidence/spec030/prd003/blocked/prd005-verified-entrypoint.json`; active locator after PRD 005 closure `crates/shacs-core/tests/fixtures/spec030_prd003_containment/prd005/verified-entrypoint-active-trust.json` | Blocked check: `cargo run --manifest-path crates/Cargo.toml --locked -p shacs-cli -- qa-fixture spec030-prd003 blocked verified-entrypoint --evidence-root .omo/evidence/spec030/prd003`; active check after PRD 005 evidence exists: `cargo run --manifest-path crates/Cargo.toml --locked -p shacs-cli -- qa-fixture spec030-prd003 run verified-entrypoint --workspace /tmp/shacs-spec030-prd003-qa --evidence-root .omo/evidence/spec030/prd003` | Blocked: `.omo/evidence/spec030/prd003/blocked/prd005-verified-entrypoint.json`; active: `.omo/evidence/spec030/prd003/verified-entrypoint-proof.json` | Before PRD 005 closure: `status=blocked`, `blocked_on=prd005_verified_entrypoint_trust`, and no success claim. After PRD 005 closure: active trust ref, entrypoint digest, dependency digest, digest mismatch rejection, and equal-or-narrower proof are present. | Missing blocked artifact before PRD 005, blocked artifact marked pass, entrypoint runner lifecycle claimed by PRD 003, stale/revoked/missing trust admits, or entrypoint digest mismatch admits. | Both artifacts include the cleanup command above or `cleanup.state=not_started` for blocked evidence. |
| `diagnostics_projection_input` | Spec 031 projection consumer; typed input owner remains Spec 030. Locator `crates/shacs-core/tests/fixtures/spec030_prd003_containment/diagnostics/projection-input.json` | `cargo run --manifest-path crates/Cargo.toml --locked -p shacs-cli -- qa-fixture spec030-prd003 run diagnostics-projection-input --workspace /tmp/shacs-spec030-prd003-qa --evidence-root .omo/evidence/spec030/prd003` | `.omo/evidence/spec030/prd003/diagnostics-projection-input.json` | Artifact contains only `ContainmentPermissionProofProjectionInput` fields, redacted refs, outcomes, and safe summaries. | Artifact claims Spec 031 rendering ownership, raw args/env/host paths/prompts/process handles appear, or missing projection input fields pass. | Artifact field `cleanup.command` equals the cleanup command above. |

Spec 035-dependent physical snapshot storage is not a runtime QA pass condition for this PRD. Until Spec 035 supplies execution snapshot persistence evidence, `.omo/evidence/spec030/prd003/blocked/spec035-execution-snapshot.json` must exist with `status=blocked`, `blocked_on=spec035_execution_snapshot_persistence`, and `prd003_state=typed_input_only`. PRD 006 must reject closure if this blocked locator is absent and Spec 035 evidence is also absent.

## Agent-Generated Artifact-Backed Read Audit

Before PRD 006 may count this PRD as closed, an agent must generate `.omo/evidence/spec030/prd003/read-audit.json` by reading this PRD and every QA artifact above. The audit invocation is:

```sh
cargo run --manifest-path crates/Cargo.toml --locked -p shacs-cli -- qa-fixture spec030-prd003 read-audit --prd docs/specs/030-policy-permission-redaction-and-containment-model/prds/003-containment-inheritance-and-permission-ceiling-proof.md --evidence-root .omo/evidence/spec030/prd003 --output .omo/evidence/spec030/prd003/read-audit.json
```

The audit PASS criteria are binary:

1. `status` is `pass`.
2. `source_prd` equals `docs/specs/030-policy-permission-redaction-and-containment-model/prds/003-containment-inheritance-and-permission-ceiling-proof.md`.
3. `matrix_locator` equals `docs/specs/030-policy-permission-redaction-and-containment-model/prds/003-containment-inheritance-and-permission-ceiling-proof.md#Parent-Child Reference Model`.
4. Every boundary artifact listed in the QA table exists or has its required blocked artifact.
5. Every non-blocked artifact has parent ref, child ref, policy snapshot ref, envelope id, containment outcome, workspace outcome, ceiling outcome, admission, and cleanup linkage.
6. Every admitted non-blocked artifact is equal-or-narrower for containment, workspace, and ceiling.
7. Missing evidence, unknown containment, unsafe containment, workspace widening, capability widening, mode widening, stale state, mismatch, malformed input, cancellation reuse, and repeated-interruption reopen cases are rejected or blocked as specified.
8. Spec 031 ownership is preserved by allowing only `diagnostics_projection_input` artifacts under Spec 030.
9. Evidence is separate from isolation claims; no artifact may mark containment evidence as kernel isolation or permission waiver.

The audit FAIL criteria are binary: any missing required artifact, any missing blocked locator, any non-concrete path, any widened admitted boundary, any stale proof reuse after cancellation, any repeated interruption that reopens terminal denial, any raw secret/env/path/prompt/process handle in evidence, any Spec 031 adapter ownership claim, or any PRD 005-dependent surface marked pass without PRD 005 active trust evidence makes `status=fail`.

## Adversarial Requirements

| Scenario | Required handling |
|---|---|
| `malformed_input` | Applies at lineage and proof parse boundaries. Unknown schema, missing required ref, raw unsafe field, invalid workspace, unknown state, or illegal ceiling field rejects before policy or spawn. |
| `stale_state` | Applies to policy snapshot, containment ref, proof ref, approval, trust, app state, plugin state, MCP state, deferred bridge, cancellation, timeout, and runtime recover. Stale state rejects before spawn. |
| `dirty_worktree` | Current authoring must verify only the requested PRD and evidence file changed. Future implementation must use temp workspaces and avoid unrelated source edits. |
| `misleading_success_output` | Process stdout, plugin stdout, MCP response, child result, or app output saying success is not proof. Only typed proof and process receipt fields count. |
| `prompt_injection` | Untrusted child/process metadata, child prompt, MCP prompt, plugin text, app prose, stdout, stderr, and deferred metadata cannot affect proof or admission. |
| `cancel_resume` | Applies when process admission state could be reused after cancellation. Cancellation invalidates reusable proof unless fresh matching evidence is built. Missing fresh proof asks or denies. |
| `repeated_interruptions` | Applies when repeated cancel or interrupt targets the same envelope or proof. Reuse must be idempotent and must not spawn a duplicate process or change prior denial to allow. |
| `hung_commands` | Future real-surface QA must use bounded commands and timeout receipts. Hanging is a failed QA artifact, not success. |
| `flaky_tests` | Tests must use deterministic fixtures, fake drivers, controllable clocks, or bounded probes. Sleeps are not proof. |
| `network_unavailable` | Applies only when a boundary asks for network capability. Missing network evidence cannot widen capability and must reject or ask under the ceiling. |
| `cleanup` | None for this documentation task. Future process cleanup is PRD 002 receipt and external owner lifecycle scope. |

## Closure Evidence

PRD 006 may count this PRD as closed only when implementation evidence includes:

1. Baseline characterization output for current permission ceiling, static containment rules, unknown containment handling, app declaration-only rejection, deferred gate bypass rejection, and stale decision rejection.
2. Failing-first history for typed lineage, missing evidence default-safe rejection, unknown and unsafe ask-or-deny handling, workspace widening, capability widening, mode widening, stale state, mismatch, malformed input, prompt injection, plugin tool proof, PRD 005-dependent boundary blocking, cancellation invalidation, repeated-interruption terminal denial preservation, and boundary-specific cases.
3. Typed Rust contracts for `ContainmentPermissionLineage`, `ContainmentBoundaryRef`, `ContainmentPermissionProof`, comparison outcomes, proof violations, admission, and diagnostics projection input.
4. Process-envelope admission evidence proving PRD 002 gates consume this proof before spawn.
5. Parent-child matrix evidence for subagent, MCP stdio, app process, plugin command/tool/hook, dependency preparation, verified entrypoint, and deferred bridge boundaries.
6. PRD 005 dependency artifacts for dependency preparation and verified entrypoint boundaries. Before PRD 005 closes, closure evidence must include blocked-on-PRD005 machine evidence for those boundaries. After PRD 005 closes, closure evidence must include exact active trust ref, digest match, stale/revoked/missing trust rejection, and equal-or-narrower proof artifacts for both boundaries.
7. Cancellation and repeated-interruption artifacts proving reusable admission proofs are invalidated after cancellation unless fresh matching evidence is built, and repeated interrupts preserve terminal denial idempotently without reopening admission or spawning a duplicate process.
8. Focused Cargo commands and workspace gates passing with `--manifest-path crates/Cargo.toml`.
9. Real-surface QA artifacts for the supported boundary families, with external-owner surfaces marked blocked on Spec 032 or PRD 005 evidence until their owner evidence exists.
10. Diagnostics handoff evidence showing Spec 031 consumes projection input while Spec 030 does not own UI or release adapters.
11. Explicit non-claim evidence stating containment evidence is not kernel isolation and unknown or unsafe is never safe.
12. `.omo/evidence/spec030/prd003/read-audit.json` exists with `status=pass` and proves every QA boundary artifact or required blocked locator satisfies the binary criteria in the artifact-backed Read audit section.

## Exit Criteria

1. Every supported child boundary has a typed containment lineage and proof before process admission.
2. Admission allows only equal-or-narrower containment, workspace, and permission ceiling.
3. Missing evidence, unknown containment, unsafe containment, stale proof, mismatched digest, workspace widening, capability widening, mode widening, app declaration-only grant, and deferred gate bypass all reject or ask as specified before spawn.
4. `BypassPermissions` denies unless parent mode, capability ceiling, static policy, workspace, and confirmed non-privileged or narrower containment all pass.
5. Diagnostics projection input exists and contains only redacted refs, outcomes, and safe summaries.
6. Spec 031 owns projection adapters and release rendering.
7. Spec 030 does not claim kernel isolation, universal sandboxing, or complete containment.
