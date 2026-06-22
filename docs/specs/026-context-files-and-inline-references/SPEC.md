# context files and inline references 아키텍처 명세

Status: Implemented for live runtime closure. Hermes의 context files와 inline `@` reference 제품 의미론을 `shacs-bot`의 Rust self-hosted runtime에 맞게 재해석해, context assembly 이후 단계의 user-controlled context injection owner boundary를 고정한다.

## 문서 목적

`009-context-assembly-and-compaction-input`은 provider input assembly와 compaction input의 현재 경계를 이미 닫은 문서다. 이 문서는 009를 다시 여는 보강이 아니라, 사용자가 workspace 안팎의 문맥을 명시적으로 제공하는 context file discovery와 inline reference resolution을 별도 owner로 둔다.

Hermes reference에서 가져올 것은 다음 제품 의미론이다.

- workspace별 context instruction file을 자동 발견해 prompt/context assembly에 포함한다.
- 하위 디렉터리로 이동할수록 가까운 context file을 추가로 발견할 수 있다.
- user message 안의 `@file`, `@folder`, `@diff`, `@staged`, `@git`, `@url` 같은 reference는 runtime이 해석해 bounded context artifact로 바꾼다.
- reference resolution은 permission, redaction, token budget, diagnostics를 통과해야 한다.
- context file과 inline reference는 session truth를 직접 수정하지 않고 provider input의 ephemeral context로만 들어간다.

그대로 가져오지 않을 것은 특정 product의 파일명 우선순위, hosted workspace policy, team-admin context rollout, remote reference marketplace, provider-specific prompt feature다. 초기 구현은 local filesystem, git working tree, URL fetch 같은 self-hosted/personal-use에 필요한 source만 다룬다.

---

## 상위 기준과의 관계

| spec | 026이 소비하는 것 | 026이 소유하는 것 |
|---|---|---|
| 001 session kernel | session event와 message truth | context reference가 session message 원문을 mutate하지 않는 규칙 |
| 003 provider runtime | canonical provider request | resolved context artifact가 provider input에 들어가기 전 shape |
| 007 main orchestrator | turn admission and policy loop | user message reference resolution을 어느 시점에 수행할지 |
| 008 config/runtime layout | workspace/profile config path | context file names, discovery depth, reference enable config |
| 009 context assembly | context assembly and compaction input | user-controlled context files와 inline references의 source/resolution owner |
| 010 host safety | filesystem permission, URL/network safety, redaction | reference source별 permission gate와 protected target behavior |
| 014 diagnostics | redacted diagnostics and inspect evidence | context file discovery, reference resolution, budget skip evidence |
| 016 verification gates | release evidence taxonomy | context reference regression and documentation evidence |
| 018 evaluation/replay | trajectory and replay evidence | replay가 live URL/git side effect 없이 resolved evidence를 해석하는 규칙 |

026은 009의 provider input assembly를 대체하지 않는다. 026은 assembly에 넘길 context artifact를 만드는 owner다.

---

## 범위

이 문서는 다음을 정의한다.

- context file candidate names and discovery order.
- workspace root와 current working directory 기준 discovery scope.
- inline reference grammar and resolver taxonomy.
- filesystem, git, URL reference permission and redaction rules.
- folder summarization과 size/token budget behavior.
- resolved artifact shape and provider input handoff.
- diagnostics, replay, CLI/TUI/local API projection.
- 구현 PRD 분할과 closure 기준.

이 문서는 다음을 정의하지 않는다.

- long-term semantic memory.
- vector search or indexing.
- hosted document connector.
- organization-wide policy distribution.
- provider-native attachments feature.
- remote plugin marketplace for reference resolvers.

---

## 핵심 정의

### context file

Context file은 workspace나 subdirectory에 있는 user-authored instruction/context 문서다. 초기 candidate는 다음처럼 제한한다.

```text
AGENTS.md
CLAUDE.md
.cursorrules
.shacs.md
.shacs-bot.md
```

