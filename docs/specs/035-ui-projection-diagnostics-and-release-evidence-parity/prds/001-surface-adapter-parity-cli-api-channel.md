# PRD 001. surface adapter parity for CLI, API, WebSocket, and channels

Status: Planned revision (implemented baseline)

## Goal

CLI output, local API responses, WebSocket events, and implemented external channel replies를 PRD 000의 shared projection source에 연결한다. 각 표면은 표현 형식만 달리하며 canonical state, reason, lineage, severity를 재해석하지 않는다.

## Scope

1. CLI status, inspect, diagnostics, session, subagent, tool, approval, recovery, context, plugin, app, media projection adapters.
2. Local API diagnostics/session/readiness-related response adapters.
3. WebSocket and external channel projection envelopes for supported events and replies.
4. Cross-surface golden fixture and parity test harness.

## Non Scope

1. Interactive TUI, REPL, onboard wizard command flow는 PRD 005가 소유한다.
2. Backpressure counters and reconnect accounting semantics는 PRD 006이 소유한다.
3. Domain owner가 아직 생산하지 않는 app/media state를 synthetic success로 만들지 않는다.

## SPEC Inputs

1. PRD 000 shared projection contract.
2. `crates/shacs-cli/src/lib.rs`, `crates/shacs-api/src/lib.rs`, `crates/shacs-channels/src/lib.rs`.
3. API dependency boundary in `crates/shacs-api/Cargo.toml` and CLI integration boundary in `crates/shacs-cli/Cargo.toml`.
4. Parent Spec 035 `Must Have` 2, `Acceptance Criteria` 1, `Closure Evidence` 2 and 5.

## Dependency Cut

1. PRD 000 must pass before adapter migration starts.
2. Surface-specific formatting may add labels, ordering, and concise summaries only.
3. Unsupported surface/capability pairs must be explicitly absent or `unsupported`; they must not silently use a different status vocabulary.

## Adapter Contract

| Surface | Required source | Required preservation |
|---|---|---|
| CLI | shared projection builder | canonical state, severity, reason code, opaque lineage |
| Local API | same builder or serialized shared envelope | schema version, state, severity, redacted reason |
| WebSocket | shared event projection | event kind, progress/final distinction, lineage |
| External channel | shared reply projection | terminal status, safe reason, thread/reply metadata without domain reinterpretation |

Each supported projection kind must have one canonical fixture that is fed to every adapter. Parity is evaluated on canonical fields, not byte-identical presentation.

Subagent projection must preserve parent lineage, progress, terminal result, inherited ceiling, and safe failure reason. Tool projection must preserve action lineage, execution/result/error state, and redacted receipt or error reason without raw arguments or output.

## Failure Rules

1. An adapter cannot rename `degraded` to success, `unknown` to ready, or `dropped` to delivered.
2. Omitted canonical fields fail parity unless the surface is documented as not supporting that capability.
3. Channel text must not claim lossless delivery when progress was coalesced or dropped.
4. API/CLI diagnostics must apply the same projection disclosure/redaction policy before serialization or rendering. 이는 underlying session/log/trace의 complete redaction을 뜻하지 않는다.
5. Sandbox `active`의 adapter scope, `trusted_native_fallback` warning, hook denial, ephemeral confirmation, raw-content disclosure는 adapter가 success/approval로 재해석하면 안 된다.

## Verification

1. Add fixture-driven parity tests for every implemented surface, including subagent progress/result/ceiling/failure and tool execution/result/error/redaction.
2. Test supported and unsupported surface/capability pairs explicitly.
3. Test raw credential, credential-bearing path/URL, provider payload, and process handle absence in every projection output while preserving safe owner references and disclosure status.
4. Exercise real CLI and API surfaces from an isolated workspace; exercise WebSocket and one configured external-channel fixture through existing channel boundaries.

Focused commands:

```sh
cargo fmt --manifest-path crates/Cargo.toml --all -- --check
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-projection
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-api
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-channels
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-cli
cargo clippy --manifest-path crates/Cargo.toml --locked -p shacs-cli --all-targets -- -D warnings
```

## Agent-Executed Surface QA

1. Run CLI status/diagnostics and subagent/tool fixtures against `/tmp/shacs-spec031-prd001` and save stdout, stderr, exit code, and cleanup receipt.
2. Start local API on a recorded temporary port, probe `/health`, request `/v1/diagnostics` and session projection routes, then stop the recorded process.
3. Connect a WebSocket client, capture projection events, and confirm final outcome vocabulary matches CLI/API for the same fixture.
4. Run an external channel adapter fixture without real credentials and prove unsupported/skipped readiness is explicit rather than success.

## Closure Evidence

1. Cross-surface fixture registry: `.omo/evidence/spec031/prd001/parity/fixture-registry.json`.
2. Canonical field comparison: `.omo/evidence/spec031/prd001/parity/parity-matrix.json`.
3. CLI/API/WebSocket/channel transcripts and cleanup receipts: `.omo/evidence/spec031/prd001/cli/qa-summary.md`, `.omo/evidence/spec031/prd001/api/canonical-field-checks.json`, and `.omo/evidence/spec031/prd001/channel/todo7-verification.md`.
4. Redaction read audit: `.omo/evidence/spec031/prd001/parity/qa-redaction.json`.

## Exit Criteria

1. Every implemented adapter consumes PRD 000 vocabulary.
2. Same owner record yields the same canonical state, severity, reason, and lineage on each supported surface.
3. Unsupported capabilities are explicit.
4. Focused gates and real-surface QA pass.
