# PRD 002. image document file context routing

## 목표

이 문서는 safe storage와 channel normalization이 끝난 stored attachment를 runtime file context artifact로 바꾸는 첫 routing wave를 고정한다. 목표는 이미지 native input, 텍스트 파일 extraction, PDF와 Office best effort extraction, unsupported binary note only 처리를 provider 호출 직전 context assembly가 소비할 수 있는 형태로 구현하는 것이다.

이 PRD는 audio와 video 분석을 구현하지 않는다. Audio와 video는 status와 metadata를 pass through 하거나 unsupported note로 남기며, bounded audio analysis는 PRD 003, bounded video analysis와 projection은 PRD 004가 소유한다.

## SPEC 입력

1. 주관 spec: `docs/specs/027-channel-attachment-intake-and-file-context/SPEC.md`
2. 교차 의존:
   1. `docs/specs/009-context-assembly-and-compaction-input/SPEC.md`
   2. `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`
   3. `docs/specs/013-user-interfaces-and-session-ux/SPEC.md`
   4. `docs/specs/026-context-files-and-inline-references/SPEC.md`
3. 선행 PRD:
   1. `docs/specs/027-channel-attachment-intake-and-file-context/prds/000-stored-attachment-model-and-media-root-safety.md`
   2. `docs/specs/027-channel-attachment-intake-and-file-context/prds/001-channel-intake-normalizers.md`

## Dependency Cut

1. 009는 provider 호출 직전 context assembly와 budget 적용 경계를 제공한다.
2. 010은 stored attachment readback, parser boundary, diagnostics redaction 기준을 제공한다.
3. 013은 included, skipped, blocked, unsupported 같은 사용자 표시 의미를 제공한다.
4. 026은 명시적 inline reference extraction과 channel uploaded attachment extraction이 섞이지 않도록 경계를 제공한다.
5. PRD 000은 safe stored attachment record와 detected MIME을 제공한다.
6. PRD 001은 channel별 payload를 같은 stored attachment 경로로 넣는다.
7. PRD 003과 PRD 004는 audio와 video family가 이 PRD에서 분석되지 않는다는 boundary를 소비한다.

## 범위

1. stored image attachment를 provider native image input 후보로 만드는 routing
2. provider와 model의 native image input capability가 없을 때 note only artifact로 남기는 처리
3. plain text, Markdown, JSON, CSV, log 계열 파일의 bounded text extraction
4. PDF와 Office 문서의 best effort text extraction
5. password protected, parser failure, unsupported document format의 note only 처리
6. unsupported binary의 metadata note only artifact
7. context budget, truncation evidence, extraction status 기록
8. audio와 video attachment의 analysis 미수행 status pass through

## 범위 제외

1. stored attachment model과 channel adapter 구현
2. OCR
3. full PDF layout parsing
4. Office macro execution과 embedded object recursion
5. archive recursion
6. audio transcription, audio summary, codec analysis
7. video subtitle extraction, keyframe extraction, scene summary
8. native outbound file delivery
9. provider별 모든 native file attachment feature 완성 선언

## 구현 요구사항

1. routing 입력은 PRD 000의 stored attachment record 또는 그 record에서 파생되어 configured media root 안에서 다시 검증되는 stored attachment reference여야 한다. Channel raw payload나 arbitrary host path를 직접 받으면 안 된다.
2. routing 기준은 detected MIME과 content family여야 한다. declared MIME은 mismatch 설명에만 쓴다.
3. 이미지 family는 provider와 model이 native image input을 지원할 때만 native image block으로 들어간다.
4. provider가 native image input을 지원하지 않으면 image note only artifact를 만들고 이미지를 분석한 것처럼 provider에게 말하지 않는다.
5. 텍스트 계열 파일은 encoding sniffing, file size cap, extraction byte cap, context token budget을 통과한 bounded excerpt만 artifact로 만든다.
6. PDF와 Office 문서는 best effort text extraction만 수행한다. OCR, layout reconstruction, macro execution, embedded object recursion은 실행하지 않는다.
7. 추출 결과는 source filename, detected MIME, byte length, truncation 여부, extraction method를 함께 가진다.
8. extraction 결과가 context budget을 넘으면 truncation evidence를 남기고 bounded excerpt만 provider context 후보로 넘긴다.
9. unsupported binary는 filename, detected MIME, byte length, digest prefix, unsupported reason을 note only artifact로 만든다.
10. audio와 video는 이 PRD에서 분석하지 않는다. stored status와 family를 유지하고 PRD 003 또는 PRD 004가 처리할 수 있게 넘기거나 analyzer missing note로 남긴다.
11. extraction 실패는 turn panic이 아니라 `extraction_failed` artifact로 표현한다.
12. provider context artifact는 system, developer, runtime instruction을 밀어내면 안 된다.

## 데이터/상태 모델

