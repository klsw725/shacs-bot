# evaluation, automation, and self-improvement 아키텍처 명세

Status: Complete (Scoped)

Implemented scope: 현재 구현은 PRD 000-014의 Rust contract, runtime helper, projection helper, and release-gate helper scope를 `crates/shacs-eval`, `crates/shacs-core`, `crates/shacs-projection`의 typed model, helper, adapter, and tests로 지원한다.

Open work moved to: [033 evaluation automation live integration](../033-evaluation-automation-live-integration/SPEC.md)

Not carried forward: 개별 provider adapter protocol, tool or MCP server internals, full skill file format, session store physical format, TUI widget design, cron parser choice, checkpoint backend detail, public training data obligation, organization approval workflow는 018 closure에 포함하지 않는다.

## 문서 목적

이 문서는 `docs/SYSTEM-FOUNDATION.md`와 numbered spec set 전체를 바탕으로 `shacs-bot`의 장기 최종 목표인 evaluation, automation, self-improvement 경계를 고정한다.

목표는 다음과 같다.

- 목표 완료 판정, capability 판정, task outcome 판정이 서로 다른 책임을 갖는다는 점을 명확히 한다.
- scheduling, heartbeat, cron, subagent, app task, channel delivery가 같은 자동화 의미론을 소비하도록 묶는다.
- 사용자가 허용한 범위 안에서 제안, 승인, checkpoint, 적용, 검증, 기록, rollback으로 이어지는 자기 개선 흐름을 정의한다.
- evaluator가 조언자라는 점과 `MainOrchestrator`가 여전히 session truth, permission, tool execution의 권한자라는 점을 고정한다.
- Rust contract와 후속 live integration 작업에서 goal evaluator, safety evaluator, task outcome evaluator, automation ledger, replay dataset, UI projection, 진단 테스트를 도출할 수 있게 한다.

이 문서는 MVP 계획서가 아니다. 최종 제품에서 필요한 모든 평가와 자동화 도메인을 한 번에 다루되, 이미 다른 numbered spec이 소유한 개념을 다시 소유하지 않는다. 018은 통합 계약을 소유하고, 하위 spec은 각자의 primitive와 projection을 계속 소유한다.

---

## 상위 기준과의 관계

이 문서는 다음 상위 기준을 전제로 한다.

- `shacs-bot`은 self-hosted / personal-use assistant runtime이다. 기본 주체는 사용자가 직접 설치하고 운영하는 본인이다.
- `MainOrchestrator`는 세션 상태, 턴 상태, tool 실행, permission 확정을 담당하는 유일한 권한자다.
- provider, tool, skill, MCP, memory, session store, runtime service, UI, diagnostics는 각각의 owner spec이 정의한 경계를 가진다.
- 자동화는 사용자를 대신해 무제한으로 움직이는 권한이 아니라, 사용자가 볼 수 있고 멈출 수 있고 되돌릴 수 있는 로컬 실행 계약이다.

따라서 이 문서는 SaaS control plane, 관리자 승인 콘솔, 조직 policy rollout, fleet automation, 멀티테넌트 evaluator 운영을 다루지 않는다.

이 문서의 핵심은 여러 subsystem에 흩어진 평가 결과를 하나의 제품 의미론으로 읽는 것이다. goal evaluator, safety evaluator, outcome evaluator는 모두 `MainOrchestrator`가 소비할 입력을 만들 뿐, 직접 세션 truth를 바꾸거나 권한을 부여하거나 tool을 실행하지 않는다.

---

## reference 채택 원칙

`docs/refs/hermes-agent`는 제품 의미론의 reference다. 복사할 codebase나 Python 구현 목표가 아니다.

| reference | 가져올 것 | 그대로 가져오지 않을 것 |
|---|---|---|
| `docs/refs/hermes-agent` | persistent goal, cron, security approval, checkpoint rollback, memory, skills, curator, MCP, delegation, batch trajectory에서 읽히는 제품 의미론 | Python 구현 방식, 파일 구조, agent 이름, 외부 운영 모델, silent self-modification 관성 |
| 기존 `shacs-bot` specs | owner boundary, `MainOrchestrator` 권한, default-deny safety, local runtime service, CLI/TUI/local API projection | spec 사이 의미 중복, 구현되지 않은 현재 기능 주장, 관리자 또는 조직 운영 가정 |

reference는 질문을 돕는 지도다. 최종 설계의 source of truth는 numbered spec set이며, 018은 evaluation, automation, self-improvement의 통합 의미만 소유한다.

