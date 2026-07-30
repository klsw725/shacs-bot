# PRD 006. sequential integration and Spec 030 closure

Status: Planned

## Goal

This PRD is the sole sequential integration and final closure gate for Spec 030. It orders PRDs 000 through 005, consumes external evidence from Specs 031, 032, and 035, and defines the exact evidence required before Spec 030 can move from `Status: Open` to `Complete (Scoped)`.

This PRD does not define a new type, domain model, permission rule, lifecycle state, storage path, projection schema, process manager, secret source, or trust registry. It accepts only evidence produced by the owner PRDs and external owner specs listed below.

## Scope

1. Define the acyclic dependency DAG for Spec 030 implementation.
2. Define implementation waves with entry and exit gates.
3. Map every stronger Spec 030 closure target to exactly one Spec 030 PRD owner and at least one external owner when external evidence is required.
4. Define external closure evidence locators for Specs 031, 032, and 035.
5. Define focused and workspace Cargo gates using `crates/Cargo.toml`.
6. Define agent executed CLI, API, process, and diagnostics QA with literal commands and required artifact locators.
7. Define failure injection, adversarial, security, and non-guarantee reviews.
8. Define documentation updates and the single final condition for changing Spec 030 and `docs/specs/README.md`.

## Non Scope

1. No Rust implementation is owned by this PRD.
2. No new domain contract may be introduced here.
3. No partial closure is allowed.
4. No user manual acceptance is allowed. Every gate must have an agent executed command transcript, structured artifact, or external evidence locator.
5. No grep-only or prose-only proof is allowed. Search may support an audit, but it cannot replace `Read`, tests, command output, real-surface QA, and artifact inspection.
6. This PRD does not edit Specs 031, 032, or 035.
7. This authoring task must keep this PRD `Status: Planned` and the parent Spec 030 `Status: Open`.

## SPEC Inputs

1. Spec 030 section `소유하는 open scope` defines current open scope and stronger closure targets.
2. Spec 030 section `Implementation PRDs` defines implementation PRDs, owner maps, internal gates, external gates, and the baseline versus final closure split.
3. Spec 030 sections `Invariants`, `Must Have`, `Must Not Have`, `Acceptance Criteria`, `Source Handoff Table`, and `Closure Evidence` define current baseline invariants, acceptance criteria, non-guarantees, and open closure evidence.
4. Spec 029 PRD 008 supplies the sequential implementation pattern: waves are dependency contracts, not suggestions.
5. Spec 031 Closure Evidence lists projection, diagnostics parity, API/channel parity, backpressure, release runner, and documentation evidence required before projection evidence can be counted.
6. Spec 032 closure evidence lists AppSupervisor, app lifecycle, permission/secret binding, extension provenance, skill trust lifecycle, dependency preparation, diagnostics, release, and documentation evidence required before lifecycle evidence can be counted.
7. Spec 035 Closure Evidence lists config migration, JSON compatibility, profile resolution, typed secret ref consumption, runtime layout, execution snapshots, immutability, budget, live context config wiring, diagnostics/replay, documentation, and skill trust persistence evidence required before persistence evidence can be counted.
8. `AGENTS.md` requires Rust verification through `cargo fmt`, `cargo clippy`, and `cargo test` using `--manifest-path crates/Cargo.toml`.

## Dependency DAG

The graph below is the complete Spec 030 closure dependency graph. It must stay acyclic.

```text
baseline
  -> PRD000_foundation_exit
  -> PRD001_secret_refs

PRD000_foundation_exit
  -> PRD002_process_envelope
  -> PRD003_containment_proof
  -> PRD004_classifier_accounting
  -> PRD005_skill_trust_permission

PRD001_secret_refs
  -> PRD002_process_envelope
  -> PRD005_skill_trust_permission
  -> PRD006_final_closure

PRD002_process_envelope
  -> PRD003_containment_proof
  -> PRD005_skill_trust_permission
  -> PRD000_downstream_consumer_audit
  -> PRD006_final_closure

PRD000_downstream_consumer_audit
  -> PRD006_final_closure

PRD003_containment_proof
  -> PRD006_final_closure

PRD004_classifier_accounting
  -> PRD006_final_closure

PRD005_skill_trust_permission
  -> PRD006_final_closure

Spec031_projection_release_evidence
  -> PRD006_final_closure

Spec032_app_and_skill_lifecycle
  -> PRD005_skill_trust_permission
  -> PRD006_final_closure

Spec032_app_and_skill_lifecycle
  -> PRD006_final_closure

Spec035_config_snapshot_trust_persistence
  -> PRD000_foundation_exit
  -> PRD006_final_closure

Spec035_config_snapshot_trust_persistence
  -> PRD001_secret_refs
  -> PRD006_final_closure

Spec035_config_snapshot_trust_persistence
  -> PRD005_skill_trust_permission
  -> PRD006_final_closure
```

Forbidden edges:

1. PRD 006 must not point back to PRD 000 through PRD 005 as an implementation input.
2. Spec 031 must not define Spec 030 domain evidence. It only renders and packages evidence.
3. Spec 032 must not define Spec 030 permission meaning. It produces app lifecycle and trust lifecycle facts.
4. Spec 035 must not define Spec 030 policy meaning. It stores config, snapshot, and trust persistence refs.
5. PRD 000 through PRD 005 must not depend on Spec 030 being `Complete (Scoped)`.

