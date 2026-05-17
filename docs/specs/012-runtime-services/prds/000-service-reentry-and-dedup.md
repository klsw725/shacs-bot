# PRD 000. service reentry and dedup

## 목표

이 문서는 `docs/specs/012-runtime-services/SPEC.md`의 하위 실행 문서다. 기존 문서는 queue, scheduler, mailbox, hooks, background worker의 durable service command envelope와 dedupe/retry/wake 계약이 이미 필요한 완료 조건처럼 보였다. 현재 문서는 그 표현을 낮추고, 2026-05-17 기준 Spec 012를 process-local runtime 경계의 current architecture mapping으로 닫는다.

이번 PRD의 목표는 다음과 같다.

1. 현재 구현된 reentry 관련 runtime 경계를 정확히 적는다.
2. process-local queue와 metadata JSON을 durable queue나 exactly-once ledger로 오해하지 않게 한다.
3. future durable service envelope, dedupe key, retry state, wake contract를 남은 작업으로 둔다.

## SPEC 입력

- 주관 spec: `docs/specs/012-runtime-services/SPEC.md`
- 선행 기준:
  - `docs/specs/001-session-kernel/SPEC.md`
  - `docs/specs/002-command-event-effect/SPEC.md`
  - `docs/specs/006-session-store/SPEC.md`
  - `docs/specs/007-main-orchestrator-policy/SPEC.md`
  - `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`
  - `docs/specs/011-subagent-runtime/SPEC.md`

## Dependency Cut

- 002는 command/effect/event 경계를 제공한다. 현재 runtime service mapping은 이 경계를 우회하면 안 된다.
- 006은 replay 가능한 session truth를 제공한다. runtime metadata JSON은 이 truth를 대체하지 않는다.
- 007은 active turn과 follow-up을 오케스트레이터 경계 안에서 판단하게 한다.
- 010은 외부 channel, email, network, secret 입력을 self-hosted 사용자 안전 범위 안에 묶는다.
- 011은 subagent와 background-like task가 parent runtime 아래로 회수되어야 함을 제공한다.

## 범위

- `MessageBus`의 process-local inbound/outbound FIFO, bounded queue, blocking consume wake, matching drain 의미 정리.
- `SessionTurnLock`, `CancellationToken`, `LoopTaskRegistry`의 process-local same-session guard, cancellation, status tracking 의미 정리.
- command router priority와 normal turn boundary 정리.
- external inbound same-session follow-up queue 정리.
- runtime metadata JSON을 cursor, diagnostic, delivery hint로 정리.
- 현재 구현 증거와 남은 future durable runtime 작업 분리.

## 범위 제외

- distributed task queue.
- multi-node leader election.
- organization inbox.
- operator dashboard.
- multi-tenant webhook fan-out.
- public webhook hosting obligation.
- multi-user task distribution.
- vendor-specific queue/cron product dependency.
- Slack, Discord, Telegram, Email, WhatsApp 바깥의 추가 channel.

## 현재 구현 상태

### 현재 아키텍처 매핑

- `MessageBus`는 process-local inbound/outbound queue다. FIFO, bounded capacity, blocking consumer wake, matching drain, queue size helper를 제공한다.
- `SessionTurnLock`은 같은 session의 active turn 중복을 막는다. duplicate active session은 retryable busy error로 보고될 수 있다.
- `LoopTaskRegistry`와 `CancellationToken`은 active loop task의 status와 cancellation을 process-local로 추적한다.
- command router는 priority, exact, prefix command를 분리한다. `/status`, `/stop`, `/restart` 계열 priority command는 active turn lock 앞에서 처리될 수 있고, exact/prefix command는 normal turn boundary를 따른다.
- external inbound same-session follow-up은 `ExternalSessionTurnCoordinator`의 process-local pending queue를 통해 직렬화된다.
- runner는 mid-turn injection을 iteration 사이에서 drain하고 cycle을 cap한다.
- progress projection은 persistence truth가 아니다. `delta`, `stream_end`, preview update는 진행 표시이며 final answer가 authoritative output이다.

### partial 또는 formal-looking but incomplete

- service command envelope라는 방향은 남아 있지만, 모든 service가 typed command envelope, typed dedupe key, retry state를 공유하는 상태는 아니다.
- wake/resume이라는 모델은 유용하지만, durable scheduler와 formal wake command envelope는 없다.
- metadata JSON은 Telegram offset, Discord REST last id/Gateway resume state, Email UIDVALIDITY/seen UID, outbound delivery status를 best-effort로 보존한다. 이것은 transactional store가 아니다.
- outbound retry는 `ChannelManager`와 adapter dispatch 수준의 attempt 관찰이다. durable retry/backoff scheduler가 아니다.
- follow-up queue는 process-local이다. durable pending-message queue, cancellation persistence, restart replay가 아니다.