---

## 범위

이 문서는 다음을 정의한다.

- runtime goal/completion evaluator
- safety/capability evaluator
- task outcome evaluator
- scheduled automation
- autonomous improvement flow
- checkpoint/rollback 통합 계약
- memory and session search가 evaluator에 제공하는 증거 경계
- skills progressive disclosure와 curator 흐름
- provider/model routing for evaluators and automation
- MCP/tool exposure and app integration
- offline evaluation, trajectory, replay
- UI/channel projection
- observability and diagnostics
- cross-spec ownership
- 불변식, 정상 시퀀스, 실패 시퀀스, 금지 패턴, 검증 관점

이 문서는 다음을 정의하지 않는다.

- 개별 provider adapter 프로토콜 세부 구현
- tool 또는 MCP server 내부 구현
- skill file format 전체
- session store의 물리 저장 형식
- TUI widget 설계 또는 웹 UI visual design
- cron expression parser 선택
- checkpoint backend 세부 저장소
- public training data 생산 의무
- 조직용 승인 워크플로우

---

## 현재 구현과 final/future boundary

현재 저장소에는 018의 final goal을 뒷받침하는 기반이 이미 여러 spec에 흩어져 있다.

- `shacs-cron`, heartbeat notification evaluator, runtime service wiring은 자동화와 task outcome 평가의 초기 표면이다.
- `MemoryStore`, Dream, session store, subagent runtime은 bounded evidence retrieval과 장기 작업 문맥의 기반이다.
- MCP default-deny, skills registry, channel workers, local API, diagnostics는 노출, projection, 관측의 기반이다.
- 010, 012, 013, 014, 017은 safety, runtime, interface, diagnostics, app operating environment의 주요 하위 경계를 이미 고정한다.

현재 구현 상태: PRD 000-014는 Rust contract, runtime helper, projection helper, release-gate helper 기준으로 구현되어 있다. 주요 근거는 `crates/shacs-eval`, `crates/shacs-core`, `crates/shacs-projection`의 typed model, pure helper, runtime adapter, projection/release helper, and related tests다. 이 closure는 아래 제품 의미론을 코드 계약으로 고정했다는 뜻이며, 모든 live runtime surface가 이미 하나의 end-to-end 제품 흐름으로 연결됐다는 뜻은 아니다.

1. persistent goal과 `/goal` 같은 목표 lifecycle의 formal model.
2. judge verdict `done`, `continue`, `blocked`와 turn budget, pause, resume, clear의 공식 상태.
3. formal capability decision input, approval correlation, stale 또는 expired approval rejection.
4. task outcome evaluator가 `notify`, `suppress`, `continue`, `escalate`, `verify`, `rollback`을 일관되게 반환하는 계약.
5. scheduled automation lifecycle, trigger normalization, run-state/idempotency/recursion guard 계약.
6. memory search, skill disclosure, curator, 자기 개선 approval/checkpoint/apply/verify/rollback, MCP exposure, replay runner, projection, diagnostics release gate의 helper-level integration.
7. offline trajectory와 replay dataset으로 local quality regression을 검증하는 표준.

따라서 018의 현재 완료 의미는 하나의 core evaluator를 추가했다는 뜻도, 최종 제품 흐름 전체가 live로 닫혔다는 뜻도 아니다. 목표 평가, 안전 평가, 결과 평가, 자동화, 자기 개선, rollback, memory, skills, provider routing, MCP exposure, replay, UI projection, diagnostics가 하나의 설명 가능한 제품 계약으로 연결되도록 Rust contract와 helper surface를 고정했다는 뜻이다.

Full live product closure 전에 남은 대표 범위:

- evaluator, automation, self-improvement, replay, projection, diagnostics가 지원되는 `AgentLoop` turn-end, local heartbeat/cron, terminal `SubagentRuntime` producer와 domain input에서 소비되는 scoped live wiring. Channel/API/TUI surface와 Tasks adapter parity는 Spec035가 소유한다.
- PRD 000-014 각각을 직접 추적하는 release coverage entry와 dedicated release runner/evidence artifact.
- `BlockedApproval`, `MissingRedactionEvidence`, duplicate/superseded consumption 같은 release blocker 및 ledger edge-case regression 보강.
- QA/goal/code/security/docs review 결과를 저장소 안에서 재현 가능한 artifact로 남기는 review evidence workflow.

---

## 핵심 정의

