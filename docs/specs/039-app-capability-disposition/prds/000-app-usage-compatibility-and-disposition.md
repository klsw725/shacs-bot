# PRD 000. 앱 흐름 사용·호환성 조사와 판정

Status: Complete (Decision)

## Goal

개인용 앱의 authoring과 설치 후 lifecycle을 실제 사용자 경로·보존 자료·유지 비용으로 함께 평가해 **유지 / 범위 축소 / 제거** 중 하나의 근거 있는 결론을 기록한다. 이 PRD 하나로 [039 SPEC](../SPEC.md)의 판정을 닫으며, 결론을 구현하거나 사용자 자료를 삭제하지 않는다.

## Scope

1. 격리된 config/data dir에서 `apps init/propose/apply/install/list/inspect/enable/start/stop/restart/recover/disable/uninstall`을 단계별로 실행해 draft, bundle, registry, activation/supervisor receipt 변화를 분리해 기록한다.
2. 실제 CLI/API·readiness·Tasks·plugin/extension consumer와 테스트/문서, 사용자가 보유한 설치 앱/로컬 스크립트의 확인 범위·미확인 범위를 조사한다. 사용자 소유 실제 config/auth/data는 변경하지 않는다.
3. Authoring과 lifecycle 각각의 사용자 가치·설정/실패/복구 비용, skill/tool/workflow 대체 결과를 비교한다. 기존 데이터와 명령을 읽고 다룰 호환 계약을 유지/축소/제거 각 경우에 적는다.
4. 단일 결론을 근거·위험·보존 자료·대체 동작·후속 구현/데이터 이전 범위·필요 회귀 테스트와 함께 결정문에 기록한다. 불충분한 근거는 `미결정`으로 남긴다.

## Non Scope

- 앱 CLI/registry/supervisor 코드 제거, 실 사용자 자료 이전·삭제, 021/032 기존 완료 판정 변경.
- 실제 앱 호출이 검색되지 않았다는 이유만으로 미사용이라고 단정하거나 계획상 simplify를 retire 확정으로 격상하는 일.

## Parent Requirement Mapping

- [039 SPEC](../SPEC.md)의 조사할 현재 계약 1–4, 판정 규칙 전체, 변경 전 불변식 전체와 판정 수용 조건 1–3을 이 PRD 하나가 소유한다. 별도 뒤 번호 PRD를 필요로 하지 않는다.

## Acceptance Criteria

1. 격리 실행에서 초안 idempotency/conflict, apply와 install/start의 분리, disabled/missing credential 차단, stop/recover와 저장 receipt를 실제 표면으로 재현한다. 초기/종료 자료를 임의로 지우지 않는다.
2. Draft/bundle/registry/journal·readiness/Tasks 소비자의 경로·읽기·호환성 목록과 authoring/lifecycle별 가치·비용 비교를 제공한다. 저장소 밖 사용량을 알 수 없으면 `미확인`이라고 적는다.
3. 유지·축소·제거 중 선택한 하나의 결론에 대해 기존 사용자 동작과 보존 자료, 필요 호환/대체·회귀 테스트·후속 작업의 실행 경계를 명시한다. 근거가 부족하면 완료하지 않는다.

## Closure Evidence

- 격리된 CLI/API·readiness/Tasks 상태의 명령별 transcript 및 자료/receipt 변화표.
- 사용자·호출자 조사 범위, 대체 경로/비용 비교, 미확인 항목과 검토 가능한 판정 기록.

이 PRD는 **판정문 자체**가 완료물이다. 기능 변경이나 다른 판정 스펙의 완료에 의존하지 않는다.

## 조사 기록 (2026-10-24)

실행 증거는 로컬 `.omo/evidence/spec039/prd000/`(추적하지 않음)에 있다. 아래 `sNN`은 `qa2/transcript.md`의 단계이고, `qa3`·`qa4`는 보충 기록이다(`qa4`의 `tN`은 그 단계). 이 evidence가 없는 checkout에서는 실행 증명으로 보지 않는다.

