# PRD 001. channel worker runtime

## 목표

이 문서는 `docs/specs/012-runtime-services/SPEC.md`의 하위 실행 문서다. Discord, Telegram 같은 외부 채널을 one-shot mailbox connector가 아니라 장기 실행 assistant surface로 다루기 위한 runtime worker 구조를 고정한다.

이번 PRD의 목표는 `shacs-bot`을 단발성 CLI 명령 묶음이 아니라, 로컬에서 계속 떠 있는 self-hosted assistant runtime으로 확장하는 것이다. 채널 worker는 외부 메시지를 받고 답장을 보낼 수 있지만, 세션 truth, approval truth, assistant 응답 확정은 여전히 `MainOrchestrator`와 session store가 소유한다.

## SPEC 입력

- 주관 spec: `docs/specs/012-runtime-services/SPEC.md`
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

## 참고 구조 입력

이 PRD는 아래 구조를 shacs-bot의 기존 core/session 계약에 맞게 축소 적용한다.

- nanobot: channel은 직접 agent를 실행하지 않고 inbound/outbound message bus를 통해 agent loop와 분리한다.
- Claude Code: assistant surface는 context 수집, action, verification, approval/user intervention이 반복되는 agent loop다.
- OpenCode: TUI와 API는 같은 로컬 runtime/server에 붙는 client surface로 보는 편이 안전하다.
- OpenClaw: 장기 실행 gateway가 session state와 queue를 소유하고, channel plugin은 transport adapter 역할에 머문다.

참고 구조에서 가져올 원칙은 구현 세부 라이브러리가 아니라 아래 경계다.

1. channel worker는 외부 transport를 소유한다.
2. runtime supervisor는 process lifecycle과 worker lifecycle을 소유한다.
3. session truth는 core/session store가 소유한다.
4. approval은 중앙 broker 또는 core projection이 소유한다.
5. outbound delivery는 assistant 완료 사실을 관찰한 뒤 transport로 전달한다.

## Dependency Cut

- 012는 mailbox, background worker, dedupe, service-owned metadata 경계를 제공한다.
- 013은 CLI/TUI/local API가 동일한 session projection과 command 의미를 공유해야 함을 제공한다.
- 015는 장기 실행 process ownership, stale owner, recovery UX의 기준을 제공한다.
- 007은 channel worker가 새 턴을 열거나 wake할 때 최종 판단자가 `MainOrchestrator`임을 제공한다.
- 002는 channel worker가 직접 상태를 수정하지 않고 command/effect/event 경계로 재진입해야 함을 제공한다.

## 범위

- 장기 실행 `ChannelWorker` 역할 정의
- `RuntimeSupervisor`와 worker lease/heartbeat 경계 정의
- one-shot connector와 assistant worker의 역할 분리
- Discord/Telegram inbound message를 assistant turn으로 연결하는 경로 정의
- pending approval을 외부 채널로 노출하고 응답을 core에 재진입시키는 경로 정의
- assistant final output을 외부 채널로 전달하는 outbound router 경계 정의
- cursor, delivery receipt, pending outbound 같은 service-owned metadata 정의
- worker restart, duplicate delivery, busy/open-turn 상황의 안전 규칙 정의

## 범위 제외

- 멀티테넌트 gateway 운영
- 조직 단위 관리자 inbox
- public webhook hosting 의무화
- Slack/Discord/Telegram/Email 바깥의 추가 채널
- 외부 채널이 session truth를 직접 소유하는 구조
- provider runtime을 Discord/Telegram worker 내부에 숨기는 구조

## 현재 구현 상태

### 이미 반영된 것

- `channel telegram-poll`은 Telegram Bot API one-shot polling connector다.
- `channel discord-poll`은 Discord REST one-shot polling connector다.
- `channel discord-worker`는 Discord REST polling 기반 foreground assistant worker로 구현돼 있다.
- 두 connector는 mailbox message와 mailbox approval response를 core 경계로 라우팅한다.
- mailbox ingress는 dedupe key를 가진 fact-only service command로 재진입한다.
- `ask`, `session wait`, local API `/wait`, TUI는 bounded loop 또는 event loop를 이미 가진다.

### 아직 남은 것

