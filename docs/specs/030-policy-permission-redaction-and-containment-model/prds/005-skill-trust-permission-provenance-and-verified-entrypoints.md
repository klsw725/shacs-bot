# PRD 005. skill trust permission provenance and verified entrypoints

Status: Planned

## Goal

This PRD defines how active external skill trust provenance becomes bounded permission input for dependency preparation and verified entrypoint actions.

The goal is narrow. Spec 032 and Spec 035 produce lifecycle and persistence evidence. This PRD consumes that evidence, checks exact source, content, dependency, capability, lifecycle, snapshot, and envelope matches, then lets the normal permission pipeline decide whether a dependency preparation or verified entrypoint action may continue.

Skill name, Markdown body, body digest, requirements text, install metadata text, or plugin-provided skill content never authorize package installation, shell execution, dependency preparation, or entrypoint execution by themselves.

## Scope

1. Define `SkillTrustPermissionInput` as the typed input consumed by permission policy for `dependency_preparation` and `verified_entrypoint` actions.
2. Define exact digest matching for skill identity, source identity, content, dependency manifest, package set, verified entrypoint, and capability scope.
3. Define lifecycle status checks for active, stale, revoked, removed, pending, malformed, and missing trust provenance.
4. Define how PRD 000 snapshot refs, PRD 001 secret refs, PRD 002 process envelopes, and PRD 003 containment proof refs carry trust provenance without making it a grant.
5. Define static policy and permission ceiling precedence over any trust provenance.
6. Define approval invalidation when lifecycle state, digest, snapshot, envelope, containment proof, dependency scope, capability scope, cancellation, or repeated interruption changes.
7. Define TDD order, focused Cargo commands, workspace Cargo gates, literal real-surface QA, external completion gates, closure artifacts, and exit criteria.

## Non-Scope

1. Trust registry domain model, state transitions, inspect, revoke, stale marking, removed marking, and lifecycle receipt vocabulary are Spec 032 scope.
2. Trust record persistence, schema migration, runtime layout, mutation admission, snapshot persistence, and execution snapshot storage are Spec 035 scope.
3. Dependency installer behavior, package manager choice, package cache layout, package verification internals, native build handling, lifecycle script handling, and dependency storage paths are Specs 032 and 035 scope.
4. Verified entrypoint runner lifecycle, process supervision, retry policy, and entrypoint result domain vocabulary are Specs 032 and 035 scope.
5. AppSupervisor lifecycle, app start, stop, restart, recover, and app process state remain Spec 032 scope.
6. UI projection, diagnostics rendering parity, and release artifact rendering remain Spec 031 scope.
7. This PRD does not require a remote marketplace, hosted signature service, organization approval console, fleet policy rollout, hosted vault, or kernel isolation.
8. This PRD does not turn skill discovery registry, session approval cache, prompt text, tool output, plugin output, or skill body text into trust provenance.

## SPEC Inputs

1. Spec 030 `소유하는 open scope` defines the read-only skill boundary, trust-derived package install, verified entrypoint authorization, and common process gate as final closure targets, not current runtime blockers.
2. Spec 030 `Implementation PRDs` and `Stronger Contract Owner Map` assign this PRD as the sole Spec 030 owner for consuming active trust provenance while keeping registry transitions, dependency installation, runner lifecycle, and persistence external.
3. Spec 030 `External Dependency Gates` keeps Spec 032 ownership of trust registry lifecycle and Spec 035 ownership of trust persistence and execution snapshots.
4. Spec 030 `Must Have`, `Must Not Have`, `Acceptance Criteria`, and `Source Handoff Table` forbid using skill name, Markdown body, content digest alone, or manifest-outside dependency text as authorization.
5. PRD 000 supplies `PolicySafetySnapshotRef`, `policy_safety_digest`, and `trust_record_ref` provenance refs.
6. PRD 001 supplies `SecretRef` and `RedactionEvidenceRef` for skill trust bindings without raw secret values.
7. PRD 002 supplies `ProcessExecutionEnvelope` for `dependency_preparation` and `verified_entrypoint` adapters.
8. PRD 003 blocks these boundaries until active exact-match trust provenance exists, then still requires equal-or-narrower containment, workspace, and ceiling proof.
9. Spec 005 supplies the current skill boundary: `SkillSourceKind`, `SkillRegistryStatus`, `SkillDescriptor`, `body_hash`, `requirements`, `install_metadata`, read-only context injection, and CLI inspect evidence. It does not supply permission grants.
10. Spec 032 supplies skill install proposal semantics, dependency manifest meaning, trust lifecycle status, inspect and revoke lifecycle, approved dependency preparation, prerequisite separation, and verified entrypoint lifecycle facts.
11. Spec 035 supplies trust record schema version, persistence, migration, owner-safe mutation, execution snapshot reference, and the rule that stale, revoked, or mismatched records are not allow provenance.

## Dependency Cut

1. PRD 000 must close first. Every trust permission input carries a known policy and safety snapshot ref and digest.
2. PRD 001 must close first when trust provenance carries secret refs for dependency or entrypoint env slots.
3. PRD 002 must close first. This PRD authorizes only typed process envelope candidates, not raw installer or runner calls.
4. PRD 003 must close for process admission. This PRD says trust provenance is active and exact; PRD 003 proves containment, workspace, and ceiling do not widen.
5. Spec 032 must produce active trust lifecycle evidence before this PRD can allow exact-match reuse.
6. Spec 035 must produce persisted trust ref and execution snapshot refs before PRD 006 can count closure evidence.
7. Current read-only skill discovery remains valid before this PRD is implemented. Dependency preparation and verified entrypoint closure stay blocked until external gates produce evidence.

