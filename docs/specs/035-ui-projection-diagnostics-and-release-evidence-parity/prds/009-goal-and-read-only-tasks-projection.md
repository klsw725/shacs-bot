# PRD 009. goal and read-only Tasks projection

Status: Open

구현 상태: 구현됨, closure 차단. Core assembler가 goal/child/workflow/automation/app/recovery owner를 `Spec035TasksProjection`으로 모으고 CLI `tasks --json`, API `GET /v1/tasks`, TUI가 이를 소비한다. Locator·freshness·goal accounting과 제한된 owner action 전달이 구현되어 있다. Task4-6 및 todo10의 goal parity/locator/mixed-state/read audit 기록은 존재하지만 현재 source-bound 최종 PASS는 아니다. [현재 차단 기록](../CLOSURE.md)을 따른다.

## Goal

Spec 033 goal accounting과 기존 child/workflow/recovery owner record를 별도 truth store 없이 하나의 read-only Tasks view로 투영한다.

## Scope

1. Goal id, state, stop reason, continuation budget, usage summary의 cross-surface parity.
2. Child, workflow, automation, app task, recovery row의 owner-backed aggregation.
3. Stale, blocked, running, recovered, done 상태와 safe next action 표시.
4. CLI, local API, TUI에서 같은 canonical fields를 소비하는 adapter.

## Non Scope

1. Resident-agent identity, durable family messaging, 별도 task database를 도입하지 않는다.
2. Projection이 goal, child, workflow, app, recovery state를 변경하지 않는다.
3. Owner locator 없는 synthetic task row를 만들지 않는다.

## Required Contract

1. 모든 row는 owner kind, opaque owner locator, observed-at/freshness, bounded state를 가진다.
2. Mutation action은 기존 command owner로 route하고 projection은 requested/completed를 구분한다.
3. Missing owner evidence는 unavailable/unknown이며 empty success로 표시하지 않는다.
4. Goal accounting은 Spec 033의 domain fact를 그대로 보존하고 Prime goal store를 새 authority로 만들지 않는다.

## Acceptance Criteria

1. CLI, local API, TUI가 같은 goal id, stop reason, continuation budget을 표시한다.
2. Child/workflow/automation/app/recovery mixed fixture가 owner locator와 freshness를 보존한다.
3. Stale/blocked/recovered rows와 next action이 owner evidence 없이 생성되지 않는다.
4. Tasks view에서 요청한 action은 owner command 결과 전까지 completed로 표시되지 않는다.

## Closure Evidence

1. Goal accounting parity matrix.
2. Owner locator coverage audit.
3. Mixed-state Tasks view fixture와 terminal TUI/API/CLI transcripts.
4. 별도 truth store와 mutation authority가 없음을 확인하는 architecture read audit.

## 현재 구현과 검증 경계

1. Mixed Tasks는 현재 구현이며 별도 task truth를 만들지 않는다. Durable replay에 child state가 없으면 coverage는 unavailable로 남고 정상 replay의 빈 집합만 available(0)이 되도록 보정됐다. F2가 두 경로를 재검토해 확인했으며 recovery 진단은 독립적으로 보존된다. 이 확인은 전체 owner 계약 충족 선언이 아니다.
2. CLI 기본 session은 `cli:direct`, API는 `api:default`이므로 parity 조회에는 같은 session을 명시한다. API는 `/v1/tasks?session_id=cli%3Adirect`처럼 인코딩한다.
3. Goal pause/resume, app stop/recover, runtime recover만 현재 광고된 action과 locator를 검증한 뒤 기존 owner로 전달한다. Child/workflow/automation mutation은 지원하지 않는다.
4. Mutation의 transport hello는 호환성 전제이지 권한이 아니다. Requested/completed는 owner 결과로 구분하며 [사용법](../../../USAGE.md#tasks-조회와-owner-변경-요청)과 기존 권한 경계를 따른다.
5. 원래 네 F2 결함은 제한된 재검토 범위에서 confirmed다. 최종 compiled QA에서 CLI/API/TUI Tasks와 보정된 encoded WebSocket 재접속을 확인했지만 전체 closure 차단은 해소되지 않았다. 과거 PASS 목록을 현재 소스로 재결속하지 않는다. 여섯 owner 종류의 mixed fixture와 실제 goal/recovery 표면 기록도 구분하며 여섯 live owner를 모두 검증했다고 주장하지 않는다. 최신 실행 근거와 남은 차단은 [CLOSURE](../CLOSURE.md)를 따른다.
