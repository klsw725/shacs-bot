# 043. 개인용 앱 기능의 퇴역과 남은 자료 보존 명세

Status: Complete (Scoped)

Consumes: 039 app capability disposition decision, 030 trusted resource boundary, 031 activation records and execution snapshots, Tasks·readiness shared projection (schema version 1), current CLI/API/TUI surfaces

## 목적과 완료 경계

[039](../039-app-capability-disposition/SPEC.md)는 `.shacsapp` 앱의 authoring(`apps init/propose/apply/install`)과 lifecycle(`apps enable`부터 `uninstall`까지)을 제품 표면에서 **제거**하고 사용자 자료는 그대로 두기로 판정했다. 이 스펙은 그 판정의 보존 계약을 실제 코드와 표면으로 옮긴다. 039의 판정을 다시 내리지 않고, [017](../017-app-operating-environment/SPEC.md)·[021](../021-app-maker-and-app-authoring/SPEC.md)·[032](../032-app-maker-runtime-and-extension-lifecycle/SPEC.md)의 과거 완료 판정도 다시 열지 않는다.

완료되면 사용자는 다음을 경험한다.

- `shacs-bot apps …`는 아무것도 실행하거나 바꾸지 않는다. 기능이 퇴역했다는 사실과 남은 앱 자료의 위치를 보여주고 0이 아닌 종료 코드로 끝난다. 아직 실행 중일 수 있는 앱은 그렇다고 알린다.
- readiness, Tasks(CLI/API/TUI), agent 실행 기록은 앱 자료가 있든 없든 같은 결과를 낸다.
- 남은 앱 자료는 바뀌지 않는다. 지우기는 사용자가 직접 한다.

이 스펙은 040·041·042의 판정이나 구현을 기다리지 않는다. release runner를 바꾸는 범위는 runner가 남아 있을 때, 지운 앱 테스트를 부르는 항목에 한정한다.

## 소유 범위