### goal

goal은 사용자가 명시적으로 설정한 지속 작업 목표다. 단일 user message가 아니며, 여러 turn, scheduled wake, channel event, subagent result, app task result를 거쳐 유지될 수 있다.

goal은 다음 상태를 가져야 한다.

- active
- paused
- blocked
- done
- cleared

goal 상태 전이는 evaluator가 직접 수행하지 않는다. evaluator는 verdict와 evidence를 제안하고, `MainOrchestrator`가 세션 truth에 반영할지 결정한다.

### completion verdict

completion verdict는 runtime goal에 대한 판정이다.

- `done`: 현재 goal이 충족됐고 추가 turn이 필요하지 않다.
- `continue`: 목표가 아직 남아 있으며 turn budget 안에서 다음 작업이 가능하다.
- `blocked`: 사용자 입력, permission, secret, 외부 시스템, 실패 복구가 필요하다.

verdict는 이유, evidence reference, confidence, 필요한 다음 action hint를 가질 수 있다. verdict만으로 tool이 실행되거나 goal이 닫히면 안 된다.

### capability decision

capability decision은 특정 action이 어떤 capability를 요구하고, 어떤 permission mode와 approval 상태에서 허용될 수 있는지 설명하는 formal input/output이다.

decision은 tool call 직전, scheduled job 실행 전, app task 실행 전, rollback 또는 restore 전, 자기 개선 적용 전에 소비된다.

### task outcome

task outcome은 cron, heartbeat, subagent, app task, channel worker, local API request 같은 비동기 실행 결과를 사용자가 볼 action으로 분류한 것이다.

허용되는 outcome class는 다음과 같다.

- `notify`
- `suppress`
- `continue`
- `escalate`
- `verify`
- `rollback`

### automation job

automation job은 사용자가 설정하거나 runtime이 승인된 흐름 안에서 만든 one-shot 또는 recurring 실행 단위다. job은 skill-backed agent 실행일 수도 있고, agent 없이 script만 실행하는 작업일 수도 있다.

### improvement proposal

improvement proposal은 runtime이 자기 설정, skill, prompt, tool exposure, app manifest, automation rule을 바꾸자고 사용자에게 제안하는 기록이다. proposal은 승인 전까지 효과가 없다.

### checkpoint

checkpoint는 destructive tool 실행, 자기 개선 적용, rollback 가능한 app/task 변경 전에 선택적으로 만드는 복구 기준점이다. checkpoint는 shadow model 또는 실제 snapshot model일 수 있지만, 사용자가 inspect, diff, restore할 수 있어야 한다.

### trajectory

trajectory는 model call, tool request, tool outcome, evaluator verdict, provider snapshot, redacted evidence, timing, token 또는 tool stat을 연결한 local replay record다. public training data 생산이 목적이 아니라, 사용자의 로컬 품질 회귀 검증과 장애 재현이 목적이다.

---

## owner boundary

018이 소유하는 것:

- evaluation -> automation -> improvement -> approval -> checkpoint -> verify -> observe로 이어지는 최종 통합 계약
- evaluator가 반환할 판정 종류와 그 판정의 권한 한계
- scheduled automation과 self-improvement가 통과해야 하는 공통 lifecycle
- replay/evaluation dataset이 가져야 할 최소 의미
- UI/channel/diagnostics가 evaluator와 automation 상태를 표시할 때 지켜야 할 공통 의미

018이 소유하지 않는 것:

- provider adapter와 auth 방식
- tool registry와 tool 실행 semantics
- skill packaging과 discovery의 전체 규칙
- session store schema와 compaction 알고리즘
- permission primitive와 secret storage backend
- runtime service queue와 channel worker 구현
- CLI/TUI/local API transport 세부 구현
- diagnostics bundle format의 모든 필드
- app bundle manifest 전체

018은 다른 spec의 owner 경계를 약화하지 않는다. 오히려 evaluator와 automation이 그 경계를 소비하는 방식을 고정한다.

---

## cross-spec ownership table

