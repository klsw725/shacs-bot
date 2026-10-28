# PRD 020. v2 판독 장치·공개 API 삭제와 문서

Status: Complete (Scoped)

Depends on: [PRD 000](000-passed-cargo-test-count-check.md), [PRD 010](010-v2-result-preservation-record.md) 완료. 뒤 번호 PRD에는 의존하지 않는다.

## Goal

새로 만들어지지 않는 v2 결과의 판독 장치와 소비자가 없는 공개 API를 지워 runner를 041이 정한 범위로 줄이고, 문서를 그 상태에 맞춘다.

## Scope

1. **삭제([041의 지울 것](../../041-release-evidence-tooling-disposition/prds/000-runner-consumers-evidence-and-disposition.md#판정-2026-10-25-범위-축소)):**
   - v2 판독 장치: `validate.rs`의 G7 pending·postrun seal 분기, `spec035_*` 모듈 전부(그 테스트 포함), `audit.rs`·`coverage_*.rs`·`external_audit_facts.rs`·`writer.rs`·`archive_integrity.rs`·`model.rs`의 v2·G7·035 legacy owner 분기와 `.omo/evidence/spec035/prd000-009/manifest.json` 참조. v2 manifest는 schema 판정에서 `UnsupportedSchema`가 된다.
   - 공개 Rust API: `inspect_spec035_receipt_admission`, `Spec035ReceiptAdmissionReport`, `Spec031ReleaseArtifactError::Spec035Evidence`. deprecation 기간 없이 지운다.
   - CLI 오류 출력의 Spec035 분기. v5에서는 닿지 않으므로 출력이 바뀌지 않는다.
2. **남기는 것(041의 남길 계약):** CLI, v5 생성(schema 이름·65행·owner 4개·명령 16개·035 보관 자료 receipt·산출물 목록), v5·v4·v3 판독과 그 보관 사본(043 앱, 044 영상, `docs/archive/spec035/current/`), 공용 명령 실행 API, `Spec031ExternalOwnerId::Spec035`, 030·033·034 runner와 그 테스트.
3. **테스트:**
   - G7 v2 판독을 확인하던 legacy 모듈의 테스트(`spec035_postrun_test.rs`의 `persisted_g7_v2_artifact_keeps_old_locators_and_validates_archived_bytes` 등)는 모듈과 함께 지운다.
   - G7 binding이 있는 v2 입력이 `UnsupportedSchema`가 되는 테스트를 `tests/it/`에 새로 둔다.
   - runner 통합 테스트(`tests/spec031_release_runner.rs`, `tests/spec031_release_runner_exploits.rs`, `tests/it/spec031_release_runner_*`), v2·035 전용이 아닌 runner 단위 테스트, 그 밖의 `tests/it/spec031_*`는 지우거나 약하게 만들지 않는다. 특히 `rejects_v2_artifact_when_legacy_binding_is_absent`, `rejects_v3_artifact_when_legacy_binding_is_present`, `persisted_v2_owner_enum_still_reads_spec035`, v4·v3 판독 테스트는 그대로 통과해야 한다.
4. **문서:**
   - `README.md`·`docs/USAGE.md`의 runner 절. v2 결과는 PRD 010의 보존 기록과 고정 commit으로 읽는다고 적는다.
   - [031 SPEC](../../031-configuration-runtime-layout-and-execution-snapshots/SPEC.md)의 퇴역 안내. 031의 `Status:` 줄은 바꾸지 않는다.
   - `docs/archive/spec035/README.md`. 보관된 `CLOSURE.md` 두 개의 `spec035_catalog.rs`·`spec035_execution.rs` 링크가 이 삭제로 끊기므로 고정 commit에서 열라는 안내를 더한다. 이 README의 3행과 보관 파일의 byte·`SHA256SUMS`는 바꾸지 않는다.
   - `docs/archive/spec043-app-environment/README.md`와 `docs/archive/spec034-video-analyzer/README.md`. 지금은 v2 결과도 현재 runner가 판독한다고 적혀 있으므로 v2는 보존 기록으로 읽는다고 고친다. 보관된 test source와 `SHA256SUMS`는 바꾸지 않는다.
   - 스펙 색인의 041·045 설명, `docs/PRODUCT-PATH-MAP.md`, `docs/SYSTEM-FOUNDATION.md`.

## Non Scope

- 041이 판정과 별개로 남긴 일. v5 형식이나 명령 집합을 바꾸는 일은 새 schema가 필요하다.
- 030·033·034 runner, 031 snapshot·replay·config 계약의 변경.

## Parent Requirement Mapping

- [045 SPEC](../SPEC.md)의 소유 범위 3·4와 수용 조건 3·4·5를 소유한다.

## Required Contract

1. 삭제는 남긴다고 정한 항목을 건드리지 않는다. 지운 API를 쓰던 외부 코드는 조용히 다르게 동작하지 않고 컴파일 단계에서 실패한다. 저장소 밖 소비자의 존재는 미확인으로 남긴다.
2. v2 입력은 어떤 경로로도 valid가 되지 않는다.
3. v5·v4·v3 판독 결과는 PRD 000 뒤와 같다.
4. 지우기 직전의 validator는 PRD 010의 고정 commit과 같다. 그 사이 `crates/shacs-projection`이나 그 path 의존 crate가 바뀌었다면 바뀐 상태에서 PRD 010의 판정표를 다시 재현하고 고정 commit을 갱신한 뒤 지운다.

## Acceptance Criteria

1. `crates/shacs-projection/src/spec031/release_runner/`에 `spec035_`로 시작하는 파일과 그 `mod` 선언이 없다. 세 공개 항목과 `.omo/evidence/spec035/prd000-009` 참조는 `crates/` 전체에서 검색 결과가 없다. 다른 owner의 Spec035 모듈(`crates/shacs-projection/src/spec035/`, `tests/it/spec035_*`)과 v5의 035 보관 자료 receipt 검사는 남는다. 지우기 직전 고정 commit과 `crates/shacs-projection`·그 path 의존 crate의 차이가 없다(또는 Required Contract 4대로 다시 고정했다).
2. 041 입력 판독에서 canonical 10개, 임시 출력 49개, m08–m13 사본이 모두 `UnsupportedSchema`이고, [045 SPEC](../SPEC.md) 수용 조건 1의 v5 입력은 PRD 000 뒤와 같은 판정이다.
3. CLI c01–c09의 출력과 exit code가 041 기록과 같다.
4. 030·033·034 runner 테스트의 결과가 변경 전과 같다. fmt, workspace clippy `-D warnings`, 변경한 package의 테스트가 통과하고, 전체 workspace 테스트는 사용자 승인을 받아 실행한다.
5. 문서 검색에서 v2 판독을 현재 기능으로 안내하는 문장이 없고(역사 기록과 보존 안내 제외), v2 결과를 보존 기록과 고정 commit으로 읽는 방법이 있다.

## Closure Evidence

1. 삭제 범위 검색과 Cargo 검증 기록.
2. 041 입력 판독 결과와 CLI 관측.
3. 고친 문서.

PRD 020은 앞 PRD의 닫힌 결과를 소비해 045를 닫는다. 다른 판정 스펙이나 신규 기능을 기다리지 않는다.

## 구현 메모 및 검증 근거 (2026-10-28)

실행 기록은 로컬 `.omo/evidence/spec045/prd020/`(추적하지 않음)에 있다.

- 지우기 전 확인(`pre-delete-check.txt`): HEAD(`e327b72`)와 고정 commit `0f02799` 사이에 `crates/` 변경이 없었다. 지우기 직전의 validator는 고정 commit의 validator와 같다.
- 삭제(`crates/shacs-projection/src/spec031/release_runner/`):
  - `spec035_*` 파일 44개(테스트 포함)와 그 `mod` 선언, `audit.rs`가 부르던 두 legacy 테스트 모듈.
  - `validate.rs`의 G7 pending·postrun seal 분기와 `pending` 단계. 판독은 schema가 v5·v4·v3이고 legacy binding(`spec035-execution-binding.json`)을 나열하지 않을 때만 시작하며, 아니면 `UnsupportedSchema`다. evidence root 검사의 v2용 건너뛰기도 없앴다.
  - `coverage_validate.rs`의 v2 예외·035 원문 경로 대체·`spec035:` 행 처리, `coverage_validate_artifact.rs`의 `spec035:` 행 분기, `coverage_matrix.rs`의 v2 행 생성(그래서 쓰지 않게 된 `repo_root` 인자도 뺐다), `coverage_audit_validate.rs`의 Spec035 owner 요구·G7 예외, `external_audit_facts.rs`의 legacy Spec035 owner fact(`.omo/evidence/spec035/prd000-009/manifest.json` 참조), `archive_integrity.rs`의 G7 digest와 binding 확인, `audit.rs`·`writer.rs`의 legacy binding 처리와 `pending-final-audit`, `model.rs`의 v2 schema 상수와 그 분기.
  - 공개 API `inspect_spec035_receipt_admission`, `Spec035ReceiptAdmissionReport`, `Spec031ReleaseArtifactError::Spec035Evidence`와 그 재노출, CLI의 Spec035 오류 상세 분기.
- 남긴 것: CLI, v5 생성, v5·v4·v3 판독과 앱·영상·035 보관 사본, 공용 명령 실행 API, `Spec031ExternalOwnerId::Spec035`, v5의 035 보관 자료 receipt 검사와 `spec031-owner-spec035` 명령.
- 검색(`search.txt`): `release_runner/`에 `spec035_` 파일과 `mod` 선언이 없고, 세 공개 항목과 `prd000-009` 참조는 `crates/`에 없다. 다른 owner의 Spec035 모듈은 그대로다. `release_runner/`는 하위 폴더를 포함해 4,314줄이 됐다.
- 테스트:
  - 지운 테스트는 v2·035 전용 단위 테스트 100개(93개 실행, 7개 ignored)뿐이다. 남은 runner 단위 테스트는 041 판정대로 13개(035 보관 자료 5, 명령 검증 5, audit 3)다.
  - 새 테스트 `tests/it/spec031_release_runner_schema.rs`의 `rejects_v2_artifact_with_g7_binding`: G7 digest를 담은 binding을 둔 v2 입력이 `UnsupportedSchema`다. 지우기 전 소스에서는 같은 입력이 legacy 판독으로 들어가 `ArtifactMismatch`였다(`g7-test-before-removal.log`).
  - `rejects_v2_artifact_when_legacy_binding_is_absent`, `rejects_v3_artifact_when_legacy_binding_is_present`, `persisted_v2_owner_enum_still_reads_spec035`, v4·v3 판독 테스트, PRD 000 회귀 테스트는 바꾸지 않고 통과했다.
- 041 입력 98쌍(`judge-98.tsv`, 지운 뒤 작업 트리 validator): v2 입력 71쌍(canonical 10개, 임시 출력 49개, m08–m13과 그 저장소 사본 판독)이 모두 `UnsupportedSchema`다. PRD 000 뒤와 달라진 것은 G7 관련 12쌍뿐이며, v5 입력 27쌍은 같은 판정이다. 개수를 맞춘 b05·b01(`judge-counted.tsv`)도 각각 `BlockedAsPass`·valid 그대로다. 보존 기록의 원본 사본 10개는 모두 `UnsupportedSchema`다(`canonical-10-after-removal.txt`).
- CLI(`cli/`): 지운 뒤 binary로 c01–c09를 다시 실행했다. stdout·stderr·exit code가 PRD 000 때(041 기록과 같음)와 같고 c08의 기존 파일은 그대로다.
- 문서: README·`docs/USAGE.md`의 runner 절, 031 SPEC의 퇴역 안내(`Status:` 줄은 그대로), `docs/archive/spec035/README.md`(끝에 고정 commit에서 여는 안내를 더했고 3행은 그대로), 앱·영상 보관 README, [v2 결과 보존 기록](../../../archive/spec031-v2-results/README.md)의 시제, 스펙 색인·`docs/PRODUCT-PATH-MAP.md`·`docs/SYSTEM-FOUNDATION.md`.

## 종료 확인 (2026-10-28)

- 범위: v2 판독 장치(파일 44개)와 소비자 없는 공개 API 세 항목, CLI의 Spec035 분기 삭제, G7 binding이 있는 v2 입력의 `UnsupportedSchema` 테스트, 공통 문서. 041이 판정과 별개로 남긴 일은 포함하지 않는다.
- 판독 결과: 041 입력의 v2 71쌍은 모두 `UnsupportedSchema`이고, 바뀐 것은 G7 관련 12쌍뿐이며 v5 27쌍은 PRD 000 뒤와 같다. CLI c01–c09도 같다.
- 테스트 변화: v2·035 전용 단위 테스트 100개(93개 실행, 7개 ignored)를 지웠고 runner 단위 테스트 13개는 남았다. `release_runner/`는 4,314줄이 됐다.
- 최종 검증: Google 옵션을 붙인 `cargo fmt --all -- --check`, workspace clippy `-D warnings`가 통과했다. `shacs-projection`은 `--no-fail-fast`로 254 passed / 0 ignored이고 실행 중 소스가 바뀌지 않았다.
- 전체 workspace 테스트: 사용자 승인을 받아 커밋 전 작업 트리에서 `--no-fail-fast`로 실행했다(`.omo/evidence/spec045/full-workspace-prd020/`). suite 196개 기준 3,260 passed / 1 failed / 7 ignored이며, 유일한 실패는 baseline인 Spec034 fixture 테스트(`mismatch_uses_isolated_tree`)다. PRD 000 때의 전체 실행과 suite별 결과를 비교하면 달라진 것은 `shacs-projection` lib(122 passed·7 ignored → 29 passed)와 `it`(101 → 102)뿐이고, 030·033·034 runner를 포함한 나머지 194개 suite는 같다(`suite-diff-vs-prd000.txt`). 실행 중 작업 트리는 바뀌지 않았다.
- 검증한 소스: `e327b72` 위의 작업 트리. 같은 코드를 이 PRD의 커밋으로 남긴다.
- Oracle 독립 검증: **PASS** (1차). 같은 검토에서 스펙 종료는 전체 workspace 결과와 상태 줄만 남았다고 판정했다.
- 남은 제한:
  - 지운 API를 쓰던 저장소 밖 코드와 runner를 부르던 외부 script의 존재는 미확인이다. 그런 코드는 컴파일 단계에서 실패하고, v2 결과는 PASS가 아닌 `UnsupportedSchema`가 된다.
  - v2 결과의 당시 판정은 [보존 기록](../../../archive/spec031-v2-results/README.md)의 고정 commit validator로만 재현된다. 원본 폴더를 잃거나 main 이력을 다시 써 고정 commit을 잃으면 재현할 수 없다.
