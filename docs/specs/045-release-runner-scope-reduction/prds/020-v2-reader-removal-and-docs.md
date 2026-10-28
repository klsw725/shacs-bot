# PRD 020. v2 판독 장치·공개 API 삭제와 문서

Status: Planned

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
