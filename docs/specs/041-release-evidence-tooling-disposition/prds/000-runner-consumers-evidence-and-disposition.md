# PRD 000. Release runner 소비자·증거·대체 경로 판정

Status: Complete (Decision)

## Goal

`spec031-release-runner`의 CLI·공개 API·source-bound artifact 계약을 누가 쓰며 어떤 검증을 제공하는지 확인하고 **유지 / 범위 축소 / 제거** 중 근거 있는 결론을 기록한다. 이 PRD 하나로 [041 SPEC](../SPEC.md)의 판정을 독립 완료하며 실제 코드 제거는 수행하지 않는다.

## Scope

1. CLI `--help`, success fixture, 실패 입력, 격리 evidence root의 current-worktree 결과와 stdout/stderr/exit 의미를 각각 관찰한다. 공개 Rust 함수, 테스트, README/USAGE, CI 및 사용자 소유 스크립트 소비를 확인하고 미확인 외부 사용을 0으로 단정하지 않는다.
2. 현행 v5(이 PRD를 쓸 때는 v3) coverage/command/external owner/source identity와 보존 v2–v4 artifact 판독을 분리한다. 정확한 requirement→명령→transcript→result 연결, cleanup/triage/dirty 관측 및 archive hash/변조 거부를 현재 동작으로 기록한다.
3. 같은 누락·명령 실패·stale source·위조 PASS·변조 입력으로 기존 runner와 제안된 대체 gate를 각각 검사해 결과/재현성 동등성·실행 비용을 비교한다. 대체가 없으면 미충족이라고 남긴다.
4. 선택한 결론의 CLI/API·artifact 보존, 033/034 독립 runner 비영향, 문서/검증 변경 범위와 후속 구현 순서를 결정문에 기록한다.

## Non Scope

- Spec031 snapshot/replay/config migration이나 다른 release runner 삭제, 과거 evidence를 현행 PASS로 재결속.
- 검증을 통과시키려고 실패 테스트를 지우거나 기존 누락/변조 거절을 완화하는 코드 변경.
- 판정과 동시에 바이너리/API를 제거하거나 기존 사용자 evidence를 덮어쓰는 일.

## Parent Requirement Mapping

- [041 SPEC](../SPEC.md)의 조사할 계약 1–4, 유지/축소/제거 판정 규칙, 변경 전 불변식과 수용 조건 1–3을 이 PRD 하나가 소유한다.

## Acceptance Criteria

1. 바이너리/공개 API/테스트·문서·CI/로컬 스크립트와 artifact 소비의 조사 범위를 적고, 격리된 root에서 help·fixture·실패·current-worktree의 차이를 실제로 확인한다.
2. 실행 시간/비용과 필수 source·owner·coverage·artifact/cleanup 검증표를 만들고, 축소·제거 선택지라면 동일한 실패 입력을 넣어 대체 gate가 fail-closed로 동등한지 확인한다. `cargo test` green만으로 동등하다고 판정하지 않는다.
3. 현행 v5/보존 v2–v4, 035 역사 자료와 033/034 별도 runner의 영향 및 검증 가능한 판독 경로를 적는다. 유지/축소/제거 가운데 하나의 이유·보존 계약·구현/회귀 범위를 결정문에 남긴다. 근거가 부족하면 미결정으로 남긴다.

## Closure Evidence

- CLI/API 호출자 목록, 독립 실행 기록과 측정된 비용·필수 gate 대응표.
- 기존/대체 경로의 누락·실패·변조 테스트 결과, 과거 artifact 호환 영향표와 검토 가능한 결정문.

이 PRD는 **release 도구의 판정**만으로 완료한다. 다른 판정 스펙이나 선택한 구현의 실행을 기다리지 않는다.

## 조사 기록 (2026-10-25)

실행 증거는 로컬 `.omo/evidence/spec041/prd000/`(추적하지 않음)에 있다. `cNN`은 실제 CLI, `tNN`은 비용 측정, `vNN`·`mNN`·`rNN`·`bNN`·`pNN`은 validator 판독, `sNN`은 보존 기록 검사 단계이며 `transcript.md`에 정리했다. 이 evidence가 없는 checkout에서는 실행 증명으로 보지 않는다.

