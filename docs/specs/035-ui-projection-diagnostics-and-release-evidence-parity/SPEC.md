# 035. UI projection, diagnostics, and release evidence parity 아키텍처 명세

Status: Open

Current machine verdict: 기존 PRD 000-007 implementation surfaces와 56개 release coverage row는 존재하지만 external owner evidence가 없어 closure가 blocked 상태다. Prime 분석에서 선택한 transport capability negotiation, snapshot-first reconnect, goal/Tasks projection은 PRD 008-009의 planned work이며 기존 구현 증거로 완료 처리하지 않는다.

Origin specs: 001, 011, 012, 013, 014, 016, 021, 023, 025, 026, 027

## 문서 목적

이 문서는 기존 spec들이 닫은 core runtime 계약을 사용자가 실제 표면에서 같은 의미로 볼 수 있게 하는 open owner boundary다. CLI, TUI, local API, external channel은 서로 다른 UI가 아니라 같은 session truth와 runtime evidence를 투영하는 projection surface다.

목표는 다음과 같다.

1. Session, subagent, durable approval, ephemeral confirmation, hook denial, progress, recovery, context, plugin, app, media 상태가 표면마다 다른 의미로 보이지 않게 한다.
2. Interactive TUI, REPL, onboard wizard가 CLI와 local API의 계약을 새로 만들지 않고 같은 command와 projection model을 소비하게 한다.
3. Readiness, degraded health, reconnect, backpressure, dropped event accounting을 사용자에게 숨기지 않는다.
4. Release runner, coverage matrix, lifecycle smoke evidence가 실제 사용자 표면과 연결되게 한다.
5. Self-hosted / personal-use 사용자가 관리자 dashboard 없이도 설치, 실행, 진단, 복구, release readiness를 확인할 수 있게 한다.
6. 검증된 transport gap에만 capability negotiation을 추가하고, reconnect는 snapshot을 먼저 확정한 뒤 delta를 적용하며, 별도 truth store 없이 goal·child·workflow·recovery를 read-only Tasks view로 모은다.

핵심 문장:

```text
어떤 표면에서 보든 같은 runtime 사실은 같은 의미로 보여야 하며, 보이지 않는 손실과 표면별 임시 성공은 release evidence가 될 수 없다.
```

## 구현된 기준선

이 문서가 받는 origin spec의 닫힌 범위는 다음을 기준선으로 본다.

1. 001은 session kernel, turn lock, checkpoint, recovery evidence의 current mapping을 닫았다.
2. 011은 subagent runtime과 parent boundary inheritance의 current scope를 닫았다.
3. 012는 process-local bus, channel worker, follow-up queue, bounded WebSocket delivery와 runtime metadata hint baseline을 닫았다.
4. 013은 CLI, TUI, local API, session UX의 기본 surface 의미를 정의했다.
5. 014는 observability, diagnostics, inspection의 기본 계약을 정의했다.
6. 016은 verification matrix, release gate, release candidate smoke의 기준을 정의했다.
7. 021은 app maker와 app authoring의 draft baseline 및 open app maker 범위를 분리했다.
8. 023은 official Docker/Compose fail-closed sandbox lane과 release evidence를 닫았다. 이 lane은 전체 runtime containment를 뜻하지 않는다.
9. 025는 plugin/hook inspect, management CLI, extension diagnostics의 local manifest scope를 닫았다.
10. 026은 context file discovery와 inline reference live provider handoff의 current scope를 닫았다.
11. 027은 attachment intake, stored attachment, media context routing, analyzer handoff의 v1 scope를 닫았다.

이 기준선은 표면 parity가 자동으로 닫혔다는 뜻이 아니다. 035는 닫힌 runtime 사실을 shared projection model로 묶고, interactive TUI/REPL/onboard wizard, approval/progress/recovery parity, readiness/degraded health, reconnect/backpressure/drop accounting, release runner evidence를 구현 범위로 받았다. Prime에서 선택한 transport negotiation, snapshot-first reconnect, read-only Tasks/goal projection은 새 planned scope다. 최종 closure는 planned scope와 아래 외부 owner evidence가 모두 통과할 때까지 열려 있다.

## 소유하는 open scope

035는 다음 계약을 소유한다.

