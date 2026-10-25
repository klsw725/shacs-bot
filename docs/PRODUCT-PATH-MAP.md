# 개인 어시스턴트 첫 경험의 코드 경로 지도

이 문서는 [`intent.md`](../intent.md)의 첫 경험을 구현하기 전에 조사한 **재사용할 경로, 연결할 경계, 당장 손대지 않을 기능**의 기준선이다. 아래 표의 "필요한 연결"은 당시 gap을 설명하며 현재 구현 상태가 아니다. 첫 수직 흐름의 scoped 완료와 비보장 범위는 [Spec 036 종료 증거](specs/036-entrusted-work-continuity-and-change-notification/SPEC.md#종료-증거-2026-10-19)를 따른다. Spec035 문서 정리와 release runner 계약 변경은 별도 작업으로 취급하고, 그 검증 기준을 새 기능 삭제와 섞지 않는다.

## 기준 장면

사용자가 “여행 날짜와 예산을 기억하고 선택지 변화를 살펴봐 줘”라고 맡긴다. 어시스턴트는 조건을 이어받고, 변화가 없으면 조용히 있으며, 예산·선택·결정 시점에 영향을 주는 변화만 근거와 함께 알려준다. 사용자가 예산을 바꾸거나 일을 중단하면 다음 확인에 반영하고, 예약은 사용자에게 묻는다. 여행 전용 기능이 아니라 지속 작업의 공통 동작을 확인하는 장면이다.

## 현재 경로와 끊긴 연결

| 단계 | 현재 재사용할 경로 | 첫 경험에 필요한 연결 |
|---|---|---|
| 사용자 위임과 기억 | `crates/shacs-cli/src/spec033_cli.rs`, `crates/shacs-core/src/runtime/goal_surface.rs:83`의 `goal set/pause/resume/done/clear`와 session metadata 저장 | 일회성 질문과 명시적 위임을 구별하고, 애매하면 물어 동의를 기록한다. 조건·이전 결정·남은 질문을 수정 가능한 진행 중인 일에 연결한다. 현재 goal은 session 단위이며 작업별 관찰 조건 자체는 아니다. |
| 주기적 확인 | `crates/shacs-cli/src/lib.rs:8942`, `crates/shacs-cli/src/automation_producer.rs:107`의 heartbeat·cron 및 `crates/shacs-core/src/runtime/automation_dispatch.rs:59`의 durable work | wake를 실제 활성 위임과 연결하고 중단·완료 뒤 실행을 막는다. 현재 producer는 `goal_id: None`, `active_goal: true`를 채우므로 활성 goal을 확인했다는 증거가 아니다 (`automation_producer.rs:137`). 새 scheduler를 만드는 것이 아니라 생산·소비 경계에 위임 상태를 연결한다. |
| 실행과 결과 | `crates/shacs-cli/src/automation_worker.rs:173`, `crates/shacs-cli/src/automation_outcome.rs:32`에서 work 실행 후 결과와 session goal 상태를 읽음 | 같은 일의 이전 관측값과 새 결과를 비교하고, 어떤 변화가 사용자 선택에 영향을 주는지 근거를 남긴다. 실행 성공을 곧 중요한 변화나 사용자 통지로 취급하지 않는다. |
| 조용히 있거나 연락 | `crates/shacs-cli/src/automation_outcome/durable_owners.rs:30`의 Notify·Suppress와 inbound queue, `crates/shacs-cli/src/lib.rs:13642`의 channel outbound | 현재 cron은 source 종류만으로 Notify를 선택하고 (`automation_producer.rs:179`), 후속 메시지 내용은 고정 instruction이다 (`durable_owners.rs:46`). 동일 관측은 억제하고, 중요한 변화에만 조건·이전값·새값·근거를 포함해 알린다. work 성공과 실제 사용자 수신은 별도 사실로 남긴다. |

이 조사 당시 durable queue, channel sender, goal snapshot, workflow executor는 **기반**이었다. 각 부품만으로 위 장면이 완성된 것은 아니며, 이들을 사용자 위임·관측·변화 판정·통지에 연결한 현재 범위는 Spec 036에 분리해 기록한다.

## 첫 수직 흐름의 통과 조건

1. 일회성 질문은 지속 감시를 만들지 않는다. 명시적 위임 또는 제안에 대한 동의만 진행 중인 일을 만든다.
2. 재시작 뒤에도 목표·조건·마지막 관측·다음 확인이 복원되고, 조건 수정은 같은 일의 다음 확인에 반영된다. 다른 대화·채널로 이어받는 것은 별도의 identity/연결 검증 없이는 완료로 주장하지 않는다.
3. pause·done·clear 뒤에는 예약된 확인이 있더라도 새 관찰과 통지가 발생하지 않는다.
4. 같은 결과가 반복되면 `Suppress`이고 사용자 메시지가 없다. 의미 있는 변화가 생기면 차이와 근거를 담은 `Notify`를 한 번 전달한다. 정기 요약은 사용자가 따로 요청한 경우에만 보낸다.
5. 전달 실패를 작업 성공으로 숨기지 않고, 예약·구매·발송 같은 확정 행동 전에 사용자 판단을 받는다.

CLI/API goal lifecycle과 session 저장 테스트(`crates/shacs-cli/tests/spec033_snapshot_cli.rs`, `crates/shacs-api/tests/spec033_snapshot_api.rs`)는 당시 출발점이었다. 위임·goal-bound wake·무변화 억제·중단 후 확인·실제 outbound의 현재 검증 근거는 Spec 036의 네 PRD와 최종 종료 증거에 기록한다. 기존 033 테스트만으로 이 흐름이 보장됐다는 뜻은 아니다.

## 기능별 처리 판단

| 판단 | 기능과 이유 | 지금 할 일 |
|---|---|---|
| **유지·연결** | session/goal 저장, cron·heartbeat, durable queue, automation outcome, channel 전달은 위 수직 흐름에 필요한 기반이다. 도구 승인·사용자 질문 경계는 실행 안전성과 사용자 개입을 지탱한다. | 위임과 조건·관측값이 각 owner를 통과하도록 연결한다. 도구별 permission approval과 목표 수준의 사용자 결정은 혼동하지 않는다. |
| **유지·후속 개선** | tool/skill discovery와 workflow recipe, 이미지 생성, 일반 파일 작성·전달은 다양한 일을 처리할 수 있게 한다. 그러나 작업별 하네스 자동 선택·사용자 변경, 계산 검증, 보고서 출처 확인이 이미 완성된 것은 아니다. | 첫 수직 흐름을 닫은 뒤 실제 작업 사례에서 전문 절차가 필요한 곳만 확장한다. |
| **후순위** | 앱 authoring/lifecycle(아래와 같이 043에서 제거), artifact self-improvement(`crates/shacs-cli/src/improvement_cli.rs:18`), 선택적 video analyzer(아래와 같이 044에서 제거)는 현재 중심 경험보다 전문 범위가 넓다. | 기능 확장을 미루되 설치된 앱, 저장 데이터, CLI/API 호출자의 호환성은 보존한다. 방법 학습은 artifact rollback과 구분해 나중에 검증한다. |
| **제거 여부 조사** | `spec031-release-runner` 같은 release evidence 도구는 사용자 어시스턴트의 직접 기능은 아니지만 검증·배포 계약과 테스트가 의존한다. 다른 후보들도 생산 호출자와 테스트가 존재해 dead code라고 볼 수 없다. | 실제 호출·CI·저장 데이터 의존성과 제거 후 대체 검증을 먼저 조사한다. 제품 우선순위가 낮다는 이유만으로 코드를 삭제하지 않는다. |

**첫 수직 흐름은 Spec 036의 현재 self-hosted 범위에서 완료했다.** 기능 삭제 여부는 이 완료를 근거로 자동 결정하지 않으며, 후순위 기능은 각 결정 스펙에서 따로 판정한다.

이 첫 흐름의 독립 계약은 [`036`](specs/036-entrusted-work-continuity-and-change-notification/SPEC.md)에, 단일 요청의 작업별 선택·검증은 [`037`](specs/037-task-specific-assistant-methods/SPEC.md)에, 배운 방법의 검토·재사용은 [`038`](specs/038-evidence-backed-method-learning/SPEC.md)에 정의한다. 셋은 각각의 수용 조건으로 종료되며 다른 신규 스펙의 완료를 전제로 삼지 않는다. 첫 흐름을 대화형 위임·웹페이지 관찰과 확인 실패·결정 마감 알림으로 넓히는 계약은 [`042`](specs/042-conversational-web-watch-and-trustworthy-silence/SPEC.md)에 정의한다.

후순위 기능의 실제 축소 여부는 미리 정하지 않는다. 앱 흐름은 [`039`](specs/039-app-capability-disposition/SPEC.md), 비디오 분석은 [`040`](specs/040-video-analyzer-capability-disposition/SPEC.md), Spec031 release tooling은 [`041`](specs/041-release-evidence-tooling-disposition/SPEC.md)에서 각각 사용자 경로·저장 자료·대체 검증을 조사해 유지/축소/제거를 판정한다. 이 결정 스펙들의 완료는 코드 삭제를 뜻하지 않는다.

앱 흐름은 039에서 **제거**로 판정했다([판정 결과](specs/039-app-capability-disposition/SPEC.md#판정-결과-2026-10-24)). 위 표의 호환 보존 원칙대로 남은 앱 자료는 지우지 않고 `apps` 명령은 읽기 전용 퇴역 안내로 바꿨으며, readiness·Tasks·agent 실행 기록에서 앱 의존을 빼고 앱 코드를 지우는 구현은 [`043`](specs/043-app-capability-retirement/SPEC.md)에서 했다.

비디오 분석은 040에서 **제거**로 판정했다([판정 결과](specs/040-video-analyzer-capability-disposition/SPEC.md#판정-결과-2026-10-24)). 위 표의 호환 보존 원칙대로 저장된 영상 첨부와 남은 분석 기록은 지우지 않고 영상 첨부는 계속 unsupported note로 모델에 알리며, analyzer와 분석 기록 표면은 [`044`](specs/044-video-analyzer-retirement/SPEC.md)에서 뺐다.
