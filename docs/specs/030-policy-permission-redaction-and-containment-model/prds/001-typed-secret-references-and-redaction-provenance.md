# PRD 001. typed secret references and redaction provenance

Status: Planned

## Goal

Spec 030 needs one typed language for secret identity, just-in-time resolution, and redacted evidence. This PRD defines that language so policy snapshots, approval requests, process envelopes, diagnostics, and release evidence can carry secret references without carrying raw secret values.

The contract is intentionally narrow. A secret reference identifies where a runtime boundary may resolve a secret. It is not the secret. Redaction evidence explains what was projected and why. It is not proof that exfiltration is impossible.

## SPEC Inputs

1. Spec 030 owns typed secret reference semantics, current best-effort redaction limits, diagnostics projection, and the final closure target for typed secret refs.
2. Spec 010 supplies the local safety baseline: env placeholders, auth handling, inspect redaction, diagnostics redaction, and the explicit future gap for raw secret value and secret reference type separation.
3. Spec 032 owns app manifest secret declarations, app binding lifecycle, missing-secret blockers, app process receipts, and skill trust lifecycle states that may produce secret needs.
4. Spec 035 owns config/profile consumption, schema-versioned persistence, migration, execution snapshot persistence, and trust persistence that consume typed secret refs.
5. `crates/shacs-redaction/src/lib.rs` is the current boundary fact: `redact_value`, `redact_string`, and `is_sensitive_key` provide best-effort projection for sensitive keys, auth headers, token prefixes, credential paths, inline env assignments, and private key blocks.

## Scope

1. Define legal and illegal serialized secret reference shapes.
2. Define supported source kinds for Spec 030 closure.
3. Define the resolution boundary and the no-persistence rule for raw values.
4. Define safe locator and safe summary rules for snapshots, receipts, approval text, diagnostics, and release evidence.
5. Define redaction provenance fields that prove a projection path ran and name its limits.
6. Define behavior-level tests and real-surface QA that future implementation must pass before Spec 030 can consume this PRD as closure evidence.

## Non-Scope

1. Hosted vaults, cloud secret managers, SaaS key stores, admin consoles, organization RBAC, and fleet rollout.
2. Raw secret storage, raw secret migration, raw secret replay, raw env dumps, or raw process receipt persistence.
3. Spec 032 app binding lifecycle, AppSupervisor start/stop/recover, trust registry state transitions, inspect/revoke lifecycle, or app receipt domain vocabulary.
4. Spec 035 config schema migration, config/profile storage, physical execution snapshot storage, runtime layout, or trust persistence.
5. A claim that redaction prevents all secret exfiltration. This PRD requires typed boundaries and best-effort projections, not complete prevention.

## Dependency Cut

1. PRD 000 supplies the immutable policy and safety correlation snapshot ref that can point at secret refs and redaction evidence.
2. PRD 002 consumes `SecretRef` and `RedactionEvidenceRef` in process envelopes. It must not add raw values to args, env, receipts, or replay inputs.
3. PRD 005 consumes secret refs only through active trust provenance and process envelopes. It must not treat skill content as a secret source.
4. PRD 006 verifies this PRD only after 032 proves app and trust lifecycle producers, and 035 proves config/profile persistence and execution snapshot consumers.

## Typed Contract

### Legal Serialized Shape

Secret references serialize as typed identity plus safe locator metadata:

```json
{
  "kind": "secret_ref",
  "schema_version": 1,
  "ref_id": "sec_01HLOCALREF",
  "source_kind": "env",
  "locator": {
    "kind": "env_var",
    "name": "OPENAI_API_KEY"
  },
  "owner": "spec035-config-profile",
  "scope": "provider-auth",
  "created_by": "config-profile",
  "created_at_ms": 0,
  "locator_digest": "sha256:...",
  "staleness_token": "sha256:...",
  "safe_summary": {
    "label": "env:OPENAI_API_KEY",
    "required": true
  }
}
```

`ref_id`, `source_kind`, `locator`, `owner`, `scope`, `locator_digest`, `staleness_token`, and `safe_summary` are required. `locator` is source-specific identity, never value material. `safe_summary` is the only user-facing form allowed in approval text, diagnostics, receipts, and release evidence.

