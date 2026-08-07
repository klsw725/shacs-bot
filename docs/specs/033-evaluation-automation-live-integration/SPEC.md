# 033. evaluation automation live integration 아키텍처 명세

Status: Open

Origin specs: 009, 012, 013, 014, 016, 018, 022

## 목적

이 문서는 specs 009, 012, 013, 014, 016, 018, 022가 implemented scope로 닫힌 뒤에도 남는 evaluation, automation, self-improvement live integration work의 owner boundary를 연다.

핵심 목적은 helper-level contract로 닫힌 evaluator, goal, automation, replay, projection, diagnostics 개념을 실제 AgentLoop, service runtime, channel worker, local API, future TUI, release evidence workflow에 end-to-end로 연결하는 것이다.

033은 새 중앙 evaluator나 policy 제품을 만드는 문서가 아니다. 현재 owner가 제공하는 trusted-runtime, scheduler, projection, execution-snapshot primitive를 live path에서 소비하고, goal accounting과 self-improvement apply-time CAS를 포함한 결과를 사용자가 재현 가능한 evidence로 검증하게 만드는 open spec이다.

## 현재 구현 baseline

현재 구현은 다음 baseline을 가진다.

1. 009는 `ContextBuilder`, memory, compaction, runner governance, provider shaping 기준의 context assembly mapping을 닫았다. Formal snapshot과 token budget model은 구현 완료가 아니다.
2. 012는 process-local bus, session turn lock, active task cancellation, channel worker wiring, follow-up queue, runtime metadata JSON hint를 닫았다. Durable queue/scheduler, owner lease/supervision, durable trace는 029의 current scoped implementation을 소비하며 033이 재구현하지 않는다.
3. 013은 CLI/session command UX, session projection/query model, local API session query, WebSocket/chat completion/streaming surface, web helper baseline을 갖는다. Shared projection, TUI/REPL/onboard, reconnect/backpressure는 035의 current implementation baseline과 planned revision을 소비한다.
4. 014는 local diagnostics snapshot, marker-based projection, projection-boundary diagnostics bundle, CLI/API diagnostics surface를 갖는다. 033 artifact transform은 이 baseline을 소비하지만 runtime trace 전체의 complete redaction을 주장하지 않는다.
5. 016은 verification family, release gate language, spec coverage matrix, blocker와 waiver 원칙을 닫았다.
6. 018은 PRD 000-014를 Rust contract, runtime helper, projection helper, release-gate helper 기준으로 닫았다. 이는 full live end-to-end product integration이 아니다.
7. 022의 permission mode, approval correlation, replay helper는 닫힌 historical baseline으로 남을 수 있다. 그러나 중앙 permission mode, durable approval, remembered allow, replay authorization은 030 전환 뒤 033의 live owner contract나 closure guarantee가 아니다.

이 baseline은 아래를 완료로 주장하지 않는다.

1. Goal evaluator가 live AgentLoop end-of-turn과 scheduled wake에서 일관되게 소비되는 path.
2. Automation job lifecycle이 service/channel/local API/TUI projection과 연결되는 path.
3. Self-improvement proposal이 immutable snapshot, apply-time CAS, pre-tool hook, 필요한 ephemeral confirmation, checkpoint, apply, verify, record, rollback candidate까지 실제 owner primitive를 통과하는 path.
4. Replay and evaluation dataset이 release coverage entry와 reproducible artifact로 남는 path.
5. QA, goal, code, security, docs review가 저장소 안에서 재현 가능한 evidence로 남는 workflow.
6. HookVeto, HeadlessConfirmationDenied, snapshot mismatch, MissingRedactionEvidence, duplicate consumption, superseded consumption 같은 release blocker edge regression.

## owned open scope

033이 소유하는 open scope는 다음이다.

