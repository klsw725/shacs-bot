# PRD 006. reconnect, backpressure, and drop accounting

Status: Planned revision (implemented baseline)

## Goal

Reconnect, bounded queue, slow consumer, coalesced progress, dropped progress, final outcome delivery를 서로 다른 사실로 accounting하고 CLI/API/WebSocket/channel projection에서 무음 손실이나 무손실 과장을 방지한다.

## Scope

1. Queue depth/capacity and backpressure observation projection.
2. Coalesced and dropped progress counters.
3. Reconnect generation and gap indication.
4. Final outcome pending/delivered/failed accounting independent from progress delivery.
5. Slow-consumer and bounded-queue deterministic tests across supported surfaces.

## Non Scope

1. Exactly-once delivery, durable network delivery, remote acknowledgement guarantee를 추가하지 않는다.
2. Queue scheduler, durable recovery, channel cursor truth를 재소유하지 않는다.
3. Dropped progress를 final outcome success로 상쇄하지 않는다.

## SPEC Inputs

1. PRDs 000 through 002 and PRD 001 adapter harness.
2. `crates/shacs-bus/src/lib.rs`, `crates/shacs-core/tests/runtime_agent.rs`, `crates/shacs-core/tests/runtime_loop.rs`, `crates/shacs-channels/src/lib.rs`.
3. Spec 029 reconnect/delivery owner evidence and Spec 033 automation/event evidence when produced.
4. Parent Spec 035 `Invariants` 4 and 8, `Must Have` 9, `Acceptance Criteria` 3, `Closure Evidence` 6.

## Dependency Cut

1. Runtime/channel owners produce queue and delivery facts; this PRD owns accounting vocabulary and adapters.
2. Progress and terminal outcome semantics from PRD 002 remain authoritative.
3. Missing owner counters are `unavailable`; adapters must not manufacture zero.
4. `final_delivered`는 해당 projection surface의 owner delivery observation일 뿐 사용자 수신, remote acknowledgement, exactly-once를 뜻하지 않는다. Tool completion, confirmation, TUI acknowledgement로 추론하지 않는다.

## Accounting Contract

Each stream or delivery projection must identify an opaque stream/session lineage and report available values independently:

1. Queue depth and capacity at observation time.
2. Accepted, emitted, coalesced, and dropped progress counts.
3. Reconnect generation or gap-known state.
4. Last observed progress sequence or opaque cursor when owner supplies it.
5. Final outcome state: `final_pending`, `final_delivered`, `final_failed`, or `unknown`.
6. Counter availability/freshness so missing values cannot appear as zero.

## Scenario Matrix

| Scenario | Required result |
|---|---|
| normal consumer | no fabricated drops; final delivery recorded independently |
| coalescing | coalesced count increases; not reported as dropped |
| bounded queue full | admission/backpressure visible; drop or rejection follows owner fact |
| slow WebSocket consumer | slow-consumer state and affected progress count visible |
| reconnect with gap | reconnect generation/gap visible; no lossless claim |
| dropped progress, final delivered | both facts visible simultaneously |
| progress delivered, final failed | terminal failure remains authoritative |
| counter unavailable | unavailable, never zero |

## Verification

1. Use bounded deterministic queues and explicit consumer barriers; unbounded sleeps are forbidden.
2. Test every scenario with canonical projection assertions.
3. Compare API/WebSocket/channel diagnostics and CLI summary for the same stream fixture.
4. Repeat reconnect and interruption to prove counters do not reset into false success.

Focused commands:

```sh
cargo fmt --manifest-path crates/Cargo.toml --all -- --check
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-bus
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core runtime_agent
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-channels
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-api websocket
cargo clippy --manifest-path crates/Cargo.toml --locked -p shacs-channels --all-targets -- -D warnings
```

## Agent-Executed Surface QA

1. Start the local API/WebSocket server with a bounded test configuration and recorded cleanup registry.
2. Drive a normal consumer, an intentionally slow consumer, disconnect/reconnect, and final-outcome cases.
3. Capture CLI diagnostics, API diagnostics, WebSocket frames, and channel projection artifacts.
4. Assert coalesced, dropped, reconnect, and final delivery fields independently; record unavailable fields as unavailable.

## Closure Evidence

1. Deterministic scenario results: `.omo/evidence/spec031/prd006/accounting-scenarios.json`.
2. Cross-surface accounting comparison: `.omo/evidence/spec031/prd006/accounting-parity.json`.
3. Reconnect and repeated interruption audit: `.omo/evidence/spec031/prd006/reconnect-audit.md`.
4. QA transcripts and cleanup receipts: `.omo/evidence/spec031/prd006/qa/frame-transcripts.json` and `.omo/evidence/spec031/prd006/qa/main-clean-final.txt`.

## Exit Criteria

1. Queue, coalescing, drops, reconnect, and final delivery are independent facts.
2. Missing accounting is never zero or success.
3. Slow consumer and reconnect cases are deterministic and surface-visible.
4. Focused gates and real-surface QA pass.
