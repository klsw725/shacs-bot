# PRD 004. classifier routing, budget, and accounting

Status: Planned

## Goal

This PRD defines the typed evidence contract for permission classifier routing, latency, token, cost, fallback, and unavailable accounting.

The classifier remains advisory. Its `allow_candidate` can only help resolve a policy state that static policy and inherited ceiling already marked reviewable. It cannot widen permission mode, bypass a static deny, bypass a protected target, bypass containment requirements, or exceed a parent ceiling.

The goal is not to make provider billing exact. The goal is to make every classifier attempt explainable: which action was reviewed, which route and model were selected, what was measured or estimated, what was unavailable and why, how fallback happened, what disposition reached policy, and which redacted diagnostics can prove the path.

## Scope

1. Define `ClassifierDecisionEvidence` and related typed accounting fields consumed by permission policy, audit, recent denial, diagnostics, and release evidence.
2. Require request and action correlation using provider call id, action id, action digest, argument digest, and policy safety snapshot ref from PRD 000.
3. Define route identity, model identity, fallback cause, latency measurement, token usage, token estimates, cost status, confidence, static policy precedence, result disposition, and redacted diagnostic refs.
4. Define accounting states for measured, estimated, unavailable, skipped, failed, and not applicable values without fabricated zeroes.
5. Define route, fallback, and unavailable lifecycle behavior for direct tool actions and resolved deferred bridge actions.
6. Require fail-closed behavior when classifier config, classifier provider, classifier parse, or accounting input is unavailable.
7. Require deterministic TDD fixtures and real-surface diagnostics QA for normal and fallback decisions.

## Non Scope

1. Provider budget model, pricing catalog, billing reconciliation, quota policy, or user spending limits.
2. Provider execution snapshot persistence, provider config persistence, model profile storage, immutable snapshot storage, or migration. Spec 035 owns those.
3. UI projection, diagnostics rendering parity, release artifact rendering, or release runner output. Spec 031 owns those.
4. Static permission rules, approval cache, recent denial retry approval, or inherited ceiling semantics. This PRD consumes those existing contracts and requires regression tests around them.
5. Provider adapter wire protocol, provider tokenization internals, or model-specific tokenizer implementation.
6. Prompt text wording for classifier instructions. Tests must assert routing, typed fields, enum values, and dispositions, not prose.

## SPEC Inputs

1. Spec 030 `## 구현된 기준선` keeps current guarded classifier fallback as the baseline and marks classifier cost, latency, and model routing accounting as incomplete.
2. Spec 030 `## 소유하는 open scope` says static policy, protected target rules, permission ceiling, and guarded classifier fallback are consumed as execution gates, not permission expansion.
3. Spec 030 `## Implementation PRDs` and `### Stronger Contract Owner Map` index this PRD as the sole Spec 030 owner for classifier routing, budget, and accounting.
4. Spec 030 `## Invariants` and `## Must Not Have` require static rule precedence and forbid classifier allow from overriding static deny or ceiling.
5. Spec 030 `### Internal Dependency Gates` says PRD 006 cannot close Spec 030 by grep-only or prose-only evidence.
6. Spec 022 PRD 003 supplies the current evaluator policy contract: classifier output is advisory, failures become ask or deny, and final allow only reaches runtime when rule, mode, evaluator, and snapshot conditions pass.
7. Spec 022 PRD 007 supplies the current digest-only recent denial record and exact retry boundary.
8. Current Rust facts are `permission_policy.rs`, `permission_recent_denials.rs`, and classifier routing in `runner.rs`.
9. Spec 031 owns projection parity and release rendering of any classifier accounting evidence.
10. Spec 035 owns provider execution snapshot persistence and model or profile source persistence.

## Dependency Cut

