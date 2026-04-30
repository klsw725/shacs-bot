# PRD 002. channel runtime follow-up waves

## 목표

이 문서는 `docs/specs/012-runtime-services/SPEC.md`의 하위 실행 문서다. `channel discord-worker` foreground REST polling slice 이후, channel worker를 장기 실행 local runtime 구조로 승격하기 위한 후속 Wave를 고정한다.

목표는 Discord 전용 worker를 그대로 키우는 것이 아니라, self-hosted / personal-use 환경에서 여러 channel transport가 같은 runtime supervisor, durable metadata, pending queue, approval broker, outbound router 경계를 공유하게 만드는 것이다. 외부 채널은 transport와 delivery를 소유하지만, session truth, approval truth, assistant final output 확정은 여전히 core/session store가 소유한다.

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
  - `docs/specs/011-subagent-runtime/SPEC.md`
- 교차 의존:
  - `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`
  - `docs/specs/013-user-interfaces-and-session-ux/SPEC.md`
  - `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`
  - `docs/specs/015-packaging-process-lifecycle-and-upgrades/SPEC.md`
  - `docs/specs/016-verification-matrix-and-release-gates/SPEC.md`

## Dependency Cut

- 001은 one-shot connector와 foreground assistant worker의 역할 분리를 제공한다.
- 001은 `channel discord-worker`가 core command/projection 경계를 통해 turn submit, wait, approval, final reply를 수행하는 최소 vertical slice를 제공한다.
- 012는 service-owned metadata, dedupe, retry, stale reentry, fact-only service signal의 owner 경계를 제공한다.
- 013은 TUI, local API, channel worker가 같은 session projection과 command 의미를 공유해야 함을 제공한다.
- 014는 worker state, retry/backoff, delivery failure를 diagnostics/inspect surface에 안전하게 노출하는 기준을 제공한다.
- 015는 long-running local runtime owner, heartbeat, stale owner, safe shutdown, recovery의 기준을 제공한다.
- 016은 후속 Wave가 release gate와 coverage matrix에 executable evidence를 남겨야 함을 제공한다.

## 범위

- durable worker metadata store
- unified local runner 또는 `shacs-bot run`
- retry/backoff policy for channel transport
- durable pending-message queue
- multi-channel worker abstraction
- Gateway / long-running transport abstraction
- worker state diagnostics와 verification evidence 계획

## 범위 제외

- 멀티테넌트 gateway 운영
- 조직 단위 관리자 inbox
- public webhook hosting 의무화
- Slack/Discord/Telegram/Email 바깥의 추가 채널
- 외부 채널 또는 metadata store가 session truth를 직접 소유하는 구조
- provider/tool runtime을 channel transport 내부에 숨기는 구조
- attachment download와 streaming edit의 상세 구현 계약

Attachment download와 streaming edit은 사용자-visible future work지만, 이 PRD에서는 후속 PRD가 필요한 별도 설계 항목으로만 남긴다. attachment는 artifact, permission, redaction, size/type 제한을 먼저 고정해야 하고, streaming edit은 partial chunk가 session truth가 아니라 transport-level provisional UI라는 점을 먼저 고정해야 한다.

## 현재 구현 상태

### 이미 반영된 것

- `channel telegram-poll`은 Telegram Bot API one-shot polling connector다.
- `channel discord-poll`은 Discord REST one-shot polling connector다.
- `channel discord-worker`는 Discord REST polling 기반 foreground assistant worker다.
- Discord worker는 accepted normal message를 `SubmitUserInput` 경계로 라우팅한다.
- Discord worker는 strict approval command를 `RespondToApproval` 경계로 라우팅하고 pending turn continuation을 수행한다.
- Discord worker는 `TurnCompleted.committed_output`을 Discord safe reply로 전달한다.
- Discord worker는 timeout 또는 outbound send 실패 시 cursor를 전진하지 않고 in-memory pending turn으로 재시도한다.
- Discord worker는 default mention policy에서 `<@bot_user_id> approval ...` prompt를 사용한다.
- Discord worker는 runtime root 아래 durable worker metadata에 pending turn mapping, prompted approval key, outbound delivery receipt를 저장하고 재시작 시 복원한다.

