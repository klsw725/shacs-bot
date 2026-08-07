# 029. durable runtime recovery and data migration 아키텍처 명세

Status: Complete (Scoped)

Origin specs: 001, 002, 005, 006, 011, 012, 014, 015

Implementation PRDs, in required order:

1. [`PRD 000: durable event store and schema registry`](prds/000-durable-event-store-and-schema-registry.md)
2. [`PRD 001: checkpoint tail replay and corruption admission`](prds/001-checkpoint-tail-replay-and-corruption-admission.md)
3. [`PRD 002: durable work queue scheduler retry and cancellation`](prds/002-durable-work-queue-scheduler-retry-and-cancellation.md)
4. [`PRD 003: channel restart state and conservative delivery`](prds/003-channel-restart-state-and-conservative-delivery.md)
5. [`PRD 004: durable child task recovery`](prds/004-durable-child-task-recovery.md)
6. [`PRD 005: durable trace log and diagnostics correlation`](prds/005-durable-trace-log-and-diagnostics-correlation.md)
7. [`PRD 006: stored-data migration runner`](prds/006-stored-data-migration-runner.md)
8. [`PRD 007: runtime owner lease gateway supervision and closure`](prds/007-runtime-owner-lease-gateway-supervision-and-closure.md)
9. [`PRD 008: sequential implementation plan`](prds/008-sequential-implementation-plan.md)

Cross-owner boundaries:

1. 029 owns durable runtime event/checkpoint/work/channel/child/diagnostics schema migration orchestration and writable-start admission. [031](../031-configuration-runtime-layout-and-execution-snapshots/SPEC.md) owns future config/profile schema transforms, formal runtime directory layout, and physical marker locations. 029 closes against the current config compatibility and runtime path-helper boundary; 031 can later extend that boundary without reopening 029.
2. 029 owns its closed redaction-before-persistence/export boundary. [030](../030-trusted-agent-runtime-and-operational-controls/SPEC.md) owns trusted-runtime data disclosure and explicitly does not provide a unified redaction taxonomy or typed secret reference model; 029 does not weaken or reopen its existing durable projection rules.
3. 029 owns owner lease/heartbeat/supervision state transitions. 031 owns where owner markers live and which runtime directories each process may mutate.

## 목적

이 문서는 기존 001, 002, 005, 006, 011, 012, 014, 015에서 current architecture closure 밖으로 남긴 durable runtime, recovery, stored-data migration 계약을 새 owner 범위로 모은다.

목표는 self-hosted, personal-use 런타임이 process restart, crash, interrupted upgrade, channel reconnect, child task interruption, partial migration 뒤에도 사용자가 inspect하고 복구할 수 있는 durable 기준을 세우는 것이다. Wave 1-8 baseline은 current Rust implementation 범위에서 닫혔다.

029는 local single-user runtime을 위한 durability 계약이다. 분산 큐, multi-user concurrency, fleet 운영, SaaS control plane은 범위가 아니다. Exactly-once delivery, 자동 reexec/process-manager, runtime worker restart/backoff 보장은 주장하지 않는다.

## 현재 구현 baseline

현재 구현은 다음 범위까지 인정한다.