- Telegram connector는 assistant 답변까지 책임지는 장기 실행 worker가 아니다.
- `discord-poll`의 mailbox message 경로는 외부 메시지를 preserved context로 합치지만, 그 자체로 assistant turn을 자동 시작하지 않는다.
- Discord worker는 `TurnCompleted.committed_output`을 Discord safe reply로 전달하지만, channel-neutral outbound router는 아직 없다.
- pending approval projection을 외부 채널 prompt로 내보내는 중앙 broker가 없다.
- Discord worker는 cursor를 처리 후 전진시키지만, cursor와 outbound delivery receipt를 함께 다루는 durable service metadata store는 아직 없다.
- TUI/API/channel worker가 동시에 mutating surface로 작동할 때 단일 runtime owner가 중재하는 구조가 아직 없다.

### 로컬 근거

- `crates/shacs-cli/src/lib.rs`
- `crates/shacs-cli/src/api.rs`
- `crates/shacs-cli/src/tui.rs`
- `crates/shacs-surface/src/session_queries.rs`
- `crates/shacs-core/src/core/orchestrator.rs`
- `crates/shacs-core/src/core/message.rs`
- `crates/shacs-runtime-adapters/src/discord.rs`
- `crates/shacs-runtime-adapters/src/telegram.rs`
- `docs/specs/012-runtime-services/prds/000-service-reentry-and-dedup.md`

## 목표 아키텍처

```text
shacs-bot run
  ├─ RuntimeSupervisor
  │   ├─ runtime lease / heartbeat
  │   ├─ worker lifecycle
  │   └─ safe shutdown / stale owner recovery
  ├─ ControlPlane
  │   ├─ TUI client
  │   ├─ Local API client
  │   └─ one-shot CLI command client
  ├─ ChannelWorkers
  │   ├─ DiscordWorker
  │   ├─ TelegramWorker
  │   └─ future Slack / Email workers
  ├─ MessageBus
  │   ├─ inbound channel messages
  │   ├─ outbound assistant replies
  │   ├─ approval prompts
  │   └─ delivery receipts
  ├─ SessionTurnDriver
  │   ├─ SubmitUserInput
  │   ├─ wait_poll_state
  │   ├─ provider/tool execution
  │   └─ TurnCompleted output observation
  ├─ ApprovalBroker
  └─ OutboundRouter
```

초기 구현은 이 전체 구조를 한 번에 만들지 않는다. 먼저 one-shot connector를 유지하면서 `channel discord-worker` 같은 foreground worker를 추가하고, 이후 `shacs-bot run`이 여러 worker와 control plane을 소유하는 구조로 승격한다.

## 역할 경계

### RuntimeSupervisor

RuntimeSupervisor가 해야 하는 일:

- 장기 실행 runtime owner lease 또는 heartbeat 유지
- worker 시작, 종료, 재시작, backoff 관리
- process lifecycle blocker 감지
- 안전 종료 시 worker metadata flush 유도
- stale owner 상태를 inspect/recovery로 드러내기

RuntimeSupervisor가 해서는 안 되는 일:

- assistant 응답 본문을 직접 확정
- approval 결과를 임의 채택
- session store replay 없이 runtime-local cache를 truth로 사용

### ChannelWorker

ChannelWorker가 해야 하는 일:

- 외부 transport 연결 유지
- sender/channel allowlist와 mention/open 정책 적용
- provider-specific payload를 normalized inbound event로 변환
- 외부 message id 기준 dedupe 후보 제공
- outbound envelope를 provider-specific send/edit/reply로 전달
- transport receipt, cursor, delivery failure를 service metadata로 기록

ChannelWorker가 해서는 안 되는 일:

- provider/tool execution 직접 수행
- `TurnCompleted`를 직접 생성
- approval policy 직접 결정
- 세션이 없는데 임의로 세션을 생성
- 열린 턴이 있는데 새 턴을 조용히 강제 개시

### SessionTurnDriver

SessionTurnDriver가 해야 하는 일:

- accepted inbound message를 `SubmitUserInput` 계열 command로 변환
- provider/tool/subagent runtime을 기존 core 경계로 실행
- `wait_poll_state` 의미를 재사용해 completed, approval, recovery, aborted를 구분
- timeout을 완료로 승격하지 않고 관찰 timeout으로만 다루기

SessionTurnDriver가 해서는 안 되는 일:

- channel transport 상태를 session truth로 저장
- external message id만 보고 assistant output을 확정

### ApprovalBroker

ApprovalBroker가 해야 하는 일:

- pending approval projection을 외부 채널 prompt로 변환
- 허용 가능한 응답 집합을 그대로 노출
- 외부 approval command를 `ReceiveChannelApprovalResponse` 경계로 재진입
- stale approval response와 duplicate response를 core 결과에 따라 처리

ApprovalBroker가 해서는 안 되는 일:

- auto-approve 기본값 제공
- natural language approval 추론
- effect 완료와 approval 수락을 같은 의미로 표시

### OutboundRouter

OutboundRouter가 해야 하는 일:

- `TurnCompleted.committed_output`을 channel별 outbound envelope로 변환
- Discord 2000자, Telegram 4096자 같은 transport 제한에 맞춰 chunking
- reply target, allowed mentions, parse mode 같은 transport 안전 옵션 적용
- delivery success/failure를 service metadata로 기록

OutboundRouter가 해서는 안 되는 일:

- 전송 성공을 session truth로 승격
- 전송 실패 때문에 completed turn을 미완료로 되돌리기
- ack를 assistant final answer처럼 표시

## 실행 모델

### 초기 foreground worker

초기 명령은 아래처럼 별도 worker로 둔다.

```sh
shacs-bot channel discord-worker \
  --session-id session-1 \
  --token-ref discord.default \
  --channel-id 123456789012345678 \
  --bot-user-id 999999999999999999 \
  --allow-from YOUR_DISCORD_USER_ID \
  --allow-channel 123456789012345678 \
  --workspace-root /tmp/ws
```

이 명령은 foreground process이며, `discord-poll`과 달리 다음 lifecycle을 수행한다.

1. 외부 메시지 수신
2. allowlist/mention policy 적용
3. approval command 우선 판별
4. 일반 메시지는 assistant turn으로 submit
5. pending approval이면 channel prompt 전송
6. turn completion까지 관찰
7. final output을 외부 채널에 reply
8. durable metadata를 업데이트한 뒤 다음 메시지 처리

### 중기 unified runtime

중기 목표는 아래 명령이다.

```sh
shacs-bot run --channel discord --channel telegram --workspace-root /tmp/ws
```

이 모드에서는 TUI와 local API가 session store를 직접 mutate하는 별도 owner가 아니라 runtime control plane client로 붙는다. 단일 runtime owner가 worker, provider execution, approval, outbound delivery를 중재한다.

## Discord transport 선택

### REST polling

REST polling은 초기 worker와 디버그에 허용한다.

- 장점: 현재 `DiscordLongPollingAdapter`를 재사용할 수 있다.
- 장점: self-hosted local 환경에서 public endpoint가 필요 없다.
- 한계: 실시간성이 낮고 rate-limit/backoff 처리가 필요하다.
- 한계: assistant surface의 장기 주 실행 모델로는 Gateway보다 약하다.

### Gateway

Gateway는 장기 목표다.

- 장점: Discord가 메시지 이벤트를 수신하라고 제공하는 기본 모델이다.
- 장점: polling cursor보다 이벤트 수신과 reconnect/resume semantics가 명확하다.
- 요구: heartbeat, reconnect, resume, identify 제한, message content intent를 다뤄야 한다.
- 요구: Gateway handler 안에서 provider 실행을 오래 붙잡지 않고 내부 queue로 넘겨야 한다.

### Interactions / webhooks

Interactions와 webhooks는 기본 경로가 아니다.

- slash command UX에는 유용하다.
- public HTTP endpoint, signature verification, 3초 initial response 제한을 요구한다.
- self-hosted personal-use 기본값으로는 Gateway 또는 local polling worker가 더 단순하다.

## 메시지 처리 규칙

### 일반 메시지

일반 메시지는 아래 조건을 만족할 때만 assistant turn으로 submit한다.

- sender가 `allow_from`에 포함되거나 `*`가 허용됨
- channel이 `allow_channel`에 포함됨
- guild channel의 `mention` 정책에서는 bot mention이 포함됨
- self message가 아님
- message id가 worker metadata에서 이미 처리 완료된 값이 아님

submit 전에 외부 message id와 session id의 pending mapping을 기록해야 한다. crash 이후 재시작 시 같은 외부 메시지를 중복 submit하지 않기 위해서다.

### 열린 턴이 있는 경우

같은 세션에 열린 턴이 있으면 worker는 새 턴을 조용히 열면 안 된다. 허용되는 초기 정책은 아래 둘 중 하나다.

