# PRD 001. channel intake normalizers

## 목표

이 문서는 Slack, Discord, Telegram, Email, WhatsApp bridge, WebSocket, local API attachment payload를 PRD 000의 `ChannelAttachmentIntakeRequest`로 정규화하는 실행 범위를 고정한다. 목표는 채널마다 다른 bytes 획득 방식과 metadata shape를 adapter 안에 가두고, safe storage 이후에는 모든 파일이 같은 stored attachment 모델로 보이게 만드는 것이다.

이 PRD는 content extraction을 다루지 않는다. 이미지, 문서, 오디오, 비디오 차이는 PRD 000의 안전 저장과 정규화가 끝난 뒤 PRD 002, PRD 003, PRD 004에서 발생한다.

## SPEC 입력

1. 주관 spec: `docs/specs/027-channel-attachment-intake-and-file-context/SPEC.md`
2. 교차 의존:
   1. `docs/specs/012-runtime-services/SPEC.md`
   2. `docs/specs/013-user-interfaces-and-session-ux/SPEC.md`
   3. `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`
   4. `docs/specs/026-context-files-and-inline-references/SPEC.md`
3. 선행 PRD: `docs/specs/027-channel-attachment-intake-and-file-context/prds/000-stored-attachment-model-and-media-root-safety.md`

## Dependency Cut

1. 012는 channel worker, message bus, session turn correlation을 제공한다.
2. 013은 channel reply와 local API response가 attachment status를 보여줘야 하는 사용자 표면 기준을 제공한다.
3. 014는 channel adapter diagnostics에서 token, signed URL, raw platform response를 가려야 한다는 기준을 제공한다.
4. 026은 user message 안의 명시적 `@file`, `@folder`, `@url` reference 경계를 소유한다. 이 PRD는 channel payload에 이미 포함된 upload만 다룬다.
5. PRD 000은 adapter가 넘길 neutral intake request와 stored attachment 결과를 제공한다.
6. PRD 002, PRD 003, PRD 004는 이 PRD가 어느 채널에서 왔는지와 무관하게 stored attachment를 소비한다.

## 범위

1. Slack file event와 message attachment metadata를 neutral intake request로 바꾸는 adapter
2. Discord attachment URL과 metadata를 neutral intake request로 바꾸는 adapter
3. Telegram photo, document, audio, video file id 기반 payload를 neutral intake request로 바꾸는 adapter
4. Email MIME attachment part를 neutral intake request로 바꾸는 adapter
5. WhatsApp bridge media frame을 neutral intake request로 바꾸는 adapter
6. WebSocket upload, data URL media, binary frame metadata를 neutral intake request로 바꾸는 adapter
7. local API multipart 또는 JSON data URL input을 neutral intake request로 바꾸는 adapter
8. channel adapter별 auth secret과 platform URL redaction
9. adapter 결과를 PRD 000 intake service에 전달하고 item status를 turn result에 보존하는 연결

## 범위 제외

1. Stored attachment 모델과 media root safety 구현 자체
2. 이미지 native provider input 생성
3. 텍스트, PDF, Office 문서 추출
4. 오디오 전사와 요약
5. 비디오 subtitle, keyframe, scene summary
6. user message text에 포함된 임의 URL 다운로드
7. native outbound file delivery
8. OCR, archive recursion, hosted file manager

## 구현 요구사항

1. 각 channel adapter는 raw platform payload를 provider input으로 직접 넘기면 안 된다.
2. adapter 출력은 PRD 000의 `ChannelAttachmentIntakeRequest` 목록이어야 한다.
3. Slack과 Discord처럼 platform file URL이 있는 경우에도 인증된 channel event가 제공한 attachment source만 다운로드 대상으로 인정한다.
4. Telegram은 file id와 file metadata를 request metadata에 담고, bot token이나 full fetch URL을 diagnostics에 남기지 않는다.
5. Email은 MIME part의 filename, content type, transfer decoded bytes, message id 또는 part id를 neutral request로 옮긴다.
6. WhatsApp bridge는 bridge가 준 media id, MIME, filename, size, bytes 또는 fetch handle을 neutral request로 옮긴다.
7. WebSocket과 local API는 upload bytes 또는 data URL body를 neutral request로 옮기되, data URL parsing 실패를 attachment item failure로 기록한다.
8. 모든 adapter는 declared byte length가 있으면 PRD 000 cap 적용 전에 전달해야 한다.
9. channel source display name은 사용자에게 보일 수 있는 값으로 제한하고 secret, token, signed URL 전체를 포함하지 않는다.
10. adapter는 content family를 확정하려고 파서를 실행하지 않는다. MIME 주장은 declared MIME으로만 전달하고 detected MIME은 PRD 000이 기록한다.
11. adapter는 content extraction을 하지 않는다. bytes와 metadata를 safe storage 경계로 넘기는 일만 한다.
12. attachment 없는 message는 기존 text only turn 경로를 깨지 않아야 한다.