1. CLI, TUI, local API, WebSocket, external channel이 공유하는 projection schema와 status vocabulary.
2. Interactive TUI, REPL, onboard wizard의 command flow, validation, recovery, cancellation semantics.
3. Durable approval prompt, ephemeral confirmation, hook denial, progress event, stop/restart/recover state, pending follow-up, degraded state의 surface parity.
4. Context file, inline reference, plugin, hook, app, attachment, generated or uploaded media projection.
5. Readiness and degraded health model for local runtime, channel worker, provider auth, plugin/app readiness, adapter별 sandbox/runtime control, storage, queue state.
6. 029/033이 생산하는 reconnect, backpressure, bounded queue, dropped event, channel delivery 상태의 projection과 accounting vocabulary.
7. 입증된 transport gap에 대한 handshake/capability negotiation과 unsupported mutation의 실행 전 차단.
8. Opaque generation/sequence 기준 snapshot-first reconnect와 이후 delta ordering.
9. 기존 owner record locator를 모으는 read-only Tasks/goal projection. 별도 task DB나 mutation authority는 만들지 않는다.
10. Release runner, coverage matrix, lifecycle smoke evidence, projection parity smoke, local install/start/diagnose/recover evidence.

Domain owner와 projection owner는 분리한다. 029는 queue/recovery runtime state와 durable redaction을, 030은 trusted runtime profile·hook denial·process control·sandbox mode·credential status·resource/data disclosure를, AgentLoop/session은 durable approval lineage를, 031은 config/profile auth source와 execution snapshot을, 032는 app/resource lifecycle state와 receipt를, 033은 evaluation/automation state와 coverage entry/review artifact를, 034는 media/analyzer state와 evidence를 생산한다. 035는 그 상태를 CLI/TUI/API/channel에 투영하는 shared adapter, parity smoke, release runner shell만 소유한다.

## Invariants

1. Projection은 session truth를 만들지 않는다. Projection은 owner runtime record를 읽고 같은 의미의 view를 만든다.
2. CLI, TUI, local API, channel reply는 같은 상태를 서로 반대 의미로 표시하면 안 된다.
3. Durable approval state는 pending, allowed, denied, expired, skipped, retry-consumed vocabulary를 쓴다. Spec 030의 confirmation과 `tool:before` veto는 각각 `ephemeral_confirmation`, `hook_denied`로 분리하며 durable approval이나 remembered allow로 승격하지 않는다.
4. Progress event는 손실 가능성이 있으면 손실 가능성을 표시해야 하며, final outcome과 혼동되면 안 된다.
5. Recovery projection은 실제 checkpoint, marker, session state와 연결되어야 한다.
6. Readiness는 단순 process alive가 아니다. Provider auth, storage, channel worker, adapter별 sandbox/runtime control, plugin/app readiness, queue health가 각각 설명되어야 한다. Sandbox active는 표시된 adapter 범위의 evidence일 뿐이다.
7. Degraded health는 성공으로 포장하지 않는다. 사용자가 계속 쓸 수 있어도 어떤 기능이 제한됐는지 보여야 한다.
8. Backpressure와 dropped event는 무음 처리하지 않는다.
9. Release evidence는 실제 사용자 surface를 통과한 smoke 결과와 coverage locator를 가져야 한다.
10. Projection parity는 visual design system 완료를 뜻하지 않는다.
11. Spec 035 redaction은 사용자-facing projection serialization boundary에 한정된다. Session, log, trace, tool output, extension data 전체가 secret-safe하다고 주장하지 않고 raw-content 가능성과 disclosure status를 숨기지 않는다.
12. Resource source, precedence, collision, digest, enabled/readiness는 provenance와 load 결과다. Permission approval, malicious-code absence, sandbox proof가 아니다.
13. Snapshot-first reconnect는 owner snapshot을 새 truth로 만들지 않으며, read-only Tasks view는 모든 row에 owner locator를 가져야 한다.

## Must Have