1. PRD 000 must provide the immutable policy and safety snapshot ref before this PRD can close. Classifier evidence must carry that ref and digest.
2. This PRD owns classifier evidence semantics and accounting enum values.
3. Spec 035 may persist provider execution snapshots that reference classifier evidence, but it must not redefine classifier accounting states or static policy precedence.
4. Spec 031 may render classifier evidence in CLI, TUI, API, diagnostics bundles, and release artifacts, but it must not turn missing accounting into success or zero cost.
5. Current runtime behavior remains valid before this PRD lands. The first implementation must characterize current classifier allow, denial, failure, prompt injection, and recent-denial behavior before adding new fields.

## Typed Contract

The future implementation must define a typed contract equivalent to this Rust shape. Module names may differ, but field meaning and enum values must stay fixed.

```rust
pub struct ClassifierDecisionEvidence {
    pub schema_id: ClassifierEvidenceSchemaId,
    pub evidence_id: ClassifierEvidenceId,
    pub created_at_unix_ms: u64,
    pub request: ClassifierRequestCorrelation,
    pub action: ClassifierActionCorrelation,
    pub route: ClassifierRouteEvidence,
    pub model: ClassifierModelEvidence,
    pub token_accounting: ClassifierTokenAccounting,
    pub latency: ClassifierLatencyAccounting,
    pub cost: ClassifierCostAccounting,
    pub verdict: ClassifierVerdictEvidence,
    pub precedence: StaticPolicyPrecedence,
    pub disposition: ClassifierDisposition,
    pub fallback: Option<ClassifierFallbackEvidence>,
    pub diagnostics: Vec<RedactedDiagnosticRef>,
}
```

Required enum values:

| Type | Required values |
|---|---|
| `ClassifierEvidenceSchemaId` | `permission_classifier_evidence.v1` |
| `ClassifierRouteKind` | `primary`, `fallback`, `skipped`, `unavailable` |
| `ClassifierFallbackCause` | `primary_unavailable`, `provider_error`, `provider_timeout`, `parse_failure`, `missing_user_request`, `ineligible_capability`, `static_policy_not_reviewable`, `config_unavailable`, `accounting_unavailable` |
| `AccountingState` | `measured`, `estimated`, `unavailable`, `skipped`, `failed`, `not_applicable` |
| `AccountingUnavailableReason` | `provider_omitted_usage`, `tokenizer_unavailable`, `price_unconfigured`, `config_unavailable`, `clock_unavailable`, `provider_error`, `parse_failure`, `malformed_accounting_input`, `static_policy_not_reviewable` |
| `ClassifierDisposition` | `not_invoked_static_policy`, `not_invoked_ceiling`, `not_invoked_ineligible`, `allow_candidate_consumed`, `ask_user`, `deny_candidate_recorded`, `fallback_used`, `failed_closed` |
| `StaticPolicyPrecedence` | `static_deny_wins`, `ceiling_wins`, `static_ask_blocks_classifier`, `classifier_reviewable`, `approval_required` |

## Field Rules

1. `request.provider_call_id` is the classifier provider request id when a provider call was attempted. If unavailable, the field is absent and `route.kind` or `cost.state` explains why.
2. `request.classifier_request_digest` is a digest of redacted classifier input. It is correlation evidence, not raw prompt replay.
3. `action` includes runtime action id, provider tool call id when available, tool name, action digest, argument digest, policy safety snapshot ref, and capability set.
4. `route.route_id` is a stable route identity such as `permission_classifier.primary` or `permission_classifier.fallback.local_static`. It is not a provider secret or config path.
5. `route.kind` records whether the route was primary, fallback, skipped, or unavailable.
6. `model.model_id` is the selected provider/model identity used for the classifier call. If no call was made, it records the intended route identity only when known.
7. `model.source_ref` points to a redacted config or provider execution snapshot ref when Spec 035 supplies one. This PRD does not store that snapshot.
8. `token_accounting.input` and `token_accounting.output` each carry an `AccountingState`. A missing provider usage field is `unavailable`, not `0`.
9. Token estimates must carry estimator id, input basis, and confidence. They must not pretend to be measured provider tokens.
10. `latency` uses monotonic injected clock measurements in tests. If the clock or timing boundary is unavailable, state is `unavailable`, not `0 ms`.
11. `cost` records measured cost, estimated cost, unavailable reason, or not applicable. Unknown price is `unavailable` or `estimated` with source, never zero.
12. `verdict` records classifier verdict, confidence, scope match, prompt injection signal count, and redacted explanation refs. External explanations are untrusted data.
13. `precedence` records the static policy decision and ceiling decision that bounded classifier review before any classifier disposition was consumed.
14. `disposition` is the machine-consumed result handed to permission policy and diagnostics.
15. `fallback` records cause, previous route, selected route, and whether a provider call happened.
16. `diagnostics` contains only redacted refs. It must not contain raw classifier response, raw command, raw arguments, raw prompt, raw secret, env, host path, process handle, raw session id, or raw turn id.

