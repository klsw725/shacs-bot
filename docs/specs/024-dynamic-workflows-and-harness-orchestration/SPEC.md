# dynamic workflows and harness orchestration 아키텍처 명세

Status: Draft. 이 문서는 prototype 계획서가 아니라 `shacs-bot`이 작업별 동적 하네스를 완성형 제품 계약으로 제공하기 위해 필요한 최종 owner boundary를 고정한다.

## 문서 목적

이 문서는 `docs/SYSTEM-FOUNDATION.md`와 numbered spec set 전체를 바탕으로, 복잡한 작업마다 맞춤형 실행 하네스를 만들고 subagent, verifier, tool surface, skill, worktree, budget, resume, diagnostics를 하나의 오케스트레이션 의미론으로 묶는 계약을 정의한다.

목표는 다음과 같다.

- 동적 워크플로우가 무엇이고 무엇이 아닌지 고정한다.
- 기존 `spawn`/subagent runtime을 단순 background task가 아니라 workflow-backed execution primitive로 확장할 때의 owner boundary를 정의한다.
- 작업별 harness plan, workflow pattern, verifier, merge, worktree isolation, budget, resume, skill sharing의 최종 상태를 명시한다.
- 긴 작업에서 발생하는 agentic laziness, self-preferential bias, goal drift를 구조적으로 줄이는 완성형 검증 경계를 정의한다.
- 사용자가 직접 설치하고 운영하는 self-hosted/personal-use 런타임이라는 전제를 유지한다.

이 문서는 JavaScript workflow runtime을 그대로 도입하자는 문서가 아니다. 외부 reference에서 가져올 것은 “작업별 하네스”, “독립 컨텍스트 subagent”, “adversarial verifier”, “fan-out/synthesize”, “loop until done”, “worktree isolation”, “budgeted execution”, “resume” 같은 제품 의미론이다. 구현 언어와 상태 전이 권한은 `shacs-bot`의 Rust runtime과 MainOrchestrator 계약을 따른다.

---

## 상위 기준과의 관계

이 문서는 다음 상위 기준을 전제로 한다.

- `MainOrchestrator`는 세션 상태, workflow 상태, child result merge, permission 확정의 유일한 권한자다.
- workflow runner, subagent, verifier, tool executor, scheduler, channel worker는 모두 effect 실행자 또는 evidence provider일 뿐 session truth를 직접 변경하지 않는다.
- `shacs-bot`은 self-hosted/personal-use AI Operating System이다. 기본 주체는 사용자가 직접 설치하고 운영하는 본인이다.
- 동적 워크플로우는 조직용 fleet orchestration, 원격 작업 마켓플레이스, SaaS admin console, 멀티테넌트 reviewer pool이 아니다.
- 워크플로우는 많은 compute를 쓰는 기능이다. 모든 작업에 자동 적용하지 않고, 복잡성, 가치, 위험, 검증 필요성이 높은 작업에서 명시적 또는 정책 기반으로 선택한다.

기존 spec과의 관계는 다음과 같다.

| spec | 023이 소비하는 것 | 023이 소유하는 것 |
|---|---|---|
| 001 session kernel | 한 턴의 truth, terminal state, recovery input | workflow state가 session turn에 붙는 방식과 workflow terminal criteria |
| 002 command/event/effect | 상태 변경과 외부 실행 분리 | workflow command/effect/event 의미와 barrier/synthesis event |
| 003 provider runtime | provider request/response, model config | workflow의 model/intelligence routing decision 의미 |
| 004 tool runtime | registry validation, tool execution, normalized outcome | workflow step이 tool execution boundary를 우회하지 않는 규칙 |
| 005 skill system | read-only skill discovery/injection | workflow recipe와 reusable harness template을 skill로 공유하는 계약 |
| 006 session store | session truth, event log, recovery | workflow checkpoint/resume evidence가 session store에 남는 의미 |
| 007 main orchestrator policy | turn policy, retry/abort, stale result reality | workflow admission, merge, retry, abort, late result decision의 owner boundary |
| 008 config/runtime layout | local config, runtime dirs, provider snapshot | workflow config, saved recipe location, worktree root layout |
| 009 context assembly | context budget, compaction input | workflow child context slicing과 original objective preservation |
| 010 host safety | fs/proc/net/secret guards, MCP default-deny | workflow quarantine, high-privilege action separation, worktree safety ceiling |
| 011 subagent runtime | spawn, child lifecycle, stale discard, bounded parallelism | workflow-level child graph, barrier, verifier pairing, merge policy |
| 012 runtime services | cron, heartbeat, channel worker runtime | recurring workflow and loop scheduling integration |
| 013 UI/session UX | CLI/TUI/local API projection | workflow progress, verifier status, budget, blocked/resume projection semantics |
| 014 diagnostics | trace, inspect, redaction, recovery evidence | workflow trace, harness plan, child graph, merge evidence, verifier evidence |
| 016 verification gates | release evidence categories | workflow implementation completion gates |
| 017 app environment | app task, app process, task ledger | app-provided workflow recipes and app task workflow execution |
| 018 evaluation/automation/self-improvement | goal evaluator, outcome evaluator, replay, improvement flow | workflow-specific harness evaluator and pattern lifecycle |
| 020 tool search | provider-visible tool progressive disclosure | workflow-local tool catalog scope and child tool search inheritance |
| 022 auto approval | permission mode, capability taxonomy, inherited ceilings | workflow step admission and automatic approval boundaries |