## 데이터/상태 모델

1. `ChannelAttachmentEnvelope`: channel, external message id, sender handle, thread metadata, attachment candidates.
2. `AttachmentSourceKind`: platform download, inline bytes, data URL, MIME part, bridge media handle, local multipart.
3. `ChannelAttachmentIntakeRequest`: PRD 000 입력 모델을 그대로 소비한다.
4. `ChannelAttachmentAdapterResult`: intake requests, adapter level skipped items, redacted diagnostics.
5. `ChannelAttachmentAdapterError`: missing credential, platform download failed, malformed data URL, MIME part decode failed, payload too large before storage.
6. `AttachmentProjectionSeed`: external item id, original filename, channel, request id, adapter status.

## 정상 시퀀스

1. channel worker가 inbound message와 attachment 후보를 수신한다.
2. channel adapter가 channel specific metadata와 bytes source를 `ChannelAttachmentIntakeRequest`로 변환한다.
3. adapter는 secret과 raw platform URL을 redacted diagnostic 형태로만 유지한다.
4. runtime이 request 목록을 PRD 000 intake service에 넘긴다.
5. PRD 000이 stored, skipped, blocked item을 반환한다.
6. session turn은 message text와 stored attachment list를 함께 보존한다.
7. 후속 runtime routing은 channel 이름이 아니라 stored attachment의 detected MIME과 status를 기준으로 동작한다.

## 실패 시퀀스

1. channel credential이 없어서 attachment bytes를 가져올 수 없으면 해당 item은 skipped 또는 blocked reason을 가진다.
2. platform download가 실패하면 adapter는 retry 폭주 없이 item failure를 만들고 turn 전체를 조용히 성공처럼 처리하지 않는다.
3. data URL이 malformed이면 item은 adapter failure로 기록되고 raw body를 diagnostics에 남기지 않는다.
4. Email MIME decoding이 실패하면 attachment item은 extraction 전 단계의 intake failure로 기록된다.
5. declared size가 adapter 단계에서 이미 cap을 넘는 것이 분명하면 bytes fetch를 생략하고 skipped 또는 blocked item을 만든다.
6. 한 채널의 adapter 실패가 다른 channel adapter나 text only turn 동작을 깨지 않아야 한다.

## 검증 관점

1. Slack, Discord, Telegram, Email, WhatsApp bridge, WebSocket, local API sample payload가 같은 neutral request shape로 변환되는지 확인한다.
2. channel token, signed URL, cookie, raw platform response가 stored record나 diagnostics에 남지 않는지 확인한다.
3. attachment 없는 message가 기존 text only turn으로 처리되는지 확인한다.
4. adapter가 content extraction을 실행하지 않는지 확인한다.
5. channel별 original filename과 declared MIME이 PRD 000으로 전달되는지 확인한다.
6. failed download, malformed data URL, MIME decode failure가 silent drop되지 않는지 확인한다.
7. 같은 이미지, 문서, audio, video 파일이 channel과 무관하게 stored attachment 결과로 이어지는지 확인한다.

## 현재 구현 상태

2026-06-22 현재 이 PRD의 v1 channel intake normalization 범위는 구현되어 있다. WebSocket/local API data URL media와 upload bytes, Email MIME attachment part, Slack file event, Discord attachment payload, Telegram photo/document/audio/video payload는 PRD 000의 stored attachment intake 경로로 연결된다. WhatsApp bridge media는 message content에 raw path marker를 넣지 않고 media list로만 넘긴다.

Slack과 Discord는 channel event 안의 platform attachment URL만 다운로드 대상으로 인정하고, Telegram은 bot API `file_id` 기반 fetch만 attachment source로 인정한다. Raw host path, message text 안의 임의 URL, malformed data URL, failed platform download는 safe projection failure로 남으며 token, signed URL, raw response body는 session/provider context에 남기지 않는다. Content extraction은 여전히 이 PRD가 아니라 PRD 002부터 PRD 004의 책임이며, text/PDF/Office bounded extraction은 PRD 002, audio/video bounded analysis는 PRD 003/004가 소유한다.

## 완료 기준

1. 대상 channel과 local API adapter가 모두 PRD 000 intake request를 생성한다.
2. adapter가 raw attachment payload를 provider input으로 직접 넘기지 않는다.
3. channel download credential과 platform URL이 diagnostics와 stored record에서 redacted 된다.
4. malformed payload, missing credential, download failure가 item status로 남는다.
5. text only message 경로가 attachment normalizer 도입 뒤에도 유지된다.
6. 같은 파일 family가 어느 channel에서 와도 같은 stored attachment routing 입력으로 보인다.
7. content extraction은 이 PRD 안에서 실행되지 않는다.