1. Shared projection schema는 session, turn, subagent, durable approval, ephemeral confirmation, hook denial, tool, trusted runtime, process control, sandbox, credential status, resource provenance, data disclosure, context, plugin, app, media, diagnostics, release evidence를 같은 vocabulary로 표현해야 한다.
2. CLI command output, TUI state, local API response, WebSocket event, channel reply는 같은 projection source를 소비해야 한다.
3. Interactive TUI는 active session, pending durable approval, ephemeral confirmation, hook denial, progress, recovery action, degraded health를 보여야 한다. Confirmation은 현재 호출에만 적용되고 headless에서 필요한 confirmation을 auto-allow하지 않는다.
4. REPL은 CLI와 같은 command router 또는 동등한 command contract를 사용해야 하며, running turn 중 priority command 의미를 보존해야 한다.
5. Onboard wizard는 config stub, credential source 또는 local auth entry setup, channel/app/plugin readiness를 안내해야 한다. Raw credential 입력이 필요하면 masked input으로 auth owner에 전달하고 wizard projection, fixture, diagnostics, wizard persistence에는 저장하지 않는다. 이후 local auth persistence 여부는 Spec 030이 소유한다.
6. Approval/progress/recovery parity tests는 CLI, TUI, local API, WebSocket 또는 channel surface 중 지원 표면을 명시하고 같은 outcome을 확인해야 한다.
7. Context/plugin/app/media projection은 included, skipped, blocked, degraded, missing, unsupported, extraction_failed 같은 reason을 표면별로 보존해야 한다.
8. Readiness health는 ready, degraded, blocked, unavailable, unknown 같은 bounded state와 safe reason을 가져야 하며 trusted profile, adapter별 process control, sandbox scope/fallback, credential status, resource/data disclosure를 독립 fact로 보존해야 한다.
9. Reconnect/backpressure/drop accounting은 queue depth, coalesced event, dropped progress, final outcome delivery 여부를 분리해 기록해야 한다.
10. Release runner는 coverage matrix와 lifecycle smoke evidence를 machine-readable artifact와 human-readable summary로 남겨야 한다.
11. Transport handshake는 기존 schema-version rejection을 재구현하지 않고 실제 capability gap이 확인된 mutation만 실행 전에 차단해야 한다.
12. Reconnect는 capability가 지원될 때 snapshot을 먼저 확정하고 동일 generation의 이후 delta만 적용해야 한다.
13. Read-only Tasks/goal view는 child, workflow, goal, recovery 상태를 owner locator와 함께 집계하고 mutation을 기존 command owner로 되돌려야 한다.

## Must Not Have

1. Visual design system, theme polish, layout system을 035 완료 조건으로 삼지 않는다.
2. Mobile app을 기본 표면으로 추가하지 않는다.
3. SaaS dashboard 또는 관리자 dashboard를 기본 projection owner로 두지 않는다.
4. 특정 CI vendor 선택을 release evidence 계약에 넣지 않는다.
5. CLI, TUI, local API가 각자 status 이름을 새로 만들어 같은 상태를 다르게 표시하게 하지 않는다.
6. Channel progress가 dropped 됐는데 final answer만 갔다고 무손실로 보고하지 않는다.
7. Onboard wizard가 raw credential을 projection, fixture, diagnostics, wizard persistence에 저장하지 않는다. Masked input은 auth owner handoff 뒤 즉시 버리고 source/status만 표시한다. Auth owner의 정의된 local persistence를 전역 비저장으로 재정의하지 않는다.
8. TUI mock 화면만으로 runtime parity를 완료 처리하지 않는다.
9. Release runner가 cargo test green만으로 lifecycle smoke를 대체하지 않는다.
10. Complete redaction, universal sandbox/process envelope, durable confirmation, typed secret reference, resource digest 기반 authorization을 projection이 새 보장으로 만들지 않는다.
11. Tasks view나 reconnect snapshot이 별도 session/task truth 또는 mutation authority가 되게 하지 않는다.

## Acceptance Criteria

