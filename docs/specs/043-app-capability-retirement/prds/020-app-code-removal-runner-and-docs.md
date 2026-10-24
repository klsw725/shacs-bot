# PRD 020. 앱 코드 삭제와 release runner·문서 정리

Status: Planned

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
   - spec031 runner의 새 run은 spec032 외부 owner fact를 요구하거나 만들지 않고 `spec031-owner-spec032` 명령을 실행하지 않는다. run 결과의 owner 구성이 바뀌므로 새 run의 schema version을 올린다(지금 `spec031.release_runner.v3`). 다른 owner의 fact와 판정은 바꾸지 않는다.
   - 보존된 runner evidence의 판독은 지금과 같은 결과를 낸다. v3와 v2는 spec032를 포함한 과거 owner 구성 그대로 판독하고, v3 판독 회귀 테스트를 더한다. 특히 추적되는 v2 G7 archive와 그 검증 테스트는 지금처럼 통과해야 한다. 지운 앱 파일을 근거로 삼던 과거 판독은 035 퇴역 때처럼 보관 자료로 확인하고, 확인 자체를 없애 통과시키지 않는다. 판독은 기록된 결과만 쓰고 지운 Cargo test를 다시 실행하지 않는다.
   - 이미 만든 과거 evidence 파일은 고치지 않는다.
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
2. runner가 남아 있으면 spec030·spec031 runner의 catalog와 명령 목록에 지운 테스트가 없고 runner 테스트가 통과한다. spec030 PRD 005에는 남은 target이 있다. spec031 runner의 새 run은 올린 schema version으로 spec032 adapter fact를 요구하지 않는다. spec032를 포함한 v3 결과의 판독 회귀 테스트와 보존된 v2 G7 archive의 검증 테스트는 지금과 같은 결과로 통과한다.
3. 최종 확인 기록에서 [043 SPEC](../SPEC.md) 수용 조건 1–4가 함께 성립한다. QA 전용 자식 process는 처음과 같은 identity로 정지 없이 실행 중이었고 신호 기록이 비어 있었다.
4. 문서 검색에서 앱을 현재 기능으로 안내하는 문장이 없다(역사 기록과 퇴역 안내 제외). 017·021·032의 `Status:` 줄은 그대로이고 퇴역 안내가 있다.

## Closure Evidence

1. 삭제 범위 검색 결과와 Cargo 검증 기록.
2. runner catalog·명령·테스트 변경과 결과.
3. 043 이전 binary 자료로 한 최종 실제 표면 QA 기록(자료 트리 비교, 남은 process 확인과 정리).
4. 고친 문서.

PRD 020은 앞 PRD의 닫힌 결과를 소비해 코드 삭제와 공통 정리로 043을 닫는다. 041의 판정이나 다른 신규 기능을 기다리지 않는다.
