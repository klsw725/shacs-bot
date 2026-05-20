# PRD 013. user facing projections and approval surfaces

## 목표

이 문서는 018 runtime 상태를 CLI, TUI, local API, channel surface가 일관되게 소비할 shared projection과 approval status 의미로 정리한다. 013 owner spec이 rendering과 UX 구현을 소유하므로, 이 PRD는 화면 설계가 아니라 surface가 읽어야 할 상태, 승인 대기, blocked, verification status 계약을 제공한다.

## SPEC 입력

- 주관 spec: `docs/specs/018-evaluation-automation-and-self-improvement/SPEC.md`
- 선행 PRD:
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/001-persistent-goal-and-continuation-loop.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/002-capability-approval-and-checkpoint-gates.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/003-task-outcome-and-scheduled-automation.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/005-self-improvement-app-and-mcp-integration.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/007-projections-diagnostics-and-release-gates.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/008-runtime-evaluator-enforcement-and-ledger-consumption.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/009-scheduled-automation-runtime-execution.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/011-self-improvement-apply-verify-and-rollback-wiring.md`
- 교차 의존:
  - `docs/specs/012-runtime-services/SPEC.md`
  - `docs/specs/013-user-interfaces-and-session-ux/SPEC.md`
  - `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`

## Dependency Cut

- 007은 initial projection과 release gate 의미를 제공한다.
- 008은 runtime decision status와 ledger consumption evidence를 제공한다.
- 009는 automation run status와 delivery status를 제공한다.
- 011은 improvement approval, verify, rollback status를 제공한다.
- 012는 local API와 channel delivery primitive를 소유한다.
- 013은 CLI, TUI, local API UX rendering과 interaction details를 소유한다.
- 014는 inspect surface와 diagnostics artifact를 소유한다.
- 018은 shared projection schema와 status semantics만 소유한다.

## 범위

- 018 shared projection schema
- goal, automation, approval, blocked, verification, replay status 의미
- CLI, TUI, local API, channel이 같은 status enum을 소비하는 기준
- approval surface에 필요한 evidence summary와 allowed actions
- redaction과 raw secret 노출 금지
- acknowledgement와 user decision handoff semantics

## 범위 제외

- CLI command 이름 최종 결정
- TUI layout, color, keyboard binding
- local API transport와 authentication primitive 구현
- channel vendor별 message format
- diagnostics bundle writer 구현
- 관리자 대시보드, 조직 승인 inbox, SaaS console

## 구현 요구사항

- shared projection은 `018Projection`으로 versioned schema를 가져야 한다.
- projection은 active goal, paused goal, blocked goal, automation runs, pending approvals, improvement proposals, replay regressions, recent evaluator decisions를 redacted summary로 제공해야 한다.
- 모든 surface는 같은 projection source를 읽어야 하며 surface별로 status 의미를 재해석하면 안 된다.
- status enum은 `idle`, `running`, `waiting_for_user`, `approval_required`, `blocked`, `verification_pending`, `verification_failed`, `rollback_available`, `rolled_back`, `completed`, `suppressed`를 포함해야 한다.
- blocked status는 blocked reason class, user action hint, evidence refs, retry eligibility를 포함해야 한다.
- approval required status는 proposal id, requested scope, risk summary, rollback plan summary, allowed decisions를 포함해야 한다.
- allowed decisions는 `approve`, `reject`, `defer`, `inspect_evidence`를 구분해야 하며 surface가 지원하지 않는 action은 숨기는 것이 아니라 unavailable reason을 제공해야 한다.
- verification status는 expected behavior summary, last verify result, failure reason, rollback eligibility를 포함해야 한다.
- automation delivery status는 target surface, severity, suppress reason, acknowledged state를 포함해야 한다.
- channel projection은 notify, escalate, blocked, approval required, verification failed처럼 user visible event만 전달해야 한다.
- local API projection은 raw secret, unredacted file content, private tool payload를 노출하면 안 된다.
- projection item은 diagnostics inspect로 이어지는 redacted evidence ref를 포함해야 한다.
- acknowledgement는 user decision과 구분되어야 하며, message를 읽었다는 사실이 approval로 해석되면 안 된다.
- projection은 self hosted local use를 기본으로 하며 관리자나 조직 queue를 전제로 만들면 안 된다.

## 데이터/상태 모델

- `018Projection`: schema version, generated at, session id, goal summaries, automation summaries, approval summaries, verification summaries, replay summaries, evidence refs.
- `ProjectionStatus`: status kind, reason class, severity, user action hint, retry eligibility, evidence refs.
- `ApprovalProjectionItem`: proposal id, target kind, requested scope, risk summary, rollback summary, allowed decisions, evidence refs.
- `BlockedProjectionItem`: source kind, source ref, blocked reason, unblock hint, retry eligibility, diagnostics ref.
- `VerificationProjectionItem`: proposal id or replay case id, expected summary, last result, failure reason, rollback eligibility.

## 정상 시퀀스

1. improvement proposal이 approval pending 상태가 된다.
2. 018 projection builder가 proposal scope, risk summary, rollback summary, evidence refs를 redacted item으로 만든다.
3. CLI, TUI, local API, channel surface가 같은 projection item을 읽는다.
4. 사용자가 surface에서 approve 또는 reject 결정을 내린다.
5. decision은 owner approval primitive로 전달되고 projection은 acknowledged와 approved를 구분해 갱신된다.

## 실패 시퀀스

1. automation run이 verify 실패 후 rollback 가능한 상태가 된다.
2. projection builder가 `verification_failed`와 `rollback_available` status를 만든다.
3. channel surface는 짧은 redacted notification만 전달한다.
4. CLI 또는 TUI는 evidence inspect action을 연결한다.
5. 사용자가 아무 결정도 하지 않으면 rollback은 자동 실행되지 않고 approval required 또는 waiting status로 남는다.

## 검증 관점

- CLI, TUI, local API, channel이 같은 source projection에서 같은 status kind를 읽는지 확인한다.
- acknowledgement가 approval이나 rejection으로 기록되지 않는지 확인한다.
- blocked status가 reason class와 unblock hint 없이 노출되지 않는지 확인한다.
- local API projection에 raw secret이나 unredacted payload가 포함되지 않는지 확인한다.
- channel projection이 suppress된 event나 inspect only evidence를 불필요하게 보내지 않는지 확인한다.
- approval item이 requested scope와 rollback summary 없이 승인 가능 상태가 되지 않는지 확인한다.

## 완료 기준

- 018 shared projection schema가 versioned 상태로 정의된다.
- 모든 user facing surface가 goal, automation, approval, blocked, verification, replay status를 같은 의미로 소비한다.
- approval, acknowledgement, inspect, retry decision의 의미가 분리된다.
- projection은 redacted evidence ref만 제공하며 raw private payload를 노출하지 않는다.
- 013 owner spec이 visual rendering을 구현할 수 있을 만큼 상태와 action semantics가 구체적이다.
