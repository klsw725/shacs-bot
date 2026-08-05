# 031. UI projection, diagnostics, and release evidence parity 아키텍처 명세

Status: Open (implemented, closure blocked)

Current machine verdict: Spec 031 implementation surfaces are present and 56 release coverage rows are mapped, but final closure is blocked by external owner evidence. The current release runner exits nonzero with `BlockedExternalEvidence` while preserving dirty-worktree triage separately, so this spec must stay Open.

Origin specs: 001, 011, 012, 013, 014, 016, 021, 023, 025, 026, 027

## 문서 목적

이 문서는 기존 spec들이 닫은 core runtime 계약을 사용자가 실제 표면에서 같은 의미로 볼 수 있게 하는 open owner boundary다. CLI, TUI, local API, external channel은 서로 다른 UI가 아니라 같은 session truth와 runtime evidence를 투영하는 projection surface다.

목표는 다음과 같다.

1. Session, subagent, approval, progress, recovery, context, plugin, app, media 상태가 표면마다 다른 의미로 보이지 않게 한다.
2. Interactive TUI, REPL, onboard wizard가 CLI와 local API의 계약을 새로 만들지 않고 같은 command와 projection model을 소비하게 한다.
3. Readiness, degraded health, reconnect, backpressure, dropped event accounting을 사용자에게 숨기지 않는다.
4. Release runner, coverage matrix, lifecycle smoke evidence가 실제 사용자 표면과 연결되게 한다.
5. Self-hosted / personal-use 사용자가 관리자 dashboard 없이도 설치, 실행, 진단, 복구, release readiness를 확인할 수 있게 한다.

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
8. 023은 containment evidence와 release evidence lane을 닫았다.
9. 025는 plugin/hook inspect, management CLI, extension diagnostics의 local manifest scope를 닫았다.
10. 026은 context file discovery와 inline reference live provider handoff의 current scope를 닫았다.
11. 027은 attachment intake, stored attachment, media context routing, analyzer handoff의 v1 scope를 닫았다.

이 기준선은 표면 parity가 자동으로 닫혔다는 뜻이 아니다. 031은 닫힌 runtime 사실을 shared projection model로 묶고, interactive TUI/REPL/onboard wizard, approval/progress/recovery parity, readiness/degraded health, reconnect/backpressure/drop accounting, release runner evidence를 구현 범위로 받았다. 최종 closure는 아래 외부 owner evidence가 모두 통과할 때까지 열려 있다.

## 소유하는 open scope

031은 다음 계약을 소유한다.

1. CLI, TUI, local API, WebSocket, external channel이 공유하는 projection schema와 status vocabulary.
2. Interactive TUI, REPL, onboard wizard의 command flow, validation, recovery, cancellation semantics.
3. Approval prompt, progress event, stop/restart/recover state, pending follow-up, degraded state의 surface parity.
4. Context file, inline reference, plugin, hook, app, attachment, generated or uploaded media projection.
5. Readiness and degraded health model for local runtime, channel worker, provider auth, plugin/app readiness, containment, storage, queue state.
6. 029/033이 생산하는 reconnect, backpressure, bounded queue, dropped event, channel delivery 상태의 projection과 accounting vocabulary.
7. Release runner, coverage matrix, lifecycle smoke evidence, projection parity smoke, local install/start/diagnose/recover evidence.

Domain owner와 projection owner는 분리한다. 029는 queue/recovery runtime state를, 030은 approval/policy/redaction/containment evidence를, 032는 app state와 receipt를, 033은 evaluation/automation state와 coverage entry/review artifact를, 034는 media/analyzer state와 evidence를, 035는 config/profile/secret-ref consumption과 execution snapshot을 생산한다. 031은 그 상태를 CLI/TUI/API/channel에 투영하는 shared adapter, parity smoke, release runner shell만 소유하며 domain state transition, config/persistence contract, evidence 생성 규칙을 다시 소유하지 않는다.

## Invariants

