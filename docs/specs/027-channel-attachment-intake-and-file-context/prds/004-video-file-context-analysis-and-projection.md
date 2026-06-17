# PRD 004. video file context analysis and projection

## 목표

이 문서는 safe storage와 channel normalization을 통과한 video attachment를 bounded video context artifact와 user facing projection으로 바꾸는 실행 범위를 고정한다. 목표는 metadata와 duration, subtitle extraction when available, PRD 003 capability를 소비한 audio track transcription, keyframe 또는 scene summary, inspect, channel, local API projection, diagnostics를 하나의 닫힌 경로로 구현하는 것이다.

이 PRD는 native outbound file delivery를 범위에 넣지 않는다. Video 파일을 provider나 channel로 다시 보내는 기능이 아니라, 사용자가 보낸 video가 어떤 정도로 context에 포함됐는지 설명하고 제한된 evidence를 provider input 후보로 넘기는 기능이다.

## SPEC 입력

1. 주관 spec: `docs/specs/027-channel-attachment-intake-and-file-context/SPEC.md`
2. 교차 의존:
   1. `docs/specs/009-context-assembly-and-compaction-input/SPEC.md`
   2. `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`
   3. `docs/specs/013-user-interfaces-and-session-ux/SPEC.md`
   4. `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`
3. 선행 PRD:
   1. `docs/specs/027-channel-attachment-intake-and-file-context/prds/000-stored-attachment-model-and-media-root-safety.md`
   2. `docs/specs/027-channel-attachment-intake-and-file-context/prds/001-channel-intake-normalizers.md`
   3. `docs/specs/027-channel-attachment-intake-and-file-context/prds/002-image-document-file-context-routing.md`
   4. `docs/specs/027-channel-attachment-intake-and-file-context/prds/003-audio-file-context-analysis.md`

## Dependency Cut

1. 009는 video context artifact가 provider context budget을 초과하지 않게 하는 handoff 기준을 제공한다.
2. 010은 untrusted video parser와 temp file safety, diagnostics redaction 기준을 제공한다.
3. 013은 channel reply, local API response, inspect surface의 사용자 표시 의미를 제공한다.
4. 014는 analyzer diagnostics와 projection evidence가 raw secret과 oversized content를 담지 않게 하는 기준을 제공한다.
5. PRD 000은 safe stored attachment와 byte cap을 제공한다.
6. PRD 001은 channel별 video payload를 같은 stored attachment로 정규화한다.
7. PRD 003은 video audio track transcription에 소비할 audio analyzer capability를 제공한다.

## 범위

1. video detected MIME과 content family routing
2. video metadata, duration, codec hint 추출
3. video duration cap과 byte cap 적용
4. subtitle track이 있을 때 bounded subtitle text extraction
5. audio track이 있고 PRD 003 analyzer capability가 있을 때 bounded transcription 또는 summary 생성
6. keyframe 또는 scene summary의 bounded extraction
7. video context artifact의 context budget과 truncation evidence
8. channel reply, local API response, inspect surface projection
9. unsupported codec, missing analyzer, subtitle 없음, keyframe 추출 실패, budget 초과 diagnostics

## 범위 제외

1. stored attachment 모델과 channel adapter 구현
2. audio analyzer 내부 구현 자체
3. 모든 video codec 지원
4. 완전한 장면 이해 또는 frame 단위 질의 응답
5. OCR
6. embedded archive recursion
7. native outbound file delivery
8. hosted video gallery 또는 remote asset library
9. 장기 video index와 semantic search

## 구현 요구사항

1. video analysis 입력은 PRD 000의 stored attachment여야 한다.
2. analyzer 실행 전에 detected MIME, byte length, duration metadata를 확인하고 policy cap을 적용해야 한다.
3. metadata extraction은 duration, container, codec hint, resolution when available, track availability를 bounded form으로 기록해야 한다.
4. duration cap을 넘는 video는 full analysis를 실행하지 않고 skipped 또는 unsupported reason을 남긴다.
5. subtitle track이 있으면 bounded subtitle extraction을 수행하고 language, char count, truncation metadata를 남긴다.
6. subtitle track이 없으면 실패처럼 포장하지 않고 `subtitle_unavailable` reason을 metadata에 남긴다.
7. audio track transcription은 PRD 003의 audio analyzer capability를 소비해야 한다. 별도 video 전용 transcription path를 새로 만들지 않는다.
8. audio track을 추출할 수 없거나 analyzer가 없으면 user visible reason을 남기고 video metadata와 다른 가능한 artifact는 유지한다.
9. keyframe 또는 scene summary는 max frames, max scenes, max summary chars, context token budget을 지켜야 한다.
10. keyframe image bytes를 provider에게 native outbound file delivery처럼 보내는 것은 범위 밖이다. 필요한 경우 summary와 metadata만 bounded context artifact로 만든다.
11. video artifact는 metadata, subtitles excerpt, audio transcript excerpt, scene summary 각각의 included, skipped, failed, truncated 상태를 구분해야 한다.
12. projection은 channel reply, local API response, inspect surface에서 파일별 status와 reason을 보여줘야 한다.
13. diagnostics에는 absolute host path, raw video bytes, raw oversized subtitles, analyzer token, signed URL을 넣지 않는다.