1. busy 안내를 보내고 cursor를 전진하지 않는다.
2. worker-owned pending queue에 저장하고 현재 턴 완료 뒤 순서대로 submit한다.

초기 구현은 1을 기본으로 둔다. 2는 durable queue가 준비된 뒤 도입한다.

### approval command

approval command는 일반 메시지보다 먼저 판별한다.

```text
approval <turn_id> <approval_request_id> <approve-once|deny|cancel-turn>
```

이 형식은 strict whole-message command다. Discord guild 채널에서 mention 정책을 쓰는 worker는 안내 문구에 `<@bot_user_id> approval ...` 형태를 출력해야 하며, adapter는 선행 bot mention을 제거한 뒤 동일한 strict command로 파싱한다. 자연어 승인은 파싱하지 않는다. malformed approval command는 mailbox context로 넣지 않는다.

### acknowledgement

ack는 transport-level receipt다.

- ack는 assistant final answer가 아니다.
- ack는 approval accepted가 아니다.
- ack는 tool/effect completed가 아니다.
- ack 실패는 session truth를 바꾸지 않는다.

## metadata와 durability

worker metadata는 service-owned metadata다.

초기 Discord worker metadata는 최소한 아래를 가져야 한다.

- `session_id`
- `channel_id`
- `token_ref`
- last accepted external message id
- pending external message id
- pending turn id
- outbound delivery status
- retry/backoff state
- updated timestamp

cursor는 fetch한 최고 메시지 기준이 아니라, durable하게 accepted/rejected/queued 처리한 지점 이후로만 전진해야 한다. 그렇지 않으면 crash 이후 사용자의 메시지를 영구히 건너뛸 수 있다.

## 정상 시퀀스

### Discord 일반 메시지

1. DiscordWorker가 메시지를 수신한다.
2. Worker가 allowlist, channel, mention, self-loop 정책을 검사한다.
3. Worker가 external message id pending metadata를 기록한다.
4. SessionTurnDriver가 `SubmitUserInput`을 발행한다.
5. Core가 turn을 열고 provider/tool runtime을 실행한다.
6. Worker가 `wait_poll_state` 의미로 상태를 관찰한다.
7. `TurnCompleted.committed_output`이 생긴다.
8. OutboundRouter가 Discord reply를 전송한다.
9. Worker가 outbound delivery metadata와 cursor를 전진시킨다.

### Discord approval

1. Core가 pending approval projection을 만든다.
2. ApprovalBroker가 Discord approval prompt를 보낸다.
3. 사용자가 strict approval command로 응답한다.
4. DiscordWorker가 approval command를 일반 context보다 먼저 감지한다.
5. Worker가 `ReceiveChannelApprovalResponse`를 발행한다.
6. Core가 stale/duplicate/valid 여부를 판단한다.
7. accepted이면 기존 turn이 계속 진행된다.
8. OutboundRouter가 최종 assistant reply를 전송한다.

## 실패 시퀀스

### worker crash after fetch before submit

- cursor를 전진하지 않는다.
- 재시작 후 같은 external message id를 다시 본다.
- pending metadata가 없으면 새로 pending 처리한다.

### worker crash after submit before outbound reply

- pending external message id와 turn id mapping을 확인한다.
- session event log에서 turn completion 여부를 확인한다.
- 완료되어 있고 outbound delivery가 없으면 reply를 재전송하거나 중복 전송 위험을 diagnostics에 노출한다.

### duplicate external message delivery

- 같은 `source_id + external_message_id`는 mailbox/service dedupe와 worker metadata 둘 다에서 idempotent해야 한다.
- 중복 메시지는 새 turn을 열면 안 된다.

### outbound delivery failure

- completed turn을 되돌리지 않는다.
- delivery failure metadata를 남긴다.
- retry 가능하면 backoff 후 재시도한다.
- retry exhaustion은 diagnostics/projection으로 보여준다.

## 금지 패턴

- `discord-poll --loop` 하나에 submit, approval, wait, outbound delivery를 모두 섞어 one-shot JSON 계약을 깨는 것
- channel worker가 직접 assistant response event를 쓰는 것
- approval text를 자연어로 추론하는 것
- ack를 final answer처럼 보내는 것
- cursor를 fetch 기준으로 먼저 전진시키는 것
- TUI/API/Discord worker가 각자 session store를 동시에 mutating owner로 사용하는 것
- public webhook 운영을 self-hosted 기본값으로 강제하는 것