### Illegal Serialized Shapes

These forms must fail during parse before any snapshot or receipt is written:

```json
{ "kind": "secret_ref", "value": "sk-live-secret" }
```

```json
{ "kind": "secret_ref", "locator": { "env_value": "hunter2" } }
```

```json
{ "kind": "secret_ref", "source_kind": "hosted_vault", "locator": { "url": "https://vault.example/key" } }
```

```json
{ "kind": "redacted_value", "raw": "Bearer ghp_secret" }
```

Illegal fields include `value`, `raw`, `secret`, `token`, `password`, `env_value`, `header_value`, `private_key`, full environment maps, full credential files, process-ready env maps, and any nested field whose value is the secret itself. Unknown `source_kind`, missing `staleness_token`, missing `locator_digest`, and mismatched `schema_version` also reject.

## Supported Source Kinds

| Source kind | Locator identity | Resolution boundary | Owner | Notes |
|---|---|---|---|---|
| `env` | Environment variable name plus profile scope | Just before provider, tool, process, or app adapter needs the value | 035 | Config may persist the ref, not the env value. Missing env is a blocker or ask/deny input. |
| `auth_store` | Provider/account auth entry id plus credential slot | Auth adapter call boundary | 035 | Auth store may own encrypted or local auth data by its own rules. 030 consumes only the ref and redacted evidence. |
| `local_secret_store` | User-local store entry id | Boundary adapter chosen by 035 | 035 | Optional local store, not a hosted vault requirement. If absent, refs using it are unsupported. |
| `app_binding` | App id, manifest digest, declared secret name | App process binding handoff | 032 produces, 035 persists, 030 consumes | App manifest declaration is not a grant. Resolution waits for 032 binding and 030 policy checks. |
| `skill_trust_binding` | Trust record ref, dependency or entrypoint secret slot | Dependency preparation or verified entrypoint envelope | 032 produces lifecycle, 035 persists, 030 consumes | Only active, exact-match trust provenance may reach PRD 005. |

Any source kind outside this table is unsupported until a future PRD adds an owner, parse rule, stale rule, tests, and diagnostics text. Unsupported refs fail closed as parse or resolution errors.

## Resolution Boundary

1. Config, app manifests, skill trust records, policy snapshots, process envelopes, approval requests, receipts, diagnostics, replay inputs, and release evidence may carry `SecretRef` only.
2. Raw secret values may exist only inside a short-lived resolver return value at the adapter boundary that needs the value.
3. The resolver must return either `ResolvedSecret` for immediate use or a typed failure. It must not expose the raw value to snapshot, receipt, replay, diagnostics, audit, approval, or release writers.
4. Process envelopes must store env and argument references plus digests of redacted projections, not a process-ready raw env map.
5. Replay reads refs and evidence only. Replay must not resolve a secret or dispatch a live process.

## Safe Locator and Summary Rules

1. `safe_summary.label` may show user-authored names such as `env:OPENAI_API_KEY` or `app:calendar.gmail_token` when the label is a locator name, not a value.
2. Absolute host paths, full credential file paths, auth blobs, provider tokens, cookie strings, private key blocks, and full environment maps are never safe summaries.
3. Path-like locators must project as an opaque locator digest or a redacted basename chosen by the source owner.
4. Approval text must show source kind, required or optional status, owner, stale status, and intended consumer. It must not show the resolved value.
5. Diagnostics must answer: which ref was needed, which source kind owned it, whether it resolved, which redaction profile ran, and which policy or approval evidence consumed it.

## Redaction Provenance and Evidence

`RedactionEvidence` records that a projection was produced before persistence or display:

```json
{
  "kind": "redaction_evidence",
  "schema_version": 1,
  "evidence_id": "red_01HLOCALREF",
  "input_ref": "sec_01HLOCALREF",
  "projection_surface": "approval_request",
  "redaction_profile": "shacs-redaction-v1",
  "classified_kinds": ["sensitive_key", "token_prefix"],
  "safe_summary_digest": "sha256:...",
  "raw_value_persisted": false,
  "best_effort": true,
  "limits": ["not_exfiltration_prevention", "not_raw_payload_integrity_proof"]
}
```