## 데이터/상태 모델

1. `VideoAnalysisPolicy`: max bytes, max duration seconds, max keyframes, max scenes, max subtitle chars, max transcript chars, max summary chars.
2. `VideoContextRequest`: stored attachment id, media root relative path, detected MIME, byte length, duration hint, policy.
3. `VideoMetadata`: duration, container, video codec, audio codec, resolution, subtitle tracks, audio track available.
4. `VideoContextArtifact`: attachment id, status, metadata, subtitles excerpt, audio transcript artifact ref, scene summary, keyframe summary, truncation, diagnostics.
5. `VideoComponentStatus`: included, skipped, unsupported, extraction failed, truncated, unavailable.
6. `VideoAnalysisReason`: duration exceeded, unsupported codec, analyzer missing, subtitle unavailable, audio track unavailable, keyframe extraction failed, scene summary failed, budget exceeded.
7. `AttachmentProjection`: attachment id, display name, family, status, included components, skipped components, reason, redacted relative path.

## 정상 시퀀스

1. routing service가 stored attachment 중 video family를 찾는다.
2. video metadata reader가 duration, codec hint, resolution, track availability를 bounded 방식으로 확인한다.
3. policy가 byte length와 duration cap을 검사한다.
4. subtitle track이 있으면 bounded subtitle extraction을 수행한다.
5. audio track이 있고 PRD 003 analyzer capability가 있으면 bounded transcription 또는 summary를 요청한다.
6. video analyzer가 제한된 keyframe 또는 scene summary를 만든다.
7. runtime이 metadata, subtitle excerpt, audio transcript ref, scene summary, truncation evidence를 `VideoContextArtifact`로 포장한다.
8. context assembly가 budget 안에서 video artifact를 provider input 후보로 넣는다.
9. projection layer가 channel reply, local API response, inspect surface에 included, skipped, unsupported, extraction failed, truncated status를 표시한다.

## 실패 시퀀스

1. stored file readback이 media root containment를 통과하지 못하면 extraction failed를 남긴다.
2. duration cap을 넘으면 subtitle, audio, keyframe analyzer를 실행하지 않고 skipped 또는 unsupported reason을 남긴다.
3. unsupported codec이면 가능한 metadata만 남기고 content analysis는 unsupported로 표시한다.
4. subtitle track이 없으면 unavailable reason을 남기되 전체 video analysis 실패로 처리하지 않는다.
5. audio track 추출이나 PRD 003 analyzer가 실패하면 audio component만 extraction failed 또는 unsupported로 남긴다.
6. keyframe 또는 scene analyzer가 실패하면 해당 component만 extraction failed로 남기고 metadata와 subtitle 결과를 유지한다.
7. context budget을 넘으면 component별 truncation evidence를 남긴다.

## 검증 관점

1. duration cap 초과 video가 analyzer 실행 없이 user visible status를 남기는지 확인한다.
2. metadata와 duration이 context artifact와 inspect projection에 포함되는지 확인한다.
3. subtitle track이 있을 때만 bounded subtitle extraction이 실행되고 없을 때 unavailable reason이 남는지 확인한다.
4. audio track transcription이 PRD 003 analyzer capability를 소비하는지 확인한다.
5. keyframe 또는 scene summary가 max frame, max scene, summary length budget을 지키는지 확인한다.
6. component 하나의 실패가 전체 video item silent drop으로 이어지지 않는지 확인한다.
7. channel reply, local API response, inspect surface가 included, truncated, unsupported, extraction failed reason을 구분하는지 확인한다.
8. native outbound file delivery가 구현 범위에 포함되지 않았는지 확인한다.
9. diagnostics에 absolute path, signed URL, raw video bytes, raw oversized subtitles가 들어가지 않는지 확인한다.

## 현재 구현 상태

2026-06-17 현재 이 PRD 전체는 구현 완료 상태가 아니다. 현재 `shacs-bot`에 video attachment가 공통 channel intake, safe stored attachment, duration cap, subtitle extraction, audio track transcription, keyframe 또는 scene summary, user facing projection을 거쳐 production turn context에 들어간다고 볼 evidence는 없다.

이미지 media path 일부 지원과 문서 utility는 video analysis 구현 evidence가 아니다. 현재 상태에서 video metadata 분석, subtitle extraction, audio track transcription, keyframe, scene summary, inspect projection을 production 지원으로 주장하면 안 된다.

## 완료 기준

1. video stored attachment가 detected MIME 기준으로 video analysis route에 들어간다.
2. byte cap과 duration cap이 subtitle, audio, keyframe analyzer 실행 전에 적용된다.
3. metadata와 duration이 video context artifact와 inspect projection에 남는다.
4. subtitle track이 있으면 bounded extraction이 수행되고 없으면 unavailable reason이 남는다.
5. audio track transcription은 PRD 003 analyzer capability를 소비한다.
6. keyframe 또는 scene summary가 bounded artifact로 생성되거나 component failure로 기록된다.
7. channel reply, local API response, inspect surface가 included, skipped, unsupported, extraction failed, truncated status를 구분한다.
8. native outbound file delivery가 scope 밖으로 유지된다.