- **실행 조건:** source `3d83bb0`. `cargo clean` 뒤 `cargo build --locked -p shacs-cli`로 만든 binary를 썼다. 모든 명령은 가짜 `HOME`과 격리 `--config`(data dir 분리)로 실행했다. data dir은 바로가기가 없는 실제 경로다. 백그라운드 owner는 job control로 띄워 터미널과 같은 SIGINT 처리를 받게 했다. 첫 시도(`qa1-invalid/`)는 QA 스크립트 결함으로 판정에서 뺐다(evidence README).
- **실제 사용자 자료:** 실제 `~/.shacs-bot`은 읽기만 했다(초안 하나의 sha256 계산과 격리 폴더로의 복사). 실행 전후로 실제 `apps/`가 없음을 확인했다.

### 명령별 결과와 바뀌는 사실

| 명령 | 바뀌는 사실 | 실제 표면 관측 |
|---|---|---|
| `apps init` | 초안 파일 4개 (`authoring/apps/draft-<id>/`) | 생성(s02). 같은 내용 재실행은 `already-exists-same-content`, 자료 변화 없음(s03). 후보를 고친 뒤 재실행은 충돌로 멈추고 덮어쓰지 않는다(s05). registry 변화 없음 |
| `apps propose` | 제안 기록 하나 (`authoring/flow/proposals/`) | `Applied: no`(s06). 같은 후보 재실행은 같은 제안 id, 자료 변화 없음(s07) |
| `apps apply` | bundle 복사, registry 등록(`installed`), checkpoint·receipt·recovery 기록 | process·journal·activation 없음(s09). 검토한 제안 id를 받지 않는다. 검토 뒤 후보를 고치자 새 제안을 기록하고 고친 후보를 적용했다(s06 대 s09). 재실행은 `already installed`(s10) |
| `apps apply --update` | bundle 교체, 이전 bundle snapshot, registry 갱신 | 새 설치처럼 초안 폴더를 넘기면 `authoring flow I/O failed: No such file or directory`(s44). `candidates/`를 넘겨야 성공한다(s45). 갱신 뒤 registry 상태가 `installed`로 돌아가 다시 enable해야 start된다(s46–s48) |
| `apps install` | registry 등록만. bundle은 이미 `<data>/apps/<id>.shacsapp/`에 있어야 한다 | 다른 위치는 거부(s12). apply 뒤 같은 bundle은 바뀌지 않는다(s11). data dir 경로에 바로가기가 있으면 apply가 저장한 경로와 install이 비교하는 실제 경로가 달라 `already registered from another bundle`로 실패한다(s73) |
| `apps list` / `inspect` | 없음 | registry와 마지막 journal receipt를 보여준다. 설치만 하고 실행 기록이 없는 앱은 `state=unavailable severity=error`(s13–s14)인데, 같은 상태에서 readiness `plugin_app`은 `ready`다 |
| `apps enable` / `disable` | registry 상태만 | s16, s52. disable은 starting·running·stopping·recovery_needed에서 거부된다(s33) |
| `apps start` | journal 요청·종료 receipt, 시도마다 execution snapshot(약 16 KB) | 차단돼도 snapshot이 남는다(s15, s17, s19). 차단 이유는 `app_not_enabled`(활성화 전 s15, 실행한 뒤 disable한 앱 qa4 t5–t7), `workspace_untrusted`와 `trusted_runtime_unavailable`, `credential_missing`, `activation_missing`이며 dispatch는 0이다. workspace trust는 config의 `plugins.trustedWorkspaces`를 직접 고쳐야 하고 CLI 명령이 없다(s18). 비밀을 환경 변수로 주면 `running`이고 자식은 별도 process group이다(s20). start 명령은 중단될 때까지 터미널을 차지한다 |
| `apps restart` / `stop` | journal 요청 | 다른 터미널의 요청을 foreground owner가 받아 새 generation·새 자식으로 재시작했고, 이어서 중단했다. 자식 없음, 마지막 receipt `Stopped`(s22–s24) |
| `apps recover` | journal 요청·완료 | 항상 `recovery_needed`로 끝난다(s31, s35, qa3). 아래 복구 흐름 참고 |
| `apps uninstall` | registry 항목 제거, 설치된 bundle 폴더 전체 삭제 | 사용자가 bundle 안에 둔 파일까지 지워졌다(s51, s53). 초안(4개), journal receipt(10개), authoring receipt, execution snapshot은 남지만 `inspect`는 `unknown app`이고 Tasks에도 행이 없다(s54). 같은 초안으로 다시 apply하면 이전 설치의 journal 상태가 새 설치에 그대로 보인다(s56) |

