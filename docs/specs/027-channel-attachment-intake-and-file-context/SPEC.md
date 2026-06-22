# channel attachment intake and file context 아키텍처 명세

Status: Implemented (v1 closure). 이 문서는 채널로 들어온 사용자 첨부 파일을 안전하게 받아 저장하고, provider 입력에 파일 문맥으로 넘기는 owner boundary를 고정한다.

## 문서 목적

이 문서는 `shacs-bot`의 외부 채널과 local API가 받는 첨부 파일을 하나의 공통 intake 계약으로 정리한다. 핵심 모델은 단순하다. 이미지, 문서, 오디오, 비디오 업로드는 서로 다른 기능이 아니라 같은 channel attachment intake operation이다. 채널에서 온 bytes를 먼저 안전하게 저장하고 정규화한 뒤, runtime context handling 단계에서 파일 종류에 따라 이미지 provider input, 텍스트 추출, best-effort 문서 추출, audio/video context analysis, 또는 note-only projection으로 갈라진다.

따라서 이 문서는 세 가지 owner 경계를 가진다. 첫째, Slack, Discord, Telegram, Email, WhatsApp bridge, WebSocket/local API에서 들어온 attachment를 channel-neutral stored attachment로 만드는 공통 intake 경계다. 둘째, 저장된 첨부 파일이 runtime media root 안에 안전하게 존재한다는 `StoredAttachment` 계약이다. 셋째, provider 호출 직전 파일 문맥을 어떻게 넘길지 결정하는 runtime file-context handoff 경계다.

이 문서는 이전에 흩어져 있던 관점을 하나의 계약으로 합친다. 채널별 다운로드, upload/data URL 처리, media path 저장, 이미지 입력, 문서 추출, audio/video context extraction, safety guard, 사용자 표시를 따로 보지 않고, inbound attachment가 session turn에 들어와 provider input 후보가 되기까지의 전체 경로를 하나의 owner boundary로 다룬다.

## 상위 기준과의 관계

| spec | 027이 소비하는 것 | 027이 소유하는 것 |
|---|---|---|
| 009 context assembly | provider 호출 직전 context assembly와 현재 media message 경로 | channel attachment에서 만든 file context artifact를 009에 넘기는 handoff 계약 |
| 010 host safety | filesystem boundary, symlink/path guard, protected target, redaction 원칙 | attachment 저장과 추출에 필요한 media root containment, MIME 검증, diagnostics redaction 요구 |
| 012 runtime services | channel worker, message bus, platform normalizer, follow-up queue | channel inbound payload 안의 attachment intake와 stored attachment 생성 규칙 |
| 013 user interfaces and session UX | CLI, TUI, local API projection 원칙 | 첨부 파일의 included, skipped, blocked, extracted 상태를 사용자가 이해하는 표시 의미 |
| 019 image generation and generated media | generated media artifact 저장 계약과 image generation tool 경계 | 사용자 inbound attachment를 generated media와 섞지 않는 규칙 |
| 026 context files and inline references | 명시적 `@file`, `@folder`, `@url` reference resolution | 채널 업로드 첨부 파일에서 자동으로 생기는 file context 경계 |

027은 009를 다시 열지 않는다. 009가 provider input assembly를 소유하고, 027은 assembly가 소비할 attachment-derived context artifact를 만든다. 027은 012의 channel runtime을 다시 정의하지도 않는다. 012는 worker와 bus의 runtime service 경계를 맡고, 027은 worker가 전달한 attachment payload를 안전 저장과 provider handoff 후보로 바꾸는 계약을 맡는다.

019와의 구분은 특히 중요하다. 019의 generated media는 agent가 만든 산출물이다. 027의 stored attachment는 사용자가 채널을 통해 보낸 inbound artifact다. 둘 다 runtime media root 아래 저장될 수 있지만, 소유자, provenance, diagnostics, provider handoff 의미가 다르다.