PRD 000 has two distinct gates to avoid a semantic cycle:

1. `PRD000_foundation_exit` is the pre-PRD002 implementation gate. It proves the typed ref, canonical digest, stale rejection, mismatch rejection, and redacted projection input exist. It does not require all downstream consumers to be wired yet.
2. `PRD000_downstream_consumer_audit` is the final consumer audit after PRD 002 has routed process envelopes through the shared ref. It proves permissioned action, approval, audit, replay, diagnostics, and process receipt consumers use the same immutable ref and digest. It cannot block PRD 002 entry and cannot redefine PRD 000 fields.

## Implementation Waves

### Wave 0. Baseline Characterization

Entry gate:

1. Read Spec 030 parent, PRDs 000 through 005, Spec 029 PRD 008, and closure sections from Specs 031, 032, and 035.
2. Confirm Spec 030 is still `Status: Open`.
3. Confirm current distributed baseline remains usable and is not being upgraded by documentation alone.

Exit gate:

1. Baseline tests named by PRDs 000 through 005 are listed in an evidence ledger.
2. Current non-guarantees are copied into the security and non-guarantee review checklist.
3. No implementation worker may proceed if any target lacks an owner.

### Wave 1. Correlation and Secret Foundations

Owners: PRD 000 and PRD 001.

Entry gate:

1. Wave 0 is evidenced.
2. Spec 035 has a current evidence locator for config, context, provider execution snapshot refs, or an explicit blocked state for missing persistence evidence.
3. External blocked state is treated as a closure blocker, not a pass.

Exit gate:

1. PRD 000 produces foundation evidence for typed policy and safety snapshot refs, canonical digest, stale rejection, mismatch rejection, and redacted diagnostics projection input. This is `PRD000_foundation_exit`, not the final downstream consumer audit.
2. PRD 001 produces typed secret ref parse rules, raw value rejection, supported source kind behavior, redaction evidence, and just-in-time resolution boundary evidence.
3. Focused Cargo gates for PRDs 000 and 001 pass.
4. Real-surface diagnostics QA produces artifacts for a permissioned action and a secret ref projection.

### Wave 2. Process Admission Foundation

Owner: PRD 002.

Entry gate:

1. PRD 000 closure evidence exists.
2. PRD 001 closure evidence exists.
3. App, dependency, or entrypoint adapters that need Spec 032 or Spec 035 evidence are marked blocked until the external evidence exists.

Exit gate:

1. Every supported process family has a typed process envelope before spawn.
2. Gate order is normalize, static policy, approval and ceiling, spawn, redacted receipt.
3. Replay proves zero live dispatch.
4. Timeout, cancellation, and repeated interruption outcomes produce receipts.
5. `PRD000_downstream_consumer_audit` records artifact-backed `Read` audits proving permissioned action, approval, audit, replay, diagnostics, and process receipt consumers use the same immutable ref and digest after PRD 002 wiring exists.
6. Focused Cargo gates and real process QA artifacts exist for current exec, plugin management, MCP configuration, app registry, and skill registry surfaces. App process start, dependency preparation, and verified entrypoint execution are blocked-on-owner gates until Specs 032, 035, and the owning PRD-specific implementations add those real surfaces.

### Wave 3. Containment and Classifier Evidence

Owners: PRD 003 and PRD 004.

Entry gate:

1. PRD 000 closure evidence exists.
2. PRD 002 closure evidence exists before PRD 003 starts.
3. PRD 004 may start after PRD 000 because it consumes policy snapshot refs but not the process envelope.

Exit gate:

1. PRD 003 proves equal-or-narrower containment, workspace, and ceiling for subagent, MCP, app, plugin, dependency preparation, verified entrypoint, and deferred bridge boundaries.
2. PRD 004 proves classifier evidence cannot override static deny or ceiling, and missing accounting is unavailable, failed, skipped, estimated, or not applicable, never fabricated zero.
3. Spec 031 projection evidence locators exist for containment proof input and classifier accounting rendering. Missing projection evidence blocks final closure.
4. Focused Cargo gates and real-surface diagnostics artifacts exist for both PRDs.

### Wave 4. Skill Trust Permission Consumption

Owner: PRD 005.

Entry gate:

1. PRD 000, PRD 001, PRD 002, and PRD 003 closure evidence exists.
2. Spec 032 has active trust lifecycle evidence for proposal, active, stale, revoked, removed, inspect, revoke, dependency preparation eligibility, runtime prerequisite separation, and verified entrypoint lifecycle facts.
3. Spec 035 has trust persistence, migration, owner-safe mutation admission, execution snapshot refs, and tests proving stale, revoked, removed, and mismatched trust are not stored as allow provenance.

Exit gate:

1. PRD 005 proves active exact-match trust is only bounded permission input.
2. Static policy, ceiling, approval correlation, PRD 002 envelope, and PRD 003 proof keep precedence over trust.
3. Stale, revoked, removed, pending, malformed, missing, mismatch, cancellation, and repeated interruption cases reject before spawn.
4. Real-surface QA records trust inspect, dependency preparation, verified entrypoint, stale/revoked/removed rejection, manifest-outside rejection, installer escalation rejection, and diagnostics artifacts.

### Wave 5. Final Integration and Closure

Owner: PRD 006.

Entry gate:

1. PRDs 000 through 005 each have closure evidence.
2. Spec 031 closure evidence locator is present and has local `PASS` status for projection, diagnostics parity, and release runner artifacts consumed by Spec 030.
3. Spec 032 closure evidence locator is present and has local `PASS` status for AppSupervisor, app process lifecycle, app and skill trust lifecycle, permission/secret binding, diagnostics, release, and docs evidence consumed by Spec 030.
4. Spec 035 closure evidence locator is present and has local `PASS` status for config persistence, profile migration, execution snapshots, typed secret ref consumption, immutable snapshots, trust persistence, diagnostics/replay, and docs evidence consumed by Spec 030.
5. Full workspace Cargo gates pass with `--manifest-path crates/Cargo.toml` and `--locked` where Cargo supports it.
6. Agent executed CLI, API, process, diagnostics, and release artifact QA all pass with artifact-backed `Read` audits.

Exit gate:

1. The mapping audit proves every stronger Spec 030 target has one Spec 030 owner and no duplicate owner.
2. The dependency audit proves the DAG has no cycle.
3. The external evidence audit proves Specs 031, 032, and 035 are not absorbed into Spec 030.
4. The failure injection matrix passes.
5. The security and non-guarantee review passes.
6. Documentation update PR is limited to Spec 030 and `docs/specs/README.md` after every other gate is evidenced.

## One-to-One Stronger Target Mapping

| Stronger Spec 030 closure target | Sole Spec 030 owner | External owner evidence required | Closure evidence locator |
|---|---|---|---|
| Unified policy and safety correlation snapshot | PRD 000 | Spec 035 for config, context, provider execution snapshot refs and immutable storage evidence. Spec 031 for diagnostics or release rendering of the ref. | `.omo/evidence/spec030/final/prd000-policy-safety-snapshot.md`, `.omo/evidence/spec030/final/external/spec035-execution-snapshot-evidence.md`, `.omo/evidence/spec030/final/external/spec031-policy-snapshot-projection.md` |
| Typed secret references and redaction provenance | PRD 001 | Spec 032 for app and trust-bound secret producers. Spec 035 for config/profile persistence and typed secret ref consumption. Spec 031 for diagnostics and release rendering. | `.omo/evidence/spec030/final/prd001-secret-ref-redaction.md`, `.omo/evidence/spec030/final/external/spec032-secret-binding-evidence.md`, `.omo/evidence/spec030/final/external/spec035-secret-ref-persistence.md`, `.omo/evidence/spec030/final/external/spec031-secret-ref-projection.md` |
| Common process envelope and side-effect permission gate | PRD 002 | Spec 032 for AppSupervisor and app process lifecycle producers. Spec 035 for physical execution snapshots and persistence refs. Spec 031 for receipt projection and release artifacts. | `.omo/evidence/spec030/final/prd002-process-envelope-gate.md`, `.omo/evidence/spec030/final/external/spec032-app-process-evidence.md`, `.omo/evidence/spec030/final/external/spec035-process-snapshot-evidence.md`, `.omo/evidence/spec030/final/external/spec031-process-receipt-projection.md` |
| Containment inheritance and permission-ceiling proof | PRD 003 | Spec 031 for projection, diagnostics parity, and release evidence rendering. Spec 032 for app lifecycle state consumed by app proofs. Spec 035 for physical execution snapshot refs. | `.omo/evidence/spec030/final/prd003-containment-proof.md`, `.omo/evidence/spec030/final/external/spec031-containment-proof-projection.md`, `.omo/evidence/spec030/final/external/spec032-app-state-proof-input.md`, `.omo/evidence/spec030/final/external/spec035-containment-snapshot-ref.md` |
| Classifier routing, budget, and accounting | PRD 004 | Spec 031 for diagnostics, API, and release artifact rendering. Spec 035 for provider execution snapshot and route source refs. | `.omo/evidence/spec030/final/prd004-classifier-accounting.md`, `.omo/evidence/spec030/final/external/spec031-classifier-projection.md`, `.omo/evidence/spec030/final/external/spec035-provider-execution-snapshot.md` |
| Skill trust permission provenance and verified entrypoints | PRD 005 | Spec 032 for trust lifecycle, inspect, revoke, dependency preparation eligibility, runtime prerequisite separation, and verified entrypoint lifecycle facts. Spec 035 for trust persistence, migration, mutation admission, and execution snapshot refs. Spec 031 for trust decision projection and release rendering. | `.omo/evidence/spec030/final/prd005-skill-trust-permission.md`, `.omo/evidence/spec030/final/external/spec032-skill-trust-lifecycle.md`, `.omo/evidence/spec030/final/external/spec035-skill-trust-persistence.md`, `.omo/evidence/spec030/final/external/spec031-skill-trust-projection.md` |

