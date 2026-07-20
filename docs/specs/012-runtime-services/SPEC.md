# runtime services 아키텍처 명세

Status: Complete (Scoped)

Implemented scope: 현재 구현은 process-local `MessageBus`, session turn lock, active task cancellation and status, command router priority, local API and WebSocket surface, channel worker wiring, process-local follow-up queue, and runtime metadata hints를 local runtime services scope로 지원한다.

Open work moved to: [029 durable runtime recovery and data migration](../029-durable-runtime-recovery-and-data-migration/SPEC.md), [031 UI projection, diagnostics, and release evidence parity](../031-ui-projection-diagnostics-and-release-evidence-parity/SPEC.md), [033 evaluation automation live integration](../033-evaluation-automation-live-integration/SPEC.md)

Not carried forward: distributed task queue, multi-node leader election, organization inbox, operator dashboard, multi-tenant webhook fan-out, public webhook hosting obligation, multi-user task distribution, vendor-specific queue or cron dependency, 현재 지원 목록 밖의 추가 channel은 이 closure에 포함하지 않는다.

## 문서 목적

이 문서는 `shacs-bot`의 runtime services를 현재 구현과 앞으로 남은 작업으로 나누어 정리한다. Spec 012는 2026-05-17 현재 아키텍처 매핑 기준으로 종료됐다. 현재 코드는 process-local message bus, session turn lock, active task cancellation/status, local runtime channel workers, channel lifecycle/retry/stream dispatch, process-local follow-up queue, runtime metadata JSON hint를 갖고 있지만, durable queue, durable scheduler, formal wake envelope, exactly-once delivery, transactional metadata store가 완성된 상태는 아니다.

이 문서의 현재 역할은 다음과 같다.

1. 현재 구현된 runtime service 경계를 정확히 설명한다.
2. 현재 아키텍처 매핑으로 인정할 수 있는 범위를 고정한다.
3. current architecture 기준 종료 범위와 future durable runtime 계약을 분리한다.

`shacs-bot`은 `self-hosted/personal-use` 성격의, 사용자가 직접 설치하고 운영하는 개인용 런타임을 기본으로 본다. 따라서 목표는 한 로컬 런타임 프로세스 안에서 사용자의 세션, 채널, 로컬 API를 안정적으로 이어 주는 것이다. 운영자 조직, multi-user task distribution, public webhook hosting obligation을 기본 제품 범위로 보지 않는다.

## 상위 기준과의 관계

이 문서는 `docs/SYSTEM-FOUNDATION.md`, `docs/specs/001-session-kernel/SPEC.md`, `docs/specs/002-command-event-effect/SPEC.md`, `docs/specs/006-session-store/SPEC.md`, `docs/specs/007-main-orchestrator-policy/SPEC.md`, `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`, `docs/specs/011-subagent-runtime/SPEC.md`를 전제로 한다.

현재 완료 판정은 하나의 formal service runtime이 queue, scheduler, mailbox, hooks, background worker를 모두 durable command envelope로 통제한다는 뜻이 아니다. 완료의 의미는 bus, lock, lifecycle, channel manager, command router, local API/WebSocket, CLI runtime wiring이 나뉘어 현재 runtime service 경계를 설명하고 검증한다는 뜻이다.

따라서 이 문서는 다음 세 층을 구분한다.

1. 현재 아키텍처 매핑: 이미 존재하는 process-local bus, inbound/outbound queueing, session lock, active task registry, cancellation token, local runtime process wiring, channel adapter lifecycle, outbound dispatch retry, stream delta dispatch, status/error reporting, process-local follow-up queue, runtime metadata JSON hint, platform normalizer, local API/WebSocket surface.
2. 부분 구현 또는 formal-looking but incomplete 영역: service command envelope, wake/reentry contract, dedupe key, retry state, owner coordination, approval/outbound abstraction처럼 방향은 맞지만 durable runtime 계약으로 닫히지 않은 영역.
3. future formal model: durable queue, durable scheduler, formal wake command envelope, exactly-once delivery, durable pending-message queue, cancellation persistence, restart replay, transactional service metadata store, durable retry/backoff scheduler, channel-neutral `ApprovalBroker`, channel-neutral `OutboundRouter`, formal `RuntimeSupervisor`, owner lease/heartbeat, stale owner recovery, safe shutdown.

## 범위

현재 문서에서 다루는 범위는 다음과 같다.

