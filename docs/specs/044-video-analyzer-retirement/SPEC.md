# 044. 비디오 분석 기능의 퇴역과 영상 첨부 안내 유지 명세

Status: Complete (Scoped)

Consumes: 040 video analyzer disposition decision, 027 attachment intake and routing, 034 generated media artifacts and release runner, 031 Spec031 release runner, current CLI/API/WebSocket/TUI/channel surfaces

## 목적과 완료 경계

[040](../040-video-analyzer-capability-disposition/SPEC.md)은 선택적 video analyzer와 그 분석 기록 표면을 제품에서 **제거**하고, 영상 첨부의 저장과 `unsupported` note는 남기기로 판정했다. 이 스펙은 그 판정의 보존·변경 계약을 실제 코드와 표면으로 옮긴다. 040의 판정을 다시 내리지 않고, [027](../027-channel-attachment-intake-and-file-context/SPEC.md)·[034](../034-generated-media-and-rich-file-context-expansion/SPEC.md)의 과거 완료 판정도 다시 열지 않는다.

완료되면 사용자는 다음을 경험한다.

- 영상 첨부는 지금처럼 저장되고, 모델은 영상마다 `video content analysis is not supported` 안내 한 줄을 받는다. 분석 기록과 analyzer 임시 폴더는 생기지 않는다.
- `runtime inspect`·`runtime diagnostics`·API·WebSocket·TUI·외부 채널은 남은 분석 기록이 있든 없든, 손상됐든 아니든 지금 기록이 없는 data dir과 같은 결과를 낸다. 남은 기록 때문에 명령이 실패하거나 채널 답장이 사라지지 않는다.
- 남은 영상 첨부와 분석 기록은 바뀌지 않는다. 지우기는 사용자가 직접 한다.

이 스펙은 041·042의 판정이나 구현, 043의 구현을 기다리지 않는다.

## 소유 범위