026과의 구분도 분명해야 한다. 026은 사용자가 message 안에 명시한 `@file`, `@folder`, `@url` 같은 inline reference를 해석한다. 027은 채널 event, MIME message, WebSocket frame, local API request에 이미 포함된 업로드 파일을 다룬다. 사용자가 문서 파일을 업로드했을 때 그것을 `@file` reference처럼 다시 쓰도록 요구하면 안 된다.

## 범위

초기 범위는 개인용 self-hosted runtime에서 실제로 필요한 채널을 기준으로 제한한다. 대상 채널은 Slack, Discord, Telegram, Email, WhatsApp bridge, WebSocket/local API다. 각 채널은 첨부 메타데이터와 bytes를 제공하는 방식이 다르지만, runtime 안에서는 같은 `ChannelAttachmentIntake` 경로로 들어와야 한다.

이 문서는 다음을 정의한다.

1. 채널별 attachment payload를 공통 intake 요청으로 정규화하는 규칙.
2. 안전한 저장 위치, 파일명, digest, MIME, size, provenance를 가진 stored attachment 계약.
3. 이미지, 텍스트, PDF, Office, unsupported binary, audio, video의 v1 runtime routing.
4. provider input에 들어가는 file context artifact와 note-only artifact의 차이.
5. size cap, duration cap, path traversal, symlink, MIME/magic-byte, protected path, diagnostics redaction, no silent drop 안전 요구사항.
6. CLI, TUI, local API, channel reply에서 사용자가 이해할 수 있는 projection 의미.
7. 구현 PRD 분할과 완료 기준.

이 문서는 다음을 완료 기능으로 선언하지 않는다. OCR, PDF layout parsing, 임의 URL 다운로드, archive recursion, native outbound file delivery, hosted file manager, remote asset library는 v1 범위가 아니다. Audio/video는 같은 intake 경로와 safe storage를 통과한 뒤 runtime file context analysis 대상으로 들어가지만, 무제한 원문 전사나 완전한 장면 이해를 보장하는 기능은 아니다.

## 핵심 정의

### channel attachment

Channel attachment는 사용자가 외부 채널 또는 local API를 통해 현재 turn과 함께 보낸 파일형 입력이다. Slack file, Discord attachment, Telegram photo/document, Email MIME attachment, WhatsApp bridge media, WebSocket/local API upload 또는 data URL media가 여기에 포함된다.

Attachment는 message text와 다르다. Message text는 session user content의 일부지만, attachment bytes는 먼저 intake safety를 통과해 stored attachment가 된 뒤에만 runtime context 후보가 된다.

### stored attachment

Stored attachment는 media root 아래 안전하게 저장되고 digest와 검증 메타데이터를 가진 attachment record다. 이 record는 raw channel payload가 아니며 provider input도 아니다. Runtime은 stored attachment record 또는 그 record에서 파생된 media-root-contained stored path reference를 기준으로 파일 종류를 판정하고, 추출 또는 provider handoff를 수행한다. 어떤 형태로 handoff되더라도 readback 단계에서는 media root containment와 symlink guard를 다시 통과해야 한다.

Stored attachment의 최소 의미 필드는 다음과 같다.

```text
attachment_id
session_id 또는 session_key
channel
external_message_id 또는 request_id
source_display_name
original_filename
sanitized_filename
media_root_relative_path
declared_mime
detected_mime
byte_length
sha256
intake_status
content_family
handoff_status
diagnostic_reason
```

`media_root_relative_path`는 diagnostic과 replay에 필요한 상대 참조다. 사용자에게 보여 줄 수는 있지만, provider나 외부 채널에 raw host absolute path를 그대로 노출하지 않는다.

### file context artifact

File context artifact는 stored attachment를 provider input에 넣기 위해 만든 ephemeral context block이다. 이미지 native input, 추출 텍스트, bounded audio transcription이나 summary, bounded video context extraction, 추출 실패 note, unsupported note가 모두 같은 artifact family에 속한다. Artifact는 session truth를 직접 바꾸지 않으며, provider 호출 직전 context assembly가 소비한다.