1. process-local runtime service 경계.
2. `MessageBus`와 inbound/outbound queueing의 현재 의미.
3. session turn lock, active loop task registry, cancellation/status tracking.
4. command router priority와 normal turn boundary.
5. `shacs-bot run`의 WebSocket 및 외부 channel transport wiring.
6. `ChannelManager`의 adapter lifecycle, outbound dispatch, retry attempt, stream delta dispatch, status/error reporting.
7. external inbound same-session serialization과 process-local pending follow-up queue.
8. runtime metadata JSON의 cursor, diagnostic, delivery hint 역할.
9. platform normalizer가 보존해야 하는 session metadata와 content contract.
10. progress projection boundary. 최종 답변은 계속 authoritative output이다.
11. MCP, Dream, Subagent 관련 lifecycle capability report의 현재 위치.
12. 현재 테스트 증거로 확인되는 동작.
13. 아직 future work로 남겨야 하는 durable service runtime 항목.

이 문서는 다음을 현재 기능으로 선언하지 않는다.

1. durable queue가 완성됐다는 주장.
2. durable scheduler와 formal wake command envelope가 완성됐다는 주장.
3. 모든 service가 typed dedupe key와 retry state를 가진 formal service command envelope를 공유한다는 주장.
4. exactly-once delivery가 보장된다는 주장.
5. pending message, cancellation, restart replay가 durable하게 복구된다는 주장.
6. service metadata가 transactional store로 관리된다는 주장.
7. retry/backoff가 durable scheduler로 관리된다는 주장.
8. channel-neutral `ApprovalBroker`와 `OutboundRouter`가 완성됐다는 주장.
9. formal `RuntimeSupervisor`, owner lease/heartbeat, stale owner recovery, safe shutdown layer가 완성됐다는 주장.
10. TUI/API/channel owner coordination이 multi-process 수준으로 해결됐다는 주장.
11. hosted webhook, Telegram webhook, provider-specific operational hardening이 완료됐다는 주장.
12. attachment download와 streaming edit 상세가 완성됐다는 주장.

## 범위 제외

아래 항목은 이 프로젝트의 현재 self-hosted/personal-use 범위에 필요하지 않다.

1. distributed task queue.
2. multi-node leader election.
3. organization inbox.
4. operator dashboard.
5. multi-tenant webhook fan-out.
6. public webhook hosting obligation.
7. multi-user task distribution.
8. vendor-specific queue/cron product dependency.
9. Slack, Discord, Telegram, Email, WhatsApp 바깥의 추가 channel. 나중에 명시 요청이 있으면 별도 spec으로 다룬다.

## 현재 구현 요약

현재 runtime services는 한 로컬 프로세스 안에서 동작하는 조합이다.

1. `crates/shacs-bus/src/lib.rs`의 `MessageBus`는 inbound/outbound FIFO, bounded queue, blocking consume wake, `drain_inbound_matching`, queue size helper를 제공한다. 이 bus는 process-local queue다. durable queue나 exactly-once delivery가 아니다.
2. `crates/shacs-core/src/runtime/loop_control.rs`의 `SessionTurnLock`, `SessionTurnGuard`, `CancellationToken`, `LoopTaskRegistry`, `ActiveLoopTask`, `ActiveLoopTaskSnapshot`, `LoopTaskStatus`는 같은 session의 동시 turn을 막고 active task cancellation/status를 process-local로 추적한다.
3. `crates/shacs-command/src/lib.rs`의 built-in command router는 priority, exact, prefix command를 분리한다. `/status`, `/stop`, `/restart` 계열 priority command는 active turn lock 앞에서 처리될 수 있고, exact/prefix command는 정상 turn boundary를 따른다.
4. `crates/shacs-cli/src/lib.rs`의 `shacs-bot run`은 WebSocket과 Telegram, Discord, Slack, Email, WhatsApp bridge transport를 하나의 local runtime process 안에서 시작한다. 외부 transport adapter는 `MessageBus`와 `ChannelManager`를 통해 inbound/outbound를 주고받는다.
5. `ExternalSessionTurnCoordinator`는 같은 session으로 들어온 external inbound를 process-local follow-up queue로 직렬화한다. shared `SessionTurnLock` 충돌은 durable recovery가 아니라 local pending과 retry 관찰로 다룬다.
6. `AgentLoopChatCompletionAdapter`는 같은 `SessionTurnLock`을 공유해 API/WebSocket/chat completion과 external channel turn이 같은 session boundary를 넘지 않게 한다.
7. runtime metadata JSON은 Telegram offset, Discord REST last id, Discord Gateway resume state, Email UIDVALIDITY/seen UID, outbound delivery status를 best-effort로 저장한다. 이것은 cursor, diagnostic, delivery hint이며 transactional metadata store가 아니다.
8. WebSocket progress는 `delta`, `stream_end`, final `message`로 보인다. Telegram, Discord, Slack, Email, WhatsApp external transport는 final-only이며 기존 message를 edit/update하지 않는다. 어느 경우에도 final answer가 authoritative output이다.
9. `crates/shacs-channels/src/lib.rs`의 `LiveChannelWorkerKind`, `LiveChannelWorkerDescriptor`, `LiveChannelWorker`, `builtin_live_worker_descriptors`, `ChannelStatus`, `ChannelRetryPolicy`, `ChannelManager`는 adapter `start`/`stop`, outbound retry, stream delta dispatch, lifecycle error status를 관리한다.
10. platform normalizer는 Slack, Discord, Telegram, Email, WhatsApp inbound를 session metadata와 content contract를 보존하는 형태로 정규화한다.
11. `crates/shacs-core/src/runtime/lifecycle.rs`의 `RuntimeCapabilityReport`, `RuntimeCapabilityStatus`, `McpLifecycle`, `DreamLifecycle`는 MCP, Dream, Subagent 같은 runtime capability의 현재 상태를 보고하는 경계다.
12. `crates/shacs-api/src/lib.rs`는 local API/WebSocket surface를 제공한다. health/models route는 no-runtime 경로로 유지되고, session lock, streaming event, WebSocket event surface는 local runtime boundary를 따른다.

