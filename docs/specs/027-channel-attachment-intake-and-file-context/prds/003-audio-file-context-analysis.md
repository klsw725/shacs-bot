# PRD 003. audio file context analysis

## 목표

이 문서는 safe storage와 channel normalization을 통과한 audio attachment를 bounded transcription 또는 bounded summary file context artifact로 만드는 실행 범위를 고정한다. 목표는 duration cap, analyzer capability abstraction, unsupported codec, analyzer missing 동작을 명확히 해서 audio 파일이 silent drop되지 않고 사용자가 이해할 수 있는 status로 투영되게 하는 것이다.

이 PRD는 video scene, keyframe, subtitle extraction을 다루지 않는다. Video 파일의 user facing projection과 scene level context extraction은 PRD 004가 소유한다.

## SPEC 입력

1. 주관 spec: `docs/specs/027-channel-attachment-intake-and-file-context/SPEC.md`
2. 교차 의존:
   1. `docs/specs/003-provider-runtime/SPEC.md`
   2. `docs/specs/009-context-assembly-and-compaction-input/SPEC.md`
   3. `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`
   4. `docs/specs/013-user-interfaces-and-session-ux/SPEC.md`
3. 선행 PRD:
   1. `docs/specs/027-channel-attachment-intake-and-file-context/prds/000-stored-attachment-model-and-media-root-safety.md`
   2. `docs/specs/027-channel-attachment-intake-and-file-context/prds/001-channel-intake-normalizers.md`
   3. `docs/specs/027-channel-attachment-intake-and-file-context/prds/002-image-document-file-context-routing.md`

## Dependency Cut

1. 003은 provider capability와 optional analyzer client resolution 패턴을 제공한다.
2. 009는 audio transcript와 summary가 context budget을 초과하지 않게 하는 handoff 기준을 제공한다.
3. 010은 untrusted media parser와 analyzer diagnostics redaction 기준을 제공한다.
4. 013은 unsupported, extraction failed, truncated status의 사용자 표시 기준을 제공한다.
5. PRD 000은 duration analysis 전에 safe stored attachment와 byte cap을 보장한다.
6. PRD 001은 audio가 어느 channel에서 왔는지와 무관하게 stored attachment로 들어오게 한다.
7. PRD 004는 video audio track transcription이 필요할 때 이 PRD의 analyzer capability를 소비한다.

## 범위

1. audio detected MIME과 content family routing
2. audio duration metadata 추출과 duration cap 적용
3. analyzer capability abstraction
4. bounded transcription artifact
5. bounded audio summary artifact
6. unsupported codec status와 analyzer missing status
7. transcript, summary, metadata의 context budget 적용과 truncation evidence
8. local API, channel reply, inspect surface가 소비할 audio processing status

## 범위 제외

1. stored attachment 모델과 channel adapter 구현
2. PDF, Office, text extraction
3. video subtitle extraction
4. video keyframe extraction과 scene summary
5. speaker diarization 완성 선언
6. 모든 audio codec 지원
7. 무제한 원문 전사
8. native outbound audio delivery
9. long term audio index 또는 searchable archive

## 구현 요구사항

1. audio analysis 입력은 PRD 000의 stored attachment여야 한다.
2. analyzer 실행 전에 detected MIME, byte length, duration metadata를 확인하고 policy cap을 적용해야 한다.
3. duration을 알 수 없으면 analyzer가 안전하게 처리할 수 있는 경우에만 bounded analysis를 시도하고, 그렇지 않으면 unsupported 또는 extraction failed로 남긴다.
4. `AudioContextAnalyzer` 같은 capability trait은 stored attachment reference와 policy budget을 받아 typed result를 반환해야 한다.
5. analyzer가 없으면 attachment는 analyzer missing reason을 가진 unsupported note가 되어야 한다.
6. codec이 지원되지 않으면 unsupported codec reason을 남기고 provider가 내용을 들은 것처럼 만들면 안 된다.
7. transcription은 max duration, max transcript chars 또는 token budget, max segments를 지켜야 한다.
8. summary가 제공되는 경우에도 transcript 없이 summary만 가능한지, transcript 기반 summary인지 metadata에 남겨야 한다.
9. transcript와 summary는 context budget을 초과하면 truncated status와 truncation evidence를 가져야 한다.
10. analyzer raw error, provider token, temp path, absolute host path는 diagnostics에 노출하지 않는다.
11. analysis 실패는 turn panic이 아니라 attachment level extraction failed로 남긴다.
12. video 파일에서 분리된 audio track을 분석하는 호출은 PRD 004가 orchestration하고, 이 PRD는 audio analyzer capability만 제공한다.

## 데이터/상태 모델

