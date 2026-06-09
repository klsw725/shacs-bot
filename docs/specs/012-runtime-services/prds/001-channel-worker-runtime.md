# PRD 001. channel worker runtime

## 목표

이 문서는 `docs/specs/012-runtime-services/SPEC.md`의 하위 실행 문서다. 기존 문서는 `RuntimeSupervisor`, owner lease, durable metadata, `ApprovalBroker`, `OutboundRouter`가 목표 계약처럼 앞에 서 있었다. 현재 문서는 2026-05-17 기준 이미 구현된 local runtime channel worker mapping을 Spec 012 current architecture 종료 범위로 설명하고, formal supervisor와 channel-neutral broker/router를 future work로 둔다.

이번 PRD의 목표는 다음과 같다.

1. `shacs-bot run`이 현재 제공하는 local runtime process wiring을 정확히 설명한다.
2. `ChannelManager`와 adapter lifecycle, outbound dispatch, retry attempt, stream delta dispatch, status/error reporting을 현재 구현으로 고정한다.
3. durable supervisor, approval broker, outbound router, webhook hardening을 완료된 요구사항처럼 보이지 않게 분리한다.

## SPEC 입력

- 주관 spec: `docs/specs/012-runtime-services/SPEC.md`
- 선행 기준:
  - `docs/specs/001-session-kernel/SPEC.md`
  - `docs/specs/002-command-event-effect/SPEC.md`
  - `docs/specs/006-session-store/SPEC.md`
  - `docs/specs/007-main-orchestrator-policy/SPEC.md`
  - `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`
  - `docs/specs/011-subagent-runtime/SPEC.md`

## 참고 구조 입력

이 PRD는 외부 channel worker를 agent loop와 분리한다는 참고 원칙만 사용한다. 현재 `shacs-bot`은 개인용 self-hosted local runtime이므로, hosted gateway나 operator platform 형태로 확장하지 않는다.

가져오는 원칙은 다음으로 제한한다.

1. channel adapter는 provider-specific transport를 소유한다.
2. session truth는 core/session store가 소유한다.
3. outbound delivery는 assistant final output을 관찰해 transport로 투영한다.
4. progress projection은 final answer를 대체하지 않는다.

현재 문서에서 `RuntimeSupervisor`, `ApprovalBroker`, `OutboundRouter`는 future abstraction 이름이다. 현재 완성된 layer로 보지 않는다.

## 범위

- `shacs-bot run` local runtime process wiring.
- WebSocket, Telegram, Discord, Slack, Email, WhatsApp bridge transport의 현재 위치.
- `MessageBus`와 `ChannelManager`를 통한 inbound/outbound 이동.
- `LiveChannelWorkerKind`, `LiveChannelWorkerDescriptor`, `LiveChannelWorker`, `builtin_live_worker_descriptors`의 worker descriptor 의미.
- adapter `start`/`stop`, `ChannelStatus`, `ChannelRetryPolicy`, lifecycle error status.
- outbound dispatch retry attempt와 stream delta dispatch.
- external inbound same-session serialization.
- runtime metadata JSON cursor, diagnostic, delivery hint.
- platform normalizer의 session metadata와 content contract 보존.
- progress projection boundary.

## 범위 제외

- public webhook hosting obligation.
- hosted webhook, Telegram webhook, provider-specific operational hardening.
- organization inbox와 operator dashboard.
- multi-tenant webhook fan-out.
- multi-user task distribution.
- distributed task queue와 multi-node leader election.
- vendor-specific queue/cron product dependency.
- Slack, Discord, Telegram, Email, WhatsApp 바깥의 추가 channel.
- attachment download와 streaming edit 상세.

## 현재 구현 상태

### 현재 아키텍처 매핑

