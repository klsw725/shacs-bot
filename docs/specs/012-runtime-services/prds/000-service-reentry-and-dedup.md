# PRD 000. service reentry and dedup

## 목표

이 문서는 `docs/specs/012-runtime-services/SPEC.md`의 하위 실행 문서다. queue, scheduler, mailbox, hooks, background worker를 제품 기능이 아니라 재진입 보조 서비스로 다루며, dedupe, retry, wake, failure-safe reentry 구현 계획을 고정한다.

이번 PRD의 목표는 어떤 서비스도 세션 truth를 직접 건드리지 못하게 하면서, 중복 전달, 재시작, 지연 전달 상황에서도 오케스트레이터가 안정적으로 같은 결론을 내리게 만드는 것이다.

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

## Dependency Cut

- 002는 service command envelope와 correlation 규약의 기반이 된다.
- 006은 replay 가능한 truth source를 제공하므로 서비스 메타데이터가 truth를 대체하면 안 된다.
- 007은 wake 이후 resume, ignore, retry, reject 결정을 가진다.
- 011은 subagent completion과 background completion이 같은 reentry 규약 아래 돌아와야 함을 요구한다.
- 015는 service restart와 interrupted lifecycle에서 stale wake가 정상 가능성임을 제공한다.

## 범위

- queue, scheduler, mailbox, hooks, background worker의 emit 범위 구현
- service-owned metadata와 session truth 경계 구현
- dedupe key와 idempotent reentry 처리 구현
- wake command와 resume 판단 입력 구현
- service restart 이후 duplicate delivery, stale wake 처리 구현
- 관측과 inspect를 위한 correlation metadata 연결
- mailbox adapter 범위를 Slack, Discord, Telegram, Email, WhatsApp bridge 다섯 채널로 제한
- Email mailbox adapter는 이미 추출된 메시지 필드 정규화까지만 포함하고 IMAP/SMTP/MIME/network/provider-specific API는 제외

## 범위 제외

- 특정 벤더 큐나 cron 선택
- Slack, Discord, Telegram, Email, WhatsApp bridge 바깥의 추가 채널 adapter
- 관리자 inbox UI
- 멀티노드 스케줄러 합의
- 외부 조직용 webhook 운영 시스템

## 현재 구현 상태

### 이미 반영된 것

- queue, scheduler, mailbox, hooks, background worker service command envelope와 dedupe 경계가 core service 모델에 구현돼 있다.
- service reentry는 fact-only command로 처리되며 duplicate delivery, stale wake, metadata loss 이후 current turn 보호, non-mailbox dedupe marker replay가 검증된다.
- Slack, Email adapter는 network-free normalizer 또는 strict approval parser 범위로 구현돼 있고, Telegram과 Discord는 CLI one-shot polling connector를 통해 같은 mailbox 경계로 라우팅된다. 장기 실행 channel worker runtime은 후속 PRD 범위로 확장됐으며, 현재 CLI runtime은 WebSocket과 Slack/Discord/Telegram/Email/WhatsApp bridge transport를 `MessageBus`와 `ChannelManager` 경계로 연결한다. 외부 agent processor와 API/CLI path는 같은 process-local `SessionTurnLock`을 공유해 session key별 turn을 직렬화하고, 같은 session follow-up을 in-memory pending queue에 보관해 현재 turn 뒤에 이어 처리한다. 이 queue와 lock은 process-local이며 PRD 002 Wave 4의 durable pending-message queue, cancellation persistence, restart replay를 의미하지 않는다. Built-in slash command는 `CommandRouter`에서 priority/exact/prefix로 분류되고 `/status`, `/stop`, `/restart` priority command만 active turn lock 전 처리된다. Exact/prefix command는 일반 user turn과 같은 lock 경계를 통과한다. Agent runner는 mid-turn injection을 model iteration 사이와 finalization 직전에 drain하되 cycle cap으로 무한 follow-up을 막는다. WebSocket provider progress는 coalesced `delta`/`stream_end` event로 노출하되 최종 `message` event를 authoritative answer로 유지한다. Telegram/Discord/Slack provider progress는 in-process preview message로 전송하고 최종 assistant answer로 같은 message를 갱신한다. Telegram topic, Slack thread, Discord thread, Email subject/reply context는 outbound reply metadata로 이어진다. Telegram offset, Discord REST last id, Discord Gateway resume state, Email IMAP seen UID + UIDVALIDITY hint, outbound delivery status는 runtime metadata JSON으로 best-effort 보존하지만 durable queue나 exactly-once delivery를 의미하지 않는다.
- accepted service/mailbox events는 `service_correlation_id`를 observability projection으로 보존한다.

### 아직 남은 것

