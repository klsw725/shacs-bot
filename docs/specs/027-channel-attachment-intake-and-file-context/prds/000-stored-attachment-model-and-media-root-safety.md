# PRD 000. stored attachment model and media root safety

## 목표

이 문서는 `docs/specs/027-channel-attachment-intake-and-file-context/SPEC.md`의 첫 실행 문서다. 목표는 모든 채널 첨부 파일이 같은 안전 저장 계약을 통과하도록 `StoredAttachment` 모델, media root containment, 파일명 정규화, digest, MIME 검증 metadata, size와 count 제한, silent drop 금지 상태 모델을 구현 가능한 수준으로 내리는 것이다.

1차 구현은 bytes가 이미 runtime에 들어온 뒤의 공통 intake 저장 경계만 다룬다. Slack, Discord, Telegram, Email, WhatsApp bridge, WebSocket, local API에서 bytes를 가져오는 adapter 구현은 PRD 001이 소유한다.

## SPEC 입력

1. 주관 spec: `docs/specs/027-channel-attachment-intake-and-file-context/SPEC.md`
2. 교차 의존:
   1. `docs/specs/008-configuration-profiles-and-runtime-layout/SPEC.md`
   2. `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`
   3. `docs/specs/012-runtime-services/SPEC.md`
   4. `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`
   5. `docs/specs/019-image-generation-and-generated-media/SPEC.md`

## Dependency Cut

1. 008은 config data dir과 runtime data layout의 기준을 제공한다. 이 PRD는 새 global storage root를 만들지 않고 runtime media root 아래 attachment subtree를 쓴다.
2. 010은 media root containment, protected path, symlink, traversal, secret redaction 기준을 제공한다.
3. 012는 channel worker가 session turn으로 넘기는 attachment 후보의 상위 runtime 경계를 제공한다.
4. 014는 diagnostics가 raw secret, signed URL, bearer token, oversized content를 노출하지 않아야 한다는 기준을 제공한다.
5. 019는 generated media artifact와 inbound stored attachment가 섞이면 안 된다는 provenance 기준을 제공한다.
6. PRD 001은 이 PRD의 `ChannelAttachmentIntakeRequest`와 `StoredAttachment` 결과를 소비한다.
7. PRD 002, PRD 003, PRD 004는 이 PRD가 만든 stored attachment만 file context routing 입력으로 받는다.

## 범위

1. `ChannelAttachmentIntakeRequest`의 channel neutral 최소 필드 의미
2. `StoredAttachment`와 attachment item status 모델
3. runtime media root 아래 attachment 저장 위치와 relative path 계약
4. original filename과 sanitized filename 분리
5. sha256 digest 계산과 byte length 기록
6. declared MIME, detected MIME, detection source, mismatch metadata
7. per file size cap, per message attachment count cap, per turn total byte cap
8. 저장 실패, 제한 초과, MIME 불명, safety 차단을 silent drop하지 않는 status와 reason
9. diagnostics와 user facing projection이 소비할 수 있는 redacted attachment summary

## 범위 제외

1. Slack, Discord, Telegram, Email, WhatsApp bridge, WebSocket, local API별 다운로드와 payload 정규화
2. 이미지 native provider input 생성
3. 텍스트, PDF, Office 문서 추출
4. 오디오 전사, 오디오 요약, 비디오 장면 분석
5. 임의 URL 다운로드
6. native outbound file delivery
7. generated media artifact 저장 계약 변경
8. 장기 file library, gallery, vector index

## 구현 요구사항

1. `ChannelAttachmentIntakeRequest`는 channel, external message id 또는 request id, source display name, original filename, declared MIME, declared byte length, byte stream 또는 already buffered bytes, session key, turn correlation id를 표현해야 한다.
2. 저장 경로는 config data dir 아래 runtime media root의 attachment subtree에만 만들어야 한다.
3. filesystem path 구성에는 `sanitized_filename` 또는 generated attachment id만 사용해야 한다. `original_filename`은 표시용 metadata로만 유지한다.
4. 파일명 정규화는 path separator, absolute path, `..`, control character, reserved device name, 지나치게 긴 이름, extension spoofing 위험을 제거하거나 안전한 대체 이름으로 바꿔야 한다.
5. 파일 생성과 readback metadata 확인은 모두 media root containment를 통과해야 하며, 기존 파일을 덮어쓰면 안 된다.
6. symlink를 따라 저장하거나 읽으면 안 된다. parent directory도 media root 안에 있고 symlink가 아니어야 한다.
7. digest는 저장된 bytes 기준 sha256으로 계산하고 stored record에 남겨야 한다.
8. MIME routing 기준은 detected MIME이어야 한다. declared MIME과 detected MIME이 다르면 mismatch metadata와 diagnostic reason을 남긴다.
9. MIME을 확정할 수 없으면 unknown으로 기록하고 성공처럼 포장하지 않는다.
10. per file size cap, per message attachment count cap, per turn total byte cap은 저장 전에 적용해야 한다. stream 처리 중 초과가 드러나면 item을 blocked 또는 skipped로 끝내고 reason을 남긴다.
11. 하나의 attachment 실패가 기본적으로 전체 turn panic으로 이어지면 안 된다. 단, media root 자체가 안전하지 않거나 쓸 수 없으면 intake operation은 typed error로 실패해야 한다.
12. 모든 결과 item은 `stored`, `blocked`, `skipped`, `unsupported`, `extraction_failed` 같은 후속 projection 가능한 상태 중 하나를 가져야 한다.
13. diagnostics에는 raw bearer token, signed URL 전체, cookie, raw oversized content, absolute host path를 넣지 않는다.

