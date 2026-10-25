# PRD 020. Spec031 runner·공통 문서·최종 확인

Status: Complete (Scoped)

Depends on: [PRD 000](000-record-surfaces-fixed.md), [PRD 010](010-video-note-code-removal-and-spec034-runner.md) 완료. 뒤 번호 PRD에는 의존하지 않는다.

## Goal

지운 영상 주입 테스트를 부르던 Spec031 release runner를 맞추고, 공통 문서를 퇴역 상태로 고친다. 044 이전 binary가 만든 실제 자료로 전체 흐름을 확인해 044를 닫는다.

## Scope

1. **Spec031 release runner:**
   - 새 run의 034 외부 owner 명령과 fact artifact는 `crates/shacs-core/tests/spec034_persistence_operations.rs#final_edit_and_variation_candidates_publish_one_complete_record_each`를 쓴다. 다른 owner의 fact와 판정은 바꾸지 않는다.
   - 명령 구성이 바뀌므로 새 run의 schema version을 올린다(지금 `spec031.release_runner.v3`).
   - v3와 v2 결과는 기록된 구성(034는 영상 주입 테스트) 그대로 판독하고, v3 판독 회귀 테스트를 더한다. 판독은 기록된 결과만 쓰고 지운 Cargo test를 다시 실행하지 않는다. 추적되는 v2 G7 archive와 그 검증 테스트는 지금처럼 통과해야 한다. 이미 만든 과거 evidence 파일은 고치지 않는다.
2. **공통 문서:**
   - README의 Spec 034 media 문단과 Spec031 runner 설명, `docs/USAGE.md`의 runner 외부 owner 설명.
   - 027·034 SPEC에 044 퇴역 안내를 더한다. `Status:` 줄은 바꾸지 않는다.
   - `docs/PRODUCT-PATH-MAP.md`와 스펙 색인의 040·044 설명.
3. **최종 확인:** 044 이전 binary(`95254eb`)로 바로가기가 없는 data dir에 실제 자료를 만든다. 영상 첨부 여러 형식과 이미지·오디오 첨부, 044 이전 코드에 가짜 analyzer를 주입해 쓴 분석 기록, 손상된 기록 사본, staging 폴더를 포함한다. 새 binary로 실제 영상 업로드, `runtime inspect`, `runtime diagnostics --bundle`, `serve`의 `/v1/media/diagnostics`·WebSocket `media_projection`, TUI `--once`, WhatsApp bridge 가짜 서버로 채널 답장 한 번을 실행하고 각 실행 전후 fixture 파일 목록과 sha256을 비교한다.

## Non Scope

- [041](../../041-release-evidence-tooling-disposition/SPEC.md)의 runner 유지·축소·제거 판정, 다른 외부 owner fact의 변경.
- 남은 자료의 정리·내보내기 도구, 027·034의 판정 변경.

## Parent Requirement Mapping

- [044 SPEC](../SPEC.md)의 소유 범위 4·5와 수용 조건 5·6을 소유한다.

## Required Contract

1. runner를 고쳐도 지운 테스트 때문에 blocked가 생기지 않고 다른 owner의 판정과 coverage 근거가 바뀌지 않는다.
2. 최종 확인은 앞 PRD의 테스트를 대신하지 않는다. 044 이전 binary의 실제 자료로 사용자 표면이 함께 성립하는지 본다.

## Acceptance Criteria

1. Spec031 runner의 새 run이 올린 schema version으로 새 034 owner 명령을 쓰고, v3 판독 회귀 테스트와 v2 G7 archive 검증 테스트가 지금과 같은 결과로 통과한다. 변경한 package의 Cargo 검증이 통과한다. 전체 workspace 테스트는 사용자 승인을 받아 실행한다.
2. 최종 확인 기록에서 [044 SPEC](../SPEC.md) 수용 조건 1·2가 함께 성립한다.
3. 문서 검색에서 영상 분석이나 분석 기록 표면을 현재 기능으로 안내하는 문장이 없다(역사 기록과 퇴역 안내 제외). 027·034의 `Status:` 줄은 그대로이고 퇴역 안내가 있다.

## Closure Evidence

1. Spec031 runner의 명령·schema·테스트 변경과 결과.
2. 044 이전 binary 자료로 한 최종 실제 표면 QA 기록(fixture 비교 포함).
3. 고친 문서.

PRD 020은 앞 PRD의 닫힌 결과를 소비해 044를 닫는다. 041의 판정이나 다른 신규 기능을 기다리지 않는다.

## 구현 메모 및 검증 근거 (2026-10-25)