1. Shared projection model이 CLI, TUI, local API, WebSocket/channel adapter 중 구현된 표면에서 같은 state vocabulary를 출력한다.
2. Durable approval pending/expiry/denial/retry-consumed는 owner lineage가 있을 때 같은 opaque lineage로 확인된다. Ephemeral confirmation과 hook denial은 별도 상태로 표시되고 approval id, expiry, retry-consumed를 만들지 않는다.
3. Progress stream은 coalescing, reconnect, slow consumer, dropped progress, final outcome delivery를 구분해 accounting한다.
4. Recovery surface는 interrupted run, pending marker, restart marker, recover command, session checkpoint를 같은 projection schema로 보여 준다.
5. Context file, inline reference, plugin, app, attachment, media projection은 included/skipped/blocked/degraded reason, safe resource reference, trusted-code/data-disclosure summary를 raw credential이나 payload 없이 보여 준다.
6. Readiness endpoint와 diagnostics command가 provider auth, storage, adapter별 sandbox/runtime control, channel worker, plugin/app readiness, queue health를 같은 severity로 표현한다.
7. Interactive TUI, REPL, onboard wizard는 mocked data가 아니라 runtime projection source 또는 recorded release fixture를 소비한다.
8. Release runner가 coverage matrix, lifecycle smoke, projection parity smoke, failure triage summary를 artifact로 남긴다.
9. Closure evidence가 old specs의 closed scope와 충돌하지 않고, 035가 표면 parity의 새 owner임을 old specs가 링크할 수 있다.
10. Handshake matrix가 supported/unsupported capability를 구분하고 unsupported mutation을 side effect 전에 거부한다.
11. Reconnect test가 snapshot-before-delta ordering, generation mismatch rejection, connection-local backpressure/drop accounting을 검증한다.
12. Tasks/goal view가 owner locator 없는 상태를 만들지 않고 CLI, local API, TUI에서 같은 goal id, stop reason, continuation budget을 보존한다.

## Source Handoff Table

| Origin spec | 닫힌 범위 | 035로 넘어온 open 계약 |
|---|---|---|
| 001 session kernel | Turn lock, checkpoint, session recovery mapping | Session and recovery projection parity across surfaces |
| 011 subagent runtime | Child runtime inheritance and tool restriction | Subagent progress, result, ceiling, failure projection parity |
| 012 runtime services | Process-local bus, channel workers, follow-up queue, backpressure baseline | Runtime/channel state의 shared projection, reconnect/backpressure/drop accounting vocabulary |
| 013 user interfaces and session UX | Core interface meanings and session UX baseline | Shared CLI/TUI/API/channel projection schema and interactive flows |
| 014 observability diagnostics | Diagnostics and inspection principle | Readiness/degraded health, projection-boundary disclosure/redaction parity, bundle evidence |
| 016 verification matrix and release gates | Verification family and release gate language | Release runner, coverage matrix, lifecycle smoke artifact contract |
| 021 app maker and app authoring | Draft app authoring baseline and open app maker split | 032가 생산하는 app state/evidence의 shared adapter와 surface parity |
| 023 zero-setup sandbox execution | Official Docker/Compose fail-closed sandbox lane and packaging smoke | 해당 lane의 sandbox state/scope 표시. 전체 runtime containment로 확대하지 않음 |
| 025 hooks and plugins | Plugin/hook inspect and local manifest scope | Plugin readiness, hook diagnostics, extension projection parity |
| 026 context files and inline references | Context discovery and live provider handoff | Context reference projection parity across CLI/TUI/API/channel |
| 027 channel attachment intake | Stored attachment and file context routing | 034가 생산하는 media/analyzer state의 shared adapter와 channel parity |

## Implementation PRDs

Spec 035는 아래 PRD를 구현하고 검증한다. PRD 007은 새 domain contract를 정의하지 않는 sequential integration and closure gate이며, planned PRD 008-009의 evidence도 소비한다.

이 스펙의 기존 구현 식별자와 증거 locator(`Spec031*`, `spec031.*`, `spec031-release-runner`, `.omo/evidence/spec031/**`)는 shipped compatibility surface이므로 재번호화하지 않는다. 문서의 semantic owner 번호와 탐색 경로만 035로 바꾼다.