- **비밀:** 저장 자료 어디에도 비밀 값 원문이 없었고 receipt에는 `SPEC039_DEMO_TOKEN:resolved:environment`만 남았다(D 구간).
- **등록되지 않은 앱:** `apps start nope.app`은 `unknown app`으로 실패하면서 `apps/app-supervisor/nope.app/`에 lock·state·receipt 두 개를 만든다(s66). `stop`은 만들지 않는다(s67). [032 CLOSURE](../../032-app-maker-runtime-and-extension-lifecycle/CLOSURE.md)의 "rather than creating lifecycle truth" 서술과 다르지만 032의 판정은 다시 열지 않는다.
- **skill을 선언한 앱:** start가 매번 `activation_missing`으로 막힌다(s61–s62). activation record를 만드는 production 경로가 없다(`ActivationStore::put`의 production 호출자 없음). start 출력은 resource 경로를 소문자로 바꿔 `skill:skills/skill.md`로 보인다. 앱 skill은 `skills list`에 나오지 않는다. 같은 `SKILL.md`를 `<data>/skills/<name>/`에 복사하면 바로 `user-global` skill로 나온다(s63–s65).

### 실행 차단과 복구

1. 활성화 전·미신뢰·비밀 누락 start는 각 이유로 막히고 process를 만들지 않는다(s15–s19). 한 번 실행한 앱을 `apps disable`한 뒤의 start도 `app_not_enabled`로 막히고 dispatch 0과 차단 receipt를 남긴다(qa4 t4–t7). 비밀을 준 start와 다른 터미널의 restart·stop은 설계대로 동작한다(s20–s24).
2. owner가 SIGINT(터미널 Ctrl+C와 같은 신호)를 받으면 receipt 없이 바로 끝난다(exit 130). 앱 자식은 부모가 init(ppid 1)이 된 채 계속 실행되고 journal은 `running`으로 남는다. `inspect`·readiness·Tasks는 계속 실행 중·준비됨·stop 가능으로 보인다(s26–s28).
3. 그 뒤 `start`는 `Running`에서 거부된다(s29). `stop`은 처리할 owner가 없는 요청만 남기고 자식은 그대로다(s30). `recover`는 process를 확인하거나 정리하지 않고 `recovery_needed`를 기록한다(s31).
4. `recovery_needed`에서는 `start`·`disable`·`uninstall`·`apply --update`가 모두 거부되고 `recover`를 다시 해도 같다(s32–s36). Tasks의 app recover action도 실행은 되지만 행은 다시 `blocked`·recover 가능으로 남는다(s37). CLI `apps recover`, CLI Tasks, API Tasks, TUI의 네 호출자가 모두 `AppSupervisor::recover(app, false)`를 부르므로 `stopped`로 끝나는 production 경로가 없다. 정상 중단된 앱에 `apps recover`만 실행해도 같은 상태가 된다(qa3 1절).
5. readiness의 `plugin_app`은 필수(Required) 구성 요소다. 이 상태에서 `plugin_app`과 전체 readiness가 `blocked`다(CLI `runtime inspect`, API `/v1/readiness`; E·J 구간, qa3). uninstall도 거부되므로 제품 명령으로는 이 상태를 벗어날 수 없다. 남은 자식은 QA 스크립트가 process group에 신호를 보내 직접 정리했다.
6. `apply` 도중 실패를 되돌리는 `AuthoringFlowStore::recover`에는 CLI 호출자가 없고, CLI `apply`는 검증 결과를 항상 `Passed`로 넘긴다.

### 저장 자료와 읽는 곳