Mapping rules:

1. PRD 006 is not listed as a stronger target owner because it owns closure sequencing only.
2. A target with missing external evidence remains open even if its Spec 030 PRD tests pass.
3. A target with two Spec 030 owners fails closure until the duplicate ownership is removed from the docs and evidence.
4. A target that points only to prose, a grep result, user approval, or admin approval fails closure.

## External Closure Evidence Locators

The final implementation worker must create or collect these repo-relative locators. The files may sit under the implementation attempt evidence directory, but each locator must include command transcripts, artifact paths, and `Read` audit notes.

| External spec | Required spec section to read | Required evidence locator | Must prove |
|---|---|---|---|
| Spec 031 | `docs/specs/031-ui-projection-diagnostics-and-release-evidence-parity/SPEC.md`, Closure Evidence | `.omo/evidence/spec030/final/external/spec031-closure-read-audit.md` | Projection schema, CLI, TUI when implemented, API/channel, diagnostics parity, backpressure, release runner, and docs evidence render Spec 030 artifacts without creating domain truth. |
| Spec 032 | `docs/specs/032-app-maker-runtime-and-extension-lifecycle/SPEC.md`, closure evidence | `.omo/evidence/spec030/final/external/spec032-closure-read-audit.md` | AppSupervisor, app start/stop/recover, permission/secret binding, extension provenance, diagnostics, release coverage, docs, and skill trust lifecycle evidence exist and are not assumed from baseline specs. |
| Spec 035 | `docs/specs/035-configuration-runtime-layout-and-execution-snapshots/SPEC.md`, Closure Evidence | `.omo/evidence/spec030/final/external/spec035-closure-read-audit.md` | Config migration, JSON compatibility, profiles, typed secret ref consumption, runtime layout, execution snapshots, immutability, budget, live context config wiring, diagnostics/replay, docs, and trust persistence evidence exist. |

External evidence failure rules:

1. `Status: Open` on any external spec is acceptable during implementation waves but blocks final closure unless the required evidence exists and the local PASS rule below is satisfied.
2. Existing Specs 005, 017, 021, 025, 026, and 029 evidence may be cited as baseline only. They cannot substitute for Specs 031, 032, or 035 closure evidence.
3. A missing locator is a hard failure.
4. A locator with only prose and no command transcript or artifact path is a hard failure.

Local PASS rule for external evidence:

1. `PASS` is a local evidence status, not admin acceptance and not user manual acceptance.
2. Each external locator must contain `status: PASS`, the exact source spec status observed by `Read`, the exact command transcript or artifact path that proves the evidence, and an artifact-backed `Read` audit written by an agent.
3. If the external spec is still `Status: Open`, the locator must also include the specific owner-gate reason and a blocked artifact. A blocked artifact is not `PASS` for final closure.
4. The artifact-backed `Read` audit must inspect the evidence file or bundle contents, not only the text of this PRD.
5. Any `PASS` derived from a human statement, issue comment, admin approval, or unchecked checklist is invalid.

## Cargo Gates

Focused commands must run before the workspace gate. All commands must be run from the repository root.

Reproducibility rule: final implementation gates must use `--locked` on `cargo test`, `cargo clippy`, and `cargo build`. `cargo fmt` does not use `--locked`; it still must use the workspace manifest and `--check`.

### Focused PRD Commands

```sh
cargo fmt --manifest-path crates/Cargo.toml --all -- --check
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core permission_action
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core permission_policy
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core permission_approval
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core permission_audit
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-redaction
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core process_envelope
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core process_gate
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core containment_permission
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core runtime_loop
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core runtime_agent
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core plugin_runtime
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core mcp
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-skills
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-app app_environment
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-app skill_trust
cargo clippy --manifest-path crates/Cargo.toml --locked -p shacs-core --all-targets -- -D warnings
```

### Workspace Commands

```sh
cargo fmt --manifest-path crates/Cargo.toml --all -- --check
cargo clippy --manifest-path crates/Cargo.toml --locked --workspace --all-targets -- -D warnings
cargo test --manifest-path crates/Cargo.toml --locked --workspace
cargo build --manifest-path crates/Cargo.toml --locked -p shacs-cli
```

Cargo failure rules:

1. A focused test failure blocks that PRD wave.
2. A workspace failure blocks Wave 5 even if focused commands pass.
3. `--locked` failures must be resolved by an intentional manifest and lockfile decision, not by silent lockfile rewrites.
4. This documentation authoring task must not run Cargo.

## Fixture Registry

Final QA must use this concrete fixture registry. The registry itself is evidence and must be saved at `.omo/evidence/spec030/final/fixture-registry.json` before QA starts. Fixture setup may create files under `.omo/evidence/spec030/final/fixtures/` and `/tmp/shacs-spec030-final-ws`, but it must not edit source files.