- `shacs-bot run`은 WebSocket server와 Telegram, Discord, Slack, Email, WhatsApp bridge transport를 하나의 local runtime process 안에서 시작한다.
- external transport adapter는 `MessageBus`와 `ChannelManager` 경계로 inbound/outbound를 전달한다.
- `ChannelManager`는 adapter lifecycle, outbound dispatch, retry attempt, stream delta dispatch, status/error reporting을 관리한다.
- `builtin_live_worker_descriptors`는 WebSocket worker를 ready로 표시하고, external worker는 설정과 runtime 조건에 따라 gated로 다룬다.
- `ExternalSessionTurnCoordinator`는 같은 session의 follow-up을 process-local pending queue에 넣어 순서화한다.
- shared `SessionTurnLock`이 busy이면 external inbound는 deferred 상태로 남아 이후 처리된다.
- `AgentLoopChatCompletionAdapter`는 API/WebSocket/chat completion과 external channel이 같은 session lock을 공유하게 한다.
- runtime metadata JSON은 Telegram offset, Discord REST last id/Gateway resume state, Email UIDVALIDITY/seen UID, outbound delivery status를 best-effort로 저장한다.
- WebSocket progress는 `delta`, `stream_end`, final `message`로 보인다. Telegram, Discord, Slack, Email, WhatsApp external transport는 final-only이며 기존 message를 edit/update하지 않는다.
- platform normalizer는 provider payload를 session metadata와 content contract를 보존하는 inbound/outbound frame으로 바꾼다.

### partial 또는 formal-looking but incomplete

- `RuntimeSupervisor`라는 formal layer는 아직 없다. 현재는 `shacs-bot run` process wiring과 `ChannelManager` lifecycle이 중심이다.
- owner lease, heartbeat, stale owner recovery, safe shutdown은 formal supervisor 계약으로 완성되지 않았다.
- channel-neutral `ApprovalBroker`는 없다. approval projection을 모든 channel에 공통 broker로 노출하는 구조는 future work다.
- channel-neutral `OutboundRouter`는 없다. 현재 outbound dispatch는 channel adapter와 manager 경계에서 다룬다.
- runtime metadata JSON은 durable service metadata store가 아니다.
- process-local follow-up queue는 durable pending-message queue가 아니다.

### future gap

- formal `RuntimeSupervisor`.
- owner lease/heartbeat, stale owner recovery, safe shutdown.
- multi-process TUI/API/channel owner coordination.
- durable pending-message queue, cancellation persistence, restart replay.
- transactional service metadata store.
- durable retry/backoff scheduler.
- channel-neutral `ApprovalBroker`와 `OutboundRouter`.
- hosted webhook, Telegram webhook, provider-specific operational hardening.
- attachment download와 streaming edit 상세.

## 역할 경계

### 현재 ChannelManager

`ChannelManager`가 현재 맡는 일은 다음과 같다.

1. adapter start/stop lifecycle을 호출한다.
2. `ChannelStatus`와 lifecycle error status를 기록한다.
3. outbound dispatch를 channel adapter로 보낸다.
4. retry attempt와 dispatch error를 관찰한다.
5. stream delta frame을 adapter로 전달한다.

`ChannelManager`가 현재 보장하지 않는 일은 다음과 같다.

1. exactly-once delivery.
2. transactional metadata write.
3. durable retry/backoff scheduling.
4. session truth 확정.
5. approval truth 확정.

### 현재 Channel adapter

adapter가 현재 맡는 일은 다음과 같다.

1. provider-specific transport를 시작하고 멈춘다.
2. inbound payload를 normalized frame으로 넘긴다.
3. outbound frame을 provider-specific send/update로 투영한다.
4. transport failure를 lifecycle 또는 dispatch error로 보고한다.

adapter가 하면 안 되는 일은 다음과 같다.

1. provider/tool execution 직접 수행.
2. assistant final answer 직접 확정.
3. approval policy 직접 결정.
4. raw transport payload를 session truth로 저장.

### 현재 ExternalSessionTurnCoordinator

coordinator가 현재 맡는 일은 다음과 같다.

1. 같은 session으로 들어온 accepted inbound를 process-local pending queue에 넣는다.
2. active turn이 끝난 뒤 follow-up을 다시 session boundary로 밀어 넣는다.
3. shared lock conflict를 deferred 상태로 남긴다.

coordinator가 현재 보장하지 않는 일은 다음과 같다.

1. restart 뒤 pending message replay.
2. durable cancellation recovery.
3. multi-process owner coordination.

## waves / next work