| 자료 | 위치 (`<data>` = config가 있는 폴더) | 쓰는 곳 | 읽는 곳 |
|---|---|---|---|
| 초안 | `authoring/apps/draft-<id>/` | `apps init`, 사용자 편집 | `init`(같은 내용인지 비교), `propose`·`apply`(후보) |
| authoring 기록 | `authoring/flow/{proposals,checkpoints,receipts,recovery,snapshots}/` | `propose`, `apply` | `apply` 내부의 digest 확인뿐이며 receipt·recovery를 보여주는 명령이 없다. 거부된 `apply --update`도 제안 파일을 다시 쓴다(s36) |
| registry | `apps/registry.json` | `apply`, `install`, `enable`, `disable`, `uninstall` | `apps` 명령, readiness, Tasks, start 판정, agent 실행 기록의 activation 조회 |
| 설치된 bundle | `apps/<id>.shacsapp/` | `apply`(복사), `--update`(교체), `uninstall`(삭제) | start(manifest·digest), activation 조회(경로 소속) |
| supervisor journal | `apps/app-supervisor/<id>/{state.json,lifecycle.lock,receipts/}` | `start`·`stop`·`restart`·`recover`, Tasks stop·recover | `list`·`inspect`, readiness, Tasks, disable·uninstall·update 판정 |
| 앱 execution snapshot | `snapshots/app-lifecycle/` | start 시도마다 | `inspect`는 참조만 표시. 파일은 `runtime snapshot inspect`(031)로 읽힌다 |
| activation record | `snapshots/activation-records.json` | production 작성자 없음 | start 판정(앱 skill), agent 실행 기록의 activation 조회 |
| 017 app ledger | `runtime/app-ledger/` | production 작성자 없음 | 없음 |

- **저장량:** 세 앱의 짧은 세션에서 `apps/` 57개 파일·212 KB(그중 journal 49개·180 KB), 초안 13개, authoring 기록 19개, 앱 execution snapshot 12개·192 KB가 생겼다(K 구간). journal은 정리되지 않고, 읽을 때마다 receipt 전체를 다시 읽는다.
- registry의 `processSnapshots`, `grantReference`, `unavailableReasons`와 `unavailable` 상태는 production이 비워 두거나 쓰지 않는다. readiness는 `unavailable`을 blocked로 읽는다.
- **과거 자료:** 실제 사용자 초안 `~/.shacs-bot/authoring/apps/draft-spec030.clock/`(7월 27일 생성)은 현재 코드로 같은 id를 새로 init한 결과와 네 파일의 sha256이 모두 같다. 고치지 않은 init 초안이다(I 구간). 7월 이후 `app_authoring.rs`의 변경은 직렬화되지 않는 `runtime: None` 한 줄이다(7885fc0). 이 초안의 복사본에서 현재 `init`은 `already-exists-same-content`, `propose`는 검증 통과였다(s81–s82).
- **config와 자료의 범위:** 다른 폴더의 config는 앱을 보지 않는다(s69). 같은 폴더의 다른 config 파일은 같은 registry와 journal을 공유한다(실제 사용자 폴더에도 config 파일이 셋 있다). trust가 없는 두 번째 config로 start하자 차단 receipt가 같은 journal에 기록돼 기본 config의 `inspect`에도 보였다(s74–s77).

### 소비자

