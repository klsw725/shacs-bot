# PRD 008: sequential implementation plan

## 목표

Spec 029를 durable truth substrate부터 lifecycle closure까지 단방향으로 구현한다. 각 wave는 앞 wave의 완료 게이트를 통과한 뒤에만 시작한다. 병렬 구현으로 schema와 recovery 의미가 분기되지 않게 하는 것이 목적이다.

Status: Complete (Scoped). 아래 순서는 권장 순서가 아니라 implementation dependency contract다.

## 공통 Dependency Cut

1. 028의 execution identity/outcome은 소비하지만 029가 다시 정의하지 않는다.
2. Session JSONL, process-local bus, runtime metadata hint는 baseline이며 durable truth로 승격하지 않는다.
3. Event store 없이 replay, queue, migration, supervisor 완료를 주장하지 않는다.
4. Distributed queue, fleet, multi-user admin workflow, general exactly-once delivery는 모든 wave에서 비범위다.
5. Spec 030은 trusted-runtime data disclosure를, Spec 035는 config/profile auth source와 formal runtime layout/marker location을 소유한다. 029는 현재 shared redaction, config compatibility, runtime path-helper boundary를 소비해 독립적으로 닫았으며 후속 spec은 그 durable projection을 약화시키지 않는다.
6. Spec 030 또는 035의 완료는 Wave 1-8이나 Spec 029 closure의 선행 조건이 아니다.

## 구현 순서

### Wave 1. Durable event truth substrate

소유 PRD: `000-durable-event-store-and-schema-registry.md`

Status: Complete (Scoped). Store/AgentLoop integration, corruption matrix, multi-process sequence, failure injection evidence가 workspace gate와 5-lane review를 통과했다.

작업:

1. Event schema, kind registry, monotonic sequence를 정의한다.
2. Append reader/writer와 framing/checksum을 구현한다.
3. Orchestrator-accepted fact만 event로 기록한다.
4. Sequence/corruption fixture를 만든다.

게이트:

- Incomplete record와 committed record를 구분해야 한다.
- Event store를 replay나 exactly-once delivery로 광고하면 안 된다.

### Wave 2. Checkpoint replay and recovery admission

소유 PRD: `001-checkpoint-tail-replay-and-corruption-admission.md`

Status: Complete (Scoped). Event-prefix-bound checkpoint, deterministic tail replay, corruption admission, CLI inspect/recover/start integration, forged checkpoint regression, workspace gate와 5-lane review가 통과했다.

작업:

1. Checkpoint schema와 included sequence를 구현한다.
2. Deterministic tail replay를 구현한다.
3. Checkpoint/tail corruption matrix를 고정한다.
4. Healthy/recoverable/inspect-only/blocked admission을 만든다.

게이트:

- Replay가 live side effect를 실행하면 안 된다.
- Corruption 상태에서 writable runtime을 열면 안 된다.

### Wave 3. Durable work, scheduler, retry, cancellation

소유 PRD: `002-durable-work-queue-scheduler-retry-and-cancellation.md`

Status: Complete (Scoped). Durable work lifecycle, bounded payload/admission, scheduler retry/exhaustion, cancellation request/outcome, stale lease recovery, external runtime dispatch/shutdown과 priority `/stop` concurrency matrix가 workspace gate와 독립 review를 통과했다.

작업:

1. Durable work item과 state transition을 구현한다.
2. Scheduler wake와 retry attempt를 보존한다.
3. Cancellation/stop/restart request를 durable하게 기록한다.
4. Restart restore와 stale lease를 검증한다.

게이트:

- Queue가 policy/session truth owner가 되면 안 된다.
- Running 중 crash한 work를 success로 추정하면 안 된다.

### Wave 4. Channel restart semantics

소유 PRD: `003-channel-restart-state-and-conservative-delivery.md`

Status: Complete (Scoped). 지원 channel worker metadata의 typed restart envelope, legacy metadata compatibility, pending durable inbound safe refs, conservative delivery statuses(`pending`, `sent_hint`, `failed_hint`, `unknown`, `dedupe_candidate`), `runtime inspect`/`channels status` projection, restart fixture regression이 workspace gate 범위로 구현됐다.

작업:

1. Common restart state와 channel-specific cursor semantics를 정의한다.
2. Pending inbound/outbound를 durable work에 연결한다.
3. Delivery/dedupe hint를 session truth와 분리한다.
4. 지원 channel restart fixture를 만든다.

게이트:

- Unknown delivery를 sent로 추정하면 안 된다.
- Exactly-once delivery를 주장하면 안 된다.

### Wave 5. Child task recovery

소유 PRD: `004-durable-child-task-recovery.md`