| PRD | Sole owner scope | Depends on |
|---|---|---|
| [PRD 000](prds/000-shared-projection-model-and-vocabulary.md) | Shared typed projection model, bounded vocabulary, versioning, redaction boundary | Existing owner records |
| [PRD 001](prds/001-surface-adapter-parity-cli-api-channel.md) | CLI, local API, WebSocket, external channel adapter parity | PRD 000 |
| [PRD 002](prds/002-approval-progress-and-recovery-parity.md) | Approval lineage, hook denial, progress/final distinction, recovery projection parity | PRDs 000-001, Spec 029, AgentLoop/session approval facts, Spec 030 hook facts |
| [PRD 003](prds/003-readiness-degraded-health-and-diagnostics.md) | Component readiness, degraded health aggregation, diagnostics parity | PRDs 000-001, component owner observations |
| [PRD 004](prds/004-context-extension-app-and-media-projection.md) | Context, plugin/hook, app, attachment/media projection and reason parity | PRDs 000-001, 003, Specs 032 and 034 evidence |
| [PRD 005](prds/005-interactive-tui-repl-and-onboard-flows.md) | Interactive TUI, REPL command parity, onboard credential-source/readiness flows | PRDs 000-004, Specs 030 and 031 owner facts |
| [PRD 006](prds/006-reconnect-backpressure-and-drop-accounting.md) | Reconnect, backpressure, coalescing, drop, final-delivery accounting | PRDs 000-002, Specs 029 and 033 evidence |
| [PRD 007](prds/007-release-runner-and-spec035-closure.md) | Release runner, coverage/lifecycle/parity smoke, failure triage, final closure | PRDs 000-006, 008-009 and all required external evidence |
| [PRD 008](prds/008-transport-capability-and-snapshot-first-reconnect.md) | 최소 capability negotiation, snapshot-first reconnect ordering | PRDs 000-001, 006, Specs 029 and 031 facts |
| [PRD 009](prds/009-goal-and-read-only-tasks-projection.md) | Goal accounting parity와 owner-backed read-only Tasks view | PRDs 000-001, 005, Spec 033 facts |

Current PRD status:

| PRD | Status | Evidence |
|---|---|---|
| PRD 000 | Planned revision (implemented baseline) | Existing shared model evidence remains; confirmation/hook/runtime-disclosure vocabulary revision is unverified. |
| PRD 001 | Planned revision (implemented baseline) | Existing adapter parity evidence remains; new disclosure/sandbox-scope parity is unverified. |
| PRD 002 | Planned revision (implemented baseline) | Existing approval/progress/recovery evidence remains; confirmation/hook separation is unverified. |
| PRD 003 | Planned revision (implemented baseline) | Existing readiness evidence remains; adapter-scoped sandbox/runtime-control revision is unverified. |
| PRD 004 | Planned revision (implemented baseline) | Existing context/extension/media evidence remains; provenance/trusted-code/disclosure fields are unverified. |
| PRD 005 | Planned revision (implemented baseline) | Existing TUI/REPL/onboard evidence remains; ephemeral confirmation and expanded diagnostics are unverified. |
| PRD 006 | Planned revision (implemented baseline) | Existing accounting evidence remains; owner-scoped final-delivery wording requires revision verification. |
| PRD 007 | Planned revision (implemented baseline) | Existing runner artifacts remain; PRD 008-009 and revised owner facts are not integrated. |
| PRD 008 | Planned | Prime 분석이 확인한 protocol gap 검증과 snapshot-first reconnect 계약. |
| PRD 009 | Planned | Goal accounting parity와 owner-backed read-only Tasks view 계약. |

Dependency rules:

1. PRD 000의 canonical vocabulary가 닫히기 전에 surface adapter가 private status contract를 추가하면 안 된다.
2. PRD 001은 interactive flow를 소유하지 않고, PRD 005는 adapter/domain contract를 재정의하지 않는다.
3. PRD 002의 terminal outcome 의미와 PRD 006의 delivery accounting은 분리한다. Progress drop과 final delivery는 동시에 참일 수 있다.
4. Spec 030/031/032/033/034가 생산해야 하는 evidence가 없으면 해당 adapter는 blocked 또는 unavailable 상태와 safe reason을 기록할 수 있지만 Spec 035 final closure는 통과할 수 없다. Spec 030의 evidence는 trusted runtime profile, hook denial, ephemeral confirmation, adapter별 process control, sandbox mode/scope, credential status, resource diagnostics/data disclosure이며 durable approval, complete redaction, universal containment로 재해석하지 않는다.
5. PRD 008은 기존 002/015 schema fence를 재소유하지 않고 입증된 transport gap만 닫는다. PRD 009는 owner state를 읽기만 하며 별도 task store를 만들지 않는다.
6. PRD 007의 dependency DAG, requirement mapping, real-surface QA, artifact-backed audit가 모두 통과하기 전에는 이 문서의 status를 변경하지 않는다.