- **실행 조건:** source `3bffc36`, 실행 전후 작업 트리가 깨끗했다. runner binary는 `shacs-projection`과 그 의존 crate의 어떤 source보다 새로 빌드된 `crates/target/debug/spec031-release-runner`다. evidence root, 보존 결과의 사본과 변조 사본, `git archive 3bffc36`로 만든 저장소 사본은 모두 시스템 임시 폴더에 두었다. current-worktree 실행은 사용자 허락을 받아 한 번만 했다(c10). 실제 사용자 config/data/auth root와 기존 release evidence에는 쓰지 않았고, 보존 결과 원본은 읽기만 했다. shell history와 저장소 밖 파일은 보지 않았다.
- **validator harness:** 저장소 밖 Rust 프로그램이 `manifest.json`을 읽어 공개 `validate_spec031_release_artifacts_with_repo_root`를 부른다. 작업 트리에 연결한 것과 `git archive 3bffc36` 사본에 연결해 따로 빌드한 것(고정 commit validator) 두 개를 썼다. 결과 사본은 `manifest.json`의 `evidence_root` 한 필드만 사본 경로로 바꾼다. 첫 시도(v00)는 이 필드를 파일에서 고치지 않아 G7 결과 두 개를 `ArtifactMismatch`로 잘못 보고했다. validator가 디스크의 manifest와 입력을 비교하기 때문에 생긴 harness 결함이므로 그 결과는 판정에서 뺐다.

### 호출자와 조사 범위

| 대상 | 결과 |
|---|---|
| 문서 | `README.md:198-199`와 `docs/USAGE.md:1190,1206`의 수동 실행 예시(current-worktree, success-fixture) |
| 저장소 안 자동화·CI | 없다. `.github`이 없고 `Dockerfile`은 `shacs-cli`만 빌드하며 runner를 부르는 script나 Makefile이 없다 |
| 테스트 | `shacs-projection`의 `tests/spec031_release_runner.rs`(34개)·`tests/spec031_release_runner_exploits.rs`(20개)와 `tests/it/spec031_release_runner_{cli,schema,triage}.rs`. binary 자체는 `it/spec031_release_runner_cli.rs`가 실행한다 |
| 과거 로컬 자동화 | `.omo/worktree-archives/spec035-20260928/`의 035 시기 driver 17개(`g7-integrated/driver.mjs`, `promote-and-run.mjs`, `workspace-no-fail-fast-20260917/driver.mjs` 등). 일부는 지금 CLI가 거절하는 `--phase`를 쓴다. 현재 쓰이지 않는 기록이다 |
| 저장소 밖 script, 다른 machine, `shacs-projection`을 라이브러리로 쓰는 외부 코드 | **미확인**. 없다고 보지 않는다 |

| 공개 Rust 항목 | 저장소 안 소비자 |
|---|---|
| `run_spec031_release_runner` | CLI binary, runner 테스트 |
| `validate_spec031_release_artifacts`, `validate_spec031_release_artifacts_with_repo_root` | runner가 실행 끝에 스스로 부르고, 테스트가 쓴다 |
| `write_spec031_release_artifacts`, `canonical_current_command_specs` | 테스트 |
| `execute_spec031_release_command` | 033 runner(`crates/shacs-core/src/runtime/spec033_release/release_runner.rs`), 030 runner(crate 안 `execute_spec031_release_command_with`) |
| `parse_cargo_test_counts` | 034 runner(`crates/shacs-core/src/runtime/generated_media_release/runner/command_execution.rs`), 030 runner |
| `inspect_spec035_receipt_admission`, `Spec035ReceiptAdmissionReport` | production·비legacy 소비자는 없다. 지울 legacy 모듈 안의 ignored 테스트 `spec035_admission_preserved_receipts_have_scoped_correspondence`(`spec035_admission_test.rs:128-133`)만 부르며, 이 테스트가 읽는 `.omo/evidence/spec035/closure/receipt-admission-20260918.json`은 증거가 보관 폴더로 옮겨져 그 경로에 없다 |
| `Spec031*` 타입과 `SPEC031_RELEASE_RUNNER_SCHEMA` | CLI binary와 테스트. `Spec031ExternalOwnerId::Spec035`는 v2 manifest를 읽을 때 필요하다 |

### CLI 관측 (c01–c10)

| 입력 | exit | stdout / stderr |
|---|---|---|
| `--help` (c01) | 0 | usage 한 줄 / 없음 |
| 인자 없음 (c02) | 1 | 없음 / `spec031 release runner failed: missing --run-id` |
| 잘못된 run id (c03), `--mode bogus` (c04) | 1 | `invalid --run-id`, `unknown --mode: bogus` |
| 035 시기 옵션 `--phase preflight` (c05) | 1 | `unknown argument: --phase` |
| git 저장소가 아닌 repo root (c06) | 1 | `MissingRequiredArtifact`. 035 보관 자료 검사가 먼저 실패한다 |
| 이미 있는 빈 evidence root (c07) | 0 | 새 실행으로 받아들인다 |
| 파일이 있는 evidence root (c08) | 1 | `Io`. 있던 파일은 그대로다 |
| success-fixture (c09) | 0 | 둘 다 비어 있다. 약 2초 |
| current-worktree (c10) | 1 | stdout 없음 / `NonzeroTestsFailed`. 844초 |

success-fixture의 PASS는 가짜 cargo project의 명령과 fixture owner fact로 runner의 작성·검증 경로를 확인할 뿐 현재 source의 출시 승인이 아니다.

