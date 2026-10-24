# PRD 010. `apps` 읽기 전용 퇴역 안내

Status: Planned

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