1. 001은 `AgentLoop`, `AgentRunner`, `SessionManager`, `SessionTurnLock`, `runtime_checkpoint`, `pending_user_turn` marker를 current session kernel mapping으로 인정한다. 별도 durable `TurnState`와 formal phase enum은 없다.
2. 002의 command, event, effect 경계 위에 formal append-only event record와 schema registry를 구현했다. Event record는 replay나 exactly-once delivery를 의미하지 않는다.
3. 005는 skill registry discovery, descriptor, body hash, context injection을 current mapping으로 인정한다. Per-turn registry snapshot과 replay provenance snapshot은 없다.
4. 006의 session별 JSONL persistence와 별도로 append-only event log, checkpoint plus deterministic tail replay, corruption fallback과 writable-start admission을 구현했다. Checkpoint는 event truth를 대체하지 않는다.
5. 011의 subagent identity, lifecycle, stale discard, bounded parallelism, cancellation cleanup 위에 durable child lifecycle과 artifact-backed run/result ref, child-run work, spawned/running restart distinction, accepted parent reentry repair를 구현했다. Stale/late/duplicate result는 parent session truth를 바꾸지 않는다.
6. 012의 process-local bus와 session lock 앞에 external inbound용 durable work queue, scheduler wake/retry/cancellation record, restart replay와 dispatcher adapter를 구현했다. Process-local follow-up queue와 channel runtime metadata hint는 durable work/session/delivery truth가 아니다. Wave 8 기준 owner lease와 bounded safe shutdown supervision evidence가 추가됐지만 runtime worker restart/backoff 보장은 아니다.
7. 014는 local diagnostics, redaction, diagnostics bundle, trace/log evidence model을 baseline으로 인정한다. Wave 6 기준 formal durable diagnostics evidence store는 event sequence에 correlation되지만 replay truth나 writable admission을 대체하지 않는다. Wave 8 기준 runtime inspect/recover, channels status, session diagnostics, local API diagnostics가 같은 redacted supervision projection을 소비한다. 이 durable redaction boundary는 029의 닫힌 계약이며 030 raw-data disclosure가 약화시키지 않는다.
8. 015는 runtime ownership marker, heartbeat, lifecycle commands, update marker, no-op schema compatibility gate를 current lifecycle baseline으로 인정한다. Wave 7 기준 stored-data transform migration framework는 명시적 `runtime migrate` surface와 partial/admission gate까지 구현됐다. Wave 8 기준 strict v1 local owner lease, owner generation, process evidence, acquired/renewed/expires time, lifecycle, stale/live-expired admission, generation-linked stop/restart request, owner-loss fence, v1 supervision-state가 구현됐다. Physical marker path/layout ownership은 031이 유지한다.

이 baseline은 Wave 1-8까지의 현재 구현이다. Durable event/replay/work/channel/child/diagnostics/migration/owner supervision은 위에 명시한 scoped boundary에서만 닫혔다. Exactly-once delivery, 자동 reexec/process-manager, fleet/admin 운영, runtime worker restart/backoff는 구현 범위로 광고하면 안 된다.

## owned scoped closure

029가 scoped closure로 닫은 범위는 다음이다.

1. Append-only event record. 최소 필드는 `event_id`, `sequence`, `session_id`, optional `turn_id`, optional `causation_id`, optional `correlation_id`, `kind`, `payload`, `recorded_at`, schema version이다.
2. Checkpoint plus tail replay. Checkpoint는 포함 sequence를 명시하고, 이후 event tail을 replay해 stable state를 복원할 수 있어야 한다.
3. Corruption recovery. Checkpoint 손상, incomplete event tail, malformed record, sequence gap, checksum mismatch를 감지하고 사용자 visible recovery 상태로 남긴다.
4. Durable queue, scheduler, retry, cancellation. Process-local bus와 active task registry를 대체하거나 보강할 durable pending work record와 wake/retry/cancel record를 정의한다.
5. Channel restart semantics. Telegram, Discord, Slack, Email, WhatsApp, WebSocket의 cursor, delivery hint, pending outbound, inbound dedupe hint를 restart 뒤 어떻게 해석할지 정한다.
6. Child recovery. Active child task, finished child result, stale child result, cancelled child task가 restart 뒤 어떻게 inspect되고 정리되는지 정한다.
7. Trace durability. Event, log, trace, diagnostics가 같은 correlation chain을 유지하되 trace가 session truth를 대체하지 않게 한다.
8. Stored-data migration. Session metadata, event log, checkpoint, queue, scheduler, channel, child, trace, diagnostics artifact 같은 durable runtime family를 버전별로 migration한다. Config/profile은 current compatibility 결과만 admission에서 소비하며 실제 transform은 031이 소유한다. 이 closure에는 tenth migration family가 없다.
9. Owner lease, heartbeat, safe shutdown, gateway supervision. Long-lived runtime owner가 누구인지, stale owner를 어떻게 판단하는지, shutdown과 restart가 durable state를 어떻게 남기는지 정한다. Stale owner는 evidence-first `runtime recover`로 정리하고, live-expired suspect owner는 먼저 process stop/kill evidence가 필요하다.
10. Local gateway supervision. CLI, local API, channel runtime, WebSocket gateway가 같은 runtime root를 어떻게 공유하고, 하나의 owner 또는 supervised child로 어떻게 동작하는지 정한다. Restart는 safe-stop intent이며 자동 reexec가 아니다.

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