### note-only attachment

Note-only attachment는 안전하게 저장됐지만 provider context artifact로 포함할 수 없는 attachment다. Unsupported binary, 검증 실패 뒤 저장하지 않은 blocked item, size cap이나 duration cap 초과 item, analyzer 미지원 또는 extraction 실패 item은 사용자가 볼 수 있는 reason과 함께 note로 투영된다. Audio/video도 codec 미지원, analyzer 부재, budget 초과, 추출 실패가 있으면 unsupported 또는 extraction_failed note로 남기며 silent drop은 허용하지 않는다.

## Channel Attachment Intake

Channel attachment intake는 채널 adapter 뒤, session turn context assembly 앞에 위치한다. Adapter는 채널별 payload를 그대로 provider context에 넣지 않는다. 먼저 attachment 후보를 channel-neutral intake request로 바꾼다. 이 request는 channel, external message id, sender-visible display name, original filename, declared MIME, declared size, download source kind, byte stream 또는 data URL body, reply/thread metadata를 담는다.

Slack, Discord, Telegram, Email, WhatsApp bridge는 파일 bytes를 얻는 방법이 다르다. Slack과 Discord는 platform file URL이나 attachment URL을 제공할 수 있고, Telegram은 file id 기반 fetch가 필요할 수 있으며, Email은 MIME part를 이미 포함한다. WhatsApp bridge와 WebSocket/local API는 bridge frame 또는 API body가 bytes나 data URL을 전달할 수 있다. 027의 규칙은 이 차이를 intake adapter 안에 가두는 것이다. Runtime context handling은 파일이 어느 채널에서 왔는지에 따라 이미지, 문서, audio, video를 다르게 취급하지 않는다.

다운로드가 필요한 경우에도 027은 arbitrary URL fetch 기능이 아니다. Runtime은 인증된 channel event가 가리키는 platform attachment source만 attachment download로 취급한다. 사용자가 message text에 쓴 URL을 따라가 파일을 내려받는 기능은 027의 v1 범위가 아니며, 명시적 URL reference는 026의 별도 경계다.

Intake는 다음 순서를 지켜야 한다.

1. 채널 payload를 공통 request로 정규화한다.
2. size cap과 channel별 declared size cap을 먼저 적용한다.
3. 파일명을 표시용 원본 이름과 저장용 sanitized filename으로 나눈다.
4. bytes를 임시 저장 위치에 받되 media root 밖 path를 만들지 않는다.
5. magic byte와 MIME을 검증하고 declared MIME과 차이를 diagnostic으로 남긴다.
6. 최종 stored path를 media root relative path로 확정하고 digest를 계산한다.
7. stored attachment record를 만들고 included, skipped, blocked, stored 상태를 기록한다.

이 순서에서 실패한 항목은 전체 turn을 조용히 통과시키면 안 된다. 사용자가 보낸 첨부 파일이 너무 크거나, MIME 검증에 실패했거나, 안전 정책 때문에 저장되지 않았다면 그 사실이 channel reply, local API response, diagnostics 중 적어도 하나의 표면에 남아야 한다.

## Stored Attachment Contract

Stored attachment는 provider input보다 오래 사는 local artifact지만, 영구 지식이나 memory가 아니다. Session과 replay가 어느 파일이 어떤 상태였는지 설명할 수 있을 만큼의 record는 남기되, raw secret이나 과도한 content excerpt를 diagnostics에 넣지 않는다.

저장 위치는 config data dir 아래 runtime media subtree여야 한다. 구현은 예를 들어 `media/attachments/<channel>/<date>/<attachment_id>` 같은 구조를 쓸 수 있지만, 계약의 핵심은 absolute user path를 저장 식별자로 삼지 않고 media root relative path와 digest를 기준으로 추적하는 것이다.

