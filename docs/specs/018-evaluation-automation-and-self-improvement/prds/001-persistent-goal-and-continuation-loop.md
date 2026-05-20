# PRD 001. persistent goal and continuation loop

## 목표

이 문서는 persistent goal lifecycle과 `/goal` 계열 사용자 경험 의미를 완전 구현하기 위한 실행 기준이다. 사용자가 설정한 목표가 여러 turn, scheduled wake, channel event, subagent result, app task result를 지나도 같은 목표로 추적되고, completion evaluator가 `done`, `continue`, `blocked`를 일관되게 제안하게 한다.

## SPEC 입력

- 주관 spec: `docs/specs/018-evaluation-automation-and-self-improvement/SPEC.md`
- 선행 PRD: `docs/specs/018-evaluation-automation-and-self-improvement/prds/000-evaluator-foundation-and-ledger.md`
- 교차 의존:
  - `docs/specs/001-session-kernel/SPEC.md`
  - `docs/specs/006-session-store/SPEC.md`
  - `docs/specs/007-main-orchestrator-policy/SPEC.md`
  - `docs/specs/012-runtime-services/SPEC.md`
  - `docs/specs/013-user-interfaces-and-session-ux/SPEC.md`

## Dependency Cut

- 000의 evaluator envelope, frozen snapshot, ledger split을 반드시 사용한다.
- 001은 goal lifecycle 통합 의미를 소유한다. command parsing, session event persistence, CLI/TUI rendering은 각 owner spec이 소유한다.
- 007은 user interruption priority와 active turn policy를 소유한다. 이 PRD는 goal loop가 그 policy를 소비해야 한다고 요구한다.
- 012는 wake와 background reentry primitive를 소유한다. 이 PRD는 persistent goal이 wake를 어떻게 해석하는지만 정의한다.

## 범위

- persistent goal 생성, 조회, pause, resume, clear, done, blocked lifecycle
- `/goal`처럼 동작하는 UX semantics와 projection 요구사항
- completion evaluator의 `done`, `continue`, `blocked` verdict 소비
- turn budget과 continuation loop 중단 조건
- 사용자 새 입력, interrupt, stop, clear의 우선순위
- goal 상태와 evaluation ledger 연결

## 범위 제외

- 자연어 command parser 세부 구현
- TUI widget layout
- 원격 협업 목표 관리
- 조직 관리자 승인 흐름
- 무제한 autonomous agent loop
- goal marketplace 또는 template catalog

## 구현 요구사항

- goal은 `active`, `paused`, `blocked`, `done`, `cleared` 상태를 가져야 한다.
- 사용자가 goal을 설정하면 session truth에 goal record가 생기고, 이후 evaluator는 이 record의 frozen snapshot만 입력으로 받는다.
- `/goal status`, `/goal pause`, `/goal resume`, `/goal clear`와 같은 UX는 동일한 상태 전이 의미를 가져야 한다. 실제 command 이름은 013의 surface가 정한다.
- completion evaluator는 `done`, `continue`, `blocked`만 goal completion verdict로 반환해야 한다.
- `done`은 goal 종료 제안일 뿐이며, orchestrator가 상태를 `done`으로 반영해야 종료된다.
- `continue`는 turn budget, permission, active cancellation, user interruption gate를 통과해야 다음 turn을 만든다.
- `blocked`는 필요한 사용자 입력, secret, permission, 외부 시스템, 실패 복구 중 하나 이상의 reason을 포함해야 한다.
- turn budget은 goal별, wake별, manual continuation별로 추적되어야 하며, 예산 초과 시 `blocked` 또는 user visible pause로 접어야 한다.
- 사용자가 새 메시지를 보내거나 stop, pause, clear를 요청하면 goal continuation보다 우선한다.
- clear는 history 삭제가 아니라 active goal을 더 이상 continuation 대상으로 보지 않는 상태 전이다.

## 데이터/상태 모델

- `PersistentGoal`: goal id, session id, status, title, original user intent ref, created at, updated at, turn budget, active continuation id.
- `GoalContinuationState`: continuation id, goal id, source, budget spent, last evaluator verdict id, last action hint, interruption flag.
- `CompletionVerdict`: kind, reason, confidence, evidence refs, next action hint, budget recommendation.
- `GoalUserAction`: set, pause, resume, clear, mark done, request continue.
- goal 상태 변경은 session event로 남고, evaluator verdict는 evaluation ledger에 남는다.

## 정상 시퀀스

1. 사용자가 지속 목표를 설정한다.
2. orchestrator가 goal record를 `active`로 기록한다.
3. turn이 끝날 때 completion evaluator용 frozen snapshot을 만든다.
4. evaluator가 `continue`를 반환한다.
5. orchestrator가 turn budget과 interruption gate를 확인한다.
6. 다음 continuation turn이 생성되고 task 또는 session ledger에 연결된다.
7. evaluator가 `done`을 반환하면 orchestrator가 goal 상태를 `done`으로 닫고 사용자에게 요약한다.

## 실패 시퀀스

1. evaluator가 낮은 confidence로 `done`을 반환하면 orchestrator는 자동 종료하지 않고 확인 또는 추가 검증으로 돌린다.
2. turn budget이 소진되면 continuation을 멈추고 goal을 `blocked` 또는 `paused` projection으로 보여준다.
3. 사용자가 중간에 새 요청을 보내면 active continuation을 중단하고 사용자 입력을 먼저 처리한다.
4. pause 상태에서 wake가 도착하면 자동 continuation을 실행하지 않고 suppressed task outcome을 남긴다.
5. clear 이후 늦게 도착한 evaluator verdict는 stale로 기록하고 적용하지 않는다.

## 검증 관점

- `done`, `continue`, `blocked` verdict별 상태 전이를 fixture로 확인한다.
- pause 상태에서 scheduled wake가 continuation을 만들지 않는지 확인한다.
- user interruption이 evaluator suggestion보다 우선하는지 확인한다.
- turn budget 초과가 무한 loop를 막는지 확인한다.
- clear 이후 stale result가 goal을 되살리지 않는지 확인한다.

## 완료 기준

- persistent goal lifecycle의 모든 상태와 전이가 구현 요구사항과 테스트로 닫힌다.
- `/goal` 계열 UX가 같은 상태 모델을 읽고 쓴다.
- completion evaluator verdict가 authority boundary를 넘지 않는다.
- user interruption, pause, clear, turn budget이 continuation loop를 항상 멈출 수 있다.