| spec | 018이 소비하는 것 | 018이 소유하는 것 |
|---|---|---|
| 003 provider runtime | provider family, model config, auth, request/response boundary | evaluator와 automation이 사용할 aux judge model, fallback, routing, provider snapshot 의미 |
| 004 tool runtime | tool registration, tool execution, MCP tool/resource/prompt primitive | evaluator verdict가 tool 실행을 직접 만들지 않는다는 제한, automation에서 tool exposure를 소비하는 방식 |
| 005 skill system | skill discovery, skill content, read-only knowledge pack 의미 | skill list/view/reference의 progressive disclosure, curator lifecycle을 자동화와 연결하는 계약 |
| 006 session store | session truth, event log, restore 가능한 history | goal/evaluator/trajectory가 session truth를 덮지 않고 evidence reference만 남기는 규칙 |
| 007 main orchestrator policy | orchestration authority, turn policy, command/effect gate | evaluator 결과를 `MainOrchestrator`가 소비하는 input으로 제한하는 final contract |
| 008 configuration profiles and runtime layout | profiles, provider config, runtime path, local layout | automation/evaluator 실행 시 provider snapshot, profile snapshot, replay metadata 요구 |
| 009 context assembly and compaction input | context budget, memory input, compaction input | bounded memory, frozen snapshot, read-only session search evidence를 evaluator에 공급하는 방식 |
| 010 host safety, permissions, and secrets | capability, permission mode, approval, secret safety, checkpoint trigger | safety/capability evaluator의 통합 소비 계약과 stale/expired approval rejection 요구 |
| 011 subagent runtime | subagent lifecycle, delegation, result handling | subagent outcome을 notify/suppress/continue/escalate/verify/rollback으로 분류하는 상위 계약 |
| 012 runtime services | cron, heartbeat, channel workers, task runtime, durable service 목표 | scheduled automation lifecycle, delivery, timeout, recursion prevention, durable job link 의미 |
| 013 user interfaces and session UX | CLI/TUI/local API projection, approval/progress/error UX | goal status, evaluator verdict, automation, rollback, approval projection의 공통 제품 의미 |
| 014 observability diagnostics and inspection | trace, inspect, diagnostics, redaction, recovery evidence | task ledger와 evaluation ledger가 남길 통합 증거, replay/recovery 관점 |
| 016 verification matrix and release gates | release gate, test category, quality bar | evaluator/automation/self-improvement에 필요한 unit, integration, recovery, UX, redaction, replay gate |
| 017 app operating environment | app bundle, app process, task ledger, permission grants, app projection | app task outcome, app automation, app improvement proposal이 통합 evaluator 흐름을 소비하는 계약 |

---

## 최종 도메인 계약

### 1. runtime goal/completion evaluator

최종 runtime은 `/goal` 같은 persistent goal 표면을 제공해야 한다. 정확한 명령 이름은 013이 정하지만, 의미는 다음을 만족해야 한다.

- goal set은 사용자가 지속 목표를 등록한다.
- goal status는 active, paused, blocked, done, cleared 상태와 최근 verdict를 보여 준다.
- goal pause는 scheduled continuation과 autonomous continuation을 멈춘다.
- goal resume은 남아 있는 turn budget과 evidence snapshot을 확인한 뒤 재개한다.
- goal clear는 현재 goal을 종료하되 history와 ledger evidence를 지우지 않는다.
- user interruption은 항상 evaluator continuation보다 우선한다.

completion evaluator는 매 turn 끝, scheduled wake 뒤, subagent 또는 app task 결과 수신 뒤 실행될 수 있다. 입력은 frozen session snapshot, current goal, turn budget, recent tool outcomes, user interruption flag, safety 상태, relevant memory reference다.

출력은 `done`, `continue`, `blocked` 중 하나와 이유다. `continue`는 남은 turn budget을 소비할 수 있음을 뜻할 뿐, 무제한 loop 권한이 아니다. `blocked`는 사용자에게 필요한 action을 설명해야 한다.

### 2. safety/capability evaluator

safety/capability evaluator는 007과 010의 경계를 소비한다. 이 evaluator는 다음 input을 받아야 한다.

- requested capability
- action summary
- tool 또는 MCP server identity
- session id와 turn id
- permission mode
- approval request id
- approval decision id
- approval expiry
- user profile snapshot
- checkpoint trigger 여부
- redaction requirement

approval correlation은 필수다. stale approval, expired approval, 다른 turn 또는 다른 capability에 대한 approval은 거부돼야 한다. permission mode는 010의 formal model을 따라야 하며, evaluator가 permission mode를 새로 발명하면 안 된다.

이 evaluator의 결과는 allow, deny, needs approval, needs checkpoint, needs secret 같은 decision hint일 수 있다. 하지만 실제 권한 확정과 tool 실행은 `MainOrchestrator`가 한다.

### 3. task outcome evaluator