### Wave 1. Current channel runtime mapping 정리

- `shacs-bot run`, `MessageBus`, `ChannelManager`, external transport adapter, session lock sharing을 현재 구현으로 설명한다.
- worker descriptor와 gated external worker 의미를 current architecture로 둔다.
- progress projection과 final answer boundary를 명시한다.

### Wave 2. Metadata와 retry 표현 축소

- runtime metadata JSON을 cursor, diagnostic, delivery hint로만 표현한다.
- outbound retry를 durable retry scheduler가 아니라 manager/adapter dispatch attempt로 설명한다.
- delivery failure가 session completion을 되돌리지 않는다는 경계를 유지한다.

### Wave 3. Future supervisor 분리

- formal `RuntimeSupervisor`, owner lease, heartbeat, stale owner recovery, safe shutdown은 future gap으로 둔다.
- channel-neutral `ApprovalBroker`와 `OutboundRouter`는 future abstraction으로 남긴다.
- hosted webhook과 provider-specific operational hardening은 current scope에서 제외한다.

## Verification Evidence

- `builtin_live_worker_descriptors_mark_websocket_ready_and_external_workers_gated`는 worker descriptor와 external worker gating을 확인한다.
- `manager_tracks_lifecycle_retries_and_stream_delta_dispatch`는 manager lifecycle retry와 stream delta dispatch를 확인한다.
- `manager_records_dispatch_error_and_clears_after_success`는 dispatch error reporting과 recovery를 확인한다.
- `manager_lifecycle_continues_after_adapter_errors`는 adapter error 뒤 lifecycle이 계속 관찰되는지 확인한다.
- `external_transport_specs_respect_enabled_and_external_only_runtime`은 external transport spec gating을 확인한다.
- `external_outbound_channel_manager_dispatches_via_channel_adapters`와 `external_outbound_channel_manager_preserves_streaming_frames`는 outbound dispatch와 streaming frame 보존을 확인한다.
- `external_session_turn_coordinator_queues_same_session_followups`와 `external_session_turn_coordinator_defers_shared_lock_conflicts`는 process-local follow-up serialization을 확인한다.
- `worker_metadata_updates_preserve_delivery_history`는 metadata JSON이 delivery history hint를 보존하는지 확인한다.
- `platform_outbound_helpers_preserve_reply_context`, `platform_normalizers_preserve_session_metadata_and_content_contracts`, `whatsapp_bridge_normalizes_auth_dedupe_group_policy_media_and_outbound_frames`는 platform normalizer와 outbound context 보존을 확인한다.
- `email_uid_validity_change_clears_seen_uid_cache`와 `email_runtime_requires_consent_and_allow_from_for_imap`은 Email cursor와 safety gate를 확인한다.

이 증거는 local channel runtime mapping의 증거다. formal supervisor, durable metadata, exactly-once delivery, channel-neutral broker/router의 완료 증거가 아니다.

## Open Risks

- current `shacs-bot run` wiring을 supervisor layer로 과장하면 owner recovery와 safe shutdown 기대가 잘못 생긴다.
- progress preview를 final answer처럼 설명하면 Email/WhatsApp final-only boundary와 WebSocket final `message` boundary가 흐려진다.
- metadata JSON을 durable metadata store처럼 설명하면 cursor loss와 duplicate delivery 상황을 잘못 다루게 된다.
- channel-neutral broker/router를 성급히 도입하면 self-hosted personal runtime에 과한 platform 구조가 된다.

## 종료 기준

- 문서는 현재 channel runtime을 local process wiring, `MessageBus`, `ChannelManager`, adapter lifecycle, process-local follow-up queue 중심으로 설명한다.
- 문서는 `RuntimeSupervisor`, owner lease, durable pending queue, transactional metadata, approval broker, outbound router를 future gap으로 둔다.
- 문서는 public webhook hosting, operator dashboard, organization inbox, extra channel expansion을 현재 범위에서 제외한다.
- PRD 001은 Spec 012 current architecture 종료의 channel worker 증거를 제공한다.
- 문서는 Spec 012 또는 PRD 001이 formal durable channel runtime으로 완료됐다고 주장하지 않는다.
