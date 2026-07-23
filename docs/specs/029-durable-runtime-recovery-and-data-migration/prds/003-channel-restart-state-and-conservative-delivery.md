# PRD 003: channel restart state and conservative delivery

## 목표

지원 channel의 cursor, inbound/outbound pending, delivery hint, duplicate hint를 durable record로 보존하되 이를 session truth나 exactly-once delivery로 과장하지 않는다.

Status: Complete (Scoped). 기존 runtime metadata JSON key를 보존하면서 typed restart envelope와 conservative inspect/status projection을 추가했다. 이 상태는 channel delivery hint이며 session truth나 exactly-once delivery 주장이 아니다.

## 범위

1. Channel-neutral restart state envelope
2. Telegram, Discord, Slack, Email, WhatsApp, WebSocket cursor/resume 의미
3. Pending inbound와 pending outbound work record
4. Delivery sent/failed/unknown hint
5. Inbound dedupe hint와 conservative redelivery
6. Channel status와 runtime inspect projection

## 비범위

- 새 channel adapter
- remote broker
- guaranteed exactly-once delivery
- channel hint를 session final answer truth로 승격하는 동작

## SPEC 입력

1. 필수 선행 PRD: `002-durable-work-queue-scheduler-retry-and-cancellation.md`
2. Current channel baseline: `../../012-runtime-services/SPEC.md`
3. Session truth boundary: `../../001-session-kernel/SPEC.md`

## Dependency Cut

1. Channel cursor와 transport acknowledgement는 delivery hint다.
2. Final assistant message/session event는 orchestrator commit 이후에만 truth가 된다.
3. Channel별 semantics는 common envelope를 소비하지만 동일한 cursor 방식으로 강제하지 않는다.
4. Dedupe hint mismatch는 메시지를 조용히 성공 처리하는 근거가 아니다.

## 구현 요구사항

1. Common record는 channel, account/connection ref, cursor kind/value ref, pending refs, last transition, schema version을 가진다.
2. Telegram offset, Discord gateway resume/last id, Slack envelope/thread hint, Email UID/UIDVALIDITY, WhatsApp/WebSocket connection hint를 각각 설명한다.
3. Pending outbound는 content raw copy보다 session/event/artifact reference를 우선한다.
4. Restart 뒤 unknown delivery는 sent로 추정하지 않는다.
5. Inbound duplicate hint는 session event correlation과 함께 평가한다.
6. Status surface는 `pending`, `sent_hint`, `failed_hint`, `unknown`, `dedupe_candidate`를 구분한다.

## 정상 시퀀스

1. Transport가 inbound 또는 outbound transition hint를 만든다.
2. Orchestrator/dispatcher가 durable work/event correlation을 기록한다.
3. Channel restart state가 cursor와 pending refs를 저장한다.
4. Restart 뒤 adapter가 state를 읽고 보수적으로 resume한다.
5. Session truth와 delivery hint는 별도 projection으로 표시된다.

## 실패 시퀀스

1. Cursor 손상 또는 UIDVALIDITY mismatch는 automatic success/skip이 아니라 resync 상태가 된다.
2. Outbound send 후 acknowledgement 전 crash는 unknown delivery로 남는다.
3. Duplicate candidate가 correlation되지 않으면 session content를 자동 제거하거나 덮어쓰지 않는다.
4. Adapter 없는 channel state는 inspect-only evidence로 유지한다.

## 검증 관점

1. 각 지원 channel의 restart fixture를 최소 하나씩 둔다.
2. Send-before-ack crash와 ack-before-durable-commit crash를 구분한다.
3. Email UIDVALIDITY 변화, Discord resume 실패, Telegram offset replay를 검증한다.
4. Pending outbound/inbound가 restart 뒤 보존되는지 확인한다.
5. Public 문서와 status에 exactly-once 표현이 없는지 확인한다.

## 완료 기준

- 지원 channel별 restart semantics가 코드와 테스트로 존재한다.
- Hint와 session truth가 projection에서 분리된다.
- Unknown delivery를 성공으로 추정하지 않는다.
