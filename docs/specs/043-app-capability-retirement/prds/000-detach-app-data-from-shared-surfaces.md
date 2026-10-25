# PRD 000. 공용 표면에서 앱 자료 분리

Status: Complete (Scoped)

Depends on: [039](../../039-app-capability-disposition/SPEC.md) 판정. 뒤 번호 PRD에는 의존하지 않는다.

## Goal

남은 앱 자료가 있든 없든 readiness, Tasks(CLI/API/TUI), agent 실행 기록이 같은 결과를 내게 한다. 공용 표면이 앱 자료를 읽거나 바꾸던 경로를 거두되 공개 형식은 그대로 둔다.

## Scope

1. **readiness:** `plugin_app` 구성 요소는 plugin 쪽 판단만 쓴다. app registry가 없거나 journal이 어떤 상태든 이 구성 요소와 전체 readiness에 영향을 주지 않는다. 구성 요소 이름, 필수 여부, plugin 쪽 판단과 집계 규칙은 그대로다.
2. **Tasks:** Tasks source가 앱 owner 사실을 모으지 않는다. 앱 행이 없고 `coverage.app`은 schema version 1 그대로 `unavailable`이다. owner 문자열 `app`은 child·workflow·automation처럼 해석 단계에서 `unsupported tasks owner/action pair`로 거부하므로 transport 협상이나 owner 조회 전에 끝난다. CLI·API·TUI의 앱 stop·recover 처리 경로를 지운다. TUI Tasks 요약 줄에서 app 항목을 뺀다. transport hello의 capability 협상 결과는 바꾸지 않는다.
3. **agent 실행 기록:** production agent가 앱 소속 activation 조회(`spec031_activation_provider`)를 설치하지 않는다. 031 activation 판정에서 앱 상태 입력과 그 거절 사유(`Spec032Lifecycle`)를 빼고, 상태·030 eligibility·workspace·source·content digest·dependency digest 대조와 digest 사유 매핑은 그대로 둔다. `LiveExecutionSnapshotSource`의 activation provider 주입점, activation record 형식, `runtime activation inspect`도 그대로다.
4. **문서:**
   - `docs/USAGE.md`의 plugin/app readiness 설명과 Tasks 절(지원 조합의 app `stop`/`recover`, 비동기 app stop 문장, `coverage.app`이 퇴역 owner라 항상 `unavailable`이라는 뜻), README의 Tasks owner 목록을 고친다.
   - [031 SPEC](../../031-configuration-runtime-layout-and-execution-snapshots/SPEC.md)에서 activation이 032 lifecycle을 소비하거나 새 실행이 그것을 다시 확인한다고 현재형으로 적은 문장(Acceptance Criteria 14, Dependency rules 3, Closure Evidence 12)과 [031 PRD 004](../../031-configuration-runtime-layout-and-execution-snapshots/prds/004-activation-persistence-replay-and-diagnostics.md) Acceptance Criteria 4에 043 퇴역 사실을 더한다. 031의 상태와 다른 계약은 바꾸지 않는다.