1. AgentLoop, service runtime, channel worker를 가로지르는 evaluator, goal, automation, self-improvement, replay, diagnostics의 live end-to-end wiring과 local API/TUI가 소비할 domain projection input.
2. Goal lifecycle: set, status, pause, resume, clear, done, blocked, continuation budget, user interruption priority.
3. Completion evaluator consumption: turn end, scheduled wake, subagent result, app task result, channel result, local API background result.
4. Automation job lifecycle: one-shot, recurring, skill-backed agent job, script-only job, no-agent maintenance job, app task job.
5. Task outcome evaluator live routing: notify, suppress, continue, escalate, verify, rollback candidate.
6. Self-improvement live flow: proposal, immutable execution snapshot, apply-time compare-and-swap, pre-tool hook, 필요한 ephemeral confirmation, checkpoint, apply, verify, record, rollback candidate, diagnostics receipt.
7. Live destructive dispatch 없이 selected trajectory를 재현할 수 있는 replay dataset과 local evaluation runner.
8. 018 PRD 000-014와 모든 033 live integration slice를 가리키는 release coverage entry.
9. Goal review, QA review, code review, security review, docs review를 위한 reproducible review artifact.
10. Hook veto, headless confirmation denial, missing hook evidence, process timeout/cleanup incomplete, sandbox fallback/failure, credential unavailable, snapshot missing/mismatch/source mutation, missing artifact redaction evidence, duplicate consumption, superseded consumption, recursion guard, delivery failure, replay mismatch edge regression.

033은 하위 owner boundary를 소비하지만 대체하지 않는다. 009/012/013/014/016/018/022는 historical implementation baseline을, 029는 durable scheduling/recovery를, 030은 trusted runtime operational facts를, 031은 immutable execution snapshot을 제공한다. Shared UI adapter, surface parity smoke, release runner shell은 035가 소유한다. 033은 evaluation/automation domain state, goal accounting, coverage entry, review artifact와 edge-regression evidence를 생산한다.

## invariants

1. Evaluator verdict는 advisory input이다. Session truth, hook veto, confirmation result, process outcome, credential status, sandbox status, app truth, service truth를 직접 바꾸거나 override하면 안 된다.
2. `MainOrchestrator`는 session state, continuation, dispatch, apply lifecycle의 권한자로 남고 030의 hook/confirmation/process 결과를 소비한다. Evaluator는 이 결과를 생성하지 않는다.
3. User interruption은 automated continuation보다 항상 우선한다.
4. Automation은 goal state, job timeout, recursion guard, immutable execution snapshot, trusted runtime profile, active hook result, adapter별 process control, sandbox/credential status, delivery policy, cancellation state의 경계를 넘어설 수 없다.
5. 모든 automation이 agent call을 요구하지는 않는다. Maintenance job과 script-only job은 계약상 model invocation이 필요 없으면 agent 없이 실행될 수 있다.
6. Replay는 hook handler, confirmation prompt, credential refresh, process launch, sandbox initialization, destructive tool, plugin command, app process action, remote delivery, config mutation, self-improvement apply를 live-dispatch하지 않는다.
7. Suppress는 user notification을 생략한다는 뜻이지 evidence deletion이 아니다.
8. Missing artifact redaction/disclosure evidence는 033 release artifact의 blocker이며, 무시 가능한 warning이 아니다. 원본 session/log/trace 전체가 secret-safe하다는 뜻은 아니다.
9. Live execution 직전에는 030 `tool:before` hook, 필요한 ephemeral confirmation, headless deny, adapter별 process capability, sandbox fallback disclosure, credential status와 031 immutable execution snapshot을 확인한다. 이 검사는 durable approval이나 replay authorization을 만들지 않는다.
10. Self-improvement apply는 proposal target digest와 current target digest의 compare-and-swap을 통과해야 한다. Stale proposal은 side effect 전에 fail closed한다.
11. Rollback은 universal automatic action이 아니다. Checkpoint availability, current hook/confirmation, process control, sandbox disclosure, user-visible evidence를 다시 통과하는 candidate action이며 timeout/kill이 side-effect rollback을 보장하지 않는다.
12. 033이 생산하는 diagnostics/release artifact는 명시적 projection-boundary redaction/disclosure transform을 통과해야 하고 transform evidence가 없으면 blocked다.

## Must Have