| Fixture | Owner | File or setup command | Lifecycle | Evidence locator |
|---|---|---|---|---|
| `spec030-plugin-observer` | PRD 002 process envelope, Spec 031 projection | `mkdir -p .omo/evidence/spec030/final/fixtures/plugin-observer/bin /tmp/shacs-spec030-final-ws/plugins/spec030-plugin-observer && cp -R .omo/evidence/spec030/final/fixtures/plugin-observer/. /tmp/shacs-spec030-final-ws/plugins/spec030-plugin-observer/` | Create fixture, run `plugins list`, run `plugins inspect spec030-plugin-observer`, run `plugins doctor`, record no process execution for inspect/doctor, clean temp plugin root. | `.omo/evidence/spec030/final/qa/plugin-observer-read-audit.md` |
| `spec030-mcp-echo` | PRD 002 process envelope, PRD 003 containment proof | `mkdir -p .omo/evidence/spec030/final/fixtures/mcp-echo/bin /tmp/shacs-spec030-final-ws/mcp && cp -R .omo/evidence/spec030/final/fixtures/mcp-echo/. /tmp/shacs-spec030-final-ws/mcp/spec030-mcp-echo/` | Configure disabled-by-default MCP stdio declaration, run `runtime inspect`, run `runtime diagnostics`, record startup blocked or envelope-gated evidence after PRD 002 implementation, clean temp MCP files. | `.omo/evidence/spec030/final/qa/mcp-echo-read-audit.md` |
| `spec030-app-clock` | Spec 032 app lifecycle, PRD 002 app process gate consumer | `cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- apps init spec030.clock --workspace /tmp/shacs-spec030-final-ws` | Current surface: authoring draft, `apps install`, `apps list`, `apps inspect`, `apps enable`, `apps disable`, `apps uninstall`. Future app process start remains blocked until Spec 032 provides a real command and evidence. | `.omo/evidence/spec030/final/qa/app-clock-registry-read-audit.md` |
| `spec030-skill-weather` | PRD 005 trust consumer, Specs 032 and 035 trust lifecycle | `mkdir -p /tmp/shacs-spec030-final-ws/skills/spec030-weather && printf '%s\n' '---' 'name: spec030-weather' 'description: Spec030 trust fixture' '---' 'Use only read-only weather fixture text.' > /tmp/shacs-spec030-final-ws/skills/spec030-weather/SKILL.md` | Current surface: `skills list --all`, `skills show spec030-weather`, `skills recipes --all`. Future trust inspect, dependency preparation, and verified entrypoint execution remain blocked until Specs 032/035 and PRD 005 provide real commands and evidence. | `.omo/evidence/spec030/final/qa/skill-weather-read-audit.md` |
| `spec030-classifier-fake-provider` | PRD 004 classifier accounting, Spec 035 provider snapshot | `mkdir -p .omo/evidence/spec030/final/fixtures/fake-provider && printf '%s\n' '{"route":"permission_classifier.primary","model":"spec030-fake-model","usage":{"input_tokens":7,"output_tokens":0},"latency_ms":3,"cost_state":"measured"}' > .omo/evidence/spec030/final/fixtures/fake-provider/allow.json` | Feed deterministic fake provider responses through PRD 004 tests and real diagnostics after implementation; include allow, static-deny-conflict, provider-error, malformed-verdict, and missing-usage files. | `.omo/evidence/spec030/final/qa/classifier-fake-provider-read-audit.md` |
| `spec030-runtime-api` | PRD 006 final API diagnostics QA | `mkdir -p /tmp/shacs-spec030-final-ws .omo/evidence/spec030/final/qa` | Start API in background, readiness probe `/health` then `/v1/diagnostics`, curl diagnostics, shutdown by recorded PID, save cleanup receipt. | `.omo/evidence/spec030/final/qa/api-diagnostics-read-audit.md` |

Fixture registry rules:

1. The registry must record the SHA 256 digest of every fixture file before use and after cleanup when the file persists as evidence.
2. Current read-only surfaces may prove current registry or diagnostics behavior. They cannot be counted as app process start, skill trust, dependency preparation, or verified entrypoint execution closure.
3. Future owner-gate surfaces must add their own concrete command, artifact path, and `Read` audit before PRD 006 can count them.
4. A missing fixture file, missing setup transcript, or missing lifecycle cleanup receipt blocks final QA.

## Agent-Executed Real-Surface QA

Every command below must run in an isolated temporary workspace. The implementation worker must save stdout, stderr, exit code, timeout setting, relevant artifact paths, and cleanup receipts.

### CLI Permission, Approval, and Audit Surface

```sh
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- ask "run pwd with the exec tool" --workspace /tmp/shacs-spec030-final-ws
```

Required artifact: `.omo/evidence/spec030/final/qa/cli-permission-approval-audit.json`.

It must contain action id, action digest, policy safety snapshot ref, approval lineage when asked, process envelope id, redacted receipt id, and no raw secret values.

### API Diagnostics Surface