파일명은 두 가지로 다룬다. `original_filename`은 사용자가 이해하는 표시 이름이고 신뢰할 수 없는 문자열이다. `sanitized_filename`은 path separator, control character, reserved device name, 지나치게 긴 이름, confusable extension spoofing을 제거하거나 완화한 저장용 이름이다. 저장용 이름만 filesystem path 구성에 쓸 수 있다.

Stored attachment는 symlink를 따라가면 안 된다. 임시 파일에서 최종 파일로 이동할 때도 parent directory가 media root 안에 있고 symlink가 아니어야 한다. Canonical path 검사는 media root containment를 통과해야 하며, `..`, absolute path, percent-encoded traversal, mixed separator traversal을 모두 거절해야 한다.

MIME은 declared MIME과 detected MIME을 구분한다. Declared MIME은 채널이나 client가 준 주장이고, detected MIME은 magic byte, extension, content sniffing을 조합한 runtime 판단이다. 두 값이 다르면 detected MIME이 routing의 기준이 되며, mismatch는 diagnostics에 남긴다. MIME 검증 실패는 성공처럼 포장하지 않는다.

## Runtime File Context Routing

Runtime file context routing은 stored attachment가 만들어진 뒤에만 실행된다. 이 단계는 파일 종류에 따라 provider input 후보를 만든다. 핵심 원칙은 파일 종류의 차이가 intake가 아니라 safe storage 이후 runtime context handling에서 발생한다는 것이다.

이미지는 v1에서 가장 직접적인 provider handoff 대상이다. PNG, JPEG, WebP, GIF 같은 안전하게 식별된 이미지 family는 provider와 모델이 native image input을 지원할 때 image input block으로 들어간다. Provider가 native image input을 지원하지 않으면 이미지가 있었다는 note와 파일 메타데이터를 남기고, 이미지를 분석한 것처럼 답하지 않는다.

텍스트 계열 파일은 bounded text extraction으로 들어간다. Plain text, Markdown, JSON, CSV, log처럼 텍스트로 안전하게 읽을 수 있는 파일은 encoding sniffing, size cap, token budget, redaction gate를 통과한 excerpt만 context artifact로 만든다. 전체 파일을 무제한 prompt에 넣지 않는다.

PDF와 Office 문서는 best-effort extraction 대상이다. v1의 목표는 문서에서 안전하게 뽑을 수 있는 텍스트를 제한된 크기로 context에 넣는 것이다. OCR, full layout parsing, 표와 이미지의 완전한 구조 복원, 매크로 실행, embedded object recursion은 v1 범위가 아니다. 추출 실패나 password-protected document는 note-only artifact로 표시한다.

Audio는 safe storage 이후 bounded transcription 또는 audio summary를 file context artifact로 만드는 대상이다. Runtime은 파일 size와 duration cap을 먼저 적용하고, 지원되는 analyzer가 있을 때만 제한된 길이의 전사, 요약, 언어나 기본 media metadata 같은 bounded evidence를 provider context 후보로 만든다. 추출 텍스트와 요약은 context budget 안에서 잘라야 하며, unsupported codec, analyzer 부재, duration 초과, parser error는 unsupported 또는 extraction_failed note로 남긴다.

Video는 safe storage 이후 bounded video context extraction을 file context artifact로 만드는 대상이다. v1 목표는 metadata, duration, keyframe 또는 scene summary, 사용 가능한 subtitle track extraction, 지원되는 경우 audio-track transcription 같은 제한된 context를 만드는 것이다. Runtime은 video size와 duration cap을 적용하고, keyframe 수, scene summary 길이, subtitle 또는 audio-track transcript 길이를 context budget 안에 묶어야 한다. Unsupported codec, missing analyzer, subtitle track 부재, audio-track 추출 미지원은 silent drop하지 않고 unsupported 또는 extraction_failed reason으로 표시한다.