029 scoped closure는 아래 조건을 current implementation evidence로 만족한다.

1. Append-only event store와 sequence: `crates/shacs-session/src/durable_event.rs`, `DurableEventStore`, `DurableEventRecord`, `crates/shacs-session/tests/durable_event_store.rs`.
2. Checkpoint, tail replay, corruption: `crates/shacs-session/src/durable_replay.rs`, `DurableCheckpointStore`, `evaluate_runtime_durable_recovery`, `crates/shacs-session/tests/durable_replay.rs`.
3. Recovery admission: `runtime_recover`, `guard_runtime_non_update_admission`, `guard_runtime_migration_admission`, `guard_runtime_ownership_acquire_admission` in `crates/shacs-cli/src/lib.rs`.
4. Durable queue/scheduler/cancellation: `crates/shacs-core/src/runtime/durable_dispatch.rs`, `DurableWorkDispatcher`, `evaluate_runtime_durable_work`, `crates/shacs-core/tests/durable_dispatch.rs`.
5. Channel restart semantics: `inspect_channel_restart_states`, `channel_restart_state_from_metadata`, `format_channel_restart_state_line`, channel worker metadata restart envelope tests in `crates/shacs-cli/src/lib.rs` and channel tests.
6. Child task recovery: `crates/shacs-core/src/runtime/subagent.rs`, `crates/shacs-core/tests/durable_child.rs`, including `child_run_work_is_leased_before_running_and_terminal_after_result`.
7. Durable trace/log/diagnostics: `crates/shacs-session/src/durable_trace.rs`, durable diagnostics inspection and redacted bundle projection in `crates/shacs-cli/src/lib.rs`.
8. Stored-data migration: `crates/shacs-session/src/durable_migration.rs`, `runtime_migrate`, `format_runtime_migrate`, `guard_runtime_migration_admission`.
9. Owner lease/heartbeat/shutdown: `RuntimeOwnershipMarker`, `RuntimeOwnerProcessEvidence`, `RuntimeOwnershipLease`, `RuntimeOwnerFence`, `RuntimeShutdownReport`, `RuntimeStopRequestMarker` in `crates/shacs-cli/src/lib.rs`. Focused PRD007 tests include `prd007_runtime_owner_marker_is_strict_v1_lease`, `prd007_stale_start_blocks_and_retains_marker`, `prd007_recover_records_owner_evidence_before_delete_and_blocks_live_expired`, `runtime_stop_and_restart_write_request_for_active_owner`, `prd007_owner_lost_shutdown_skips_checkpoint_and_keeps_marker`, `prd007_owner_lost_mismatched_generation_does_not_overwrite_supervision`, `prd007_owner_lost_processor_does_not_requeue_or_terminal_work`, `prd007_runtime_wait_observes_owner_fence_loss`, `prd007_runtime_wait_reports_processor_unexpected_exit`, `prd007_shutdown_timeout_report_is_bounded_and_unknown`, `runtime_stale_ownership_cleanup_rechecks_active_marker_before_removing`, `runtime_ownership_preserve_keeps_marker_for_failed_shutdown_recovery`, `runtime_recover_clears_stale_ownership_marker`, `prd007_runtime_final_shutdown_state_retains_owner_after_marker_cleanup`, `runtime_stop_reports_no_active_or_stale_owner`.
10. Gateway supervision and shared projections: `RuntimeSupervisionState`, `RuntimeSupervisionOwner`, `RuntimeSupervisionComponent`, `runtime_supervision_from_owner`, `runtime_components_for_mode`, `runtime_supervisor_projection`, `format_runtime_inspect`, `format_runtime_recover`, `session_diagnostics`, local API diagnostics projection in `crates/shacs-cli/src/lib.rs`. Focused tests include `prd007_supervision_records_api_only_and_channel_components`, `prd007_component_report_uses_names_without_raw_secret`, `prd007_recover_and_session_diagnostics_project_redacted_supervision`.
11. Documentation and CLI inspect separation: `README.md`, `docs/USAGE.md`, this SPEC, PRD007, PRD008 now separate scoped implementation evidence from final full gate/manual review evidence.
12. Exactly-once wording audit: docs and projections keep durable queue/channel metadata as hints, not delivery/session truth. 029 does not claim exactly-once delivery, auto reexec/process-manager, fleet/admin operation, or runtime worker restart/backoff.

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