## Exact Typed Trust Provenance Input

The future implementation must define a typed contract equivalent to this Rust shape. Module names may change, but field meaning and rejection behavior must not change.

```rust
pub struct SkillTrustPermissionInput {
    pub schema_id: SkillTrustPermissionSchemaId,
    pub input_id: SkillTrustPermissionInputId,
    pub action_kind: SkillTrustActionKind,
    pub trust_record_ref: TrustRecordRef,
    pub trust_owner_ref: TrustOwnerRef,
    pub lifecycle_status: TrustLifecycleStatus,
    pub lifecycle_status_digest: TrustLifecycleStatusDigest,
    pub approved_by_local_user_ref: LocalApprovalActorRef,
    pub approved_at_unix_ms: u64,
    pub expires_at_unix_ms: Option<u64>,
    pub skill_identity: SkillIdentityMatch,
    pub source_identity: SkillSourceIdentityMatch,
    pub content_identity: SkillContentIdentityMatch,
    pub dependency_identity: SkillDependencyIdentityMatch,
    pub capability_identity: SkillCapabilityIdentityMatch,
    pub entrypoint_identity: Option<VerifiedEntrypointIdentityMatch>,
    pub policy_safety_snapshot_ref: PolicySafetySnapshotRef,
    pub process_envelope_id: ProcessEnvelopeId,
    pub containment_proof_ref: Option<ContainmentPermissionProofRef>,
    pub secret_refs: Vec<SecretRef>,
    pub redaction_evidence_refs: Vec<RedactionEvidenceRef>,
    pub execution_snapshot_ref: Option<ExecutionSnapshotRef>,
    pub staleness_token: TrustStalenessToken,
    pub canonical_input_digest: SkillTrustPermissionDigest,
    pub redacted_summary: RedactedSkillTrustPermissionSummary,
}
```

Required enum values and field rules:

| Type or field | Required contract |
|---|---|
| `SkillTrustPermissionSchemaId` | Only `skill_trust_permission_input.v1` is valid. Unknown schema rejects before policy. |
| `SkillTrustActionKind` | `dependency_preparation` or `verified_entrypoint`. Any other action kind is malformed for this PRD. |
| `TrustLifecycleStatus` | `active`, `stale`, `revoked`, `removed`, `pending`, `malformed`, `missing`. Only `active` may continue. |
| `trust_record_ref` | Opaque ref from 032 and 035. It is not a registry path, package path, or approval cache key. |
| `trust_owner_ref` | Names the external lifecycle owner that can say whether the trust record is active now. |
| `approved_by_local_user_ref` | Safe local approval actor ref from the trust proposal lifecycle. This PRD does not own the approval UI or registry write. |
| `expires_at_unix_ms` | If present and expired, the input is stale. The value cannot outlive the policy snapshot or owner lifecycle proof. |
| `skill_identity` | Includes skill name, registry source kind, descriptor ref, and descriptor digest. Name is display identity only and cannot authorize alone. |
| `source_identity` | Includes source kind, source ref, source digest, plugin or workspace owner ref when present, and source staleness token. |
| `content_identity` | Includes approved content digest, current content digest, descriptor `body_hash`, normalized content digest, and included file set digest when available. |
| `dependency_identity` | Includes approved dependency manifest digest, current dependency manifest digest, pinned package set digest, expected package/version digest, required runtime kind, and native build or lifecycle script flags. |
| `capability_identity` | Includes approved capability scope digest, current requested capability digest, workspace or network scope refs, and permission capability set. |
| `entrypoint_identity` | Required for `verified_entrypoint`, absent for pure `dependency_preparation`. It includes approved entrypoint digest, current entrypoint digest, declared command ref, redacted argv schema digest, and owning manifest digest. |
| `policy_safety_snapshot_ref` | Required and must match PRD 000 digest used by the process envelope and approval. |
| `process_envelope_id` | Required and must match the PRD 002 envelope being evaluated. A trust input for one envelope cannot authorize another. |
| `containment_proof_ref` | Optional at input parse, required before PRD 002 spawn admission when PRD 003 closes. |
| `secret_refs` | Secret refs only. Raw secret, package repository credential, full env, or resolved token rejects as malformed input. |
| `execution_snapshot_ref` | Ref from Spec 035 when available. Missing ref blocks PRD 006 closure but does not let 030 choose a storage path. |
| `staleness_token` | Must match current owner state from 032 or 035. Mismatch rejects as stale. |
| `canonical_input_digest` | Digest over all machine fields except display-only redacted text. It is correlation evidence, not raw payload proof. |
| `redacted_summary` | Safe for approval, receipt, diagnostics, and release evidence. It cannot contain raw skill body, raw dependency file, raw command text with secrets, raw env, absolute host path, process handle, or raw package credential. |

## Exact Digest Matching

Exact match means the approved value, the current value, and the value embedded in the process envelope all compare equal where that field applies. A display string match is never enough.