Unsupported binary는 저장과 note-only projection까지만 수행한다. Runtime은 파일명, detected MIME, size, digest 일부, unsupported reason을 표시할 수 있지만, 내용을 읽은 것처럼 provider에게 전달하지 않는다.

Provider handoff는 context budget을 따라야 한다. 이미지 input, 텍스트 extraction, audio transcript나 summary, video metadata와 scene summary는 active user message보다 우선할 수 없고, system/developer/runtime instruction을 밀어내면 안 된다. Budget overflow는 truncation 또는 skipped evidence를 남긴다.

## Safety

Attachment intake는 외부 input을 파일로 저장하고 일부를 provider prompt로 넘기는 경계이므로 010의 safety 원칙을 강하게 소비한다. 기본 요구사항은 다음과 같다.

1. 모든 stored attachment path는 media root containment를 통과해야 한다.
2. 저장 파일명은 sanitized filename만 사용해야 하며 original filename은 표시용 metadata로만 써야 한다.
3. per-file size cap, per-message attachment count cap, per-turn total bytes cap을 가져야 한다.
4. Symlink, hardlink surprise, path traversal, absolute path, encoded traversal은 저장과 읽기 모두에서 거절해야 한다.
5. MIME과 magic byte를 검증해야 하며 mismatch와 unknown은 routing reason으로 기록해야 한다.
6. Protected credential/config paths는 attachment source나 extraction target으로 쓰면 안 된다.
7. Diagnostics는 raw token, cookie, authorization header, full platform download URL, raw secret-like content를 저장하면 안 된다.
8. 실패한 attachment는 silent drop하지 않고 blocked, skipped, unsupported, extraction_failed 같은 reason을 남겨야 한다.

Channel download가 필요한 경우 인증 정보는 channel adapter 안에서만 사용해야 한다. Stored attachment record와 diagnostics에는 bearer token, signed URL 전체, cookie, provider raw response를 넣지 않는다. URL의 host나 channel file id처럼 문제 분석에 필요한 최소 metadata만 redacted form으로 남긴다.

Extraction은 untrusted parser boundary다. PDF와 Office 문서를 처리할 때 매크로나 embedded object를 실행하지 않으며, archive recursion을 수행하지 않는다. Audio/video analyzer도 같은 boundary 안에 있으며 file size, duration, extracted text, summary 길이를 제한해야 한다. Parser error, unsupported codec, missing analyzer는 attachment-level unsupported 또는 extraction_failed로 남기고, session turn 전체를 panic으로 중단하지 않는다.

## User-Facing Projection

사용자는 자신이 보낸 파일이 runtime에 어떻게 처리됐는지 알 수 있어야 한다. Projection의 목적은 세부 구현을 노출하는 것이 아니라, 파일이 포함됐는지, 생략됐는지, 차단됐는지, 어떤 제한 때문에 일부만 쓰였는지 설명하는 것이다.

Channel reply와 local API response는 적어도 attachment count와 item status를 표현할 수 있어야 한다. 예를 들어 이미지 두 장이 provider input에 포함되고 PDF 하나가 size cap 때문에 skipped 됐다면, 최종 답변은 PDF를 읽은 것처럼 말하면 안 된다. TUI나 inspect surface는 stored path의 redacted relative ref, detected MIME, byte length, extraction status, truncation status, blocked reason을 더 자세히 보여줄 수 있다.

Projection status는 다음 의미를 가진다.

1. `stored`: 안전 저장은 끝났지만 provider context에는 아직 들어가지 않았다.
2. `included_native`: provider native file 또는 image input으로 전달됐다.
3. `included_text`: 추출 텍스트가 bounded context로 전달됐다.
4. `truncated`: 일부만 전달됐고 truncation evidence가 있다.
5. `unsupported`: 저장됐지만 해당 파일 family, codec, analyzer, budget 조건 때문에 provider context artifact를 만들 수 없다.
6. `blocked`: safety 정책 때문에 저장 또는 사용이 거절됐다.
7. `extraction_failed`: 저장은 됐지만 추출 단계에서 실패했다.