c10은 명령 16개를 실행해 65행 중 64행 pass·1행 blocked(`spec031:command:test-workspace`), owner audit 4개(029/030/033/034) pass, 035 보관 자료 receipt 통과, dirty observation 없음으로 summary `BLOCKED`를 남겼다.

- `cargo test --workspace`가 `shacs-core` lib의 034 테스트 `runtime::generated_media_release::runner::fixture::tests::mismatch_uses_isolated_tree` 하나에서 실패했다. 이 테스트는 추적하지 않는 `.omo/evidence/spec034/task-12-integration.json`을 읽는데(`catalog::FIXTURES[0]`) 이 checkout에 없어 `NotFound`가 난다. 043 이전 source에서도 같았다. runner 결함이 아니며 runner는 이 실패를 그대로 보고했다.
- 이 명령에는 `--no-fail-fast`가 없어 첫 실패 package에서 멈췄고 transcript에 남은 테스트는 1,088개다(043의 `--no-fail-fast` 전체 실행은 3,214개 통과·1개 실패). 실패를 PASS로 바꾸지는 않지만 그 뒤 package의 결과는 남지 않는다.
- `spec031-owner-spec035` 명령(CLI readiness parity 테스트, 3.3초)은 v5에서도 실행되지만 어떤 행이나 audit도 인용하지 않는다.

### 비용

| 항목 | 시간 |
|---|---|
| current-worktree 전체 (c10) | 844초 |
| └ `cargo test --workspace` | 706초(84%) |
| └ 집중 테스트 5개와 owner 테스트 6개 | 87초 |
| └ clippy workspace | 28초 |
| └ fmt, CLI·TUI build | 22초 |
| success-fixture (c09) | 약 2초 |
| runner 단위 테스트 `--lib spec031::release_runner` 113개(7개 ignored) (t01) | 36.87초 |
| └ v2·035 전용 100개(`spec035_*` 모듈 85개, `audit::spec035_*` 15개) | `spec035_*` 85개만 35.99초 (t02) |
| └ 나머지 13개(035 보관 자료 5, 명령 검증 5, audit 3) | 약 1초 |
| runner 통합 테스트 54개 (t03) | 16초 |

`crates/shacs-projection/src/spec031/release_runner/`는 11,440줄이고 그중 `spec035_*` 파일이 6,865줄(테스트 포함)이다. 나머지 4,575줄에도 v2 분기가 있다. [043](../../043-app-capability-retirement/SPEC.md)과 [044](../../044-video-analyzer-retirement/SPEC.md)는 기능을 퇴역시키면서 v2 판독이 깨지지 않도록 `spec035_*` fixture와 v2 분기를 고쳐야 했다.

### 검증 항목과 변조 입력 (m01–m13, r·b, `out/validate-*.txt`)

변조 결과는 JSON 사본을 모두 맞춰 고치고 `summary.md`도 다시 만들었다(만드는 방법은 기록된 두 summary와 byte 단위로 같음을 확인했다). 그래서 거절이 허술한 위조 때문에 생긴 것이 아니다.

| 검증 항목 | 입력 | 판정 |
|---|---|---|
| 필수 산출물 | v5 fixture에서 `results.json` 삭제 (m02) | `MissingRequiredArtifact` |
| transcript 결속 | v5 fixture의 stdout 한 바이트 추가 (m03) | `ArtifactMismatch` |
| owner audit | v5 fixture에서 034 audit 삭제 (m04) | `ArtifactMismatch` |
| 실패 명령 | 기록 그대로의 c10 (m05) | `NonzeroTestsFailed` |
| 위조 PASS | 실패한 workspace 테스트를 통과로 바꾸고 개수 1088/0을 남김 (m06) | `NonzeroTestsFailed`. transcript를 다시 센다 |
| 위조 PASS | 같은 위조에서 개수를 `null`로 비움. summary는 PASS, stdout에는 `test result: FAILED`가 그대로 (m07) | **valid**. 전체 cargo gate 기록에 개수가 없으면 transcript를 보지 않는다(`command_validate.rs:154`). 현재 validator의 fail-open이다 |
| 035 보관 자료 hash | 저장소 사본의 `docs/archive/spec035/current/SPEC.md`에 한 바이트 추가 (r02) | v5 fixture·m07 모두 `ArtifactMismatch` |
| owner fact 존재 | 사본에서 034 owner 테스트 이름 변경 (r05, b05) | m07을 그 사본 경로로 옮기면 `BlockedAsPass` |
| stale source | 사본에서 031 요구사항 문구(SPEC 63행)나 034 Status를 `Open`으로 바꿈 (r03, r04, b03, b04) | 모두 valid. v5는 commit/tree를 결속하지 않아 실행 뒤 바뀐 문구·status를 모른다 |
| repo root 결속 | current-worktree 결과를 다른 경로의 사본으로 판독 (r01) | `InvalidCommandEvidence`. 명령 cwd가 repo root 경로에 묶인다 |