```sh
mkdir -p .omo/evidence/spec030/final/qa /tmp/shacs-spec030-final-ws
API_PORT=38930
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- serve --workspace /tmp/shacs-spec030-final-ws --host 127.0.0.1 --port ${API_PORT} > .omo/evidence/spec030/final/qa/api-server.stdout 2> .omo/evidence/spec030/final/qa/api-server.stderr &
API_PID=$!
printf '{"pid":%s,"port":%s}\n' "${API_PID}" "${API_PORT}" > .omo/evidence/spec030/final/qa/api-server-process-receipt.json
python3 -c "import json, sys, time, urllib.request; port=38930; deadline=time.time()+30; last=None
while time.time() < deadline:
    try:
        urllib.request.urlopen(f'http://127.0.0.1:{port}/health', timeout=1).read()
        print(json.dumps({'ready': True, 'probe': '/health'}))
        sys.exit(0)
    except Exception as exc:
        last=repr(exc); time.sleep(0.5)
print(json.dumps({'ready': False, 'last_error': last})); sys.exit(1)" > .omo/evidence/spec030/final/qa/api-readiness.json
curl --fail --show-error --silent --max-time 10 http://127.0.0.1:${API_PORT}/v1/diagnostics > .omo/evidence/spec030/final/qa/api-diagnostics.json
kill ${API_PID}
wait ${API_PID} || true
printf '{"pid":%s,"shutdown":"requested","cleanup":"complete"}\n' "${API_PID}" > .omo/evidence/spec030/final/qa/api-cleanup-receipt.json
```

Required artifacts: `.omo/evidence/spec030/final/qa/api-server-process-receipt.json`, `.omo/evidence/spec030/final/qa/api-readiness.json`, `.omo/evidence/spec030/final/qa/api-diagnostics.json`, and `.omo/evidence/spec030/final/qa/api-cleanup-receipt.json`.

The readiness probe must pass within 30 seconds before `curl` runs. The diagnostics artifact must contain the same redacted projection vocabulary as CLI diagnostics for policy snapshot, secret ref, process receipt, containment proof, classifier evidence, and trust decision refs when present. The cleanup receipt must prove the recorded PID was shut down.

### Process Adapter Surface

```sh
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- plugins list --workspace /tmp/shacs-spec030-final-ws
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- plugins inspect spec030-plugin-observer --workspace /tmp/shacs-spec030-final-ws
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- plugins doctor --workspace /tmp/shacs-spec030-final-ws
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- runtime inspect --workspace /tmp/shacs-spec030-final-ws
```

Required artifacts: `.omo/evidence/spec030/final/qa/plugin-observer-read-audit.md` and `.omo/evidence/spec030/final/qa/mcp-echo-read-audit.md`.

Current plugin and MCP surfaces are management and inspection surfaces. They prove discovery, projection, and diagnostics without executing plugin commands or starting MCP stdio. After PRD 002 implements process envelopes for plugin command-backed tools and MCP stdio startup, the final worker must add artifact-backed process receipt audits for the same concrete fixtures before PRD 006 closure.

### App Registry and Skill Registry Current Surfaces

These commands use current real surfaces only. They do not claim app process start, skill trust inspect, dependency preparation, or verified entrypoint execution exists.

```sh
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- apps init spec030.clock --workspace /tmp/shacs-spec030-final-ws
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- apps list --workspace /tmp/shacs-spec030-final-ws
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- apps inspect spec030.clock --workspace /tmp/shacs-spec030-final-ws
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- apps enable spec030.clock --workspace /tmp/shacs-spec030-final-ws
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- apps disable spec030.clock --workspace /tmp/shacs-spec030-final-ws
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- skills list --all --workspace /tmp/shacs-spec030-final-ws
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- skills show spec030-weather --workspace /tmp/shacs-spec030-final-ws
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- skills recipes --all --workspace /tmp/shacs-spec030-final-ws
```

Required artifacts: `.omo/evidence/spec030/final/qa/app-clock-registry-read-audit.md` and `.omo/evidence/spec030/final/qa/skill-weather-read-audit.md`.

Owner-gate future surfaces:

1. App process start remains blocked until Spec 032 provides a real CLI or API command and evidence locator. Until then, `.omo/evidence/spec030/final/blocked/spec032-app-process-start-blocked.md` is required and final closure fails.
2. Skill trust inspect remains blocked until Spec 032 and Spec 035 provide a real CLI or API command and evidence locator. Until then, `.omo/evidence/spec030/final/blocked/spec032-skill-trust-inspect-blocked.md` is required and final closure fails.
3. Dependency preparation remains blocked until PRD 005, Spec 032, and Spec 035 provide a real CLI or API command and evidence locator. Until then, `.omo/evidence/spec030/final/blocked/prd005-skill-prepare-blocked.md` is required and final closure fails.
4. Verified entrypoint execution remains blocked until PRD 005, Spec 032, and Spec 035 provide a real CLI or API command and evidence locator. Until then, `.omo/evidence/spec030/final/blocked/prd005-skill-entrypoint-blocked.md` is required and final closure fails.
5. Replacing commands must be real CLI or API surfaces and must include command transcript, typed artifact, cleanup receipt when they create resources, and artifact-backed `Read` audit. A design note or manual checklist is not a replacement.

### Diagnostics Bundle Surface

```sh
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- runtime diagnostics --bundle .omo/evidence/spec030/final/qa/runtime-diagnostics.zip --workspace /tmp/shacs-spec030-final-ws
```

Required artifacts: `.omo/evidence/spec030/final/qa/runtime-diagnostics.zip` and `.omo/evidence/spec030/final/qa/runtime-diagnostics-readout.md`.