---

## Reference 채택 원칙

Anthropic Claude Code의 dynamic workflows 문서는 이 spec의 제품 의미론 reference다. 그대로 가져오는 것과 가져오지 않을 것을 분리한다.

| reference concept | 가져올 것 | 그대로 가져오지 않을 것 |
|---|---|---|
| task-specific harness | 작업마다 하네스 구조를 다르게 만드는 의미 | JavaScript 파일 실행을 필수 구현으로 고정 |
| subagent orchestration | 독립 컨텍스트 child와 focused goal | parent 권한을 무제한 복제하는 child process |
| adversarial verification | verifier가 별도 컨텍스트에서 rubric 검증 | 모든 작업에 N명 reviewer를 띄우는 낭비 |
| fan-out and synthesize | 병렬 child 결과를 barrier 뒤 병합 | child가 session truth를 직접 쓰는 구조 |
| tournament | 비교 판단이 유리한 대량 ranking/selection | 비용 제한 없는 무한 bracket |
| loop until done | stop condition 기반 반복 | 사용자 승인 없는 무기한 자동 실행 |
| worktree isolation | write-capable child의 main tree 손상 방지 | git state를 조용히 rewrite/merge하는 자동화 |
| token budget | workflow 비용 상한 | provider별 과금 제어를 완성 기능처럼 과장 |
| save/share via skill | workflow recipe를 skill로 재사용 | 실행 가능한 plugin code를 skill 권한으로 부여 |

---

## 범위

이 문서는 다음을 정의한다.

- workflow definition과 workflow run lifecycle
- workflow harness plan과 pattern taxonomy
- workflow admission, planning, execution, barrier, synthesis, verification, terminal state
- subagent graph, verifier pairing, adversarial review, tournament, loop-until-done semantics
- workflow-local context slicing과 original objective preservation
- workflow-local tool surface, Tool Search inheritance, quarantine, permission ceiling
- worktree isolation policy and merge handoff
- model/intelligence routing and budget snapshots
- checkpoint, resume, stale/late result handling
- saved workflow recipe and skill distribution semantics
- observability, diagnostics, replay, release gate requirements

이 문서는 다음을 정의하지 않는다.

- JavaScript workflow interpreter implementation
- external cloud workflow service
- remote marketplace or signed public workflow registry
- organization admin approval console
- multi-user reviewer assignment
- provider-native workflow beta 기능
- individual workflow recipe implementation details
- visual UI layout

---

## 핵심 정의

### dynamic workflow

Dynamic workflow는 사용자의 복잡한 목표를 달성하기 위해 runtime이 한 턴 또는 지속 goal 안에서 만든 작업별 하네스 실행이다. workflow는 고정된 단일 agent loop가 아니라, pattern, child graph, verifier, budget, tool scope, worktree policy, merge policy를 포함하는 실행 계획이다.

Workflow는 session truth가 아니다. Workflow state는 session truth에 붙는 structured execution state이며, terminal state 반영은 MainOrchestrator가 수행한다.

### harness plan

Harness plan은 workflow 실행 전 또는 실행 중에 만들어지는 typed plan이다. 최소 필드는 다음이다.

```text
workflow_id
origin_session_id
origin_turn_id
objective
constraints
pattern
steps
child_graph
verifier_graph
context_policy
tool_scope_policy
permission_policy
worktree_policy
model_routing_policy
budget_policy
checkpoint_policy
merge_policy
stop_condition
resume_policy
```