| 소비자 | 읽거나 바꾸는 것 | 확인 방법 |
|---|---|---|
| CLI `apps` 하위 명령 13개 | 위 명령 표 | 실제 실행 |
| readiness `plugin_app` (CLI `runtime inspect`, API `/v1/readiness`, onboard wizard) | registry와 마지막 receipt. registry가 없으면 `unavailable`(severity error), 활성화 뒤 정상 중단이면 `degraded`, 차단 receipt나 `recovery_needed`면 `blocked`. 필수 구성 요소라 `blocked`는 전체 readiness를 막는다 | CLI·API 실제 실행(A·C·D·E·J 구간). wizard는 코드 |
| Tasks (CLI `tasks`, API `/v1/tasks`, TUI) | registry 앱의 마지막 receipt로 행을 만들고 stop·recover action을 journal에 쓴다. 공개 JSON에 `coverage.app`, owner `app`, app process 상태 어휘(`shacs-projection` spec035)가 있다 | CLI 조회·action과 API 조회를 실제 실행. TUI는 같은 함수 호출을 코드로 확인 |
| agent 실행 기록 | 모델 요청마다 resource 후보마다 config를 다시 읽고 registry에서 소속 앱을 찾는다(`spec031_activation_provider`). 새로 onboard한 workspace의 resource 후보는 78개였다(`runtime inspect`, qa4 1절). workspace의 skill 수에 따라 바뀌는 표본이다. 앱 bundle 안 resource에 손으로 쓴 activation record가 있을 때만 결과가 생긴다 | 코드 경로와 후보 수. 시간은 측정하지 않았다 |
| 실행 activation 판정 (`activation_execution`) | 앱 상태가 `enabled`일 때만 허용 | 코드 |
| permission 보호 대상 | agent 파일 도구의 `apps/registry` 쓰기를 `AppRegistry` 보호 대상으로 분류 | 코드 |
| release runner (`shacs-projection`) | spec030 target `target-app-registry`가 `shacs-app` 테스트 `spec030_app_registry_baseline`을, spec031 외부 owner `spec031-owner-spec032`가 `app_environment.rs`의 테스트를 이름으로 부른다 | 코드 |
| production 호출자 없음 | `resolve_app_extension_provenance`(032 PRD 002 projection), `ActivationStore::put`, `persist_ledger_entry`, `replace_process_snapshots`, `AuthoringFlowStore::recover` | 코드 검색 |
| 대화·위임 작업 | agent tool·skill·command·context 어디에도 앱을 만들거나 실행·사용하는 경로가 없다. 앱은 goal·위임 작업(036)·알림과 연결되지 않는다 | 코드 검색 |

- **확인한 사용 범위:** 이 기기의 실제 data dir에는 설치된 앱·registry·journal이 없고 고치지 않은 초안 하나만 있다. 기본 Docker Compose도 같은 `~/.shacs-bot`을 mount하므로 이 기기에서는 같은 범위다. `~/bin`은 없고 `~/.local/bin`과 저장소 `docs/scripts`에는 `apps` 호출이 없다.
- **미확인:** 다른 기기·다른 data dir·다른 사용자의 설치, shell history와 그 밖의 개인 스크립트. 이 범위의 사용량을 0으로 보지 않는다.

### 가치와 비용 비교

| | authoring (`init`·`propose`·`apply`·`install`) | lifecycle (`enable`부터 `uninstall`까지) |
|---|---|---|
| 실제로 주는 것 | 파일 2개 scaffold, 정적 검증, data dir 복사와 등록. 모델은 쓰이지 않는다(intent는 hash로만 남는다). 검토(`propose`)와 적용(`apply`)이 묶이지 않는다 | 선언한 argv를 foreground에서 실행. 활성화·trust·비밀·digest 차단과 receipt. 다른 터미널의 restart·stop |
| 첫 실행까지의 사용자 부담 | manifest JSON을 직접 쓴다. 갱신은 새 설치와 다른 경로 형식이다 | init, manifest 편집, apply, enable, config trust 직접 편집, 비밀 환경 변수, 전용 터미널에서 start, 중단할 때 두 번째 터미널 |
| 실패·복구 비용 | apply 중간 실패를 되돌리는 명령이 없고 오류는 원시 I/O 메시지다 | Ctrl+C 한 번에 고아 process와 사실과 다른 `running`. 이후 제품 명령으로 벗어날 수 없는 `recovery_needed`와 전체 readiness `blocked`. uninstall은 bundle 안 사용자 파일까지 지운다 |
| 상시 process | 해당 없음 | 앱이 동작하는 동안 owner 터미널이 열려 있어야 한다. 백그라운드 owner가 없고, 활성화한 앱을 정상 중단해 두면 readiness `plugin_app`이 `degraded`다(s25 뒤 관찰, qa3) |
| 같은 결과의 대체 경로 | 절차·지식 전달: `<data>/skills/<name>/SKILL.md` 하나로 바로 발견된다(s64–s65). 도구 노출: 025 plugin command tool과 MCP 설정(기존 owner, 이번에 실행하지 않음) | 명령 실행: 셸에서 바로 실행하고 Ctrl+C 한 번으로 자식까지 끝난다(qa3 2절). 반복 관찰: 036 위임 작업. 대화 중 실행: 004 `exec`와 승인 경계 |
| 사용 근거 | 고치지 않은 초안 1개, 외부 미확인 | 설치 0개, 외부 미확인 |
| 유지 비용 (둘을 합쳐 셈) | 앱 전용 source 파일 약 3,200줄, `shacs-cli/src/lib.rs` 안 앱 코드 약 1,300줄과 inline 테스트 약 480줄, 앱 전용 테스트 파일 약 2,000줄. `tests/` 바로 아래 앱 전용 통합 테스트 실행 파일 8개 | readiness, Tasks(CLI/API/TUI)와 공개 projection 어휘, 실행 activation 판정, agent 모델 요청 경로, release runner 두 곳과 결합 |

