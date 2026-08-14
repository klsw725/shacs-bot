# Spec 033 closure evidence

Verdict: `COMPLETE (SCOPED) - final source-bound release execution PASS`

Spec 033의 self-hosted/local owner 구현은 goal accounting, advisory evaluator consumption, durable automation outcome routing, CAS self-improvement, recorded-only replay, CLI/local API domain projection input과 source-manifest-bound release evidence를 제공한다. QA, goal, code, security, docs 최종 review와 final source-bound release execution을 모두 통과한 `Complete (Scoped)`다. 최종 manifest는 결정론적 locator에 생성되었으며, 향후 재실행에서 명령 실패 또는 manifest 미생성은 shipping을 차단한다. TUI/Tasks adapter parity와 그 release closure는 Spec 035 소유다.

## Requirement mapping

| Owner | 구현 | 집중 증거 |
|---|---|---|
| PRD 000 | legal goal state machine, append-only transition history, session mutation lock, CLI/API lifecycle | `spec033_goal_accounting`, CLI/API parity tests |
| PRD 001 | AgentLoop turn-end evaluator, durable goal continuation, routed consumption, six task-outcome routes | `runtime_loop`, `spec033_automation_service` |
| PRD 002 | Spec029 durable producer/dispatcher, current 030/031 gates, one-shot/recurring/read-only no-agent/skill-backed jobs, unsupported adapter fail-closed | `spec033_automation_dispatch`, `spec033_automation_service`, `spec033_release_edges` |
| PRD 003 | process-wide local artifact CAS, durable transaction recovery, current hook/confirmation/process/sandbox/credential gates | `spec033_local_improvement*`, CLI/API surface tests |
| PRD 004 | transactional trajectory store, recorded-only replay, redacted review artifacts, exact blocker suite | `spec033_snapshot_replay`, `spec033_release_runner`, `spec033_release_closure` |
| PRD 005 | domain projection input/diagnostics, 37 requirement rows, 17 exact blocker rows | five final reviews PASS; final source-bound release PASS |

지원되는 production producer 호출 경계는 `AgentLoop` turn-end evaluation, local heartbeat/cron scheduling, `SubagentRuntime`이 terminal merge를 수락한 뒤 비직렬화 필드로 전달하는 subagent result다. App-task, channel result, local API background result는 `AutomationSourceEventKind`의 typed vocabulary로만 존재하며 production owner-terminal producer가 없다. 이 경계의 metadata/serialized claim은 durable enqueue와 evaluator route side effect 전에 거부된다.

## Current v6 candidate evidence

- Trajectory ID: `spec033-production-no-provider-20260814-v6`
- Trajectory store: `.omo/evidence/spec033-current/production-trajectories-v6`
- Trajectory internal digest: `sha256:2ecb1a26f54f181cc3fc02b0ed2961ea0d90e080cac18ba8e5fad8c6d0601702`
- Trajectory file SHA-256: `96d99e23db43f64357a1865d1f84b6cd4b0bd2f34077a202e6289cd78bc6aae1`
- Artifact schemas: projection `v5`, release `v5`, review `v3`
- Final release execution: PASS; manifest locator: `.omo/evidence/spec033-current/final-production-20260814-v6/manifest.json`
- Coverage: 37 requirement rows and 17 exact blocker rows
- Cargo gates: fmt PASS, clippy PASS, workspace tests 2,357 passed / 0 failed / 1 ignored, clean locked workspace build PASS
- Surface QA: v4 PASS
- Final binary SHA-256: `shacs-bot` `150bfcc0afc48c5670fd99fee76919231731594b9de84f9ae815aa8481e097b0`; `shacs-tui` `cc0e2ca476e795fd1aef8e67661e500cfe3f4627164d102c326a8c5cf90b0f63`; release runner `a228e3ff4858264e7296b10822449b84066b7d11fcc7953c7adeb37547ad95e1`

이 trajectory와 검증 산출물은 gitignored local artifact다. 위 digest는 trajectory 무결성 증거이며 source digest는 tracked docs가 가리키는 generated source manifest에서 확인한다. 현재 review verdict와 정확한 잔여 blocker는 [`evidence/index.json`](evidence/index.json)에 있다. Cargo evidence는 review verdict로 취급하지 않는다.

## Final review state

| Review kind | Verdict | Final |
|---|---|---|
| QA | PASS | yes |
| goal | PASS | yes |
| code | PASS | yes |
| security | PASS | yes |
| docs | PASS | yes |

Tracked closure review blocker는 없다. 향후 재실행에도 release failure는 shipping blocker로 남는다.

1. `final-production-20260814-v6` release runner는 이 정확한 source tree에 대해 성공적으로 실행되었고 source manifest에 결합된 evidence를 생성했다. 향후 재실행의 실행 실패 또는 manifest locator 미생성은 shipping을 차단한다.

## Reproducible commands

```sh
cargo fmt --manifest-path crates/Cargo.toml --all -- --check
cargo clippy --manifest-path crates/Cargo.toml --workspace --all-targets -- -D warnings
cargo test --manifest-path crates/Cargo.toml --workspace
cargo clean --manifest-path crates/Cargo.toml
cargo build --manifest-path crates/Cargo.toml --locked --workspace
```

재현 가능한 final source-bound release command와 generated manifest locator:

```sh
./crates/target/debug/spec033-release-runner --run-id final-production-20260814-v6 --repo-root . --evidence-root .omo/evidence/spec033-current/final-production-20260814-v6 --trajectory-root .omo/evidence/spec033-current/production-trajectories-v6 --trajectory-id spec033-production-no-provider-20260814-v6 --data-dir /tmp/shacs-spec033-data-v6 --mode current-worktree
```

생성된 locator는 `.omo/evidence/spec033-current/final-production-20260814-v6/manifest.json`이다. source manifest digest는 결정론적으로 생성된 manifest에만 포함한다.

## Owner-fact audit

- 029 remains durable scheduling/recovery truth. Spec 033 only produces and consumes typed automation payloads and terminal facts.
- 030 remains hook, confirmation, process, sandbox and credential truth. Automation and self-improvement resolve current facts immediately before effects.
- 031 remains immutable execution snapshot truth. Replay never treats snapshots as current authorization.
- 035는 033 owner facts를 소비하지만 TUI/Tasks adapter parity와 planned release work를 자체 소유한다. 033은 035 closure를 주장하지 않는다.

## Non-guarantees

- Recorded replay is not current authorization, permission grant, or live source truth.
- Local owner receipt origin is a typed recorded provenance label, not cryptographic authentication.
- Bounded release transcripts do not prove complete redaction of all runtime session, log, or trace data.
- Local Cargo and owner evidence do not prove remote delivery ACK, read receipt, or exactly-once delivery.
- Unsupported script-only and external app-task execution adapters fail closed before side effects.
- App-task, channel, local-API background result claims cannot synthesize owner-terminal acceptance; those producer boundaries remain unavailable.
- Self-improvement is limited to an explicitly configured user-owned local artifact root. It is not silent runtime code replacement or a universal updater.
- Failed verification creates a rollback candidate only. Rollback is never automatic and must re-enter current gates.
