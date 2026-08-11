# PRD 002. path-specific process lifecycle controls

Status: Complete (Scoped)

Implementation evidence: `crates/shacs-core/tests/spec030_process_controlled_child.rs`와 `spec030_sandbox_adapter.rs`; Linux active bwrap lane은 `SHACS_REQUIRE_BWRAP=1`로 실제 실행한다.

## Goal

공통 process envelope 대신 각 실제 spawn adapter의 lifecycle control과 비보장을 명시한다.

## Adapter matrix

| Adapter | Required controls | Explicit limits |
|---|---|---|
| Bash | cwd existence, timeout, abort, process-tree kill, bounded output | workspace containment 없음 |
| Generic exec | argv spawn, cwd, env merge, bounded output, timeout, abort, descendant TERM→KILL | static policy 없음 |
| Credential command resolver | shell command, stdout capture, timeout, abort, process-lifetime cache | typed secret boundary 없음 |
| Package operation | source resolution, install/update outcome, selected timeout | universal approval 없음 |
| Python kernel | cwd/env, startup, interrupt, restart, shutdown | OS 권한 축소 없음 |
| Daemon worker | generation fencing, readiness, crash recovery, cleanup | security isolation 없음 |
| MCP | HTTP lifecycle 또는 Python-managed stdio disclosure | common host spawn gate 없음 |

## Invariants

1. Adapter마다 실제 제공하는 timeout, cancellation, env, cwd, cleanup을 별도로 기록한다.
2. Parent environment를 사용하는 경로는 이를 숨기지 않는다.
3. Timeout은 side-effect rollback이 아니다.
4. Direct spawn 경로가 존재함을 diagnostics와 문서에서 인정한다.
5. 공통 gate가 없다는 사실은 current trusted model의 explicit contract다.
6. Command-backed credential resolution은 generic exec와 별도 adapter로 진단하되 같은 timeout·abort·bounded-output 기준을 적용한다.

## Scoped baseline

1. 현재 등록된 Bash, generic argv exec, configured credential command, configured package command, daemon ownership lease, MCP adapter만 실제 control evidence를 요구한다.
2. Persistent Python kernel adapter는 현재 scoped baseline에 등록하지 않는다. Kernel capability는 `unsupported`와 reason을 투영하고 lifecycle control이나 isolation을 주장하지 않는다.
3. Package operation은 사용자가 구성한 `program + args` 실행과 결과만 소유한다. Dependency solver, package-manager 설치, global mutation은 제공하지 않는다.
4. 등록되지 않은 adapter를 `supported`로 표시하거나 공통 process gate가 존재한다고 추론하지 않는다.

## Acceptance Criteria

1. Bash와 generic exec의 success, bounded output, timeout, abort, descendant cleanup, invalid cwd가 실제 process QA를 통과한다.
2. Daemon stale generation과 startup failure가 orphan worker를 남기지 않는다.
3. 등록된 kernel adapter는 interrupt·restart·shutdown을 실제 surface에서 검증한다. Adapter가 없는 scoped baseline은 `unsupported`와 reason을 투영하고 kernel isolation을 주장하지 않는다.
4. Package와 MCP 경로가 자신이 제공하지 않는 control을 표시하지 않는다.
5. Process diagnostics가 adapter, cwd summary, timeout state, terminal outcome을 표시한다.
6. Command-backed credential resolver의 success, timeout, non-zero exit, empty output, cache behavior를 검증한다.