1. **공용 표면에서 앱 자료 분리:** readiness `plugin_app`은 plugin 상태만 반영한다. Tasks(CLI `tasks`, API `/v1/tasks`, TUI)는 앱 행과 앱 action을 만들지 않고, schema version 1의 `coverage.app`을 `unavailable`로 보고한다. owner `app`의 action 요청은 기존 `unsupported tasks owner/action pair`로 거부한다. agent의 모델 요청마다 앱 registry와 config를 다시 읽던 앱 소속 activation 조회를 없애고, 031 activation 판정에서 032 앱 상태 입력을 뺀다.
2. **읽기 전용 퇴역 안내:** `apps`·`app` 명령은 하위 명령과 인자에 관계없이 같은 퇴역 안내를 낸다. config가 정한 data dir에서 남은 앱 자료를 찾아 위치와 개수, 앱별 registry 상태·마지막 journal 상태·bundle 유무를 보여준다. journal이 `starting`·`running`·`stopping`·`recovery_needed`이거나 상태를 읽을 수 없는 앱은 process가 아직 실행 중일 수 있고 제품이 확인하거나 멈추지 않았다고 밝힌다. 전체 도움말은 앱을 사용할 수 있는 기능으로 안내하지 않는다.
3. **남은 앱 코드 삭제:** 범위 1·2가 표면을 거둔 뒤에도 남는 `shacs-app` crate, `shacs_core`의 앱 재노출, `AppSupervisor`, 앱 extension provenance, CLI·API·TUI의 앱 코드와 앱 동작을 고정하던 테스트를 지운다. Tasks schema version 1의 앱 어휘와 그 형식을 고정하는 projection·TUI 표시 테스트는 남긴다.
4. **release runner와 공통 문서 정리:** runner가 남아 있으면 지운 앱 테스트를 부르는 spec030 target `target-app-registry`를 없애고, spec031 runner의 새 run이 외부 owner fact `spec032`(`spec031-owner-spec032`)를 요구하거나 만들지 않게 한다. 이미 보존된 runner evidence(추적되는 v2 G7 archive 포함)의 판독 결과는 바꾸지 않는다. spec033 runner의 blocker catalog가 지운 앱 테스트를 근거로 쓰던 edge 하나는 033 자동화 자신의 테스트로 근거를 옮긴다(구현 중 발견, [PRD 020](prds/020-app-code-removal-runner-and-docs.md#scope)). 그 계약을 설명하는 문서를 함께 고친다. 기초 문서·스펙 색인·제품 경로 지도에서 앱을 현재 기능으로 안내하는 부분을 퇴역 안내로 바꾸고, 017·021·032에는 상태 줄을 바꾸지 않고 퇴역 안내를 더한다.

## 기존 owner와의 경계

| 기존 owner | 043이 소비하는 것 | 043이 소유하지 않는 것 |
|---|---|---|
| [039](../039-app-capability-disposition/SPEC.md) | 제거 판정, 보존할 자료와 동작, 바뀌는 표면, 회귀 테스트 기준 | 판정의 재검토, 유지·축소로의 변경 |
| [017](../017-app-operating-environment/SPEC.md), [021](../021-app-maker-and-app-authoring/SPEC.md), [032](../032-app-maker-runtime-and-extension-lifecycle/SPEC.md) | 과거 저장 형식(읽기만) | 과거 완료 판정과 evidence의 재해석, 앱 기능의 수리나 새 구현 |
| [030](../030-trusted-agent-runtime-and-operational-controls/SPEC.md) | trusted workspace, process·sandbox 경계, resource eligibility | 경계 완화, 다른 resource의 activation 의미 변경 |
| [031](../031-configuration-runtime-layout-and-execution-snapshots/SPEC.md) | activation record 형식과 `runtime activation inspect`, execution snapshot 형식, Spec031 projection 어휘 | record·snapshot 형식 변경. Spec031 projection의 `app` 종류와 `spec032` 출처 어휘 삭제(`runtime inspect`는 runtime owner 표시에 `app` 종류를 쓴다) |
| Tasks·readiness 공용 projection | schema version 1의 JSON 형식, readiness 구성 요소 이름 `plugin_app`, transport hello 협상 결과 | schema version 변경, 다른 owner 행·action의 의미 변경 |
| [025](../025-user-extensible-hooks-and-plugins/SPEC.md) plugin | plugin discovery와 plugin 쪽 readiness 판단 | plugins 폴더가 없을 때 `plugin_app`이 `unavailable`이 되는 plugin 쪽 판단의 변경 |
| [041](../041-release-evidence-tooling-disposition/SPEC.md) | 현재 runner 구조와 과거 evidence 판독 규칙 | runner 자체의 유지·축소·제거 판정. 043은 앱 관련 항목과 그 때문에 필요한 run schema version 변경만 다룬다 |

## 반드시 지킬 것

- 다음 자료를 지우거나 고쳐 쓰지 않는다: `<data>/apps/`(registry, bundle, supervisor journal), `authoring/apps/`, `authoring/flow/`, `snapshots/app-lifecycle/`, `snapshots/activation-records.json`, `runtime/app-ledger/`. 이전·변환·자동 정리를 하지 않는다(`<data>`는 config가 있는 폴더).
- 남은 앱 자료를 읽는 경로는 파일·폴더·lock을 만들지 않는다. registry에 적힌 경로나 data dir 밖을 가리키는 symlink를 따라가지 않고 data dir 안의 정해진 위치만 읽는다. 형식이 어긋난 파일은 원문 없이 위치와 읽을 수 없다는 사실만 보인다.
- 남은 앱 process를 실행하거나 멈추거나 신호를 보내지 않는다. 확인하지 못한 앱을 `stopped`로 표시하지 않는다.
- schema version 1인 Tasks JSON, readiness 구성 요소 이름, transport hello 협상 결과, execution snapshot과 activation record 형식은 바뀌지 않는다.
- agent 파일 도구의 `apps/registry` 보호 분류는 남긴다. 다른 owner의 저장·공개 형식에 들어 있는 앱 관련 어휘(예: permission 출처 `AppTask`, 자동화 이벤트 `AppTaskResult`, activation 출처 `ActivationSource::App`)도 형식 호환을 위해 남긴다.
- 지금의 앱 동작을 고정하던 테스트는 그 동작을 거두는 PRD에서 지우거나 바꾼다. `#[ignore]`나 `#[allow(...)]`로 남기지 않는다. 사용자 문서는 동작을 바꾸는 PRD에서 함께 고친다.
- 017·021·032의 `Status:` 줄은 바꾸지 않는다.
- PRD들은 한 브랜치에서 번호 순서대로 구현해 한 번에 병합한다(039의 "한 변경 단위"). 각 PRD는 자기 수용 조건으로 따로 닫는다.

## 수용 조건

검증에 쓰는 **남은 자료 fixture**는 043 이전 binary(예: 039 판정 커밋 `06047cd`)로 미리 만든 실제 형식의 파일이다. 테스트 실행 중에 앱 코드나 `apps` 명령으로 만들지 않는다. fixture에는 활성화된 앱의 registry, 사용자 파일이 든 bundle, `running`·`recovery_needed`·`stopped` journal, 등록되지 않은 id의 journal, 초안, authoring 기록, 앱 execution snapshot이 들어간다. activation record는 production에서 만드는 경로가 없으므로(039 조사), 수용 조건 4에 필요한 활성 record는 테스트가 현재 031 activation record 형식으로 따로 쓴다.

1. fixture에서 `apps`의 모든 형태(하위 명령 13개, 하위 명령 없음, `--help`, 알 수 없는 하위 명령, 별칭 `app`)가 같은 안내와 같은 0이 아닌 종료 코드를 낸다. 안내는 남은 위치와 앱 id를 모두 보여주고, `running`·`recovery_needed`·읽을 수 없는 journal의 앱은 실행 중일 수 있다고 밝히며 `stopped`로 표시하지 않는다.
2. fixture가 있을 때와 없을 때 readiness(`plugin_app`과 전체), CLI `tasks --json`, API `/v1/readiness`·`/v1/tasks`, TUI Tasks 화면의 결과가 같다. 앱 행이 없고 `coverage.app`은 `unavailable`이다. CLI·API의 owner `app` action은 `unsupported tasks owner/action pair`로 거부된다.
3. 043 이전 binary로 실제 자료를 만들고 새 binary로 위 명령과 agent 요청 한 번을 실행한 뒤에도 보존 목록 자료의 파일·폴더 목록과 내용이 같다(runtime이 보존 목록 밖에 쓰는 파일은 비교하지 않는다). `apps` 안내는 data dir 어디에도 파일·폴더를 만들지 않는다. 043 이전 binary가 남긴 QA 전용 앱 자식 process는 같은 identity(pid·시작 시각·명령줄)로 계속 실행되고, 정지 상태가 아니며, 잡을 수 있는 신호를 하나도 기록하지 않는다.
4. 앱 bundle 안 resource에 맞는 활성 activation record가 있어도 execution snapshot에 activation 참조가 붙지 않는다. 031 activation 판정은 앱 상태 입력 없이 기존의 상태·eligibility·workspace·source·content·dependency 대조를 그대로 한다.
5. workspace에 `shacs-app` crate와 앱 전용 코드·테스트가 없고, fmt·workspace clippy `-D warnings`·변경한 package의 테스트가 통과한다. release runner가 남아 있으면 지운 테스트를 부르거나 다시 실행하지 않는다. spec030의 PRD 005 coverage는 남은 target이 유지하고, spec031 runner의 새 run은 spec032 adapter fact를 요구하지 않으며, 보존된 v2 G7 archive의 검증 테스트는 지금과 같은 결과로 통과한다.
6. 사용자 문서가 퇴역 사실, 남은 자료 위치, 직접 지울 때 확인할 것, 대체 경로(사용자 skill 등)를 설명한다. 앱을 현재 기능으로 안내하는 문서가 남지 않는다(역사 기록과 퇴역 안내는 제외).

위 조건은 043의 단독 종료 기준이다. 테스트 green이나 039의 조사 기록만으로 완료 처리하지 않는다.

## 실행 PRD와 독립 종료

각 PRD는 앞 번호의 닫힌 결과와 039의 판정만 소비하며, 구현 전 상태는 모두 `Planned`다.

| PRD | 유일하게 소유하는 결과 | 앞선 PRD 소비 | 자체 완료 표면 |
|---|---|---|---|
| [000 공용 표면에서 앱 자료 분리](prds/000-detach-app-data-from-shared-surfaces.md) | readiness·Tasks(CLI/API/TUI)·agent 실행 기록·031 activation 판정의 앱 의존 제거 | 없음 | fixture 유무에 따른 readiness·Tasks·snapshot 결과 비교 |
| [010 `apps` 읽기 전용 퇴역 안내](prds/010-read-only-apps-retirement-notice.md) | `apps`·`app` 안내, 남은 자료 탐색, 도움말 | 없음 | fixture에서의 안내 내용·종료 코드·무변경 |
| [020 앱 코드 삭제와 runner·문서 정리](prds/020-app-code-removal-runner-and-docs.md) | crate와 앱 전용 코드·테스트 삭제, runner 항목 정리, 공통 문서, 043 이전 binary 자료로의 최종 확인 | 000, 010 | 앱 코드 없는 workspace와 실제 표면 QA |

소유 범위와 수용 조건마다 owner는 하나다. 범위 1과 수용 조건 2·4는 000, 범위 2와 수용 조건 1은 010, 범위 3·4와 수용 조건 3·5·6은 020이 소유한다. 010은 000의 결과를 소비하지 않지만, 000이 `apps` 명령으로 상태를 만들던 readiness·Tasks 테스트를 먼저 거두므로 번호 순서대로 구현한다. 다른 PRD의 결과를 끝까지 이어 확인하는 것은 소비이지 공동 소유가 아니다. 043 최종 판정에는 세 PRD의 종료 증거와 020의 최종 실제 표면 기록을 함께 확인하며 별도의 뒤 번호 PRD를 요구하지 않는다.

## 종료 증거 (2026-10-25)

PRD [000](prds/000-detach-app-data-from-shared-surfaces.md), [010](prds/010-read-only-apps-retirement-notice.md), [020](prds/020-app-code-removal-runner-and-docs.md)이 같은 독립 Oracle 검토에서 각각 **PASS**를 받았고, 수용 조건 1–6과 반드시 지킬 것 전체도 스펙 차원에서 **PASS**였다(1차, 차단 지적 없음). 비차단 지적 네 개는 반영했다(아래). 이는 현재 self-hosted 지원 범위의 scoped 완료이며 전체 release seal이 아니다.

- 조건 1: 043 이전 binary로 만든 자료에서 `apps`의 20개 형태(하위 명령 13개, 하위 명령 없음, `--help`, `-h`, 알 수 없는 하위 명령, 별칭 `app`, 하위 명령 뒤 `--config`)가 모두 종료 코드 1과 같은 안내를 냈다. 안내는 남은 위치 일곱 곳과 앱 id 다섯 개를 보였고 `running`·`recovery_needed` 앱을 실행 중일 수 있다고 밝혔으며 `stopped`로 표시하지 않았다(010).
- 조건 2: fixture 유무와 상관없이 readiness(CLI `runtime inspect`·diagnostics, API `/v1/readiness`·`/v1/diagnostics`), CLI `tasks --json`, API `/v1/tasks`, TUI `--once`의 결과가 같았다. 앱 행이 없고 `coverage.app`은 `unavailable`이며 owner `app`의 action은 CLI·API에서 `unsupported tasks owner/action pair`(API 400)로 거부됐다(000).
- 조건 3: 043 이전 binary(`1e8d141`)로 만든 실제 자료에서 위 명령과 시험용 provider를 쓴 agent 요청을 실행한 뒤에도 보존 목록 자료가 그대로였고, `apps` 전후에는 data dir 전체가 그대로였다. owner를 SIGINT로 끝내 남긴 QA 전용 자식 process는 끝까지 같은 pid·시작 시각·명령줄로 정지 없이 실행 중이었고 신호 기록이 비어 있었다. 정리는 identity를 다시 확인한 뒤 QA가 직접 했다(020).
- 조건 4: 앱 bundle 안 resource에 맞춘 활성 activation record가 있어도 agent 요청의 execution snapshot에 activation 참조가 붙지 않았다. 031 activation 판정은 앱 상태 없이 기존 거절 사유를 그대로 냈다(000).
- 조건 5: workspace에 `shacs-app` crate와 앱 전용 코드·테스트가 없다. fmt와 workspace clippy `-D warnings`, 변경한 package의 테스트가 통과했다. spec030 runner에서 `target-app-registry`를 뺐고 PRD 005는 남은 target이 맡는다. spec031 runner의 새 run은 `spec031.release_runner.v5`로 spec032 fact를 요구하지 않고, v4·v3·v2 결과는 보관 사본으로 판독하며, 보존된 v2 G7 archive 테스트는 그대로 통과한다. 구현 중 발견한 spec033 runner의 `AbortCleanupIncomplete` edge는 033 자동화 테스트로 근거를 옮겼다(020).
- 조건 6: 사용 가이드가 퇴역 사실, 남은 자료 위치, 직접 지울 때 확인할 것, 대체 경로(사용자 skill 등)를 설명한다. 017·021·032의 `Status:` 줄은 그대로이고 퇴역 안내가 있다(020).

변경한 package의 테스트 결과(`--no-fail-fast`, `.omo/evidence/spec043/final-tests/`):

- `shacs-core`: 1,560 passed / 1 failed / 3 ignored. 유일한 실패는 044 때부터 있던 Spec034 fixture 테스트(`mismatch_uses_isolated_tree`)이며, 043 이전 소스(`1e8d141`)에서도 같은 오류로 실패한다.
- `shacs-projection` 345 passed / 7 ignored, `shacs-cli` 456 passed / 3 ignored, `shacs-api` 117, `shacs-tui` 38 passed.
- 코드 커밋 `fffcac6`에서 사용자 승인을 받아 전체 workspace 테스트를 실행했다(`.omo/evidence/spec043/full-workspace/`). suite 196개 기준 3,214 passed / 1 failed / 13 ignored이며, 유일한 실패는 같은 Spec034 fixture 테스트다.
- 독립 검토의 비차단 지적: 읽을 수 없는 bundle·journal·초안 폴더를 없거나 비었다고 보이던 표시를 `present, unreadable`로 고치고 테스트를 더했다(패키지 테스트 뒤의 유일한 코드 변경, 해당 테스트·clippy·fmt 재확인, 위 전체 실행에 포함). 패키지 테스트 뒤 바뀐 문서와 source digest는 `.omo/evidence/spec043/verification-summary.txt`에 적었다. 기초 문서의 오래된 스펙 상태 목록도 고쳤다.

남은 앱 자료를 정리하거나 내보내는 도구, 남은 앱 process를 확인하거나 멈추는 도구는 제공하지 않는다. 사용자 안내는 [사용 가이드](../../USAGE.md#앱)를 따른다.