v5 실행 산출물은 명령 transcript를 fnv64로, 035 보관 자료를 sha256으로 묶는다. dirty worktree는 판정과 별도인 observation으로 남고, cleanup receipt가 없거나 정리되지 않았으면 거절한다(통합 테스트 `spec031_release_artifact_contract_rejects_missing_cleanup_receipt`, `rejects_cleanup_receipt_not_cleaned_status`). 통합 테스트 54개는 이 밖에 symlink·경로 탈출·중복 명령·status/exit 불일치·owner fact 대체·추가·순서 변경·누락을 다룬다.

### 보존 자료와 판독 (v01, v02, p01)

| 자료 | 위치 | 현재 판독 |
|---|---|---|
| v5 | 이번 조사의 success-fixture·current-worktree 결과뿐 | success-fixture valid(m01), current-worktree `NonzeroTestsFailed`(m05) |
| v4·v3 | 이 machine에 보존된 결과가 없다 | 테스트가 만든 입력으로만 확인(`reads_v4_runs_with_the_retired_app_owner_fact_from_the_archive`, `reads_v3_runs_with_the_retired_video_owner_fact_from_the_archive`). 043 앱·044 영상 보관 사본의 sha256으로 지운 테스트의 존재를 확인한다 |
| v2, execution binding 없음 | canonical 10개 중 8개(`f1-r01-current-final`, `final-compiled-runner-{current,fixture}`, `workspace-no-fail-fast-20260917/observed-current` 등). current-worktree 6개는 146행 중 141행 blocked·summary `BLOCKED`, fixture 2개는 146행 pass·summary `PASS` | 모두 `UnsupportedSchema`. 지금 validator는 이 형식을 읽지 않는다 |
| v2, G7 binding | `closure/finalization/g7-integrated/{runner-output,retained-originals}`. summary `pending-final-audit`, 146행(Spec031 66 pass, Spec035 74 pass, Spec035 postrun 6 blocked), owner 6개 pass, 명령 17개 통과 | 둘 다 valid. `docs/archive/spec035/g7/`의 sha256과 `spec035_*` 장치로 판독한다 |
| v2, 테스트 임시 출력 | `closure/f1-r01-owned-tmp/` 아래 49개. 035 F1 보정 때 runner 계약·exploit 테스트가 만든 임시 root이며 일부러 변조한 입력도 들어 있다 | 모두 `UnsupportedSchema`(`out/validate-owned-tmp-summary.txt`) |
| 035 비활성 문서 | `docs/archive/spec035/current/` 12개와 `SHA256SUMS` | 실행·판독 때마다 sha256 확인. v5 coverage의 `spec031:archive:spec035` 행 |

035 보관 폴더에서 `schema`가 v2인 `manifest.json`은 59개다(`out/v2-manifests.txt`). 이 판정에서 **canonical v2 결과**는 `f1-r01-owned-tmp` 밖의 10개를 뜻한다. 035의 최종 인계도 이 임시 root를 보존 hash 감사에서 빼고 지우지 않았다(`closure/final-handoff.md:39`, `closure/final-compiled-qa.json`의 `historicalIntegrity.scope`).

고정 commit validator(p01)는 이 조사에서 쓴 (repo root, 입력) 98쌍 모두에서 작업 트리 validator와 같은 판정을 냈다(`out/pinned-vs-working.tsv`: m01–m13, canonical 10개, 임시 출력 49개, r01–r05·b01–b05). `3bffc36`은 지금 작업 트리와 같은 source이므로 이 결과는 판정이 바뀌었는지가 아니라 `git archive`로 꺼낸 commit에서 validator를 다시 빌드해 같은 판정을 얻는 절차가 동작함을 보인다.

출처 연결: `docs/USAGE.md`의 과거 035 runner summary 링크 두 개는 증거가 `.omo/worktree-archives/spec035-20260928/`로 옮겨져 끊겨 있어 이번에 그 위치로 고쳤다. runner 밖의 035 evidence 링크(`README.md:193`, `docs/USAGE.md:215,950,956`)도 같은 이유로 끊겨 있지만 이 판정의 범위가 아니어서 그대로 두었다. 보관된 `docs/archive/spec035/*/CLOSURE.md`의 `video_analyzer_spec035/store.rs` 링크는 044 뒤 끊겼고, 보관 문서는 byte가 고정되어 고치지 않는다.

### 보존 기록 실증 (s01–s09)