1. `AudioAnalysisPolicy`: max bytes, max duration seconds, max transcript chars, max summary chars, max segments.
2. `AudioAnalyzerCapability`: supported MIME families, max duration, transcription support, summary support, language detection support.
3. `AudioContextRequest`: stored attachment id, media root relative path, detected MIME, byte length, duration hint, policy.
4. `AudioContextArtifact`: attachment id, status, duration, language, transcript excerpt, summary, segments, truncation, analyzer id.
5. `AudioAnalysisStatus`: included text, truncated, unsupported, extraction failed.
6. `AudioAnalysisReason`: analyzer missing, unsupported codec, duration exceeded, metadata unavailable, analyzer failed, budget exceeded.

## 정상 시퀀스

1. routing service가 stored attachment 중 audio family를 찾는다.
2. audio metadata reader가 duration과 codec hint를 bounded 방식으로 확인한다.
3. policy가 byte length와 duration cap을 검사한다.
4. analyzer resolver가 사용 가능한 `AudioContextAnalyzer` capability를 찾는다.
5. analyzer가 bounded transcription 또는 summary를 만든다.
6. runtime이 transcript, summary, duration, language, truncation metadata를 `AudioContextArtifact`로 포장한다.
7. context assembly는 budget 안에서 audio artifact를 provider input 후보로 넣는다.
8. projection layer는 included, truncated, unsupported, extraction failed status를 사용자에게 보여준다.

## 실패 시퀀스

1. stored file readback이 media root containment를 통과하지 못하면 extraction failed를 남긴다.
2. duration cap을 넘으면 analyzer를 실행하지 않고 unsupported 또는 skipped reason을 남긴다.
3. analyzer capability가 없으면 analyzer missing unsupported note를 만든다.
4. codec이 지원되지 않으면 unsupported codec note를 만든다.
5. analyzer가 실패하면 extraction failed로 남기고 raw provider response를 diagnostics에 넣지 않는다.
6. transcript가 budget을 넘으면 truncated artifact를 만들고 잘린 길이를 metadata에 남긴다.
7. language detection이 실패해도 transcript나 summary가 있으면 해당 failure는 metadata reason으로만 남긴다.

## 검증 관점

1. duration cap 초과 audio가 analyzer 실행 없이 unsupported 또는 skipped status가 되는지 확인한다.
2. analyzer missing이 silent drop이 아니라 user visible note로 남는지 확인한다.
3. unsupported codec이 audio 내용을 분석한 것처럼 provider input에 들어가지 않는지 확인한다.
4. transcript와 summary가 context budget에 맞게 잘리고 truncation evidence를 갖는지 확인한다.
5. analyzer failure가 turn panic으로 번지지 않고 extraction failed artifact가 되는지 확인한다.
6. diagnostics에 absolute path, provider token, raw audio bytes, raw oversized transcript가 들어가지 않는지 확인한다.
7. PRD 004가 video audio track 전사에 같은 analyzer capability를 재사용할 수 있는지 확인한다.

## 현재 구현 상태

2026-06-19 현재 이 PRD는 stored audio routing과 analyzer capability abstraction 기준으로 구현되어 있다. Audio attachment는 common intake, safe stored attachment, detected audio family를 거친 뒤 analyzer가 주입되어 있으면 bounded transcript 또는 summary text artifact로 라우팅된다. Analyzer 실행 전 WAV와 단순 MP4/M4A duration metadata는 bounded parser로 확인해 duration cap을 적용하며, duration을 알 수 없는 형식은 policy가 허용할 때만 analyzer로 전달된다. Analyzer가 없으면 analyzer missing unsupported note가 남고, analyzer가 unsupported 또는 failed를 반환하면 attachment-level unsupported 또는 extraction_failed note로 남는다. Core runtime은 `AudioContextAnalyzer` trait와 기존 provider `TranscriptionClient`를 감싸는 analyzer adapter를 제공한다.

구현된 범위는 typed `AudioContextAnalyzer` capability, WAV/단순 MP4/M4A duration cap, context-budget 기반 transcript/summary/language bounding, truncation evidence, untrusted transcript/summary marker다. Full codec metadata reader, diarization, segment model, language detection 완성, 또는 기본 config에서 항상 활성화되는 remote transcription provider를 완료 기능으로 주장하지 않는다. Video 파일에서 분리된 audio track orchestration은 PRD 004 범위로 남는다.

## 완료 기준

1. audio stored attachment가 detected MIME 기준으로 audio analysis route에 들어간다.
2. duration cap과 byte cap이 analyzer 실행 전에 적용된다.
3. analyzer capability abstraction이 unsupported provider와 analyzer missing을 구분한다.
4. bounded transcript 또는 summary artifact가 context budget과 truncation evidence를 지킨다.
5. unsupported codec, analyzer missing, duration exceeded, analyzer failed가 user visible status로 남는다.
6. diagnostics가 secret, absolute host path, raw oversized content를 노출하지 않는다.
7. PRD 004가 video audio track transcription에 이 capability를 소비할 수 있다.
