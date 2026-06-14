# PRD 001: context file discovery and ordering

## 목표

Workspace context file을 deterministic하게 발견하고 provider input ordering을 고정한다. Discovery는 convenience 기능일 뿐, permission이나 system prompt 권한을 만들지 않는다.

## 범위

- default context filename list
- workspace root to current directory discovery
- config-provided extra context files
- symlink and workspace boundary handling
- deterministic ordering
- size limit and skipped diagnostics

## 비범위

- hosted workspace policy rollout
- organization-admin context distribution
- executable context file format

## 구현 요구사항

1. 기본 candidate filename은 `AGENTS.md`, `CLAUDE.md`, `.cursorrules`, `.shacs.md`, `.shacs-bot.md`로 시작해야 한다.
2. Discovery는 workspace root에서 current directory까지 deterministic order로 수행해야 한다.
3. Workspace boundary 밖 파일은 explicit allow path 없이 포함하면 안 된다.
4. Symlink는 canonical path가 workspace boundary와 protected target policy를 통과해야 한다.
5. 너무 큰 context file은 truncate 또는 skip하고 evidence를 남겨야 한다.
6. Context file은 user-context로 들어가며 system prompt authority를 갖지 않아야 한다.

## SPEC 입력

1. 주관 spec은 `docs/specs/026-context-files-and-inline-references/SPEC.md`다.
2. workspace/runtime layout은 `docs/specs/008-configuration-profiles-and-runtime-layout/SPEC.md`를 소비한다.
3. filesystem safety는 `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`를 소비한다.
4. context assembly handoff는 `docs/specs/009-context-assembly-and-compaction-input/SPEC.md`를 소비한다.
5. diagnostics는 `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`를 소비한다.

## Dependency Cut

1. Discovery는 context file 후보와 ordering만 소유한다.
2. Context file은 executable code가 아니며 permission을 얻지 않는다.
3. Provider input formatting과 budget cut은 PRD 002가 소유한다.
4. Workspace boundary 밖 discovery는 explicit allow path 없이는 금지된다.
5. organization-wide context rollout은 비범위다.

## 구현 매핑

| Requirement | Likely crate/module | Test perspective |
|---|---|---|
| context filename defaults | `crates/shacs-config/src/lib.rs`, `crates/shacs-core/src/runtime/context_files.rs` | default candidate order |
| root-to-current discovery | `crates/shacs-core/src/runtime/context_files.rs` | nested directory ordering |
| symlink/workspace boundary | `crates/shacs-core/src/tools/fs.rs` 또는 safety module | outside-root symlink denied |
| discovery diagnostics | `crates/shacs-core/src/runtime/diagnostics.rs` | included/skipped/truncated snapshot |

## 데이터/상태 모델

1. `ContextFileCandidate`: path, source directory depth, filename kind, configured/default source를 가진다.
2. `ContextFileDiscoveryOrder`: root, intermediate, current, config-provided order를 stable하게 보존한다.
3. `ContextFileReadStatus`: `included`, `skipped_missing`, `denied_boundary`, `truncated`, `parse_error`를 구분한다.
4. `ContextFileDigest`: content digest와 size/token estimate를 가진다.
5. `ContextFileProjection`: user-facing inspect를 위한 path, order, status, reason read model이다.

## 정상 시퀀스

1. runtime이 workspace root와 current directory를 받는다.
2. root에서 current directory까지 candidate filename을 deterministic하게 찾는다.
3. canonical path가 workspace boundary를 통과한다.
4. size limit 안의 file은 artifact 후보가 되고 digest를 계산한다.
5. discovery snapshot은 009 handoff와 diagnostics가 소비할 수 있게 저장된다.

## 실패 시퀀스

1. candidate가 symlink로 workspace 밖을 가리키면 denied status가 된다.
2. oversized file은 truncation 또는 skipped evidence를 남긴다.
3. unreadable file은 전체 turn failure가 아니라 skipped diagnostic이다.
4. duplicate filename은 deterministic order로 모두 기록되며 silent override하지 않는다.
5. context file content는 system prompt authority로 승격되지 않는다.

## 검증 관점

1. 첫 failing test는 nested directory context files가 root-to-current order로 정렬되는지 확인한다.
2. symlink outside workspace, oversized file, missing file fixture를 둔다.
3. diagnostics snapshot은 included/skipped/truncated reason을 보여야 한다.
4. config-provided extra context files가 default candidates 뒤에서 deterministic하게 정렬되는지 확인한다.

## Cargo 검증

1. `cargo fmt --manifest-path crates/shacs-core/Cargo.toml -- --check`
2. `cargo clippy --manifest-path crates/shacs-core/Cargo.toml --all-targets -- -D warnings`
3. `cargo test --manifest-path crates/shacs-core/Cargo.toml context_file`
4. config를 건드렸다면 `cargo test --manifest-path crates/shacs-config/Cargo.toml context`

## 완료 기준

- Root, nested directory, duplicate filename, symlink, oversized file case가 테스트된다.
- Diagnostics가 included/skipped/truncated reason을 표시한다.

## 구현 상태

Status: Implemented for PRD 001 discovery/order boundary only. Provider input formatting, resolver behavior, budget handoff, replay, and user-facing CLI/API projection remain open in later PRDs.

Evidence:

- `crates/shacs-core/src/runtime/context_files.rs` adds default context filename discovery, root-to-current directory ordering, configured extra context files, workspace-boundary denial, truncation evidence, digest and token estimate fields.
- Public runtime exports are available through `crates/shacs-core/src/runtime/mod.rs`.
- `cargo test --manifest-path crates/shacs-core/Cargo.toml context_file` passes with root/nested ordering, duplicate filename, symlink outside workspace, oversized file, configured extra, and missing-file coverage.
