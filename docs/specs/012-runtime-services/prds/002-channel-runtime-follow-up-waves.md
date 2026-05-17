# PRD 002. channel runtime follow-up waves

## 목표

이 문서는 `docs/specs/012-runtime-services/SPEC.md`와 `docs/specs/012-runtime-services/prds/001-channel-worker-runtime.md`의 후속 실행 문서다. 기존 문서는 durable metadata store, pending queue, retry/backoff controller, supervisor, broker/router를 다음 필수 구현처럼 제시했다. 현재 문서는 2026-05-17 기준 이미 구현된 channel runtime follow-up 범위를 Spec 012 current architecture 종료 증거로 정리하고, durable runtime 확장은 명시적인 future gap으로 둔다.

이번 PRD의 목표는 다음과 같다.

1. 현재 `shacs-bot run`과 channel runtime 후속 구현의 실제 상태를 기록한다.
2. 다음 작업을 current architecture 문서 정렬과 작은 local runtime 개선 중심으로 제한한다.
3. durable supervisor, webhook hosting, transactional metadata, multi-process owner coordination을 완료 주장이나 기본 범위로 두지 않는다.

## SPEC 입력

- 주관 spec: `docs/specs/012-runtime-services/SPEC.md`
- 선행 PRD:
  - `docs/specs/012-runtime-services/prds/000-service-reentry-and-dedup.md`
  - `docs/specs/012-runtime-services/prds/001-channel-worker-runtime.md`
- 선행 기준:
  - `docs/specs/001-session-kernel/SPEC.md`
  - `docs/specs/002-command-event-effect/SPEC.md`
  - `docs/specs/006-session-store/SPEC.md`
  - `docs/specs/007-main-orchestrator-policy/SPEC.md`
  - `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`
  - `docs/specs/011-subagent-runtime/SPEC.md`

## Dependency Cut

- PRD 000은 process-local reentry, queueing, lock, cancellation/status tracking의 현재 의미를 제공한다.
- PRD 001은 local channel runtime process와 `ChannelManager` 중심 경계를 제공한다.
- 013은 TUI, local API, channel worker가 같은 session projection과 command 의미를 공유해야 함을 제공한다.
- 014는 status, dispatch error, retry attempt, metadata hint를 diagnostics surface에 노출할 때의 기준을 제공한다.
- 015는 process lifecycle과 upgrades를 다루지만, 현재 문서는 formal owner lease나 stale owner recovery가 구현됐다고 보지 않는다.
- 016은 release gate와 coverage matrix가 executable evidence를 가져야 함을 제공한다.

## 범위

- `shacs-bot run`에서 WebSocket과 external channel transport가 함께 뜨는 현재 상태.
- Discord Gateway, Slack Socket Mode, Telegram long polling, Email IMAP polling, WhatsApp bridge WebSocket의 current transport family.
- `MessageBus`와 `ChannelManager` 경계를 통한 inbound/outbound dispatch.
- process-local same-session follow-up queue.
- runtime metadata JSON의 best-effort cursor, diagnostic, delivery hint.
- progress projection 차이. WebSocket은 `delta`/`stream_end`와 final `message`, Telegram/Discord/Slack은 preview update, Email/WhatsApp은 final-only.
- platform normalizer와 outbound helper의 session metadata, reply context, content contract 보존.
- 남은 future durable runtime work 정리.

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
- hosted webhook, Telegram webhook, provider-specific operational hardening.
- attachment download와 streaming edit 상세.

## 현재 구현 상태

### 현재 아키텍처 매핑

- `shacs-bot run`은 WebSocket server와 Telegram, Discord, Slack, Email, WhatsApp bridge transport를 같은 local runtime process 안에서 시작한다.
- Discord Gateway, Slack Socket Mode, Telegram long polling, Email IMAP polling, WhatsApp bridge WebSocket은 현재 구현된 long-running transport family다.
- external runtime은 `MessageBus`와 `ChannelManager` 경계를 통해 inbound/outbound를 전달한다.
- channel adapter lifecycle은 `ChannelManager`의 start/stop, status, lifecycle error reporting으로 관리된다.
- outbound dispatch는 channel adapter를 통해 수행되고, retry attempt와 dispatch error는 manager/metadata 경계에서 관찰된다.
- same-session accepted inbound는 process-local `SessionTurnLock`과 in-memory pending follow-up queue로 직렬화된다.
- shared lock conflict가 있으면 external inbound는 deferred/pending 처리로 돌아가 이후 재시도된다.
- runtime metadata JSON은 Telegram offset, Discord REST last id, Discord Gateway resume state, Email UIDVALIDITY/seen UID, outbound delivery status를 best-effort로 저장한다.
- WebSocket progress는 coalesced `delta`, `stream_end`, final `message` event로 전달된다.
- Telegram, Discord, Slack progress는 preview update로 전달된다. Email과 WhatsApp은 final-only다.
- platform normalizer와 outbound helper는 session metadata, reply context, content contract를 보존한다.

### partial 또는 formal-looking but incomplete

- retry/backoff state는 durable metadata store에 저장되지 않는다.
- process-local pending follow-up queue는 durable pending-message queue가 아니다.
- cancellation persistence와 restart replay는 없다.
- runtime metadata JSON은 transaction이나 exactly-once delivery를 보장하지 않는다.
- channel-neutral `ApprovalBroker`와 `OutboundRouter`는 없다.
- formal `RuntimeSupervisor`, owner lease/heartbeat, stale owner recovery, safe shutdown layer는 없다.
- multi-process TUI/API/channel owner coordination은 아직 해결된 상태가 아니다.