### 아직 남은 것

- retry/backoff state가 durable metadata store에 저장되지 않는다.
- `shacs-bot run` 또는 동등한 unified local runtime owner가 없다.
- Discord REST polling loop는 transient 429/5xx/timeout을 분류해 backoff하지 않는다.
- 열린 turn 중 도착한 메시지는 durable queue에 들어가지 않고 busy 안내와 cursor 미전진으로 처리된다.
- worker runtime이 Discord에 하드코딩돼 있고 Telegram/Slack/Email assistant worker로 확장할 공통 추상화가 없다.
- Gateway, Socket Mode, webhook-like ingest, IMAP polling 같은 long-running transport lifecycle abstraction이 없다.
- channel-neutral `ApprovalBroker`와 `OutboundRouter`가 없다.

### 로컬 근거

- `crates/shacs-cli/src/lib.rs`
- `crates/shacs-cli/src/api.rs`
- `crates/shacs-cli/src/tui.rs`
- `crates/shacs-cli/src/transport.rs`
- `crates/shacs-core/src/core/store.rs`
- `crates/shacs-core/src/core/lifecycle.rs`
- `crates/shacs-contracts/src/service.rs`
- `crates/shacs-surface/src/session_queries.rs`
- `crates/shacs-runtime-adapters/src/discord.rs`
- `crates/shacs-runtime-adapters/src/telegram.rs`
- `crates/shacs-runtime-adapters/src/mailbox.rs`

## 목표 아키텍처

```text
shacs-bot run
  ├─ RuntimeSupervisor
  │   ├─ owner lease / heartbeat
  │   ├─ worker lifecycle
  │   ├─ retry/backoff scheduling
  │   └─ safe shutdown / stale owner recovery
  ├─ ControlPlane
  │   ├─ TUI client
  │   ├─ Local API client
  │   └─ one-shot CLI command client
  ├─ WorkerMetadataStore
  │   ├─ cursors
  │   ├─ pending external message mappings
  │   ├─ outbound delivery receipts
  │   └─ retry/backoff observations
  ├─ PendingMessageQueue
  ├─ ChannelWorkers
  │   ├─ DiscordWorker
  │   ├─ TelegramWorker
  │   └─ future Slack / Email workers
  ├─ ChannelTransports
  │   ├─ REST polling
  │   ├─ Gateway / Socket Mode
  │   └─ webhook-like ingest / IMAP polling
  ├─ SessionTurnDriver
  ├─ ApprovalBroker
  └─ OutboundRouter
```

## 역할 경계

### WorkerMetadataStore

WorkerMetadataStore가 해야 하는 일:

- channel worker cursor를 저장한다.
- external message id와 submitted turn id의 pending mapping을 저장한다.
- approval prompt delivery state를 저장한다.
- outbound reply delivery receipt와 failure를 저장한다.
- retry/backoff observation을 diagnostics가 읽을 수 있게 저장한다.

WorkerMetadataStore가 해서는 안 되는 일:

- session truth를 대체한다.
- assistant final output을 확정한다.
- approval response를 채택한다.
- raw secret 또는 provider token을 저장한다.

### RuntimeSupervisor

RuntimeSupervisor가 해야 하는 일:

- runtime owner lease와 heartbeat를 유지한다.
- worker start, stop, restart, backoff를 관리한다.
- safe shutdown 시 metadata flush를 유도한다.
- stale owner와 interrupted runtime을 inspect/recovery surface에 노출한다.
- TUI, local API, channel worker의 mutation 경계를 단일 owner 전략 아래 정렬한다.