task outcome evaluator는 cron, heartbeat, subagent, app task, channel worker, local API background task의 결과를 사용자 행동으로 분류한다.

출력 class는 다음 의미를 가진다.

- `notify`: 사용자가 알아야 할 완료, 실패, 승인 대기, artifact 생성.
- `suppress`: 중복, 정상 heartbeat, 사용자 action이 필요 없는 상태.
- `continue`: 같은 goal 또는 automation job에서 다음 step을 실행할 수 있음.
- `escalate`: 사용자 입력, secret, approval, 수동 복구가 필요함.
- `verify`: 결과물을 확인하는 검증 step이 필요함.
- `rollback`: 승인된 rollback path를 검토해야 함.

분류는 channel별 noise suppression을 지원하되, session truth나 ledger evidence를 숨기면 안 된다. suppress는 알림을 줄이는 의미이지 증거를 삭제하는 의미가 아니다.

### 4. scheduled automation

scheduled automation은 012 runtime services의 durable runtime 목표를 소비한다.

최종 automation은 다음 job 종류를 지원해야 한다.

- one-shot job
- recurring job
- skill-backed agent job
- script-only job
- no-agent maintenance job
- app task job

모든 job은 owner session 또는 app reference, schedule, timeout, delivery target, permission snapshot, provider snapshot, recursion guard, result policy를 가져야 한다.

recursion prevention은 필수다. scheduled job이 새 scheduled job을 만들거나 자기 자신을 재등록할 때는 explicit user approval 또는 사전에 승인된 rule이 필요하다. timeout은 job 단위와 step 단위에서 모두 해석 가능해야 한다.

delivery는 CLI/TUI/local API/channel projection을 통해 이뤄질 수 있다. delivery 실패는 job 성공 실패와 분리해서 기록한다.

### 5. autonomous improvement flow

자기 개선은 silent self-modification이 아니다. 최종 흐름은 항상 다음 단계를 거쳐야 한다.

```text
proposal
-> approval
-> checkpoint
-> apply
-> verify
-> record
-> rollback if needed
```

proposal은 바꿀 대상, 이유, 예상 효과, 위험, 검증 계획, rollback 계획을 보여야 한다. approval은 010의 approval correlation을 따른다. checkpoint는 010과 014의 안전 및 복구 evidence를 소비한다.

apply는 승인된 diff 또는 설정 변경만 수행해야 한다. verify는 016의 gate 관점을 소비하고, 실패하면 record 후 rollback 후보가 된다. record는 task ledger와 evaluation ledger에 redacted evidence를 남긴다.

runtime은 자기 코드를 몰래 수정하거나 skill을 몰래 삭제하거나 provider routing을 몰래 바꾸면 안 된다.

### 6. checkpoint/rollback

checkpoint/rollback은 010과 014를 소비한다.

최종 모델은 opt-in shadow/checkpoint model을 허용한다. destructive tool, broad filesystem write, config mutation, self-improvement apply, app uninstall, automation rule mutation은 checkpoint trigger가 될 수 있다.

필수 기능은 다음과 같다.

- per-turn dedupe
- checkpoint inspect
- diff view
- restore
- rollback outcome record
- failed rollback diagnostics

per-turn dedupe는 같은 turn에서 같은 대상에 checkpoint가 반복 생성되는 것을 막는다. restore는 사용자 승인 또는 명시 command 없이 자동 실행되면 안 된다. rollback은 실패할 수 있으며, 실패 자체도 session corruption 없이 기록돼야 한다.

### 7. memory and session search

memory와 session search는 009, 006, 014를 소비한다.

evaluator는 bounded memory만 받아야 한다. 장기 memory 전체를 무제한으로 읽지 않는다. session search와 summarization은 frozen snapshot을 만들고, evaluator는 그 snapshot의 evidence reference를 읽는다.

규칙은 다음과 같다.

- memory retrieval은 read-only evidence retrieval이다.
- session search는 session truth를 바꾸지 않는다.
- summarization은 raw history 대체물이 아니라 bounded context artifact다.
- frozen snapshot은 verdict 이후 바뀐 상태를 소급해서 포함하지 않는다.
- redaction은 memory, session search, trajectory에 동일하게 적용된다.

### 8. skills progressive disclosure and curator

skills 경계는 005와 017을 소비한다.

최종 제품은 skill list, skill view, skill reference를 제공해야 한다. 사용자는 어떤 skill이 active, stale, archived 상태인지 볼 수 있어야 한다.