기능 가치와 별도로 판단한 보호 경계: `apply`·`install`이 process·grant·activation을 만들지 않는 것과 비밀 값을 저장하지 않는 것은 앱 명령이 있는 동안의 성질이다. agent 파일 도구의 `apps/registry` 보호는 자료가 남아 있는 한 계속 필요하다.

### 세 선택지의 호환 계약

- **유지:** 사용자 수요가 확인되지 않았다. 유지하려면 적어도 다음을 먼저 고쳐야 한다. 정리를 확인하고 `stopped`로 끝나는 recover, owner가 끝날 때의 자식 정리와 사실과 다른 `running`, 새 설치와 같은 갱신 경로와 갱신 뒤 활성화 유지, uninstall의 사용자 파일 삭제, 등록되지 않은 앱의 journal 생성, 앱을 쓰지 않는 사용자의 readiness 의미. 기존 명령과 위 저장 자료 형식은 그대로 읽어야 한다.
- **범위 축소:** (a) authoring만 남기면 bundle을 쓰는 곳이 없다. (b) install·registry·inspect만 남기면 registry를 쓰는 곳이 없다. (c) lifecycle만 남기면 유지와 같은 수정이 필요하고 어시스턴트 쪽 소비자가 없다. 관측된 가치가 남는 단계가 없다. 축소한다면 남는 명령은 위 저장 자료를 그대로 읽고, 빠지는 명령은 아래 제거와 같은 안내를 내야 한다.
- **제거:** 아래 판정과 계약이다.

## 판정 (2026-10-24): 제거

앱 authoring과 lifecycle 전체를 제품 표면에서 제거한다. 근거는 다음과 같다.

1. 어시스턴트 경험과 연결되지 않는다. 대화·위임 작업·알림·agent tool/skill/context 어디에서도 앱을 만들거나 쓰지 않는다. 앱 skill은 발견되지 않고, skill을 선언한 앱은 시작할 수도 없다.
2. 사용 근거가 없다. 이 기기의 실제 자료에는 설치가 없고 고치지 않은 초안 하나만 있다. 외부 사용은 미확인이며 아래의 비파괴 계약으로 그 위험을 다룬다.
3. lifecycle의 실패·복구가 실제 표면에서 성립하지 않는다. Ctrl+C 한 번으로 고아 process와 사실과 다른 `running`이 생긴다. 그 뒤나 정상 중단된 앱에 `recover`를 실행한 뒤에는 제품 명령으로 벗어날 수 없는 `recovery_needed`가 전체 readiness를 `blocked`로 고정하고 uninstall도 막는다.
4. 같은 작업에 부담이 훨씬 적은 기존 경로(사용자 skill, 셸 실행, 025 plugin tool, 036 위임 작업)가 있다.
5. 앱을 쓰지 않는 사용자에게도 비용이 있다. 필수 readiness 구성 요소가 `unavailable`(severity error)로 보이고, agent의 모든 모델 요청이 resource 후보마다 config를 다시 읽고 registry를 조회한다.

이 판정은 코드 변경이나 사용자 자료 삭제가 아니며 데이터 삭제 허가도 아니다. 아래 계약은 별도 구현 작업이 지킬 경계다.

### 보존할 사용자 자료와 동작

