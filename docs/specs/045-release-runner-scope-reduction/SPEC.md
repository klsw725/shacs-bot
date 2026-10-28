# 045. Spec031 release runner의 범위 축소 구현 명세

Status: Planned

Consumes: 041 release evidence tooling disposition decision and its investigation inputs, 031 release artifact contract and `spec031-release-runner`, shared command execution API used by the 030·033·034 runners, 035 inactive archive, 043·044 archived owner facts

## 목적과 완료 경계

[041](../041-release-evidence-tooling-disposition/SPEC.md)은 `spec031-release-runner`를 **범위 축소**로 판정했다. CLI, v5 생성, v5·v4·v3 판독과 030·033·034 runner가 쓰는 공용 명령 실행 API는 남기고, 퇴역한 035 시기의 v2 판독 장치와 production 소비자가 없는 공개 API는 canonical v2 결과의 보존 기록을 만든 뒤 지운다. 조사에서 찾은 validator fail-open은 그보다 먼저 고친다. 이 스펙은 그 판정의 [남길 계약·지울 것·보존 계약·구현 순서·회귀 기준](../041-release-evidence-tooling-disposition/prds/000-runner-consumers-evidence-and-disposition.md#판정-2026-10-25-범위-축소)을 실제 코드와 문서로 옮긴다. 041의 판정을 다시 내리지 않고, [031](../031-configuration-runtime-layout-and-execution-snapshots/SPEC.md)의 과거 완료 판정도 다시 열지 않는다.

완료되면 runner를 쓰는 사용자는 다음을 경험한다.

- 통과로 기록된 `cargo test` 명령의 테스트 개수가 비어 있거나 transcript에서 다시 센 값과 다르면 validator가 결과를 거절한다. 손으로 고친 결과를 PASS로 받아들이지 않는다.
- runner의 옵션·mode·출력·종료 코드와 v5 결과 형식, v5·v4·v3 결과의 판독은 그대로다. 바뀌는 판독은 위의 개수 검사뿐이다.
- v2 결과는 `UnsupportedSchema`로 거절된다. canonical v2 결과 10개는 추적되는 sha256 기록으로 byte를 확인하고, 기록된 고정 commit의 validator로 당시 판정을 다시 낼 수 있다.

이 스펙은 다른 판정 스펙이나 신규 기능의 구현을 기다리지 않는다.

## 소유 범위

1. **통과 기록의 개수 검사:** 통과로 기록된 `cargo test` 명령은 gate 종류와 schema(v5·v4·v3)에 관계없이 기록된 개수가 있어야 하고, transcript에서 다시 센 값과 같아야 한다. 개수 없는 기록은 실패나 시간 초과로 기록된 명령일 때만 받아들여 지금처럼 명령 실패로 판정한다.
2. **v2 결과 보존 기록:** v2 판독을 지우기 전에 canonical v2 결과 10개의 파일별 sha256 기록, 결과별 당시 schema·행 수·summary 상태와 고정 commit validator의 판정표, 고정 commit hash와 harness 원본·실행 방법을 추적되는 문서로 남긴다.
3. **v2 판독 장치와 공개 API 삭제:** 041의 "지울 것"(v2 판독 장치, 공개 Rust API 세 항목, CLI 오류 출력의 Spec035 분기)을 지우고, v2 manifest를 schema 판정에서 `UnsupportedSchema`로 거절한다.
4. **문서:** 동작이 바뀌는 PRD에서 `README.md`·`docs/USAGE.md`의 runner 절, 031 SPEC의 안내, `docs/archive/spec035/README.md`의 안내, 스펙 색인, `docs/PRODUCT-PATH-MAP.md`, `docs/SYSTEM-FOUNDATION.md`를 고친다.

## 기존 owner와의 경계

| 기존 owner | 045가 소비하는 것 | 045가 소유하지 않는 것 |
|---|---|---|
| [041](../041-release-evidence-tooling-disposition/SPEC.md) | 범위 축소 판정, 남길 계약·지울 것·보존 계약·구현 순서·회귀 기준, 조사 입력(로컬 `.omo/evidence/spec041/prd000/`) | 판정의 재검토, 유지·제거로의 변경. 판정과 별개로 남긴 일(034 테스트 `mismatch_uses_isolated_tree`의 추적하지 않는 파일 읽기, workspace 명령의 `--no-fail-fast` 부재, v5의 commit/tree 결속 부재, 아무도 인용하지 않는 `spec031-owner-spec035` 명령) |
| [031](../031-configuration-runtime-layout-and-execution-snapshots/SPEC.md) | release artifact 계약과 CLI, v5 형식(65행·owner 4개·명령 16개·035 보관 자료 receipt·산출물 목록) | v5 형식·명령 집합의 변경, 031 완료 판정의 재해석, runtime snapshot·replay·config migration |
| 030·033·034 runner | `execute_spec031_release_command`·`execute_spec031_release_command_with`·`parse_cargo_test_counts`와 명령 타입(바꾸지 않는다) | 각 runner의 catalog·archive·schema·validator |
| [035 비활성 보관 자료](../../archive/spec035/README.md) | `docs/archive/spec035/`의 byte와 `SHA256SUMS`, 로컬 `.omo/worktree-archives/spec035-20260928/`의 v2 결과(읽기만) | 보관 자료의 재분류, 당시 146행 결과를 현재 PASS로 재결속하는 일 |
| [043](../043-app-capability-retirement/SPEC.md)·[044](../044-video-analyzer-retirement/SPEC.md) | 앱·영상 보관 사본과 v4·v3 판독 구성 | 그 구성과 보관 사본의 변경 |

## 반드시 지킬 것

- 실패한 Cargo 명령, missing/unknown owner fact, 불완전한 cleanup, stale source, 변조된 artifact를 PASS로 바꾸지 않는다. success-fixture 실행을 semantic closure라고 쓰지 않는다.
- 검증을 통과시키려고 테스트를 지우거나 assertion을 약하게 바꾸지 않는다. 지우는 테스트는 지우는 v2 판독 장치와 공개 API만 고정하던 것이다. `#[ignore]`나 `#[allow(...)]`로 남기지 않는다.
- v2 결과 원본 폴더(`.omo/worktree-archives/spec035-20260928/`)와 그 안의 테스트 임시 출력 49개, 041 조사 자료, 이미 만든 release evidence를 지우거나 고치지 않는다. `docs/archive/spec035/`의 byte·`SHA256SUMS`와 그 README의 3행(v5 coverage의 source locator)은 바꾸지 않는다.
- 어떤 테스트도 추적하지 않는 `.omo` 자료를 읽지 않는다.
- CLI의 옵션 네 개·mode 두 개와 stdout·stderr·exit code(041 조사의 c01–c09)는 그대로다. `Spec031ExternalOwnerId::Spec035`는 v2 manifest를 읽어 `UnsupportedSchema`로 거절하기 위해 남긴다.
- 041의 `Status:` 줄은 바꾸지 않는다. 사용자 문서는 동작을 바꾸는 PRD에서 함께 고친다.
- PRD는 번호 순서대로 구현한다. v2 판독을 지우는 PRD 020은 고정 commit을 기록한 PRD 010이 닫힌 뒤에 한다. 각 PRD는 자기 수용 조건으로 따로 닫는다.

## 수용 조건

검증 입력은 041 조사 입력을 그대로 쓴다. `.omo/evidence/spec041/prd000/`에 기록된 m01–m13·r01–r05·b01–b05 사본, canonical v2 결과 10개와 테스트 임시 출력 49개의 사본, s01–s08 사본이며 위치와 만든 방법은 [041 조사 기록](../041-release-evidence-tooling-disposition/prds/000-runner-consumers-evidence-and-disposition.md#조사-기록-2026-10-25)에 있다. 이 로컬 자료가 없는 checkout에서는 실행 증명으로 보지 않는다.

1. 작업 트리 validator로 판독하면 m07과, m07 위조가 들어 있는 다른 입력(b01–b05와 저장소 사본에 넘긴 m07)이 모두 거절되고 나머지 입력의 판정은 수정 전 소스와 같다. 명령 검사가 owner audit보다 먼저 돌므로 041이 `BlockedAsPass`로 적은 b05는 `NonzeroTestsFailed`가 되고, b05의 transcript와 개수만 통과 상태로 맞춘 입력은 `BlockedAsPass`다. 통과로 기록된 `cargo test`의 개수를 비운 v5·v4·v3 입력이 모두 거절되고, 개수 없이 실패·시간 초과로 기록된 명령은 그대로 `CommandFailed`·`CommandTimedOut`이며, v4·v3 판독 테스트는 통과한다.
2. 보존 기록 검사에서 canonical 원본 10개는 통과하고 s02–s08 사본은 모두 거절된다. 기록한 고정 commit에서 다시 빌드한 validator가 판정표를 그대로 재현하고, 그 commit은 main 이력에 남아 있다.
3. v2 판독을 지운 뒤 canonical 10개, 임시 출력 49개, m08–m13 사본이 모두 `UnsupportedSchema`이며 valid가 되지 않는다. 조건 1의 v5 입력 판정은 지우기 전과 같다. 지운 이름의 검색 결과는 문서의 역사 기록과 보존 안내뿐이다.
4. CLI c01–c09의 출력과 exit code가 041 기록과 같다. 030·033·034 runner 테스트의 결과가 변경 전과 같다. fmt, workspace clippy `-D warnings`, 변경한 package의 테스트가 통과하고, 전체 workspace 테스트는 사용자 승인을 받아 실행한다.
5. 사용자 문서가 v2 결과를 보존 기록과 고정 commit으로 읽는 방법을 설명한다. 고친 fail-open을 현재 한계로 안내하거나 지운 v2 판독을 현재 기능으로 안내하는 문장이 없다(역사 기록 제외).

위 조건은 045의 단독 종료 기준이다. 테스트 green이나 041의 조사 기록만으로 완료 처리하지 않는다.

## 남은 위험과 한계

041이 남긴 미확인 사항을 그대로 이어받는다. 045의 종료 기록은 아래를 확인된 사실로 바꾸어 주장하지 않는다.

- 저장소 밖 script·다른 machine의 runner 사용과 `shacs-projection`을 라이브러리로 쓰는 외부 코드는 미확인이다. 외부 소비자가 없다고 주장하지 않는다. 지운 API를 쓰던 코드는 컴파일 단계에서 실패하고, v2 결과는 PASS가 아닌 `UnsupportedSchema`가 된다.
- 이 machine에는 실제 v3·v4 결과가 없어 그 판독은 테스트 입력으로만 확인한다. PRD 000 뒤에는 "통과 + 개수 없음" 기록이 든 v3–v5 결과가 새로 거절되며, 그런 결과가 저장소 밖에 있는지는 미확인이다. 모든 과거 v3·v4 결과가 그대로 읽힌다고 주장하지 않는다.
- 보존 기록은 byte가 같다는 것만 증명한다. 추적하지 않는 원본 v2 폴더를 잃으면 기록만으로는 결과를 읽을 수 없다.
- v5 결과는 commit/tree를 결속하지 않아 실행 뒤 바뀐 요구사항 문구나 owner status를 알아채지 못한다. 041이 별개로 남긴 일이며 045가 고치지 않는다.

## 실행 PRD와 독립 종료

각 PRD는 앞 번호의 닫힌 결과와 041의 판정만 소비하며, 구현 전 상태는 모두 `Planned`다.

| PRD | 유일하게 소유하는 결과 | 앞선 PRD 소비 | 자체 완료 표면 |
|---|---|---|---|
| [000 통과 기록의 테스트 개수 검사](prds/000-passed-cargo-test-count-check.md) | 개수 검사 수정, 회귀 테스트, fail-open을 한계로 안내하던 사용자 문서 | 없음 | 041 입력의 수정 전후 판독, 회귀 테스트 |
| [010 v2 결과 보존 기록](prds/010-v2-result-preservation-record.md) | 추적되는 보존 문서(sha256 기록·판정표·고정 commit·harness)와 그 검사 | 000 | s01–s08 검사, 고정 commit validator의 판정표 재현 |
| [020 v2 판독 장치·공개 API 삭제와 문서](prds/020-v2-reader-removal-and-docs.md) | 삭제, 새 `UnsupportedSchema` 테스트, CLI 분기 삭제, 공통 문서 | 000, 010 | 삭제 범위 검색, 041 입력 판독, CLI 관측, runner 테스트, 문서 검색 |

범위 1과 수용 조건 1은 000, 범위 2와 수용 조건 2는 010, 범위 3·4와 수용 조건 3·4·5는 020이 소유한다. 각 PRD는 자기 변경에 맞는 사용자 문서와 Cargo 검증을 함께 끝낸다. 010은 000의 수정이 들어간 commit을 고정 commit으로 삼고, 020은 010의 보존 기록이 있어야 v2 판독을 지울 수 있으므로 번호 순서대로 구현한다. 020이 앞 PRD의 결과를 끝까지 이어 확인하는 것은 소비이지 공동 소유가 아니다. 045 최종 판정에는 세 PRD의 종료 증거를 함께 확인하며 별도의 뒤 번호 PRD를 요구하지 않는다.