- hosted webhook, Telegram webhook, provider-specific 운영 hardening은 이 PRD 범위 밖이다. Discord REST one-shot polling CLI는 현재 포함 범위다. 장기 실행 assistant channel worker와 unified runtime supervisor 설계는 `docs/specs/012-runtime-services/prds/001-channel-worker-runtime.md`에서 별도 확장 범위로 다룬다.
- service metadata와 session truth 경계는 유지되지만, 장기 운영용 metadata storage 고도화는 아직 별도 확장 범위다.

### 로컬 근거

- `crates/shacs-command/src/lib.rs`
- `crates/shacs-command/tests/router.rs`
- `crates/shacs-core/src/runtime/agent_loop.rs`
- `crates/shacs-core/src/runtime/runner.rs`
- `crates/shacs-core/tests/runtime_loop.rs`
- `crates/shacs-core/tests/runtime_agent.rs`
- `crates/shacs-channels/src/lib.rs`
- `crates/shacs-api/src/lib.rs`

## TDD 계획

1. 서비스별 dedupe key 생성과 envelope validation 단위 테스트를 만든다.
2. 같은 key가 두 번 들어와도 turn이 다시 열리지 않는 idempotency 테스트를 추가한다.
3. scheduler, mailbox, worker가 wake command를 보내고 오케스트레이터가 resume 여부를 판단하는 통합 테스트를 추가한다.
4. service restart 뒤 duplicate delivery, stale wake, already-closed turn 재진입 테스트를 추가한다.
5. emit 금지 command를 서비스가 만들 수 없도록 타입 또는 검증 테스트를 추가한다.

## 구현 웨이브

### Wave 1. Service envelope와 emit 범위 고정

- 서비스별 command envelope 스키마와 공통 correlation 필드를 정의한다.
- queue, scheduler, mailbox, hooks, background worker가 emit 가능한 command 집합을 타입으로 제한한다.
- 세션 상태 직접 변경 command와 privileged command는 생성 단계에서 막는다.

### Wave 2. Dedupe와 metadata 경계 구현

- 서비스별 dedupe key 계산기를 구현한다.
- service-owned metadata 저장소와 session truth 저장소를 분리한다.
- 오케스트레이터 재진입 시 processed marker 검사와 idempotent 처리 경로를 구현한다.

### Wave 3. Wake and resume 경로 연결

- wake command를 단순 "확인 필요" 신호로 제한한다.
- 오케스트레이터가 replay, open turn, pending effect, stale 여부를 보고 resume 또는 ignore를 결정하게 만든다.
- mailbox approval response와 background completion도 같은 재진입 규약으로 묶는다.

### Wave 4. 재시작, 중복 전달, 관측 가능성 회귀 검증

- 서비스 재시작 후 이전 delivery 재전송을 허용하되 truth가 변하지 않게 만든다.
- late service signal과 stale wake가 inspect, diagnostics, trace에서 구분되도록 연결한다.
- duplicate delivery, missed fire, cancelled work, stale background completion 테스트를 묶는다.

## Verification Evidence

- 단위/통합 테스트: `crates/shacs-command/tests/router.rs`가 built-in slash command priority/exact/prefix dispatch boundary를 검증한다.
- 통합 테스트: `crates/shacs-core/tests/runtime_loop.rs`가 active session locking, priority command bypass, same-session pending follow-up drain, channel chat/session key preservation, registered cancellation observation을 검증한다.
- runtime evidence: `crates/shacs-channels/src/lib.rs`, `crates/shacs-api/src/lib.rs`, `crates/shacs-cli/src/lib.rs` inline tests가 channel/API/CLI service entrypoint wiring을 검증한다.
- 안전성 테스트: privileged command bypass 불가, closed turn reopen 방지
- 현 slice matrix는 별도 contracts crate가 아니라 실제 Cargo tests와 문서 locator를 기준으로 유지한다.

## Open Risks

- 서비스 메타데이터와 truth 저장소의 경계가 흐리면 replay correctness가 깨질 수 있다.
- dedupe key 설계가 약하면 다른 이벤트를 중복으로 잘못 묶을 수 있다.
- restart 직후 stale wake 폭주가 있으면 diagnostics는 많아지지만 실제 상태는 안 바뀌는 상황을 잘 설명해야 한다.
- 참고 메모: service reentry의 dedupe/stale 판단은 007의 ingress arbitration과 shared correlation 계약에 의존하므로, 서비스 레벨에서 우선순위를 독자 정의하면 안 된다.

## 종료 기준

- 모든 runtime service 결과가 command envelope로만 재진입한다.
- 서비스 메타데이터가 없어도 session truth replay가 가능하다.
- duplicate delivery와 stale wake가 truth를 재적용하거나 닫힌 턴을 되살리지 않는다.
- emit 금지 command를 서비스가 만들 수 없거나 즉시 거절된다.
- 012와 016이 요구하는 단위, 통합, 내구성 검증 증거가 확보된다.