1. Active, paused, blocked, done, cleared 상태와 latest evaluator verdict를 보여주는 goal command와 projection path.
2. Evaluator에게 state authority를 주지 않으면서 정해진 경계에서 completion evaluator output을 실행하거나 소비하는 AgentLoop integration.
3. Normalized trigger, run state, timeout, idempotency key, recursion guard, execution snapshot id/digest, trusted runtime ref, hook status, adapter별 process control, sandbox/credential status, unattended/headless mode, delivery target, result policy를 갖는 scheduled automation integration.
4. Channel worker, service job, subagent result, app task result, local API background result를 다루는 task outcome evaluator integration.
5. Immutable execution snapshot, apply-time CAS, pre-tool hook, 필요한 ephemeral confirmation, checkpoint, apply, verify, record, rollback candidate를 거치는 self-improvement proposal store와 live flow.
6. Goal, automation, evaluator, hook outcome, confirmation event, verify, rollback candidate의 domain state vocabulary와 projection input. CLI/local API/channel/TUI adapter parity는 035가 소유한다.
7. Local trajectory를 선택하고 031 immutable execution snapshot과 recorded artifact를 읽고 expected verdict/outcome을 비교하며 live source 재조회와 destructive dispatch를 거부하는 replay runner.
8. Goal id, automation job id, turn id, hook/confirmation event id, checkpoint id, trajectory id, execution snapshot id/digest, safe artifact reference를 연결하는 diagnostics receipt.
9. 각 018 PRD 000-014 helper contract와 각 033 live integration path를 가리키는 release coverage entry.
10. QA, goal adherence, code quality, security, docs consistency, release readiness를 위해 repo에 저장되는 reproducible review artifact.
11. HookVeto, HeadlessConfirmationDenied, MissingHookEvidence, ProcessTimeout, AbortCleanupIncomplete, SandboxUnsupported/Failed, CredentialSourceUnavailable, SnapshotMissing/DigestMismatch/SourceMutation, MissingRedactionEvidence, duplicate result consumption, superseded run consumption, recursion guard, replay mismatch, delivery failure edge regression test.

## Must Not Have

1. Central evaluator SaaS, hosted judge service, organization evaluator dashboard, fleet scoring control plane.
2. Arbitrary remote self-update 또는 silent runtime code replacement.
3. 모든 automation job에 대한 mandatory agent call.
4. 실패 뒤 universal automatic rollback.
5. Hook veto를 override하거나, confirmation을 durable approval로 승격하거나, auto-allow를 기억하거나, process/credential/sandbox/tool/app/delivery/config/apply action을 직접 수행하는 evaluator verdict.
6. Live tool, live plugin command, live app action, live remote delivery, live self-improvement apply를 실행하는 replay path.
7. Raw secret, provider hidden reasoning, unredacted external payload, 불필요한 full file content를 저장하는 diagnostics artifact.
8. Channel notification success를 job success로 취급하는 구현. Delivery state와 job state는 분리돼야 한다.
9. Manual happy path가 한 번 동작했다는 이유로 release gate를 waive하는 방식.
10. Owner evidence 없이 completed, execution_allowed, rolled_back, recovered state를 만들어 내는 TUI 또는 API projection.
11. Execution snapshot id/digest를 permission grant, capability ceiling, durable approval, replay authorization으로 사용하는 구현.

## acceptance criteria