Evidence must be attached to approval requests, diagnostics, receipts, and release artifacts that mention a secret ref. `raw_value_persisted` must always be false. If redaction fails or cannot classify a surface, the future implementation must block persistence and leave failure evidence without the raw value.

## Source-Kind, Resolution, and Owner Matrix

| Consumer | May carry `SecretRef` | May resolve raw value | Must record redaction evidence | External owner |
|---|---:|---:|---:|---|
| Config/profile persistence | yes | no | yes for projections | 035 |
| App manifest declaration | yes | no | yes for receipts | 032 |
| Policy/safety snapshot | yes | no | yes | PRD 000 |
| Approval request | yes | no | yes | 030 and 022 approval correlation |
| Process envelope | yes | no | yes | PRD 002 |
| Adapter boundary | yes | yes, just in time | no raw persistence allowed | Source owner plus adapter owner |
| Process receipt | yes | no | yes | PRD 002 and 032 for app receipts |
| Diagnostics bundle | yes | no | yes | 031 projection, 030 semantics |
| Replay input | yes | no | yes | 030 replay contract and 031 projection |

## Normal Sequence

1. 035 reads config or profile and parses a legal `SecretRef` for a provider key.
2. 032 may produce an app binding declaration that points at the same kind of typed ref, but it does not resolve the value.
3. PRD 000 snapshot creation includes the `SecretRef` identity and safe summary digest.
4. PRD 002 process envelope carries the `SecretRef`, intended consumer, and redacted env or argument digest.
5. Runtime policy evaluates static rules, ceilings, containment, and approval needs before any resolution.
6. Approval text shows safe locator summary and redaction provenance.
7. The adapter resolves the value just in time, uses it, then drops it.
8. Receipt and diagnostics store `SecretRef`, decision refs, redaction evidence, and safe summaries only.

## Failure Sequences

1. Raw value serialization appears in config, snapshot, envelope, approval, receipt, diagnostics, or replay input. Parser rejects it and records a malformed secret ref diagnostic without the value.
2. `source_kind` is unsupported. The action is not allowed, and diagnostics show unsupported source kind, owner, and consumer.
3. `staleness_token` mismatches current owner state. The approval or process envelope is stale and cannot be reused.
4. A ref points to a missing env var, missing auth entry, missing app binding, revoked trust record, or removed source. The boundary returns missing or stale ref evidence and blocks or asks through policy.
5. Redaction projection cannot produce evidence before persistence. Persistence is blocked, and release evidence records redaction failure without raw payload.
6. Replay attempts to resolve a secret or live-dispatch a process. Replay fails as invalid and records the attempted consumer only.

## Diagnostics and Approval Evidence

Approval evidence must include request id, action digest, snapshot digest, secret ref id, source kind, safe summary digest, redaction evidence id, requested consumer, expiry, and consumed state. It must not include the raw value.

Diagnostics must include counts for resolved, missing, stale, unsupported, and malformed refs. Each item must link to redaction evidence and owner source kind. Diagnostics may show `env:NAME` when the name is the configured locator. It must never print value material, full env, private key blocks, auth headers, cookie values, or host credential paths.

## Exact TDD Order

Future implementation must follow this order. Writing production code before the listed red tests is not allowed.

1. Baseline characterization: run current tests for `shacs-redaction`, permission action redaction, config env placeholder handling, and diagnostics redaction. Add characterization tests only if a current boundary lacks a named test.
2. Red proof for raw value serialization rejection: add parser tests where `value`, `raw`, `token`, nested `env_value`, and `redacted_value.raw` fail before snapshot or receipt creation.
3. Red proof for unsupported and stale refs: add tests for unknown `source_kind`, missing `staleness_token`, owner-state digest mismatch, missing env, missing auth entry, revoked trust, and removed app binding.
4. Red proof for redaction projection: add tests proving approval, envelope, diagnostics, receipt, and replay projections contain `SecretRef` and `RedactionEvidence`, not raw values.
5. Red proof for diagnostics: add tests for resolved, missing, stale, unsupported, and malformed counters plus safe summaries.
6. Red proof for approval evidence: add tests proving approval correlation includes action digest, snapshot digest, secret ref id, redaction evidence id, expiry, and consumed state, and rejects reuse after stale ref changes.
7. Minimal implementation: add the smallest typed model, serde parser, resolver trait boundary, and projection hooks that make the red tests pass.
8. Refactor only after green: remove duplicate projection code and keep ownership cuts with 032 and 035 intact.
9. Final regression: run focused Cargo gates, diagnostics real-surface QA, and full closure checklist before PRD 006 can consume the evidence.