5. **테스트:** 남은 자료 fixture([043 SPEC](../SPEC.md#수용-조건))의 유무로 결과를 비교하는 테스트를 더한다. 이 범위의 앱 동작을 고정하던 기존 테스트는 지우거나 바꾼다: Tasks source가 앱 행을 만들고 앱 action을 처리하던 사례(core·CLI·API·TUI), readiness owner의 앱 사례(`apps` 명령으로 상태를 만들던 사례 포함), activation 판정·live snapshot의 앱 상태 사례. schema version 1의 앱 어휘를 고정하는 projection 모델 테스트와 TUI 행 표시 테스트는 형식 테스트이므로 남긴다.

## Non Scope

- `apps` 명령과 남은 자료 안내([010](010-read-only-apps-retirement-notice.md)), 앱 crate·코드 삭제와 release runner([020](020-app-code-removal-runner-and-docs.md)).
- plugins 폴더가 없을 때 plugin 쪽 `unavailable` 판단, readiness 집계 규칙, Tasks schema version, transport capability 목록.
- goal·child·workflow·automation·recovery owner의 행과 action.

## Parent Requirement Mapping

- [043 SPEC](../SPEC.md)의 소유 범위 1과 수용 조건 2·4를 소유한다.

## Required Contract

1. Tasks schema version 1의 JSON 형식(`coverage.app` 포함), readiness 구성 요소 이름 `plugin_app`, transport hello 협상 결과, execution snapshot 형식은 바뀌지 않는다.
2. 이 PRD가 바꾼 표면은 남은 앱 자료를 읽지도 쓰지도 않는다.
3. 031 activation 판정의 다른 거절 사유와 순서는 그대로다. 앱 상태를 대신할 새 입력을 만들지 않는다.

## Acceptance Criteria

1. fixture가 있을 때와 없을 때 CLI `runtime inspect`와 API `/v1/readiness`의 `plugin_app`과 전체 상태가 같다. 빈 plugins 폴더가 있으면 `plugin_app`은 앱 때문에 `unavailable`·`degraded`·`blocked`가 되지 않는다.
2. fixture가 있을 때와 없을 때 CLI `tasks --json`, API `/v1/tasks`, TUI `--once`의 결과가 같고, 앱 행이 없으며 `coverage.app`은 `unavailable`이다.
3. CLI와 API(`--allow-api-side-effects`)의 owner `app` stop·recover 요청은 `unsupported tasks owner/action pair`로 거부되고(API는 기존 400 응답), fixture의 파일·폴더 목록과 내용이 바뀌지 않는다.
4. fixture의 앱 bundle 안 resource에 맞춰 테스트가 현재 031 activation record 형식으로 쓴 활성 record가 있어도, agent 요청의 execution snapshot에 activation 참조가 붙지 않는다. 031 activation 판정 테스트는 앱 상태 없이 기존 거절 사유를 그대로 낸다.

## Closure Evidence

1. fixture 유무 비교 테스트와 activation 판정 테스트.
2. 격리 config에서 CLI·API·TUI를 실제로 실행한 기록(fixture 유무별 readiness·Tasks 결과, app action 거절, 실행 전후 자료 트리 비교).
3. 고친 사용자 문서.

PRD 000은 공용 표면의 앱 의존만 거둔다. `apps` 명령이 아직 남아 있어도 이 PRD의 수용 조건으로 종료한다.

## 구현 메모 및 검증 근거 (2026-10-25)

- 남은 자료 fixture: `crates/shacs-core/tests/fixtures/spec043_leftover_apps/`. 043 이전 binary(`1e8d141`, 039 판정 커밋 `06047cd` 이후 앱 코드 변화 없음)의 `apps` 명령으로 만든 실제 파일 61개(74 KB)다. 활성화된 앱 세 개의 registry, 사용자 파일이 든 bundle, `running`·`recovery_needed`·`stopped` journal, 등록되지 않은 id(`nope.app`)의 `failed` journal, 초안 네 개, authoring 기록, 앱 execution snapshot 세 개가 들어 있다. 만든 과정은 `.omo/evidence/spec043/fixture/`(추적하지 않음)에 있다. 테스트는 이 파일을 복사해 쓰며 앱 코드나 `apps` 명령으로 자료를 만들지 않는다.
- 코드:
  - readiness: `crates/shacs-cli/src/spec031_cli/readiness_plugin_app.rs`의 `observe`는 plugin 쪽 판단만 쓴다. app registry·journal을 읽던 부분을 지웠다. 구성 요소 이름 `plugin_app`, 필수 여부, plugin 판단과 집계 규칙(`unknown`은 이전처럼 `ready`)은 그대로이며, 요약 문구와 `shacs-projection` remediation 문구에서 app owner 언급만 뺐다.
  - Tasks: `crates/shacs-core/src/runtime/spec035_tasks_source.rs`가 앱 owner 사실을 모으지 않고, `Spec035TasksOwnerSnapshots`에서 앱 입력을 지웠다. `assemble_spec035_tasks`는 schema version 1의 `coverage.app`을 항상 `unavailable`로 둔다. `Spec035TasksSemanticAction::parse`는 owner `app`을 child·workflow·automation과 같이 해석 단계에서 `unsupported tasks owner/action pair`로 거부하며 `AppStop`·`AppRecover`가 없다. CLI(`spec035_tasks_cli.rs`)·API(`spec035_tasks.rs`)·TUI(`action_runner.rs`)의 앱 처리 경로와 TUI Tasks 요약 줄의 app 항목(`tasks_view.rs`)을 지웠다. transport hello 협상 결과(`task_stop` 포함)는 그대로다.
  - agent 실행 기록: production builder가 앱 소속 activation 조회를 설치하지 않는다(`crates/shacs-cli/src/spec031_activation_provider.rs` 삭제). `crates/shacs-core/src/runtime/activation_execution.rs`의 `ActivationCurrentIdentity`에서 앱 상태 입력과 `Spec032Lifecycle` 사유를 지웠고 나머지 대조와 순서, digest 사유 매핑은 그대로다. `LiveExecutionSnapshotSource::with_activation_provider`, activation record 형식, `runtime activation inspect`도 그대로다.
- 테스트:
  - CLI `spec043_leftover_apps::tasks_json_ignores_leftover_app_data`(실제 binary `tasks --json`이 fixture 유무에 같은 출력, 앱 행 없음, `coverage.app` unavailable), `tasks_app_actions_are_unsupported_without_touching_leftover_app_data`(app stop·recover 거부, 자료 무변경).
  - CLI `readiness_owner_parity::leftover_app_data_does_not_change_cli_or_api_readiness`: 실제 binary의 `runtime diagnostics --bundle`·`runtime inspect`와 `serve`의 `/v1/readiness`·`/v1/diagnostics`가 fixture를 넣기 전후로 같은 readiness를 내고 빈 plugins 폴더에서 `plugin_app`이 `ready`다.
  - CLI `spec031_activation_production_builder::production_builder_attaches_no_activation_ref_inside_leftover_app_bundle`: fixture registry의 bundle 경로를 제자리로 바꾸고(옛 조회라면 소속 앱을 찾았을 상태) bundle 안 resource에 현재 031 형식의 활성 record를 써도 agent 요청의 execution snapshot에 activation 참조가 없다.
  - API `spec035_tasks_api::leftover_app_data_leaves_tasks_unchanged_and_app_actions_unsupported`, TUI `spec035_tasks_live::once_tasks_render_ignores_leftover_app_data`(실제 `shacs-tui --once`).
  - core `spec031_activation_execution`: 앱 상태 없이 기존 거절 사유(`ContentDigest`, `Spec030Eligibility`, `WorkspaceIdentity`, `SourceIdentity`)를 그대로 낸다.
  - 지우거나 바꾼 기존 테스트: core `it/spec035_tasks_app_path.rs`와 CLI `it/spec035_tasks_app_path.rs`(삭제), core `spec035_tasks_assembler`·`it/spec035_tasks_actions`·`it/spec035_tasks_terminal_regressions`의 앱 사례, `it/spec031_activation_live_snapshot`의 앱 상태 인자, CLI inline `readiness_owner_tests`의 앱 사례와 `readiness_owner_support`의 빈 registry 생성, API의 app stop 사례, TUI 요약 줄 기대값. schema version 1 앱 어휘를 고정하는 `shacs-projection` `spec035_tasks_model`과 TUI 앱 행 표시 테스트는 남겼다.
- 문서: `docs/USAGE.md` readiness 절·Tasks 절(지원 조합, app 거절, `coverage.app` 의미, 비동기 app stop 문장 삭제), README Tasks owner 목록, [031 SPEC](../../031-configuration-runtime-layout-and-execution-snapshots/SPEC.md) Acceptance Criteria 14·Dependency rules 3·Closure Evidence 12와 [031 PRD 004](../../031-configuration-runtime-layout-and-execution-snapshots/prds/004-activation-persistence-replay-and-diagnostics.md) Acceptance Criteria 4의 퇴역 안내. 031의 상태와 다른 계약은 바꾸지 않았다.
- 실제 표면(`.omo/evidence/spec043/final-qa/transcript.md`): 043 이전 binary로 만든 자료에서 N21–N26(`runtime inspect`, `tasks --json`, CLI app action, `serve`의 readiness·Tasks·app action, TUI `--once`)과 phase 2b(같은 순간에 만든 두 사본의 앱 자료 유무 비교, 빈 plugins 폴더에서 `plugin_app` `ready`). 각 실행 뒤 보존 목록 자료가 그대로였다.

## 종료 확인 (2026-10-25)

- 범위: readiness `plugin_app`의 plugin 전용 판단, Tasks(CLI/API/TUI)의 앱 행·action 제거와 `coverage.app` 유지, production agent의 앱 소속 activation 조회 제거와 031 activation 판정의 앱 상태 입력 제거, 그 표면의 사용자 문서. `apps` 안내(010)와 코드 삭제·runner(020)는 포함하지 않는다.
- 최종 검증(변경한 package, `--no-fail-fast`, 기록은 `.omo/evidence/spec043/final-tests/`):
  - `shacs-cli` 456 passed / 3 ignored, `shacs-api` 117, `shacs-tui` 38, `shacs-projection` 345 passed / 7 ignored.
  - `shacs-core` 1,560 passed / 1 failed / 3 ignored. 유일한 실패는 baseline 실패다(`runtime::generated_media_release::runner::fixture::tests::mismatch_uses_isolated_tree`, git이 추적하지 않는 Spec034 로컬 evidence 파일 누락). 043 이전 소스(`1e8d141`)에서도 같은 오류로 실패한다.
  - `cargo fmt --all -- --check`와 workspace clippy `-D warnings`도 통과했다.
  - 전체 workspace 실행은 코드 커밋 `fffcac6`에서 사용자 승인을 받아 했다([SPEC 종료 증거](../SPEC.md#종료-증거-2026-10-25)).
- Oracle 독립 검증: **PASS** (1차, 차단 지적 없음, `.omo/evidence/spec043/review/gates.md`).
- 실제 표면: 위 구현 메모의 N21–N26과 phase 2b.
- 남은 제한: readiness 구성 요소 이름은 형식을 지키려고 `plugin_app`으로 남는다. plugins 폴더가 없으면 plugin 쪽 판단으로 `unavailable`이며 앱과는 무관하다.