1. Projection은 session truth를 만들지 않는다. Projection은 owner runtime record를 읽고 같은 의미의 view를 만든다.
2. CLI, TUI, local API, channel reply는 같은 상태를 서로 반대 의미로 표시하면 안 된다.
3. Approval state는 pending, allowed, denied, expired, skipped, retry-consumed 같은 공통 vocabulary를 써야 한다.
4. Progress event는 손실 가능성이 있으면 손실 가능성을 표시해야 하며, final outcome과 혼동되면 안 된다.
5. Recovery projection은 실제 checkpoint, marker, session state와 연결되어야 한다.
6. Readiness는 단순 process alive가 아니다. Provider auth, storage, channel worker, containment, plugin/app readiness, queue health가 각각 설명되어야 한다.
7. Degraded health는 성공으로 포장하지 않는다. 사용자가 계속 쓸 수 있어도 어떤 기능이 제한됐는지 보여야 한다.
8. Backpressure와 dropped event는 무음 처리하지 않는다.
9. Release evidence는 실제 사용자 surface를 통과한 smoke 결과와 coverage locator를 가져야 한다.
10. Projection parity는 visual design system 완료를 뜻하지 않는다.

## Must Have

1. Shared projection schema는 session, turn, subagent, approval, tool, context, plugin, app, media, diagnostics, release evidence를 같은 vocabulary로 표현해야 한다.
2. CLI command output, TUI state, local API response, WebSocket event, channel reply는 같은 projection source를 소비해야 한다.
3. Interactive TUI는 active session, pending approval, progress, recovery action, degraded health를 조작 가능한 상태로 보여야 한다.
4. REPL은 CLI와 같은 command router 또는 동등한 command contract를 사용해야 하며, running turn 중 priority command 의미를 보존해야 한다.
5. Onboard wizard는 config stub, secret ref placeholder, channel/app/plugin readiness를 raw secret 없이 안내해야 한다.
6. Approval/progress/recovery parity tests는 CLI, TUI, local API, WebSocket 또는 channel surface 중 지원 표면을 명시하고 같은 outcome을 확인해야 한다.
7. Context/plugin/app/media projection은 included, skipped, blocked, degraded, missing, unsupported, extraction_failed 같은 reason을 표면별로 보존해야 한다.
8. Readiness health는 ready, degraded, blocked, unavailable, unknown 같은 bounded state와 redacted reason을 가져야 한다.
9. Reconnect/backpressure/drop accounting은 queue depth, coalesced event, dropped progress, final outcome delivery 여부를 분리해 기록해야 한다.
10. Release runner는 coverage matrix와 lifecycle smoke evidence를 machine-readable artifact와 human-readable summary로 남겨야 한다.

## Must Not Have

1. Visual design system, theme polish, layout system을 031 완료 조건으로 삼지 않는다.
2. Mobile app을 기본 표면으로 추가하지 않는다.
3. SaaS dashboard 또는 관리자 dashboard를 기본 projection owner로 두지 않는다.
4. 특정 CI vendor 선택을 release evidence 계약에 넣지 않는다.
5. CLI, TUI, local API가 각자 status 이름을 새로 만들어 같은 상태를 다르게 표시하게 하지 않는다.
6. Channel progress가 dropped 됐는데 final answer만 갔다고 무손실로 보고하지 않는다.
7. Onboard wizard가 secret 값을 수집하거나 저장했다는 듯 표시하지 않는다. Secret value와 secret ref placeholder는 분리한다.
8. TUI mock 화면만으로 runtime parity를 완료 처리하지 않는다.
9. Release runner가 cargo test green만으로 lifecycle smoke를 대체하지 않는다.

## Acceptance Criteria

1. Shared projection model이 CLI, TUI, local API, WebSocket/channel adapter 중 구현된 표면에서 같은 state vocabulary를 출력한다.
2. Pending approval, approval expiry, denial, retry consumed 상태가 CLI와 interactive surface에서 같은 action id 또는 digest lineage로 확인된다.
3. Progress stream은 coalescing, reconnect, slow consumer, dropped progress, final outcome delivery를 구분해 accounting한다.
4. Recovery surface는 interrupted run, pending marker, restart marker, recover command, session checkpoint를 같은 projection schema로 보여 준다.
5. Context file, inline reference, plugin, app, attachment, media projection은 included/skipped/blocked/degraded reason을 raw path나 secret 없이 보여 준다.
6. Readiness endpoint와 diagnostics command가 provider auth, storage, containment, channel worker, plugin/app readiness, queue health를 같은 severity로 표현한다.
7. Interactive TUI, REPL, onboard wizard는 mocked data가 아니라 runtime projection source 또는 recorded release fixture를 소비한다.
8. Release runner가 coverage matrix, lifecycle smoke, projection parity smoke, failure triage summary를 artifact로 남긴다.
9. Closure evidence가 old specs의 closed scope와 충돌하지 않고, 031이 표면 parity의 새 owner임을 old specs가 링크할 수 있다.

