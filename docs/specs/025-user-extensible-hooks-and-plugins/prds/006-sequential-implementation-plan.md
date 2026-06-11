# PRD 006: sequential implementation plan

## 목표

Spec 025의 user-extensible hooks/plugins를 안전한 순서로 구현할 수 있게 PRD 000-005를 dependency-ordered wave로 묶는다. Plugin은 discovery만으로 실행되면 안 되므로, 구현 순서는 activation state와 diagnostics를 먼저 닫고, hook/tool/command 실행 surface는 permission과 replay safety 이후에 연결한다.

이 PRD는 public marketplace, in-process dynamic plugin ABI, organization governance를 추가하지 않는다.

## Dependency Cut

1. 008은 plugin config path와 profile/runtime layout을 제공한다.
2. 004는 plugin tool execution이 통과해야 할 tool runtime boundary를 제공한다.
3. 005는 plugin-provided Markdown skill이 소비해야 할 read-only skill boundary를 제공한다.
4. 010/022는 permission, protected target, approval ceiling을 제공한다.
5. 014/018은 diagnostics, replay, redaction evidence를 제공한다.
6. 020은 enabled plugin tool이 deferrable catalog 후보가 되는 조건을 소비한다.

## 구현 순서

### Wave 1. Manifest discovery without execution

소유 PRD: `000-plugin-manifest-discovery-and-config-gates.md`

목표: plugin directory를 발견하더라도 user opt-in 전에는 실행 가능한 surface가 생기지 않게 한다.

작업:

1. user-data plugin root와 workspace-local plugin root를 분리한다.
2. manifest schema, version, digest, source root, surfaces, required refs를 parse한다.
3. plugin state를 `not_enabled`, `enabled`, `disabled`, `blocked`로 구분한다.
4. `disabled`가 `enabled`보다 우선하게 한다.
5. workspace-local plugin은 explicit trust gate 없이는 executable surface로 load하지 않는다.
6. broken manifest와 missing env/config ref는 blocked/not-ready diagnostics로 남긴다.

게이트:

- 이 wave에서는 hook callback, command-backed tool, command execution을 호출하지 않는다.
- discovered plugin이 provider-visible tool, active skill, command list에 나타나면 안 된다.

### Wave 2. Management and diagnostics shell

소유 PRD: `005-user-facing-management-and-diagnostics.md`

목표: 실행 surface를 열기 전에 사용자가 plugin 상태와 blocked reason을 이해할 수 있게 한다.

작업:

1. `plugins list`, `plugins inspect`, `plugins enable`, `plugins disable`, `plugins doctor` 의미를 구현한다.
2. `hooks list`, `hooks inspect`는 아직 callback 실행 없이 subscription metadata만 보여준다.
3. enable/disable은 running session prompt/toolset을 조용히 mutate하지 않는다.
4. inspect는 manifest digest, source root, surfaces, missing refs, blocked reason을 보여준다.

게이트:

- 사용자-facing enable 이후에도 reload/next-session semantics가 명확해야 한다.
- blocked plugin의 surface가 active runtime에 들어가면 안 된다.

### Wave 3. Hook event catalog and observer-only dispatch

소유 PRD: `001-hook-event-catalog-and-dispatch.md`

목표: hook event catalog를 runtime에 연결하되, 먼저 observer-only hook부터 timeout/error isolation을 검증한다.

작업:

1. hook event payload를 versioned, bounded, redacted shape로 정의한다.
2. `runtime:start`, `runtime:stop`, `session:start`, `session:end`, `llm:after`, `tool:after`, `subagent:end` 같은 observer-only event를 dispatch한다.
3. hook timeout, invalid output, callback failure가 runtime crash가 아니라 hook diagnostic으로 남게 한다.
4. hook dispatch count, last error, timeout count를 diagnostics에 남긴다.

게이트:

- observer-only hook이 command/tool/provider behavior를 바꾸면 안 된다.
- hook failure가 runtime turn failure로 승격되면 안 된다.

### Wave 4. Limited behavior-affecting hooks

소유 PRD: `001-hook-event-catalog-and-dispatch.md`, `004-permission-secret-and-replay-safety.md`

목표: behavior-affecting hook을 제한 event에만 연결하고 permission approval을 만들 수 없게 한다.

작업:

1. `tool:before`는 block 제안만 허용하고 allow/approval을 만들지 못하게 한다.
2. `llm:before` injection은 system prompt가 아니라 ephemeral user-message side context로만 붙인다.
3. `command:before`와 `channel:inbound` rewrite/skip은 router/orchestrator 재검증을 통과하게 한다.
4. `tool:transform_result`는 output size limit과 redaction pass를 통과하게 한다.
5. conflict order와 first-block-wins 같은 deterministic merge rule을 고정한다.

게이트:

- hook이 permission ceiling을 높이는 경로가 없어야 한다.
- hook output이 session truth를 직접 mutate하면 안 된다.

### Wave 5. Command-backed plugin tools

소유 PRD: `002-command-backed-plugin-tools.md`

목표: plugin-provided tool을 core tool 확장 없이 command/MCP-backed boundary로 등록한다.

작업:

1. plugin tool source kind와 schema digest를 registry metadata로 보존한다.
2. command-backed handler는 bounded JSON args와 bounded JSON/text result contract를 따른다.
3. handler timeout, exit status, stderr summary를 redacted diagnostics에 남긴다.
4. plugin tool은 004 runtime executor와 010/022 permission boundary를 통과한다.
5. enabled plugin tool만 020 Tool Search deferrable candidate로 연결한다.

게이트:

- plugin tool이 core/builtin tool로 승격되면 안 된다.
- disabled/blocked plugin tool은 provider-visible surface와 deferred catalog에 없어야 한다.

### Wave 6. Plugin skills and commands

소유 PRD: `003-plugin-skills-and-commands.md`

목표: plugin-provided skill과 command를 기존 skill registry와 command router 경계로만 연결한다.

작업:

1. plugin skill source kind와 namespace를 `plugin:<plugin-name>/<skill-name>` 형태로 보존한다.
2. disabled/blocked plugin의 skill은 active/available set에서 제외한다.
3. skill name conflict는 자동 병합하지 않고 diagnostic으로 남긴다.
4. plugin command는 command router로 재진입하고 session store를 직접 수정하지 않는다.
5. command help/inspect는 source plugin과 required permission을 표시한다.

게이트:

- plugin skill이 permission이나 tool visibility를 얻으면 안 된다.
- plugin command가 MainOrchestrator/command router를 우회하면 안 된다.

### Wave 7. Secret, permission, replay hardening

소유 PRD: `004-permission-secret-and-replay-safety.md`

목표: extension surface가 local runtime safety를 약화하지 않도록 공통 hardening을 닫는다.

작업:

1. plugin permission ceiling request와 granted runtime ceiling을 분리한다.
2. secret ref metadata만 manifest에 저장하고 raw secret value는 저장하지 않는다.
3. handler env는 explicit allow-list/ref만 포함한다.
4. diagnostics args/result/env는 redaction pass 후 저장한다.
5. replay는 plugin tool/hook side effect를 live-dispatch하지 않고 recorded evidence를 사용한다.

게이트:

- secret leakage regression과 replay no-live-dispatch regression이 통과해야 한다.
- project-local plugin trust gate가 없는 executable surface load는 blocked여야 한다.

### Wave 8. Release evidence closure

소유 PRD: `005-user-facing-management-and-diagnostics.md`

목표: Spec 025 closure를 선언할 수 있는 사용자-facing evidence와 문서화를 닫는다.

작업:

1. discovery, hook dispatch, plugin tool, plugin skill/command, safety/replay, UI projection evidence bucket을 모두 채운다.
2. user docs는 opt-in, blocked reason, reload/next-session semantics, secret refs, trust gate를 설명한다.
3. diagnostics bundle은 plugin state, hook errors, plugin tool mapping, redaction status를 포함한다.
4. public marketplace, Python loader, organization governance를 구현 완료처럼 주장하지 않는다.

게이트:

- 사용자가 어떤 plugin/hook이 왜 load되지 않았는지 알 수 있어야 한다.
- release evidence 없이 enabled executable plugin surface를 supported라고 표시하면 안 된다.

## 전체 완료 기준

- Discovery와 execution이 분리되어 있고 새 plugin 기본 상태는 `not_enabled`다.
- Hook dispatch는 observer-only와 제한 behavior-affecting event를 구분한다.
- Plugin tool, skill, command는 각 owner boundary를 우회하지 않는다.
- Plugin은 permission ceiling을 높이거나 raw secret을 자동 획득하지 못한다.
- Replay와 diagnostics는 live side effect 없이 plugin/hook behavior를 설명할 수 있다.
- 관련 Rust 변경은 해당 crate 기준 `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`를 통과한다.