## 구현 웨이브

### Wave 1. One-shot connector 보존과 공통 처리 분리

- `discord-poll`, `telegram-poll`의 one-shot 계약을 유지한다.
- poll, normalize, approval parse, route, ack 처리 코드를 worker에서 재사용 가능한 내부 함수로 분리한다.
- 문서에서 one-shot connector와 worker의 역할을 분리한다.

### Wave 2. Discord foreground worker

- `channel discord-worker`를 추가한다.
- REST polling 기반으로 시작하되 interval, backoff, shutdown, cursor durability를 갖춘다.
- 일반 메시지를 assistant turn으로 submit하고 completion을 Discord reply로 보낸다.
- pending approval prompt와 strict approval response를 연결한다.
- 열린 턴이 있을 때 busy 안내 또는 no-cursor-advance 정책을 적용한다.

### Wave 3. Worker metadata store

- cursor, pending mapping, outbound delivery receipt를 같은 metadata 경계에 저장한다.
- crash/restart 후 duplicate submit과 lost reply를 줄인다.
- diagnostics와 inspect projection에서 worker 상태를 읽을 수 있게 한다.

### Wave 4. Unified local runtime

- `shacs-bot run` 또는 동등한 runtime supervisor 명령을 추가한다.
- Discord/Telegram worker와 local API를 같은 owner process 아래 둔다.
- TUI는 장기적으로 store 직접 mutation이 아니라 runtime control plane에 붙는 client가 된다.

### Wave 5. Discord Gateway worker

- REST polling worker를 유지하되 Gateway worker를 추가한다.
- heartbeat, reconnect, resume, identify 제한, message content intent를 검증한다.
- Gateway event handler는 내부 queue enqueue만 수행하고 provider execution을 직접 기다리지 않는다.

## Verification Evidence 계획

- CLI parser 테스트: `channel discord-worker` 옵션 파싱
- 단위 테스트: worker metadata가 cursor와 pending mapping을 함께 보존
- 단위 테스트: malformed approval command가 mailbox context로 들어가지 않음
- 통합 테스트: Discord inbound message가 `SubmitUserInput` 경로로 turn을 열고 `TurnCompleted` 후 outbound envelope를 생성
- 통합 테스트: pending approval prompt가 생성되고 strict approval response가 기존 turn을 계속 진행시킴
- 내구성 테스트: crash after fetch before submit에서 cursor가 전진하지 않음
- 내구성 테스트: crash after submit before outbound reply에서 completion과 delivery receipt를 재조정
- 중복 테스트: duplicate external message id가 새 turn을 열지 않음
- 안전성 테스트: 열린 턴 중 새 메시지가 조용히 cursor advance되지 않음
- transport 테스트: Discord reply가 `allowed_mentions.parse = []`, `replied_user = false`를 유지
- release gate: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`

## Open Risks

- runtime owner 없이 worker와 TUI/API가 동시에 mutation하면 session store race가 생길 수 있다.
- REST polling은 Gateway보다 rate-limit/backoff와 message content intent 제약에 취약하다.
- outbound delivery receipt가 약하면 completed answer를 Discord에 중복 전송하거나 영구 누락할 수 있다.
- pending queue 없이 busy 안내만 쓰면 사용자가 연속 메시지를 보낼 때 UX가 딱딱해질 수 있다.
- Gateway 구현을 직접 작성하면 reconnect/resume 오류가 assistant availability를 해칠 수 있다.
- `discord-poll`과 `discord-worker`의 책임이 문서와 CLI help에서 명확히 갈라지지 않으면 사용자가 one-shot connector를 assistant surface로 오해할 수 있다.

## 종료 기준

- one-shot connector와 장기 실행 worker의 역할이 CLI, docs, tests에서 분리된다.
- Discord worker가 일반 메시지, approval response, final reply delivery를 하나의 assistant lifecycle로 처리한다.
- channel worker는 세션 truth를 직접 쓰지 않고 core command/projection 경계를 사용한다.
- cursor는 durable processing 이후에만 전진한다.
- worker restart와 duplicate delivery가 새 turn 중복 생성이나 lost message를 만들지 않는다.
- TUI/API/channel worker 동시 실행에 대한 runtime ownership 전략이 구현 또는 명시적 제한으로 고정된다.
