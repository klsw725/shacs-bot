# PRD 003: filesystem git url resolvers

## 목표

초기 inline reference source인 file, folder, diff, staged, git, URL resolver의 read-only bounded contract를 고정한다.

## 범위

- file resolver
- folder resolver
- `@diff` and `@staged`
- `@git:<rev>` and `@git:<rev>:<path>`
- `@url:<https-url>` and bare https URL reference
- content-type, size, timeout gates

## 비범위

- arbitrary shell command references
- mutable git operation
- authenticated web connector
- browser rendering or screenshot extraction

## 구현 요구사항

1. File resolver는 workspace/protected target policy를 통과한 regular file만 읽어야 한다.
2. Folder resolver는 recursive full dump가 아니라 bounded listing과 selected summary를 반환해야 한다.
3. Diff/staged resolver는 read-only git status/diff source만 사용해야 한다.
4. Git resolver는 working tree를 변경하지 않아야 하며 missing revision/path를 artifact-level error로 남겨야 한다.
5. URL resolver는 HTTPS를 기본으로 하고 network permission, timeout, content-type, size limit을 통과해야 한다.
6. URL content는 untrusted external context label을 가져야 한다.

## SPEC 입력

1. 주관 spec은 `docs/specs/026-context-files-and-inline-references/SPEC.md`다.
2. filesystem/protected target safety는 `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`를 소비한다.
3. URL/network permission은 010/022의 permission mode를 소비한다.
4. artifact model은 PRD 000을 소비한다.
5. budget and provider handoff는 PRD 002가 소비한다.

## Dependency Cut

1. Parser와 artifact model이 선행되어야 한다.
2. Resolver는 read-only source access만 수행한다.
3. Git resolver는 working tree를 변경하지 않는다.
4. URL resolver는 authenticated connector나 browser rendering을 구현하지 않는다.
5. arbitrary shell command reference는 비범위다.

## 구현 매핑

| Requirement | Likely crate/module | Test perspective |
|---|---|---|
| file/folder resolver | `crates/shacs-core/src/runtime/context_refs.rs`, filesystem safety helper | outside workspace denied |
| diff/staged/git resolver | `crates/shacs-core/src/runtime/context_refs.rs` | read-only git fixture |
| URL resolver | `crates/shacs-core/src/tools/web.rs` 또는 runtime adapter | disabled network and oversized content |
| artifact normalization | `crates/shacs-core/src/runtime/context.rs` | digest/redaction status required |

## 데이터/상태 모델

1. `FileReferenceTarget`: canonical path, display path, optional line range, file kind를 가진다.
2. `FolderReferenceSummary`: bounded listing, omitted count, selected summary entries를 가진다.
3. `GitReferenceTarget`: revision, optional path, query kind(diff/staged/object)을 가진다.
4. `UrlReferenceTarget`: normalized HTTPS URL, content type, fetch size, timeout status를 가진다.
5. `ResolverOutput`: artifact or skipped/denied error plus evidence digest를 가진다.

## 정상 시퀀스

1. parser가 file/folder/git/url target을 resolver에 넘긴다.
2. resolver가 workspace, permission, size, content-type gate를 확인한다.
3. source를 read-only로 가져와 artifact content 또는 summary를 만든다.
4. artifact는 digest, byte count, token estimate, redaction status를 가진다.
5. budget planner가 후속 포함 여부를 결정한다.

## 실패 시퀀스

1. outside-workspace path는 denied artifact가 된다.
2. folder가 너무 크면 bounded listing과 omitted count만 남긴다.
3. missing git rev/path는 artifact-level error가 된다.
4. disabled network 또는 oversized URL은 skipped diagnostic으로 남는다.
5. binary file은 raw text inline이 아니라 metadata/actionable block으로 처리한다.

## 검증 관점

1. 첫 failing test는 outside-workspace `@file`이 denied되는지 확인한다.
2. folder limit, missing git rev, disabled network, oversized URL fixture를 둔다.
3. URL content에는 untrusted external context label이 있어야 한다.
4. 모든 resolver output은 digest와 redaction status를 가져야 한다.

## Cargo 검증

1. `cargo fmt --manifest-path crates/shacs-core/Cargo.toml -- --check`
2. `cargo clippy --manifest-path crates/shacs-core/Cargo.toml --all-targets -- -D warnings`
3. `cargo test --manifest-path crates/shacs-core/Cargo.toml context_resolver`
4. URL fetch adapter를 건드렸다면 `cargo test --manifest-path crates/shacs-core/Cargo.toml web`

## 완료 기준

- Resolver tests가 outside-workspace path, folder limit, missing git rev, disabled network, oversized URL을 포함한다.
- 모든 resolver output은 digest와 redaction status를 가진다.
