# PRD 000. unified policy and safety correlation snapshot

Status: Planned

## Goal

이 PRD는 permissioned action, approval, audit, replay, diagnostics가 같은 policy and safety correlation snapshot을 참조하게 하는 foundation typed 계약을 정의한다.

목표는 action마다 흩어진 `action_digest`, `snapshot_digest`, permission mode, capability ceiling, containment evidence, provenance summary를 하나의 immutable ref와 canonical digest로 묶는 것이다. 이 ref는 권한을 새로 만들지 않는다. 소비자가 같은 안전 맥락을 보고 있는지 비교하고, stale 또는 mismatched approval과 replay를 거절하는 correlation evidence다.

## Scope

1. `PolicySafetySnapshotRef`와 `PolicySafetySnapshot`의 필드, schema identity, digest material을 정의한다.
2. Permissioned action normalization이 snapshot ref와 digest를 생성하거나 받는 foundation 경계를 정의한다.
3. Approval request, approval decision, audit record, replay input, diagnostics summary가 같은 ref와 digest를 소비하는 foundation 규칙을 정의한다.
4. Snapshot mismatch, stale snapshot, schema mismatch, unavailable source provenance를 fail closed로 접는 foundation 규칙을 정의한다.
5. Downstream PRDs가 소비할 typed ref readiness gate를 정의하되, downstream consumer evidence를 이 PRD 완료 조건에 섞지 않는다.
6. Future Rust worker가 쓸 TDD 순서, focused Cargo targets, real surface QA를 정의한다.

## Non Scope

1. Config snapshot, context snapshot, provider execution snapshot 생성은 Spec 035가 소유한다.
2. Snapshot persistence, physical storage path, migration, retention, cleanup은 Spec 035가 소유한다.
3. Provider adapter immutability와 provider wire shaping은 Spec 035가 소유한다.
4. Process execution envelope, process receipt, process missing-ref rejection, and process consumer evidence are owned by PRD 002. This PRD only makes the ref available for PRD 002 to consume.
5. Typed secret refs와 redaction provenance는 PRD 001이 소유한다. 이 PRD는 raw secret value를 snapshot field로 받지 않는다.
6. UI projection, diagnostics rendering, release artifact rendering은 Spec 031이 소유한다.
7. AppSupervisor lifecycle, trust registry lifecycle, dependency install, verified entrypoint runner는 Spec 032와 Spec 035가 소유한다.
8. PRD 006 owns final downstream consumer audit across PRD 002 through PRD 005 before Spec 030 closure.
9. 이 PRD는 persistence path를 만들지 않는다.

## SPEC Inputs