Harness plan은 provider가 만든 자연어 계획만으로 충분하지 않다. Runtime이 inspect, replay, resume, diagnostics, release evidence에서 읽을 수 있는 structured form을 가져야 한다.

### workflow pattern

Workflow pattern은 harness plan이 채택한 고수준 실행 구조다. 초기 complete product contract에서 지원해야 할 pattern family는 다음이다.

- `classify_and_act`
- `fan_out_and_synthesize`
- `adversarial_verification`
- `generate_and_filter`
- `tournament`
- `loop_until_done`
- `workflow_sequence`
- `hybrid`

`hybrid`는 여러 pattern을 조합하는 경우다. `hybrid`라도 내부 step은 inspect 가능한 pattern family 중 하나로 분해되어야 한다.

### workflow child

Workflow child는 harness plan에 의해 생성된 subagent 실행 단위다. 기존 subagent runtime의 child task와 연결되지만, workflow context에서는 parent workflow id, step id, expected output schema, verifier requirement, budget slice, tool scope, worktree policy를 추가로 가진다.

### verifier

Verifier는 다른 child 또는 synthesis result를 원본 objective, rubric, evidence, codebase, external source에 비추어 검증하는 별도 child다. Verifier는 검증 대상 child의 raw reasoning을 신뢰해서는 안 되며, 가능한 한 독립 evidence를 사용해야 한다.

### barrier

Barrier는 fan-out, tournament round, verification wave처럼 여러 child 결과가 모여야 다음 step으로 넘어갈 수 있는 workflow boundary다. Barrier가 열리기 전에 synthesis 또는 terminal success를 선언하면 안 된다.

### synthesis

Synthesis는 barrier 이후 child result, verifier verdict, evidence ref를 MainOrchestrator가 소비할 수 있는 하나의 structured outcome으로 합치는 단계다. Synthesis는 child result의 단순 concat이 아니다. conflict resolution, confidence, omitted evidence, unresolved issue를 명시해야 한다.

### workflow recipe

Workflow recipe는 특정 작업군에 재사용 가능한 harness template이다. Recipe는 skill, app bundle, local config, builtin template에서 제공될 수 있지만 실행 권한을 직접 얻지 않는다. Recipe는 pattern과 prompt scaffold, expected output schema, suggested verifier, safety hints를 제공하는 read-only input이다.

### quarantine

Quarantine은 untrusted input을 읽는 child와 high-privilege action을 수행하는 child를 분리하는 workflow safety pattern이다. Public issue, Slack/Email, 웹 페이지, 외부 문서, user-uploaded file처럼 prompt injection 가능성이 있는 입력을 읽은 child는 직접 write/exec/secret/tool action을 수행하면 안 된다.

---

## 제품 목표

Dynamic workflows의 최종 제품 목표는 다음이다.

1. 사용자가 복잡한 작업을 요청하면 runtime이 필요한 경우 작업별 harness plan을 만든다.
2. Plan은 original objective와 금지 조건을 structured constraints로 보존한다.
3. Plan은 작업을 독립 child와 verifier로 분해하고, 각 child에 필요한 최소 context와 tool scope만 준다.
4. Child는 독립 context window에서 실행되어 cross-contamination과 goal drift를 줄인다.
5. Write-capable child는 필요하면 격리 worktree에서 실행되고, merge는 parent가 검증 후 결정한다.
6. Barrier와 synthesis는 모든 필수 child/verifier 결과가 도착하기 전까지 success를 선언하지 않는다.
7. Workflow는 budget, timeout, parallelism, stop condition을 가진다.
8. Interruption 또는 process restart 이후 workflow는 checkpoint와 event log 기준으로 resume 또는 visible blocked 상태가 된다.
9. Workflow progress, child graph, verifier result, budget, blocked reason은 CLI/TUI/local API/channel projection에서 확인할 수 있다.
10. Workflow recipe는 skill로 저장/공유할 수 있지만 권한 확장 경로가 되지 않는다.

---

## 실패 모드와 방어 전략

### Agentic laziness

Agentic laziness는 전체 작업 중 일부만 처리하고 완료를 선언하는 실패다. Workflow는 다음으로 방어한다.