agent-authored skill lifecycle은 다음을 따른다.

- draft proposal
- dry-run
- user approval
- active
- stale
- archived

curator는 skill을 자동 삭제하지 않는다. stale 판정은 사용 빈도, 실패율, 중복, outdated reference를 설명할 수 있어야 한다. archived는 사용자가 되돌릴 수 있어야 하며, active skill discovery에서 빠지는 의미를 갖는다.

progressive disclosure는 모든 skill 내용을 항상 prompt에 넣는 방식이 아니다. evaluator와 orchestrator는 skill list, skill summary, explicit reference, 필요한 section view를 단계적으로 소비한다.

### 9. provider/model routing for evaluators and automation

provider/model routing은 003과 008을 소비한다.

최종 제품은 main generation model과 evaluator model을 분리할 수 있어야 한다. aux judge model은 completion verdict, outcome classification, replay scoring, redaction quality check에 쓰일 수 있다.

필수 계약은 다음과 같다.

- provider snapshot을 trajectory와 ledger에 남긴다.
- fallback은 같은 permission, redaction, budget 경계 안에서만 동작한다.
- routing 실패는 `blocked` 또는 `escalate`로 해석될 수 있다.
- evaluator model이 main model보다 높은 권한을 갖지 않는다.
- provider auth와 profile 의미는 003과 008을 따른다.

automation job은 실행 시점의 provider snapshot을 가져야 한다. 나중에 profile이 바뀌어도 replay와 diagnostics는 당시 snapshot을 설명할 수 있어야 한다.

### 10. MCP/tool exposure and app integration

MCP/tool exposure는 004와 017을 소비한다.

최종 기본값은 default-deny다. exposed tool, resource, prompt는 명시적으로 projection되어야 하며, server status와 app status를 함께 보여야 한다.

필수 projection은 다음 의미를 포함한다.

- server identity
- availability
- exposed tool list
- exposed resource list
- exposed prompt list
- permission requirement
- app bundle reference
- last health status
- last safety denial

evaluator는 tool exposure를 읽을 수 있지만 노출 상태를 직접 바꾸지 않는다. app integration은 017의 app bundle과 task ledger 의미를 따른다.

### 11. offline evaluation/trajectory/replay

offline evaluation은 014와 016을 소비한다.

최종 제품은 redacted trajectory JSONL 또는 동등한 구조를 남길 수 있어야 한다. 형식은 future Rust 설계에서 정하되, 의미는 다음을 포함해야 한다.

- session id와 turn id
- goal id 또는 automation job id
- model/provider snapshot
- prompt/context digest
- tool call summary
- tool result class
- evaluator verdict
- approval decision reference
- checkpoint reference
- redaction marker
- timing과 tool stats
- replay eligibility

이 데이터는 mandatory public training data가 아니다. 기본 목적은 사용자의 로컬 품질 회귀, replay, evaluator 개선 검증, 장애 재현이다.

replay는 실제 destructive tool을 재실행하지 않아야 한다. 필요한 경우 recorded tool result와 dry-run projection을 사용한다.

### 12. UI/channel projections

UI와 channel projection은 013과 012를 소비한다.

CLI, TUI, local API, 외부 channel은 같은 의미를 표시해야 한다.

필수 projection은 다음과 같다.

- goal status
- evaluator verdict
- approval request와 approval decision
- automation job status
- scheduled delivery state
- checkpoint status
- rollback action
- task outcome class
- blocked reason
- replay 또는 diagnostics reference

channel은 사용자 interruption priority를 지켜야 한다. 사용자가 중단, pause, clear, deny를 보낸 경우 scheduled continuation보다 먼저 처리한다.

외부 channel은 모든 세부 evidence를 늘어놓지 않을 수 있다. 하지만 local API와 inspect surface에서는 사용자가 근거를 따라갈 수 있어야 한다.

### 13. observability and diagnostics

observability는 014와 016을 소비한다.

최종 제품은 task ledger와 evaluation ledger를 구분해야 한다.

- task ledger는 automation job, app task, tool use, checkpoint, artifact, delivery 결과를 설명한다.
- evaluation ledger는 goal verdict, capability decision, outcome classification, replay score, evaluator model snapshot을 설명한다.

trace와 diagnostics bundle은 redaction을 통과해야 한다. raw secret, provider hidden reasoning, 필요 이상의 file contents를 저장하면 안 된다.

