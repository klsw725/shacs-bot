# PRD 000. 공용 표면에서 앱 자료 분리

Status: Planned

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