| Match dimension | Approved field | Current field | Envelope field | Rejection when different |
|---|---|---|---|---|
| Skill identity | approved skill name, registry source kind, descriptor digest | current active descriptor identity | `entrypoint_ref.owner_source` | `skill_identity_mismatch` |
| Source identity | approved source ref, source kind, source digest | current source ref and digest from active registry or lifecycle owner | source ref in `entrypoint_ref` | `source_mismatch` |
| Content identity | approved content digest and included file set digest | current content digest and current `body_hash` projection | content digest in `entrypoint_ref` | `content_digest_mismatch` |
| Dependency manifest | approved dependency manifest digest | current dependency manifest digest | dependency manifest digest in dependency refs | `dependency_manifest_mismatch` |
| Package set | approved pinned package set digest | prepared or missing package set digest | package ref digest in executable or dependency refs | `package_set_mismatch` |
| Required runtime | approved runtime kind and version range | current runtime availability fact from external owner | runtime ref in dependency envelope | `runtime_prerequisite_mismatch` |
| Capability scope | approved capability scope digest | requested capability set and scope digest | `permission_action.capability_set` | `capability_scope_mismatch` |
| Verified entrypoint | approved entrypoint digest | current entrypoint digest | `entrypoint_ref` digest | `entrypoint_digest_mismatch` |
| Policy snapshot | approved policy safety digest | current policy safety digest | `policy_safety_snapshot_ref` | `snapshot_mismatch` |
| Process envelope | approved or freshly built envelope id | current envelope id | `process_envelope_id` | `envelope_mismatch` |
| Secret binding | approved secret ref ids and safe consumer slots | current secret refs from 001 | envelope `secret_refs` | `secret_ref_mismatch` |
| Lifecycle state | active lifecycle status digest | current lifecycle status digest | trust provenance ref | `lifecycle_status_mismatch` |

`SkillDescriptor.body_hash` is useful read-only evidence, but it is not sufficient alone. An exact match must include source identity, content identity, dependency identity, capability identity, lifecycle status, and the current envelope and snapshot refs.

## Trust Input Validation Matrix

| Input condition | Dependency preparation | Verified entrypoint | Required decision |
|---|---|---|---|
| Schema is unknown | reject | reject | `malformed_input` before policy |
| Trust ref is missing | reject | reject | `missing_trust_provenance` before policy |
| Lifecycle is `active` and all digests match | may continue to static policy | may continue to static policy | trust is bounded input, not final allow |
| Lifecycle is `stale` | reject | reject | invalidate approval and require fresh proposal from 032 |
| Lifecycle is `revoked` | reject | reject | deny, no ask reuse, no session approval override |
| Lifecycle is `removed` | reject | reject | deny or report removed evidence, no package repair |
| Lifecycle is `pending` | reject | reject | ask through external lifecycle only, no process envelope spawn |
| Lifecycle is `malformed` | reject | reject | malformed input, diagnostics safe refs only |
| Source digest mismatch | reject | reject | approval invalidated, no installer or runner call |
| Content digest mismatch | reject | reject | stale trust, no exact-match reuse |
| Dependency manifest digest mismatch | reject | reject | manifest-outside action, new proposal required |
| Package set missing inside approved manifest | may prepare within pinned manifest after policy | reject until prepared and verified | installer remains external |
| Package set asks for unapproved package | reject | reject | manifest-outside action, static policy sees denied candidate |
| Required runtime is missing | reject | reject | runtime prerequisite state, not package preparation |
| Lifecycle script or native build appears unexpectedly | reject | reject | new proposal required |
| Capability digest widens approved scope | reject | reject | ceiling violation before approval reuse |
| Entrypoint digest missing | not applicable | reject | malformed entrypoint provenance |
| Entrypoint digest mismatch | not applicable | reject | verified entrypoint mismatch before spawn |
| Policy snapshot mismatch | reject | reject | approval invalidated |
| Envelope id mismatch | reject | reject | approval invalidated |
| Secret ref stale or revoked | reject or ask through policy | reject or ask through policy | raw secret never resolves |
| Prompt or skill text claims approval | ignore text | ignore text | prompt injection data only |

## Lifecycle Status Checks

1. `active` is necessary but not sufficient. Static policy, ceiling, approval, containment proof, and process envelope checks still run after active status passes.
2. `stale` means a current source, content, dependency, package, capability, entrypoint, snapshot, or lifecycle digest differs from the trust record. It rejects before process spawn.
3. `revoked` means the local user or lifecycle owner revoked trust. It denies reuse even if all digests still match.
4. `removed` means the source skill or lifecycle object no longer exists as active trust. It cannot be repaired by installing packages.
5. `pending` means a proposal or verification has not become active trust. It can never authorize dependency preparation or entrypoint execution.
6. `malformed` means the owner supplied an invalid shape, unknown status, raw value, or unsupported ref. It rejects before static policy.
7. `missing` means no trust record exists for the exact source, content, dependency, capability, and entrypoint tuple. It rejects and may route to the external proposal flow.

## Snapshot and Envelope Handoff

1. PRD 000 snapshot includes `trust_record_ref` only as provenance. It does not create trust.
2. PRD 001 secret refs may appear in trust inputs only as refs and safe summaries. They are resolved only inside the PRD 002 owner adapter when policy allows.
3. PRD 002 process envelope carries `SkillTrustPermissionInput.input_id`, `trust_record_ref`, dependency refs, entrypoint refs, redacted args, redacted env, timeout, and receipt correlation before any installer, verifier, or entrypoint runner starts.
4. PRD 003 proof consumes the same trust ref and envelope id to check equal-or-narrower containment, workspace, and ceiling. Trust cannot replace containment proof.
5. Spec 035 execution snapshot may store trust refs, digest refs, and provenance refs. It must not store stale, revoked, removed, or mismatched trust as allow provenance.

