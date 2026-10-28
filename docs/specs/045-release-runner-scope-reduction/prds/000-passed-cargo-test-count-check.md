# PRD 000. 통과 기록의 테스트 개수 검사

Status: Complete (Scoped)

Depends on: 없음. 뒤 번호 PRD에 의존하지 않는다.

## Goal

통과로 기록된 `cargo test --workspace` 명령의 테스트 개수가 비어 있으면 transcript를 보지 않고 받아들이던 validator의 fail-open([041 조사](../../041-release-evidence-tooling-disposition/prds/000-runner-consumers-evidence-and-disposition.md#조사-기록-2026-10-25)의 m07)을 고친다. v2 판독 장치는 건드리지 않는다.

## Scope

1. **개수 검사:** Spec031 validator는 통과로 기록된 `cargo test` 명령마다 transcript의 `test result:` 줄을 다시 세고, 기록된 개수가 그 값과 같을 때만 받아들인다. gate 종류(`FullCargoGate` 포함)와 schema(v5·v4·v3)에 관계없다. 개수 없는 기록은 실패나 시간 초과로 기록된 명령일 때만 받아들이며, 그 기록은 지금처럼 `CommandFailed`·`CommandTimedOut`으로 거절된다.
2. **회귀 테스트:** `crates/shacs-projection/tests/it/spec031_release_runner_counts.rs`에 둔다. 기존 v4·v3 판독 테스트와 같은 fixture helper로 v5 current-worktree fixture와 그것을 v4·v3로 바꾼 fixture를 만들고, 세 schema마다 `spec031-test-workspace` 기록만 바꾼 입력 네 개를 판독한다. 다른 mirror와 hash는 맞춰 다시 쓴다.
   - 통과 기록의 개수만 비운 입력(041 판정 3단계가 정한 별도 입력)과, 거기에 transcript까지 실패 결과로 바꾼 입력(041 m07과 같은 위조)은 거절된다.
   - 개수 없이 실패·시간 초과로 기록한 입력은 지금처럼 `CommandFailed`·`CommandTimedOut`이다.
   - fixture helper(`valid_non_fixture_artifacts_on_disk`, `convert_to_legacy_run`, `convert_to_v3_retired_spec034_run`과 그 함수들이 쓰는 함수)는 `tests/spec031_release_runner_exploits.rs`에서 `tests/it/spec031_release_runner_fixture.rs`로 옮겨 두 test binary가 함께 쓴다. exploits 파일은 runner의 `spec031-test-release-runner` 명령이 이름으로 실행하므로 그 자리에 남고(AGENTS.md), 옮긴 helper를 `#[path]`로 불러온다. 옮긴 함수는 visibility와, 플랫폼과 무관한 `temp_path`·`temp_base`·`workspace_root`에 붙어 있던 `#[cfg(unix)]` 말고는 바꾸지 않으며 새 test binary는 생기지 않는다.
3. **사용자 문서:** `docs/USAGE.md` Spec 031 release evidence 절에서 fail-open을 현재 한계로 안내하던 문장을 고친 동작으로 바꾼다.

## Non Scope

- v2 보존 기록([PRD 010](010-v2-result-preservation-record.md)), v2 판독 장치·공개 API·CLI 분기 삭제와 공통 문서([PRD 020](020-v2-reader-removal-and-docs.md)).
- runner가 개수를 기록하는 방식(`command.rs`의 `Option`), `parse_cargo_test_counts`, 030·033·034 runner와 그 validator.
- 041이 판정과 별개로 남긴 일.

## Parent Requirement Mapping

- [045 SPEC](../SPEC.md)의 소유 범위 1과 수용 조건 1을 소유한다.

## Required Contract

1. 통과로 기록된 `cargo test` 명령이 받아들여지려면 기록된 개수가 transcript에서 다시 센 값과 같아야 한다. 개수가 없거나 transcript에서 셀 수 없으면 거절한다.
2. 실패·시간 초과로 기록된 명령, 개수가 있는 기록, `cargo test`가 아닌 명령과 success-fixture의 판정은 바뀌지 않는다.
3. runner가 새로 쓰는 결과(v5)의 형식과 CLI 출력은 바뀌지 않는다.

## Acceptance Criteria

1. 회귀 테스트에서 v5·v4·v3 fixture는 위조 전 valid다. 개수만 비운 입력은 `ArtifactMismatch`, transcript까지 바꾼 입력은 `NonzeroTestsFailed`로 거절되고, 개수 없는 실패·시간 초과 기록은 `CommandFailed`·`CommandTimedOut`이다. 수정 전 소스에서는 앞의 두 입력이 valid이고 뒤의 두 입력은 같은 오류다.
2. 041 입력 98쌍(repo root와 입력의 짝)을 수정 전 소스(`4b449f9`)와 작업 트리에서 각각 빌드한 validator로 판독한다. 수정 전 판정은 041 기록과 같다. 판정이 바뀌는 것은 m07 위조가 들어 있는 입력뿐이며 모두 거절된다.
3. 명령 검사가 owner audit보다 먼저 돌므로 b05는 `BlockedAsPass`에서 `NonzeroTestsFailed`로 바뀐다(041 회귀 기준 1의 `BlockedAsPass`는 수정 전 판정이다). b05의 transcript와 개수만 통과 상태로 맞춘 입력은 두 validator 모두에서 `BlockedAsPass`이고, 같은 방식으로 맞춘 b01(owner 테스트가 그대로인 사본)은 valid다.
4. CLI c01–c09의 출력과 exit code가 041 기록과 같다.
5. 변경한 package의 Cargo 검증이 통과한다.

## Closure Evidence

1. 수정, 회귀 테스트와 수정 전 실패 기록.
2. 041 입력의 수정 전후 판정표와 CLI 관측.
3. Cargo 검증 기록과 고친 문서.

PRD 000은 v2 판독 장치와 무관하게 닫힌다. 뒤 번호 PRD의 완료를 기다리지 않는다.

## 구현 메모 및 검증 근거 (2026-10-28)

실행 기록은 로컬 `.omo/evidence/spec045/prd000/`(추적하지 않음)에 있고 `transcript.md`에 정리했다.

- 코드: `crates/shacs-projection/src/spec031/release_runner/command_validate.rs`의 `validate_cargo_test_evidence`는 `FullCargoGate` 기록에 개수가 없을 때 실패·시간 초과 기록만 transcript를 보지 않고 넘긴다. 통과 기록은 다른 gate처럼 transcript를 다시 세어 기록된 개수와 비교한다. 넘긴 실패·시간 초과 기록은 호출하는 `validate_command_record`가 상태로 거절한다.
- 테스트:
  - `tests/it/spec031_release_runner_counts.rs`의 `rejects_passed_workspace_tests_without_counts_in_v5_v4_and_v3_runs`는 세 schema마다 다섯 입력(위조 전, 개수만 비움, transcript까지 실패, 개수 없는 실패, 개수 없는 시간 초과)의 판정표를 한 번에 비교한다.
  - fixture helper 함수 24개와 상수 4개를 `tests/spec031_release_runner_exploits.rs`에서 `tests/it/spec031_release_runner_fixture.rs`로 옮겼다. exploits 파일은 이를 `#[path]`로 불러와 `fixture::`로 부르며 테스트 20개는 그대로다. 옮긴 항목은 `4b449f9`와 본문이 같고 visibility와 세 helper의 `#[cfg(unix)]`만 다르다.
- 수정 전 실패: `command_validate.rs`만 `4b449f9`로 되돌리고 회귀 테스트를 돌렸다. 세 schema 모두 개수만 비운 입력과 transcript까지 실패인 입력이 valid였고, 개수 없는 실패·시간 초과는 지금과 같은 오류였다(`regression-before-fix.log`).
- 041 입력 98쌍(`judge-98.tsv`): 041 harness를 `git archive 4b449f9`와 작업 트리에 각각 연결해 빌드했다. 수정 전 validator는 98쌍 모두 041 기록과 같은 판정을 냈다. 수정 뒤 바뀐 9쌍은 모두 m07 위조가 든 입력이다.
  - m07은 이 저장소에서 valid, 저장소 사본 네 개에서 `InvalidCommandEvidence`(명령 cwd 불일치)였다가 모두 `NonzeroTestsFailed`가 됐다. 명령 loop 안에서 개수 검사가 cwd 대조보다 먼저 돈다.
  - b01·b03·b04는 valid에서, b05는 `BlockedAsPass`에서 `NonzeroTestsFailed`가 됐다.
  - 035 보관 자료를 바꾼 사본에 넘긴 m07과 b02는 그 검사가 먼저 돌아 `ArtifactMismatch` 그대로다.
  - 041의 임시 사본은 macOS 임시 폴더 정리로 일부 파일이 지워져 있었다(G7 사본은 `manifest.json`만 남음). 그 사본으로 한 첫 판독은 무효 처리했고, 보존된 원본(`.omo/evidence/spec041/prd000/mutated/`, 035 보관 폴더)에서 78개 입력을 041과 같은 방식으로 다시 옮겨 판독했다.
- b05의 transcript와 개수를 통과 상태로 맞춘 사본은 두 validator 모두 `BlockedAsPass`였고, 같은 방식으로 맞춘 b01은 valid였다(`judge-counted.tsv`). 개수가 맞는 입력에서는 owner fact 검사가 그대로 거절한다.
- CLI: 수정 뒤 binary로 c01–c09를 다시 실행했다. stdout·stderr·exit code가 041 기록과 같았고 c08의 기존 파일은 그대로였다(`cli/`). c10(current-worktree)은 `cargo test --workspace`를 돌리므로 승인 없이 실행하지 않았다.
- 문서: `docs/USAGE.md` Spec 031 release evidence 절의 fail-open 한계 문장을 고친 동작과 045 안내로 바꿨다.

## 종료 확인 (2026-10-28)

- 범위: 통과 기록의 개수 검사, 회귀 테스트와 fixture helper 이동, fail-open을 한계로 안내하던 문서. v2 보존 기록(PRD 010)과 v2 판독 삭제(PRD 020)는 포함하지 않는다.
- 최종 검증: Google 옵션을 붙인 `cargo fmt --all -- --check`, workspace clippy `-D warnings`가 통과했다. `shacs-projection`은 `--no-fail-fast`로 346 passed / 7 ignored이고 실행 중 소스가 바뀌지 않았다.
- 전체 workspace 테스트: 사용자 승인을 받아 커밋 전 작업 트리에서 `--no-fail-fast`로 실행했다(`.omo/evidence/spec045/full-workspace-prd000/`). suite 196개 기준 3,352 passed / 1 failed / 14 ignored이며, 유일한 실패는 baseline인 Spec034 fixture 테스트(`runtime::generated_media_release::runner::fixture::tests::mismatch_uses_isolated_tree`, 추적하지 않는 Spec034 evidence 파일 누락)다. 실행 중 작업 트리는 바뀌지 않았다.
- 검증한 소스: `4b449f9` 위의 작업 트리. 같은 코드를 이 PRD의 커밋으로 남긴다.
- Oracle 독립 검증: **PASS** (구현 2차).
  - 계획 1차는 BLOCKED였다. 회귀 테스트를 `tests/it/` 대신 exploits 파일에 두었고, b05의 `BlockedAsPass`를 설명 없이 "거절"로 낮췄으며, 개수 없는 실패·시간 초과 기록의 판정을 검사하지 않았다는 지적이었다(PRD 020 검색 범위·보관 README와 041 미확인 사항의 승계도 함께 지적). fixture helper를 옮겨 테스트를 `tests/it/`에 두고, b05의 바뀐 판정과 개수를 맞춘 b05의 `BlockedAsPass`를 수용 조건과 기록에 넣고, 두 경우를 회귀 테스트에 더했다. 계획 2차는 PASS였다.
  - 구현 1차는 BLOCKED였다. 새 테스트가 다른 모듈의 자유 함수를 직접 가져와 불렀다. 모듈 경로로 부르게 고치고 Cargo 검증과 수정 전후 기록을 다시 만들었다.
- 남은 제한:
  - 이 machine에는 실제 v3·v4 결과가 없어 그 판독은 테스트 입력으로만 확인했다. 수정 뒤 "통과 + 개수 없음" 기록이 든 v3–v5 결과는 새로 거절되며, 그런 결과가 저장소 밖에 있는지는 미확인이다.
  - 개수 위조와 다른 변조가 함께 든 입력은 명령 검사가 먼저 돌아 `NonzeroTestsFailed`로 보고된다. 041 회귀 기준 1의 b05 `BlockedAsPass`는 수정 전 판정이다.