## Source Handoff Table

| Origin spec | 닫힌 범위 | 031로 넘어온 open 계약 |
|---|---|---|
| 001 session kernel | Turn lock, checkpoint, session recovery mapping | Session and recovery projection parity across surfaces |
| 011 subagent runtime | Child runtime inheritance and tool restriction | Subagent progress, result, ceiling, failure projection parity |
| 012 runtime services | Process-local bus, channel workers, follow-up queue, backpressure baseline | Runtime/channel state의 shared projection, reconnect/backpressure/drop accounting vocabulary |
| 013 user interfaces and session UX | Core interface meanings and session UX baseline | Shared CLI/TUI/API/channel projection schema and interactive flows |
| 014 observability diagnostics | Diagnostics and inspection principle | Readiness/degraded health, redacted diagnostics parity, bundle evidence |
| 016 verification matrix and release gates | Verification family and release gate language | Release runner, coverage matrix, lifecycle smoke artifact contract |
| 021 app maker and app authoring | Draft app authoring baseline and open app maker split | 032가 생산하는 app state/evidence의 shared adapter와 surface parity |
| 023 zero-setup sandbox execution | Containment evidence and packaging smoke lane | Containment readiness, degraded state, lifecycle smoke display |
| 025 hooks and plugins | Plugin/hook inspect and local manifest scope | Plugin readiness, hook diagnostics, extension projection parity |
| 026 context files and inline references | Context discovery and live provider handoff | Context reference projection parity across CLI/TUI/API/channel |
| 027 channel attachment intake | Stored attachment and file context routing | 034가 생산하는 media/analyzer state의 shared adapter와 channel parity |

## Implementation PRDs

Spec 031은 아래 PRD를 순서대로 구현하고 검증한다. PRD 007은 새 domain contract를 정의하지 않는 유일한 sequential integration and closure gate다.

| PRD | Sole owner scope | Depends on |
|---|---|---|
| [PRD 000](prds/000-shared-projection-model-and-vocabulary.md) | Shared typed projection model, bounded vocabulary, versioning, redaction boundary | Existing owner records |
| [PRD 001](prds/001-surface-adapter-parity-cli-api-channel.md) | CLI, local API, WebSocket, external channel adapter parity | PRD 000 |
| [PRD 002](prds/002-approval-progress-and-recovery-parity.md) | Approval lineage, progress/final distinction, recovery projection parity | PRDs 000-001, Specs 029-030 owner facts |
| [PRD 003](prds/003-readiness-degraded-health-and-diagnostics.md) | Component readiness, degraded health aggregation, diagnostics parity | PRDs 000-001, component owner observations |
| [PRD 004](prds/004-context-extension-app-and-media-projection.md) | Context, plugin/hook, app, attachment/media projection and reason parity | PRDs 000-001, 003, Specs 032 and 034 evidence |
| [PRD 005](prds/005-interactive-tui-repl-and-onboard-flows.md) | Interactive TUI, REPL command parity, onboard secret-ref/readiness flows | PRDs 000-004, Specs 030 and 035 owner facts |
| [PRD 006](prds/006-reconnect-backpressure-and-drop-accounting.md) | Reconnect, backpressure, coalescing, drop, final-delivery accounting | PRDs 000-002, Specs 029 and 033 evidence |
| [PRD 007](prds/007-release-runner-and-spec031-closure.md) | Release runner, coverage/lifecycle/parity smoke, failure triage, final closure | PRDs 000-006 and all required external evidence |

Current PRD status:

| PRD | Status | Evidence |
|---|---|---|
| PRD 000 | Implemented, closure blocked | Shared projection model is covered by Spec031 release matrix rows. |
| PRD 001 | Implemented, closure blocked | CLI, API, WebSocket, and channel adapter parity evidence is mapped into the release matrix. |
| PRD 002 | Implemented, closure blocked | Approval, progress, and recovery parity evidence is mapped into the release matrix. |
| PRD 003 | Implemented, closure blocked | Readiness, degraded health, and diagnostics parity evidence is mapped into the release matrix. |
| PRD 004 | Implemented, closure blocked | Context, extension, app, and media projection evidence is present, with external-owner blockers preserved. |
| PRD 005 | Implemented, closure blocked | Verified TUI, REPL, and secret-ref onboard wizard evidence is mapped into the release matrix. |
| PRD 006 | Implemented, closure blocked | Reconnect, backpressure, drop accounting, and final-delivery hint evidence is mapped into the release matrix. |
| PRD 007 | Implemented, closure blocked | Release runner artifacts exist, but the current closure run is `BLOCKED`. |