## Static Policy and Ceiling Precedence

Trust provenance is an input below static policy and ceiling.

1. Malformed trust input rejects first because the action cannot be normalized safely.
2. Missing, stale, revoked, removed, pending, or mismatched trust rejects before approval reuse.
3. Static policy evaluates protected targets, raw credential export, proc exec summary, manifest-outside dependency, unknown unsafe containment, native build or lifecycle script flags, and runtime prerequisite state before any installer or runner call.
4. Static deny wins over active trust.
5. Permission ceiling wins over active trust.
6. Static ask remains ask unless a fresh matching approval is correlated for the same action, snapshot, envelope, trust digest, scope, and expiry.
7. Classifier allow cannot override static deny, ceiling rejection, trust mismatch, or revoked lifecycle state.
8. Process spawn may happen only after PRD 002 and PRD 003 admit the same envelope and proof.

## Exact Decision Ordering

The only valid order is:

1. Parse `SkillTrustPermissionInput` and reject unknown schema, raw fields, missing required refs, unsupported action kind, or malformed digest fields.
2. Query or consume the external owner proof from 032 and 035 for the current trust record status. This step reads status evidence only. It does not mutate the registry.
3. Require lifecycle status `active` and matching `lifecycle_status_digest` and `staleness_token`.
4. Compare skill, source, content, dependency manifest, package set, required runtime, capability, entrypoint, secret ref, snapshot, and envelope digests exactly.
5. Normalize the action into the PRD 002 process envelope for `dependency_preparation` or `verified_entrypoint`.
6. Run static policy. Reject manifest-outside packages, unapproved lifecycle scripts, native build escalation, global install, runtime installer escalation, raw credential export, protected target access, and unsafe containment states.
7. Run permission ceiling. Reject capability, workspace, network, filesystem, process, or mode widening.
8. Run approval correlation only if static policy asks. Approval must match action digest, policy safety digest, trust input digest, envelope id, scope, expiry, consumed state, and lifecycle status digest.
9. Run PRD 003 containment and ceiling proof for the same envelope.
10. Hand the allowed envelope to the external dependency preparation or verified entrypoint adapter. Installer and runner behavior stay external.
11. Record a redacted receipt, trust decision refs, and diagnostics refs. Raw skill body, raw dependency file, raw env, raw package credential, raw stdout, raw stderr, process handles, and absolute host paths cannot persist.

## Approval Invalidation

An existing approval is invalid and cannot be consumed when any of these changes after approval creation:

1. Trust lifecycle status, lifecycle status digest, staleness token, or trust owner ref.
2. Skill source kind, source ref, source digest, or descriptor digest.
3. Content digest, descriptor `body_hash`, included file set digest, or dependency manifest digest.
4. Package set digest, resolved version digest, required runtime fact, native build flag, lifecycle script flag, or package repository ref.
5. Capability scope digest, workspace scope, network scope, filesystem scope, process capability, or permission mode.
6. Verified entrypoint digest, owning manifest digest, command ref, or argv schema digest.
7. Policy safety snapshot ref, policy safety digest, process envelope id, containment proof id, or secret ref id.
8. Cancellation, timeout, runtime recover, AppSupervisor restart, plugin reload, skill source reload, trust revoke, skill removal, or dependency verification failure.
9. Repeated interruption after denial or cancellation. The prior terminal denial remains terminal until fresh matching evidence is built.

## Normal Sequences

### Dependency Preparation

1. Spec 032 produces an active trust record for a skill source, content digest, dependency manifest digest, pinned package set, required runtime, and capability scope.
2. Spec 035 persists the trust ref and supplies execution snapshot refs without raw package credentials or raw env.
3. Runtime receives a dependency preparation candidate because an approved package is missing.
4. Runtime builds `SkillTrustPermissionInput` with action kind `dependency_preparation` and exact current digests.
5. Runtime validates active lifecycle status and exact matches, then normalizes a PRD 002 process envelope.
6. Static policy and ceiling run. A package inside the approved pinned manifest may continue. Manifest-outside package, global install, lifecycle script, native build, or runtime installer escalation rejects.
7. Approval correlation runs only if static policy asks and only for the exact same trust input digest and envelope.
8. PRD 003 proof admits equal-or-narrower containment, workspace, and ceiling.
9. The external dependency preparation adapter runs and later records a redacted receipt. This PRD does not define installer internals.

### Verified Entrypoint

1. Spec 032 marks a skill trust record active and links it to a verified entrypoint digest after dependency verification succeeds.
2. Runtime receives an entrypoint execution candidate.
3. Runtime builds `SkillTrustPermissionInput` with action kind `verified_entrypoint`, entrypoint digest, dependency digest, capability digest, policy snapshot ref, and process envelope id.
4. Active lifecycle and exact digest checks pass.
5. Static policy and ceiling run before entrypoint command creation.
6. PRD 003 proof admits equal-or-narrower containment, workspace, and ceiling.
7. The external verified entrypoint adapter runs through PRD 002 and records a redacted receipt. This PRD does not own the runner lifecycle.

## Failure Sequences