## 데이터/상태 모델

1. `ChannelAttachmentIntakeRequest`: session key, turn id, channel, external message id, source display name, original filename, declared MIME, declared byte length, content source, received at.
2. `StoredAttachment`: attachment id, session key, channel, source display name, original filename, sanitized filename, media root relative path, declared MIME, detected MIME, MIME detection source, MIME mismatch flag, byte length, sha256, content family, intake status, diagnostic reason, created at.
3. `AttachmentIntakeStatus`: stored, blocked, skipped.
4. `AttachmentHandoffStatus`: pending, included native, included text, truncated, unsupported, extraction failed, blocked.
5. `AttachmentLimitPolicy`: max attachment count per message, max bytes per file, max bytes per turn.
6. `MimeDetectionMetadata`: declared MIME, detected MIME, detection source, mismatch flag, confidence family.
7. `AttachmentDiagnosticSummary`: attachment id, redacted display name, redacted relative path, byte length, detected MIME, MIME detection source, MIME mismatch flag, status, reason.

## 정상 시퀀스

1. caller가 channel neutral `ChannelAttachmentIntakeRequest` 목록을 전달한다.
2. intake service가 message attachment count cap과 turn total byte budget을 확인한다.
3. 각 item의 original filename을 표시용 값으로 보존하고 sanitized filename 또는 attachment id 기반 저장 이름을 만든다.
4. bytes를 media root 아래 attachment target에 exclusive create 방식으로 쓰며 per file cap을 적용한다.
5. 저장된 bytes의 sha256과 byte length를 계산한다.
6. declared MIME, detected MIME, detection source, mismatch flag를 기록하고 detected MIME 기준 content family를 정한다.
7. 최종 path가 media root containment를 통과하면 stored attachment record를 만든다.
8. result는 stored item과 skipped 또는 blocked item을 모두 포함해서 caller에게 반환한다.

## 실패 시퀀스

1. 파일 수가 count cap을 넘으면 초과 item은 skipped로 기록되고 reason을 가진다.
2. declared size 또는 stream byte count가 cap을 넘으면 item은 blocked 또는 skipped로 끝나고 partial file은 최종 stored attachment가 되지 않는다.
3. sanitized filename을 만들 수 없으면 generated safe filename을 쓰고 original filename은 표시용으로만 남긴다.
4. path traversal, absolute path, symlink, media root 탈출이 감지되면 item은 blocked가 되고 host path를 diagnostics에 남기지 않는다.
5. MIME이 unknown이거나 mismatch면 stored record에 그대로 남기고 후속 routing이 note only로 처리할 수 있게 한다.
6. media root가 없거나 안전하게 만들 수 없으면 intake operation은 typed infrastructure error로 실패한다.

## 검증 관점

1. original filename에 path traversal과 control character가 있어도 저장 path가 media root 밖으로 나가지 않는지 확인한다.
2. symlink parent와 symlink target을 통해 media root를 벗어나려는 입력이 blocked 되는지 확인한다.
3. sha256과 byte length가 저장된 bytes 기준으로 안정적으로 기록되는지 확인한다.
4. declared MIME과 detected MIME mismatch, detection source가 routing metadata와 diagnostics에 남는지 확인한다.
5. file size, attachment count, total turn byte cap 초과가 silent drop되지 않는지 확인한다.
6. diagnostics에 absolute host path, signed URL, bearer token, raw oversized content가 들어가지 않는지 확인한다.
7. generated media artifact와 inbound stored attachment의 provenance가 구분되는지 확인한다.

## 현재 구현 상태

2026-06-17 현재 PRD 000의 공통 stored attachment intake 계약은 `shacs-utils::attachments`에 구현되어 있다. 구현된 범위는 channel neutral `ChannelAttachmentIntakeRequest`, `StoredAttachment`, media root relative path 저장, filename sanitization, sha256 digest, MIME detection metadata, size/count/turn byte cap, blocked/skipped/stored status, redacted diagnostic summary다.

다만 이 구현은 bytes가 이미 runtime에 들어온 뒤의 공통 저장 경계다. Slack, Discord, Telegram, Email, WhatsApp bridge, WebSocket, local API별 다운로드와 payload 정규화는 PRD 001 범위이며, image/document/audio/video file context routing은 PRD 002부터 PRD 004 범위다.

## 완료 기준

1. `ChannelAttachmentIntakeRequest`와 `StoredAttachment` 타입 의미가 public runtime 경계에서 고정된다.
2. 모든 stored attachment path가 media root relative path로 기록되고 containment 테스트를 통과한다.
3. filename sanitization이 traversal, absolute path, separator, control character, reserved name을 막는다.
4. sha256, byte length, declared MIME, detected MIME, mismatch metadata가 저장된다.
5. per file size cap, per message attachment count cap, per turn total byte cap이 적용된다.
6. blocked, skipped, stored item이 모두 result에 남고 silent drop이 없다.
7. diagnostics와 projection summary가 secret과 absolute host path를 노출하지 않는다.
8. PRD 001부터 PRD 004가 이 stored attachment record를 입력으로 삼을 수 있다.