## 현재 아키텍처 매핑

### MessageBus와 queueing

현재 queueing은 `MessageBus`의 process-local bounded FIFO다.

인정할 수 있는 현재 의미는 다음과 같다.

1. inbound channel message와 outbound assistant frame을 같은 bus abstraction으로 이동한다.
2. queue size와 capacity를 관찰할 수 있다.
3. blocking consumer는 clone된 bus handle 사이에서도 wake된다.
4. `drain_inbound_matching`은 조건에 맞는 inbound를 제한된 개수만 drain하고 나머지 FIFO를 보존한다.

아직 future work로 남는 의미는 다음과 같다.

1. durable queue.
2. restart replay.
3. exactly-once delivery.
4. service-wide typed dedupe marker.
5. durable retry/backoff scheduler.

### Session turn lock과 active task control

현재 동시성 제어는 process-local이다.

인정할 수 있는 현재 의미는 다음과 같다.

1. 같은 session의 중복 active turn을 거절한다.
2. priority command는 등록된 active loop task를 취소하거나 status를 조회할 수 있다.
3. `CancellationToken`은 provider call 전 cancellation을 관찰하게 한다.
4. active task snapshot은 local diagnostics/status에 필요한 상태를 제공한다.

아직 future work로 남는 의미는 다음과 같다.

1. cancellation persistence.
2. restart 뒤 active task replay.
3. multi-process owner lease.
4. stale owner recovery.

### Command router boundary

현재 command router는 priority command와 normal turn command를 구분한다.

1. `/status`, `/stop`, `/restart` 계열은 active turn lock 앞에서 처리될 수 있다.
2. exact/prefix command는 normal turn boundary를 따른다.
3. router priority는 service runtime policy가 아니라 built-in command dispatch boundary다.

### Channel runtime

현재 channel runtime은 local process wiring이다.

1. WebSocket, Telegram, Discord, Slack, Email, WhatsApp bridge transport가 `shacs-bot run` 안에서 시작된다.
2. `ChannelManager`가 adapter lifecycle, status, lifecycle error, outbound dispatch, retry attempt, stream delta dispatch를 맡는다.
3. external inbound는 same-session follow-up queue를 통해 직렬화된다.
4. platform normalizer는 session metadata와 content contract를 보존한다.
5. progress projection은 사용자에게 진행 상태를 보여 주지만 final answer를 대체하지 않는다.

아직 future work로 남는 의미는 다음과 같다.

1. durable pending-message queue.
2. transactional delivery metadata.
3. channel-neutral `ApprovalBroker`.
4. channel-neutral `OutboundRouter`.
5. hosted webhook과 provider-specific hardening.
6. attachment download와 streaming edit 상세.

### Lifecycle capability report

현재 lifecycle capability report는 runtime capability의 상태를 설명하는 local report다.

1. MCP lifecycle과 Dream lifecycle은 capability status로 노출된다.
2. 이 report는 service supervisor 계약이나 owner lease가 아니다.
3. Subagent runtime과 연결되는 부분은 current capability visibility로만 본다.