- objective를 checklist 또는 item set으로 구조화한다.
- fan-out 대상 수와 completed 수를 tracking한다.
- barrier가 필수 child result 누락 시 synthesis를 막는다.
- verifier가 coverage를 확인한다.
- terminal state에는 coverage summary와 omitted items를 포함한다.

### Self-preferential bias

Self-preferential bias는 생성자가 자신의 결과를 과신하는 실패다. Workflow는 다음으로 방어한다.

- verifier를 생성 child와 별도 context에서 실행한다.
- verifier prompt에는 원본 objective, rubric, independent evidence requirement를 넣는다.
- verifier는 pass/fail만이 아니라 disputed claim, missing evidence, confidence를 반환한다.
- synthesis는 verifier disagreement를 숨기면 안 된다.

### Goal drift

Goal drift는 긴 실행과 compaction을 거치며 초기 요구와 금지 조건이 손실되는 실패다. Workflow는 다음으로 방어한다.

- original objective와 constraints를 workflow root snapshot으로 고정한다.
- child prompt는 root snapshot에서 필요한 slice만 받되 핵심 금지 조건은 반복 주입한다.
- resume 후에도 root snapshot을 재사용한다.
- synthesis와 verifier는 root snapshot 기준으로 compliance를 확인한다.

---

## Workflow lifecycle

Workflow run은 다음 상태를 가진다.

```text
planned
admitted
running
waiting_for_children
verifying
synthesizing
waiting_for_user
blocked
completed
failed
cancelled
stale
```

### 정상 시퀀스

1. User turn 또는 active goal이 workflow-worthy task를 만든다.
2. MainOrchestrator가 workflow admission을 판단한다.
3. Runtime이 recipe 후보와 task context를 바탕으로 harness plan을 만든다.
4. MainOrchestrator가 plan의 permission, budget, worktree, tool scope를 확정한다.
5. Workflow run이 `admitted` 상태로 event log에 기록된다.
6. Child graph의 ready step이 spawn된다.
7. Child는 제한된 context, tool registry, budget, worktree policy 아래 실행된다.
8. Child result는 direct session write가 아니라 workflow child result event로 돌아온다.
9. Barrier가 열린 step은 verifier 또는 synthesis로 넘어간다.
10. Verifier는 rubric과 independent evidence로 child result를 검증한다.
11. Synthesis가 accepted result, rejected result, unresolved issue, evidence ref를 묶는다.
12. MainOrchestrator가 final workflow result를 session answer, goal verdict, app task outcome 중 하나로 반영한다.

### 실패 시퀀스

1. Admission이 실패하면 workflow를 만들지 않고 일반 agent loop 또는 user clarification으로 돌아간다.
2. Plan이 permission ceiling을 넘으면 blocked 또는 ask_user approval로 전환한다.
3. Child가 timeout되면 retry policy 또는 failure fact로 synthesis한다.
4. Child result correlation이 맞지 않으면 stale로 기록하고 merge하지 않는다.
5. Verifier가 실패를 반환하면 synthesis는 success를 선언할 수 없다. retry, partial completion, blocked, failed 중 하나를 선택해야 한다.
6. Budget이 소진되면 workflow는 `blocked` 또는 `failed`로 닫고 남은 작업과 재개 조건을 표시한다.
7. Process restart 이후 checkpoint가 불충분하면 workflow는 자동 성공이 아니라 visible interrupted/blocked state가 된다.

---

## Admission policy

Workflow는 모든 요청에 적용하지 않는다. Admission input은 다음을 포함한다.

```text
objective_complexity
estimated_item_count
requires_parallelism
requires_independent_verification
requires_adversarial_review
requires_large_context_partitioning
requires_write_isolation
requires_recurring_loop
risk_level
user_requested_workflow
available_budget
```

Admission result는 다음 중 하나다.

```text
use_regular_loop
use_quick_workflow
use_dynamic_workflow
ask_user_for_scope
blocked_by_policy
```

`quick_workflow`는 작은 verifier 또는 classifier처럼 1-2개 child만 쓰는 제한형 workflow다. 단순 lint, 작은 typo, 단일 파일 수정에는 dynamic workflow를 자동 사용하면 안 된다.

---

## Pattern contracts

### classify_and_act

Classifier child가 task type, risk, required tools, model tier, workflow pattern을 판정한다. Classifier result는 action을 직접 실행하지 않는다. MainOrchestrator가 classifier evidence를 소비해 다음 step을 확정한다.