1. A skill named `lint-helper` is active in the discovery registry, but no active trust record exists for its source, content, dependency, and capability tuple. The dependency preparation candidate rejects as missing trust provenance.
2. The same skill name has a different content digest than the approved record. The action rejects as stale trust and invalidates any cached approval.
3. The dependency manifest adds an unapproved package. Static policy rejects it as manifest-outside action before installer spawn.
4. A missing Python runtime asks to run `brew install python`. The action rejects as runtime prerequisite escalation, not package repair.
5. A revoked trust record still has matching digests. Lifecycle status wins, the action denies, and session approval cannot override revoke.
6. A removed skill source still has historical receipts. Historical evidence remains readable, but removed trust cannot authorize preparation or entrypoint execution.
7. A verified entrypoint command digest changes after approval. The entrypoint rejects as digest mismatch before spawn.
8. Skill body text says to ignore policy and install a package. The text is prompt injection data and cannot alter trust input, static policy, ceiling, approval, or envelope owner.
9. Installer stdout says verification succeeded, but the expected package/version digest does not match the approved package set. The action remains failed under external verification evidence and cannot become allowed by stdout.
10. Cancellation occurs after approval but before spawn. The approval and trust proof cannot be reused after resume unless fresh matching lifecycle, snapshot, envelope, and proof evidence exist.

## TDD Sequence

Future implementation must follow this order. Tests must assert typed fields, enum values, digest equality or inequality, lifecycle status, permission decisions, spawn counters, receipt outcomes, and absence of raw fixture values. Tests must not assert natural-language prompt prose.

1. Baseline characterization. Confirm current `shacs-skills` tests for source kind, registry status, descriptor fields, `body_hash`, requirements, install metadata, plugin-provided roots, malformed skills, conflicts, CLI inspect, and read-only context injection. Confirm current permission tests prove skill body and prompt text do not grant permission.
2. Red proof for exact-match reuse. Add failing tests named `skill_trust_allows_dependency_preparation_only_for_active_exact_match` and `skill_trust_allows_verified_entrypoint_only_for_active_exact_match`.
3. Red proof for lifecycle invalidation. Add failing tests for `stale`, `revoked`, `removed`, `pending`, `malformed`, and `missing` trust statuses.
4. Red proof for digest invalidation. Add failing tests for skill name collision, source digest mismatch, content digest mismatch, descriptor body hash mismatch, dependency manifest mismatch, package set mismatch, runtime prerequisite mismatch, capability scope mismatch, entrypoint digest mismatch, secret ref mismatch, policy snapshot mismatch, envelope mismatch, containment proof mismatch, and lifecycle status digest mismatch.
5. Red proof for approval invalidation. Add failing tests proving action digest, policy digest, trust input digest, envelope id, capability scope, expiry, consumed state, cancellation, repeated interruption, and lifecycle changes reject approval reuse.
6. Red proof for static policy and ceiling precedence. Add failing tests named `skill_trust_cannot_override_static_deny`, `skill_trust_cannot_override_permission_ceiling`, `skill_trust_rejects_manifest_outside_dependency`, `skill_trust_rejects_runtime_installer_escalation`, and `skill_trust_rejects_unapproved_native_build_or_lifecycle_script`.
7. Red proof for prompt injection and misleading output. Add tests proving skill body, skill name, install metadata text, prompt text, plugin output, MCP prompt, stdout, and stderr cannot authorize or change a trust decision.
8. Red proof for process handoff. Add tests proving dependency preparation and verified entrypoint candidates cannot spawn without PRD 002 envelope id, PRD 000 snapshot ref, active trust input digest, and PRD 003 proof where required.
9. Minimal implementation. Add only the typed input parser, digest comparison, lifecycle status consumer, permission decision hook, approval correlation fields, redacted diagnostics refs, and process envelope handoff. Do not add registry transitions, installer behavior, package verification, runner lifecycle, or storage paths under Spec 030.
10. Refactor only after focused tests pass. Keep skill registry read-only and keep trust lifecycle and persistence ownership external.
11. Final regression. Run focused Cargo commands, workspace gates, literal real-surface QA, external completion gate review, and closure artifact audit before PRD 006 may consume the evidence.

## Focused Cargo Targets and Commands

Future implementation must use the workspace manifest explicitly.

```sh
cargo fmt --manifest-path crates/Cargo.toml --all -- --check
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-skills
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core skill_trust
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core process_gate
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core permission_policy
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-app skill_trust
cargo clippy --manifest-path crates/Cargo.toml --locked -p shacs-core --all-targets -- -D warnings
```

Before PRD 006 can consume closure evidence, run the workspace gates from `AGENTS.md`:

```sh
cargo fmt --manifest-path crates/Cargo.toml --all -- --check
cargo clippy --manifest-path crates/Cargo.toml --locked --workspace --all-targets -- -D warnings
cargo test --manifest-path crates/Cargo.toml --locked --workspace
```

No authoring worker for this documentation task should run Cargo. The commands above are future implementation gates.

## Literal Real-Surface QA and Fixture Ownership

This PRD must not claim that `skills trust inspect`, `skills prepare`, or `skills run-entrypoint` exist today. Those surfaces are future 032 or 035 deliverables. Until they exist with evidence, dependency-preparation and verified-entrypoint QA is `BLOCKED_EXTERNAL_SURFACE`, not pass.

All future QA must use an evidence directory chosen by the implementation worker, referred to below as `<evidence-dir>/prd005/`. Every artifact must be redacted JSON, a command transcript, or a bounded test output. Every row has binary result `PASS` or `FAIL`. `BLOCKED_EXTERNAL_SURFACE` is neither pass nor failure for this PRD implementation; PRD 006 cannot consume closure while any required external surface remains blocked.

