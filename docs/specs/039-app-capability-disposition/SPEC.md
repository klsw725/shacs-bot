# 039. 개인용 앱 기능의 유지·축소·제거 판정 명세

Status: Complete (Decision)

Consumes: 021 app authoring baseline, 030 trusted resource boundary, 032 app lifecycle, current CLI/Tasks/readiness surfaces

## 목적과 종료 조건

이 스펙은 `.shacsapp`의 **초안·제안·적용·설치**와 **설치 후 활성화·실행·중단·복구·제거**가 [`intent.md`](../../../intent.md)의 개인 어시스턴트에 어떻게 기여하는지 실제 사용 경로와 비용으로 판정한다. 기존 [021](../021-app-maker-and-app-authoring/SPEC.md)·[032](../032-app-maker-runtime-and-extension-lifecycle/SPEC.md)의 완료 판정을 다시 열거나 설치 기능을 지금 삭제하자는 문서가 아니다.

완료 산출물은 authoring과 lifecycle을 각각 평가한 뒤 전체 기능에 대해 **유지 / 범위 축소 / 제거 중 하나의 근거 있는 결론**과 실행 시 보존할 계약을 기록한 결정문이다. 증거가 없으면 `미결정`으로 남고 완료 처리하지 않는다. 이 스펙의 결정 완료는 코드를 변경하거나 사용자 앱을 이전했다는 뜻이 아니며, 선택한 변경의 구현은 별도 작업에서 검증한다. 다른 신규 기능이나 판정 스펙의 완료 없이 독립적으로 결정할 수 있다.

## 조사할 현재 계약

1. **사용자 진입:** `docs/USAGE.md`의 `apps init/propose/apply/install/list/inspect/enable/start/stop/restart/recover/disable/uninstall`을 격리된 config/data dir에서 실제로 실행하고, 각 명령이 초안·registry·process 중 어느 사실을 변경하는지 기록한다. `apps init`의 초안 생성과 install/start를 혼동하지 않는다.
2. **사용자 자료:** `authoring/apps/draft-<app-id>/`, `apps/<app-id>.shacsapp/`, registry, activation·supervisor journal과 과거 receipt의 읽기·수정·제거 방식을 확인한다 (`crates/shacs-app/src/app_authoring.rs`, `crates/shacs-app/src/app_lifecycle.rs`, `crates/shacs-core/src/runtime/app_supervisor.rs`). Config가 다른 사용자 자료를 섞지 않는지와 과거 버전 자료를 읽을 수 있는지도 평가한다.
3. **실제 소비자:** CLI와 readiness·Tasks·plugin/extension 진입점에서 app id, enabled/running/recovery 상태, owner locator를 누가 읽는지 기록한다. 저장소 내부 테스트·문서뿐 아니라 사용자가 설치한 앱과 로컬 스크립트의 존재도 확인한다. 발견되지 않은 사용은 `없음`이 아니라 `미확인`으로 적는다.
4. **운영 가치와 비용:** 사용자가 앱을 직접 작성·설치해 얻은 결과와 일반 skill/tool/workflow로 대체 가능한 작업을 비교한다. 설정·오류·업데이트·복구 비용과 상시 process 필요 여부를 측정하고, 기능 가치가 저하되더라도 기존 보호 경계의 필요성을 별도로 판단한다.

## 판정 규칙

| 결론 | 필요한 증거와 후속 계약 |
|---|---|
| 유지 | 사용자에게 필요한 authoring 또는 app 실행 경로가 확인되고, 보존할 명령·data/receipt·readiness/Tasks 계약과 유지 검증 범위를 명시한다. 단지 테스트가 있다는 이유는 사용 가치의 증거가 아니다. |
| 범위 축소 | 남길 단계(예: authoring, 설치, 실행)를 각각 밝히고 빠지는 명령의 대체 안내, 보존할 bundle/registry/receipt 읽기, 기존 설치물의 stop/recover/uninstall 경로와 호환 테스트를 정의한다. |
| 제거 | 실제 설치·호출·projection 소비와 보관 자료를 조사한 뒤, 사용자 소유 앱·초안·receipt를 유실하거나 잘못 읽지 않을 이전/내보내기/명시적 보존 선택과 CLI·API·문서 변경 범위를 정의한다. 제거 결정을 곧바로 데이터 삭제 허가로 해석하지 않는다. |