1. **분석 기록 표면 고정:** production ContextBuilder는 분석 기록 publication을 붙이지 않는다. `runtime inspect`·`runtime diagnostics`·`GET /v1/media/diagnostics`·WebSocket `{"type":"media_projection"}`·TUI·외부 채널 전송은 `media/projections/current.json`을 읽지 않고 [040의 고정 결과](../040-video-analyzer-capability-disposition/prds/000-video-runtime-value-and-compatibility-decision.md#바뀌는-표면)를 낸다.
2. **영상 안내와 analyzer·분석 기록·replay 코드 삭제:** stored attachment routing이 영상을 위 안내 하나로 바꾼다. `shacs-core`의 video analyzer runtime·evidence·bounds·disclosure·owner facts·Spec035 mapping·`Spec035MediaProjectionStore`, `media_evidence_replay`, `ContextBuilder`의 video 주입 API와 video 타입, `ContextBuildRequest.analyzer_invocation`, turn control의 analyzer invocation, `shacs-projection`의 Spec035 media projection 형식, CLI·API·TUI·채널에 남은 분석 기록 변환 코드와 이 동작을 고정하던 테스트·예제를 지운다.
3. **034 release runner 정리:** 22개 요구사항 catalog를 과거 식별자로 남기되 근거가 사라진 9개(`034-MH007`–`034-MH010`, `034-AC007`–`034-AC009`, `034-AC011`, `034-AC012`)를 퇴역으로 기록하고 현재 coverage나 PASS로 보고하지 않는다. `034-AC006`은 남는 생성 artifact·edit·remote output 검사로 다시 묶는다. analyzer·표면 parity·snapshot replay에만 근거한 owner audit 종류(`trusted_analyzer_source`, `projection_parity`, `execution_snapshot`)도 퇴역한다. 바뀐 결과 형식의 schema version을 올린다.
4. **Spec031 release runner 정리:** 새 run은 034 외부 owner fact를 남는 생성 media 테스트로 확인하고 schema version을 올린다. 이전 version의 결과는 그 version의 구성으로 판독한다.
5. **공통 문서와 최종 확인:** README의 media·runner 설명, 027·034 SPEC의 퇴역 안내, `docs/PRODUCT-PATH-MAP.md`, 스펙 색인을 고친다. 044 이전 binary로 만든 실제 자료와 남은 기록으로 새 binary의 표면을 확인한다.

## 기존 owner와의 경계

| 기존 owner | 044가 소비하는 것 | 044가 소유하지 않는 것 |
|---|---|---|
| [040](../040-video-analyzer-capability-disposition/SPEC.md) | 제거 판정, 보존할 자료와 동작, 바뀌는 표면, 회귀 테스트 기준 | 판정의 재검토, 유지·축소로의 변경 |
| [027](../027-channel-attachment-intake-and-file-context/SPEC.md) | 첨부 intake·저장 위치·한도, 파일 종류별 routing, silent drop 금지 | intake 한도·저장 형식 변경, 이미지·문서·오디오 routing 변경, 027 판정의 재해석 |
| [034](../034-generated-media-and-rich-file-context-expansion/SPEC.md) | 생성 artifact 저장 형식, `image_generate`, remote output 정책, 문서 정책(`034-AC010`), runner 구조 | 생성 media 동작 변경, 과거 22/22 증거의 재해석, `Status:` 줄 변경 |
| [030](../030-trusted-agent-runtime-and-operational-controls/SPEC.md)·[031](../031-configuration-runtime-layout-and-execution-snapshots/SPEC.md) | owner fact·execution snapshot·activation record 형식, Spec031 runner 구조 | 형식 변경, 다른 외부 owner fact의 의미 변경 |
| [041](../041-release-evidence-tooling-disposition/SPEC.md) | 현재 runner 구조와 과거 evidence 판독 규칙 | runner 자체의 유지·축소·제거 판정 |
| [043](../043-app-capability-retirement/SPEC.md) | 없음 | 앱 퇴역. 두 스펙이 같은 Spec031 runner 파일을 고치면 나중에 병합하는 쪽이 schema version을 한 번 더 올린다 |

## 반드시 지킬 것

- 다음 자료를 지우거나 고쳐 쓰지 않는다: `<data>/media/attachments/`, `<data>/media/projections/`(기록과 임시 파일 포함), `<workspace>/.shacs-video-analyzer-staging/`. 이전·변환·자동 정리를 하지 않는다(`<data>`는 config가 있는 폴더).
- 제거 뒤 `media/projections/` 아래를 읽거나 만드는 production 경로가 없다.
- 이미지·문서·오디오 routing, 오디오 analyzer 추상화와 `TranscriptionAudioAnalyzer`, `audio/mp4` 길이 읽기, `message` 도구의 영상 전송, 생성 artifact 형식과 `runtime inspect`의 생성 이미지 목록은 바뀌지 않는다.
- 진단 JSON의 `spec035_media_projections` 키는 빈 배열로 남는다. `GET /v1/media/diagnostics` 경로와 404 응답, WebSocket `media_projection`의 not found 오류는 남는다.
- 지금의 동작을 고정하던 테스트는 그 동작을 거두는 PRD에서 지우거나 바꾼다. `#[ignore]`나 `#[allow(...)]`로 남기지 않는다. 사용자 문서는 동작을 바꾸는 PRD에서 함께 고친다.
- 027·034의 `Status:` 줄은 바꾸지 않는다.
- PRD들은 한 브랜치에서 번호 순서대로 구현해 한 번에 병합한다(040의 "한 변경 단위"). 각 PRD는 자기 수용 조건으로 따로 닫는다.

## 수용 조건

검증에 쓰는 **남은 자료 fixture**는 실제 형식의 파일이다. 저장된 영상 첨부는 044 이전 binary(040 판정 커밋 `95254eb`)의 API로 만든다. 분석 기록은 production 작성 경로가 없으므로(040 조사) 044 이전 코드에 가짜 analyzer를 주입한 기록(040 h07과 같은 형식)과, 잘린 JSON으로 손상된 기록을 쓴다. staging 폴더는 빈 폴더와 파일이 든 폴더를 둔다.

1. 실제 MP4·QuickTime MOV, EBML 헤더만 있는 WebM, 깨진 MP4를 API로 보내면 모델 입력에 `[attachment:unsupported] name=… channel=… mime=… bytes=… sha256_prefix=… reason=video content analysis is not supported`가 하나씩 들어가고 `media/projections/`와 staging 폴더가 생기지 않는다. 이미지·문서·오디오 note는 지금과 같다.
2. fixture에서 `runtime inspect`와 `runtime diagnostics --bundle`이 종료 코드 0으로 `Spec035 media projections: unavailable (canonical records unavailable)`과 빈 `spec035_media_projections`를 내고, `/v1/media/diagnostics`가 404 `media projection is unavailable`, WebSocket `media_projection`이 not found, TUI `--once`가 `media: state=unavailable reason=unavailable freshness=unavailable`, `media lineage: unavailable`, `media disclosure: unavailable`을 내며, 외부 채널 전송이 media metadata 없이 전달된다. fixture 파일의 목록과 sha256은 그대로다.
3. workspace에 analyzer·분석 기록·replay 코드와 공개 API가 없다. `VideoContextAnalyzer`·`with_video_analyzer`·`Spec035MediaProjectionStore`·`Spec035MediaProjection`·`media_evidence_replay`·`AnalyzerInvocation` 검색 결과는 문서의 역사 기록과 퇴역 안내뿐이다. fmt, workspace clippy `-D warnings`, 변경한 package의 테스트가 통과한다. 전체 workspace 테스트는 사용자 승인을 받아 실행한다.
4. 034 runner의 새 run은 올린 schema version으로 현재 요구사항 13개만 coverage에 담고, 퇴역한 9개를 근거 없이 퇴역 사유(`040`)와 함께 따로 기록하며, owner audit은 남은 5종만 담는다. 통합 테스트는 13개 receipt와 퇴역 목록을 확인하고 `034-AC006`은 남는 출력에서 금지 자료가 없음을 확인한다. runner 테스트가 통과한다.
5. Spec031 runner의 새 run은 올린 schema version으로 034 외부 owner fact를 `spec034_persistence_operations`의 생성 media 테스트로 확인한다. v3·v2 결과는 지금과 같은 구성으로 판독되고, 보존된 v2 G7 archive의 검증 테스트가 지금과 같은 결과로 통과한다.
6. 사용자 문서가 퇴역 사실, 남은 자료 위치, 직접 지울 때 확인할 것을 설명한다. 영상 분석이나 분석 기록 표면을 현재 기능으로 안내하는 문서가 남지 않는다(역사 기록과 퇴역 안내는 제외). 027·034의 `Status:` 줄은 그대로이고 퇴역 안내가 있다.

위 조건은 044의 단독 종료 기준이다. 테스트 green이나 040의 조사 기록만으로 완료 처리하지 않는다.

## 실행 PRD와 독립 종료

각 PRD는 앞 번호의 닫힌 결과와 040의 판정만 소비하며, 구현 전 상태는 모두 `Planned`다.

| PRD | 유일하게 소유하는 결과 | 앞선 PRD 소비 | 자체 완료 표면 |
|---|---|---|---|
| [000 분석 기록 표면 고정](prds/000-record-surfaces-fixed.md) | production publication 해제, 기록을 읽던 여섯 표면의 고정 결과, 그 표면의 사용자 문서 | 없음 | fixture에서의 표면 결과와 파일 비교 |
| [010 영상 안내와 analyzer 코드·034 runner 정리](prds/010-video-note-code-removal-and-spec034-runner.md) | 영상 note, analyzer·분석 기록·replay 코드와 공개 API·테스트 삭제, 034 runner 정리, 영상 안내의 사용자 문서 | 000 | 실제 영상 업로드, 삭제 범위 검색, 034 runner 테스트 |
| [020 Spec031 runner·공통 문서·최종 확인](prds/020-spec031-runner-docs-and-final-qa.md) | Spec031 runner 정리, 공통 문서, 044 이전 binary 자료로의 최종 확인 | 000, 010 | Spec031 runner 테스트, 문서 검색, 실제 표면 QA |

범위 1과 수용 조건 2는 000, 범위 2·3과 수용 조건 1·3·4는 010, 범위 4·5와 수용 조건 5·6은 020이 소유한다. 010은 000이 표면에서 기록 읽기를 거둔 뒤에 기록 형식 코드를 지우므로 번호 순서대로 구현한다. 020이 앞 PRD의 결과를 끝까지 이어 확인하는 것은 소비이지 공동 소유가 아니다. 044 최종 판정에는 세 PRD의 종료 증거와 020의 최종 실제 표면 기록을 함께 확인하며 별도의 뒤 번호 PRD를 요구하지 않는다.

## 종료 증거 (2026-10-25)

PRD [000](prds/000-record-surfaces-fixed.md), [010](prds/010-video-note-code-removal-and-spec034-runner.md), [020](prds/020-spec031-runner-docs-and-final-qa.md)이 각각 독립 Oracle **PASS**를 받았다(000·020은 1차, 010은 2차). 같은 검토에서 수용 조건 1–6과 반드시 지킬 것 전체도 스펙 차원에서 **PASS**로 판정했다(2차). 이는 현재 self-hosted 지원 범위의 scoped 완료이며 전체 release seal이 아니다.

- 조건 1: 새 data dir에서 실제 MP4·MOV, EBML 헤더만 있는 WebM, 깨진 MP4를 API로 보냈다. provider 요청마다 고정 reason의 `unsupported` note가 하나씩 들어갔고 `media/projections/`와 staging 폴더는 생기지 않았다. PNG·WAV note는 044 이전 binary와 같았다(010).
- 조건 2: 044 이전 binary(`95254eb`)로 fixture를 만들었다. 실제 첨부, 044 이전 코드에 가짜 analyzer를 주입해 쓴 기록, 빈 staging 폴더와 파일이 든 staging 폴더가 들어갔고, 기록을 잘라 손상시킨 사본도 만들었다. 두 fixture에서 여섯 표면이 고정 결과를 냈다. `runtime inspect`·`runtime diagnostics --bundle`은 종료 코드 0이었고, 실행마다 fixture 파일 목록과 sha256이 그대로였다. 같은 손상 사본에서 044 이전 binary는 inspect와 진단 묶음이 실패했다(000).
- 조건 3: 지정한 이름의 검색 결과는 코드에 없고 문서의 역사 기록·퇴역 안내뿐이다. fmt, workspace clippy `-D warnings`, 변경한 package의 테스트가 통과했다. 테스트의 유일한 실패는 아래 baseline 실패다(010).
- 조건 4: 034 runner의 새 run은 현재 13개 coverage, 퇴역 9개(`040`), owner audit 5종을 올린 schema version으로 낸다. 통합 테스트가 13개 receipt, 퇴역 목록, `034-AC006`의 남는 출력 검사를 확인한다(010).
- 조건 5: Spec031 runner의 새 run은 `spec031.release_runner.v4`로 `spec034_persistence_operations` fact를 쓴다. v3·v2 결과는 기록된 구성 그대로 보관 사본으로 판독한다. v2 G7 archive 테스트도 통과한다(020).
- 조건 6: README·사용 가이드가 퇴역 사실, 남은 자료 위치, 직접 지울 때 확인할 것을 설명한다. 027·034 SPEC에는 퇴역 안내가 있고 `Status:` 줄은 그대로다(020).

변경한 package의 테스트 결과(`--no-fail-fast`, `.omo/evidence/spec044/final-tests/`):

- `shacs-core`: 1,577 passed / 1 failed / 3 ignored. 유일한 실패는 baseline부터 있던 Spec034 fixture 테스트(`mismatch_uses_isolated_tree`)이며, 수정 전 소스(`95254eb`)에서도 같은 오류로 실패한다.
- `shacs-projection` 344 passed / 7 ignored, `shacs-cli` 470 passed / 3 ignored, `shacs-api` 117, `shacs-tui` 37, `shacs-channels` 33 passed.
- 커밋 `663bb1c`와 같은 코드에서 사용자 승인을 받아 전체 workspace 테스트를 실행했다(`.omo/evidence/spec044/full-workspace/`). suite 206개 기준 3,277 passed / 1 failed / 13 ignored이며, 유일한 실패는 같은 Spec034 fixture 테스트다.

영상 내용 분석과, 남은 영상 첨부·분석 기록·staging 폴더를 정리하거나 내보내는 도구는 제공하지 않는다. 사용자 안내는 [사용 가이드](../../USAGE.md#spec-034-generated-media와-media-diagnostics)를 따른다.
