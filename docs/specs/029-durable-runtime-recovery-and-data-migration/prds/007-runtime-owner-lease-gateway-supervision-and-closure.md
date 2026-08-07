# PRD 007: runtime owner lease gateway supervision and closure

## 목표

하나의 local runtime root를 소유하는 active owner, heartbeat, stale recovery, safe shutdown과 CLI/local API/WebSocket/channel worker supervision을 통합하고 Spec 029 closure evidence를 완성한다.

Status: Complete (Scoped). 이 PRD는 local single-user runtime lifecycle만 소유한다. Focused PRD007 test evidence는 통과했지만 최종 full workspace gate/manual review 기록은 별도 closure evidence로 남긴다.

## 범위

1. Durable owner lease와 heartbeat
2. Active owner conflict, stale owner, safe takeover
3. Stop, restart, shutdown, crash 상태 구분
4. Local API, WebSocket, external processor, channel component의 supervised 상태
5. Runtime start/recover admission 통합
6. CLI/API/channel diagnostics와 closure evidence

## 비범위

- fleet management
- multi-user organization owner
- systemd/launchd/Docker 자체를 새로 구현하는 작업
- remote control plane
- exactly-once delivery
- automatic reexec, process-manager, runtime worker restart/backoff

## SPEC 입력

1. 필수 선행 PRD: `006-stored-data-migration-runner.md`
2. Lifecycle baseline: `../../015-packaging-process-lifecycle-and-upgrades/SPEC.md`
3. Durable cancellation/work/channel/trace state는 PRD 002, 003, 005를 소비한다.
4. Current runtime path helper를 소비한다. Formal runtime directory ownership과 physical marker location의 future owner는 `../../031-configuration-runtime-layout-and-execution-snapshots/SPEC.md`이며, 031 구현 완료는 이 PRD의 선행 조건이 아니다.

## Dependency Cut

1. Lease는 local runtime root 보호 장치이며 authorization 조직 모델이 아니다.
2. Heartbeat 만료만으로 stale owner evidence를 삭제하지 않는다.
3. Supervisor는 child process 상태를 관찰하지만 session truth를 직접 만들지 않는다.
4. Safe shutdown은 pending work/cancellation/event flush 결과를 기록한다.
5. 이 PRD는 lease/heartbeat/supervision 상태 전이를 소유한다. Runtime root layout, marker path, directory cleanup admission은 031이 소유한다.

## 구현 요구사항

1. Lease record는 owner generation, process evidence, acquired/renewed/expires time, lifecycle state, schema version을 가지며 031-owned runtime layout helper가 제공한 위치에 저장된다.
2. Active owner conflict와 stale owner marker는 second writable start를 차단한다.
3. Stale takeover는 prior owner evidence, heartbeat age, recovery admission을 확인하고, `runtime recover`가 owner lifecycle event를 먼저 기록한 뒤 marker를 정리한다.
4. PID가 살아 있는데 heartbeat만 만료된 live-expired owner는 suspect로 보고 recover를 차단한다. 사용자는 먼저 stop 요청 또는 process kill로 실행 중 owner를 멈춰야 한다.
5. Stop/restart request는 durable runtime control request와 current owner generation에 연결된다.
6. Restart request는 safe-stop intent이며 자동 reexec나 새 process start가 아니다.
7. Safe shutdown은 queue/work/child/event/checkpoint flush와 supervised component stop 결과를 bounded outcome으로 남긴다.
8. Local API, WebSocket, external processor, channel component는 owner 또는 supervised component로 status에 표시된다.
9. Crash/restart 후 orphaned child와 pending delivery를 자동 성공으로 만들지 않는다.
10. `runtime inspect`, `runtime recover`, channel status, session diagnostics, API diagnostics가 같은 redacted recovery/supervision model을 보여준다.

## 정상 시퀀스

1. Runtime start가 029 durable migration/recovery admission과 current config/path-helper admission을 모두 통과한다.
2. Active conflict가 없으면 owner lease를 획득한다.
3. Supervisor가 configured children을 시작하고 상태를 기록한다.
4. Heartbeat가 lease를 갱신한다.
5. Stop/restart 시 durable request, child stop, flush, terminal owner state를 순서대로 기록한다. Restart 뒤 새 process 시작은 명시적 next start 또는 외부 OS supervisor 책임이다.

## 실패 시퀀스