canonical v2 결과 10개 각각의 파일별 sha256을 결과 폴더 기준 상대 경로로 기록하고(`shasum -a 256` 형식) 정확한 파일 목록과 함께 검사했다. 원본 10개는 모두 통과했다(s01). G7 사본은 transcript 변경, binding 삭제, 파일 추가, blocked 행 위조, 명령 실패 위조를 모두 거절했고(s02–s07), binding 없는 v2 사본의 BLOCKED→PASS 위조도 거절했다(s08). `shasum -a 256 -c`만으로는 추가된 파일을 잡지 못했으므로(s09) 파일 목록 비교가 필요하다.

### 대체 가능성

| 능력 | Spec031 runner v5 | `cargo test`만 | 033·034 runner | v2 보존 경로(sha256 기록 + 고정 commit validator) |
|---|---|---|---|---|
| 031 요구사항 65행 ↔ 명령 ↔ transcript 결속 | 있음 | 없음 | 없음. 각자 033 blocker 17개, 034 요구사항만 본다 | 고정 commit에서 당시 146행 판정을 재현 |
| 029/030/033/034 정확한 owner fact | 있음 | 없음 | 자기 것만 | — |
| 실패 명령·누락·transcript 변조 거절 | 있음 | 실패 명령만 | 조사하지 않음 | 모든 byte 변경·누락·추가를 거절 |
| 위조 PASS 거절 | 개수를 비운 위조는 통과(m07) | 해당 없음 | 조사하지 않음 | byte 변경으로 거절 |
| source identity | repo root 경로, 035 보관 자료 hash, owner 테스트 존재. commit/tree 결속 없음 | 없음 | 각자 source manifest 모듈이 있다(이번에 시험하지 않음) | commit hash와 결과 byte 고정 |
| 사용자 진단 | `summary.md`, triage, stderr 오류 이름 | test 출력 | 자기 형식 | 검사 출력의 변경·누락·추가 목록 |

033·034 runner는 별도 소유자이며 031 요구사항을 보지 않는다. source manifest는 031에 가져올 수 있는 방식일 뿐 031 계약을 대신하지 않는다.

### 033/034(와 030) runner 영향

세 runner가 쓰는 `execute_spec031_release_command`·`execute_spec031_release_command_with`·`parse_cargo_test_counts`와 명령 타입은 남긴다. v5의 029/030/033/034 owner fact, 각 runner의 catalog·archive·schema는 이 판정으로 바뀌지 않는다.

### 세 선택지

- **유지:** 현재 경로의 독립 가치는 확인됐다. 하지만 runner 코드 대부분과 단위 테스트 시간 대부분이 새로 만들어지지 않는 v2 형식 판독에 쓰이고, 그 장치가 읽는 결과는 v2 59개 중 G7 두 개뿐이며, 퇴역 작업마다 고쳐야 했다. 유지할 근거가 이 부분에는 없다.
- **범위 축소:** 아래 판정이다.
- **제거:** 031 요구사항·owner fact·보관 자료를 묶는 다른 gate가 없다. `cargo test`와 033·034 runner는 대체가 아니다. 대체 gate를 실행으로 보일 수 없어 근거가 없다.

## 판정 (2026-10-25): 범위 축소

Spec031 release runner의 현재 경로(CLI, v5 생성, v5·v4·v3 판독, 033·034·030이 쓰는 공용 명령 실행 API)는 남긴다. 퇴역한 035 시기의 v2 판독 장치와 production 소비자가 없는 공개 API는 지운다. canonical v2 결과는 byte 기록과 고정 commit validator라는 별도 보존 경로로 읽는다. 근거는 다음과 같다.

1. 현재 경로는 대체가 없는 독립 검증이다. 031 요구사항 65행을 정확한 명령·transcript·owner fact·035 보관 자료 hash에 묶고, 실패 명령·누락 산출물·transcript 변조·owner audit 삭제·보관 자료 변조·owner 테스트 삭제를 거절했다(c10, m02–m06, r02, b05).
2. v2 판독 장치가 runner 코드(`spec035_*` 6,865줄/11,440줄)와 단위 테스트(113개 중 100개, 약 36초/37초)의 대부분이다. 그런데 새 v2 결과는 만들어지지 않고(CLI는 v5만 쓰며 035 시기 `--phase`를 거절한다, c05), canonical v2 결과 10개 중 8개와 테스트 임시 출력 49개는 지금도 읽지 못한다(v01). 읽히는 것은 G7 두 개뿐이다.
3. 이 장치는 기능을 퇴역시킬 때마다 고쳐야 했다(043, 044).
4. 보존 경로가 같은 입력을 막는다는 것을 실행으로 보였다. 고정 commit validator는 이 조사의 98쌍 입력 모두에서 지금과 같은 판정을 냈고(p01), byte 기록은 원본을 통과시키고 변조·누락·추가·위조 사본을 모두 거절했다(s01–s08). 축소된 validator에 v2를 넘기면 `UnsupportedSchema`로 끝나 PASS가 되지 않는다. 지금 canonical 8개와 임시 출력 49개가 받는 판정과 같다.
5. `inspect_spec035_receipt_admission`과 `Spec035ReceiptAdmissionReport`는 production·비legacy 소비자가 없다. 지울 legacy 모듈의 ignored 테스트 하나만 부른다.