- Spec031 release runner(`crates/shacs-projection/src/spec031/release_runner/`):
  - 새 run은 `spec031.release_runner.v4`다. v3와 v4는 같은 결과 배치(`is_current_layout`)이고, v2와 v3는 034 fact로 지워진 영상 주입 테스트를 쓴다(`uses_retired_spec034_fact`).
  - 034 owner의 descriptor와 명령은 schema에 따라 고른다(`external_owner_facts_for`, `exact_owner_commands`, `required_worktree_commands_for`). 새 run은 `spec034_persistence_operations`의 `final_edit_and_variation_candidates_publish_one_complete_record_each`를 쓴다. 다른 owner의 fact·명령·판정은 그대로다.
  - fact 존재 확인은 결과 작성(`audit.rs`)과 판독(`coverage_audit_validate.rs`)이 같은 `owner_fact_exists`를 쓴다. v2·v3의 지운 테스트는 다시 실행하지 않는다. [보관 사본](../../../archive/spec034-video-analyzer/README.md)의 sha256(코드에 고정)과 테스트 이름으로만 존재를 확인하고, 사본이 없거나 바뀌면 blocked다.
- 테스트(`shacs-projection`):
  - `reads_v3_runs_with_the_retired_video_owner_fact_from_the_archive`: v3 current-worktree run이 끝까지 판독되고, 사본을 바꾸면 `BlockedAsPass`가 된다.
    - 이를 위해 기존 helper `valid_non_fixture_artifacts_on_disk`를 고쳤다. 자신이 쓴 cleanup receipt를 등록하고, owner 원문을 fact 파일 뒤에 쓰며, Spec031 원문 둘을 복사한다. 그래서 끝까지 판독되는 fixture가 됐다.
  - 기존 v2 G7 archive 테스트(`spec035_postrun_test`, `spec035_postrun_process` 등)는 결과 작성이 run의 schema descriptor를 쓰게 한 뒤 그대로 통과한다.
  - 새 034 owner 명령을 runner와 같은 인자로 따로 실행해 통과를 확인했다(`.omo/evidence/spec044/final-tests/spec031-new-spec034-owner-command.log`).
- 공통 문서:
  - README의 Spec 034 media 문단과 Spec031 runner 설명.
  - `docs/USAGE.md`: runner 외부 owner 설명, endpoint 목록의 `/v1/media/diagnostics`, media 절 제목·예시, 직접 지울 때 확인할 것.
  - 027·034 SPEC의 퇴역 안내. `Status:` 줄은 그대로다.
  - `docs/PRODUCT-PATH-MAP.md`, 스펙 색인의 034·040·044 설명, 040 PRD의 USAGE 절 링크.
- 최종 확인(`.omo/evidence/spec044/final-qa/transcript.md`):
  - 044 이전 binary(`95254eb`)로 fixture를 만들었다. 영상 첨부 네 형식과 PNG·WAV를 API로 올렸다. 044 이전 코드에 가짜 analyzer를 주입한 harness로 h07 형식 기록을 썼다. 분석 중 강제 종료해 빈 staging 폴더와 파일이 든 staging 폴더를 남겼다. 손상된 기록 사본도 만들었다.
  - 새 binary로 실제 업로드, `runtime inspect`, `runtime diagnostics --bundle`, `serve`의 `/v1/media/diagnostics`·WebSocket `media_projection`, TUI `--once`, WhatsApp bridge 가짜 서버로 채널 답장 한 번을 실행했다. 실행마다 fixture 파일 목록과 sha256을 비교했다(결과는 PRD 000·010 구현 메모).
- 문서 검색: README·USAGE에서 영상 분석이나 분석 기록 표면을 현재 기능으로 안내하는 문장은 없다. 남은 언급은 027·034의 역사 기록(퇴역 안내 포함), 040·044, 보관 사본이다.

## 종료 확인 (2026-10-25)

- 범위: Spec031 runner v4와 v3·v2 판독, 공통 문서, 044 이전 binary 자료로 한 최종 실제 표면 확인. 041의 runner 판정, 다른 외부 owner fact, 남은 자료의 정리·내보내기 도구는 포함하지 않는다.
- 최종 검증(변경한 package, `--no-fail-fast`, 기록은 `.omo/evidence/spec044/final-tests/`):
  - `shacs-projection` 344 passed / 7 ignored. 첫 실행은 문서를 고치는 동안 돌아 source-bound runner 테스트가 `SourceMismatch`로 실패했다. 고치기를 멈춘 뒤 다시 실행해 통과했다(`shacs-projection.rerun.log`). 그 사이 `crates/` 파일은 바뀌지 않았다.
  - 다른 package 결과와 `shacs-core`의 baseline 실패 하나는 PRD 000·010 종료 확인과 같다.
  - `cargo fmt --all -- --check`와 workspace clippy `-D warnings`도 통과했다.
  - 전체 workspace 실행은 Spec 044 마무리 때 사용자 승인을 받아 했다([SPEC 종료 증거](../SPEC.md#종료-증거-2026-10-25)).
- 실제 표면: `.omo/evidence/spec044/final-qa/transcript.md`의 F·N·U 단계.
- Oracle 독립 검증: **PASS** (1차).
- 남은 제한: v2·v3 결과 판독은 보관 사본이 있어야 한다. 사본이 없거나 바뀌면 해당 034 owner는 blocked가 된다.