## Focused Cargo Targets and Commands

Future implementation must run these from the repository root:

```sh
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-redaction
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-config secret_ref
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core --test permission_action secret_ref
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core --test permission_policy secret_ref
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-cli runtime_diagnostics_secret_ref
```

Before closure, run the normal Rust gates from `AGENTS.md` if any Rust changed:

```sh
cargo fmt --manifest-path crates/Cargo.toml --all -- --check
cargo clippy --manifest-path crates/Cargo.toml --locked --workspace --all-targets -- -D warnings
cargo test --manifest-path crates/Cargo.toml --locked --workspace
```

Do not add dependency-lock flags to format commands. Retain `--locked` on clippy, test, and build commands that support dependency locking.

## Agent-Executed Real-Surface Diagnostics and Approval QA

The implementation worker must create an isolated temp workspace and drive the real CLI or local API surface. No user click, shell paste confirmation, or human inspection may be part of the pass condition.

### Fixture Setup

Owner: PRD 001 defines the typed fixture shape, Spec 035 owns config/profile persistence, Spec 032 owns app binding lifecycle, PRD 002 owns process-envelope consumption, and Spec 022 approval correlation remains the approval decision owner.

The QA fixture must be generated under one temporary root:

```text
<tmp-root>/workspace/
<tmp-root>/config/secret-ref-fixture.json
<tmp-root>/artifacts/runtime-diagnostics.json
<tmp-root>/artifacts/runtime-diagnostics.zip
<tmp-root>/artifacts/approval-request.json
<tmp-root>/artifacts/approval-decision.json
<tmp-root>/artifacts/raw-value-audit.json
<tmp-root>/artifacts/cleanup-receipt.json
```

The fixture owner fields are exact:

```json
{
  "provider_secret_owner": "spec035-config-profile",
  "app_binding_owner": "spec032-app-binding-lifecycle",
  "approval_owner": "spec022-approval-correlation",
  "process_owner": "spec030-prd002-process-envelope",
  "secret_refs": [
    {
      "ref_id": "sec_prd001_env_happy",
      "source_kind": "env",
      "locator": { "kind": "env_var", "name": "SHACS_PRD001_HAPPY_SECRET" },
      "owner": "spec035-config-profile",
      "scope": "provider-auth"
    },
    {
      "ref_id": "sec_prd001_app_missing",
      "source_kind": "app_binding",
      "locator": { "app_id": "fixture-app", "manifest_digest": "sha256:fixture", "name": "calendar_token" },
      "owner": "spec032-app-binding-lifecycle",
      "scope": "app-process-env"
    }
  ]
}
```

Set only this raw fixture value, and never write it into any fixture file:

```sh
export SHACS_PRD001_HAPPY_SECRET='sk-prd001-raw-fixture-value'
```

### Diagnostics Invocation

The diagnostics path must run as an executable command:

```sh
cargo run --manifest-path crates/Cargo.toml --locked -p shacs-cli -- runtime diagnostics --workspace <tmp-root>/workspace --config <tmp-root>/config/secret-ref-fixture.json --bundle <tmp-root>/artifacts/runtime-diagnostics.zip > <tmp-root>/artifacts/runtime-diagnostics.json
```

PASS requires `runtime-diagnostics.json` and the bundle projection to contain `sec_prd001_env_happy`, `sec_prd001_app_missing`, `source_kind`, `owner`, `safe_summary`, and `redaction_evidence_id`. FAIL if the raw fixture value, full env map, private key block, auth header, cookie value, or credential path appears.

### Approval-Path Invocation