| Fixture | Owner | Deterministic setup | Invocation | Required artifacts | PASS | FAIL | Cleanup |
|---|---|---|---|---|---|---|---|
| Current skill registry non-grant baseline | Current PRD 005 worker using existing `shacs-skills` and CLI skill inspect surfaces | Create `<tmp-workspace>/skills/prd005-safe/SKILL.md` with fixed frontmatter name, description, requirements, install metadata text, and harmless body. Create `<tmp-workspace>/skills/prd005-stale/SKILL.md` with same name or altered body for conflict and digest fixtures. | Use current skill list/show or current `shacs-skills` focused tests only. Do not use trust lifecycle commands. | `<evidence-dir>/prd005/current-skill-registry/skill-list.json`, `skill-show.json`, `body-hash.txt`, `non-grant-audit.json`, `command-transcript.txt` | Artifact shows source kind, status, descriptor, `body_hash`, requirements, install metadata, malformed or conflicted state where applicable, and no permission allow, trust record, installer, runner, or package preparation. | Any artifact treats skill name, body, `body_hash`, requirements, or install metadata as trust approval or permission allow. | Remove `<tmp-workspace>` after copying artifacts. |
| Skill trust lifecycle fixture | Spec 032 | Spec 032 must introduce a deterministic fixture or API that emits active, stale, revoked, removed, pending, malformed, and missing trust records for the same skill/source/content/dependency/capability tuple without relying on wall clock. | Spec 032-owned command or local API, named in Spec 032 closure evidence. This PRD does not prescribe the command name. | `<evidence-dir>/prd005/external/spec032/trust-lifecycle/active.json`, `stale.json`, `revoked.json`, `removed.json`, `pending.json`, `malformed.json`, `missing.json`, `owner-command.txt` | Each artifact contains trust record ref, owner ref, lifecycle status, lifecycle status digest, staleness token, approval actor ref, approved at time, safe summary, and no raw secret or raw dependency payload. | Missing lifecycle status, nondeterministic state, no owner command/API locator, raw secret, or evidence that discovery/session approval cache is used as trust registry. | Spec 032 fixture cleanup removes temp lifecycle store and preserves copied evidence. |
| Trust persistence and execution snapshot fixture | Spec 035 | Spec 035 must persist the 032 trust fixtures with schema version, migration state, owner-safe mutation marker, and execution snapshot refs for dependency and entrypoint actions. | Spec 035-owned command, local API, or migration test fixture named in Spec 035 closure evidence. This PRD does not prescribe the command name. | `<evidence-dir>/prd005/external/spec035/trust-persistence/active-snapshot.json`, `stale-rejected.json`, `revoked-rejected.json`, `removed-rejected.json`, `mismatch-rejected.json`, `mutation-admission.json`, `owner-command.txt` | Active exact trust is persisted only as refs and digests. Stale, revoked, removed, and mismatched trust are not stored as allow provenance. | Storage path is asserted by PRD 005, raw secret or package credential persists, stale or revoked state appears as allow provenance, or owner command/API locator is missing. | Spec 035 fixture cleanup removes temp runtime root and preserves copied evidence. |
| Dependency preparation permission input | PRD 005 consumes 032 and 035 evidence; installer remains external | Use 032 active trust fixture plus 035 execution snapshot fixture. Dependency manifest includes one pinned package inside scope and one deterministic manifest-outside package for rejection. Fake provider dependency input is a redacted fixture at `<evidence-dir>/prd005/fake-provider/dependency-request.json` containing provider call id, action digest, requested package digest, and no raw prompt. | PRD 005 focused tests or future policy API that accepts fixture files. If 032 has not introduced a dependency preparation command/API, mark `BLOCKED_EXTERNAL_SURFACE` and record the missing external evidence locator. | `<evidence-dir>/prd005/dependency-preparation/permission-input.json`, `decision-allow-within-manifest.json`, `decision-deny-manifest-outside.json`, `decision-deny-runtime-installer.json`, `spawn-counter.json`, `fake-provider/dependency-request.json` | Active exact trust becomes bounded input, static policy and ceiling run, within-manifest candidate may proceed to PRD 002, manifest-outside and runtime installer escalation deny, spawn counter remains zero on denials. | Any installer spawn occurs before policy, manifest-outside package proceeds, runtime installer escalation proceeds, fake provider prose changes decision, or raw package credential persists. | Remove temp workspace and fake provider files after copying artifacts. |
| Verified entrypoint permission input | PRD 005 consumes 032 and 035 evidence; runner remains external | Use 032 active trust fixture plus 035 execution snapshot fixture. Entrypoint fixture has approved entrypoint digest and one altered command digest. Fake provider entrypoint request is at `<evidence-dir>/prd005/fake-provider/entrypoint-request.json` with action digest, entrypoint digest, capability digest, and no raw prompt. | PRD 005 focused tests or future policy API that accepts fixture files. If 032 has not introduced a verified entrypoint command/API, mark `BLOCKED_EXTERNAL_SURFACE` and record the missing external evidence locator. | `<evidence-dir>/prd005/verified-entrypoint/permission-input.json`, `decision-allow-exact-entrypoint.json`, `decision-deny-entrypoint-mismatch.json`, `decision-deny-capability-widening.json`, `spawn-counter.json`, `fake-provider/entrypoint-request.json` | Exact active trust may proceed to PRD 002 and PRD 003. Entrypoint digest mismatch and capability widening reject before runner spawn. | Runner spawn occurs before policy/proof, entrypoint mismatch proceeds, capability widening proceeds, fake provider prose changes decision, or raw args/env persist. | Remove temp workspace and fake provider files after copying artifacts. |
| App and process envelope handoff | PRD 002 and PRD 003, with Spec 032 for app/process lifecycle | Use a deterministic app/process fixture owned by Spec 032 with app id, manifest digest, process candidate id, workspace scope, and lifecycle status. Use PRD 002 envelope and PRD 003 proof fixtures for the same envelope id. | PRD 002 and PRD 003 focused tests or future local API named in their closure evidence. PRD 005 consumes only artifact refs. | `<evidence-dir>/prd005/process-handoff/process-envelope.json`, `containment-proof.json`, `app-process-fixture.json`, `admission-decision.json`, `receipt-or-blocked.json` | Envelope id, policy digest, trust input digest, app/process fixture, and containment proof match exactly. Static policy and ceiling precede spawn. | PRD 005 creates app lifecycle state, AppSupervisor ownership is absorbed, proof is missing, envelope mismatch proceeds, or process spawns before gate. | Remove temp app workspace and copy receipts before cleanup. |
| Diagnostics projection | Spec 031 renders; PRD 005 supplies refs | Use the artifacts above as projection input. If Spec 031 projection is unavailable, record `BLOCKED_EXTERNAL_SURFACE`. | Current `runtime diagnostics` may be used only for baseline diagnostics it actually supports. Future trust projection command/API must be named by Spec 031 closure evidence. | `<evidence-dir>/prd005/diagnostics/projection-input.json`, `diagnostics-output.txt`, `raw-value-audit.txt`, `blocked-external-surface.json` when applicable | Diagnostics show trust ref, lifecycle status, exact-match fields, policy snapshot ref, envelope id, rejection code, and redacted receipt refs with no raw values. | Diagnostics claims success from missing trust projection, omits lifecycle or precedence, or leaks raw body, raw dependency payload, raw env, raw secret, raw stdout, raw stderr, process handle, or absolute host path. | Delete temp diagnostics bundle after copying redacted artifacts. |