## 변경 전 불변식

- 평가 중에는 사용자의 실제 config/auth/data dir을 변경하지 않는다. 모든 실험은 별도 사용자 소유 격리 자료로 수행한다.
- `apps apply`가 process 시작이나 permission/credential grant를 만들지 않는 기존 경계를 축소 결론만으로 약화하지 않는다. 저장된 비밀, 활성 resource, trusted-code 실행 범위는 [030](../030-trusted-agent-runtime-and-operational-controls/SPEC.md)과 현재 owner가 정의한 그대로 둔다.
- Authoring 초안과 실행 중인 앱을 하나의 상태로 합치지 않는다. 이미 설치한 앱을 중단해도 기존 사용자 자료가 자동 삭제된 것으로 표시하지 않는다.
- 036의 지속 작업 경험이나 새로운 앱 marketplace의 완료를 039 판정의 조건으로 삼지 않는다.

## 판정 수용 조건

1. 위 명령·저장 자료·consumer 표에 각각 실제 실행 또는 읽기 증거, 미확인 항목, 영향받는 테스트/문서가 연결된다. 초안 재실행의 멱등성과 충돌, 실행 차단과 복구를 적어도 한 번 실제 표면에서 확인한다.
2. Authoring과 lifecycle 각각의 가치·호환 위험을 평가하고, 대체 가능한 경로가 있다면 같은 작업의 결과와 사용자 부담을 비교한다. 사용량을 알 수 없으면 추측으로 0으로 적지 않는다.
3. 결론 하나와 이유, 보존/변경할 사용자 동작·자료·owner 사실, 변경 시 실패할 회귀 테스트, 이후 필요한 구현·데이터 이전 작업을 포함한 결정문을 남긴다. 근거가 부족하면 미결정 상태와 필요한 관측을 밝히고 완료하지 않는다.

이 스펙이 완료돼도 021·032의 과거 implementation evidence나 사용자 앱이 자동으로 삭제되지는 않는다. 다른 판정 스펙의 실행을 기다리지 않는 독립적인 결정이다.

## 실행 PRD

| PRD | 유일하게 소유하는 결과 | 선행 PRD |
|---|---|---|
| [000 앱 사용·호환성 판정](prds/000-app-usage-compatibility-and-disposition.md) | 조사 계약 1–4, 판정 수용 조건 1–3의 실제 근거와 유지/축소/제거 결정문 | 없음 |

PRD 000은 조사만 끝내거나 판정만 추측해서는 완료할 수 없다. 이 한 PRD의 근거와 결론이 갖춰지면 039는 `Complete (Decision)`으로 닫을 수 있으나, 이는 앱 코드·사용자 자료의 변경 완료가 아니다.

## 판정 결과 (2026-10-24)

결론은 **제거**다. 앱 authoring과 lifecycle 전체를 제품 표면에서 빼되 사용자 자료는 그대로 둔다. 근거·호환 계약·회귀 테스트·후속 범위는 [PRD 000의 판정](prds/000-app-usage-compatibility-and-disposition.md#판정-2026-10-24-제거)에 있다.

- 앱은 대화·위임 작업·알림·agent tool/skill/context와 연결되지 않는다. 이 기기의 실제 자료에는 설치가 없고 고치지 않은 초안 하나만 있다. 외부 사용은 미확인이다.
- 격리 실제 표면에서 Ctrl+C 한 번이 고아 process와 사실과 다른 `running`을 남겼다. `recover` 뒤에는 제품 명령으로 벗어날 수 없는 `recovery_needed`가 전체 readiness를 `blocked`로 고정하고 uninstall도 막았다.
- 같은 작업에는 사용자 skill, 셸 실행, 025 plugin tool, 036 위임 작업이 더 적은 부담으로 있다.
- 구현은 별도 작업이다. 그 작업은 남은 앱 자료를 지우거나 고쳐 쓰지 않고, `apps` 명령을 읽기 전용 퇴역 안내로 바꾸며, readiness·Tasks·agent 실행 기록에서 앱 의존을 뺀다. 그때까지 현재 명령은 그대로 동작한다.

이 판정은 021·032의 과거 완료 판정을 다시 열지 않고 다른 판정 스펙의 실행을 기다리지 않는다.