## Closure Evidence

035는 아래 증거가 모두 연결될 때 닫을 수 있다.

1. Projection schema evidence: shared typed projection model과 status vocabulary가 표면별 adapter보다 안쪽에 있다.
2. CLI evidence: status, diagnostics, approval, recover, context/plugin/app/media projection command가 shared model을 출력한다.
3. TUI evidence: interactive TUI가 pending approval, progress, degraded health, recovery action을 실제 runtime projection으로 표시한다.
4. REPL and onboard evidence: REPL command parity와 onboard wizard credential-source/readiness flow가 masked handoff와 status-only projection으로 검증된다.
5. API and channel evidence: local API, WebSocket, external channel reply가 같은 projection status와 redacted reason을 보존한다.
6. Backpressure evidence: reconnect, slow consumer, bounded queue, coalesced progress, dropped event counter, final outcome delivery case가 테스트된다.
7. Release evidence: release runner artifact가 coverage matrix, lifecycle smoke, projection parity smoke, failure triage, command locator를 포함한다.
8. Documentation evidence: old specs가 035를 open owner로 링크해도 UI mock, SaaS dashboard, mobile app, CI vendor 선택으로 범위가 넓어지지 않는다.
9. Transport/reconnect evidence: capability matrix와 snapshot-before-delta ordering이 실제 adapter surface에서 검증된다.
10. Goal/Tasks evidence: CLI, local API, TUI가 동일 goal accounting과 owner-backed read-only task rows를 표시한다.

## Current Closure Blockers

The historical 2026-08-04 closure evidence is indexed by `.omo/evidence/spec031/prd007/task-21-spec031-implementation/current-final-20260804-12/manifest.json`. Spec 030의 current owner audit은 source-bound `.omo/evidence/spec030/prd006/current-worktree-final/closure-manifest.json`으로 갱신되었으며, 아래 표에서 historical Spec 030 row를 대체한다.

| Owner | Verdict | Locator | Reason |
|---|---|---|---|
| Spec 029 | PASS | `.omo/evidence/spec031/prd007/task-21-spec031-implementation/current-final-20260804-12/external/spec029-read-audit.md` | Artifact-backed exact fact audit passes. |
| Spec 030 | PASS | `.omo/evidence/spec030/prd006/current-worktree-final/closure-manifest.json` | Source-bound trusted-runtime owner facts, CLI/TUI/API parity, process controls, credential lifecycle, bwrap lifecycle, manual QA and cleanup gates pass. |
| Spec 032 | BLOCKED | `.omo/evidence/spec031/prd007/task-21-spec031-implementation/current-final-20260804-12/external/spec032-read-audit.md` | Required app/resource lifecycle owner-fact artifacts are absent. |
| Spec 033 | BLOCKED | `.omo/evidence/spec031/prd007/task-21-spec031-implementation/current-final-20260804-12/external/spec033-read-audit.md` | Required automation/evaluation owner-fact artifacts are absent. |
| Spec 034 | BLOCKED | `.omo/evidence/spec031/prd007/task-21-spec031-implementation/current-final-20260804-12/external/spec034-read-audit.md` | Required media/analyzer owner-fact artifacts are absent. |
| Spec 031 | BLOCKED | `.omo/evidence/spec031/prd007/task-21-spec031-implementation/current-final-20260804-12/external/spec035-read-audit.md` | Required config/auth-source/execution-snapshot owner-fact artifacts are absent. The historical artifact filename remains unchanged. |

Historical release runner locator `.omo/evidence/spec031/prd007/task-21-spec031-implementation/current-final-20260804-12/summary.md` reports `status: BLOCKED`. Its historical Spec 030 read-audit row is superseded by the current PASS locator above; Specs031/032/033/034와 planned PRD 008-009는 계속 Spec 035 closure를 차단한다. `failure-triage.json`의 dirty state는 당시 기록이며 external evidence blocker를 가리지 않는다. The success fixture at `.omo/evidence/spec031/prd007/task-20-spec031-implementation/success-fixture-final-20260804-05/summary.md` proves the runner can pass an isolated fixture, not that semantic Spec 035 closure has passed.
