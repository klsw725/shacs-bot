# 029. durable runtime recovery and data migration 아키텍처 명세

Status: Open

Origin specs: 001, 002, 005, 006, 011, 012, 014, 015

## 목적

이 문서는 기존 001, 002, 005, 006, 011, 012, 014, 015에서 current architecture closure 밖으로 남긴 durable runtime, recovery, stored-data migration 계약을 새 owner 범위로 모은다.

목표는 self-hosted, personal-use 런타임이 process restart, crash, interrupted upgrade, channel reconnect, child task interruption, partial migration 뒤에도 사용자가 inspect하고 복구할 수 있는 durable 기준을 세우는 것이다. 이 문서는 현재 구현 완료를 주장하지 않는다.

029는 local single-user runtime을 위한 durability 계약이다. 분산 큐, multi-user concurrency, fleet 운영, SaaS control plane은 범위가 아니다. Exactly-once delivery도 증명 없이 주장하지 않는다.

## 현재 구현 baseline

현재 구현은 다음 범위까지 인정한다.

1. 001은 `AgentLoop`, `AgentRunner`, `SessionManager`, `SessionTurnLock`, `runtime_checkpoint`, `pending_user_turn` marker를 current session kernel mapping으로 인정한다. 별도 durable `TurnState`와 formal phase enum은 없다.
2. 002는 command, event, effect를 개념어로 정리했다. Formal append-only event record는 없다.
3. 005는 skill registry discovery, descriptor, body hash, context injection을 current mapping으로 인정한다. Per-turn registry snapshot과 replay provenance snapshot은 없다.
4. 006은 session별 JSONL file, metadata header, message records, `last_consolidated`, `save_with_fsync`, recovery marker materialization을 current persistence로 인정한다. Append-only event log, checkpoint plus tail replay, corruption fallback은 없다.
5. 011은 subagent identity, lifecycle, stale discard, synthetic inbound, bounded parallelism, cancellation cleanup을 current mapping으로 인정한다. Durable child recovery는 없다.
6. 012는 process-local bus, session lock, active task registry, channel workers, process-local follow-up queue, runtime metadata JSON hint를 current services로 인정한다. Durable queue, scheduler, restart replay, owner lease, safe shutdown supervisor는 없다.
7. 014는 local diagnostics, redaction, diagnostics bundle, trace/log evidence model을 baseline으로 인정한다. Durable trace/log store와 event replay 기반 inspection은 없다.
8. 015는 runtime ownership marker, heartbeat, lifecycle commands, update marker, no-op schema compatibility gate를 current lifecycle baseline으로 인정한다. Stored-data transform migration framework는 없다.

이 baseline은 새 작업의 출발점이다. 029 closure 전까지 durable queue, durable replay, stored-data migration, exactly-once delivery를 구현됐다고 말하면 안 된다.

## owned open scope

029가 소유하는 열린 범위는 다음이다.

1. Append-only event record. 최소 필드는 `event_id`, `sequence`, `session_id`, optional `turn_id`, optional `causation_id`, optional `correlation_id`, `kind`, `payload`, `recorded_at`, schema version이다.
2. Checkpoint plus tail replay. Checkpoint는 포함 sequence를 명시하고, 이후 event tail을 replay해 stable state를 복원할 수 있어야 한다.
3. Corruption recovery. Checkpoint 손상, incomplete event tail, malformed record, sequence gap, checksum mismatch를 감지하고 사용자 visible recovery 상태로 남긴다.
4. Durable queue, scheduler, retry, cancellation. Process-local bus와 active task registry를 대체하거나 보강할 durable pending work record와 wake/retry/cancel record를 정의한다.
5. Channel restart semantics. Telegram, Discord, Slack, Email, WhatsApp, WebSocket의 cursor, delivery hint, pending outbound, inbound dedupe hint를 restart 뒤 어떻게 해석할지 정한다.
6. Child recovery. Active child task, finished child result, stale child result, cancelled child task가 restart 뒤 어떻게 inspect되고 정리되는지 정한다.
7. Trace durability. Event, log, trace, diagnostics가 같은 correlation chain을 유지하되 trace가 session truth를 대체하지 않게 한다.
8. Stored-data migration. Config, session metadata, event log, checkpoint, queue, scheduler, trace, diagnostics artifact schema를 버전별로 migration한다.
9. Owner lease, heartbeat, safe shutdown, gateway supervision. Long-lived runtime owner가 누구인지, stale owner를 어떻게 판단하는지, shutdown과 restart가 durable state를 어떻게 남기는지 정한다.
10. Local gateway supervision. CLI, local API, channel runtime, WebSocket gateway가 같은 runtime root를 어떻게 공유하고, 하나의 owner 또는 supervised child로 어떻게 동작하는지 정한다.