## Agent-Generated Artifact-Backed Audit

Before PRD 006 may count this PRD as closed, an agent must generate `<evidence-dir>/prd005/audit/read-trace.json` and `<evidence-dir>/prd005/audit/read-trace.md` from the artifacts above. A prose-only read trace is not sufficient.

The audit must include these machine fields:

```json
{
  "prd": "030-005",
  "result": "PASS_OR_FAIL",
  "blocked_external_surfaces": [],
  "artifact_refs": [],
  "active_trust_trace": {
    "trust_record_ref": "",
    "lifecycle_status_digest": "",
    "source_digest": "",
    "content_digest": "",
    "dependency_manifest_digest": "",
    "package_set_digest": "",
    "capability_digest": "",
    "entrypoint_digest": "",
    "policy_safety_digest": "",
    "process_envelope_id": "",
    "containment_proof_id": ""
  },
  "decision_order_observed": [
    "parse_trust_input",
    "external_lifecycle_status",
    "exact_digest_match",
    "process_envelope_normalization",
    "static_policy",
    "permission_ceiling",
    "approval_correlation_if_needed",
    "containment_proof",
    "external_adapter_handoff",
    "redacted_receipt"
  ],
  "negative_cases": {
    "stale": "PASS_OR_FAIL",
    "revoked": "PASS_OR_FAIL",
    "removed": "PASS_OR_FAIL",
    "digest_mismatch": "PASS_OR_FAIL",
    "manifest_outside": "PASS_OR_FAIL",
    "installer_escalation": "PASS_OR_FAIL",
    "entrypoint_mismatch": "PASS_OR_FAIL",
    "ownership_absorption": "PASS_OR_FAIL"
  },
  "raw_value_audit": "PASS_OR_FAIL"
}
```

Binary audit rules:

1. `PASS` requires no blocked external surfaces and all required artifacts present.
2. `FAIL` applies when any negative case proceeds, any raw value persists, any decision order step is missing, or PRD 005 owns an external domain.
3. `BLOCKED_EXTERNAL_SURFACE` must name the missing 032, 035, 031, PRD 002, or PRD 003 evidence locator. It cannot be counted as pass by PRD 006.
4. The audit must confirm active trust has source, content, dependency, package set, capability, lifecycle, policy snapshot, envelope, and proof refs.
5. The audit must confirm static policy and ceiling run after trust validation and before spawn.
6. The audit must confirm stale, revoked, removed, source mismatch, content mismatch, dependency mismatch, package mismatch, capability mismatch, entrypoint mismatch, snapshot mismatch, envelope mismatch, manifest-outside action, installer escalation, and ownership absorption reject.
7. The audit must confirm registry lifecycle, inspect, revoke, installer, package verification, runner, storage, diagnostics rendering, and release rendering remain external owner work.

## Adversarial Requirements