PRD 001 does not own an approval runner. PRD 002 must expose a process-envelope fixture that can request approval without live side effects, and Spec 022 must expose or retain an approval-correlation test/API surface that consumes the request. Until those surfaces exist with the artifact locators below, PRD 006 must block Spec 030 closure.

The required CLI invocation is:

```sh
cargo run --manifest-path crates/Cargo.toml --locked -p shacs-cli -- permission qa secret-ref-approval --workspace <tmp-root>/workspace --config <tmp-root>/config/secret-ref-fixture.json --request-out <tmp-root>/artifacts/approval-request.json --decision-out <tmp-root>/artifacts/approval-decision.json
```

If approval QA is exposed through local API instead of CLI, the equivalent required invocation is:

```sh
curl -sS -X POST http://127.0.0.1:<port>/v1/permission/qa/secret-ref-approval -H 'content-type: application/json' --data-binary @<tmp-root>/config/secret-ref-fixture.json > <tmp-root>/artifacts/approval-request.json
```

PASS requires the approval artifact to include `approval_request_id`, `action_digest`, `snapshot_digest`, `secret_ref_ids`, `redaction_evidence_ids`, `expires_at`, and `consumed=false`. FAIL if it contains the raw fixture value or if the action is marked approved after `staleness_token` changes.

### Structured Raw-Value Audit

The implementation worker must write this machine-readable audit result:

```json
{
  "artifact": "<tmp-root>/artifacts/raw-value-audit.json",
  "raw_fixture_value": "sha256:<digest-of-raw-value>",
  "checked_artifacts": [
    "runtime-diagnostics.json",
    "runtime-diagnostics.zip",
    "approval-request.json",
    "approval-decision.json"
  ],
  "forbidden_patterns": [
    "sk-prd001-raw-fixture-value",
    "Authorization: Bearer",
    "-----BEGIN PRIVATE KEY-----",
    "SHACS_PRD001_HAPPY_SECRET=",
    "Cookie:"
  ],
  "result": "PASS"
}
```

The only accepted values for `result` are `PASS` and `FAIL`. PRD 006 must reject missing audit files, non-structured prose audit, unchecked artifacts, missing forbidden patterns, or `FAIL`.

## PRD 006 Cleanup Registry and Receipt Linkage

PRD 006 must maintain a cleanup registry entry for every temporary artifact created by this PRD's QA. The registry must be linked from the closure evidence for PRD 001 and must name cleanup ownership before final closure.

Required registry shape:

```json
{
  "registry_id": "cleanup_prd001_secret_ref_qa",
  "owner": "spec030-prd006-closure",
  "artifacts": [
    { "kind": "tmp_workspace", "path": "<tmp-root>/workspace" },
    { "kind": "tmp_config", "path": "<tmp-root>/config/secret-ref-fixture.json" },
    { "kind": "tmp_env", "name": "SHACS_PRD001_HAPPY_SECRET", "value_persisted": false },
    { "kind": "diagnostics_bundle", "path": "<tmp-root>/artifacts/runtime-diagnostics.zip" },
    { "kind": "approval_artifact", "path": "<tmp-root>/artifacts/approval-request.json" },
    { "kind": "approval_artifact", "path": "<tmp-root>/artifacts/approval-decision.json" },
    { "kind": "audit_artifact", "path": "<tmp-root>/artifacts/raw-value-audit.json" }
  ],
  "cleanup_receipt": "<tmp-root>/artifacts/cleanup-receipt.json"
}
```

Required cleanup receipt shape:

```json
{
  "registry_id": "cleanup_prd001_secret_ref_qa",
  "owner": "spec030-prd006-closure",
  "removed_artifacts": ["<tmp-root>/workspace", "<tmp-root>/config", "<tmp-root>/artifacts"],
  "env_unset": ["SHACS_PRD001_HAPPY_SECRET"],
  "raw_value_persisted": false,
  "result": "PASS"
}
```

PRD 006 must fail closure if temp config, env, bundle, approval artifact, audit artifact, or workspace cleanup is missing a registry entry or cleanup receipt.

## Adversarial Matrix