- 다음 자료를 지우거나 고쳐 쓰지 않는다: `<data>/apps/`(registry, bundle, supervisor journal), `authoring/apps/`, `authoring/flow/`, `snapshots/app-lifecycle/`, `snapshots/activation-records.json`, `runtime/app-ledger/`. 이전 작업은 필요 없다. 자료가 문서화한 위치의 평범한 JSON과 bundle 파일로 남는 것을 명시적 보존이자 내보내기로 삼는다. 지우기는 사용자가 직접 한다.
- `shacs-bot apps …`(별칭 `app` 포함)는 알 수 없는 명령 오류 대신 읽기 전용 퇴역 안내를 낸다. 안내에는 기능 제거 사실과, config의 data dir에서 찾은 남은 앱 자료 위치가 들어간다. journal이 `starting`·`running`·`stopping`·`recovery_needed`인 앱은 process가 아직 실행 중일 수 있고 제품이 중단을 확인할 수 없다고 밝히며 `stopped`로 표시하지 않는다. 종료 코드는 0이 아니고 파일을 만들거나 바꾸지 않는다.
- 남은 앱 process를 실행하거나 멈추지 않는다.
- agent 파일 도구의 `apps/registry` 보호 분류는 유지한다.

### 바뀌는 표면

- **CLI:** `apps` 하위 명령 13개의 동작을 위 안내로 바꾼다.
- **readiness:** `plugin_app`은 plugin 상태만 반영한다. registry가 없거나 앱 자료가 남아 있어도 앱 때문에 `unavailable`·`degraded`·`blocked`가 되지 않는다. 공개 구성 요소 이름 `plugin_app`은 readiness schema를 바꾸지 않는 한 유지한다.
- **Tasks (CLI/API/TUI):** 앱 행을 만들지 않는다. Tasks `schema_version` 1을 유지하는 동안 `coverage.app` 키를 남기고 `unavailable`로 보고하며, 이 값이 앱 owner 퇴역을 뜻한다고 문서에 적는다. 키를 없애려면 schema version을 올리고 API·TUI 소비자를 함께 바꾼다. owner `app`의 action 요청은 기존 `unsupported tasks owner/action pair`로 거부하고 파일을 바꾸지 않는다.
- **agent 실행 기록:** 앱 소속 activation 조회(`spec031_activation_provider`)를 없앤다. execution snapshot의 resource `activation_ref`는 지금처럼 비어 있다. 031 activation record 형식과 `runtime activation inspect`는 031 owner 범위로 그대로 읽힌다. `activation_execution`에서 앱 상태 입력을 빼는 방식은 030/031 owner가 다른 resource의 의미를 바꾸지 않는 범위에서 정한다.
- **release runner:** 제거 시점에 runner가 남아 있으면 spec030 `target-app-registry`와 spec031 `spec031-owner-spec032`를 같은 변경에서 고쳐, 없는 테스트 때문에 blocked가 되지 않게 한다. 041의 runner 판정은 039 종료의 조건이 아니다.
- **문서:** `docs/USAGE.md`의 앱 절과 Tasks 절의 app stop·recover, README의 Tasks owner 목록, `docs/SYSTEM-FOUNDATION.md`의 app bundle 배치, `docs/PRODUCT-PATH-MAP.md`, 스펙 색인. 017·021·032는 과거 완료 판정을 그대로 두고 퇴역 안내만 더한다.

### 구현 때 필요한 회귀 테스트

1. 남은 자료 fixture(활성화된 앱의 registry, 사용자 파일이 든 bundle, `recovery_needed` journal, 초안, authoring 기록, snapshot)에서 `apps list`·`apps start <id>`·`apps uninstall <id>`·`runtime inspect`·`tasks --json`을 실행해도 모든 파일의 sha256이 그대로이고, `apps`는 0이 아닌 종료 코드를 낸다.
2. 같은 fixture에서 `apps` 안내가 남은 자료 위치를 나열하고 `recovery_needed` 앱을 `stopped`로 표시하지 않는다.
3. 앱 자료가 없고 빈 plugins 폴더가 있으면 `plugin_app`이 앱 때문에 `unavailable`이 되지 않는다. fixture가 있어도 전체 readiness가 앱 때문에 `blocked`가 되지 않는다.
4. Tasks에 앱 행이 없고 `coverage.app`이 위 규칙을 따른다. owner `app` action은 `unsupported`이고 파일을 바꾸지 않는다.
5. 지금의 앱 동작을 고정하는 기존 테스트는 같은 변경에서 지우거나 바꾼다: `shacs-app/tests/`의 5개 파일. `shacs-core/tests/`의 `app_compat.rs`·`spec032_app_supervisor.rs`·`spec032_app_extension_provenance.rs`·`spec031_activation_execution.rs`와 `it/`의 `spec035_tasks_app_path.rs`·`spec031_activation_live_snapshot.rs`, `spec035_tasks_actions.rs`의 앱 부분. `shacs-cli`의 inline 앱 테스트와 `it/`의 `spec035_tasks_app_path.rs`, `spec031_owner_remediation*`·`readiness_owner_support`·`spec031_activation_production_builder`의 앱 부분. `shacs-api/tests/it/spec035_tasks_api.rs`·`shacs-projection/tests/it/spec035_tasks_model.rs`·`shacs-tui/tests/it/spec035_tasks_view.rs`의 앱 부분과 `shacs-projection/tests/spec031_release_runner_exploits.rs`.

