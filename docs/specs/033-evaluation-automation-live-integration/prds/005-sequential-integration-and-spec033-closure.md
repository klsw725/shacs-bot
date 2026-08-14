# PRD 005. sequential integration and Spec 033 closure

Status: Complete (Scoped)

## Goal

PRD 000-004를 live goal/evaluator/automation/self-improvement/replay flow로 통합하고 Spec 033 parent requirements와 closure evidence를 완전히 검증한다.

## Scope

1. Acyclic DAG, requirement mapping, external owner-fact audit.
2. Goal-to-evaluator-to-automation and self-improvement end-to-end smoke.
3. Replay/review/release evidence and cross-surface projection audit.
4. Final coverage, documentation, cleanup, closure verdict.

## Non Scope

1. 새 evaluator, scheduler, permission, snapshot, projection truth를 정의하지 않는다.
2. Specs 029-031, 035의 `Complete` 상태를 요구하지 않는다.
3. Missing owner facts를 success fixture로 대체하지 않는다.

## Dependency DAG

```text
PRD000_goal
  -> PRD001_evaluator
  -> PRD002_automation
PRD001..PRD002
  -> PRD003_self_improvement
PRD001..PRD003
  -> PRD004_replay_review
PRD000..PRD004
required_owner_fact_audits
  -> PRD005_final_closure
```

## Requirement Mapping

1. Goal/accounting: PRD 000.
2. Evaluator/outcome routing: PRD 001.
3. Automation lifecycle: PRD 002.
4. CAS self-improvement: PRD 003.
5. Replay/review/release evidence: PRD 004.
6. Integration, documentation, final closure: PRD 005.

Primary parent requirements owned by this PRD:

- Primary Must Have: 6
- Primary Acceptance Criteria: 9-11

## Acceptance Criteria

1. Every parent Must Have and Acceptance Criterion has one primary PRD.
2. End-to-end tests cover interactive and headless execution, failure, cancellation, recovery, replay.
3. Exact owner facts may pass local audits while source specs remain Open.
4. Release summary records commands, artifacts, failures, disclosure, cleanup, and remaining non-guarantees.

## Closure Evidence

1. Requirement/DAG audit.
2. End-to-end goal/automation/self-improvement transcript.
3. Replay/review/coverage artifacts.
4. External owner-fact audits and final Spec033 closure summary.

Closure state: tracked evidence contract는 [`../evidence/index.json`](../evidence/index.json)에 있고 QA/goal/code/security/docs final review와 `final-production-20260814-v6` source-bound release execution은 모두 PASS다. 향후 실행 실패 또는 manifest 미생성은 shipping을 차단한다. 035의 TUI/Tasks parity와 planned PRDs 008-009는 이 PRD가 닫지 않는다.
