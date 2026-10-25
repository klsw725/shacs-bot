# PRD 000. 분석 기록 표면 고정

Status: Complete (Scoped)

Depends on: 없음. 뒤 번호 PRD에 의존하지 않는다.

## Goal

분석 기록(`<data>/media/projections/current.json`)을 쓰고 읽던 production 경로를 사용자 표면에서 끊는다. 기록을 읽던 여섯 표면은 기록이 없는 data dir의 결과로 고정한다. 영상 note와 코드 삭제는 [PRD 010](010-video-note-code-removal-and-spec034-runner.md)이 한다.

## Scope

1. **production publication 해제:** `AgentLoopChatCompletionAdapter::context_builder`는 `with_video_projection_publication`을 붙이지 않는다.
2. **기록을 읽던 표면:** 모두 `media/projections/` 아래를 읽지 않는다.
   - `runtime inspect`: `Spec035 media projections: unavailable (canonical records unavailable)` 한 줄.
   - `runtime diagnostics`(bundle 포함): 진단 JSON `spec035_media_projections`는 빈 배열.
   - `GET /v1/media/diagnostics`: 404 `media projection is unavailable`.
   - WebSocket `{"type":"media_projection"}`: 같은 not found 오류.
   - TUI: session 행마다 `media: state=unavailable reason=unavailable freshness=unavailable`, `media lineage: unavailable`, `media disclosure: unavailable`.
   - 외부 채널 전송: `media_capability`·`media_delivery_status` metadata를 붙이지 않는다.
3. **표면에 남는 코드:** 위 고정 결과만 만들고, 기록을 표시·변환하던 CLI·TUI·채널 코드 중 쓰이지 않게 된 것은 지운다.
4. **사용자 문서:** `docs/USAGE.md`의 Spec 034 media 절에서 이 표면들의 설명을 고정 결과로 고치고, 남은 기록 파일은 읽지 않으며 직접 지워도 된다고 적는다.

## Non Scope

- 영상 note 변경, analyzer·분석 기록 형식·replay 코드와 공개 API 삭제, release runner(PRD 010·020).

## Parent Requirement Mapping

- [044 SPEC](../SPEC.md)의 소유 범위 1과 수용 조건 2를 소유한다.

## Required Contract

1. 고정 결과는 남은 기록의 유무·형식과 무관하다. 기록을 읽지 않으므로 손상된 기록 때문에 명령·응답·전송이 실패하지 않는다.
2. 남은 자료를 만들거나 바꾸지 않는다.
3. 진단 JSON 키, API 경로와 응답 코드, WebSocket 오류 종류는 지금 기록이 없을 때와 같다.

## Acceptance Criteria

1. 정상·손상 분석 기록과 staging 폴더가 남은 fixture에서 위 여섯 표면이 정한 결과를 내고 `runtime inspect`·`runtime diagnostics --bundle`의 종료 코드가 0이다. fixture 파일의 목록과 sha256이 그대로다.
2. 지금 동작(기록 읽기와 표시)을 고정하던 테스트를 지우거나 고친다. 새 테스트는 위 조건만 확인한다.
3. 변경한 package의 Cargo 검증이 통과한다.

## Closure Evidence

1. fixture에서의 여섯 표면 결과와 파일 비교.
2. 변경한 package의 Cargo 검증 기록과 고친 문서.

PRD 000은 사용자 표면의 동작을 바꾸는 단계이며 뒤 번호 PRD의 완료를 기다리지 않는다.

## 구현 메모 및 검증 근거 (2026-10-25)

- 코드:
  - production publication 해제: `crates/shacs-cli/src/spec035_cli_media/context_builder.rs`의 ContextBuilder는 분석 기록 publication을 붙이지 않는다. 그 API 자체는 PRD 010에서 지웠다.
  - `runtime inspect`·`runtime diagnostics`: `runtime_inspect.rs`는 기록을 읽지 않고 `runtime_format.rs`가 고정 줄 하나를 낸다. 진단 JSON의 `spec035_media_projections`는 빈 배열이다(`crates/shacs-cli/src/lib.rs`).
  - API·WebSocket: `crates/shacs-api/src/media_api/request.rs`의 `GET /v1/media/diagnostics`는 항상 404 `media projection is unavailable`이다. `websocket.rs`는 `{"type":"media_projection"}` frame을 같은 not found 오류로 거절한다. adapter의 기록 조회 메서드는 지웠다.
  - TUI: `media_view.rs`는 unavailable 세 줄만 만들고, `live_source.rs`는 기록을 읽지 않는다.
  - 외부 채널: `channel_adapter.rs`의 전송은 메시지를 그대로 넘긴다. `shacs-channels`의 기록 변환 코드와 재노출은 지웠다.
