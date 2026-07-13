# PRD 010: sequential implementation plan

## 목표

Spec 024의 PRD 000-009를 실제 구현 가능한 순서로 묶는다. Dynamic workflows는 상태 모델, child graph, verifier, worktree, budget, permission, resume, projection, runtime wiring이 서로 의존하므로, 구현자는 아래 순서를 건너뛰지 않아야 한다.

이 PRD는 새 workflow feature를 추가하지 않는다. 목적은 prototype이 아니라 closure까지 갈 수 있는 dependency-ordered execution plan을 제공하는 것이다.

## Dependency Cut

1. MainOrchestrator가 session truth와 workflow state transition의 최종 권한자다.
2. Subagent runtime은 011의 child execution boundary를 소비한다.
3. Tool Search child scope는 020을 소비한다.
4. Permission ceiling과 approval은 010/022를 소비한다.
5. Diagnostics/replay evidence는 014/018을 소비한다.
6. UI rendering은 013이 소유하므로 이 PRD는 shared projection data와 action semantics만 정의한다.
7. JavaScript workflow interpreter, external marketplace, organization fleet orchestration은 비범위다.

## 구현 순서

### Wave 1. Typed foundation and admission

소유 PRD: `000-workflow-state-and-harness-plan.md`

목표: 실행 전에 모든 후속 wave가 공유할 workflow identity, state, harness plan, admission result, checkpoint schema를 고정한다.

작업:

1. `WorkflowRunState`, `WorkflowPattern`, `WorkflowHarnessPlan`, admission input/result를 Rust 타입으로 정의한다.
2. plan digest, origin session/turn, permission ceiling, model/budget snapshot을 stable evidence로 만든다.
3. admission helper가 regular loop, quick workflow, dynamic workflow, ask-user, blocked를 구분하게 한다.
4. terminal state와 resume decision을 fail-closed로 만든다.

게이트:

- 이 wave가 끝나기 전에는 child spawn, verifier, worktree mutation을 구현하지 않는다.
- typed plan 없이 workflow execution branch로 들어가면 안 된다.

### Wave 2. Pattern engine and child graph without side effects

소유 PRD: `001-pattern-engine-and-child-graph.md`

목표: fan-out, barrier, synthesis graph를 순수 계획/상태 전이로 먼저 닫는다.

작업:

1. workflow pattern taxonomy를 admission result에서 harness plan으로 연결한다.
2. child graph, barrier, fan-out, synthesis node를 typed data로 표현한다.
3. child output contract와 evidence ref shape를 정의한다.
4. barrier가 열리기 전 final success를 만들 수 없게 한다.

게이트:

- 이 wave에서는 실제 child process/spawn을 실행하지 않는다.
- graph invariant test가 없는 상태로 verifier/worktree wave로 넘어가지 않는다.

### Wave 3. Runtime execution wiring for read-only child paths

소유 PRD: `009-runtime-execution-wiring-and-monitoring.md`

목표: MainOrchestrator admission에서 child execution handoff, progress event, final synthesis까지 최소 read-only 경로를 실제 runtime에 연결한다.

작업:

1. dynamic workflow branch를 runner/orchestrator execution path에 연결한다.
2. child context copy와 session truth isolation을 보장한다.
3. child progress/completion event를 monitorable event로 발행한다.
4. final synthesis는 child output을 소비하되 child가 session store를 직접 mutate한 것처럼 취급하지 않는다.

게이트:

- write-capable child, worktree merge, quarantine escalation은 아직 열지 않는다.
- read-only child workflow smoke가 통과해야 다음 wave로 간다.

### Wave 4. Verifier and adversarial review gate

소유 PRD: `002-verifier-and-adversarial-review.md`

목표: verifier가 장식 단계가 아니라 final success를 막을 수 있는 독립 gate가 되게 한다.

작업:

1. verifier graph와 rubric input을 child graph와 분리한다.
2. verifier child는 별도 context와 evidence requirement를 갖는다.
3. verifier failure, inconclusive, timeout이 final success를 막게 한다.
4. synthesis는 verifier verdict와 evidence ref를 함께 소비한다.

게이트:

- verifier result 없이 success를 닫는 경로가 있으면 안 된다.
- verifier가 parent session truth를 직접 수정하면 안 된다.

### Wave 5. Budget, timeout, model routing

소유 PRD: `004-budget-timeout-and-model-routing.md`

목표: workflow run이 무제한 비용/시간/모델 선택으로 확장되지 않게 한다.

작업:

1. workflow, child, verifier별 budget snapshot을 생성한다.
2. timeout과 max parallelism을 runtime에서 enforce한다.
3. model routing decision은 plan/evidence로 남긴다.
4. budget exhaustion은 success가 아니라 blocked/failed state가 된다.

