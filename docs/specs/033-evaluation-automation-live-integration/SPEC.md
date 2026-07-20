# 033. evaluation automation live integration 아키텍처 명세

Status: Open

Origin specs: 009, 012, 013, 014, 016, 018, 022

## 목적

이 문서는 specs 009, 012, 013, 014, 016, 018, 022가 implemented scope로 닫힌 뒤에도 남는 evaluation, automation, self-improvement live integration work의 owner boundary를 연다.

핵심 목적은 helper-level contract로 닫힌 evaluator, goal, automation, replay, projection, diagnostics 개념을 실제 AgentLoop, service runtime, channel worker, local API, future TUI, release evidence workflow에 end-to-end로 연결하는 것이다.

033은 새 중앙 evaluator 제품을 만드는 문서가 아니다. 이미 닫힌 primitive를 live runtime path에서 소비하고, 사용자가 재현 가능한 evidence로 검증할 수 있게 만드는 open spec이다.

## 현재 구현 baseline

현재 구현은 다음 baseline을 가진다.

1. 009는 `ContextBuilder`, memory, compaction, runner governance, provider shaping 기준의 context assembly mapping을 닫았다. Formal snapshot과 token budget model은 구현 완료가 아니다.
2. 012는 process-local bus, session turn lock, active task cancellation, channel worker wiring, follow-up queue, runtime metadata JSON hint를 닫았다. Durable queue, durable scheduler, owner lease, formal supervisor는 구현 완료가 아니다.
3. 013은 CLI/session command UX, session projection/query model, local API session query, WebSocket/chat completion/streaming surface, web helper baseline을 갖는다. Terminal TUI와 full shared projection builder는 future surface다.
4. 014는 local diagnostics snapshot, marker-based projection, redacted diagnostics bundle, CLI/API diagnostics surface를 갖는다. Durable trace store나 remote observability product는 구현 완료가 아니다.
5. 016은 verification family, release gate language, spec coverage matrix, blocker와 waiver 원칙을 닫았다.
6. 018은 PRD 000-014를 Rust contract, runtime helper, projection helper, release-gate helper 기준으로 닫았다. 이는 full live end-to-end product integration이 아니다.
7. 022는 permission mode, capability taxonomy, action normalization, runtime policy gate, approval correlation, audit, replay invariant, guarded classifier-backed auto mode, recent denial retry slice를 닫았다.

이 baseline은 아래를 완료로 주장하지 않는다.

1. Goal evaluator가 live AgentLoop end-of-turn과 scheduled wake에서 일관되게 소비되는 path.
2. Automation job lifecycle이 service/channel/local API/TUI projection과 연결되는 path.
3. Self-improvement proposal이 approval, checkpoint, apply, verify, record, rollback candidate까지 실제 owner primitive를 통과하는 path.
4. Replay and evaluation dataset이 release coverage entry와 reproducible artifact로 남는 path.
5. QA, goal, code, security, docs review가 저장소 안에서 재현 가능한 evidence로 남는 workflow.
6. BlockedApproval, MissingRedactionEvidence, duplicate consumption, superseded consumption 같은 release blocker edge regression.

## owned open scope

033이 소유하는 open scope는 다음이다.

1. AgentLoop, service runtime, channel worker를 가로지르는 evaluator, goal, automation, self-improvement, replay, diagnostics의 live end-to-end wiring과 local API/TUI가 소비할 domain projection input.
2. Goal lifecycle: set, status, pause, resume, clear, done, blocked, continuation budget, user interruption priority.
3. Completion evaluator consumption: turn end, scheduled wake, subagent result, app task result, channel result, local API background result.
4. Automation job lifecycle: one-shot, recurring, skill-backed agent job, script-only job, no-agent maintenance job, app task job.
5. Task outcome evaluator live routing: notify, suppress, continue, escalate, verify, rollback candidate.
6. Self-improvement live flow: proposal, approval, checkpoint, apply, verify, record, rollback candidate, diagnostics receipt.
7. Live destructive dispatch 없이 selected trajectory를 재현할 수 있는 replay dataset과 local evaluation runner.
8. 018 PRD 000-014와 모든 033 live integration slice를 가리키는 release coverage entry.
9. Goal review, QA review, code review, security review, docs review를 위한 reproducible review artifact.
10. Blocked approval, missing redaction evidence, stale approval, expired approval, consumed approval, duplicate consumption, superseded consumption, recursion guard, delivery failure, replay mismatch edge regression.

