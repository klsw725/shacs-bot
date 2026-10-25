# PRD 010. `apps` 읽기 전용 퇴역 안내

Status: Complete (Scoped)

Depends on: [039](../../039-app-capability-disposition/SPEC.md) 판정. [PRD 000](000-detach-app-data-from-shared-surfaces.md)의 결과는 소비하지 않지만 번호 순서대로 구현한다. 뒤 번호 PRD에는 의존하지 않는다.

## Goal

`apps`·`app`이 아무것도 실행하거나 바꾸지 않고, 기능이 퇴역했다는 사실과 남은 앱 자료를 정확히 알려 사용자가 직접 판단하게 한다. 실행 중일 수 있는 앱을 멈춘 것처럼 보이게 하지 않는다.

## Scope

1. **명령 형태:** `apps`·`app`의 모든 형태(하위 명령 13개, 하위 명령 없음, `--help`·`-h`, 알 수 없는 하위 명령, 그 밖의 인자)가 같은 안내를 내고 같은 0이 아닌 종료 코드로 끝난다. 기존 하위 명령의 처리 코드를 지운다. 전체 도움말은 `apps`를 사용할 수 있는 기능으로 안내하지 않는다.
2. **탐색 범위:** `--config`(없으면 기본 config)가 정한 data dir 안의 정해진 위치만 읽는다: `apps/registry.json`, `apps/<id>.shacsapp/`, `apps/app-supervisor/<id>/state.json`, `authoring/apps/draft-<id>/`, `authoring/flow/`, `snapshots/app-lifecycle/`, `runtime/app-ledger/`. registry에 적힌 경로와 data dir 밖을 가리키는 symlink는 따라가지 않는다. `apps/app-supervisor/<id>/receipts/`와 `lifecycle.lock`은 symlink를 따라가지 않는 메타데이터 조회로 존재와 개수만 확인하고, 열거나 새로 만들지 않는다(지금의 journal 읽기 API는 lock 파일을 만들므로 쓰지 않는다). `snapshots/activation-records.json`은 031 자료라 앱 자료로 나열하지 않지만 보존 대상이다.
3. **안내 내용:**
   - 퇴역 사실과 [039 판정](../../039-app-capability-disposition/SPEC.md#판정-결과-2026-10-24), data dir, 위치별 존재와 파일 수.
   - registry나 journal 어느 쪽에서 찾은 앱 id든 id별 registry 상태, 마지막 journal 상태, bundle 유무.
   - journal이 `starting`·`running`·`stopping`·`recovery_needed`이거나 읽을 수 없는 앱은 process가 실행 중일 수 있고 제품이 확인하거나 멈추지 않았다고 밝힌다. `stopped`·`failed` journal은 마지막 기록을 그대로 보이되 지금 실행 중이 아님을 보증하지 않는다.
   - 자료는 지우지 않았고 사용자가 직접 지울 수 있다는 사실, 지우기 전에 실행 중일 수 있는 process를 먼저 확인하라는 주의, 절차·지식을 어시스턴트에 줄 대체 경로(`<data>/skills/<name>/SKILL.md`).
   - 남은 자료가 없으면 그 사실을 말한다.
4. **읽기 방식:** 파일·폴더·lock을 만들지 않고 config를 다시 쓰지 않는다. config 파일이 없거나 읽을 수 없어도 아무것도 만들지 않고, config 경로가 정한 data dir을 읽기만 한다. 형식이 어긋난 파일은 원문 없이 위치와 읽을 수 없다는 사실만 보인다. bundle 안 파일 내용, registry의 비밀 요청 이름과 권한 요청은 보이지 않는다. 이 탐색은 앱 crate 없이 동작해 [PRD 020](020-app-code-removal-runner-and-docs.md) 뒤에도 그대로 남는다.
5. **문서:** `docs/USAGE.md`의 앱 절을 퇴역 안내, 남은 자료 위치, 직접 지울 때의 주의, 대체 경로로 바꾼다. `docs/SYSTEM-FOUNDATION.md`의 app bundle 배치와 skill 위치 목록의 앱 줄을 퇴역 사실로 고친다.
6. **테스트:** fixture([043 SPEC](../SPEC.md#수용-조건))와 빈 data dir에서 모든 명령 형태의 안내·종료 코드, 남은 위치와 앱 id 나열, 비종료·읽을 수 없는 journal의 문구, 자료 트리 무변경을 확인한다. 형식이 어긋난 registry·`state.json`, data dir 밖을 가리키는 symlink(위치 폴더·`receipts/`·`lifecycle.lock`이 symlink인 경우 포함), config가 없는 경우를 더한다. CLI의 앱 명령을 고정하던 기존 테스트(parser, `apps_*` 단위 테스트, 앱 명령을 부르던 통합 테스트)는 지우거나 바꾼다.

## Non Scope

- readiness·Tasks·agent 실행 기록([000](000-detach-app-data-from-shared-surfaces.md)), crate 삭제와 release runner([020](020-app-code-removal-runner-and-docs.md)).
- 남은 자료의 정리·내보내기·이전 명령, 남은 process를 확인하거나 멈추는 도구, 기계용 JSON 출력.

## Parent Requirement Mapping

- [043 SPEC](../SPEC.md)의 소유 범위 2와 수용 조건 1을 소유한다.

## Required Contract

1. 안내는 읽기만 한다. 확인할 수 없는 앱을 멈췄다고 하지 않는다.
2. 종료 코드와 안내 구조는 모든 명령 형태에서 같다. 남은 자료 유무는 안내 내용만 바꾼다.
3. 위치는 사용자가 찾아갈 수 있게 data dir 기준으로 보인다.

## Acceptance Criteria

1. fixture와 빈 data dir에서 모든 명령 형태가 같은 구조의 안내와 같은 0이 아닌 종료 코드를 낸다.
2. fixture에서 안내가 남은 위치와 앱 id를 모두 보여주고, `running`·`recovery_needed`·읽을 수 없는 journal의 앱을 실행 중일 수 있다고 밝히며 `stopped`로 표시하지 않는다.
3. 모든 실행 뒤 data dir 전체의 파일·폴더 목록과 sha256이 같다. config가 없을 때도 아무것도 만들지 않는다. 형식이 어긋난 파일의 원문과 data dir 밖 대상은 출력에 나오지 않는다.
4. 실제 binary로 위 1–3을 실행한 기록이 있다.

## Closure Evidence

1. 명령 형태·자료 상태별 테스트와 자료 트리 비교.
2. 격리 config에서 실제 binary로 실행한 안내 출력과 종료 코드 기록.
3. 고친 사용자 문서와 도움말.

PRD 010은 `apps` 표면만 소유한다. 앱 crate가 남아 있어도 이 PRD의 수용 조건으로 종료한다.

## 구현 메모 및 검증 근거 (2026-10-25)

- 코드: `crates/shacs-cli/src/apps_retirement.rs`. `apps`·`app` 뒤의 모든 인자를 받되 `--config`/`-c` 값만 꺼내고(없으면 전역 `--config`, 그것도 없으면 기본 config), config 파일을 읽지 않고 그 폴더를 data dir로 삼는다. 안내는 `CliError::Retired`로 stderr에 나가고 종료 코드는 1이다. 정해진 위치(`apps/registry.json`, `apps/<id>.shacsapp/`, `apps/app-supervisor/<id>/`, `authoring/apps/`, `authoring/flow/`, `snapshots/app-lifecycle/`, `runtime/app-ledger/`)만 data dir부터 한 단계씩 symlink를 따라가지 않는 메타데이터 조회로 찾는다. 경로의 어느 단계든 symlink면 "symlink, not followed"로만 표시한다. `receipts/`와 `lifecycle.lock`은 개수만 세고 열지 않으며, 읽는 파일은 `registry.json`과 `state.json`뿐이다(최대 1 MiB, JSON 필드 `entries.*.lifecycleState`와 `state`만, 알려진 값만 표시). bundle 안 파일 내용, registry의 경로·비밀·권한 요청은 출력하지 않는다. 앱 id는 039 이전 규칙(소문자·숫자·`-_.`)에 맞을 때만 그대로 보인다. 앱 crate를 쓰지 않는다.
- 기존 하위 명령 처리 코드(`run_apps_command`, `apps_*` 함수·보고서·형식 함수·parser, `spec031_cli/app.rs`, `CliError`의 앱 variant)를 지웠고 전체 도움말에서 `apps` 줄과 `--bundle`·`--app-id` 설명을 뺐다.
- 테스트(`crates/shacs-cli/tests/it/spec043_leftover_apps.rs`, 실제 binary):
  - `every_apps_form_prints_one_read_only_notice_and_fails`: fixture와 빈 data dir에서 19개 형태와 하위 명령 뒤 `--config` 형태가 종료 코드 1, 빈 stdout, 같은 안내를 내고 data dir 전체(파일·폴더·symlink)가 그대로다.
  - `notice_lists_leftover_locations_and_never_claims_a_live_app_stopped`: 일곱 위치의 개수, 앱 id 네 개, `running`·`recovery_needed`의 "may still be running" 문구, `stopped`·`failed`의 "last record" 문구, registry 경로 비노출.
  - `malformed_leftover_files_are_named_without_their_contents`: 깨진 `registry.json`·`state.json`의 원문이 나오지 않고 위치와 unreadable만 나온다.
  - `symlinks_out_of_the_data_dir_are_never_followed`: `authoring/flow`(위치 폴더), `receipts/`, `lifecycle.lock`, `state.json`을 data dir 밖을 가리키는 symlink로 바꿔도 밖의 내용이 출력에 나오지 않고 밖과 안의 파일이 그대로다.
  - `missing_config_still_reads_only_and_creates_nothing`: config가 없어도 같은 폴더를 읽기만 하고, 없는 data dir은 만들지 않는다.
  - `unreadable_leftover_folders_are_reported_as_unreadable`: 목록을 읽을 수 없는 journal·초안·`apps/` 폴더를 없거나 비었다고 하지 않고 `present, unreadable`로 표시한다(독립 검토의 비차단 지적을 반영한 수정).
  - 지운 기존 테스트: inline `parser_handles_apps_command_surface`와 `apps_*` 단위 테스트 9개, `spec031_owner_remediation`의 앱 명령 사례 3개. 도움말 테스트는 `apps` 줄이 없음을 확인하도록 바꿨다.
- 문서: `docs/USAGE.md` 앱 절(퇴역 안내, 위치 표, 실행 중일 수 있는 앱, 직접 지울 때의 주의, 대체 경로), `docs/SYSTEM-FOUNDATION.md`의 app bundle 배치와 skill 위치 목록.
- 실제 표면(`.omo/evidence/spec043/final-qa/transcript.md` N01–N20): 043 이전 binary로 만든 자료(실행 중인 QA 자식이 있는 `running` 앱 포함)에서 20개 형태가 모두 종료 코드 1과 같은 안내를 냈고 매번 data dir 전체가 그대로였다.

## 종료 확인 (2026-10-25)

- 범위: `apps`·`app`의 모든 형태에 대한 읽기 전용 퇴역 안내, 남은 자료 탐색, 도움말과 사용자 문서. readiness·Tasks·agent 실행 기록(000)과 crate 삭제·runner(020)는 포함하지 않는다.
- 최종 검증(변경한 package, `--no-fail-fast`, 기록은 `.omo/evidence/spec043/final-tests/`):
  - `shacs-cli` 456 passed / 3 ignored, `shacs-api` 117, `shacs-tui` 38, `shacs-projection` 345 passed / 7 ignored.
  - `shacs-core` 1,560 passed / 1 failed / 3 ignored. 유일한 실패는 baseline 실패다(`runtime::generated_media_release::runner::fixture::tests::mismatch_uses_isolated_tree`, git이 추적하지 않는 Spec034 로컬 evidence 파일 누락). 043 이전 소스(`1e8d141`)에서도 같은 오류로 실패한다.
  - `cargo fmt --all -- --check`와 workspace clippy `-D warnings`도 통과했다.
  - 전체 workspace 실행은 코드 커밋 `fffcac6`에서 사용자 승인을 받아 했다([SPEC 종료 증거](../SPEC.md#종료-증거-2026-10-25)).
- Oracle 독립 검증: **PASS** (1차, 차단 지적 없음, `.omo/evidence/spec043/review/gates.md`).
- 실제 표면: 위 구현 메모의 N01–N20. 표면 QA는 세 PRD가 모두 들어간 binary로 한 번 했다. 독립 검토 뒤 읽을 수 없는 폴더 표시만 고쳤으며 그 변경은 테스트로 확인했다(QA 자료에는 읽을 수 없는 폴더가 없었다).
- 남은 제한: 안내는 남은 process를 확인하거나 멈추지 않으며 기계용 JSON 출력이 없다. 확인 직후 경로가 바뀌는 경쟁 상황까지 막지는 않으며, 안내는 읽기만 한다.
