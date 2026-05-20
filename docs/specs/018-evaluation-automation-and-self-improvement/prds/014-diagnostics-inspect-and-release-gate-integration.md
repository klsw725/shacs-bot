# PRD 014. diagnostics inspect and release gate integration

## 목표

이 문서는 018 전체를 diagnostics bundle, ledger inspect, release coverage gate와 연결하는 마지막 통합 기준이다. 목표는 evaluator, automation, memory, self improvement, replay, projection이 남긴 evidence를 014 inspect surface와 016 release gate runner가 검증할 수 있게 하고, spec 018 종료 조건을 명확히 하는 것이다.

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
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/007-projections-diagnostics-and-release-gates.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/008-runtime-evaluator-enforcement-and-ledger-consumption.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/009-scheduled-automation-runtime-execution.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/010-memory-skill-curator-runtime-integration.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/011-self-improvement-apply-verify-and-rollback-wiring.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/012-replay-runner-and-auxiliary-judge-routing.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/013-user-facing-projections-and-approval-surfaces.md`
- 교차 의존:
  - `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`
  - `docs/specs/016-verification-matrix-and-release-gates/SPEC.md`

## Dependency Cut

- 모든 018 PRD는 evidence refs, ledger records, projection status를 제공한다.
- 014는 diagnostics bundle writer와 inspect surface를 소유한다. 018은 bundle에 필요한 artifact와 query key를 요구한다.
- 016은 release gate runner와 coverage matrix를 소유한다. 018은 gate가 확인해야 할 coverage entries와 blocker evidence를 요구한다.
- 018은 diagnostics 저장소, inspect UI, release runner 구현을 재정의하지 않는다.

## 범위

- diagnostics bundle에 포함할 018 evidence manifest
- ledger inspect query key와 redaction status 요구사항
- release coverage entries와 blocker evidence 정의
- evaluator, automation, memory, self improvement, replay, projection end to end trace 연결
- final 018 closure criteria와 missing coverage handling

## 범위 제외

- diagnostics bundle writer 구현
- inspect command 또는 TUI layout 구현
- release gate runner 구현
- CI vendor 선택
- 원격 observability SaaS
- 조직 release approval, 관리자 signoff, fleet health gate

## 구현 요구사항

- diagnostics bundle은 018 evidence manifest를 포함해야 한다.
- evidence manifest는 evaluator runs, ledger consumption records, automation runs, memory evidence sets, skill disclosure records, improvement proposals, replay runs, projection snapshots를 참조해야 한다.
- 모든 evidence ref는 redaction status와 source owner를 가져야 한다.
- raw secret, full private file content, unredacted tool payload는 diagnostics bundle에 포함하면 안 된다.
- ledger inspect는 evaluation ledger와 task ledger를 verdict id, goal id, task run id, proposal id, replay run id, projection item id로 조회할 수 있어야 한다.
- inspect result는 source verdict, runtime decision, consumption status, projection item, diagnostics artifact ref를 연결해야 한다.
- stale, expired, duplicate, superseded verdict는 release blocker가 아니라면 별도 skipped evidence로 집계해야 한다.
- release coverage matrix에는 evaluator foundation, goal continuation, approval gate, automation runtime, memory skill integration, self improvement wiring, replay runner, projection semantics, diagnostics integration entry가 있어야 한다.
- 각 coverage entry는 test ref, replay case ref 또는 manual verification ref 중 하나 이상을 가져야 한다.
- release gate는 blocked approval, unverified applied improvement, failed replay regression, missing redaction evidence, missing ledger consumption evidence를 blocker로 표시해야 한다.
- blocker evidence는 user local inspect에서 확인 가능한 redacted refs로 제공되어야 한다.
- final 018 closure는 모든 prior PRD의 완료 기준이 owner boundary를 지키며 연결되었을 때만 가능하다.
- 018 closure는 code가 현재 이미 통합을 수행한다는 claim을 요구하지 않는다. 구현 완료 시점의 evidence와 gate 통과를 요구한다.

## 데이터/상태 모델

- `018DiagnosticsEvidenceManifest`: manifest id, generated at, evaluator refs, ledger refs, automation refs, memory refs, improvement refs, replay refs, projection refs, redaction summary.
- `LedgerInspectQuery`: query kind, target ref, include skipped, include diagnostics refs, redaction profile.
- `LedgerInspectResult`: source refs, consumption records, runtime decisions, projection items, diagnostics artifact refs, skipped evidence.
- `018ReleaseCoverageEntry`: entry id, capability area, required evidence, test refs, replay refs, manual refs, status, blocker refs.
- `018ReleaseBlocker`: blocker id, category, source ref, severity, redacted summary, resolution hint.

## 정상 시퀀스

1. release gate가 018 coverage matrix를 실행한다.
2. 016 runner가 test, replay, manual verification refs를 수집한다.
3. 014 diagnostics writer가 018 evidence manifest를 bundle에 포함한다.
4. gate가 ledger consumption, projection status, replay result, redaction evidence를 확인한다.
5. blocker가 없으면 018 coverage entry들이 pass 상태가 된다.
6. 사용자는 local inspect로 각 pass entry의 evidence refs를 확인할 수 있다.

## 실패 시퀀스

1. self improvement proposal이 applied 상태지만 verify record가 없다.
2. release gate가 `unverified_applied_improvement` blocker를 만든다.
3. blocker evidence는 proposal id, apply record ref, missing verify requirement를 포함한다.
4. diagnostics bundle은 raw diff 대신 redacted summary와 owner refs를 포함한다.
5. gate는 018 closure를 fail로 표시하고 resolution hint를 제공한다.

## 검증 관점

- diagnostics bundle에 018 evidence manifest가 포함되고 raw secret이 없는지 확인한다.
- ledger inspect가 verdict id에서 runtime decision과 projection item까지 추적되는지 확인한다.
- release coverage entry가 test, replay, manual verification evidence 없이 pass되지 않는지 확인한다.
- stale verdict가 적절히 skipped evidence로 집계되고 runtime blocker로 오해되지 않는지 확인한다.
- unverified applied improvement와 failed replay regression이 release blocker가 되는지 확인한다.
- final closure가 모든 018 integration bucket을 포함하는지 확인한다.

## 완료 기준

- 018 diagnostics evidence manifest와 ledger inspect query contract가 정의된다.
- 014 owner surface가 필요한 evidence refs와 redaction status를 받을 수 있다.
- 016 release gate가 018 coverage entries와 blocker evidence를 평가할 수 있다.
- evaluator, automation, memory, self improvement, replay, projection의 end to end trace가 diagnostics에서 이어진다.
- spec 018의 남은 runtime, product, diagnostics, release integration closure 기준이 빠짐없이 문서화된다.