033은 하위 owner boundary를 소비하지만 대체하지 않는다. 009/012/013/014/016/018/022는 구현 baseline을 제공한다. Shared UI adapter, surface parity smoke, release runner shell은 031이 소유한다. 033은 evaluation/automation domain state, coverage entry, review artifact와 edge-regression evidence를 생산한다.

## invariants

1. Evaluator verdict는 advisory input이다. Session truth, permission truth, app truth, service truth를 직접 바꾸면 안 된다.
2. `MainOrchestrator`는 session state, continuation, permission consumption, tool execution, approval result, apply result의 권한자로 남는다.
3. User interruption은 automated continuation보다 항상 우선한다.
4. Automation은 goal state, job timeout, recursion guard, permission snapshot, delivery policy, cancellation state의 경계를 넘어설 수 없다.
5. 모든 automation이 agent call을 요구하지는 않는다. Maintenance job과 script-only job은 계약상 model invocation이 필요 없으면 agent 없이 실행될 수 있다.
6. Replay는 destructive tool, plugin command, app process action, remote delivery, self-improvement apply를 live-dispatch하지 않는다.
7. Suppress는 user notification을 생략한다는 뜻이지 evidence deletion이 아니다.
8. Missing redaction evidence는 release artifact의 blocker이며, 무시 가능한 warning이 아니다.
9. 022의 approval correlation은 self-improvement apply, rollback candidate execution, broad automation mutation, permissioned automation step에 필수다.
10. Rollback은 universal automatic action이 아니다. Owner policy, permission, checkpoint availability, user-visible evidence rule을 통과해야 하는 candidate action이다.
11. Diagnostics와 release evidence는 저장 또는 표시 전에 redaction을 통과해야 한다.

## Must Have

1. Active, paused, blocked, done, cleared 상태와 latest evaluator verdict를 보여주는 goal command와 projection path.
2. Evaluator에게 state authority를 주지 않으면서 정해진 경계에서 completion evaluator output을 실행하거나 소비하는 AgentLoop integration.
3. Normalized trigger, run state, timeout, idempotency key, recursion guard, permission snapshot, delivery target, result policy를 갖는 scheduled automation integration.
4. Channel worker, service job, subagent result, app task result, local API background result를 다루는 task outcome evaluator integration.
5. Approval, checkpoint, apply, verify, record, rollback candidate를 거치는 self-improvement proposal store와 live flow.
6. Goal, automation, evaluator, approval, verify, rollback candidate의 domain state vocabulary와 projection input. CLI/local API/channel/TUI adapter parity는 031이 소유한다.
7. Local trajectory를 선택하고 frozen snapshot을 읽고 expected verdict/outcome을 비교하며 live destructive dispatch를 거부하는 replay runner.
8. Goal id, automation job id, turn id, approval id, checkpoint id, trajectory id, redacted artifact reference를 연결하는 diagnostics receipt.
9. 각 018 PRD 000-014 helper contract와 각 033 live integration path를 가리키는 release coverage entry.
10. QA, goal adherence, code quality, security, docs consistency, release readiness를 위해 repo에 저장되는 reproducible review artifact.
11. BlockedApproval, MissingRedactionEvidence, stale approval, expired approval, consumed approval, duplicate result consumption, superseded run consumption, recursion guard, replay mismatch, delivery failure edge regression test.

## Must Not Have

1. Central evaluator SaaS, hosted judge service, organization evaluator dashboard, fleet scoring control plane.
2. Arbitrary remote self-update 또는 silent runtime code replacement.
3. 모든 automation job에 대한 mandatory agent call.
4. 실패 뒤 universal automatic rollback.
5. Permission을 직접 approve하거나, approval을 consume하거나, tool을 execute하거나, app process를 start하거나, external delivery를 send하거나, config를 write하는 evaluator verdict.
6. Live tool, live plugin command, live app action, live remote delivery, live self-improvement apply를 실행하는 replay path.
7. Raw secret, provider hidden reasoning, unredacted external payload, 불필요한 full file content를 저장하는 diagnostics artifact.
8. Channel notification success를 job success로 취급하는 구현. Delivery state와 job state는 분리돼야 한다.
9. Manual happy path가 한 번 동작했다는 이유로 release gate를 waive하는 방식.
10. Owner evidence 없이 completed, approved, rolled back, recovered state를 만들어 내는 TUI 또는 API projection.

## acceptance criteria

