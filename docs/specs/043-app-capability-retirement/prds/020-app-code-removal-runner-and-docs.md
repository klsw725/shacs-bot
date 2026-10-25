# PRD 020. 앱 코드 삭제와 release runner·문서 정리

Status: Complete (Scoped)

Depends on: [PRD 000](000-detach-app-data-from-shared-surfaces.md), [PRD 010](010-read-only-apps-retirement-notice.md) 완료. 뒤 번호 PRD에는 의존하지 않는다.

## Goal

production과 테스트에서 앱 코드를 없애 유지 비용을 거두고, release runner와 공통 문서를 그 상태에 맞춘다. 043 이전 binary가 만든 실제 자료로 전체 흐름을 확인해 043을 닫는다.

## Scope

1. **삭제:** `crates/shacs-app` crate와 workspace member·의존, `shacs_core`의 `app`·`app_authoring`·`app_lifecycle` 재노출, `AppSupervisor`와 그 재노출, 앱 extension provenance, 남은 CLI·API·TUI 앱 코드, 앞 PRD에서 처리하지 않은 앱 전용 테스트([039 판정의 회귀 테스트 5](../../039-app-capability-disposition/prds/000-app-usage-compatibility-and-disposition.md#구현-때-필요한-회귀-테스트) 목록). `Cargo.lock`은 manifest 변경에 따라 cargo가 갱신하며 손으로 고치지 않는다.
2. **남기는 것:**
   - agent 파일 도구의 `apps/registry` 보호 분류, PRD 010의 안내와 탐색.
   - 다른 owner의 저장·공개 형식에 든 앱 관련 어휘: Spec031 projection의 `app` 종류와 `spec032` 출처 어휘, permission 출처 `AppTask`, 자동화 이벤트 `AppTaskResult`, activation 출처 `ActivationSource::App`.
   - 031 activation record와 execution snapshot 형식.
   - Tasks schema version 1의 앱 어휘와 그 형식을 고정하는 projection·TUI 표시 테스트(039 목록에 있어도 앱 동작이 아니라 형식을 고정하므로 남긴다).
3. **release runner(남아 있을 때):**
   - spec030 catalog에서 `target-app-registry`를 뺀다. PRD 005 coverage는 남은 target이 유지한다.
   - spec031 runner의 새 run은 spec032 외부 owner fact를 요구하거나 만들지 않고 `spec031-owner-spec032` 명령을 실행하지 않는다. run 결과의 owner 구성이 바뀌므로 새 run의 schema version을 올린다(이 PRD를 쓸 때는 `spec031.release_runner.v3`였고, [044](../../044-video-analyzer-retirement/SPEC.md)가 먼저 v4로 올렸으므로 043은 v5로 올린다). 다른 owner의 fact와 판정은 바꾸지 않는다.
   - 보존된 runner evidence의 판독은 지금과 같은 결과를 낸다. v4·v3·v2는 spec032를 포함한 과거 owner 구성 그대로 판독하고, v4 판독 회귀 테스트를 더한다. 특히 추적되는 v2 G7 archive와 그 검증 테스트는 지금처럼 통과해야 한다. 지운 앱 파일을 근거로 삼던 과거 판독은 035 퇴역 때처럼 보관 자료로 확인하고, 확인 자체를 없애 통과시키지 않는다. 판독은 기록된 결과만 쓰고 지운 Cargo test를 다시 실행하지 않는다.
   - 이미 만든 과거 evidence 파일은 고치지 않는다.
   - (구현 중 발견, 2026-10-25) spec033 runner의 blocker catalog에서 `AbortCleanupIncomplete` edge가 지울 앱 테스트(`spec032_app_supervisor::uncertain_cleanup_remains_recovery_needed`)와 `crates/shacs-core/src/runtime/app_supervisor.rs`를 근거로 삼고 있었다. 039 조사와 이 PRD의 초안은 이 소비자를 놓쳤다. 이 edge의 근거를 033 자동화 자신의 cleanup 실패 규칙(`crates/shacs-core/src/runtime/automation_gates.rs`의 `process_job_result`)을 확인하는 테스트로 옮긴다. 17개 blocker 구성, 한 blocker당 정확한 통과 테스트 하나라는 033 판정 규칙, schema version은 바꾸지 않는다. 033 runner는 현재 schema와 현재 source에 결속된 결과만 받으므로 다시 판독할 과거 결과가 없다.
4. **공통 문서:**
   - runner 외부 owner를 설명하는 README·`docs/USAGE.md`·[031 SPEC](../../031-configuration-runtime-layout-and-execution-snapshots/SPEC.md)의 문장. 031의 상태는 바꾸지 않는다.
   - 스펙 색인의 제품 범위 요약(AI app 언급)과 043 상태, `docs/PRODUCT-PATH-MAP.md`.
   - 017·021·032 SPEC에 043 퇴역 안내를 더한다. `Status:` 줄은 바꾸지 않는다.
5. **최종 확인:** 043 이전 binary(예: `06047cd`)로 바로가기가 없는 data dir에 실제 자료를 만든다. 설치·활성화한 앱, 실행 뒤 정상 중단한 앱, `recover`로 `recovery_needed`가 된 앱, owner를 SIGINT로 끝내 `running` journal과 남은 자식 process가 있는 앱, 등록되지 않은 id의 journal, 사용자 파일이 든 bundle, 초안, authoring 기록을 포함한다.
   - 남은 자식 process는 QA 전용 명령으로 만든다. 명령줄에 고유 표지를 넣고, 잡을 수 있는 신호(HUP·INT·QUIT·USR1·USR2·TERM 등)를 받으면 data dir 밖의 기록 파일에 남긴다. 시작 직후 pid·시작 시각·명령줄을 기록한다.
   - 새 binary로 `apps`의 모든 형태, `runtime inspect`, `tasks --json`, CLI Tasks app action, `serve`의 `/v1/readiness`·`/v1/tasks`·app action, TUI `--once`, 시험용 provider로 agent 요청 한 번을 실행한다.
   - 각 실행 전후에 보존 목록 자료의 파일·폴더 목록과 sha256을 비교한다. `apps` 실행 전후에는 data dir 전체를 비교한다. runtime이 보존 목록 밖에 쓰는 파일은 비교하지 않는다.
   - 모든 실행 뒤 자식 process의 pid·시작 시각·명령줄이 처음과 같고 정지 상태가 아니며 신호 기록 파일이 비어 있음을 확인한다. 정리하기 직전에 identity를 다시 확인한 뒤 QA가 직접 정리하고 기록한다.

## Non Scope

- [041](../../041-release-evidence-tooling-disposition/SPEC.md)의 runner 유지·축소·제거 판정, Spec031 projection 어휘의 정리.
- 남은 자료의 정리·내보내기 도구, 017·021·032의 판정 변경.

## Parent Requirement Mapping

- [043 SPEC](../SPEC.md)의 소유 범위 3·4와 수용 조건 3·5·6을 소유한다.

## Required Contract

1. 삭제는 남긴다고 정한 항목을 건드리지 않는다. 앱 자료를 읽는 production 경로는 PRD 010의 탐색 하나만 남는다.
2. runner를 고쳐도 지운 테스트 때문에 blocked가 생기지 않고, 남은 owner의 판정과 coverage 근거가 바뀌지 않는다.
3. 최종 확인은 앞 PRD들의 테스트를 대신하지 않는다. 043 이전 binary의 실제 자료로 사용자 표면이 함께 성립하는지 본다.

## Acceptance Criteria

1. workspace에 `shacs-app` crate와 앱 전용 코드·테스트가 없다. `shacs_app`·`AppSupervisor`·`AppRegistryStore`·`app_lifecycle` 검색 결과는 남긴다고 정한 항목과 PRD 010의 탐색·fixture뿐이다. fmt, workspace clippy `-D warnings`, 변경한 package의 테스트가 통과한다. 전체 workspace 테스트는 사용자 승인을 받아 실행한다.
2. runner가 남아 있으면 spec030·spec031·spec033 runner의 catalog와 명령 목록에 지운 테스트가 없고 runner 테스트가 통과한다. spec030 PRD 005에는 남은 target이 있다. spec031 runner의 새 run은 올린 schema version으로 spec032 adapter fact를 요구하지 않는다. spec032를 포함한 v4 결과의 판독 회귀 테스트와 보존된 v2 G7 archive의 검증 테스트는 지금과 같은 결과로 통과한다. spec033 runner의 `AbortCleanupIncomplete` edge는 033 자동화 테스트로 통과한다.
3. 최종 확인 기록에서 [043 SPEC](../SPEC.md) 수용 조건 1–4가 함께 성립한다. QA 전용 자식 process는 처음과 같은 identity로 정지 없이 실행 중이었고 신호 기록이 비어 있었다.
4. 문서 검색에서 앱을 현재 기능으로 안내하는 문장이 없다(역사 기록과 퇴역 안내 제외). 017·021·032의 `Status:` 줄은 그대로이고 퇴역 안내가 있다.

## Closure Evidence

1. 삭제 범위 검색 결과와 Cargo 검증 기록.
2. runner catalog·명령·테스트 변경과 결과.
3. 043 이전 binary 자료로 한 최종 실제 표면 QA 기록(자료 트리 비교, 남은 process 확인과 정리).
4. 고친 문서.

PRD 020은 앞 PRD의 닫힌 결과를 소비해 코드 삭제와 공통 정리로 043을 닫는다. 041의 판정이나 다른 신규 기능을 기다리지 않는다.

## 구현 메모 및 검증 근거 (2026-10-25)

- 삭제: `crates/shacs-app` crate(소스 8개, 테스트 5개)와 workspace member, `shacs-core`·`shacs-cli`의 의존. `Cargo.lock`은 cargo가 갱신했고 `shacs-app` 항목과 의존 두 줄만 빠졌다. `shacs_core`의 `app`·`app_authoring`·`app_lifecycle` 재노출, `runtime/app_supervisor.rs`, `app_extension_provenance.rs`, `app_extension_projection.rs`와 그 재노출, core 앱 전용 테스트(`app_compat.rs`, `spec032_app_supervisor.rs`, `spec032_app_extension_provenance.rs`와 support). 검색 결과(`shacs_app`·`AppSupervisor`·`AppRegistryStore`·`app_lifecycle`)는 아래 runner의 과거 결과 판독 descriptor뿐이다.
- 남긴 것: agent 파일 도구의 `apps/registry` 보호 분류, Spec031 projection의 `app` 종류와 `spec032` 출처 어휘, permission 출처 `AppTask`, 자동화 이벤트 `AppTaskResult`, `ActivationSource::App`, containment의 `RuntimeBoundaryKind::AppProcess`, 022 permission ceiling의 `app_declaration_grants_permission`, 018 curator의 `app_provided_skill_reference_evidence`. 모두 다른 owner의 형식·계약이며 앱 crate를 쓰지 않는다. Tasks schema version 1 앱 어휘의 형식 테스트도 남겼다.
- release runner:
  - spec030: catalog에서 `target-app-registry`를 뺐다. PRD 005는 남은 target(`target-core-resources`, `target-cli-user-plugin` 등)이 맡는다.
  - spec031: 새 run은 `spec031.release_runner.v5`이며 외부 owner가 029/030/033/034 넷, coverage가 65행이다. `spec031-owner-spec032` 명령과 032 fact를 요구하거나 만들지 않는다. v4·v3·v2 결과는 032 owner를 포함한 기록된 구성으로 판독하고, 그 032 fact(`crates/shacs-app/tests/app_environment.rs#enable_disable_lifecycle_projection_does_not_create_process_truth`)는 지운 파일 대신 sha256을 고정한 [보관 사본](../../../archive/spec043-app-environment/README.md)으로만 확인하며 테스트를 다시 실행하지 않는다. summary의 기대 owner 수도 schema에서 계산한다. 테스트: `reads_v4_runs_with_the_retired_app_owner_fact_from_the_archive`(v4 결과 판독, 보관 사본 변조 시 `BlockedAsPass`), v3 판독 테스트는 032 owner를 포함하도록 고쳤고, 현재 run은 owner 넷·65행을 확인한다. 보존된 v2 G7 archive 테스트(`persisted_g7_v2_artifact_keeps_old_locators_and_validates_archived_bytes` 등)는 fixture가 지운 파일 대신 보관 사본을 복사하도록만 바꿔 그대로 통과한다.
  - spec033(구현 중 발견): `AbortCleanupIncomplete` edge의 근거를 지운 앱 테스트에서 `crates/shacs-core/tests/spec033_automation_dispatch.rs`의 `incomplete_process_cleanup_is_never_reported_as_success`(cleanup 실패는 job·durable work 모두 `failed`, `automation_gates.rs`)로 옮겼다. 17개 blocker와 판정 규칙, schema는 그대로다.
- 공통 문서: README·`docs/USAGE.md`의 runner 설명(65행, owner 넷, v5, 보관 사본), [031 SPEC](../../031-configuration-runtime-layout-and-execution-snapshots/SPEC.md) Scoped Closure Record의 퇴역 안내, 스펙 색인(제품 범위 요약, 032·039 행), `docs/PRODUCT-PATH-MAP.md`, `docs/SYSTEM-FOUNDATION.md`(목표 문단, 참고 조합), 017·021·032 SPEC의 퇴역 안내(`Status:` 줄 그대로).
- 최종 실제 표면 QA(`.omo/evidence/spec043/final-qa/transcript.md`): 043 이전 binary로 바로가기 없는 data dir에 설치·활성화한 앱, 정상 중단한 앱(bundle에 사용자 파일), `recover`로 `recovery_needed`가 된 앱, owner를 SIGINT로 끝내 `running` journal과 QA 전용 자식 process가 남은 앱, 등록되지 않은 id의 journal, 초안, authoring 기록을 만들었다. 043 binary로 `apps` 20개 형태, `runtime inspect`, `tasks --json`, CLI app action, `serve`의 readiness·Tasks·app action, 시험용 provider로 agent 요청, TUI `--once`를 실행했고 각 실행 뒤 보존 목록 자료(`apps` 전후는 data dir 전체)가 그대로였다. QA 자식은 끝까지 같은 pid·ppid·pgid·시작 시각·명령줄로 정지 없이 실행 중이었고 신호 기록이 비어 있었으며, 정리 직전 identity를 다시 확인한 뒤 QA 스크립트가 process group에 SIGKILL을 보내 정리했다. QA 스크립트의 두 harness 오류(첫 SIGINT가 wrapper subshell에 간 것, zsh에서 잘못 만든 수동 manifest)와 잘못 정한 phase 2 확인 세 개는 기록에 그대로 남기고 바로잡았다.

## 종료 확인 (2026-10-25)

- 범위: `shacs-app` crate와 앱 전용 코드·테스트 삭제, spec030·spec031·spec033 runner 정리, 공통 문서, 043 이전 binary 자료로 한 최종 실제 표면 확인. 041의 runner 판정, Spec031 projection 어휘 정리, 남은 자료의 정리·내보내기 도구는 포함하지 않는다.
- 최종 검증(변경한 package, `--no-fail-fast`, 기록은 `.omo/evidence/spec043/final-tests/`):
  - `shacs-cli` 456 passed / 3 ignored, `shacs-api` 117, `shacs-tui` 38, `shacs-projection` 345 passed / 7 ignored.
  - `shacs-core` 1,560 passed / 1 failed / 3 ignored. 유일한 실패는 baseline 실패다(`runtime::generated_media_release::runner::fixture::tests::mismatch_uses_isolated_tree`, git이 추적하지 않는 Spec034 로컬 evidence 파일 누락). 043 이전 소스(`1e8d141`)에서도 같은 오류로 실패한다.
  - `cargo fmt --all -- --check`와 workspace clippy `-D warnings`도 통과했다.
  - 전체 workspace 실행은 코드 커밋 `fffcac6`에서 사용자 승인을 받아 했다([SPEC 종료 증거](../SPEC.md#종료-증거-2026-10-25)).
- Oracle 독립 검증: **PASS** (1차, 차단 지적 없음, `.omo/evidence/spec043/review/gates.md`).
- 실제 표면: `.omo/evidence/spec043/final-qa/transcript.md`의 F·N 단계, phase 2b와 phase 3.
- 남은 제한: v4·v3·v2 결과 판독은 보관 사본이 있어야 한다. 사본이 없거나 바뀌면 해당 032 owner는 blocked가 된다.