### future gap

- durable queue와 durable scheduler.
- formal wake command envelope.
- formal service command envelope와 typed dedupe key/retry state.
- durable pending-message queue.
- cancellation persistence와 restart replay.
- transactional service metadata store.
- durable retry/backoff scheduler.
- channel-neutral `ApprovalBroker`와 `OutboundRouter`.
- formal `RuntimeSupervisor`, owner lease/heartbeat, stale owner recovery, safe shutdown.
- hosted webhook, Telegram webhook, provider-specific operational hardening.
- attachment download와 streaming edit 상세.

## waves / next work

### Wave 1. Documentation alignment

- PRD 000, 001, 002와 Spec 012가 process-local runtime mapping을 같은 용어로 설명하게 맞춘다.
- durable queue, exactly-once, transactional metadata, formal supervisor 표현을 future gap으로 옮긴다.
- self-hosted/personal-use 범위 밖 platform 문장을 제거하거나 범위 제외로 옮긴다.

### Wave 2. Current runtime evidence 유지

- `MessageBus`, `SessionTurnLock`, `LoopTaskRegistry`, `ChannelManager`, `ExternalSessionTurnCoordinator`, runtime metadata JSON, platform normalizer 테스트 이름을 문서 evidence에 연결한다.
- API/WebSocket local surface evidence는 health/models/chat/WebSocket/streaming tests로 요약한다.
- verification matrix에서 durable runtime 완료 증거처럼 읽히는 표현을 피한다.

### Wave 3. Small local runtime improvements only

- current architecture 안에서 필요한 작업은 status/error wording, metadata diagnostics, progress projection 설명 개선처럼 local runtime에 닿는 작은 작업으로 제한한다.
- durable retry/backoff, webhook hosting, supervisor owner lease는 별도 설계 결정 전까지 시작하지 않는다.

### Wave 4. Formal durable runtime decision

- 나중에 필요성이 확인되면 durable queue, scheduler, wake envelope, typed dedupe/retry, transactional metadata, restart replay를 별도 PRD로 연다.
- 그 결정은 self-hosted personal runtime에 정말 필요한지 먼저 확인해야 한다.

## Verification Evidence

- Bus: `bus_preserves_fifo_and_sizes_for_inbound_and_outbound`, `bounded_bus_reports_capacity_for_both_queues`, `cloned_bus_handles_share_queues_and_blocking_consumers_wake`, `drain_matching_limits_matches_and_preserves_retained_fifo`.
- Loop/control: `loop_priority_new_cancels_registered_task_before_clearing_session`, `loop_priority_status_reports_registered_async_task`, `session_turn_lock_rejects_duplicate_active_session`, `loop_observes_registered_cancellation_token_before_provider_call`, `adapter_reports_duplicate_session_turn_as_retryable_busy_error`.
- Runner/progress: `stream_coalescer_batches_text_deltas_without_session_persistence`, `runtime_runner_drains_mid_turn_injections_between_iterations`, `runtime_runner_caps_mid_turn_injection_cycles`.
- Channel worker/metadata: `builtin_live_worker_descriptors_mark_websocket_ready_and_external_workers_gated`, `manager_tracks_lifecycle_retries_and_stream_delta_dispatch`, `manager_records_dispatch_error_and_clears_after_success`, `manager_lifecycle_continues_after_adapter_errors`, `external_transport_specs_respect_enabled_and_external_only_runtime`, `external_outbound_channel_manager_dispatches_via_channel_adapters`, `external_outbound_channel_manager_preserves_streaming_frames`, `external_session_turn_coordinator_queues_same_session_followups`, `external_session_turn_coordinator_defers_shared_lock_conflicts`, `worker_metadata_updates_preserve_delivery_history`, `platform_outbound_helpers_preserve_reply_context`, `email_uid_validity_change_clears_seen_uid_cache`, `email_runtime_requires_consent_and_allow_from_for_imap`, `platform_normalizers_preserve_session_metadata_and_content_contracts`, `whatsapp_bridge_normalizes_auth_dedupe_group_policy_media_and_outbound_frames`.
- API/local surface: `crates/shacs-api/src/lib.rs`의 health, models, chat, WebSocket, streaming tests를 local API/WebSocket runtime evidence로 요약한다.

이 증거는 current local runtime과 channel follow-up behavior를 뒷받침한다. durable queue, durable scheduler, transactional metadata, formal supervisor의 완료 증거는 아니다.

## Open Risks

- 문서가 다시 future durable runtime을 현재 요구사항처럼 말하면 구현 상태와 제품 범위가 어긋난다.
- process-local follow-up queue를 restart-safe queue로 읽으면 장애 복구 기대가 잘못 잡힌다.
- metadata JSON을 exactly-once ledger로 읽으면 channel duplicate, cursor reset, delivery retry 설명이 틀어진다.
- hosted webhook이나 multi-process owner coordination을 기본 범위로 넣으면 개인용 self-hosted 제품보다 platform 운영 제품처럼 변한다.

## 종료 기준

- PRD 002는 현재 `shacs-bot run` channel runtime follow-up 구현을 Spec 012 current architecture 종료 범위의 local runtime mapping으로 설명한다.
- PRD 002는 durable queue, scheduler, wake envelope, retry/backoff scheduler, transactional metadata, supervisor, broker/router를 future gap으로 둔다.
- PRD 002는 public webhook hosting, organization inbox, operator dashboard, multi-tenant fan-out, extra channel expansion을 현재 범위에서 제외한다.
- PRD 002는 Spec 012가 formal durable runtime으로 완료됐다고 주장하지 않는다.