1. Goal lifecycle can be exercised through CLI and local API, and the same state is visible through projections.
2. AgentLoop records completion evaluator input and output at turn end, respects user interruption, and refuses unbounded continuation.
3. Scheduled automation can run at least one one-shot job, one recurring job, one no-agent maintenance job, and one skill-backed agent job under the same lifecycle contract.
4. Task outcome evaluator routes results to notify, suppress, continue, escalate, verify, rollback candidate without deleting evidence.
5. Self-improvement proposal cannot apply without approval correlation and checkpoint requirement handling. Failed verify records evidence and exposes rollback candidate only when safe.
6. Permission-sensitive automation and self-improvement steps pass 022 action normalization and approval correlation.
7. Replay runner reproduces selected trajectories from local artifacts and fails closed when a case would require live destructive dispatch.
8. Diagnostics bundle includes redacted goal, automation, evaluator, approval, checkpoint, replay, projection evidence, and fails release checks if redaction evidence is missing.
9. 033이 goal status, automation run status, evaluator verdict, pending approval, verification result, rollback candidate의 domain vocabulary와 evidence를 생산하고, 031 adapter가 CLI, local API, channel notification/status, future TUI에 같은 의미로 투영한다.
10. Release coverage entry lists 033 and maps code paths, tests, commands, artifacts, and remaining waivers. No waiver may hide MissingRedactionEvidence or approval mismatch.
11. Review artifacts for QA, goal, code, security, docs are reproducible from stored commands or local artifacts and are referenced by release evidence.

## source handoff table

| origin spec | 닫힌 implemented scope | 033으로 넘어오는 open work |
|---|---|---|
| 009 context assembly and compaction input | Current context builder, memory, compaction, runner governance, provider shaping mapping | Frozen evaluator input snapshot, replay context evidence, redaction-safe evaluator source references |
| 012 runtime services | Process-local bus, lock, active task registry, channel runtime wiring, follow-up queue, metadata hints | Automation job lifecycle, scheduled wake integration, service result outcome routing, delivery/job state split |
| 013 user interfaces and session UX | CLI/session projection, local API session query, WebSocket/chat surface, web helper baseline | Goal, automation, evaluator, approval, verify, rollback candidate projection parity across CLI, local API, channel, future TUI |
| 014 observability diagnostics and inspection | Local diagnostics, redaction model, runtime marker projection, diagnostics bundle | Evaluation ledger, automation receipt, replay artifact, review evidence, MissingRedactionEvidence blocker |
| 016 verification matrix and release gates | Verification family, release gates, blocker language, coverage matrix | 033 release coverage entries, reproducible review artifacts, edge regression gate |
| 018 evaluation automation and self-improvement | PRD 000-014 Rust contract/runtime-helper/projection-helper/release-gate-helper closure | Live AgentLoop/service/channel/API/TUI integration and end-to-end product closure |
| 022 auto approval permissions | Permission mode, capability taxonomy, runtime policy gate, approval correlation, audit, replay invariant, recent denial slice | Automation and self-improvement action gate consumption, stale/expired/consumed approval regression, permission-sensitive rollback candidate handling |

## closure evidence

033는 아직 open 상태다. 이 spec을 닫으려면 아래 evidence가 저장소에 있어야 한다.

1. 코드 증거: AgentLoop evaluator integration, goal lifecycle commands, automation job runtime, task outcome routing, self-improvement live flow, replay runner, projection builders, diagnostics artifact writer.
2. 테스트 증거: goal state tests, automation lifecycle tests, service/channel/local API integration tests, permission correlation tests, self-improvement apply and verify tests, replay fail-closed tests, projection parity tests.
3. Edge 증거: BlockedApproval, MissingRedactionEvidence, stale approval, expired approval, consumed approval, duplicate consumption, superseded consumption, recursion guard, delivery failure, replay mismatch regression.
4. Release 증거: 033 coverage entry와 018 PRD 000-014 live wiring direct entry. 각 entry는 local에서 다시 실행 가능한 command 또는 artifact를 가리켜야 한다.
5. Review 증거: redacted, reproducible, release coverage에 연결된 stored QA, goal, code, security, docs review artifact.
6. Interface 증거: goal, automation, evaluator verdict, pending approval, verify result, rollback candidate에 대한 CLI와 local API snapshot. Future TUI는 완료 주장 전에 같은 projection 의미를 소비해야 한다.
7. Diagnostics 증거: evaluator input, automation run, approval correlation, checkpoint, replay result, delivery result, review artifact가 secret을 누출하지 않음을 증명하는 redacted bundle 또는 fixture.

현재 closure evidence는 없다. 018 PRD 000-014 helper closure와 022 permission closure는 baseline input일 뿐이며, 033 live integration closure가 아니다.