완료 조건:

- classifier output schema가 task class, confidence, evidence, recommended pattern을 포함한다.
- low confidence면 ask_user 또는 fallback regular loop로 간다.

### fan_out_and_synthesize

작업을 item, module, claim, source, file group 등 독립 slice로 나누고 child를 병렬 실행한 뒤 synthesis한다.

완료 조건:

- fan-out input set과 expected count가 기록된다.
- 각 child output은 structured result와 evidence ref를 가진다.
- barrier는 required child result 누락을 허용하지 않는다.
- synthesis는 conflict, missing, duplicate를 처리한다.

### adversarial_verification

생성 child 또는 synthesis result를 별도 verifier가 rubric 기준으로 검증한다.

완료 조건:

- verifier는 원본 objective와 rubric을 받는다.
- verifier는 target result의 claim을 evidence와 대조한다.
- fail 또는 uncertain verdict는 final success를 막는다.

### generate_and_filter

여러 후보를 생성하고 rubric, dedupe, verifier로 필터링한다.

완료 조건:

- candidate provenance가 남는다.
- filtering criteria가 기록된다.
- 최종 결과는 rejected reason을 요약할 수 있다.

### tournament

비교 판단이 절대 점수보다 안정적인 sorting/selection 작업에 사용한다.

완료 조건:

- bracket 또는 pairwise comparison order가 기록된다.
- judge rubric과 tie-breaker가 명시된다.
- budget ceiling과 max rounds가 있다.

### loop_until_done

종료 조건이 불명확한 debugging, triage, research, log mining에 사용한다.

완료 조건:

- stop condition이 명시된다.
- max iteration, budget, no-new-findings threshold가 있다.
- loop continuation은 goal/permission/budget guard를 통과해야 한다.

---

## Context and objective preservation

Workflow context policy는 다음을 만족해야 한다.

1. Root objective snapshot은 child에게 전달되는 모든 context slice의 기준이다.
2. Child는 필요한 최소 file/session/evidence slice만 받는다.
3. 외부 untrusted content는 명시적으로 untrusted block으로 표시된다.
4. Compaction 또는 resume 후에도 root objective와 constraints는 손실되면 안 된다.
5. Child result는 root objective 대비 coverage와 deviations를 보고해야 한다.

Context owner는 009다. 024는 workflow가 context slicing을 어떻게 소비하는지 정의한다.

---

## Tool surface and Tool Search

Workflow child의 tool surface는 parent registry 전체가 아니다. 다음 규칙을 따른다.

1. Child tool registry는 011의 subagent registry 제한을 따른다.
2. Tool Search catalog는 child registry 안의 deferrable tool만 포함한다.
3. Workflow-local bridge scope는 parent-only MCP capability를 노출하면 안 된다.
4. Core recovery tools는 owner spec이 허용한 범위에서만 visible하다.
5. Quarantine child는 read/search/fetch/classify 같은 low-privilege tools만 사용해야 한다.
6. High-privilege action child는 untrusted raw content 대신 sanitized synthesis를 입력으로 받아야 한다.

Tool execution은 항상 004 runtime boundary를 통과한다. Workflow가 tool call을 직접 unwrap하거나 permission을 우회하면 안 된다.

---

## Worktree isolation

Write-capable workflow child는 다음 조건에서 worktree isolation을 사용할 수 있어야 한다.

- 대규모 refactor, migration, multi-file edit
- 서로 다른 child가 같은 repository를 수정할 가능성이 있는 경우
- risky fix 탐색 또는 competing implementation tournament
- verifier가 diff를 독립 검토해야 하는 경우

Worktree policy는 다음 값을 가진다.

```text
none
read_only_snapshot
isolated_worktree_required
isolated_worktree_optional
```

완성형 구현의 worktree contract는 다음이다.

1. Worktree 생성은 MainOrchestrator가 승인한 effect여야 한다.
2. Branch/worktree name은 workflow id와 child id를 포함해 충돌을 피한다.
3. Child는 할당된 worktree 밖을 수정할 수 없다.
4. Heavy commands such as full build/test는 workflow scheduler가 concurrency를 제한할 수 있어야 한다.
5. Child diff는 parent synthesis로 돌아오며 자동 merge되지 않는다.
6. Merge는 verifier, cargo check/test, permission gate, user-visible diff evidence를 통과해야 한다.
7. Failed child worktree는 cleanup 가능해야 하되, diagnostics를 남기기 전 삭제하면 안 된다.