게이트:

- budget snapshot 없는 child spawn을 금지한다.
- timeout을 ignored success로 포장하는 경로가 없어야 한다.

### Wave 6. Permission ceilings and quarantine

소유 PRD: `006-quarantine-and-permission-ceilings.md`

목표: untrusted reader와 privileged actor를 분리하고 child가 parent보다 넓은 권한을 갖지 않게 한다.

작업:

1. child permission ceiling을 harness plan에서 runtime registry로 전달한다.
2. Tool Search child catalog가 child registry 밖 tool을 보지 못하게 한다.
3. quarantine pattern은 untrusted content reader와 high-privilege actor를 분리한다.
4. approval integration은 workflow recipe나 skill이 permission을 부여하지 못하게 한다.

게이트:

- child가 parent scope 밖 tool을 search/describe/call할 수 있으면 중단한다.
- untrusted reader output이 privileged action으로 바로 연결되면 안 된다.

### Wave 7. Worktree isolation and merge handoff

소유 PRD: `003-worktree-isolation-and-merge-handoff.md`

목표: write-capable child를 isolated worktree로 제한하고 merge를 user-visible handoff로 만든다.

작업:

1. write-capable child는 assigned worktree 밖을 수정하지 못한다.
2. child diff evidence를 redaction-safe하게 수집한다.
3. merge approval과 cleanup diagnostics를 분리한다.
4. verifier가 diff evidence를 검토할 수 있게 한다.

게이트:

- worktree cleanup failure가 success로 숨겨지면 안 된다.
- merge는 child가 직접 parent session/worktree에 적용하는 동작이 아니다.

### Wave 8. Skill-backed recipes

소유 PRD: `005-skill-backed-workflow-recipes.md`

목표: workflow recipe를 skill로 제공하되 skill이 permission이나 execution authority를 얻지 못하게 한다.

작업:

1. recipe metadata와 skill discovery를 연결한다.
2. malformed/conflicting recipe를 blocked diagnostics로 남긴다.
3. saved workflow inspect surface에 recipe source와 digest를 표시한다.
4. recipe가 harness plan 생성을 도울 수는 있어도 permission ceiling을 높이지 못하게 한다.

게이트:

- skill body가 executable workflow code로 승격되면 안 된다.
- recipe conflict가 silent override가 되면 안 된다.

### Wave 9. Resume, replay, diagnostics

소유 PRD: `007-resume-replay-and-diagnostics.md`

목표: interruption/restart 이후 workflow가 resume 또는 visible blocked state가 되게 한다.

작업:

1. checkpoint resume, stale child result, late verifier result를 구분한다.
2. replay는 destructive child/tool action을 live 재실행하지 않는다.
3. diagnostics bundle은 plan, graph, child/verifier events, merge decision, blocked reason을 포함한다.
4. stale result가 final synthesis에 섞이지 않게 한다.

게이트:

- resume 실패를 완료로 간주하면 안 된다.
- replay가 live side effect를 발생시키면 안 된다.

### Wave 10. User-facing projection and release closure

소유 PRD: `008-user-facing-projection-and-release-gates.md`

목표: CLI/TUI/local API/channel이 workflow 상태를 과장 없이 같은 의미로 보여주고, Spec 024 closure evidence를 닫는다.

작업:

1. workflow progress, child graph, verifier, budget, blocked/resume state projection을 정의한다.
2. user interruption, cancel, inspect, resume action semantics를 정리한다.
3. release checklist가 PRD 000-009 evidence를 모두 요구하게 한다.
4. 사용자 문서가 dynamic workflows를 prototype 또는 provider-native feature로 과장하지 않게 한다.

게이트:

- projection이 success/failure/blocker를 숨기면 안 된다.
- release gate가 verifier, budget, permission, replay evidence 없이 pass하면 안 된다.

## 전체 완료 기준

- PRD 000-009가 모두 구현 상태와 검증 증거를 가진다.
- Read-only workflow, verifier-gated workflow, permission-scoped child workflow, worktree-isolated workflow, interrupted/resumed workflow smoke가 존재한다.
- Child와 verifier는 session truth를 직접 수정하지 않는다.
- Budget, timeout, permission ceiling, Tool Search child scope가 runtime에서 enforce된다.
- Diagnostics/replay evidence만으로 workflow 실행 흐름을 설명할 수 있다.
- 관련 Rust 변경은 workspace manifest 기준 `cargo fmt --manifest-path crates/Cargo.toml --all -- --check`, `cargo clippy --manifest-path crates/Cargo.toml -p shacs-core --all-targets -- -D warnings`, `cargo test --manifest-path crates/Cargo.toml -p shacs-core`처럼 범위를 명시해 통과한다.