| Scenario | Required handling |
|---|---|
| `malformed_input` | Applies at the trust provenance boundary. Unknown schema, missing trust ref, unsupported action kind, raw secret, raw env, raw package credential, raw dependency payload, raw command payload, invalid digest, unknown lifecycle status, or missing required exact-match field rejects before policy or spawn. |
| `prompt_injection` | Skill name, Markdown body, requirements text, install metadata, prompt text, plugin output, MCP prompt, stdout, and stderr are untrusted data. They cannot authorize, raise mode, widen ceiling, select package, select runner, or change lifecycle status. |
| `stale_state` | Applies to current authoring and future implementation. Re-read specs and source before claiming behavior. Stale trust, stale snapshot, stale envelope, stale proof, stale package verification, or stale approval rejects before spawn. |
| `dirty_worktree` | Current authoring must create only this PRD and its evidence file. Future implementation must use temp workspaces and keep fixture mutations outside source files. |
| `misleading_success_output` | Installer or entrypoint stdout saying success is not proof. Typed lifecycle, digest, verification, receipt, and policy fields decide outcome. |
| `cancel_resume` | Cancellation after approval invalidates reusable trust permission proof unless fresh active lifecycle, exact digests, policy snapshot, envelope, and containment proof are rebuilt. |
| `repeated_interruptions` | Repeated cancel or interrupt preserves terminal denial and must not reopen approval, trust proof, installer spawn, or runner spawn. |
| `flaky_tests` | Use deterministic lifecycle fixtures, fake clocks, controlled owner-state refs, and spawn counters. Sleeps are not proof. |
| `hung_commands` | Future CLI probes must be bounded. A hung installer or entrypoint is a timeout failure artifact, not pending success. |
| `cleanup` | None for this documentation task. Future process cleanup is PRD 002 receipt and external lifecycle scope. |

## External Completion Gates

PRD 006 may consume this PRD only after the external gates below are evidenced.

1. Spec 032 has typed trust lifecycle evidence for proposal, active, stale, revoked, removed, inspect, revoke, dependency preparation eligibility, runtime prerequisite separation, and verified entrypoint lifecycle facts.
2. Spec 035 has schema-versioned trust persistence, migration, owner-safe mutation admission, execution snapshot refs, and tests proving stale, revoked, removed, and mismatched trust are not stored as allow provenance.
3. PRD 000 has typed policy and safety snapshot refs consumed by trust permission input and process envelopes.
4. PRD 001 has typed secret refs and redaction evidence for any trust-bound secrets.
5. PRD 002 has process envelope adapters for dependency preparation and verified entrypoint candidates.
6. PRD 003 has containment and ceiling proof for dependency preparation and verified entrypoint boundaries, including blocked-on-PRD005 evidence before this PRD closes and exact trust evidence after it closes.
7. Spec 031 has projection and release evidence rendering for trust decision refs if PRD 006 requires user-facing parity.

## Closure Artifacts

Implementation evidence must include all artifacts below.

1. Baseline characterization output for current `shacs-skills` registry, descriptor, body hash, requirements, install metadata, plugin-provided roots, malformed and conflicted states, and read-only context injection.
2. Failing-first history for exact-match reuse, lifecycle invalidation, digest invalidation, approval invalidation, static policy precedence, ceiling precedence, prompt injection, misleading output, malformed input, cancellation, and repeated interruptions.
3. Typed Rust contracts for `SkillTrustPermissionInput`, identity match structs, lifecycle status consumer, canonical digest, decision outcomes, and redacted diagnostics refs.
4. Process envelope evidence proving dependency preparation and verified entrypoint candidates enter PRD 002 before installer or runner spawn.
5. Containment proof evidence proving PRD 003 compares equal-or-narrower containment, workspace, and ceiling for both boundaries.
6. Focused Cargo command output and workspace Cargo gate output with `--manifest-path crates/Cargo.toml`.
7. Real-surface QA transcripts and redacted artifacts for active exact-match reuse, stale rejection, revoked rejection, removed rejection, digest mismatch rejection, manifest-outside rejection, installer escalation rejection, and verified entrypoint rejection.
8. Structured audit proving raw skill body, raw dependency manifest payload, raw package credential, raw env, raw secret, raw stdout, raw stderr, process handles, absolute host paths, and raw approval prose do not persist in trust input, envelope, receipt, replay input, diagnostics, or release evidence.
9. Owner handoff evidence showing 032 owns lifecycle and registry transitions, 035 owns persistence and execution snapshot refs, PRD 002 owns process envelope and receipt, PRD 003 owns containment proof, and external adapters own installer, package verification, and runner internals.

## Exit Criteria

1. `SkillTrustPermissionInput` exists and rejects malformed, missing, stale, revoked, removed, pending, mismatched, and prompt-injected trust provenance before spawn.
2. Active trust is accepted only as bounded input when skill, source, content, dependency, package set, capability, entrypoint, lifecycle, policy snapshot, envelope, secret refs, and proof refs match exactly.
3. Static policy, permission ceiling, approval correlation, and containment proof keep precedence over active trust.
4. Approval reuse is invalidated by any lifecycle, digest, snapshot, envelope, proof, cancellation, or repeated-interruption change.
5. Dependency preparation may run only for approved pinned manifest scope after policy and proof admit. Runtime installer escalation, global install, unapproved package, unexpected native build, and unexpected lifecycle script reject.
6. Verified entrypoint may run only when active exact-match trust, dependency verification evidence, entrypoint digest, policy snapshot, envelope, and proof all match.
7. Skill name, body, body hash, requirements text, install metadata text, prompt text, plugin output, stdout, and stderr never authorize.
8. Trust registry transitions, inspect, revoke, installer behavior, package verification, entrypoint runner lifecycle, persistence, projection, and release rendering remain external owner work.