## invariants

1. Durable event는 오케스트레이터가 확정한 사실이어야 하며, executor의 raw output이나 transport hint를 그대로 truth로 승격하면 안 된다.
2. Checkpoint는 replay 최적화일 뿐이다. Checkpoint가 손상되면 event tail 또는 이전 checkpoint로 복구 판단을 해야 한다.
3. Queue와 scheduler는 policy owner가 아니다. Wake, retry, cancel 대상을 보존하지만 채택 여부는 오케스트레이터가 판단한다.
4. Process restart 뒤 자동 성공 추정은 금지된다. 열린 턴과 pending effect는 completed로 둔갑하면 안 된다.
5. Channel cursor와 delivery status는 session truth가 아니다. Outbound final answer와 session event는 오케스트레이터 경계에서만 확정된다.
6. Child recovery는 stale result를 부모 session content로 persistence하지 않는 기존 invariant를 유지해야 한다.
7. Trace와 diagnostics는 replay 입력을 보강할 수 있지만 replay truth를 대체하면 안 된다.
8. Migration은 mutation 전에 compatibility와 plan을 기록해야 하며, partial migration 상태에서는 normal running을 막아야 한다.
9. Owner lease와 heartbeat는 local runtime root 보호 장치다. 사용자 조직이나 fleet owner 모델로 확장하지 않는다.
10. Exactly-once delivery는 검증된 record protocol과 failure tests가 없는 한 주장하지 않는다.

## Must Have

1. Append-only event record schema와 monotonic sequence rule.
2. Checkpoint format, included sequence, tail replay algorithm, replay stop condition.
3. Corruption detection과 fallback decision. 예: block start, inspect only, recover marker, discard incomplete tail with evidence.
4. Durable queue record. 최소한 command kind, payload reference, session key, dedupe hint, attempt, next wake time, cancellation state를 설명해야 한다.
5. Durable scheduler와 retry/backoff state. Retry decision 자체가 아니라 다음 wake와 attempt state를 보존한다.
6. Durable cancellation record. Stop/restart/cancel request가 process memory 없이도 관찰되어야 한다.
7. Channel restart semantics. Cursor, inbound pending, outbound pending, delivery hint, duplicate hint를 channel별로 설명해야 한다.
8. Child task recovery record. Active, running, completed, failed, timed out, cancelled, stale state를 restart 뒤 inspect할 수 있어야 한다.
9. Trace durability. `session_id`, `turn_id`, `effect_id`, `child_task_id`, `service_correlation_id`가 event, trace, diagnostics 사이에서 이어져야 한다.
10. Stored-data migration runner. Plan, start marker, per-family migration result, partial marker, completion marker, rollback or inspect-only fallback을 남겨야 한다.
11. Owner lease와 heartbeat. Active owner, stale owner, safe takeover, safe shutdown, stop request, restart request를 구분해야 한다.
12. Gateway supervision. Local API, WebSocket, channel workers가 foreground owner와 같은 lifecycle contract를 따르거나 supervised child로 보고되어야 한다.

## Must Not Have

1. Distributed queue 또는 multi-node worker queue.
2. Multi-user concurrency control, shared task board, organization inbox.
3. Fleet management, SaaS updater, hosted control plane.
4. Exactly-once delivery claim. 단, record protocol과 crash matrix로 증명된 특정 local invariant는 제한적으로 말할 수 있다.
5. Event log 없이 trace나 diagnostics만으로 replay truth를 만드는 구조.
6. Checkpoint 하나만 믿고 event tail을 버리는 구조.
7. Corruption을 조용히 무시하고 normal running으로 진입하는 구조.
8. Partial migration 상태에서 writable runtime을 여는 구조.
9. Channel metadata hint를 session truth로 승격하는 구조.
10. Stale owner marker를 사용자 evidence 없이 자동 삭제하는 구조.
11. Child result를 restart 뒤 correlation 없이 부모 session에 주입하는 구조.
12. 운영자 조직이나 관리자 승인 workflow를 기본 사용자 흐름으로 가정하는 문서.

## acceptance criteria

029는 아래 조건을 모두 만족할 때 닫을 수 있다.