1. [Spec 030 `## 소유하는 open scope`](../SPEC.md#소유하는-open-scope) requires correlation evidence for action digest, snapshot digest, target summary, approval, static policy, permission ceiling, and classifier fallback.
2. [Spec 030 `## Implementation PRDs`](../SPEC.md#implementation-prds) assigns this PRD as the sole Spec 030 owner of the unified policy and safety correlation snapshot.
3. [Spec 030 `### Stronger Contract Owner Map`](../SPEC.md#stronger-contract-owner-map) requires one immutable typed correlation ref while keeping Spec 035 ownership of config, context, provider execution snapshot persistence, and migration.
4. [Spec 030 `### Baseline Conformance vs Final Closure`](../SPEC.md#baseline-conformance-vs-final-closure) says current baseline remains conformant until stronger PRDs close.
5. [Spec 022 PRD 001 `## 데이터/상태 모델`](../../022-auto-approval-permissions/prds/001-permissioned-action-normalization-and-decision-gate.md#데이터상태-모델) supplies current `PermissionedAction`, `action_digest`, `argument_digest`, `snapshot_digest`, permission mode snapshot, containment ref, target refs, and origin.
6. [Spec 022 PRD 004 `## 데이터/상태 모델`](../../022-auto-approval-permissions/prds/004-approval-request-cache-and-user-decision-correlation.md#데이터상태-모델) supplies approval request and decision correlation using action digest and snapshot digest.
7. Current Rust inputs are `crates/shacs-core/src/runtime/permission_action.rs`, `crates/shacs-core/src/runtime/permission_approval.rs`, and `crates/shacs-core/src/runtime/permission_audit.rs`.
8. [Spec 035 `## 035가 소유하는 열린 범위`](../../035-configuration-runtime-layout-and-execution-snapshots/SPEC.md#035가-소유하는-열린-범위), [Spec 035 `## Must Have`](../../035-configuration-runtime-layout-and-execution-snapshots/SPEC.md#must-have), and [Spec 035 `## Acceptance Criteria`](../../035-configuration-runtime-layout-and-execution-snapshots/SPEC.md#acceptance-criteria) keep execution snapshot creation, storage, provenance, migration, and immutability outside this PRD.

## Dependency Cut

1. Spec 030 owns the meaning of the policy and safety correlation ref.
2. PRD 000 foundation readiness closes when the typed ref, canonical digest, approval/audit/replay/diagnostics foundation consumers, stale rejection, mismatch rejection, and redacted diagnostics projection pass.
3. PRD 002 may start after PRD 000 foundation readiness. PRD 002 owns process envelope missing-ref rejection, process receipt linkage, pre-spawn process consumer tests, and process QA.
4. PRD 003 through PRD 005 may consume `PolicySafetySnapshotRef` after PRD 000 foundation readiness, but their consumer evidence remains their own completion evidence.
5. PRD 006 must audit all downstream consumer receipts before final Spec 030 closure. That audit is not a PRD 000 prerequisite, which avoids a semantic cycle.
6. Spec 035 may later store or replay external execution snapshots that include this ref, but it must not redefine the ref fields or digest semantics.
7. Current distributed permission code remains valid before this PRD is implemented. The implementation must characterize that baseline before adding new behavior.
8. The first implementation must not require a new database, runtime directory, or migration family.

## Exact Typed Contract

The future implementation must define a typed contract equivalent to the following Rust shape. Names may move to the module that best fits the existing runtime, but field meaning must stay fixed.

```rust
pub struct PolicySafetySnapshotRef {
    pub schema_id: PolicySafetySnapshotSchemaId,
    pub snapshot_id: PolicySafetySnapshotId,
    pub policy_safety_digest: PolicySafetyDigest,
    pub created_at_unix_ms: u64,
    pub redacted_summary: RedactedPolicySafetySummary,
}

pub struct PolicySafetySnapshot {
    pub schema_id: PolicySafetySnapshotSchemaId,
    pub snapshot_id: PolicySafetySnapshotId,
    pub created_at_unix_ms: u64,
    pub expires_at_unix_ms: Option<u64>,
    pub permission_mode: PermissionModeSnapshot,
    pub capability_ceiling: CapabilityCeilingRef,
    pub containment: Option<ContainmentSnapshotRef>,
    pub source_refs: Vec<PolicySafetySourceRef>,
    pub provenance_refs: Vec<PolicySafetyProvenanceRef>,
    pub creation_reason: PolicySafetySnapshotCreationReason,
    pub redacted_summary: RedactedPolicySafetySummary,
}
```

Required enum values:

| Type | Required values |
|---|---|
| `PolicySafetySnapshotSchemaId` | `policy_safety_snapshot.v1` |
| `PolicySafetySnapshotCreationReason` | `permissioned_action`, `approval_request`, `approval_replay`, `diagnostics_replay`, `downstream_consumer` |
| `PolicySafetySourceRef.kind` | `permission_config`, `session_option`, `inherited_context`, `containment_evidence`, `runtime_policy`, `external_execution_snapshot_ref` |
| `PolicySafetyProvenanceRef.kind` | `config_profile_ref`, `context_snapshot_ref`, `provider_execution_snapshot_ref`, `trust_record_ref`, `runtime_event_ref`, `diagnostics_ref` |

Field rules:

1. `schema_id` is part of every comparison. Unknown schema means reject, not best effort accept.
2. `snapshot_id` is an opaque local identity for correlation only. It is not a storage path.
3. `policy_safety_digest` is the SHA 256 digest of the canonical snapshot material described below.
4. `created_at_unix_ms` is the local runtime creation time used for stale checks and diagnostics.
5. `expires_at_unix_ms` is optional. If present and now is later than the value, the snapshot is stale.
6. `permission_mode` is the current typed permission mode snapshot. User prompt, tool output, plugin hook output, subagent prompt, and skill content cannot raise it.
7. `capability_ceiling` records the maximum capabilities admitted at this boundary. Children and deferred boundaries can only narrow it.
8. `containment` is a ref to containment evidence. Unknown is allowed as evidence but never safe evidence.
9. `source_refs` name the policy inputs used to build this snapshot without storing raw config, context, provider payload, or secrets.
10. `provenance_refs` point to external owner evidence when available. They are refs only and cannot force Spec 035 storage decisions.
11. `creation_reason` explains why a snapshot was made and is part of digest material.
12. `redacted_summary` is safe for approval prompts, audit, replay diagnostics, and bundle diagnostics. It cannot contain raw tool args, raw provider payload, raw secret values, absolute host paths, or process handles.

## Canonical Digest

The implementation must compute `policy_safety_digest` from a deterministic JSON representation with sorted object keys and stable array ordering. The digest material is exactly:

1. `schema_id`
2. `snapshot_id`
3. `created_at_unix_ms`
4. `expires_at_unix_ms`
5. `permission_mode`
6. `capability_ceiling`
7. `containment`
8. `source_refs`
9. `provenance_refs`
10. `creation_reason`
11. `redacted_summary`

The digest must not include storage path, memory address, display text outside `redacted_summary`, raw provider input, raw config value, raw context block, or raw secret value. Reordering `source_refs` or `provenance_refs` before canonicalization must not change the digest if the entries are semantically identical. Changing permission mode, ceiling, containment digest, source refs, provenance refs, creation reason, or redacted summary must change the digest.

## Field and Consumer Matrix

| Field | PRD 000 foundation consumers | PRD 002 downstream process consumer | PRD 006 final audit |
|---|---|---|---|
| `schema_id` | permissioned action, approval, audit, replay, diagnostics reject unknown schema | process envelope rejects unknown schema before spawn | verify every downstream receipt records known schema |
| `snapshot_id` | bind request, decision, audit, replay, diagnostics to exact ref | bind process envelope and receipt to exact ref | verify no consumer uses an unlinked ref |
| `policy_safety_digest` | replace legacy `snapshot_digest` comparison for foundation consumers | bind process receipt and mutation rejection | verify digest equality or rejection across all PRDs |
| `created_at_unix_ms` | audit and diagnostics show snapshot age | receipt carries foundation creation time | verify stale windows are evidenced |
| `expires_at_unix_ms` | approval and replay reject stale snapshots | launch rejects stale snapshot | verify stale rejection receipts exist |
| `permission_mode` | policy input, approval risk input, audit mode, diagnostics mode source | pre-spawn gate input | verify no prompt or content raised mode |
| `capability_ceiling` | approval scope and replay compare ceiling | process requested capability cannot exceed ceiling | verify non-widening receipts exist |
| `containment` | policy input, approval risk input, audit summary, diagnostics warning | PRD 003 proof input via PRD 002 envelope | verify unknown or unsafe never became safe |
| `source_refs` | bind approval context and replay without live lookup | envelope source set input | verify source mismatch rejection exists |
| `provenance_refs` | carry external refs without owning storage | envelope external owner refs | verify external refs do not imply 035 storage |
| `creation_reason` | distinguish action, approval replay, diagnostics replay | `downstream_consumer` for PRD 002 handoff | verify reason-specific rejection exists |
| `redacted_summary` | approval, audit, replay, diagnostics safe summary | process receipt safe summary | verify no raw secret, payload, path, or process handle |

## Normal Sequence

1. Runtime normalizes a tool candidate into a permissioned action.
2. Runtime builds a `PolicySafetySnapshot` from the current typed permission mode, inherited capability ceiling, containment ref, source refs, provenance refs, creation reason, and redacted summary.
3. Runtime computes canonical `policy_safety_digest` and emits a `PolicySafetySnapshotRef`.
4. Permission policy evaluates the action using the snapshot ref and digest.
5. If policy returns `ask`, the approval request stores the action digest, `policy_safety_digest`, schema id, snapshot id, requested scope, risk summary, and expiry.
6. Approval automation in tests or QA consumes a decision only when request id, action digest, `policy_safety_digest`, schema id, snapshot id, scope, expiry, and consumed state are valid.
7. Audit records store the action id, action digest, `policy_safety_digest`, snapshot ref, decision, reason, approval ref, containment summary, and redacted summary.
8. Replay and diagnostics read the same immutable ref and digest. They do not look up live config, context, provider payload, or storage paths to reinterpret the decision.
9. PRD 002 later consumes the ready ref in process envelopes and records its own consumer evidence before PRD 006 final audit.

## Failure Sequence

1. Approval arrives with the right action digest and a different `policy_safety_digest`.
2. Runtime rejects it with snapshot mismatch before tool execution.
3. Replay receives a snapshot ref with a known id but an unknown schema id.
4. Runtime rejects it as schema mismatch and records redacted diagnostics.
5. Snapshot expiry has passed.
6. Runtime rejects it as stale and asks again or denies according to current permission mode.
7. Source refs differ from the snapshot that created the approval.
8. Runtime rejects the approval as source mismatch because the digest must differ.
9. Containment changes from confirmed non privileged to unknown or unsafe.
10. Runtime rejects reuse or asks again according to static policy and ceiling.
11. Process envelope missing-ref rejection is not proven here. PRD 002 must prove it before PRD 002 completes, and PRD 006 must audit that receipt before final closure.

## Stale and Mismatch Rejection

The PRD 000 foundation implementation must reject all of these before tool side effects:

1. Unknown `schema_id`.
2. `policy_safety_digest` mismatch.
3. `snapshot_id` mismatch when a foundation consumer requires exact ref reuse.
4. Expired `expires_at_unix_ms`.
5. Approval decision created after snapshot expiry.
6. Capability request outside `capability_ceiling`.
7. Containment digest mismatch.
8. Source or provenance ref mismatch.
9. `creation_reason` mismatch for approval replay or diagnostics replay.
10. Missing snapshot ref on foundation consumers after they move to the unified contract.

PRD 002 owns missing snapshot ref rejection for process envelopes and process spawn. PRD 006 audits that PRD 002 evidence exists before final closure.

## TDD Sequence

Implementation must follow this order.

1. Baseline characterization first. Add tests around current `PermissionedAction.snapshot_digest`, approval snapshot mismatch, audit record fields, and diagnostics summary so existing distributed behavior is fixed before the new type lands.
2. Behavior level failing proof. Add failing tests that name machine consumed behavior, not prose. Required PRD 000 foundation failures are `approval_rejects_policy_safety_digest_mismatch`, `approval_rejects_stale_policy_safety_snapshot`, `replay_rejects_unknown_policy_safety_schema`, `foundation_consumer_rejects_missing_policy_safety_ref`, and `diagnostics_projects_redacted_policy_safety_ref_without_raw_values`.
3. Minimum implementation. Add the typed ref, typed snapshot, canonical digest builder, conversion from current permission action input, and approval comparison update. Do not add storage, migration, provider snapshot creation, or process envelope logic.
4. Refactor only after the focused tests pass. Keep legacy `snapshot_digest` as an adapter field only if needed for compatibility during the same change. It must be derived from or compared against `policy_safety_digest`, not a second source of truth.
5. Automated checks. Run focused tests, then clippy, then wider tests named below.
6. Real surface QA. Drive a permissioned action through approval automation, audit, replay, and diagnostics using the agent-executable CLI surface and record the immutable ref and digest in evidence.
7. Downstream audit handoff. Record that PRD 002 still owes process envelope missing-ref evidence and PRD 006 still owes downstream receipt audit.

Test assertions must target typed fields, enum values, digest equality or inequality, rejection errors, artifact JSON fields, and redacted serialized shapes. Tests must not assert natural language prompt sentences.

## Focused Cargo Targets and Commands

Use the workspace manifest explicitly.

```sh
cargo fmt --manifest-path crates/Cargo.toml --all -- --check
cargo test --manifest-path crates/Cargo.toml -p shacs-core permission_action
cargo test --manifest-path crates/Cargo.toml -p shacs-core permission_policy
cargo test --manifest-path crates/Cargo.toml -p shacs-core permission_approval
cargo test --manifest-path crates/Cargo.toml -p shacs-core permission_audit
cargo clippy --manifest-path crates/Cargo.toml -p shacs-core --all-targets -- -D warnings
```

For CI and final closure gates, use locked dependency resolution.

```sh
cargo fmt --manifest-path crates/Cargo.toml --all -- --check
cargo clippy --locked --manifest-path crates/Cargo.toml -p shacs-core --all-targets -- -D warnings
cargo test --locked --manifest-path crates/Cargo.toml -p shacs-core permission_action
cargo test --locked --manifest-path crates/Cargo.toml -p shacs-core permission_policy
cargo test --locked --manifest-path crates/Cargo.toml -p shacs-core permission_approval
cargo test --locked --manifest-path crates/Cargo.toml -p shacs-core permission_audit
cargo test --locked --manifest-path crates/Cargo.toml -p shacs-core runtime
cargo test --locked --manifest-path crates/Cargo.toml --workspace
```

## Agent Executed Real Surface Diagnostics QA

The implementation worker must use a real surface, not only unit tests. No manual user approval step is allowed.

### Required QA command surface

PRD 000 owns the CLI command surface needed to exercise the foundation flow below. If the surface does not exist when implementation reaches QA, the QA result is `BLOCKED` and PRD 000 cannot complete. The required CLI surface is:

```sh
cargo run --locked --manifest-path crates/Cargo.toml -p shacs-cli -- policy-safety qa setup --workspace "$SHACS_QA_WORKSPACE" --artifacts "$SHACS_QA_ARTIFACTS"
cargo run --locked --manifest-path crates/Cargo.toml -p shacs-cli -- policy-safety qa run-action --workspace "$SHACS_QA_WORKSPACE" --artifacts "$SHACS_QA_ARTIFACTS" --fixture write-file-approval --approval-mode auto-approve-exact
cargo run --locked --manifest-path crates/Cargo.toml -p shacs-cli -- policy-safety qa replay --workspace "$SHACS_QA_WORKSPACE" --artifacts "$SHACS_QA_ARTIFACTS" --fixture write-file-approval
cargo run --locked --manifest-path crates/Cargo.toml -p shacs-cli -- policy-safety qa mutate --workspace "$SHACS_QA_WORKSPACE" --artifacts "$SHACS_QA_ARTIFACTS" --field policy_safety_digest --expect reject
cargo run --locked --manifest-path crates/Cargo.toml -p shacs-cli -- policy-safety qa mutate --workspace "$SHACS_QA_WORKSPACE" --artifacts "$SHACS_QA_ARTIFACTS" --field expires_at_unix_ms --value stale --expect reject
cargo run --locked --manifest-path crates/Cargo.toml -p shacs-cli -- runtime diagnostics --workspace "$SHACS_QA_WORKSPACE" --bundle "$SHACS_QA_ARTIFACTS/diagnostics.zip"
```

### Fixture setup and owner

1. PRD 000 implementation owns the `write-file-approval` foundation fixture until PRD 002 introduces process fixtures.
2. The fixture must create a temporary workspace and artifact directory, not mutate the developer workspace.
3. The fixture action is one permissioned filesystem write that requires approval and is safe to execute in the temporary workspace.
4. Approval automation uses a local synthetic actor equivalent to `ApprovalActor::LocalUser` with `approval-mode auto-approve-exact`. It must generate a real `ApprovalRequest` and `ApprovalDecision`; it must not bypass approval correlation.
5. Cleanup must remove the temporary workspace unless `SHACS_QA_KEEP_ARTIFACTS=1` is set. Cleanup receipts are mandatory PRD 006 audit inputs.

### Required artifacts and PASS or FAIL rules

| Artifact | Producer | PASS | FAIL |
|---|---|---|---|
| `$SHACS_QA_ARTIFACTS/fixture.json` | setup command | includes workspace path, artifact path, fixture id, cleanup registry id | missing fixture id or points at repo root |
| `$SHACS_QA_ARTIFACTS/action.json` | run-action command | includes `action_digest`, `schema_id`, `snapshot_id`, `policy_safety_digest`, redacted summary | missing ref or includes raw secret/raw absolute host path |
| `$SHACS_QA_ARTIFACTS/approval.json` | run-action command | request and decision share action digest, snapshot id, schema id, digest, scope, expiry, and consumed state is valid | approval is synthetic allow without request/decision correlation |
| `$SHACS_QA_ARTIFACTS/audit.json` | run-action command | records action id, approval ref, schema id, snapshot id, digest, decision, reason | records only prose or omits machine refs |
| `$SHACS_QA_ARTIFACTS/replay.json` | replay command | accepts only same ref and digest while approval scope is valid | performs live source lookup or changes digest |
| `$SHACS_QA_ARTIFACTS/diagnostics.zip` | diagnostics command | includes redacted schema id, snapshot id, digest, summary, and artifact refs | contains raw provider payload, raw secret, process handle, or unredacted host path |
| `$SHACS_QA_ARTIFACTS/mutation-policy_safety_digest.json` | mutate command | reports `rejected_before_execution` for digest mismatch | executes action or reports success |
| `$SHACS_QA_ARTIFACTS/mutation-expires_at_unix_ms.json` | mutate command | reports `rejected_before_execution` for stale snapshot | executes action or asks after execution |
| `$SHACS_QA_ARTIFACTS/cleanup-receipt.json` | cleanup command or harness shutdown | includes cleanup registry id, removed temp paths, retained artifacts reason | missing cleanup status |

## PRD 006 Cleanup Registry and Receipt Linkage

PRD 000 implementation evidence must emit a cleanup registry record for PRD 006 to audit. The registry entry must include:

1. `owner_prd`: `030-prd-000`
2. `fixture_id`: `write-file-approval`
3. `cleanup_registry_id`
4. `workspace_path_redacted`
5. `artifact_dir_redacted`
6. `created_artifacts`
7. `removed_paths`
8. `retained_paths_with_reason`
9. `policy_safety_digest`
10. `qa_result`: `pass`, `fail`, or `blocked`

PRD 006 final closure must fail if the PRD 000 cleanup registry entry is missing, if the cleanup receipt is not linked to the action/approval/audit/replay/diagnostics artifacts, or if retained artifacts lack a reason.

## Applicable 9 Class Adversarial Matrix

| Class | PRD 000 probe | Required result |
|---|---|---|
| `dirty_worktree` | run QA from a temporary workspace and record `git status --short` before evidence capture | unrelated dirty files are not mutated or claimed |
| `stale_state` | reread Spec 030 owner map and Spec 035 ownership sections before final implementation evidence | PRD 000 still avoids 035 storage and PRD 002 process ownership |
| `misleading_success_output` | inspect artifact JSON fields after commands return success | PASS requires machine fields, not exit code alone |
| `semantic_cycle` | check PRD 000 exit criteria do not require PRD 002 consumer evidence | PRD 000 foundation closes before PRD 002 |
| `ownership_bleed` | search diff for config/context/provider snapshot persistence or process envelope implementation in PRD 000 change | forbidden ownership is absent |
| `manual_approval_dependency` | run approval with `--approval-mode auto-approve-exact` | no manual user step is required |
| `future_surface_absent` | run the required CLI QA command surface | missing surface is `BLOCKED`, not PASS |
| `mutation_rejection_gap` | mutate digest and expiry artifacts | both mutations reject before execution |
| `cleanup_leak` | inspect cleanup registry and receipt | temp workspace is removed or retained with explicit reason |

## Evidence and Exit Criteria

This PRD has two distinct gates.

### PRD 000 foundation readiness gate

PRD 000 is ready for PRD 002 only when the future implementation evidence includes all items below.

1. Baseline characterization tests passed before implementation tests were made green.
2. Failing first proof exists for digest mismatch, stale snapshot, unknown schema, missing foundation ref, and redacted diagnostics projection.
3. `PolicySafetySnapshotRef` and `PolicySafetySnapshot` exist as typed Rust contracts with known schema id and canonical digest.
4. Permissioned action, approval, audit, replay, and diagnostics foundation consumers use one immutable ref and digest.
5. No new config, context, provider execution snapshot creation, persistence path, migration family, adapter immutability rule, physical storage location, process envelope, or process receipt was added under PRD 000.
6. Focused Cargo commands pass, and CI/final Cargo gates use `--locked` where Cargo supports it.
7. Agent-executed CLI QA proves one permissioned action flows through approval automation to audit, replay, and diagnostics using the same ref and digest.
8. Mutation artifacts prove stale and mismatch rejection before execution.
9. Cleanup registry and receipt artifacts are produced for PRD 006 audit.

### Downstream consumer audit gate

PRD 000 does not close downstream consumers. PRD 002 owns process envelope missing-ref and receipt evidence. PRDs 003 through 005 own their own consumer evidence. PRD 006 must audit all downstream receipts before final Spec 030 closure.

## Closure Gate for Consumers

PRDs 002 through 005 and PRD 006 may consume this PRD after the PRD 000 foundation readiness gate passes. They must not assume Spec 035 execution snapshot storage exists unless Spec 035 separately provides evidence. They must not treat PRD 000 foundation readiness as proof that process envelopes, containment inheritance, classifier accounting, or skill-trust consumer receipts are complete.