이 projection은 사용자에게 파일 분석 결과를 보장하지 않는다. Provider가 이미지, 추출 텍스트, audio summary, video context artifact를 실제로 어떻게 해석했는지는 provider response의 영역이다. Runtime이 보장하는 것은 어떤 파일 문맥을 provider input 후보로 만들었는지와 어떤 파일을 넣지 않았는지의 evidence다.

## Non-goals / 범위 제외

v1은 첨부 파일 intake와 runtime context handoff를 닫는 데 집중한다. 다음은 범위 밖이다.

1. OCR.
2. Full PDF layout parsing.
3. 임의 URL 다운로드.
4. Archive recursion.
5. Native outbound file delivery.
6. Hosted document connector.
7. Remote asset library 또는 gallery.
8. 조직 관리자용 파일 정책 배포.
9. 장기 semantic memory나 vector indexing.
10. 첨부 파일을 tool permission 또는 filesystem permission으로 승격하는 동작.
11. Provider-native attachment feature 전체를 provider별로 완성했다는 선언.
12. Audio/video의 무제한 전사, 완전한 장면 분석, 임의 codec 전체 지원.

## PRD 분할

이 spec은 아래 다섯 PRD로 나눈다. 공통 target은 같다. Image, document, audio, video upload는 같은 channel intake path를 통과하고, 파일 종류별 차이는 safe storage와 normalization 이후 runtime file context 단계에서만 발생한다.

1. [`prds/000-stored-attachment-model-and-media-root-safety.md`](prds/000-stored-attachment-model-and-media-root-safety.md): stored attachment model, media root containment, filename sanitization, digest, MIME detection metadata, size/count/turn byte caps, no silent drop status model.
2. [`prds/001-channel-intake-normalizers.md`](prds/001-channel-intake-normalizers.md): Slack, Discord, Telegram, Email, WhatsApp bridge, WebSocket, local API adapter가 bytes와 metadata를 공통 intake request와 stored attachment model로 정규화하는 범위.
3. [`prds/002-image-document-file-context-routing.md`](prds/002-image-document-file-context-routing.md): image native input, text extraction, PDF/Office best effort extraction, unsupported binary note only routing. Audio와 video analysis는 status pass through만 다룬다.
4. [`prds/003-audio-file-context-analysis.md`](prds/003-audio-file-context-analysis.md): bounded audio transcription과 summary, duration caps, analyzer capability abstraction, unsupported codec과 analyzer missing behavior.
5. [`prds/004-video-file-context-analysis-and-projection.md`](prds/004-video-file-context-analysis-and-projection.md): bounded video metadata, duration, subtitle extraction, PRD 003 capability를 소비한 audio track transcription, keyframe 또는 scene summary, inspect/channel/local API projection과 diagnostics. Native outbound delivery는 범위 밖이다.

## 현재 구현 상태

2026-06-22 현재 이 spec은 v1 완료 기준으로 구현되어 있다. PRD 000부터 PRD 004까지의 범위에서 common stored attachment intake, channel intake normalization, image/document/unsupported-binary file context routing, audio analyzer capability handoff, video analyzer capability handoff가 연결되어 있다. `crates/shacs-core/src/runtime/context.rs`의 `ContextBuilder`는 configured media root 아래 stored attachment reference를 file context routing layer로 넘기고, 기존 workspace image media path의 image URL block 동작은 기본값으로 유지한다.

`crates/shacs-utils/src/document.rs`의 text, PDF, Office 계열 문서 추출 helper는 PRD 002 routing에서 bounded text/document extraction으로 연결되어 있다. 추출은 best effort이며 OCR, full layout parsing, macro execution, embedded object recursion, archive recursion을 완료 기능으로 선언하지 않는다. Provider/model native image input capability가 없거나 extraction이 실패하거나 unsupported binary가 들어오면 note-only artifact로 남긴다.