diagnostics bundle은 사용자가 local 환경에서 장애를 설명할 수 있을 만큼의 evidence를 담아야 한다.

- runtime snapshot
- active goals
- automation job summary
- recent evaluator verdicts
- failed approvals
- denied capabilities
- checkpoint/rollback status
- provider snapshot
- redaction report
- replay eligibility summary

---

## evaluator authority boundary

evaluator 결과는 직접적인 권한이 아니다.

다음은 금지된다.

- completion evaluator가 session truth를 직접 `done`으로 변경하는 것.
- safety evaluator가 approval 없이 permission을 부여하는 것.
- outcome evaluator가 tool을 직접 실행하는 것.
- automation evaluator가 job을 무제한 재귀 생성하는 것.
- replay evaluator가 실제 destructive tool을 다시 호출하는 것.

공식 흐름은 항상 다음과 같다.

```text
input snapshot
-> evaluator verdict
-> MainOrchestrator policy decision
-> approved command/effect
-> session/event/ledger record
-> projection
```

`MainOrchestrator`는 evaluator verdict를 거부하거나 사용자 확인을 요구할 수 있다. evaluator confidence가 높아도 이 권한 경계는 바뀌지 않는다.

---

## invariants

1. `MainOrchestrator`만 session truth와 effect execution을 확정한다.
2. evaluator는 frozen input snapshot을 읽고 verdict를 반환한다.
3. user interruption은 scheduled continuation과 autonomous continuation보다 우선한다.
4. approval은 request id, decision id, capability, turn 또는 job scope, expiry가 맞아야 한다.
5. stale 또는 expired approval은 거부된다.
6. automation job은 timeout과 recursion guard를 가져야 한다.
7. destructive action과 self-improvement apply는 checkpoint 정책을 통과해야 한다.
8. suppress는 알림 억제일 뿐 evidence 삭제가 아니다.
9. memory와 session search는 read-only evidence retrieval이다.
10. skill curator는 user approval 없이 active skill을 삭제하지 않는다.
11. evaluator model은 provider/model routing을 통해 선택될 수 있지만 더 높은 권한을 갖지 않는다.
12. trajectory와 diagnostics는 redaction을 통과해야 한다.
13. replay는 실제 destructive effect를 재실행하지 않는다.

---

## normal sequences

### persistent goal completion

```text
user sets goal
-> MainOrchestrator records active goal
-> turn executes approved steps
-> completion evaluator reads frozen snapshot
-> verdict done/continue/blocked returned
-> MainOrchestrator accepts or asks user
-> goal projection and evaluation ledger updated
```

### scheduled automation

```text
user creates recurring job
-> schedule stored with permission and provider snapshot
-> runtime service wakes job
-> safety/capability evaluator checks action
-> MainOrchestrator approves command/effect
-> job executes with timeout
-> task outcome evaluator classifies result
-> delivery projection and ledger updated
```

### self-improvement

```text
runtime proposes improvement
-> user reviews proposal
-> approval correlated to proposal
-> checkpoint created or verified
-> approved change applied
-> verification gate runs
-> result recorded
-> rollback offered or executed if approved and needed
```

### replay evaluation

```text
trajectory selected
-> redacted replay input assembled
-> provider/model snapshot restored as metadata
-> evaluator scores or compares outcome
-> no destructive tool executes
-> evaluation ledger records result
```

---

## failure sequences

### stale approval

```text
tool action requests approval
-> user approves after expiry or for different scope
-> safety evaluator marks stale approval
-> MainOrchestrator denies effect
-> user sees new approval request or blocked reason
-> evaluation ledger records stale rejection
```

### turn budget exhausted

```text
goal evaluator returns continue
-> remaining turn budget is zero
-> MainOrchestrator refuses continuation
-> goal becomes blocked or paused by policy
-> UI shows budget exhaustion and resume options
```

### automation recursion attempt

```text
scheduled job tries to create another recurring job
-> recursion guard detects unapproved expansion
-> safety evaluator returns needs approval or deny
-> MainOrchestrator stops new job creation
-> task outcome becomes escalate
```

### checkpoint restore failure

```text
rollback requested
-> checkpoint restore starts after approval
-> restore fails partially
-> MainOrchestrator records failed rollback
-> diagnostics bundle includes diff and failure reason
-> user sees manual recovery options
```

### evaluator provider failure

```text
completion evaluator selects aux judge model
-> provider call fails
-> fallback route checked against same policy
-> fallback succeeds or blocked verdict is produced
-> provider snapshot and failure reason recorded
```