## Accounting States

Accounting is evidence. It must never hide missing information with a successful looking value.

| State | Meaning | Numeric value allowed | Closure rule |
|---|---|---:|---|
| `measured` | Provider or runtime measured the value directly | yes | include source ref and unit |
| `estimated` | Runtime estimated with a named estimator | yes | include estimator id, basis, and confidence |
| `unavailable` | Value should exist but was not available | no | include unavailable reason |
| `skipped` | Classifier was intentionally not invoked | no | include skip reason |
| `failed` | Attempt failed before usable accounting existed | no | include failure reason |
| `not_applicable` | Value does not apply to this route | no | include route reason |

Rules:

1. `0` is valid only when state is `measured` or `estimated` and the source proves a real zero, such as zero output tokens from a provider usage object.
2. Missing usage, missing price, missing model id, timeout, provider error, parse failure, and malformed accounting input are not zero.
3. Diagnostics and release evidence must display unavailable and estimated states as such.
4. Cost accounting must include currency or pricing source only when known. If currency is unknown, cost is unavailable.
5. Token estimates must not be used as provider budget enforcement unless a separate provider budget owner defines that policy.

## Static Policy Precedence

Classifier routing can happen only after static policy and ceiling say the action is reviewable.

1. If inherited ceiling evaluation rejects the request, the classifier is not invoked. Disposition is `not_invoked_ceiling` and policy decision remains deny.
2. If static policy returns hard deny, the classifier is not invoked. Disposition is `not_invoked_static_policy` and policy decision remains deny or current ask-or-deny behavior where the existing mode already requires it.
3. If static ask is caused by containment unknown or unsummarized proc exec, classifier cannot convert that state into allow.
4. If action capability is outside the classifier eligibility set, classifier is not invoked and the action stays approval-gated or denied.
5. Only when mode is `Auto`, action is reviewable, user request summary exists, capability is eligible, and static policy is not blocking may classifier evidence be consumed.
6. Even then, only high confidence `allow_candidate` with requested scope can produce `allow_candidate_consumed`. Prompt injection signals, low confidence, adjacent scope, hostile scope, parse failure, timeout, or unavailable provider become ask or deny.

## Route and Fallback Lifecycle

1. Runtime normalizes the action and computes static policy plus inherited ceiling before selecting a classifier route.
2. Runtime records `not_invoked_static_policy`, `not_invoked_ceiling`, or `not_invoked_ineligible` when those gates block classifier use.
3. If reviewable, runtime selects the primary classifier route from current config or execution snapshot input.
4. Runtime emits a redacted request correlation before provider dispatch or records config unavailable.
5. Provider success with parseable verdict records route, model, token, latency, cost state, verdict, and disposition.
6. Provider error, timeout, parse failure, malformed accounting input, or missing config records fallback evidence.
7. Fallback may ask the user, deny in non-interactive mode, or keep the current static policy disposition. Fallback cannot produce a silent allow.
8. Recent classifier denials may be recorded only for classifier-origin `deny_candidate` decisions that pass the current PRD 007 retryability rules. Failed, skipped, unavailable, static deny, and protected target paths are not retryable recent denials.
9. Every lifecycle step must keep the same action digest, argument digest, policy safety snapshot digest, and redacted diagnostic refs.

## Normal Sequence