1. Active owner가 있으면 second start를 차단하고 inspect evidence를 보여준다.
2. Heartbeat가 stale이어도 즉시 marker를 삭제하지 않고 recovery decision과 prior owner evidence를 요구한다.
3. Live-expired owner는 process evidence가 살아 있으면 suspect로 차단한다.
4. Child start/stop 실패는 failed_shutdown supervision state로 남는다.
5. Owner-loss fence는 owner_lost shutdown을 기록하고 marker를 보존한다.
6. Shutdown 중 crash는 다음 start에서 pending shutdown/recovery 상태로 복원된다.
7. Partial migration/corruption 상태에서는 lease 획득 전 inspect-only로 차단한다.

## 검증 관점

1. Active owner conflict, stale heartbeat, evidence-first safe takeover를 검증한다.
2. Live-expired suspect block, normal stop, restart safe-stop intent, forced crash, shutdown interruption을 검증한다.
3. API/WebSocket/external processor/channel component start/stop/failed 상태를 검증한다.
4. Pending queue/channel/child state가 shutdown/restart에서 자동 성공으로 바뀌지 않는지 확인한다.
5. CLI와 local API manual QA로 inspect/recover/status/diagnostics를 실행한다.
6. Exactly-once, auto reexec/process-manager, fleet, admin, runtime worker restart/backoff wording이 문서와 projection에 없는지 확인한다.

## 구현 evidence

1. `crates/shacs-cli/src/lib.rs`: `RuntimeOwnershipMarker`, `RuntimeOwnerProcessEvidence`, `RuntimeOwnershipLease`, `RuntimeOwnerFence`, `RuntimeStopRequestMarker`, `RuntimeShutdownReport`, `RuntimeSupervisionState`, `RuntimeSupervisionOwner`, `RuntimeSupervisionComponent`.
2. `crates/shacs-cli/src/lib.rs`: `runtime_stop`, `runtime_restart`, `runtime_recover`, `classify_runtime_ownership_marker`, `remove_stale_runtime_ownership_marker_locked`, `append_runtime_owner_lifecycle`, `write_runtime_supervision_state`, `runtime_supervisor_projection`, `format_runtime_inspect`, `format_runtime_recover`.
3. Focused PRD007 test evidence: `prd007_runtime_owner_marker_is_strict_v1_lease`, `prd007_stale_start_blocks_and_retains_marker`, `prd007_recover_records_owner_evidence_before_delete_and_blocks_live_expired`, `runtime_stop_and_restart_write_request_for_active_owner`, `prd007_owner_lost_shutdown_skips_checkpoint_and_keeps_marker`, `prd007_owner_lost_mismatched_generation_does_not_overwrite_supervision`, `prd007_owner_lost_processor_does_not_requeue_or_terminal_work`, `prd007_runtime_wait_observes_owner_fence_loss`, `prd007_runtime_wait_reports_processor_unexpected_exit`, `prd007_shutdown_timeout_report_is_bounded_and_unknown`, `runtime_stale_ownership_cleanup_rechecks_active_marker_before_removing`, `runtime_ownership_preserve_keeps_marker_for_failed_shutdown_recovery`, `runtime_recover_clears_stale_ownership_marker`, `prd007_runtime_final_shutdown_state_retains_owner_after_marker_cleanup`, `runtime_stop_reports_no_active_or_stale_owner`.
4. Shared redacted projection evidence: `prd007_supervision_records_api_only_and_channel_components`, `prd007_component_report_uses_names_without_raw_secret`, `prd007_recover_and_session_diagnostics_project_redacted_supervision`, plus API diagnostics projection in `AgentLoopChatCompletionAdapter::diagnostics_snapshot`.
5. Scope evidence: 029 keeps its closed durable redaction boundary, 030 owns trusted-runtime data disclosure, and 031 owns credential source plus physical runtime path/layout. PRD007 consumes the current shared redaction and path-helper boundaries only.

## 완료 기준

- Owner lease와 supervisor lifecycle이 focused crash/shutdown matrix로 검증됐다.
- 모든 029 acceptance criterion의 scoped implementation evidence가 연결됐다.
- 문서와 CLI/API가 baseline과 029 구현 범위를 구분한다.
- Focused PRD007 test set 22개와 external runtime focused test set 51개가 통과했다. Workspace fmt/clippy/test와 `shacs-cli` build가 통과했고, 격리된 실제 runtime CLI QA에서 active conflict, stop/restart, no auto-reexec, startup failure, stale recover, supervision projection을 확인했다.