## service-owned metadata와 session truth

현재 metadata 경계는 다음처럼 좁게 본다.

1. runtime metadata JSON은 cursor, diagnostic, outbound delivery hint다.
2. external message id, channel cursor, delivery status는 service-owned metadata다.
3. `SessionState`, turn result, assistant final answer, approval truth는 session truth다.
4. service-owned metadata가 유실되거나 중복돼도 session truth를 대체하면 안 된다.
5. metadata JSON은 best-effort 저장이다. transactional metadata store, durable dedupe store, exactly-once ledger로 취급하면 안 된다.

## reentry, dedupe, retry의 현재 의미

현재 reentry와 dedupe는 formal service command envelope가 아니라 각 boundary의 조합이다.

1. channel inbound는 normalizer와 external coordinator를 거쳐 session turn으로 들어온다.
2. 같은 session의 follow-up은 process-local pending queue에서 순서화된다.
3. `MessageBus`는 FIFO와 bounded queue를 제공하지만 durable dedupe key를 소유하지 않는다.
4. `ChannelManager` retry는 outbound dispatch attempt와 lifecycle 관찰이다. 세션 정책 retry가 아니다.
5. runtime metadata JSON의 cursor는 중복을 줄이는 hint다. exactly-once guarantee가 아니다.
6. priority command cancellation/status는 process-local active task registry에 의존한다.

future formal model에서 다룰 수 있는 항목은 다음과 같다.

1. typed service command envelope.
2. service-wide dedupe key.
3. retry state persistence.
4. formal wake command envelope.
5. stale wake handling.
6. durable cancellation and restart replay.

## 현재 검증 증거

현재 아키텍처 매핑을 뒷받침하는 테스트 증거는 다음과 같다.

1. Bus: `bus_preserves_fifo_and_sizes_for_inbound_and_outbound`, `bounded_bus_reports_capacity_for_both_queues`, `cloned_bus_handles_share_queues_and_blocking_consumers_wake`, `drain_matching_limits_matches_and_preserves_retained_fifo`.
2. Loop/control: `loop_priority_new_cancels_registered_task_before_clearing_session`, `loop_priority_status_reports_registered_async_task`, `session_turn_lock_rejects_duplicate_active_session`, `loop_observes_registered_cancellation_token_before_provider_call`, `adapter_reports_duplicate_session_turn_as_retryable_busy_error`.
3. Runner/progress: `stream_coalescer_batches_text_deltas_without_session_persistence`, `runtime_runner_drains_mid_turn_injections_between_iterations`, `runtime_runner_caps_mid_turn_injection_cycles`.
4. Channel worker/metadata: `builtin_live_worker_descriptors_mark_websocket_ready_and_external_workers_gated`, `manager_tracks_lifecycle_retries_and_stream_delta_dispatch`, `manager_records_dispatch_error_and_clears_after_success`, `manager_lifecycle_continues_after_adapter_errors`, `external_transport_specs_respect_enabled_and_external_only_runtime`, `external_outbound_channel_manager_dispatches_via_channel_adapters`, `external_outbound_channel_manager_preserves_streaming_frames`, `external_session_turn_coordinator_queues_same_session_followups`, `external_session_turn_coordinator_defers_shared_lock_conflicts`, `worker_metadata_updates_preserve_delivery_history`, `platform_outbound_helpers_preserve_reply_context`, `email_uid_validity_change_clears_seen_uid_cache`, `email_runtime_requires_consent_and_allow_from_for_imap`, `platform_normalizers_preserve_session_metadata_and_content_contracts`, `whatsapp_bridge_normalizes_auth_dedupe_group_policy_media_and_outbound_frames`.
5. API/local surface: `crates/shacs-api/src/lib.rs`의 health, models, chat, WebSocket, streaming tests는 local API/WebSocket surface가 runtime lock과 event projection을 따르는 증거로 본다.

이 증거는 current architecture mapping 기준 Spec 012 종료를 뒷받침한다. Spec 012가 durable service runtime으로 완료됐다는 증거는 아니다.

## 종료 기준

Spec 012는 current architecture 기준으로 닫는다. 이 기준은 다음과 같다.

1. process-local bus, lock, channel runtime, metadata hint, lifecycle report를 현재 runtime services 경계로 인정한다.
2. durable service runtime은 future work로 남긴다.
3. durable queue, scheduler, wake envelope, typed dedupe/retry state, restart replay, transactional metadata, supervisor owner lease가 구현됐다고 주장하지 않는다.

이 종료는 formal durable runtime 완료 선언이 아니다.