1. Runtime normalizes a direct tool action or resolved deferred bridge action into a permissioned action.
2. Runtime attaches `PolicySafetySnapshotRef` from PRD 000 and computes static policy plus inherited ceiling.
3. Static policy and ceiling mark the action reviewable.
4. Runtime selects `permission_classifier.primary` with provider and model identity from current execution inputs.
5. Runtime sends a redacted classifier request using the action digest, argument digest, capability set, target refs, redacted arguments, and redacted user request summary.
6. Runtime measures latency with an injected clock and records provider usage if present.
7. Runtime parses a typed verdict. External explanation and evidence refs are treated as data and projected through redaction.
8. High confidence, requested-scope `allow_candidate` becomes `allow_candidate_consumed`; otherwise the action asks or denies.
9. Diagnostics receive redacted refs for request, action, route, model, accounting states, fallback, and final disposition.

## Failure Sequences

1. Static policy returns deny or inherited ceiling rejects. Runtime does not call the classifier and records `static_deny_wins` or `ceiling_wins`.
2. Classifier config is missing or unreadable. Runtime records `config_unavailable`, fails closed to ask or deny, and does not fabricate route, model, token, latency, or cost success.
3. Provider call fails or times out. Runtime records fallback cause, failed or unavailable accounting states, and asks or denies.
4. Provider response contains prose-wrapped JSON, nested JSON, aliases, unknown enum values, or malformed accounting input. Parser rejects it or accounting rejects it, and disposition is `failed_closed` or `fallback_used`.
5. Provider returns high-confidence allow for a protected target, denied capability, ceiling violation, unknown containment, or unsummarized unsafe exec. Static precedence prevents allow.
6. Provider omits usage or price is not configured. Runtime records unavailable token or cost state, not zero.
7. Diagnostics rendering is unavailable. Runtime stores redacted diagnostic refs when possible and blocks success claims that depend on missing diagnostics.

## Diagnostics Handoff to Spec 031

This PRD supplies typed evidence. Spec 031 renders it.

The handoff payload must include:

1. `classifier_evidence_id` and schema id.
2. Action id, provider call id when available, action digest, argument digest, and policy safety snapshot digest.
3. Route id, route kind, model id, model source ref, fallback cause, and disposition.
4. Token accounting states for input and output with measured value, estimate details, or unavailable reason.
5. Latency state with measured duration, estimate, or unavailable reason.
6. Cost state with measured amount, estimate details, or unavailable reason.
7. Classifier verdict, confidence, scope match, prompt injection signal count, and redacted explanation refs.
8. Static policy precedence and ceiling outcome.
9. Redacted diagnostic refs only.

Spec 031 must fail its projection QA if unavailable accounting is displayed as zero, if fallback is shown as success, or if static policy precedence disappears from the visible projection.

## Persistence Handoff to Spec 035

Spec 035 may persist provider execution snapshots and model or profile source refs consumed by classifier evidence.

This PRD requires only refs and redacted summaries:

1. `model.source_ref` may point at a Spec 035 execution snapshot or config profile ref.
2. `request.classifier_request_digest` may be included in a provider execution snapshot, but raw classifier prompts, raw action arguments, raw user prompts, and raw provider responses are not required by this PRD.
3. Price source, tokenizer source, and provider usage source may be refs from Spec 035 when available.
4. Missing Spec 035 persistence means accounting state becomes unavailable or estimated, not successful zero.
5. This PRD must not choose storage paths, migration families, retention rules, or profile schema.

## Deterministic TDD Sequence

Implementation must follow this order.