Scoped closure evidence는 아래 파일과 symbols에 연결한다.

1. Event store schema와 writer, reader, sequence validation code 위치.
2. Checkpoint schema, included sequence, replay implementation, replay diagnostics code 위치.
3. Corruption recovery 테스트. Checkpoint corruption, tail truncation, malformed event, sequence gap, checksum mismatch가 포함되어야 한다.
4. Durable queue, scheduler, retry, cancellation 테스트. Restart 뒤 pending work와 cancel marker가 살아 있어야 한다.
5. Channel restart 테스트. Telegram, Discord, Slack, Email, WhatsApp, WebSocket 중 구현 범위에 포함된 channel의 cursor, pending outbound, duplicate hint 처리 증거가 있어야 한다.
6. Child recovery 테스트. Active child, completed child, failed child, cancelled child, stale result, duplicate result를 restart 뒤 검증해야 한다.
7. Trace durability와 redaction 테스트. Correlation id chain이 유지되고 secret 원문이 남지 않아야 한다.
8. Stored-data migration 테스트. Plan, no-op, real transform, partial interruption, resume or blocked recovery, inspect-only path가 포함되어야 한다.
9. Owner lease와 heartbeat 테스트: `crates/shacs-cli/src/lib.rs`의 PRD007 focused tests가 strict v1 lease, active/stale start block, evidence-first recover, live-expired suspect block, durable event와 결합된 generation-linked stop/restart request, crash-safe mutation lock, bounded nofollow marker read, owner-loss fence, failed/timeout shutdown outcomes, turn panic recovery, supervision projection을 검증한다. 최종 focused PRD007 test set은 22개가 통과했다.
10. CLI 또는 local API projection evidence: `format_runtime_inspect`, `format_runtime_recover`, `format_channels_status`, `format_session_diagnostics`, local API `/v1/diagnostics` projection이 durable recovery state와 redacted supervision을 공유한다.
11. Documentation evidence: `README.md`, `docs/USAGE.md`, `docs/specs/029-durable-runtime-recovery-and-data-migration/SPEC.md`, PRD007, PRD008이 old baseline, Wave 1-8 durable redaction closure, 030 trusted-runtime disclosure, 031 credential-source 및 physical path/layout ownership을 분리한다.
12. Full gate evidence: 최종 `cargo fmt --manifest-path crates/Cargo.toml --all -- --check`, `cargo clippy --manifest-path crates/Cargo.toml --workspace --all-targets -- -D warnings`, `cargo test --manifest-path crates/Cargo.toml --workspace`, `cargo build --manifest-path crates/Cargo.toml -p shacs-cli`가 통과했다. 격리된 임시 config/workspace의 실제 CLI QA에서 active/idle owner, second start block, stop/restart safe shutdown, no auto-reexec, startup failure, stale recover, named component projection과 redaction을 확인했다.