---

## Budget, timeout, and model routing

Workflow budget policy는 다음을 포함한다.

```text
max_total_tokens
max_child_tokens
max_verifier_tokens
max_iterations
max_parallel_children
max_wall_clock_ms
max_heavy_commands
```

Budget은 단순 prompt hint가 아니라 runtime이 관찰 가능한 snapshot이어야 한다. Provider가 정확한 token usage를 제공하지 않는 경우에도 estimated usage와 known usage를 구분해 기록한다.

Model routing policy는 다음을 포함한다.

```text
classifier_model_hint
child_model_hint
verifier_model_hint
synthesis_model_hint
fallback_model_policy
```

Model routing은 provider selection을 우회하지 않는다. 003 provider runtime과 007 policy가 최종 선택을 통제한다.

---

## Resume and checkpoint

Workflow resume은 complete product contract의 필수 기능이다.

Checkpoint에는 최소한 다음이 포함되어야 한다.

```text
workflow_id
root_objective_snapshot
harness_plan_digest
state
completed_steps
active_children
pending_barriers
budget_usage
worktree_refs
evidence_refs
last_safe_resume_point
```

Resume 규칙:

1. Active child result가 correlation과 checkpoint digest에 맞으면 merge 후보가 될 수 있다.
2. Mismatch result는 stale로 기록하고 session content에 반영하지 않는다.
3. Pending barrier는 completed child set을 재구성한 뒤 열릴 수 있다.
4. Worktree가 사라졌거나 dirty state가 불명확하면 blocked로 전환한다.
5. Resume이 불가능한 workflow는 자동 성공이 아니라 interrupted/blocked evidence로 사용자에게 표시한다.

---

## Saved workflow recipes and skills

Workflow recipe는 skill로 공유할 수 있다. 단, skill은 여전히 read-only knowledge pack이다.

Skill-backed workflow recipe는 다음을 제공할 수 있다.

- pattern hints
- prompt templates
- rubric
- output schema
- verifier instructions
- suggested budget
- suggested tool scope
- safety warnings
- worktree policy hints

Skill-backed recipe는 다음을 제공할 수 없다.

- permission grant
- executable plugin authority
- hidden tool access
- bypassed approval
- direct session state mutation

Workflow recipe discovery는 005 skill system을 소비한다. Recipe precedence, conflict, malformed state는 skill registry의 선택 규칙을 따른다.

---

## User-facing projection

CLI/TUI/local API/channel projection은 다음을 표시할 수 있어야 한다.

```text
workflow id
objective summary
pattern
state
progress count
active child count
pending barrier
verifier status
budget usage
worktree refs
blocked reason
next action
resume availability
```

Projection은 raw secret, untrusted payload, full hidden prompt, full tool schema를 노출하면 안 된다. Channel projection은 user-visible event만 보내며, 모든 low-level child progress를 spam하면 안 된다.

---

## Observability and diagnostics

Workflow diagnostics는 다음 evidence를 남겨야 한다.

- harness plan digest and redacted summary
- admission decision and reason
- pattern choice and recipe source
- child graph and verifier graph
- per-child tool scope digest
- worktree refs and merge refs
- barrier open/close events
- verifier verdict and evidence refs
- synthesis decision
- budget usage
- resume checkpoint
- stale/late result decisions

Diagnostics는 release gate와 replay에서 destructive tool을 재실행하지 않고도 workflow behavior를 해석할 수 있어야 한다.

---

## Security and permission invariants

다음 불변식은 절대 깨면 안 된다.

1. Workflow는 MainOrchestrator 권한을 대체하지 않는다.
2. Child 또는 verifier는 session truth를 직접 수정하지 않는다.
3. Workflow recipe 또는 skill은 tool permission을 부여하지 않는다.
4. Untrusted content를 읽은 child는 high-privilege action을 직접 수행하지 않는다.
5. Worktree child는 할당된 worktree와 permission ceiling 밖을 수정하지 않는다.
6. Tool Search는 child scope 밖의 tool을 search/describe/call할 수 없다.
7. Verifier failure를 숨기고 success로 닫으면 안 된다.
8. Budget exhaustion을 success로 포장하면 안 된다.
9. Resume 실패를 완료로 간주하면 안 된다.
10. Workflow loop는 user interruption, budget, permission, recursion guard를 통과해야 한다.