따라서 현 구현을 이렇게 해석한다. Slack/Discord/Telegram은 인증된 channel event가 제공한 platform attachment source만 다운로드 대상으로 삼고, Email MIME part와 WebSocket/local API data URL 또는 upload bytes는 같은 stored attachment intake 경로로 들어간다. WhatsApp bridge media는 bridge가 준 media list를 raw content에 노출하지 않고 media 경로로만 넘긴다. 이미지, 텍스트, PDF, Office 문서, unsupported binary는 PRD 002 범위의 stored attachment routing으로 닫혔다. Audio는 PRD 003 범위에서 analyzer가 주입되면 bounded transcript 또는 summary artifact로 라우팅되고, analyzer missing, unsupported codec, analyzer failure는 user-visible note로 남는다. Video는 PRD 004 최소 범위에서 deferred-only 동작을 벗어나 capability-based analyzer route로 들어간다. Runtime에 video analyzer가 주입된 경우에만 byte/duration cap 이후 bounded metadata, subtitle, scene/keyframe summary, PRD 003 audio analyzer 재사용 결과를 context artifact 후보로 만들며, analyzer missing은 unsupported note로 표시한다. 기본 ffmpeg, built-in full codec support, native outbound video delivery, video-specific inspect/local API projection 완성, 임의 URL 다운로드는 완료 기능으로 주장하지 않는다.

## 완료 기준

Spec 027을 완료로 보려면 아래 기준이 테스트와 문서로 확인되어야 한다.

1. Slack, Discord, Telegram, Email, WhatsApp bridge, WebSocket/local API attachment payload가 공통 intake request로 정규화된다.
2. Stored attachment는 media root 밖 path를 만들 수 없고, symlink와 path traversal을 거절하며, sanitized filename과 digest를 기록한다.
3. Size cap, attachment count cap, total turn bytes cap이 적용되고 초과 항목은 silent drop되지 않는다.
4. Declared MIME과 detected MIME을 구분하고, mismatch와 unknown type은 routing과 diagnostics에 반영된다.
5. Image attachment는 provider와 모델이 지원할 때 native image input으로 들어가고, 미지원이면 note-only로 남는다.
6. Text attachment는 bounded extraction과 truncation evidence를 거쳐 provider context에 들어간다.
7. PDF/Office attachment는 best-effort text extraction만 수행하며, OCR이나 layout parsing을 완료 기능으로 광고하지 않는다.
8. Audio attachment는 size와 duration cap을 통과한 뒤 지원되는 analyzer가 있을 때 bounded transcription 또는 audio summary를 file context artifact로 만들고, 실패나 미지원은 unsupported 또는 extraction_failed로 표시한다.
9. Video attachment는 size와 duration cap을 통과한 뒤 metadata, duration, keyframe 또는 scene summary, subtitle track extraction, 지원되는 경우 audio-track transcription 같은 bounded context artifact를 만들고, 실패나 미지원은 unsupported 또는 extraction_failed로 표시한다.
10. Unsupported binary는 저장 여부와 unsupported reason을 사용자에게 표시하고 provider가 내용을 읽은 것처럼 만들지 않는다.
11. Diagnostics와 inspect surface는 raw secret, bearer token, signed URL 전체, raw oversized content를 포함하지 않는다.
12. 009 context assembly handoff 테스트가 image native input, text extraction, audio/video extracted context, skipped, blocked, unsupported, extraction failed 상태를 구분한다.
13. 019 generated media artifact와 027 inbound stored attachment가 provenance와 diagnostics에서 섞이지 않는다.
14. 026 inline reference resolver 없이도 channel-uploaded attachment가 runtime file context 후보가 된다.
15. 문서와 사용자 표시가 OCR, full PDF layout parsing, arbitrary URL download, archive recursion, native outbound file delivery, audio/video의 무제한 전사나 완전한 장면 분석을 v1 지원으로 주장하지 않는다.