| Class | Applicability | Required probe | PASS | FAIL |
|---|---|---|---|---|
| `malformed_refs` | Applicable | Feed illegal shapes from `Illegal Serialized Shapes` into the parser before snapshot creation | Parser rejects and records malformed diagnostic without raw value | Snapshot, receipt, or approval artifact is written |
| `stale_refs` | Applicable | Change `staleness_token` or owner digest after approval request creation | Approval and envelope reuse reject as stale | Existing approval remains executable |
| `prompt_injection` | Applicable at app/skill text boundaries | Include prompt text that asks the runtime to print or persist the secret | Text is treated as untrusted content and cannot alter `SecretRef` policy | Prompt text becomes permission, resolver, or redaction instruction |
| `raw_leaks` | Applicable | Run structured raw-value audit over diagnostics, bundles, approvals, receipts, replay inputs, and release evidence | Audit result is `PASS` and all forbidden patterns are absent | Any raw fixture value or forbidden pattern appears |
| `dirty_worktree` | Applicable | Run `git status --short` before and after implementation | Only expected PRD, code, test, and evidence files are touched | External owner specs, Cargo lockfile, or unrelated docs change without scope |
| `hung_diagnostics` | Applicable | Run diagnostics and approval QA under bounded timeout chosen by PRD 006 | Timeout produces failure artifact without raw value | Command hangs or partial raw artifact remains |
| `flaky_tests` | Applicable | Run focused tests twice or through the repository flake policy selected by PRD 006 | Same pass/fail result and same artifact schema | Intermittent pass hides redaction, stale, or approval failures |
| `misleading_success_output` | Applicable | Treat exit code zero as insufficient and inspect structured artifacts | Artifacts contain required refs, evidence ids, and PASS audit | CLI says success while required fields or raw audit are missing |
| `cancel_resume` | Applicable if QA or approval can be interrupted | Interrupt after approval request and before decision consumption, then resume | Resumed path revalidates `staleness_token`, expiry, and raw-value absence | Resumed path consumes stale approval or skips audit |
| `repeated_interruption` | Applicable if PRD 006 repeats cancel/resume | Repeat interruption around diagnostics bundle and approval artifact writes | Idempotent receipt and cleanup registry remain valid | Duplicate artifacts, leaked env, or consumed approval state mismatch |
| `unsupported_external_surface` | Applicable until PRD 002/022/031 expose QA surfaces | Check artifact locators and owner evidence before closure | PRD 006 blocks closure until evidence exists | Closure accepts future/external surface by prose claim |

All probes must be agent-executed and recorded as structured evidence. Grep-only success is not enough when a structured artifact exists.

## Closure Evidence

PRD 006 may count this PRD as closed only when the implementation change records:

1. Test names for raw value serialization rejection, unsupported refs, stale refs, redaction projection, diagnostics counters, and approval evidence.
2. Focused Cargo command output for the commands above.
3. Real CLI diagnostics QA output or saved artifact path showing config/app declaration to typed ref to just-in-time resolution to redacted evidence.
4. A grep or structured audit proving snapshots, receipts, diagnostics, replay inputs, and approval evidence do not contain the fixture raw values.
5. Owner handoff evidence showing 032 still owns app binding lifecycle and 035 still owns config/profile persistence and execution snapshot persistence.
6. A non-guarantee review confirming no text or diagnostic claims complete exfiltration prevention, kernel isolation, mandatory hosted vaults, or raw-payload integrity proof.
7. Cleanup registry and cleanup receipt artifacts linked from PRD 006 closure evidence.
8. The adversarial matrix above with PASS or blocked-owner evidence for every applicable class.

## Exit Criteria

1. Legal and illegal serialized shapes are enforced by tests.
2. All supported source kinds have owner, locator, resolution, stale, and diagnostics behavior.
3. Raw secret values never enter snapshots, receipts, approval evidence, diagnostics, replay inputs, or release evidence.
4. Secret resolution happens only just in time at the adapter boundary.
5. Redaction evidence is present for every persisted or displayed secret-ref projection.
6. Best-effort wording remains explicit, and complete prevention is not claimed.
7. 032 and 035 ownership stays external and evidenced.