이 목록은 config로 확장 가능하되 기본값은 작게 유지한다. Context file은 executable code가 아니며 permission을 얻지 않는다.

### inline reference

Inline reference는 user message 안의 `@...` token을 runtime이 해석해 context artifact로 바꾸는 표식이다. Reference token은 user-authored text의 일부로 남고, resolved content는 별도 ephemeral context block으로 provider input에 추가된다.

초기 reference kind:

- `@path/to/file`: single file content.
- `@path/to/folder`: bounded folder listing and selected file summaries.
- `@diff`: working tree diff.
- `@staged`: staged diff.
- `@git:<rev>` 또는 `@git:<rev>:<path>`: git object or path at revision.
- `@url:<https-url>` 또는 bare `@https://...`: fetched URL content.

### resolved context artifact

Resolved context artifact는 provider input에 넘길 normalized context block이다. 최소 필드는 다음이다.

```text
kind
source
display_name
content
byte_count
token_estimate
digest
redaction_status
truncation_status
permission_evidence
```

Artifact는 durable session truth가 아니다. Replay와 diagnostics를 위해 digest와 redacted excerpt/evidence는 남길 수 있지만, raw external content를 무조건 저장하지 않는다.

---

## Context File Discovery

Discovery는 workspace root에서 current working directory까지 가까워지는 순서로 수행한다. 동일 filename이 여러 directory에 있으면 모두 candidate가 될 수 있지만, provider input ordering은 deterministic해야 한다.

권장 ordering:

1. workspace root context files.
2. intermediate directory context files.
3. current directory context files.
4. explicit config-provided context files.

Discovery rule:

1. 파일은 workspace boundary 밖을 기본 탐색하지 않는다.
2. symlink는 canonical path가 workspace boundary와 protected target policy를 통과해야 한다.
3. 너무 큰 파일은 size limit을 적용하고 truncation evidence를 남긴다.
4. context file parse 실패는 전체 turn 실패가 아니라 skipped diagnostics다.
5. context file 내용은 user instruction이지만 system prompt 권한을 갖지 않는다.

---

## Inline Reference Resolution

Reference resolution은 provider call 직전, user message normalization 이후, context assembly handoff 전에 수행한다. Resolution은 current workspace, config, permission mode, network policy를 입력으로 받는다.

Resolution rule:

1. Reference parser는 code block 내부와 escaped `\@` token을 기본적으로 해석하지 않는다.
2. Ambiguous token은 해석하지 않고 원문으로 둔다.
3. Filesystem path는 workspace root 또는 explicit allow path 안에서만 resolve한다.
4. Folder reference는 recursive full dump가 아니라 bounded listing과 file summary 후보를 만든다.
5. Git reference는 read-only command/source로만 동작하고 working tree를 변경하지 않는다.
6. URL reference는 network permission과 content-type/size limit을 통과해야 한다.
7. Resolver별 failure는 artifact-level skipped/error block으로 남고 전체 turn을 실패시키지 않는다. 단, 사용자가 strict mode를 켠 경우 ask-user 또는 blocked turn으로 올릴 수 있다.

---

## Budget and Provider Input

Context files와 inline references는 provider context budget을 공유한다. Budget priority는 기본적으로 다음 순서를 따른다.

1. active user message.
2. required system/developer/runtime instructions.
3. explicitly referenced inline artifacts.
4. nearest context files.
5. broader ancestor context files.

Inline reference는 사용자가 turn에서 명시한 것이므로 자동 발견 context file보다 우선한다. 하지만 explicit reference라도 protected target, permission, size, redaction gate를 넘지 못하면 포함하지 않는다.

Budget overflow는 silent drop이 아니라 truncation 또는 skipped evidence를 남겨야 한다.

---

## Safety and Replay

불변식:

1. Context file과 inline reference는 permission을 부여하지 않는다.
2. Reference resolution은 session message 원문을 직접 수정하지 않는다.
3. Protected path와 secret-like content는 010의 redaction/gate를 통과해야 한다.
4. URL fetch는 user-controlled external input으로 취급하고 prompt injection label을 가져야 한다.
5. Folder reference는 repository 전체를 무제한 dump하지 않는다.
6. Replay는 live URL fetch나 mutable git state를 다시 실행하지 않고 recorded digest/evidence를 사용해야 한다.
7. Diagnostics는 raw secret이나 oversized content를 저장하지 않는다.

---

## User-Facing Projection

Inspect surface는 최소한 아래를 보여야 한다.

- discovered context file count and paths.
- included/skipped/truncated context file status.
- inline reference tokens and resolved kind.
- permission denied, missing path, network disabled, unsupported content type.
- byte/token budget usage.
- redaction/truncation status and digest.

CLI/TUI/local API command 이름은 013이 최종 UX로 조정할 수 있지만 의미는 다음을 제공해야 한다.

```text
context files list
context files inspect
context refs parse <message>
context refs resolve <message>
```

`parse`는 side-effect-free dry run이어야 하고, `resolve`도 read-only source만 사용해야 한다.

---

## PRD 분할

1. `prds/000-reference-grammar-and-resolution-model.md`: inline `@` grammar, parser, escaped/code-block handling, resolved artifact model.
2. `prds/001-context-file-discovery-and-ordering.md`: context filename defaults, root-to-current discovery, symlink/workspace boundary, ordering.
3. `prds/002-provider-input-budget-and-handoff.md`: budget priority, truncation/skipped evidence, 009 assembly handoff, context block formatting.
4. `prds/003-filesystem-git-url-resolvers.md`: file/folder/git/url resolver contracts, read-only command usage, network and content-type gates.
5. `prds/004-permission-redaction-and-replay-safety.md`: protected target, secret redaction, prompt-injection labeling, replay evidence.
6. `prds/005-user-facing-diagnostics-and-release-evidence.md`: CLI/TUI/API projection, diagnostics bundle, release gate and docs evidence.
7. `prds/006-sequential-implementation-plan.md`: PRD 000-005의 parser, discovery, resolver, budget, safety, diagnostics 구현 순서와 gate.

---

## 완료 기준

- Context file discovery가 deterministic하고 workspace boundary를 넘지 않는다.
- Inline `@` parser가 file/folder/diff/staged/git/url reference를 구분하고 code block/escape를 존중한다.
- Resolved artifact가 provider input으로 들어가기 전에 permission, redaction, budget gate를 통과한다.
- Explicit reference는 자동 context file보다 우선하지만 safety gate를 우회하지 않는다.
- Folder/git/url resolver는 read-only이고 bounded output을 보장한다.
- Diagnostics와 replay가 live refetch 없이 resolved evidence를 해석한다.
- UI/API projection은 included/skipped/truncated/denied reason을 보여준다.
- 문서는 context references를 long-term memory, vector search, hosted connector로 과장하지 않는다.

## 구현 Evidence

- `crates/shacs-core/src/runtime/agent_loop.rs`는 일반 user turn에서 현재 메시지의 inline `@` reference와 workspace/current-directory context files를 resolver, safety gate, budget handoff에 태워 `ContextProviderHandoff`를 만들되, 기존 system prompt bootstrap으로 이미 소비되는 workspace-root bootstrap files는 provider context에 중복 주입하지 않는다.
- `crates/shacs-core/src/runtime/runner.rs`는 handoff block을 provider 요청 메시지에만 ephemeral user-context로 주입해 system prompt 권한으로 승격하지 않고, `AgentRunResult.messages`와 session 원문 메시지에는 저장하지 않는다.
- 집중 회귀 테스트는 provider가 context block을 받는지, current-directory context file과 configured live budget이 live handoff에 적용되는지, workspace-root bootstrap files와 symlink alias가 provider context에 중복되지 않는지, legacy bootstrap symlink가 protected/outside target을 system prompt로 읽지 않는지, `ask_user` resume/finalization retry에도 같은 context가 들어가는지, session/result message에 context block이 남지 않는지 검증한다.