The readout must be an artifact-backed `Read` audit of the bundle contents and prove raw provider payload, raw tool args, raw secret values, raw env maps, absolute host paths, process handles, raw stdout, and raw stderr are absent from persisted evidence.

## Failure Injection Matrix

| Failure class | Required injection | Required result | Artifact locator |
|---|---|---|---|
| Malformed policy snapshot | Unknown schema, missing digest, mismatched digest, expired ref | Reject before approval reuse or process spawn | `.omo/evidence/spec030/final/failures/policy-snapshot-malformed.json` |
| Raw secret serialization | `value`, `raw`, `env_value`, auth header, private key block in a secret ref fixture | Parser rejects before snapshot, receipt, or diagnostics persistence | `.omo/evidence/spec030/final/failures/secret-ref-raw-value.json` |
| Process bypass | Adapter attempts direct spawn without `ProcessExecutionEnvelope` | Test or code gate rejects bypass, live dispatch count stays zero | `.omo/evidence/spec030/final/failures/process-bypass.json` |
| Unknown or unsafe containment | Native unknown or privileged evidence with bypass request | Ask or deny, never safe or sandboxed | `.omo/evidence/spec030/final/failures/containment-unknown-unsafe.json` |
| Classifier static-deny conflict | Classifier fixture returns allow for protected target or ceiling violation | Static deny or ceiling wins, classifier cannot allow | `.omo/evidence/spec030/final/failures/classifier-precedence.json` |
| Missing accounting | Provider omits usage, price, model source, or timing | Accounting state is unavailable, failed, skipped, estimated, or not applicable, never fabricated zero | `.omo/evidence/spec030/final/failures/classifier-accounting-unavailable.json` |
| Stale trust | Trust lifecycle, source digest, content digest, dependency digest, capability, snapshot, envelope, or proof changes | Reject before installer or entrypoint spawn | `.omo/evidence/spec030/final/failures/skill-trust-stale.json` |
| Revoked or removed trust | Trust record status becomes revoked or removed | Deny reuse even if digests match | `.omo/evidence/spec030/final/failures/skill-trust-revoked-removed.json` |
| Manifest-outside dependency | Package, runtime installer, global install, native build, or lifecycle script appears outside approved manifest | Reject before process spawn | `.omo/evidence/spec030/final/failures/manifest-outside-dependency.json` |
| Replay live dispatch | Replay attempts exec, MCP, plugin, app process, dependency preparation, entrypoint, or secret resolution | Replay rejected, live dispatch count zero | `.omo/evidence/spec030/final/failures/replay-live-dispatch.json` |
| Cancellation and resume | Cancel after approval or after process allow, then resume | Fresh matching evidence required, no assumed success | `.omo/evidence/spec030/final/failures/cancel-resume.json` |
| Repeated interruptions | Repeat cancel or interrupt against the same envelope, proof, or trust input | Terminal denial or cancellation remains idempotent, no duplicate spawn | `.omo/evidence/spec030/final/failures/repeated-interruptions.json` |
| Hung command | Test process exceeds bounded timeout | Timeout receipt records cleanup disposition, late success cannot overwrite terminal outcome | `.omo/evidence/spec030/final/failures/hung-command-timeout.json` |
| Flaky timing | Test would need sleep or wall-clock polling | Test is rejected until it uses fake clock, completion signal, or bounded probe | `.omo/evidence/spec030/final/failures/flaky-test-rejection.md` |
| Misleading success output | stdout says success while typed receipt, digest, or lifecycle evidence fails | Typed evidence wins, success text ignored | `.omo/evidence/spec030/final/failures/misleading-success-output.json` |
| Prompt injection | User prompt, skill body, plugin output, MCP prompt, child result, stdout, or stderr asks to bypass policy | Text is data only and cannot raise mode, widen ceiling, approve, or select adapter | `.omo/evidence/spec030/final/failures/prompt-injection.json` |
| Dirty worktree | Fixture or implementation mutates unrelated files | Gate fails until unrelated changes are removed or isolated outside source | `.omo/evidence/spec030/final/failures/dirty-worktree.md` |

## UltraQA Class Mapping

| UltraQA class | Final implementation gate | Direct authoring gate for this task |
|---|---|---|
| `malformed_input` | PRD 000 through PRD 005 parser and gate tests plus failure matrix artifacts. | Review PRD006 for required malformed fixtures and no new hidden contract. |
| `prompt_injection` | PRD 002, PRD 003, PRD 004, and PRD 005 tests proving text cannot authorize. | Read-walk confirms prompt injection is mapped to final gates. |
| `cancel_resume` | PRD 002 receipt, PRD 003 proof invalidation, PRD 005 approval invalidation, real-surface cancel QA. | Artifact-backed `Read` audit confirms cancel and resume is required and never accepted manually. |
| `stale_state` | Snapshot, secret ref, process envelope, containment proof, classifier evidence, trust lifecycle, app state, external evidence locators reject stale reuse. | Re-read plan, parent spec, PRDs 000 through 005, and external closure sections before writing evidence. |
| `dirty_worktree` | Future implementation uses temp workspaces and `git status --short` scope gates. | Verify only PRD006 and this task evidence file are created. |
| `hung_commands` | All CLI/API/process QA commands use bounded timeout and timeout receipt artifacts. | Confirm authoring runs no Cargo and records no unbounded command proof. |
| `flaky_tests` | Tests use deterministic fixtures, fake clocks, completion signals, or bounded probes. | Confirm PRD006 rejects sleeps and wall-clock proof. |
| `misleading_success_output` | Typed evidence, receipt outcome, lifecycle status, and diagnostics artifacts beat stdout success text. | Read-walk confirms no gate accepts successful prose or command output without typed artifact. |
| `repeated_interruptions` | PRD 002, PRD 003, and PRD 005 require idempotent repeated interruption evidence with no duplicate spawn. | Read-walk confirms repeated interruption is mapped to final gates. |