Status: Complete (Scoped). Artifact-first child run/result persistence, durable child-run work lifecycle, accepted parent reentry repair, spawned/running recovery distinction, full correlation validation, cancellation/late/stale/duplicate replay, bounded opaque diagnostics가 workspace gate와 5-lane review를 통과했다.

작업:

1. Durable child lifecycle record를 구현한다.
2. Restart 뒤 active/terminal child를 inspect한다.
3. Cancellation, duplicate, late, stale decision을 복원한다.
4. Accepted result만 parent orchestrator reentry로 보낸다.

게이트:

- 기존 four-field correlation을 약화하면 안 된다.
- Stale result를 parent session content로 persist하면 안 된다.

### Wave 6. Durable diagnostics correlation

소유 PRD: `005-durable-trace-log-and-diagnostics-correlation.md`

Status: Complete (Scoped). Formal durable diagnostics evidence store, event-sequence correlation, redaction-before-persist/export, bounded preview/artifact refs, active-recovery-biased retention, corrupt-tail non-authority, and CLI/API/bundle projection parity are implemented for current runtime work/child transitions.

작업:

1. Event sequence와 trace/log/diagnostics를 연결한다.
2. Redaction-before-persist/export를 적용한다.
3. Retention과 artifact reference를 bounded하게 만든다.
4. CLI/API/bundle projection을 맞춘다.

게이트:

- Trace가 event truth를 대체하면 안 된다.
- Raw secret/path/payload가 persisted evidence에 남으면 안 된다.

### Wave 7. Stored-data migration

소유 PRD: `006-stored-data-migration-runner.md`

Status: Complete (Scoped). Dedicated `shacs-session::durable_migration` runner, explicit `runtime migrate` CLI, migration admission block, v0->v1 fixture transforms, interruption/resume matrix, and redacted projection tests are implemented. No silent migration runs on runtime start.

작업:

1. Session/event/checkpoint/queue/scheduler/channel/child/trace/diagnostics artifact durable family의 schema inventory를 만든다. Implemented in `crates/shacs-session/src/durable_migration.rs`.
2. Dry plan과 start/family/partial/complete marker를 구현한다. Implemented as dry-run report plus `runtime/migration-ledger.json` for apply/resume.
3. Interruption/resume/rollback/inspect-only matrix를 검증한다. Covered by per-family before/during/after interruption tests, bounded backup, unknown newer/missing path blockers.
4. Runtime start/update/recover admission에 연결하고 current config compatibility result를 결합한다. `shacs-cli` admission blocks writable runtime and exposes only explicit `runtime migrate --apply/--resume`.

게이트:

- Plan/start marker 이전에 mutation하면 안 된다.
- Partial migration 상태에서 writable runtime을 열면 안 된다.

### Wave 8. Owner lease, supervision, closure

소유 PRD: `007-runtime-owner-lease-gateway-supervision-and-closure.md`

Status: Complete (Scoped). Strict v1 local owner lease, stale/live-expired admission, durable event와 결합된 generation-linked stop/restart request, crash-safe mutation lock, bounded nofollow marker read, owner-loss fence, v1 supervision-state, and shared redacted runtime projections are implemented for the current local lifecycle boundary. Focused PRD007/external tests, full workspace gate, and isolated manual lifecycle QA passed.

작업:

1. Owner lease와 heartbeat/safe takeover를 구현했다. Evidence: `RuntimeOwnershipMarker`, `RuntimeOwnerProcessEvidence`, `RuntimeOwnershipLease`, `classify_runtime_ownership_marker`, `prd007_runtime_owner_marker_is_strict_v1_lease`, `prd007_stale_start_blocks_and_retains_marker`, `prd007_recover_records_owner_evidence_before_delete_and_blocks_live_expired`.
2. API/WebSocket/external processor/channel component를 supervision projection으로 연결했다. Evidence: `RuntimeSupervisionState`, `runtime_components_for_mode`, `runtime_supervisor_projection`, `prd007_supervision_records_api_only_and_channel_components`.
3. Safe shutdown/stop/restart/crash recovery를 검증했다. Evidence: `RuntimeShutdownReport`, `RuntimeStopRequestMarker`, `runtime_stop`, `runtime_restart`, `RuntimeOwnerFence`, `runtime_stop_and_restart_write_request_for_active_owner`, `prd007_owner_lost_shutdown_skips_checkpoint_and_keeps_marker`, `prd007_runtime_wait_observes_owner_fence_loss`, `prd007_runtime_wait_reports_processor_unexpected_exit`.
4. Inspect/recover/status/docs/release evidence를 current scoped boundary로 완성했다. Evidence: `format_runtime_inspect`, `format_runtime_recover`, `session_diagnostics`, `prd007_recover_and_session_diagnostics_project_redacted_supervision`, `README.md`, `docs/USAGE.md`, PRD007, this plan.