### 후속 구현 범위

- 별도 구현 스펙 [043](../../043-app-capability-retirement/SPEC.md)이 위 계약 전체를 소유한다. 자료 이전은 없다. 안내·readiness·Tasks·agent 실행 기록·release runner·문서를 한 변경 단위로 바꾸고 `shacs-app` crate와 앱 전용 코드를 지운다. 이 구현은 다른 신규 기능이나 040·041의 판정을 기다리지 않는다.
- 구현 전까지 현재 명령은 그대로 동작한다. 이번 작업에서 [사용 가이드의 앱 절](../../../USAGE.md#앱)에 판정과 확인된 한계를 적었다.

### 남은 위험과 한계

- 이 기기 밖의 사용은 미확인이다. 자료는 그대로 두고 명령은 0이 아닌 종료 코드와 함께 위치를 안내하므로, 자료를 조용히 잃거나 잘못 읽지 않는다.
- 업그레이드 시점에 고아 앱 process가 실행 중인 사용자가 있을 수 있다. 제품은 이를 확인할 수 없고, 안내는 확인할 수 없다는 사실만 말한다.
- `shacs-bot apps`를 부르는 개인 스크립트는 0이 아닌 종료 코드로 실패한다. 조용히 다른 동작을 하지 않는다.
- agent 요청 경로의 비용은 후보 수와 코드 경로만 확인했고 시간은 측정하지 않았다. TUI와 API의 Tasks action, onboard wizard는 실행하지 않고 코드로 확인했다.

## 종료 확인 (2026-10-24)

- 수용 조건 1: init 멱등성(s03)과 충돌(s05), propose·apply·install·start의 분리(s06–s15), 활성화 전(s15)·disable한 앱(qa4 t5–t7)·미신뢰(s17)·비밀 누락(s19)·activation 누락(s61) 차단, restart·stop(s22–s24), owner 비정상 종료와 recover(s26–s37, qa3), 저장 receipt와 자료 변화를 격리 실제 표면에서 재현했다. 실험 자료는 지우지 않았다. 남은 자식 process만 직접 정리했고 그 사실을 기록했다.
- 수용 조건 2: 저장 자료·소비자 표와 authoring·lifecycle별 가치·비용 비교를 기록했다. 이 기기 밖의 사용량은 미확인으로 적었다.
- 수용 조건 3: 결론(제거)과 보존할 자료·동작, 바뀌는 표면, 회귀 테스트, 후속 구현 범위를 기록했다.
- 문서만 바꿨으므로 Cargo 검증은 생략했다(AGENTS.md). 실행에 쓴 binary는 `cargo clean` 뒤 `cargo build --locked -p shacs-cli`로 만들었다.
- 독립 검토(읽기 전용 Oracle): **PASS** (2차). 기록은 `.omo/evidence/spec039/prd000/review/gates.md`에 있다.
  - 1차는 BLOCKED였다. 명시적으로 disable한 앱의 start 차단을 재현하지 않았다는 지적이었다. qa4로 재현해 반영했다. 함께 받은 비차단 지적에 따라 resource 후보 수를 새 onboard 기준 78개로 고치고 원본 출력을 남겼으며, USAGE의 상태 이름과 Unix 한정 표현을 바로잡았다.
  - 2차는 수정 사항과 문서 간 일관성을 다시 확인했고 차단·비차단 지적이 없었다.