RuntimeSupervisor가 해서는 안 되는 일:

- provider/tool execution 결과를 임의 생성한다.
- session store replay 없이 runtime-local cache를 truth로 사용한다.
- external transport availability를 session completion으로 승격한다.

### RetryBackoffController

RetryBackoffController가 해야 하는 일:

- 429, 5xx, timeout, transport failure를 retryable/non-retryable로 분류한다.
- `Retry-After` 같은 provider hint를 보존한다.
- exponential backoff와 jitter를 적용한다.
- retry exhaustion을 diagnostics에 노출한다.

RetryBackoffController가 해서는 안 되는 일:

- retry attempt를 turn policy retry count로 섞는다.
- outbound retry 성공을 session truth로 승격한다.
- non-idempotent send를 중복 전송하게 만든다.

### PendingMessageQueue

PendingMessageQueue가 해야 하는 일:

- 열린 turn 중 도착한 accepted inbound message를 durable queue에 저장한다.
- external message id 기준 dedupe를 적용한다.
- 현재 turn 완료 뒤 FIFO 또는 명시된 ordering policy로 다음 message를 submit한다.
- queue 상태와 blocked reason을 inspect/diagnostics에 노출한다.

PendingMessageQueue가 해서는 안 되는 일:

- session이 없는데 임의로 session을 생성한다.
- 열린 turn이 있는데 새 turn을 조용히 강제 개시한다.
- queued message를 preserved context에 몰래 합친다.

### ChannelWorker abstraction

ChannelWorker가 해야 하는 일:

- provider-specific payload를 normalized inbound event로 변환한다.
- sender/channel allowlist와 mention/open policy를 적용한다.
- channel-specific outbound send/edit/reply를 수행한다.
- cursor, delivery receipt, retry observation을 WorkerMetadataStore에 기록한다.
- core command/projection 경계를 통해 SessionTurnDriver, ApprovalBroker, OutboundRouter와 연결한다.

ChannelWorker가 해서는 안 되는 일:

- provider/tool execution 직접 수행
- `TurnCompleted` 직접 생성
- approval policy 직접 결정
- raw transport payload를 session truth에 직접 저장

### GatewayTransport abstraction

GatewayTransport가 해야 하는 일:

- long-running connection lifecycle을 관리한다.
- heartbeat, reconnect, resume, backoff를 수행한다.
- inbound event를 durable queue 또는 worker ingress boundary로 넘긴다.
- shutdown과 runtime ownership을 따른다.
- channel-specific protocol detail을 adapter 내부로 격리한다.

GatewayTransport가 해서는 안 되는 일:

- provider execution 완료까지 event handler를 붙잡는다.
- session store를 직접 mutate한다.
- Discord 전용 Gateway 모델을 모든 channel에 강제한다.

Discord Gateway는 이 abstraction의 첫 구현 후보일 뿐이다. Slack Socket Mode, Telegram long polling 또는 webhook-like ingest, Email IMAP polling도 같은 long-running transport family로 다뤄야 한다.

## 구현 웨이브

### Wave 1. Durable worker metadata

- cursor file 경계를 WorkerMetadataStore로 승격한다.
- external message id, submitted turn id, reply target, outbound delivery state를 durable하게 저장한다.
- crash after fetch before submit, crash after submit before reply, duplicate external id를 테스트한다.
- metadata store가 session truth를 대체하지 못하도록 타입과 테스트로 막는다.

### Wave 2. Unified local runner

- `shacs-bot run` 또는 동등한 local runtime owner command를 추가한다.
- runtime owner lease, heartbeat, safe shutdown, stale recovery를 연결한다.
- TUI/API/channel worker가 같은 owner process 또는 control plane 아래에서 mutation을 수행하도록 정렬한다.
- active owner 중복 실행과 stale owner recovery를 테스트한다.

### Wave 3. Retry/backoff policy