1. `FileContextRoutingInput`: stored attachment 또는 media-root-contained stored attachment reference, provider capability snapshot, context budget, runtime policy.
2. `FileContextArtifact`: attachment id, artifact kind, display name, detected MIME, content family, status, body, metadata, truncation.
3. `ArtifactKind`: native image, extracted text, document text, note only, deferred media.
4. `ExtractionStatus`: included native, included text, truncated, unsupported, extraction failed, deferred.
5. `ExtractionMetadata`: extraction method, byte range, char count, token estimate, digest prefix, reason.
6. `DocumentExtractionError`: password protected, parser unavailable, malformed document, budget exceeded, unsupported format.

## 정상 시퀀스

1. runtime이 stored attachment 목록 또는 session media에 남은 stored attachment reference와 provider capability snapshot을 routing service에 전달한다.
2. routing service가 detected MIME 기준으로 image, text, document, audio, video, binary family를 나눈다.
3. image family는 native image input capability를 확인하고 가능하면 native image artifact를 만든다.
4. text family는 bounded text extraction을 수행하고 truncation metadata를 붙인다.
5. PDF와 Office family는 best effort text extraction을 수행하고 성공한 excerpt를 document text artifact로 만든다.
6. unsupported binary는 note only artifact로 남긴다.
7. audio와 video는 이 PRD에서 분석하지 않고 deferred 또는 unsupported status로 후속 단계에 넘긴다.
8. context assembly는 artifact status와 budget을 기준으로 provider input을 만든다.

## 실패 시퀀스

1. stored attachment file을 media root containment 안에서 다시 열 수 없으면 extraction failed artifact를 만든다.
2. provider가 native image input을 지원하지 않으면 image note only artifact를 만들고 image bytes를 text처럼 넣지 않는다.
3. 텍스트 encoding을 안전하게 판정할 수 없으면 unsupported 또는 extraction failed reason을 남긴다.
4. PDF나 Office parser가 실패하면 extraction failed note를 만들고 문서를 읽은 것처럼 답하지 않게 한다.
5. password protected 문서는 note only 또는 extraction failed로 남긴다.
6. extraction budget을 넘으면 truncated artifact를 만들고 잘린 사실을 metadata에 남긴다.
7. audio나 video가 들어오면 이 PRD는 분석하지 않고 PRD 003 또는 PRD 004의 책임으로 남긴다.

## 검증 관점

1. image attachment가 provider native image capability가 있을 때만 native image artifact가 되는지 확인한다.
2. native image capability가 없을 때 note only가 남고 분석된 것처럼 provider prompt에 들어가지 않는지 확인한다.
3. text file extraction이 byte cap과 context budget을 지키는지 확인한다.
4. PDF와 Office extraction failure가 note only 또는 extraction failed status로 남는지 확인한다.
5. unsupported binary가 contents 없이 metadata note만 만드는지 확인한다.
6. audio와 video가 이 PRD에서 transcription이나 scene summary를 만들지 않는지 확인한다.
7. absolute host path와 raw oversized content가 artifact body나 diagnostics에 들어가지 않는지 확인한다.

## 현재 구현 상태

2026-06-18 현재 이 PRD는 stored attachment routing 기준으로 구현 완료 상태다. `crates/shacs-core/src/runtime/context.rs`의 `ContextBuilder`는 configured media root를 받아 session media에 남은 `attachments/<channel>/...` stored attachment reference를 `crates/shacs-core/src/runtime/file_context.rs`의 routing layer로 넘긴다. Routing layer는 해당 reference를 media root 안에서 다시 검증한 뒤 artifact를 만들며, typed `StoredAttachment` record 전체가 session history에 직렬화되어야 한다고 요구하지 않는다. 기존 workspace image media path의 image URL block 동작은 기본값으로 유지된다.

구현된 routing은 image family를 provider/model native image capability가 있을 때만 native image URL input으로 넣고, capability가 없으면 note only artifact로 남긴다. text/PDF/Office 계열은 bounded text extraction, truncation status, 또는 `extraction_failed` note로 표현하며 Office ZIP text entry decompression에는 byte cap을 둔다. unsupported binary는 contents 없이 metadata note만 남기며, original symlink leaf는 canonical routing 전에 blocked note로 거부한다. audio/video는 이 PRD에서 분석하지 않고 `deferred` note로만 남긴다. PRD 003의 audio analysis와 PRD 004의 video analysis/projection은 여전히 별도 범위다.

## 완료 기준

1. stored attachment 또는 media-root-contained stored attachment reference만 routing input으로 받는다.
2. image native input은 provider capability가 있을 때만 생성된다.
3. text extraction은 bounded excerpt와 truncation evidence를 만든다.
4. PDF와 Office extraction은 best effort text만 수행하고 실패 status를 남긴다.
5. unsupported binary는 note only artifact로 남고 contents를 provider에게 넘기지 않는다.
6. audio와 video는 이 PRD에서 분석되지 않고 후속 PRD 책임으로 분리된다.
7. context assembly handoff가 included native, included text, truncated, unsupported, extraction failed, deferred status를 구분한다.