1. Append-only event store가 Rust 타입과 저장 format으로 구현되어 있고 sequence invariant 테스트가 있다.
2. Checkpoint plus tail replay가 정상 case, checkpoint 손상, tail 손상, incomplete record, sequence gap을 테스트한다.
3. Recovery command 또는 runtime start admission이 corruption, partial migration, stale owner, pending cancellation을 구분한다.
4. Durable queue와 scheduler가 restart 뒤 pending work, retry wake, cancellation request를 복원한다.
5. Channel runtime이 restart 뒤 cursor와 pending outbound/inbound state를 보수적으로 다루고, duplicate hint를 session truth로 과장하지 않는다.
6. Subagent active child와 finished child result가 restart 뒤 inspect 가능하며, stale 또는 mismatch result가 session content로 persist되지 않는다.
7. Trace/log/diagnostics durable record가 event sequence와 correlation id를 연결하고, redaction rule을 통과한다.
8. Stored-data migration runner가 dry plan, start, partial, complete, inspect-only blocked path를 검증한다.
9. Owner lease와 heartbeat가 active owner conflict, stale owner recovery, safe shutdown, stop request, restart request를 구분한다.
10. Local API, WebSocket, channel gateway가 runtime owner 또는 supervisor lifecycle 아래에서 start, stop, restart, diagnostics에 반영된다.
11. Documentation과 CLI inspect output이 current baseline과 029 구현 범위를 분리한다.
12. Exactly-once wording이 없거나, 제한된 invariant마다 근거 테스트와 failure model을 함께 둔다.

## handoff table back to source specs

| Source spec | 029가 인수하는 열린 작업 | 029에서의 closure 방향 |
| --- | --- | --- |
| 001 | Inspectable open-turn recovery record, checkpoint schema, recovery evidence, queue/scheduler effect boundary | 별도 durable `TurnState` 전면 도입을 요구하지 않고 durable event, checkpoint, replay, recovery admission으로 열린 턴을 설명한다 |
| 002 | Formal event record, replay, checkpoint durability, scheduler/mailbox durability | Command/effect 용어를 보존하며 append-only event와 durable work record를 추가한다 |
| 005 | Per-turn skill registry snapshot, replay/effect provenance snapshot | Skill descriptor와 body hash를 turn snapshot 또는 event payload reference로 보존한다 |
| 006 | Append-only event log, sequence, checkpoint plus tail replay, corruption fallback, incomplete tail discard | Session JSONL baseline을 과장하지 않고 새 durable store와 recovery protocol을 구현한다 |
| 011 | Process restart 이후 child task recovery, child timeout/retry/cancel durability | Child task record와 replay/recovery decision으로 active child를 inspect하고 정리한다 |
| 012 | Durable queue, scheduler, wake envelope, cancellation persistence, restart replay, owner lease, safe shutdown | Process-local service를 durable work and supervisor contract로 보강한다 |
| 014 | Durable trace/log store, event replay inspection, heartbeat and recovery evidence | Trace는 truth 보조로 두고 event sequence와 redacted diagnostics를 durable하게 연결한다 |
| 015 | Stored-data transform migration, compatibility gate, partial migration recovery, owner heartbeat lifecycle | Migration runner와 owner lease/safe shutdown/gateway supervision을 구현한다 |

## implementation evidence required for closure

Closure를 주장하려면 최소한 아래 증거가 있어야 한다.

1. Event store schema와 writer, reader, sequence validation code 위치.
2. Checkpoint schema, included sequence, replay implementation, replay diagnostics code 위치.
3. Corruption recovery 테스트. Checkpoint corruption, tail truncation, malformed event, sequence gap, checksum mismatch가 포함되어야 한다.
4. Durable queue, scheduler, retry, cancellation 테스트. Restart 뒤 pending work와 cancel marker가 살아 있어야 한다.
5. Channel restart 테스트. Telegram, Discord, Slack, Email, WhatsApp, WebSocket 중 구현 범위에 포함된 channel의 cursor, pending outbound, duplicate hint 처리 증거가 있어야 한다.
6. Child recovery 테스트. Active child, completed child, failed child, cancelled child, stale result, duplicate result를 restart 뒤 검증해야 한다.
7. Trace durability와 redaction 테스트. Correlation id chain이 유지되고 secret 원문이 남지 않아야 한다.
8. Stored-data migration 테스트. Plan, no-op, real transform, partial interruption, resume or blocked recovery, inspect-only path가 포함되어야 한다.
9. Owner lease와 heartbeat 테스트. Active owner conflict, stale owner recovery, safe shutdown, stop request, restart request, gateway supervised child 상태를 검증해야 한다.
10. CLI 또는 local API manual evidence. `runtime inspect`, `runtime recover`, channel status, session diagnostics가 durable recovery state를 보여야 한다.
11. Documentation evidence. Old specs의 closure 범위와 029 closure 범위를 구분하는 handoff 또는 release note가 있어야 한다.
12. `cargo fmt --manifest-path crates/Cargo.toml --all -- --check`, `cargo clippy --manifest-path crates/Cargo.toml --workspace --all-targets -- -D warnings`, `cargo test --manifest-path crates/Cargo.toml --workspace` 통과 기록.