1. Baseline characterization. Lock current tests for `decide_permission`, classifier allow, classifier denial, provider error, malformed verdict, prompt injection signal, recent denial sanitization, and protected target classifier skip.
2. Red proof for static precedence. Add failing tests named `classifier_allow_cannot_override_static_deny` and `classifier_allow_cannot_override_inherited_ceiling`.
3. Red proof for accounting states. Add failing tests where missing provider usage, missing price, provider error, parse failure, malformed accounting input, and missing model source produce unavailable or failed states, not zero.
4. Red proof for route and fallback. Add failing tests for primary route success, config unavailable fallback, provider timeout fallback, parse failure fallback, missing user request skip, and ineligible capability skip.
5. Red proof for diagnostics refs. Add tests proving classifier evidence projections contain evidence id, route id, model id, accounting states, fallback cause, static precedence, and redacted refs without raw prompt, command, argument, secret, host path, raw session id, or raw turn id.
6. Red proof for latency determinism. Use an injected deterministic clock. No sleeps, wall-clock assertions, polling delays, or provider timing guesses are allowed.
7. Minimal implementation. Add typed evidence, route selection evidence, accounting state model, fallback evidence, static precedence recording, and diagnostics handoff.
8. Refactor only after green. Keep classifier policy advisory and preserve current denial/retry behavior.
9. Wider regression. Run focused Cargo commands and real-surface diagnostics QA below.

Test assertions must target machine-consumed fields, enum values, accounting states, routing identity, digest correlation, disposition, and redacted serialized shapes. Tests must not assert classifier prompt prose.

## Focused Cargo Targets and Commands

Future implementation must run these from the repository root:

```sh
cargo fmt --manifest-path crates/Cargo.toml --all -- --check
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core --test permission_policy classifier
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core --test runtime_loop classifier
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core --test runtime_agent classifier
cargo clippy --manifest-path crates/Cargo.toml --locked -p shacs-core --all-targets -- -D warnings
```

Before PRD 006 can consume this closure evidence, run the workspace gate from `AGENTS.md`:

```sh
cargo fmt --manifest-path crates/Cargo.toml --all -- --check
cargo clippy --manifest-path crates/Cargo.toml --locked --workspace --all-targets -- -D warnings
cargo test --manifest-path crates/Cargo.toml --locked --workspace
```

## Agent-Executed Real-Surface Diagnostics QA

The implementation worker must make the literal commands in this section pass. Each command exits `0` for PASS and non-zero for FAIL. No network provider is allowed during this QA.

Deterministic fixture contract:

| Field | Value |
|---|---|
| Fixture name | `spec030-prd004-classifier-accounting-v1` |
| Fixture owner | PRD 004 implementation worker |
| Fixture locator | `crates/shacs-core/tests/fixtures/spec030_prd004_classifier_accounting/` |
| CLI config | `crates/shacs-core/tests/fixtures/spec030_prd004_classifier_accounting/config.json` |
| Fake provider script | `crates/shacs-core/tests/fixtures/spec030_prd004_classifier_accounting/provider-fixture.jsonl` |
| Workspace | `/tmp/shacs-prd004-classifier-qa/workspace` |
| Artifacts | `/tmp/shacs-prd004-classifier-qa/artifacts/*.json` and `/tmp/shacs-prd004-classifier-qa/artifacts/diagnostics.zip` |
| PASS receipt | `/tmp/shacs-prd004-classifier-qa/artifacts/prd004-classifier-accounting-receipt.json` |

The fixture must contain these named cases: `normal`, `missing_accounting`, `provider_error`, `malformed_verdict`, `static_deny_precedence`, and `diagnostics_bundle`. The fake provider must expose deterministic timestamps, token usage, omitted usage, provider error, malformed response, and static-deny allow-attempt cases without reading wall clock time.

Setup command:

```sh
rm -rf /tmp/shacs-prd004-classifier-qa && mkdir -p /tmp/shacs-prd004-classifier-qa/workspace /tmp/shacs-prd004-classifier-qa/artifacts
```

Normal classifier decision command and PASS assertion:

```sh
SHACS_FAKE_PROVIDER_FIXTURE=crates/shacs-core/tests/fixtures/spec030_prd004_classifier_accounting/provider-fixture.jsonl SHACS_FAKE_PROVIDER_CASE=normal cargo run --manifest-path crates/Cargo.toml --locked -p shacs-cli -- ask "run the exact verification command" --workspace /tmp/shacs-prd004-classifier-qa/workspace --config crates/shacs-core/tests/fixtures/spec030_prd004_classifier_accounting/config.json --diagnostics-json /tmp/shacs-prd004-classifier-qa/artifacts/normal.json
rg -q '"disposition":"allow_candidate_consumed"' /tmp/shacs-prd004-classifier-qa/artifacts/normal.json && rg -q '"precedence":"classifier_reviewable"' /tmp/shacs-prd004-classifier-qa/artifacts/normal.json && rg -q '"token_accounting"' /tmp/shacs-prd004-classifier-qa/artifacts/normal.json && rg -q '"cost"' /tmp/shacs-prd004-classifier-qa/artifacts/normal.json
```

Missing-accounting fallback command and PASS assertion:

```sh
SHACS_FAKE_PROVIDER_FIXTURE=crates/shacs-core/tests/fixtures/spec030_prd004_classifier_accounting/provider-fixture.jsonl SHACS_FAKE_PROVIDER_CASE=missing_accounting cargo run --manifest-path crates/Cargo.toml --locked -p shacs-cli -- ask "run the exact verification command" --workspace /tmp/shacs-prd004-classifier-qa/workspace --config crates/shacs-core/tests/fixtures/spec030_prd004_classifier_accounting/config.json --diagnostics-json /tmp/shacs-prd004-classifier-qa/artifacts/missing-accounting.json
rg -q '"state":"unavailable"' /tmp/shacs-prd004-classifier-qa/artifacts/missing-accounting.json && rg -q '"unavailable_reason":"provider_omitted_usage"' /tmp/shacs-prd004-classifier-qa/artifacts/missing-accounting.json && ! rg -q '"input_tokens":0|"output_tokens":0|"cost_amount":0' /tmp/shacs-prd004-classifier-qa/artifacts/missing-accounting.json
```

Provider error fallback command and PASS assertion:

```sh
SHACS_FAKE_PROVIDER_FIXTURE=crates/shacs-core/tests/fixtures/spec030_prd004_classifier_accounting/provider-fixture.jsonl SHACS_FAKE_PROVIDER_CASE=provider_error cargo run --manifest-path crates/Cargo.toml --locked -p shacs-cli -- ask "run the exact verification command" --workspace /tmp/shacs-prd004-classifier-qa/workspace --config crates/shacs-core/tests/fixtures/spec030_prd004_classifier_accounting/config.json --diagnostics-json /tmp/shacs-prd004-classifier-qa/artifacts/provider-error.json
rg -q '"fallback_cause":"provider_error"' /tmp/shacs-prd004-classifier-qa/artifacts/provider-error.json && rg -q '"disposition":"(failed_closed|fallback_used)"' /tmp/shacs-prd004-classifier-qa/artifacts/provider-error.json && ! rg -q '"can_handoff_to_tool_runtime":true' /tmp/shacs-prd004-classifier-qa/artifacts/provider-error.json
```

Malformed verdict command and PASS assertion:

```sh
SHACS_FAKE_PROVIDER_FIXTURE=crates/shacs-core/tests/fixtures/spec030_prd004_classifier_accounting/provider-fixture.jsonl SHACS_FAKE_PROVIDER_CASE=malformed_verdict cargo run --manifest-path crates/Cargo.toml --locked -p shacs-cli -- ask "run the exact verification command" --workspace /tmp/shacs-prd004-classifier-qa/workspace --config crates/shacs-core/tests/fixtures/spec030_prd004_classifier_accounting/config.json --diagnostics-json /tmp/shacs-prd004-classifier-qa/artifacts/malformed-verdict.json
rg -q '"fallback_cause":"parse_failure"' /tmp/shacs-prd004-classifier-qa/artifacts/malformed-verdict.json && rg -q '"disposition":"failed_closed"' /tmp/shacs-prd004-classifier-qa/artifacts/malformed-verdict.json && ! rg -q '"allow_candidate_consumed"' /tmp/shacs-prd004-classifier-qa/artifacts/malformed-verdict.json
```

Static deny precedence command and PASS assertion:

```sh
SHACS_FAKE_PROVIDER_FIXTURE=crates/shacs-core/tests/fixtures/spec030_prd004_classifier_accounting/provider-fixture.jsonl SHACS_FAKE_PROVIDER_CASE=static_deny_precedence cargo run --manifest-path crates/Cargo.toml --locked -p shacs-cli -- ask "write to .git/config" --workspace /tmp/shacs-prd004-classifier-qa/workspace --config crates/shacs-core/tests/fixtures/spec030_prd004_classifier_accounting/config.json --diagnostics-json /tmp/shacs-prd004-classifier-qa/artifacts/static-deny-precedence.json
rg -q '"precedence":"static_deny_wins"' /tmp/shacs-prd004-classifier-qa/artifacts/static-deny-precedence.json && rg -q '"disposition":"not_invoked_static_policy"' /tmp/shacs-prd004-classifier-qa/artifacts/static-deny-precedence.json && ! rg -q '"classifier_request_sent":true|"allow_candidate_consumed"|"can_handoff_to_tool_runtime":true' /tmp/shacs-prd004-classifier-qa/artifacts/static-deny-precedence.json
```

Diagnostics bundle inspection command and PASS assertion:

```sh
cargo run --manifest-path crates/Cargo.toml --locked -p shacs-cli -- runtime diagnostics --workspace /tmp/shacs-prd004-classifier-qa/workspace --config crates/shacs-core/tests/fixtures/spec030_prd004_classifier_accounting/config.json --bundle /tmp/shacs-prd004-classifier-qa/artifacts/diagnostics.zip --diagnostics-json /tmp/shacs-prd004-classifier-qa/artifacts/diagnostics.json
rg -q '"classifier_evidence_id"' /tmp/shacs-prd004-classifier-qa/artifacts/diagnostics.json && rg -q '"(StaticPolicyPrecedence|static_deny_wins|classifier_reviewable)"' /tmp/shacs-prd004-classifier-qa/artifacts/diagnostics.json && test -s /tmp/shacs-prd004-classifier-qa/artifacts/diagnostics.zip
```

Raw-data audit command and PASS assertion:

```sh
rg -q 'classifier_evidence_id|route_id|model_id|token_accounting|latency|cost|fallback_cause|precedence' /tmp/shacs-prd004-classifier-qa/artifacts && ! rg -q 'RAW_CLASSIFIER_PROMPT_SECRET|RAW_COMMAND_SECRET|RAW_ARGUMENT_SECRET|RAW_PROVIDER_RESPONSE_SECRET|sk-live|Bearer |/Users/example|/tmp/shacs/raw-session|raw-turn' /tmp/shacs-prd004-classifier-qa/artifacts
```

Receipt command and PASS assertion:

```sh
cargo run --manifest-path crates/Cargo.toml --locked -p shacs-cli -- runtime diagnostics --workspace /tmp/shacs-prd004-classifier-qa/workspace --config crates/shacs-core/tests/fixtures/spec030_prd004_classifier_accounting/config.json --classifier-accounting-receipt /tmp/shacs-prd004-classifier-qa/artifacts/prd004-classifier-accounting-receipt.json
rg -q '"fixture":"spec030-prd004-classifier-accounting-v1"' /tmp/shacs-prd004-classifier-qa/artifacts/prd004-classifier-accounting-receipt.json && rg -q '"cleanup_registry_id":"prd006.cleanup.classifier-accounting"' /tmp/shacs-prd004-classifier-qa/artifacts/prd004-classifier-accounting-receipt.json && rg -q '"result":"pass"' /tmp/shacs-prd004-classifier-qa/artifacts/prd004-classifier-accounting-receipt.json
```

## PRD 006 Cleanup Registry and Receipt Linkage

PRD 006 may consume this PRD only when the implementation emits a cleanup receipt and registry entry with these exact fields:

```json
{
  "cleanup_registry_id": "prd006.cleanup.classifier-accounting",
  "owner_prd": "030-004-classifier-routing-budget-and-accounting",
  "fixture": "spec030-prd004-classifier-accounting-v1",
  "workspace": "/tmp/shacs-prd004-classifier-qa/workspace",
  "artifact_dir": "/tmp/shacs-prd004-classifier-qa/artifacts",
  "receipt": "/tmp/shacs-prd004-classifier-qa/artifacts/prd004-classifier-accounting-receipt.json",
  "cleanup_command": "rm -rf /tmp/shacs-prd004-classifier-qa",
  "result": "pass"
}
```

The receipt proves that PRD 004 produced real-surface classifier accounting evidence and that PRD 006 can clean temporary artifacts without owning classifier semantics. If the receipt is missing, has `result` other than `pass`, or omits the cleanup command, PRD 006 must reject this PRD as incomplete.

## Adversarial Matrix

| Class | Required probe | PASS condition | FAIL condition |
|---|---|---|---|
| `malformed_input` | Run `SHACS_FAKE_PROVIDER_CASE=malformed_verdict` and malformed accounting input fixture | disposition is `failed_closed` and accounting state is `failed` or `unavailable` | malformed input becomes allow or zero accounting |
| `prompt_injection` | Fixture includes classifier explanation and evidence refs that ask to widen policy | explanation is redacted data only and static precedence remains unchanged | explanation changes route, mode, ceiling, or disposition |
| `stale_state` | Reuse an old policy safety snapshot digest in the fixture | action is rejected or asks again before classifier allow is consumed | stale digest reaches `allow_candidate_consumed` |
| `dirty_worktree` | Run QA with pre-existing unrelated tracked changes outside PRD 004 | receipt lists only PRD 004 artifacts and does not require plan checkbox edits | QA edits plan, parent spec, 031, 035, or unrelated files |
| `hung_provider` | Fake provider never returns for the classifier case | timeout records `provider_timeout` and fail-closed disposition | command hangs beyond test timeout or reports success |
| `flaky_latency` | Run normal fixture twice with injected deterministic clock | latency evidence is identical and no sleeps are used | wall-clock drift changes expected evidence |
| `misleading_success_output` | CLI prints success text while diagnostics disposition is not allow | binary PASS assertion follows diagnostics disposition, not prose | prose success hides failed accounting or fallback |
| `static_policy_bypass` | Static-deny fixture returns classifier allow if called | classifier is not invoked and precedence is `static_deny_wins` | classifier call overrides static deny or ceiling |
| `raw_data_leakage` | Raw-data audit searches artifacts for raw prompt, command, args, provider response, secrets, env, host path, session, and turn values | no raw marker appears in artifacts or bundle projection | any raw marker appears |

## Evidence and Exit Criteria

PRD 006 may count this PRD as closed only when implementation evidence includes all items below.

1. Baseline characterization passed before new implementation tests went green.
2. Failing-first tests prove classifier allow cannot override static deny and cannot override inherited ceiling.
3. `ClassifierDecisionEvidence` and accounting state types exist with schema id, request/action correlation, route/model identity, token state, latency state, cost state, verdict, fallback, precedence, disposition, and redacted diagnostics refs.
4. Missing accounting, missing config, provider error, parse failure, malformed accounting input, and missing price are unavailable, failed, skipped, or estimated. They are not zero or success.
5. Route and fallback lifecycle is tested for direct tool and resolved deferred bridge paths.
6. Prompt injection signals and external classifier explanations are treated as untrusted redacted data.
7. Focused Cargo commands and workspace Cargo gate pass with `--manifest-path crates/Cargo.toml`.
8. Real-surface diagnostics QA proves normal and fallback decisions flow through accounting into diagnostics.
9. Spec 031 ownership of projection and release rendering remains external.
10. Spec 035 ownership of provider execution snapshot persistence, config persistence, route source persistence, and migration remains external.

## Closure Gate for Consumers

PRD 006 may consume this PRD only after typed evidence, static precedence, unavailable accounting, route fallback lifecycle, deterministic latency tests, redacted diagnostics refs, focused Cargo gates, workspace Cargo gate, and real-surface QA evidence exist.

Until then, current guarded classifier fallback remains the runtime baseline, and classifier route, latency, token, and cost accounting remain planned closure targets.