1. Goal lifecycle can be exercised through CLI and local API, and the same state is visible through projections.
2. AgentLoop records completion evaluator input and output at turn end, respects user interruption, and refuses unbounded continuation.
3. Scheduled automation can run at least one one-shot job, one recurring job, one no-agent maintenance job, and one skill-backed agent job under the same lifecycle contract. Headless confirmation-required step은 auto-allow하지 않고, trusted native fallback과 sandbox-required 차이를 evidence에 남긴다.
4. Task outcome evaluator routes results to notify, suppress, continue, escalate, verify, rollback candidate without deleting evidence.
5. Self-improvement proposal cannot apply without immutable execution snapshot, checkpoint handling, apply-time CAS, current hook result, and required ephemeral confirmation. Headless confirmation이 불가능하면 deny하고, failed verify는 evidence와 rollback candidate만 남긴다.
6. Execution-sensitive automation과 self-improvement step은 030 hook/process/credential/sandbox contract와 031 execution snapshot을 소비한다. 022 approval correlation은 compatibility helper일 수 있으나 closure guarantee가 아니다.
7. Replay runner reproduces selected trajectories from local artifacts and fails closed when a case would require live destructive dispatch.
8. Diagnostics bundle includes projection-redacted goal, automation, evaluator, hook/confirmation, checkpoint, execution snapshot, replay, projection evidence and transform status. 원본 runtime trace의 secret-safety를 주장하지 않으며 transform evidence가 없으면 release를 막는다.
9. 033이 goal id/state/stop reason/continuation budget, automation run status, evaluator verdict, hook/confirmation outcome, verification result, rollback candidate의 domain vocabulary와 evidence를 생산하고, 035 adapter가 같은 의미로 투영한다.
10. Release coverage entry lists 033 and maps code paths, tests, commands, artifacts, and remaining waivers. No waiver may hide MissingRedactionEvidence, missing hook evidence, headless denial, snapshot mismatch, sandbox/credential disclosure 누락.
11. Review artifacts for QA, goal, code, security, docs are reproducible from stored commands or local artifacts and are referenced by release evidence.

## source handoff table

| origin spec | 닫힌 implemented scope | 033으로 넘어오는 open work |
|---|---|---|
| 009 context assembly and compaction input | Current context builder, memory, compaction, runner governance, provider shaping mapping | 031 snapshot에 포함될 evaluator input/context provenance와 safe source references |
| 012 runtime services | Process-local bus, lock, active task registry, channel runtime wiring, follow-up queue, metadata hints | Automation job lifecycle, scheduled wake integration, service result outcome routing, delivery/job state split |
| 013 user interfaces and session UX | CLI/session projection, local API session query, WebSocket/chat surface, web helper baseline | Goal, automation, evaluator, hook/confirmation, verify, rollback candidate projection inputs |
| 014 observability diagnostics and inspection | Local diagnostics, projection-boundary redaction points, runtime marker projection, diagnostics bundle | Evaluation ledger, automation receipt, replay artifact, review evidence, MissingRedactionEvidence blocker |
| 016 verification matrix and release gates | Verification family, release gates, blocker language, coverage matrix | 033 release coverage entries, reproducible review artifacts, edge regression gate |
| 018 evaluation automation and self-improvement | PRD 000-014 Rust contract/runtime-helper/projection-helper/release-gate-helper closure | Live AgentLoop/service/channel/API/TUI integration and end-to-end product closure |
| 022 auto approval permissions | Historical normalization/audit/replay helpers and closed implementation evidence | Compatibility input only. Central permission mode, durable approval correlation, replay authorization은 033 신규 보장이 아님 |
| 029 durable runtime recovery | Durable queue/scheduler, owner lease/supervision, trace/recovery scoped baseline | Automation scheduling/recovery facts를 소비하고 별도 scheduler·lease·trace를 재구현하지 않음 |
| 030 trusted agent runtime | Pre-tool veto, ephemeral confirmation/headless deny, adapter별 process control, credential/sandbox/resource/data disclosure | Live automation/self-improvement gate와 evidence, replay no-live-dispatch invariant |
| 031 configuration and snapshots | Immutable config/context/provider/trusted-runtime execution snapshot과 provenance | Evaluator, automation, self-improvement, replay input과 diagnostics receipt |
| 035 UI projection parity | Shared projection, TUI/REPL/onboard, reconnect/release baseline과 planned Tasks view | 033 goal/automation owner facts를 cross-surface adapter와 release evidence로 소비 |

## Implementation PRDs

Spec 033은 goal accounting에서 evaluator, automation, CAS self-improvement, replay와 final closure까지 아래 단계로 구현한다. 각 PRD는 exact owner-fact contract만 소비하며 외부 spec의 `Complete` 상태를 자신의 exit criterion으로 요구하지 않는다.

