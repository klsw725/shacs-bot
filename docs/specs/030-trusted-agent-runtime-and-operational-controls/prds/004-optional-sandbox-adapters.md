# PRD 004. optional sandbox adapters

Status: Complete (Scoped)

Implementation evidence: `crates/shacs-core/tests/spec030_sandbox_adapter.rs`가 fallback/required/policy/active fact를 검증하며, Linux active lane은 별도 strict record로 closure runner에 입력한다.

## Goal

OS-level 또는 외부 sandbox를 adapter별 선택 기능으로 제공하고 적용 범위와 fallback을 정확히 표시한다.

## Scope

1. Local bash sandbox adapter.
2. macOS `sandbox-exec`, Linux `bubblewrap` 또는 동등 runtime integration.
3. Filesystem read/write와 network 정책 설정.
4. External cloud/container sandbox adapter.
5. `active`, `disabled`, `unsupported`, `failed` 상태와 runtime profile별 fallback.

## Invariants

1. Sandbox는 실제 wrapper를 통과한 adapter에만 적용된다.
2. Bash sandbox는 Python kernel, extension, package manager, MCP, daemon을 자동 포함하지 않는다.
3. Unsupported platform과 initialization failure는 사용자에게 표시한다.
4. `trusted_native_fallback`은 active가 아닌 상태를 경고하고 native 실행을 허용한다. `sandbox_required`는 active가 아니면 해당 adapter 실행을 거부한다.
5. External sandbox의 network default와 비용·lifecycle은 별도 서비스 사실로 표시한다.

## Acceptance Criteria

1. Active bash sandbox에서 denyRead, allowWrite, network policy가 실제 명령으로 검증된다.
2. Disabled/unsupported/failed 상태에서 `trusted_native_fallback`은 native fallback 경고를, `sandbox_required`는 실행 거부를 표시한다.
3. Sandbox status와 적용 adapter 목록이 CLI/TUI/API에서 parity를 유지한다.
4. `--no-sandbox` 또는 동등 설정이 current state에 반영된다.
5. 문서가 sandbox를 universal containment 또는 kernel isolation으로 표현하지 않는다.