### future gap

- durable queue.
- durable scheduler와 formal wake command envelope.
- 모든 runtime service에 공통으로 적용되는 formal service command envelope.
- typed dedupe key와 retry state persistence.
- exactly-once delivery.
- durable pending-message queue.
- cancellation persistence와 restart replay.
- transactional service metadata store.
- durable retry/backoff scheduler.
- formal `RuntimeSupervisor`, owner lease/heartbeat, stale owner recovery, safe shutdown.
- multi-process TUI/API/channel owner coordination.

## waves / next work

### Wave 1. Current mapping 정리

- process-local bus, turn lock, active task registry, command router priority, follow-up queue를 current architecture로 고정한다.
- metadata JSON을 cursor, diagnostic, delivery hint로만 설명한다.
- durable queue, exactly-once, transactional metadata 주장을 제거한다.

### Wave 2. Reentry vocabulary 축소

- service reentry를 모든 service가 공유하는 완료된 envelope가 아니라, 현재 channel/runtime boundary가 따르는 재진입 방향으로 표현한다.
- wake/resume은 future formal model로 남긴다.
- retry는 transport/outbound dispatch attempt와 session policy retry를 구분한다.

### Wave 3. 검증 증거 연결

- bus FIFO와 bounded queue 테스트를 current mapping evidence로 연결한다.
- loop/control 테스트를 same-session lock, cancellation, priority command evidence로 연결한다.
- runner/progress 테스트를 progress projection과 mid-turn injection evidence로 연결한다.

### Wave 4. Formal durable runtime 결정 보류

- durable queue, scheduler, wake envelope, typed dedupe/retry state, restart replay를 future work로 유지한다.
- Spec 012는 current architecture 기준으로 닫고, formal durable runtime 기준 종료는 별도 future work로 남긴다.

## Verification Evidence

- Bus: `bus_preserves_fifo_and_sizes_for_inbound_and_outbound`, `bounded_bus_reports_capacity_for_both_queues`, `cloned_bus_handles_share_queues_and_blocking_consumers_wake`, `drain_matching_limits_matches_and_preserves_retained_fifo`.
- Loop/control: `loop_priority_new_cancels_registered_task_before_clearing_session`, `loop_priority_status_reports_registered_async_task`, `session_turn_lock_rejects_duplicate_active_session`, `loop_observes_registered_cancellation_token_before_provider_call`, `adapter_reports_duplicate_session_turn_as_retryable_busy_error`.
- Runner/progress: `stream_coalescer_batches_text_deltas_without_session_persistence`, `runtime_runner_drains_mid_turn_injections_between_iterations`, `runtime_runner_caps_mid_turn_injection_cycles`.
- API/local surface는 `crates/shacs-api/src/lib.rs`의 health, models, chat, WebSocket, streaming tests로 확인되는 local runtime boundary를 증거로 삼는다.

이 증거는 current process-local architecture를 설명한다. durable queue, durable scheduler, exactly-once delivery, transactional metadata store의 완료 증거로 쓰면 안 된다.

## Open Risks

- process-local queue와 lock을 durable runtime guarantee처럼 읽으면 restart recovery 기대가 과해진다.
- metadata JSON을 dedupe ledger처럼 쓰면 중복 전달이나 cursor loss를 잘못 설명하게 된다.
- priority command boundary를 service policy 전체로 확장하면 built-in command router의 현재 의미를 넘어선다.
- future durable runtime을 도입할 때 현재 local simplicity를 잃을 수 있다. self-hosted/personal-use 기본값을 유지해야 한다.

## 종료 기준

- 문서는 process-local queue, lock, active task tracking, command priority, follow-up queue, metadata JSON hint를 현재 구현으로 설명한다.
- 문서는 durable queue, scheduler, wake envelope, exactly-once, transactional metadata, restart replay를 future gap으로 설명한다.
- 문서는 Spec 012를 current architecture 기준으로 닫는다.
- 문서는 Spec 012가 formal durable runtime으로 완료됐다고 주장하지 않는다.
- future durable runtime을 진행하려면 별도 PRD에서 envelope, dedupe, retry, wake, recovery 기준과 테스트를 다시 잡는다.
