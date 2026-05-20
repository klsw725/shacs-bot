# PRD 007. projections diagnostics and release gates

## 목표

이 문서는 018의 CLI, TUI, local API, channel projection, diagnostics bundle, ledger inspect, release gate를 완전 구현하기 위한 마지막 실행 기준이다. 앞선 018 PRD가 만든 evaluator, automation, self improvement, replay 상태를 사용자가 로컬에서 이해하고 검증할 수 있게 한다.

## SPEC 입력

- 주관 spec: `docs/specs/018-evaluation-automation-and-self-improvement/SPEC.md`
- 선행 PRD:
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/000-evaluator-foundation-and-ledger.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/001-persistent-goal-and-continuation-loop.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/002-capability-approval-and-checkpoint-gates.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/003-task-outcome-and-scheduled-automation.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/004-memory-search-skills-and-curator.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/005-self-improvement-app-and-mcp-integration.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/006-trajectory-replay-and-provider-routing.md`
- 교차 의존:
  - `docs/specs/012-runtime-services/SPEC.md`
  - `docs/specs/013-user-interfaces-and-session-ux/SPEC.md`
  - `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`
  - `docs/specs/016-verification-matrix-and-release-gates/SPEC.md`

## Dependency Cut

- 이 PRD는 모든 prior 018 PRD에 의존한다.
- 012는 service status, channel worker, local API primitive를 소유한다.
- 013은 CLI, TUI, local API UX projection을 소유한다.
- 014는 diagnostics bundle과 inspect surface를 소유한다.
- 016은 coverage matrix와 release gate runner를 소유한다.
- 018은 projection이 읽어야 할 evaluator, task, improvement, replay 의미와 release coverage 요구사항을 제공한다.

## 범위

- CLI, TUI, local API, channel projection 요구사항
- task ledger와 evaluation ledger inspect
- diagnostics bundle에 포함할 018 evidence
- release gate와 coverage matrix 업데이트
- evaluator, automation, self improvement, replay 상태의 user facing summary
- failure triage와 redaction 확인

## 범위 제외

- 새 웹 UI visual design
- 원격 observability SaaS
- 관리자 대시보드
- 조직 release approval
- CI vendor 선택
- public benchmark publication

## 구현 요구사항

- CLI와 TUI는 active goal, paused goal, blocked goal, pending approval, automation job, recent task outcome, improvement proposal, replay regression 상태를 요약해야 한다.
- local API는 같은 projection을 redacted JSON으로 제공해야 하며, raw secret과 private payload를 노출하면 안 된다.
- channel projection은 notify, escalate, blocked, approval required, verification failed 같은 user visible event만 전달해야 한다.
- inspect surface는 task ledger와 evaluation ledger를 correlation id, goal id, job id, proposal id, trajectory id로 조회할 수 있어야 한다.
- diagnostics bundle은 frozen snapshot digest, evaluator verdict summary, denied outcome, checkpoint ref, delivery failure, replay result를 포함할 수 있어야 한다.
- bundle은 redaction profile과 redaction failure 여부를 명시해야 한다.
- release gate는 018 coverage matrix를 추가하고, 각 PRD별 fixture와 smoke evidence를 연결해야 한다.
- release blocker는 redaction failure, approval bypass, stale verdict apply, destructive replay effect, silent self modification, unbounded continuation loop를 포함해야 한다.
- projection은 self hosted 개인 사용자를 기준으로 하며 관리자, 조직, fleet workflow를 만들지 않는다.

## 데이터/상태 모델

- `EvaluationProjection`: active verdicts, stale verdicts, denied outcomes, confidence summary, evidence refs.
- `AutomationProjection`: jobs, runs, outcomes, delivery states, recursion guard state.
- `ImprovementProjection`: proposals, approval state, checkpoint state, verification state, rollback state.
- `ReplayProjection`: dataset cases, replay runs, regressions, inconclusive runs, invalid fixtures.
- `LedgerInspectQuery`: correlation id, session id, goal id, job id, proposal id, trajectory id, time range.
- `Spec018CoverageEntry`: PRD id, requirement id, test evidence, diagnostics evidence, release gate status.

## 정상 시퀀스

1. 사용자가 CLI 또는 TUI에서 status를 요청한다.
2. projection layer가 task ledger, evaluation ledger, owner runtime state를 redacted read model로 합친다.
3. active goal과 automation job, pending approval, recent outcome이 표시된다.
4. 사용자가 inspect query를 실행한다.
5. diagnostics bundle이 같은 correlation id의 snapshot, verdict, task outcome, checkpoint, replay evidence를 묶는다.
6. release gate가 coverage matrix에서 018 evidence를 확인한다.

## 실패 시퀀스

1. projection 중 redaction failure가 발생하면 해당 payload를 숨기고 failure marker를 표시한다.
2. ledger record가 일부 누락되면 success처럼 보이지 않게 incomplete diagnostics로 표시한다.
3. channel delivery가 실패해도 CLI/TUI/local API inspect에서는 delivery failure를 확인할 수 있어야 한다.
4. release gate가 blocker family를 찾으면 release candidate를 중단한다.
5. stale evaluator verdict가 projection에 남아도 active truth처럼 표시하지 않는다.

## 검증 관점

- CLI, TUI, local API, channel projection이 같은 상태 의미를 공유하는지 확인한다.
- ledger inspect가 correlation id로 task와 evaluation record를 함께 찾는지 확인한다.
- diagnostics bundle에 raw secret이 포함되지 않는지 확인한다.
- release gate가 018 coverage matrix 누락을 blocker로 처리하는지 확인한다.
- stale, denied, blocked, failed 상태가 success로 오해되지 않는지 확인한다.

## 완료 기준

- 018 전체 상태가 CLI, TUI, local API, channel에서 일관된 redacted projection으로 보인다.
- task ledger와 evaluation ledger inspect가 구현되고 diagnostics bundle과 연결된다.
- 018 coverage matrix가 016 release gate에 추가된다.
- release blocker family가 자동 검증 또는 명시 evidence로 확인된다.
- SaaS, 조직 관리자, fleet 운영 가정 없이 self hosted 개인 사용 기준으로 닫힌다.