- 테스트: 기록을 읽고 표시하던 테스트·예제를 지웠다. CLI의 기존 기록 표시 테스트, `shacs-api/tests/it/spec034_media_api/`와 API 예제 둘, `shacs-channels/tests/it/spec035_media_projection*`와 채널 예제 둘, TUI 예제가 여기에 든다. 새 테스트는 고정 결과만 확인한다.
  - CLI `runtime_inspect_and_diagnostics_ignore_leftover_media_records`: 정상·손상 기록과 staging 폴더가 남은 data dir에서 inspect·diagnostics가 고정 결과를 내고 남은 파일의 sha256이 그대로다.
  - CLI `external_channel_send_ignores_leftover_media_records`: 같은 자료에서 채널 전송이 media metadata 없이 전달된다.
  - API `media_diagnostics_stay_unavailable_with_a_leftover_record`, TUI `media_lines_stay_unavailable_with_leftover_records`.
- 문서: `docs/USAGE.md`의 Spec 034 media 절에 고정 결과와, 남은 기록을 읽지 않으므로 직접 지워도 된다는 안내를 적었다.
- 실제 표면(`.omo/evidence/spec044/final-qa/transcript.md` N1–N5): 044 이전 binary로 만든 fixture A(정상 기록)와 그 기록을 잘라 손상시킨 사본 B에서 여섯 표면이 위 고정 결과를 냈다. `runtime inspect`·`runtime diagnostics --bundle`은 종료 코드 0이고 bundle의 `spec035_media_projections`는 `[]`였다. 채널 답장은 `text,to,type`만 가진 frame으로 전달됐다. 각 실행 뒤 fixture 파일 목록과 sha256이 그대로였다. 같은 사본 B에서 044 이전 binary는 inspect와 진단 묶음이 종료 코드 1로 실패했다.
  - 표면 QA는 세 PRD가 모두 들어간 최종 작업 트리의 binary로 한 번 했다. fixture A의 첫 N3·N5 시도는 앞 단계에서 멈추지 않은 044 이전 `serve`에 닿아 무효 처리하고 다시 했다(기록에 남김).

## 종료 확인 (2026-10-25)

- 범위: production 분석 기록 publication 해제, 기록을 읽던 여섯 표면의 고정 결과, 그 표면의 사용자 문서. 영상 note와 코드 삭제(PRD 010), Spec031 runner와 공통 문서(PRD 020)는 포함하지 않는다.
- 최종 검증(변경한 package, `--no-fail-fast`, 기록은 `.omo/evidence/spec044/final-tests/`):
  - `shacs-cli` 470 passed / 3 ignored, `shacs-api` 117, `shacs-tui` 37, `shacs-channels` 33 passed.
  - `shacs-core` 1,577 passed / 1 failed / 3 ignored. 유일한 실패는 baseline 실패다(`runtime::generated_media_release::runner::fixture::tests::mismatch_uses_isolated_tree`, git이 추적하지 않는 Spec034 로컬 evidence 파일 누락). 수정 전 소스(`95254eb`)에서도 같은 오류로 실패한다.
  - `cargo fmt --all -- --check`와 workspace clippy `-D warnings`도 통과했다.
  - 전체 workspace 실행은 Spec 044 마무리 때 사용자 승인을 받아 했다([SPEC 종료 증거](../SPEC.md#종료-증거-2026-10-25)).
- 실제 표면: 위 구현 메모의 N1–N5.
- Oracle 독립 검증: **PASS** (1차).
- 남은 제한: 남은 기록과 staging 폴더는 정리하지 않는다. 지우기는 사용자가 직접 한다.