- Discord REST polling/send에서 429, 5xx, timeout, transport failure를 분류한다.
- `ProviderHttpResponse` 또는 channel response metadata에 retry hint를 보존한다.
- retry/backoff state를 WorkerMetadataStore와 diagnostics에 기록한다.
- retry exhaustion, duplicate outbound 방지, non-retryable error 처리를 테스트한다.

### Wave 4. Durable pending-message queue

- busy 안내 + cursor 미전진만 있는 현재 정책을 durable pending queue로 확장한다.
- 열린 turn 중 도착한 accepted message를 queue에 넣고 current turn 완료 뒤 순서대로 submit한다.
- queue dedupe key, ordering, cancellation, stale session handling을 구현한다.
- queued message가 session truth를 직접 바꾸지 않는지 테스트한다.

### Wave 5. Multi-channel worker abstraction

- Discord worker의 common turn-driving logic을 ChannelWorker / SessionTurnDriver / ApprovalBroker / OutboundRouter 경계로 분리한다.
- Telegram worker를 두 번째 구현 후보로 삼아 channel-neutral contract를 검증한다.
- Discord 2000자, Telegram 4096자 같은 outbound limit을 channel adapter별 policy로 분리한다.
- common worker contract test를 추가한다.

### Wave 6. Gateway / long-running transport abstraction

- GatewayTransport abstraction을 추가한다.
- Discord Gateway를 첫 adapter 후보로 구현하되, abstraction은 Discord 전용이 아니어야 한다.
- heartbeat, reconnect, resume, identify/rate-limit, shutdown을 테스트한다.
- inbound handler는 durable queue enqueue까지만 수행하고 provider execution을 기다리지 않게 만든다.

## Verification Evidence 계획

- 단위 테스트: WorkerMetadataStore cursor/pending/outbound receipt roundtrip
- 내구성 테스트: crash after fetch before submit, crash after submit before reply, restart after outbound failure
- 중복 테스트: duplicate external message id, duplicate approval response, duplicate outbound retry
- lifecycle 테스트: active owner duplicate start rejection, stale owner recovery, safe shutdown metadata flush
- retry 테스트: 429 retry-after, 5xx backoff, timeout backoff, retry exhaustion diagnostics
- queue 테스트: open turn 중 pending enqueue, FIFO submit, cancellation, stale session rejection
- abstraction 테스트: Discord/Telegram worker contract parity
- Gateway 테스트: heartbeat timeout, reconnect, resume, identify backoff, handler enqueue-only
- release gate: 새 테스트와 coverage matrix evidence locator가 `scripts/release-gate`에 연결돼야 한다.

## Open Risks

- service metadata가 session truth처럼 오용될 수 있다.
- pending queue가 사용자 메시지를 영구 보류할 수 있다.
- unified runtime owner가 기존 CLI/TUI/API mutation UX와 충돌할 수 있다.
- retry/backoff가 duplicate outbound delivery를 만들 수 있다.
- Gateway reconnect/resume 오류가 silent message loss로 이어질 수 있다.
- channel-neutral abstraction을 너무 일찍 일반화하면 Discord/Telegram의 실제 transport 차이를 숨길 수 있다.

## 종료 기준

- restart 이후 cursor, pending message, outbound delivery state가 복구된다.
- `shacs-bot run` 또는 동등한 unified owner 전략이 구현되거나 명시적 제한으로 고정된다.
- retry/backoff와 pending queue가 session truth를 직접 변경하지 않는다.
- Discord worker logic이 channel-neutral worker 경계로 분리되어 Telegram worker 같은 두 번째 구현을 받을 수 있다.
- GatewayTransport는 provider execution을 직접 수행하지 않고 durable ingress boundary까지만 책임진다.
- diagnostics/inspect surface에서 worker lifecycle, queue, retry, delivery failure를 설명할 수 있다.
- release-gate와 coverage matrix가 후속 Wave evidence를 추적한다.