---

## prohibited patterns

- evaluator가 직접 session truth를 변경하는 것.
- evaluator가 직접 tool, MCP, shell, filesystem effect를 실행하는 것.
- approval id correlation 없이 capability를 허용하는 것.
- expired approval을 편의상 재사용하는 것.
- scheduled job이 recursion guard 없이 자기 자신을 재등록하는 것.
- goal continuation이 user interruption을 무시하는 것.
- `continue` verdict를 무제한 loop 권한으로 해석하는 것.
- suppress outcome을 ledger 삭제로 해석하는 것.
- memory search 결과를 session truth처럼 쓰는 것.
- skill curator가 user approval 없이 skill을 삭제하는 것.
- self-improvement가 proposal과 approval 없이 config, skill, tool exposure, app manifest를 바꾸는 것.
- checkpoint 없이 destructive mutation을 자동 실행하는 것.
- replay가 recorded destructive tool을 실제로 재실행하는 것.
- provider hidden reasoning, raw secret, 필요 이상의 file contents를 trajectory에 저장하는 것.
- Hermes reference의 Python 구조나 code를 `shacs-bot` 구현 목표로 복제하는 것.
- SaaS, 관리자, 조직, fleet workflow를 기본 제품 전제로 끌어오는 것.

---

## verification and test perspective

이 문서는 현재 Rust contract/runtime-helper closure의 검증 관점을 기록한다. Full live product closure를 선언하려면 016의 release gate를 소비해 다음 gate와 018-specific coverage evidence를 통과해야 한다.

기본 Rust gate:

```sh
cargo fmt --manifest-path crates/Cargo.toml --all -- --check
cargo check --manifest-path crates/Cargo.toml --workspace --locked
cargo clippy --manifest-path crates/Cargo.toml --workspace --all-targets -- -D warnings
cargo test --manifest-path crates/Cargo.toml --workspace --locked
```

필수 테스트 관점:

- unit test: verdict mapping, approval correlation, stale/expired rejection, recursion guard, outcome classification.
- integration test: goal lifecycle, scheduled job wake, subagent/app task outcome, provider fallback, UI projection.
- recovery test: checkpoint create, per-turn dedupe, diff, restore, failed rollback diagnostics.
- UX test: CLI/TUI/local API/channel이 같은 goal, approval, automation, rollback 상태를 표시하는지.
- redaction test: trajectory, diagnostics bundle, ledger에 raw secret과 hidden reasoning이 남지 않는지.
- replay test: recorded trajectory가 destructive tool 없이 재평가되는지.
- self-improvement test: proposal, approval, checkpoint, apply, verify, record, rollback 흐름이 silent mutation 없이 동작하는지.
- memory/session search test: frozen snapshot과 bounded evidence가 verdict 이후 상태를 소급하지 않는지.

검증은 evaluator 정확도만 보지 않는다. 권한 경계, user interruption priority, timeout, delivery failure, observability, rollback 가능성을 함께 확인해야 한다.

---

## 명시적 비범위

018은 다음을 최종 제품 요구로 보지 않는다.

- 중앙 SaaS evaluator 서비스.
- 조직별 관리자 승인 흐름.
- fleet 단위 policy 배포.
- public training dataset 자동 업로드.
- 임의 remote code self-update.
- Hermes code 또는 Python runtime 복제.
- 모든 자동화가 agent를 반드시 호출해야 한다는 요구.
- 모든 실패를 자동 rollback해야 한다는 요구.

사용자는 자기 로컬 런타임에서 목표와 자동화를 보고, 멈추고, 승인하고, 복구할 수 있어야 한다. 그 범위를 넘어선 운영 플랫폼은 이 spec의 기본 가정이 아니다.

---

## 결론

018의 최종 계약은 `shacs-bot`이 단순히 더 많은 cron과 evaluator를 갖는 것이 아니라, 목표 완료 판정, capability 안전 판정, task outcome 판정, scheduled automation, self-improvement, checkpoint/rollback, memory, skills, provider routing, MCP exposure, replay, UI projection, diagnostics를 하나의 설명 가능한 개인용 runtime 경험으로 묶는 것이다.

권한의 중심은 끝까지 `MainOrchestrator`에 남는다. evaluator는 판단을 제공하고, automation은 승인된 lifecycle을 실행하며, ledger와 diagnostics는 사용자가 결과를 이해하고 되돌릴 수 있게 해야 한다.
