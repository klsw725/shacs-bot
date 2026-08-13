# Spec 032 closure evidence

Verdict: `Complete (Scoped)`

Spec 032는 local self-hosted app authoring, foreground process lifecycle, activation provenance projection 범위에서 닫는다. Detached daemon, auto-reexec, managed Python/Node runtime 설치, live plugin reload, local API/TUI parity는 보장하지 않는다.

## Requirement mapping

| Owner | 구현 | 집중 증거 |
|---|---|---|
| PRD 000 | `AppSupervisor`, lifecycle journal, `apps start/stop/restart/recover`, controlled-child cleanup | `spec032_app_lifecycle`, `spec032_app_supervisor`, CLI transcript |
| PRD 001 | proposal, revision/installed-digest CAS, checkpoint, apply, verify, install/update recovery | `spec032_app_authoring_flow`, `apps propose/apply` transcript |
| PRD 002 | app/source/content/dependency provenance와 active/stale/disabled/revoked/removed/untrusted projection | `spec032_app_extension_provenance` |
| PRD 003 | Cargo gates, real CLI smoke, user documentation, owner-boundary audit | 이 문서와 `docs/USAGE.md` |

## Owner-fact audit

- 030 owner facts: workspace trust는 config의 explicit trusted workspace assertion에서 읽고, process 실행은 기존 controlled-child process-group cleanup 경계를 소비한다.
- 031 owner facts: executable skill이 선언된 app start는 persisted activation record를 읽어 missing/stale/disabled/revoked/removed blocker로 변환한다. Install/apply는 activation record를 생성하지 않는다.
- 035 handoff: CLI는 typed lifecycle receipt를 투영한다. Local API/TUI parity는 032 closure에서 주장하지 않는다.

## Reproducible commands

```sh
cargo fmt --manifest-path crates/Cargo.toml --all -- --check
cargo clippy --manifest-path crates/Cargo.toml --workspace --all-targets -- -D warnings
cargo test --manifest-path crates/Cargo.toml --workspace
cargo clean --manifest-path crates/Cargo.toml
cargo build --manifest-path crates/Cargo.toml --locked --workspace
```

Focused test targets:

```sh
cargo test --manifest-path crates/Cargo.toml -p shacs-app --test spec032_app_lifecycle --test spec032_app_authoring_flow
cargo test --manifest-path crates/Cargo.toml -p shacs-core --test spec032_app_supervisor --test spec032_app_extension_provenance
cargo test --manifest-path crates/Cargo.toml -p shacs-cli parser_handles_apps_command_surface --lib
```

## Real-surface observations

- `apps propose` produced a static validation/risk receipt and reported `Applied: no`, `Runtime authorization created: no`.
- `apps apply` produced checkpoint/install handoff and reported no runtime authorization, activation, or process start.
- Starting an installed but not enabled app produced a completed failed receipt with zero process dispatches.
- After explicit enable, foreground `apps start` accepted a separate `apps stop` request, dispatched one controlled child, and completed in `Stopped`.
- `apps recover` read the same journal and completed without process dispatch.
- Unknown app start failed with `unknown app` rather than creating lifecycle truth.

## Non-guarantees

- Authoring apply is not permission, credential, activation, or replay authorization.
- Install/enable do not resolve required secrets and do not create grant references.
- App runtime is foreground-owned; no daemonization, external process-manager replacement, or automatic reexec is provided.
- Runtime declaration executes only explicit argv. Natural-language install commands and global package/system installers are not inferred.
- Extension provenance is a deterministic projection over owner facts; it does not create or mutate activation decisions.
- Replay readers report zero discovery, dependency preparation, credential resolution, and entrypoint dispatch.
- Concrete dependency environment layout and managed language runtime installation remain outside this scoped closure.