게이트:

- Active/stale owner conflict에서 second writable runtime을 열면 안 된다.
- Stale owner recover는 owner lifecycle evidence를 먼저 기록해야 하며, live-expired suspect owner는 stop/kill evidence 없이 정리하면 안 된다.
- Restart는 safe-stop intent만 의미하며 자동 reexec/process-manager 주장을 하면 안 된다.
- Exactly-once, fleet/admin, runtime worker restart/backoff wording을 쓰면 안 된다.
- Spec 029는 focused crash matrix, full workspace gate, 실제 CLI lifecycle QA를 포함한 scoped implementation evidence로 완료 처리한다.

## Acceptance criterion 매핑

| Spec 029 criterion | Owner PRD |
|---|---|
| append-only event store와 sequence | 000 |
| checkpoint, tail replay, corruption | 001 |
| recovery admission | 001, 002, 006, 007 |
| durable queue/scheduler/cancellation | 002 |
| channel restart semantics | 003 |
| child task recovery | 004 |
| durable trace/log/diagnostics | 005 |
| stored-data migration | 006 |
| owner lease/heartbeat/shutdown | 007 |
| gateway supervision | 007 |
| docs/CLI inspect separation | 001, 003, 005, 007 |
| exactly-once wording audit | 000, 002, 003, 007 |

Concrete Wave 8 evidence mapping:

| Requirement | Files/symbols/tests |
|---|---|
| strict v1 owner lease | `crates/shacs-cli/src/lib.rs`: `RuntimeOwnershipMarker`, `RuntimeOwnerProcessEvidence`, `runtime_ownership_marker_value`, `read_runtime_ownership_marker`; test `prd007_runtime_owner_marker_is_strict_v1_lease` |
| active/stale start block | `RuntimeOwnershipLease::acquire`, `acquire_runtime_ownership_marker`, `active_runtime_ownership_error`; test `prd007_stale_start_blocks_and_retains_marker` |
| evidence-first stale recovery | `runtime_recover`, `append_runtime_owner_lifecycle`, `remove_stale_runtime_ownership_marker_locked`; tests `prd007_recover_records_owner_evidence_before_delete_and_blocks_live_expired`, `runtime_recover_clears_stale_ownership_marker` |
| live-expired suspect block | `runtime_recover`, `classify_runtime_ownership_marker`; test `prd007_recover_records_owner_evidence_before_delete_and_blocks_live_expired` |
| generation-linked stop/restart | `runtime_stop`, `runtime_restart`, `append_runtime_control_request`, `RuntimeStopRequestMarker`; test `runtime_stop_and_restart_write_request_for_active_owner` |
| restart safe-stop semantics | `wait_for_ctrl_c_or_runtime_request`, `format_runtime_restart`; docs `README.md`, `docs/USAGE.md` |
| owner-loss fence | `RuntimeOwnerFence`, `RuntimeOwnershipLease::fail_shutdown`, `record_owner_lost_shutdown`; tests `prd007_runtime_wait_observes_owner_fence_loss`, `prd007_owner_lost_shutdown_skips_checkpoint_and_keeps_marker`, `prd007_owner_lost_processor_does_not_requeue_or_terminal_work` |
| supervision state | `RuntimeSupervisionState`, `RuntimeSupervisionShutdown`, `RuntimeShutdownStepReport`, `write_runtime_supervision_state`; tests `prd007_runtime_final_shutdown_state_retains_owner_after_marker_cleanup`, `prd007_supervision_records_api_only_and_channel_components` |
| redacted projections | `runtime_supervisor_projection`, `format_runtime_inspect`, `format_runtime_recover`, `session_diagnostics`, API diagnostics snapshot; tests `prd007_component_report_uses_names_without_raw_secret`, `prd007_recover_and_session_diagnostics_project_redacted_supervision` |

## 전체 완료 기준

1. Wave 1-8이 순서대로 완료되고 각 scoped gate evidence가 존재한다.
2. Restart/crash/corruption/migration failure matrix가 focused 자동 테스트로 고정된다.
3. `runtime inspect`, `runtime recover`, channel status, session diagnostics, API diagnostics는 같은 redacted recovery/supervision projection을 사용한다.
4. Focused PRD007 test set 22개와 external focused test set 51개, workspace fmt/clippy/test와 `shacs-cli` build, isolated manual CLI lifecycle QA가 통과했다.
5. [Spec 029](../SPEC.md)는 구현 evidence를 반영해 `Complete (Scoped)`로 변경한다.
6. Spec 030/035 미완료는 029의 current boundary 검증을 막지 않으며, 후속 owner 구현은 029를 다시 열지 않고 adapter 결과를 확장한다. 030은 raw-content disclosure를, 035는 auth source와 physical path/layout ownership을 소유한다.