Dependency rules:

1. PRD 000의 canonical vocabulary가 닫히기 전에 surface adapter가 private status contract를 추가하면 안 된다.
2. PRD 001은 interactive flow를 소유하지 않고, PRD 005는 adapter/domain contract를 재정의하지 않는다.
3. PRD 002의 terminal outcome 의미와 PRD 006의 delivery accounting은 분리한다. Progress drop과 final delivery는 동시에 참일 수 있다.
4. Spec 030/032/033/034/035가 생산해야 하는 evidence가 없으면 해당 adapter는 blocked 또는 unavailable 상태와 safe reason을 기록할 수 있지만 Spec 031 final closure는 통과할 수 없다.
5. PRD 007의 dependency DAG, requirement mapping, real-surface QA, artifact-backed audit가 모두 통과하기 전에는 이 문서의 status를 변경하지 않는다.

## Closure Evidence

031은 아래 증거가 모두 연결될 때 닫을 수 있다.

1. Projection schema evidence: shared typed projection model과 status vocabulary가 표면별 adapter보다 안쪽에 있다.
2. CLI evidence: status, diagnostics, approval, recover, context/plugin/app/media projection command가 shared model을 출력한다.
3. TUI evidence: interactive TUI가 pending approval, progress, degraded health, recovery action을 실제 runtime projection으로 표시한다.
4. REPL and onboard evidence: REPL command parity와 onboard wizard secret-ref/readiness flow가 raw secret 없이 검증된다.
5. API and channel evidence: local API, WebSocket, external channel reply가 같은 projection status와 redacted reason을 보존한다.
6. Backpressure evidence: reconnect, slow consumer, bounded queue, coalesced progress, dropped event counter, final outcome delivery case가 테스트된다.
7. Release evidence: release runner artifact가 coverage matrix, lifecycle smoke, projection parity smoke, failure triage, command locator를 포함한다.
8. Documentation evidence: old specs가 031을 open owner로 링크해도 UI mock, SaaS dashboard, mobile app, CI vendor 선택으로 범위가 넓어지지 않는다.

## Current Closure Blockers

The current closure evidence is indexed by `.omo/evidence/spec031/prd007/task-21-spec031-implementation/current-final-20260804-12/manifest.json`.

| Owner | Verdict | Locator | Reason |
|---|---|---|---|
| Spec 029 | PASS | `.omo/evidence/spec031/prd007/task-21-spec031-implementation/current-final-20260804-12/external/spec029-read-audit.md` | Artifact-backed exact fact audit passes. |
| Spec 030 | BLOCKED | `.omo/evidence/spec031/prd007/task-21-spec031-implementation/current-final-20260804-12/external/spec030-read-audit.md` | Spec030 remains `Status: Open`; final closure evidence is absent. |
| Spec 032 | BLOCKED | `.omo/evidence/spec031/prd007/task-21-spec031-implementation/current-final-20260804-12/external/spec032-read-audit.md` | Spec032 remains `Status: Open`; closure facts are absent. |
| Spec 033 | BLOCKED | `.omo/evidence/spec031/prd007/task-21-spec031-implementation/current-final-20260804-12/external/spec033-read-audit.md` | Spec033 remains `Status: Open`; closure facts are absent. |
| Spec 034 | BLOCKED | `.omo/evidence/spec031/prd007/task-21-spec031-implementation/current-final-20260804-12/external/spec034-read-audit.md` | Spec034 remains `Status: Open`; closure facts are absent. |
| Spec 035 | BLOCKED | `.omo/evidence/spec031/prd007/task-21-spec031-implementation/current-final-20260804-12/external/spec035-read-audit.md` | Spec035 remains `Status: Open`; closure facts are absent. |

Release runner locator: `.omo/evidence/spec031/prd007/task-21-spec031-implementation/current-final-20260804-12/summary.md` reports `status: BLOCKED`. `failure-triage.json` lists `triage/dirty-worktree.json` and `triage/blocked-external-evidence.json`; the dirty state is recorded but does not mask the external evidence blocker. The success fixture at `.omo/evidence/spec031/prd007/task-20-spec031-implementation/success-fixture-final-20260804-05/summary.md` proves the runner can pass an isolated fixture, not that Spec031 closure has passed.