| PRD | Sole owner scope | Depends on |
|---|---|---|
| [PRD 000](prds/000-goal-accounting-and-projection-input.md) | Goal lifecycle, stop reason, continuation budget, projection owner facts | 018 goal baseline |
| [PRD 001](prds/001-completion-evaluator-live-integration.md) | Completion evaluator consumption과 task outcome routing | PRD 000, 018 evaluator baseline |
| [PRD 002](prds/002-automation-job-lifecycle-and-outcome-routing.md) | One-shot/recurring/no-agent/skill-backed automation lifecycle | PRD 001, Specs 029/030/031 fact contracts |
| [PRD 003](prds/003-self-improvement-cas-apply-and-verify.md) | Proposal, immutable snapshot, apply-time CAS, hook/confirmation, checkpoint/apply/verify | PRDs 001-002, Specs 030/031 fact contracts |
| [PRD 004](prds/004-snapshot-replay-review-and-release-evidence.md) | Snapshot-based replay, review artifacts, edge regression, coverage entry | PRDs 001-003, Specs 031/035 fact contracts |
| [PRD 005](prds/005-sequential-integration-and-spec033-closure.md) | End-to-end integration, requirement mapping, final Spec033 closure | PRDs 000-004, required owner-fact audits |

Current PRD status:

| PRD | Status |
|---|---|
| PRD 000 | Planned |
| PRD 001 | Planned |
| PRD 002 | Planned |
| PRD 003 | Planned |
| PRD 004 | Planned |
| PRD 005 | Planned |

Dependency rules:

1. Goal projection은 PRD 000 owner fact가 먼저 존재해야 하며 035 adapter가 goal truth를 만들지 않는다.
2. Evaluator는 advisory output만 생산하고 PRD 002-003의 live action을 직접 실행하지 않는다.
3. Replay PRD는 live hook, confirmation, credential, process, delivery, apply를 호출하지 않는다.
4. PRD 005는 외부 spec status가 아니라 exact owner facts와 local artifacts만 검사한다.

## closure evidence

033는 아직 open 상태다. 이 spec을 닫으려면 아래 evidence가 저장소에 있어야 한다.

1. 코드 증거: AgentLoop evaluator integration, goal lifecycle commands, automation job runtime, task outcome routing, self-improvement live flow, replay runner, projection builders, diagnostics artifact writer.
2. 테스트 증거: goal accounting tests, automation lifecycle tests, service/channel/local API integration tests, hook/headless confirmation tests, apply-time CAS와 self-improvement verify tests, replay fail-closed tests, projection parity tests.
3. Edge 증거: HookVeto, HeadlessConfirmationDenied, MissingHookEvidence, ProcessTimeout, AbortCleanupIncomplete, sandbox/credential failure, snapshot missing/mismatch/source mutation, MissingRedactionEvidence, duplicate/superseded consumption, recursion guard, delivery failure, replay mismatch regression.
4. Release 증거: 033 coverage entry와 018 PRD 000-014 live wiring direct entry. 각 entry는 local에서 다시 실행 가능한 command 또는 artifact를 가리켜야 한다.
5. Review 증거: redacted, reproducible, release coverage에 연결된 stored QA, goal, code, security, docs review artifact.
6. Interface 증거: goal id/state/stop reason/continuation budget, automation, evaluator verdict, hook/confirmation outcome, verify result, rollback candidate에 대한 CLI와 local API snapshot. 035 TUI/Tasks view는 완료 주장 전에 같은 projection 의미를 소비해야 한다.
7. Diagnostics 증거: evaluator input, automation run, hook/confirmation event, checkpoint, execution snapshot, replay result, delivery result, review artifact에 projection-boundary transform을 적용하고 raw credential/full unnecessary payload를 artifact에 넣지 않았음을 증명하는 bundle 또는 fixture. 원본 trace 전체의 complete redaction을 주장하지 않는다.

현재 closure evidence는 없다. 018 PRD 000-014 helper closure와 022 historical implementation은 baseline input일 뿐이며, 033 live integration closure가 아니다.