## Cleanup Registry and Receipts

Every future QA run must create `.omo/evidence/spec030/final/qa/cleanup-registry.json` before creating resources.

Required registry fields:

1. `run_id`.
2. `created_at_unix_ms`.
3. `tmp_workspace`.
4. `tmp_config_paths`.
5. `tmp_ports`.
6. `spawned_processes` as redacted process refs, not raw host handles.
7. `diagnostics_bundles`.
8. `app_ids` and `skill_refs` used as fixtures.
9. `cleanup_commands`.
10. `cleanup_receipts`.
11. `leftover_resources` with safe reason when cleanup cannot remove a resource.

The final QA gate must save `.omo/evidence/spec030/final/qa/cleanup-receipt.json`. Missing cleanup receipt blocks closure even if all functional tests pass.

## Security and Non-Guarantee Review

The final closure worker must run a security review and record `.omo/evidence/spec030/final/security/non-guarantee-review.md`.

Required checks:

1. No text claims redaction is complete secret prevention.
2. No text claims a digest is raw payload integrity proof.
3. No text claims Docker, Compose, bwrap, process envelope, or containment evidence is kernel isolation.
4. No text treats native unknown containment as safe, sandboxed, harmless, warning-only, or always executable.
5. No classifier allow can override static deny, protected target rules, raw credential export denial, containment requirements, or inherited ceiling.
6. No skill name, Markdown body, body hash, requirements text, install metadata, plugin output, MCP prompt, stdout, or stderr can authorize package install, dependency preparation, or verified entrypoint execution.
7. No app manifest permission declaration becomes a persistent grant.
8. No raw secret value, full env map, raw provider payload, raw command with secret material, process handle, absolute host path, raw stdout, or raw stderr persists in snapshot, envelope, receipt, replay input, diagnostics, or release evidence.
9. No hosted vault, SaaS dashboard, admin console, fleet rollout, remote marketplace, dynamic ABI, central approval service, or multi-user RBAC is introduced as a closure requirement.
10. No partial closure wording lets Spec 030 close while PRD or external evidence is missing.

## Documentation Update List

Only after every gate above is evidenced, the closure worker may update documentation.

Allowed final documentation changes:

1. `docs/specs/030-policy-permission-redaction-and-containment-model/SPEC.md`: change `Status: Open` to `Status: Complete (Scoped)` and replace open closure evidence with exact implementation evidence locators.
2. `docs/specs/README.md`: move or reword Spec 030 in the open owner table so the index reflects the final scoped status.

Forbidden final documentation changes unless a separate owner approves them:

1. Specs 031, 032, 035 contract text.
2. Rust source, tests, Cargo manifests, lockfile, config files, or runtime behavior during the final docs-only status transition.
3. User-facing claims that widen non-guarantees.
4. Historical origin spec rewrites.

## Final Closure Condition

Spec 030 and `docs/specs/README.md` may change only after all of the following are true in the same evidence ledger:

1. PRDs 000 through 005 have passed their focused tests, real-surface QA, exit criteria, and closure evidence audits.
2. Spec 031, Spec 032, and Spec 035 external evidence locators exist and pass artifact-backed `Read` audits.
3. The dependency DAG audit proves no cycle.
4. The stronger target mapping audit proves every target maps exactly once to one Spec 030 owner and required external owners.
5. Focused Cargo commands and full workspace Cargo commands pass using `--manifest-path crates/Cargo.toml`.
6. CLI, API, process, diagnostics, app, skill trust, failure injection, and cleanup receipt artifacts exist.
7. Security and non-guarantee review passes.
8. Documentation update diff contains only Spec 030 and `docs/specs/README.md`.

If any item is missing, Spec 030 remains `Status: Open` and `docs/specs/README.md` keeps Spec 030 in the open owner set.

## Authoring Verification for This PRD

This authoring task is complete only when:

1. The author has read `AGENTS.md`, the full plan, parent Spec 030, PRDs 000 through 005, Spec 029 PRD 008, and closure sections from Specs 031, 032, and 035.
2. This file is the only new PRD.
3. `.omo/evidence/task-8-spec030-closure-prds.md` records the artifact-backed `Read` audit, DAG audit, mapping audit, no-index whitespace check, and no Cargo run statement.
4. `Status: Planned` remains in this file.
5. Parent Spec 030 remains `Status: Open`.