축소는 출시 gate 시간을 거의 줄이지 않는다. current-worktree 시간의 84%는 어느 gate에나 필요한 `cargo test --workspace`다. 줄어드는 것은 runner 단위 테스트 약 36초와 퇴역 작업마다 생기던 유지 부담이다.

이 조사에서 현재 validator의 fail-open(m07)을 찾았다. 축소와 무관한 기존 결함이지만 "실패한 Cargo 명령을 PASS로 바꾸지 않는다"는 불변식에 어긋나므로 후속 구현의 첫 단계로 고친다. 이 판정은 코드 변경이나 자료 삭제가 아니다. 아래 계약은 별도 구현 작업이 지킬 경계다.

### 남길 계약

- **CLI:** 옵션 네 개, 두 mode, stdout·stderr·exit code(c01–c10)를 그대로 둔다.
- **v5 생성:** schema 이름, 65행, owner 4개, 명령 16개(`spec031-owner-spec035` 포함), 035 보관 자료 receipt, 산출물 목록을 바꾸지 않는다.
- **판독:** v5·v4·v3 판독과 그 보관 사본(043 앱, 044 영상, `docs/archive/spec035/current/`)을 그대로 둔다. 바뀌는 것은 아래 1단계의 개수 검사 강화뿐이다.
- **공개 API:** 아래 "지울 것"의 세 항목 외에는 그대로다. `Spec031ExternalOwnerId::Spec035`는 v2 manifest를 읽어 `UnsupportedSchema`로 거절하기 위해 남긴다.
- **테스트:** 통합 테스트 54개, 남는 단위 테스트 13개, `tests/it/spec031_*`를 지우거나 약하게 만들지 않는다. 030·033·034 runner와 그 테스트는 손대지 않는다.

### 지울 것