---

## PRD 분할

완성형 closure를 위해 PRD는 다음 wave로 나눈다. 각 PRD는 prototype가 아니라 해당 영역의 구현, 문서, 테스트, diagnostics evidence까지 닫는 것을 목표로 한다.

1. `prds/000-workflow-state-and-harness-plan.md`: workflow id, state machine, harness plan schema, admission result, event log/checkpoint foundation.
2. `prds/001-pattern-engine-and-child-graph.md`: pattern taxonomy, child graph, barrier, fan-out/synthesis, structured child output.
3. `prds/002-verifier-and-adversarial-review.md`: verifier graph, rubric, independent evidence, verifier failure handling, synthesis integration.
4. `prds/003-worktree-isolation-and-merge-handoff.md`: isolated worktree effect, child worktree policy, diff evidence, merge approval, cleanup diagnostics.
5. `prds/004-budget-timeout-and-model-routing.md`: budget snapshots, child/verifier token limits, timeout enforcement, model routing policy.
6. `prds/005-skill-backed-workflow-recipes.md`: recipe metadata, skill integration, malformed/conflict handling, saved workflow inspect surface.
7. `prds/006-quarantine-and-permission-ceilings.md`: untrusted input quarantine, high-privilege separation, Tool Search child scope, approval integration.
8. `prds/007-resume-replay-and-diagnostics.md`: checkpoint resume, stale/late child results, replay evidence, diagnostics bundle.
9. `prds/008-user-facing-projection-and-release-gates.md`: CLI/TUI/local API/channel projection, release evidence checklist, documentation updates.

---

## 완료 기준

Spec 024의 closure는 다음이 모두 충족될 때만 선언할 수 있다.

- Workflow run state와 harness plan이 typed Rust model로 존재한다.
- Workflow admission이 regular loop, quick workflow, dynamic workflow, blocked, ask-user를 구분한다.
- 최소 pattern family가 구현되고 각 pattern의 barrier/synthesis semantics가 테스트된다.
- Verifier child가 별도 context와 rubric으로 결과를 검증하고, failure가 final success를 막는다.
- Workflow child는 parent scope를 초과하지 않는 tool registry와 Tool Search catalog만 사용한다.
- Quarantine pattern이 untrusted reader와 privileged actor를 분리한다.
- Worktree isolation이 write-capable child에 대해 생성, 실행, diff, verifier, merge handoff, cleanup evidence를 제공한다.
- Budget, timeout, parallelism, model routing snapshot이 runtime에서 관찰 가능하다.
- Interruption/restart 이후 workflow가 checkpoint 기준으로 resume 또는 visible blocked state가 된다.
- Saved workflow recipe가 skill로 제공될 수 있고, skill이 permission을 얻지 못한다는 테스트가 있다.
- CLI/TUI/local API/channel projection이 workflow progress, verifier, budget, blocked/resume state를 일관되게 표시한다.
- Diagnostics/replay가 harness plan, child graph, verifier verdict, merge decision, stale result를 redacted evidence로 남긴다.
- Rust 검증은 관련 crate 기준 `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`를 통과한다.
- 문서와 사용자 가이드는 dynamic workflows를 prototype 또는 provider-native beta로 과장하지 않는다.

---

## 명시적 비목표

- 모든 작업에 dynamic workflow를 자동 적용하지 않는다.
- JavaScript workflow interpreter를 필수 요구사항으로 삼지 않는다.
- 외부 workflow marketplace를 만들지 않는다.
- 조직 관리자 승인 체계를 도입하지 않는다.
- Skill을 executable plugin code로 승격하지 않는다.
- Child끼리 직접 session state를 공유하거나 수정하게 하지 않는다.
- Verifier를 비용만 쓰는 장식 단계로 두지 않는다.

---

## 설계 판단 요약

이 spec의 핵심 판단은 다음이다.

`shacs-bot`의 dynamic workflows는 Claude Code의 기능을 복제하는 것이 아니라, 작업별 하네스라는 아이디어를 `MainOrchestrator` 중심의 Rust self-hosted runtime 계약으로 재해석하는 것이다. 완성형 목표는 “복잡한 작업을 더 많이 병렬화한다”가 아니라, 복잡한 작업을 독립 컨텍스트, 격리 worktree, adversarial verification, budget, checkpoint, diagnostics 아래에서 끝까지 검증 가능하게 완료하는 것이다.
