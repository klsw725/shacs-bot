# PRD 010. 영상 안내와 analyzer 코드·034 runner 정리

Status: Complete (Scoped)

Depends on: [PRD 000](000-record-surfaces-fixed.md) 완료. 뒤 번호 PRD에는 의존하지 않는다.

## Goal

영상 첨부를 고정 안내 하나로 모델에 알리고, video analyzer·분석 기록·replay 코드와 공개 API를 없애 유지 비용을 거둔다. 그 코드에 기대던 034 release runner를 같은 변경에서 맞춘다.

## Scope

1. **영상 note:** stored attachment routing에서 영상 계열은 `[attachment:unsupported]` note 하나가 된다. 필드(name, channel, mime, bytes, sha256_prefix)는 지금과 같고 reason은 `video content analysis is not supported`다. 영상 길이 판정, analyzer 호출, staging, 분석 기록 쓰기를 거치지 않는다. 첨부 intake와 byte 한도, 이미지·문서·오디오 routing은 바꾸지 않는다.
2. **삭제:**
   - `shacs-core`: `video_analyzer_runtime`, `video_analyzer_evidence`·`video_analyzer_bounds`, `video_analyzer_disclosure`, `video_analyzer_owner_facts`, `video_analyzer_spec035`(`Spec035MediaProjectionStore` 포함), `media_evidence_replay`, file context의 video route·blocks·types와 video helper, `ContextBuilder`의 `with_video_analyzer`·`with_video_analyzer_staging_root`·`with_video_analyzer_owner_refs`·`with_video_media_provenance`·`with_video_projection_publication`, `ContextBuildRequest.analyzer_invocation`, turn control의 analyzer invocation과 그 재노출.
   - `shacs-projection`: Spec035 media projection 형식(`spec035/media_*`).
   - PRD 000 뒤에도 남은 API·채널의 분석 기록 형식 코드.
   - 위 동작을 고정하던 테스트·예제([040 회귀 테스트 5](../../040-video-analyzer-capability-disposition/prds/000-video-runtime-value-and-compatibility-decision.md#구현-때-필요한-회귀-테스트)의 목록).
3. **남기는 것:** PRD 000의 고정 결과, 오디오 analyzer 추상화와 `TranscriptionAudioAnalyzer`, `audio/mp4` 길이 읽기, turn의 취소·마감 전달, 생성 artifact 형식과 `image_generate`, 034 runner의 `bounded_evidence_is_not_complete_video_understanding` non-guarantee와 034 문서 정책 파일.
4. **034 release runner:**
   - `SPEC034_REQUIREMENTS`의 22개 식별자는 과거 catalog로 남기고 퇴역한 9개를 따로 정의한다.
   - 새 run의 coverage는 현재 13개만 통합 테스트 근거로 담고, 퇴역한 9개는 근거 없이 퇴역 사유 `040`과 함께 따로 기록한다. owner audit은 `current_os_authority`, `network_guard`, `credential_status`, `sandbox_scope`, `raw_content_disclosure`만 담는다. manifest·coverage·owner audit의 schema version을 올리고 요구사항 수 검증을 현재 수로 바꾼다.
   - 034 release evidence 형식(`spec034.release_evidence`, `spec034.owner_facts`)도 같은 기준으로 version을 올린다. 퇴역한 요구사항이나 owner 종류가 들어오면 거부한다.
   - 통합 테스트(`spec034_sequential_integration`과 example fixture)는 analyzer·분석 기록 표면·replay 부분을 지우고 13개 receipt와 퇴역 목록을 확인한다. `034-AC006`의 금지 자료 검사는 생성 artifact·edit·remote output 직렬화로 다시 묶고 금지 항목 목록은 줄이지 않는다.
5. **사용자 문서:** `docs/USAGE.md`의 로컬 API 첨부 설명과 Spec 034 media 절의 영상 설명을 새 note로 고친다.

## Non Scope

- Spec031 runner, 공통 문서, 최종 확인(PRD 020).
- 오디오 analyzer 연결, provider native video input 같은 새 기능, 034 문서 정책의 변경.

## Parent Requirement Mapping

- [044 SPEC](../SPEC.md)의 소유 범위 2·3과 수용 조건 1·3·4를 소유한다.

## Required Contract

1. 영상 note는 모든 영상 입력에서 같은 reason을 쓰며 설정으로 켤 수 있다는 뜻이나 길이 문구가 없다. 영상을 조용히 빼거나 본 것처럼 표시하지 않는다.
2. 삭제는 남긴다고 정한 항목을 건드리지 않는다. 분석 기록을 읽거나 쓰는 production 경로가 남지 않는다.
3. 034 runner를 고쳐도 지운 테스트 때문에 blocked가 생기지 않고, 퇴역한 요구사항·owner 종류를 현재 PASS로 보고하지 않으며, 남은 요구사항의 근거가 바뀌지 않는다.

## Acceptance Criteria

1. 실제 MP4·MOV, EBML 헤더 WebM, 깨진 MP4를 API로 보낸 provider 요청에 고정 reason의 note가 하나씩 있고, `media/projections/`와 staging 폴더가 생기지 않는다. 이미지·문서·오디오 routing 테스트가 그대로 통과한다.
2. [044 SPEC](../SPEC.md) 수용 조건 3의 검색 결과가 정한 범위뿐이다. fmt, workspace clippy `-D warnings`, 변경한 package의 테스트가 통과한다.
3. 034 runner의 새 run이 13개 coverage, 9개 퇴역 기록, 5종 owner audit을 올린 schema version으로 내고, runner·schema·통합 테스트가 통과한다.

## Closure Evidence

1. 실제 영상 업로드의 provider 요청 기록.
2. 삭제 범위 검색 결과와 Cargo 검증 기록.
3. 034 runner의 catalog·schema·테스트 변경과 결과, 고친 문서.

PRD 010은 PRD 000의 닫힌 결과를 소비한다. 다른 신규 기능이나 041의 판정을 기다리지 않는다.

## 구현 메모 및 검증 근거 (2026-10-25)

- 영상 note: `file_context/spec034_context/attachment_dispatch.rs`에서 영상 계열은 `base_note.with_reason("video content analysis is not supported")`의 `unsupported` note 하나가 된다. 길이 판정, analyzer 호출, staging, 기록 쓰기를 거치지 않는다. `route_stored_attachment`는 경로·media root·이미지 입력 지원 여부·오디오 analyzer만 받는다.
- 삭제:
  - `shacs-core`: `video_analyzer_runtime`(staging·supervisor 포함), `video_analyzer_bounds`·`_evidence`·`_disclosure`·`_owner_facts`·`_spec035`(`Spec035MediaProjectionStore` 포함), `media_evidence_replay`, file context의 `video_route`·`video_blocks`·`video_types`와 video helper.
  - `ContextBuilder`의 video 주입 API 전부: `context/video.rs`는 오디오만 남긴 `context/media.rs`가 됐다. `ContextBuildRequest.analyzer_invocation`, `analyzer_turn_control`·`agent_loop`의 analyzer invocation, `runtime/mod.rs`의 재노출도 지웠다.
  - `shacs-projection`: `spec035/media_*` 여덟 파일과 그 테스트·예제.
  - 테스트·예제: [040 회귀 테스트 5](../../040-video-analyzer-capability-disposition/prds/000-video-runtime-value-and-compatibility-decision.md#구현-때-필요한-회귀-테스트)의 목록과 같다. `runtime_agent.rs`의 주입 테스트는 `runtime_context_routes_stored_video_as_unsupported_note`로 바꿨다.
- 남긴 것:
  - 오디오 analyzer와 `TranscriptionAudioAnalyzer`, `audio/mp4` 길이 읽기(`sniff_mp4_duration_seconds`, `blocks_m4a_over_duration_cap_before_analyzer`).
  - turn 취소·마감 전달(`analyzer_turn_control`), 생성 artifact와 `image_generate`.
  - 034 runner의 `bounded_evidence_is_not_complete_video_understanding` non-guarantee와 `documentation-policy.json`.
- 034 release runner:
  - catalog: `requirements()`는 현재 13개, `retired_requirements()`는 9개를 퇴역 사유 `040`과 함께 돌려준다.
  - schema: run manifest `spec034.release_runner.v3`, coverage `spec034.coverage.v2`(`retired_requirements` 추가), owner audit `spec034.runner_owner_audits.v2`(5종)으로 올렸다. 검증은 catalog와 정확히 같은지 보고, 요구사항 수는 현재 수와 비교한다.
  - `shacs-projection`의 `spec034.release_evidence.v2`·`spec034.owner_facts.v2`는 퇴역 식별자(`RetiredRequirement`)와 퇴역 owner 종류를 거부한다.
  - 통합 테스트와 example fixture는 13개 receipt와 퇴역 목록을 확인한다. `034-AC006`은 생성 artifact·edit·remote output 직렬화에서 같은 금지 항목 8종이 없는지 본다.
- 테스트: `file_context`의 `routes_every_stored_video_as_one_unsupported_note`, `runtime_agent`의 위 테스트, `spec034_release_runner`(success-fixture run의 `requirement_count` 13), `spec034_sequential_integration` 2개, `spec034_evidence_schema` 7개.
- 문서: `docs/USAGE.md`의 로컬 API 첨부 설명과 Spec 034 media 절을 새 note와 13/9 보고로 고쳤다.
- 삭제 범위 검색: `VideoContextAnalyzer`·`with_video_analyzer`·`Spec035MediaProjectionStore`·`Spec035MediaProjection`·`media_evidence_replay`·`AnalyzerInvocation`·`analyzer_invocation`은 코드에 없다. 문서에는 040·044, 보관 사본, 027 PRD 004(역사 기록)에만 남는다.
- 실제 업로드(`.omo/evidence/spec044/final-qa/transcript.md` U1–U3): 새 data dir에서 실제 MP4·MOV, EBML 헤더만 있는 WebM, 깨진 MP4를 API로 보냈다. provider 요청마다 고정 reason의 note가 하나씩 있었고 영상 내용 표지는 없었다(`m1=no m2=no`). `media/projections/`와 staging 폴더는 생기지 않았다. PNG·WAV note는 044 이전 binary와 같았다.

## 종료 확인 (2026-10-25)

- 범위: 영상 note, analyzer·분석 기록·replay 코드와 공개 API·테스트·예제 삭제, 034 runner 정리, 영상 안내의 사용자 문서. Spec031 runner와 최종 확인(PRD 020)은 포함하지 않는다.
- 최종 검증(변경한 package, `--no-fail-fast`, 기록은 `.omo/evidence/spec044/final-tests/`):
  - `shacs-core` 1,577 passed / 1 failed / 3 ignored. 유일한 실패는 PRD 000과 같은 baseline 실패다(`mismatch_uses_isolated_tree`, Spec034 로컬 evidence 파일 누락).
  - `shacs-projection` 344 passed / 7 ignored. `shacs-cli` 470 passed / 3 ignored, `shacs-api` 117, `shacs-tui` 37, `shacs-channels` 33 passed.
  - `cargo fmt --all -- --check`와 workspace clippy `-D warnings`도 통과했다.
  - 전체 workspace 실행은 Spec 044 마무리 때 사용자 승인을 받아 했다([SPEC 종료 증거](../SPEC.md#종료-증거-2026-10-25)).
- 실제 업로드: 위 구현 메모의 U1–U3.
- Oracle 독립 검증: **PASS** (2차).
  - 1차는 BLOCKED였다. 유일한 지적은 `shacs-core` 실행의 위 baseline 실패였다.
  - 수정 전 소스(`95254eb`)에서 같은 실패를 재현한 기록(`final-tests/baseline-95254eb-mismatch_uses_isolated_tree.log`)이 근거였다. [AGENTS.md](../../../../AGENTS.md)의 "무관한 기존 실패는 고치지 않고 이름만 보고한다" 규칙도 함께 근거로 냈다. 2차에서 지적이 철회됐고 코드는 바꾸지 않았다.
- 남은 제한: 영상 내용 분석은 제공하지 않는다. 034의 과거 22/22 기록은 다시 해석하지 않는다.