- **v2 판독 장치:** `validate.rs`의 G7 pending·postrun seal 분기, `spec035_*` 모듈 전부(그 테스트 포함), `audit.rs`·`coverage_*.rs`·`external_audit_facts.rs`·`writer.rs`·`archive_integrity.rs`·`model.rs`의 v2·G7·035 legacy owner 분기와 `.omo/evidence/spec035/prd000-009/manifest.json` 참조. v2 manifest는 schema 판정에서 `UnsupportedSchema`가 된다.
- **공개 Rust API:** `inspect_spec035_receipt_admission`, `Spec035ReceiptAdmissionReport`, `Spec031ReleaseArtifactError::Spec035Evidence`를 deprecation 기간 없이 지운다. 셋 다 지울 legacy 모듈과 그 테스트 밖에서는 쓰이지 않고 `shacs-projection`은 문서화된 라이브러리 약속 없이 path dependency로만 쓰이는 0.1.0 workspace crate다([040 판정](../../040-video-analyzer-capability-disposition/prds/000-video-runtime-value-and-compatibility-decision.md#판정-2026-10-24-제거)과 같은 기준). 외부 코드는 조용히 다르게 동작하지 않고 컴파일 단계에서 실패한다.
- **CLI 오류 출력의 Spec035 분기:** v5에서는 닿지 않으므로 지워도 출력이 바뀌지 않는다.

### 보존 계약 (v2 결과)

- 원본 v2 결과 폴더(로컬 `.omo/worktree-archives/spec035-20260928/`, 추적하지 않음)는 지우거나 고치지 않는다.
- 보존 대상은 canonical v2 결과 10개다. `f1-r01-owned-tmp`의 테스트 임시 출력 49개도 지우거나 고치지 않지만, 035 인계와 같은 경계로 의미·hash 보존 대상에서 뺀다. 이들은 지금도 축소 뒤에도 `UnsupportedSchema`다.
- v2 판독을 지우기 전에 추적되는 보존 문서를 만든다. (a) canonical 10개 결과의 파일별 sha256 기록(결과 폴더 기준 상대 경로, `shasum -a 256` 형식). (b) 결과마다 당시 schema·행 수·summary 상태와 고정 commit validator의 판정표(G7 두 개 valid, 나머지 8개 `UnsupportedSchema`). (c) 고정 commit hash와 harness 원본·실행 방법(결과를 복사하고 `manifest.json`의 `evidence_root`만 사본 경로로 바꾼다). 고정 commit은 v2 판독을 지우기 직전의 main 이력에 남는 commit이며 위 판정표를 재현해야 한다.
- 검사는 `shasum -a 256 -c`와 파일 목록 비교를 함께 쓴다(s09). 기록만으로는 결과의 의미를 다시 판정하지 못한다. 의미는 고정 commit의 validator와 당시 source로만 재현한다.
- G7의 146행·owner 6개와 `pending-final-audit`, 다른 8개의 `BLOCKED`·`PASS` summary는 당시 schema의 결과로만 읽는다. 현행 65행·owner 4개로 재분류하거나 현재 PASS로 재결속하지 않는다.
- `docs/archive/spec035/`의 byte와 `SHA256SUMS`는 그대로다. 보관된 `CLOSURE.md` 두 개의 `spec035_catalog.rs`·`spec035_execution.rs` 링크는 이 제거로 끊기므로, hash 대상이 아닌 `docs/archive/spec035/README.md`에 고정 commit에서 열라는 안내를 더한다. 이 README의 3행은 v5 coverage 행의 source locator이므로 바꾸지 않는다.

### 후속 구현 범위와 순서

별도 구현 스펙 [045](../../045-release-runner-scope-reduction/SPEC.md)가 아래 범위와 순서를 소유한다.

1. **fail-open 수정(축소와 별개, 먼저):** 통과로 기록된 `cargo test` 명령은 개수가 있어야 하고 transcript에서 다시 센 값과 같아야 한다. 개수 없는 기록은 실패나 시간 초과일 때만 받아들인다. 이것은 지원하는 모든 schema(v3–v5)의 판독을 강화하는 호환성 변경이다. runner는 개수를 `Option`으로 남기고(`command.rs:70-74`) 결과 줄이 없거나 읽히지 않으면 `None`이 되며(`command.rs:98-100`), 지금 validator는 전체 cargo gate에서 이를 받아들인다(`command_validate.rs:154-156`). 그래서 "통과 + 개수 없음" 기록이 있는 v3·v4·v5 결과는 수정 뒤 새로 거절된다. 이 machine의 v5 두 결과(`cargo test` 기록 27개)와 v2 59개(810개)에는 그런 기록이 없었고(`out/validate-owned-tmp-summary.txt`), 실제 v3·v4 결과는 없어 확인하지 못했다. 회귀 테스트는 `tests/it/`에 두고 v3·v4 변환 fixture에도 같은 기대 결과를 고정한다.
2. **보존 문서:** 위 (a)–(c)를 만들고 고정 commit validator가 판정표를 재현하는지 확인한다.
3. **v2 판독 제거:** 같은 변경에서 v2 판독 장치·세 공개 항목·CLI 분기를 지운다. G7 v2 판독을 확인하던 테스트(`spec035_postrun_test.rs`의 `persisted_g7_v2_artifact_keeps_old_locators_and_validates_archived_bytes` 등)는 legacy 모듈과 함께 지우고, G7 binding이 있는 v2 입력이 `UnsupportedSchema`가 되는 테스트를 `tests/it/`에 새로 둔다. 통합 테스트 `rejects_v2_artifact_when_legacy_binding_is_absent`, `rejects_v3_artifact_when_legacy_binding_is_present`(현재 layout이 legacy binding을 나열하면 지금처럼 `UnsupportedSchema`), `persisted_v2_owner_enum_still_reads_spec035`는 그대로 통과해야 한다. `convert_to_legacy_run`은 v4·v3 결과를 만드는 helper이므로 그 판독 테스트(`reads_v4_runs_with_the_retired_app_owner_fact_from_the_archive`, `reads_v3_runs_with_the_retired_video_owner_fact_from_the_archive`)는 바꾸지 않는다. 1단계의 새 거절은 그 사본에서 통과한 `cargo test`의 개수만 비운 별도 입력으로 확인한다.
4. **문서:** `docs/USAGE.md`·`README.md`의 runner 절(v2는 보존 문서로 읽는다), 031 SPEC 안내, `docs/archive/spec035/README.md` 안내, 스펙 색인, `docs/PRODUCT-PATH-MAP.md`, `docs/SYSTEM-FOUNDATION.md`.

이 판정과 별개로 남기는 일이며 축소의 선행 조건이 아니다: 034 테스트 `mismatch_uses_isolated_tree`가 추적하지 않는 파일을 읽는 문제(034 소유), workspace 명령의 `--no-fail-fast` 부재, v5 실행의 commit/tree 결속 부재, 아무도 인용하지 않는 `spec031-owner-spec035` 명령. 뒤의 셋은 명령 집합이나 artifact를 바꾸므로 새 schema가 필요하다.

### 구현 때 필요한 회귀 기준

이번 입력(`.omo/evidence/spec041/prd000/`)을 그대로 쓴다.

1. v5: fixture 사본 valid, c10 사본 `NonzeroTestsFailed`, m02 `MissingRequiredArtifact`, m03·m04·r02 `ArtifactMismatch`, m06 `NonzeroTestsFailed`, b05 `BlockedAsPass`. m07은 1단계 뒤 거절된다. v3·v4 판독 테스트가 통과하고, 통과로 기록된 `cargo test`의 개수를 비운 v3·v4·v5 입력은 모두 거절된다.
2. v2: canonical 10개, 임시 출력 49개, m08–m13 사본이 모두 `UnsupportedSchema`이며 valid가 되지 않는다.
3. 보존 기록: canonical 원본 10개는 통과하고 s02–s08의 사본은 모두 거절된다.
4. 고정 commit validator: 기록한 판정표와 같다.
5. CLI: c01–c09의 출력과 exit code가 같다.
6. 030·033·034 runner 테스트가 그대로 통과한다. 어떤 테스트도 추적하지 않는 `.omo` 자료를 읽지 않는다.

### 남은 위험과 한계

- 저장소 밖 script·다른 machine의 사용과 `shacs-projection`을 라이브러리로 쓰는 외부 코드는 미확인이다. CLI는 그대로이고, v2를 넘기면 PASS가 아닌 `UnsupportedSchema`가 나오며, 지운 API는 컴파일 단계에서 실패하므로 조용히 다르게 동작하지 않는다.
- v3·v4 보존 결과는 이 machine에 없어 테스트 입력으로만 판독을 확인했다. 판독 경로는 남지만, 1단계 수정 뒤에는 "통과 + 개수 없음" 기록이 있는 결과가 새로 거절된다. 그런 결과가 저장소 밖에 있는지는 미확인이다.
- 보존 기록은 byte가 같다는 것만 증명한다. 원본 v2 폴더는 추적하지 않는 로컬 자료이므로 이를 잃으면 기록만으로는 결과를 읽을 수 없다.
- m07의 fail-open은 고칠 때까지 남는다. 그때까지 PASS는 runner가 직접 만든 evidence root에서만 믿는다.
- v5는 commit/tree를 결속하지 않아 실행 뒤 바뀐 요구사항 문구나 owner status를 감지하지 못한다(r03, r04).
- 이 checkout의 current-worktree gate는 034 테스트 때문에 계속 `BLOCKED`다.

## 종료 확인 (2026-10-25)

- 수용 조건 1: CLI·공개 API·문서·테스트·CI·과거 로컬 자동화를 조사했고 저장소 밖 사용을 미확인으로 남겼다. 격리된 evidence root에서 help·인자 오류·fixture·current-worktree를 실제로 실행해 stdout/stderr/exit를 기록했다(c01–c10).
- 수용 조건 2: 명령별 실행 시간과 단위·통합 테스트 비용을 측정하고, 필수 산출물·transcript·owner fact·보관 자료·source 결속의 검증표를 만들었다. 같은 누락·실패·위조 PASS·변조·stale source 입력을 현재 validator, 고정 commit validator, byte 기록에 넣어 비교했다(m01–m13, r01–r05, b01–b05, p01, s01–s09). `cargo test` 성공만으로 동등하다고 보지 않았고, 현재 validator의 fail-open과 source 결속 한계를 그대로 기록했다.
- 수용 조건 3: v5·v4·v3·v2와 035 보관 자료의 판독 경로, 033/034/030 runner 영향, 세 선택지를 비교해 결론 하나(범위 축소)와 보존 계약·구현 범위·회귀 기준을 남겼다.
- 문서만 바꿨으므로 workspace Cargo 검증(fmt·clippy·test)은 하지 않았다(AGENTS.md). 증거 수집으로 허락받은 current-worktree 실행 1회, runner 단위·통합 테스트 시간 측정과 목록 조회, 저장소 밖 harness 빌드만 했다.
- 독립 검토(읽기 전용 Oracle): **PASS** (3차). 기록은 `.omo/evidence/spec041/prd000/review/gates.md`에 있다.
  - 1차는 BLOCKED였다. 공개 admission API를 부르는 ignored legacy 테스트를 빠뜨려 "소비자 없음"이라고 썼고, 보존 v2 결과 "10개"의 범위(전체 59개 중 테스트 임시 출력 49개 제외)를 밝히지 않았으며, fail-open 수정이 v3–v5 판독을 바꾸는 호환성 변경이라는 점을 빠뜨렸다는 지적이었다. 소비자 서술을 고치고 canonical 10개와 임시 출력 49개의 경계·판정을 기록했으며, 수정의 호환성 영향과 회귀 기준을 더했다.
  - 2차는 BLOCKED였다. 후속 3단계가 v4·v3 결과를 만드는 `convert_to_legacy_run` 테스트까지 `UnsupportedSchema`로 바꾸라고 잘못 지시했고, 고정 commit validator가 "모든 입력"을 재현했다는 주장이 실행 기록보다 넓었다는 지적이었다. 테스트 지시를 G7 v2로 좁혔고, 두 validator를 98쌍 입력 전부에 실행해 모두 같은 판정임을 기록했다.
  - 3차는 차단·비차단 지적이 없었다.
