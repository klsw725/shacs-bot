# PRD 006: sequential implementation plan

## 목표

Spec 026의 context files와 inline `@` references를 구현 가능한 순서로 묶는다. Reference parser와 artifact model이 모든 resolver와 provider input handoff의 선행 조건이므로, 구현자는 아래 순서대로 parser, discovery, resolver, budget, safety, diagnostics를 닫아야 한다.

이 PRD는 long-term memory, vector search, hosted connector, provider-native attachment feature를 추가하지 않는다.

## Dependency Cut

1. 009는 provider input assembly와 compaction input을 제공한다. 026은 assembly에 넘길 context artifact를 만든다.
2. 010은 filesystem/network permission, protected target, redaction boundary를 제공한다.
3. 014는 diagnostics and inspect evidence를 제공한다.
4. 018은 replay/evaluation evidence를 제공한다.
5. 013은 CLI/TUI/local API rendering과 command naming을 소유한다.
6. 026 implementation은 session message 원문을 mutate하지 않는다.

## 구현 순서

### Wave 1. Reference grammar and artifact model

소유 PRD: `000-reference-grammar-and-resolution-model.md`

목표: user message에서 inline `@` reference를 파싱하되, 아직 source content를 읽지 않는다.

작업:

1. escaped `\@`, fenced code block, adjacent punctuation, email/handle-like text를 구분하는 parser를 만든다.
2. `file`, `folder`, `diff`, `staged`, `git`, `url`, `unsupported`, `unresolved` kind를 구분한다.
3. parser output은 original span, normalized target, kind, parse diagnostic을 포함한다.
4. resolved context artifact model을 정의하되 resolver는 stub 또는 not-yet-resolved 상태만 만든다.
5. parser failure는 turn failure가 아니라 reference-level diagnostic으로 남긴다.

게이트:

- 이 wave에서는 filesystem/git/url content를 읽지 않는다.
- parser가 user message text를 rewrite하면 안 된다.

### Wave 2. Context file discovery and ordering

소유 PRD: `001-context-file-discovery-and-ordering.md`

목표: workspace context files를 deterministic order로 찾고 포함 후보를 만든다.

작업:

1. default candidate filename list를 구현한다.
2. workspace root에서 current directory까지 discovery order를 고정한다.
3. config-provided extra context files를 deterministic order로 추가한다.
4. symlink canonical path와 workspace boundary를 검사한다.
5. oversized context file은 truncate 또는 skip evidence를 남긴다.

게이트:

- context file은 system prompt authority를 얻지 않는다.
- workspace boundary 밖 파일은 explicit allow path 없이 포함하지 않는다.

### Wave 3. Filesystem and folder resolvers

소유 PRD: `003-filesystem-git-url-resolvers.md`

목표: `@file`과 `@folder`를 read-only, bounded artifact로 해석한다.

작업:

1. file resolver는 regular file, workspace/protected target policy, size limit을 통과해야 한다.
2. folder resolver는 recursive full dump가 아니라 bounded listing과 selected summary를 반환한다.
3. binary file은 raw text inline 대신 actionable metadata artifact로 처리한다.
4. missing path, denied path, unsupported file type은 skipped/denied artifact로 남긴다.
5. artifact digest, byte count, token estimate, redaction status를 채운다.

게이트:

- folder reference가 repository 전체를 무제한 dump하면 안 된다.
- protected path denial이 raw secret/path details를 과도하게 노출하면 안 된다.

### Wave 4. Git and URL resolvers

소유 PRD: `003-filesystem-git-url-resolvers.md`, `004-permission-redaction-and-replay-safety.md`

목표: `@diff`, `@staged`, `@git`, `@url`을 read-only source로 해석한다.

작업:

1. `@diff`와 `@staged`는 read-only working tree/staged diff source만 사용한다.
2. `@git:<rev>`와 `@git:<rev>:<path>`는 working tree를 변경하지 않는다.
3. URL resolver는 HTTPS, network permission, timeout, content-type, size limit을 통과해야 한다.
4. URL content는 untrusted external context label을 가진다.
5. missing revision, unsupported URL content, disabled network는 artifact-level error/skipped state로 남긴다.

게이트:

- git resolver가 mutable operation을 수행하면 안 된다.
- URL fetch failure가 전체 turn crash가 되면 안 된다.

### Wave 5. Provider input budget and 009 handoff

소유 PRD: `002-provider-input-budget-and-handoff.md`

목표: resolved artifact를 009 context assembly가 소비할 수 있는 typed input으로 넘긴다.

작업:

1. budget priority를 active user message, required runtime instructions, explicit inline artifacts, nearest context files, broader ancestor context files 순서로 적용한다.
2. explicit reference는 auto context file보다 우선하지만 safety gate를 우회하지 않는다.
3. budget overflow는 skipped/truncated evidence를 남긴다.
4. context block formatting은 source label, trust label, truncation label을 포함한다.
5. provider input handoff는 session message 원문 rewrite가 아니라 artifact list injection으로 수행한다.

게이트:

- budget overflow silent drop이 없어야 한다.
- artifact가 provider input에 들어가기 전 permission/redaction gate를 통과해야 한다.

### Wave 6. Permission, redaction, replay safety

소유 PRD: `004-permission-redaction-and-replay-safety.md`

목표: context reference가 permission, secret, replay boundary를 약화하지 않게 한다.

작업:

1. protected target deny와 secret-like content redaction을 resolver output과 diagnostics 양쪽에 적용한다.
2. URL/external content에는 prompt-injection label을 붙인다.
3. strict mode가 아닌 reference denial은 skipped evidence로 처리한다.
4. replay는 live URL fetch나 mutable working tree diff를 다시 실행하지 않고 recorded digest/excerpt/evidence를 사용한다.
5. diagnostics bundle은 raw secret과 oversized content를 저장하지 않는다.

게이트:

- reference artifact가 tool permission이나 approval state를 부여하면 안 된다.
- replay no-refetch regression이 통과해야 한다.

### Wave 7. User-facing diagnostics and release evidence

소유 PRD: `005-user-facing-diagnostics-and-release-evidence.md`

목표: 사용자가 context file/reference가 왜 포함, 스킵, 절단, 거부되었는지 알 수 있게 한다.

작업:

1. `context files list/inspect` projection을 구현한다.
2. `context refs parse <message>`는 source read 없이 token/kind/target을 보여준다.
3. `context refs resolve <message>`는 read-only resolver와 permission/redaction/budget status를 보여준다.
4. diagnostics는 byte/token budget, redaction/truncation, denied/skipped reason을 포함한다.
5. user docs는 supported syntax, limits, safety behavior, replay behavior를 설명한다.

게이트:

- 사용자가 `@file`이 포함되지 않은 이유를 알 수 있어야 한다.
- release evidence가 parser, discovery, resolver, budget, safety, replay, docs를 모두 요구해야 한다.

## 전체 완료 기준

- Parser는 code block, escape, email/handle, URL, git, path case를 안정적으로 구분한다.
- Context file discovery는 deterministic하고 workspace boundary를 넘지 않는다.
- Filesystem, folder, git, URL resolver는 read-only이고 bounded output을 보장한다.
- Resolved artifact는 provider input handoff 전에 permission, redaction, budget gate를 통과한다.
- Diagnostics/replay는 live refetch 없이 included/skipped/truncated/denied reason을 설명한다.
- 관련 Rust 변경은 해당 crate 기준 `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`를 통과한다.
