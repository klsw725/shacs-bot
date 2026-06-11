# PRD 007: sequential implementation plan

## 목표

Spec 020은 PRD 000-005가 이미 구현 증거를 가진 상태다. 이 PRD의 목표는 남은 closure work인 PRD 006을 020-owned 범위에서 닫고, plugin-provided tool 구현은 Spec 025로 handoff하는 검증 게이트를 고정하는 것이다.

이 문서는 새 기능 범위를 넓히지 않는다. 구현자는 아래 순서대로 작업해 user-facing configuration diagnostics, source-kind evidence, release evidence를 닫는다.

## Dependency Cut

1. PRD 000-005는 완료된 foundation으로 소비한다.
2. PRD 006은 남은 user-facing closure와 025 plugin tool boundary handoff를 소유한다.
3. Spec 025의 plugin discovery/activation/tool registration은 020 closure blocker가 아니라 025 구현 범위다.
4. Spec 004 tool runtime, 010 host safety, 022 auto approval boundary는 재구현하지 않고 소비한다.
5. Provider-native Tool Search beta, marketplace, core tool deferral은 이 PRD 범위가 아니다.

## 구현 순서

### Wave 1. 현재 Tool Search closure inventory

목표: 기존 완료 PRD 000-005의 구현이 PRD 006 착수 기준을 만족하는지 확인한다.

작업:

1. `tools.toolSearch` normalized config와 runtime 전달 경로가 유지되는지 확인한다.
2. surface assembler가 core visible set과 deferrable set을 분리하는지 확인한다.
3. bridge dispatcher가 current deferred scope만 unwrap하는지 확인한다.
4. runner provider request live wiring이 bridge routing을 통과하는지 확인한다.
5. MCP default-deny와 child registry-only catalog regression이 살아 있는지 확인한다.
6. diagnostics/replay evidence가 bridge call과 underlying tool mapping을 남기는지 확인한다.

완료 기준:

- 기존 Tool Search focused tests가 pass한다.
- Spec 025가 plugin tool integration을 추가할 seam과 owner boundary가 확인된다.

### Wave 2. User-facing config and diagnostics evidence

목표: 사용자가 Tool Search가 켜졌는지, pass-through 되었는지, 왜 그런지 runtime diagnostics evidence에서 설명할 수 있게 한다.

작업:

1. `enabled=auto|on|off`, `thresholdPct`, `searchDefaultLimit`, `maxSearchLimit` 의미를 사용자-facing PRD 문서와 runtime config evidence에서 같은 의미로 설명한다.
2. 현재 turn 기준 activation reason을 `off`, `threshold`, `forced_on`, `no_deferrable_tools`, `bridge_collision`, `unknown_context_window` family로 구분한다.
3. deferred catalog summary에 deferred count와 source kind count를 포함한다.
4. activation event detail/result를 redaction-safe하게 표시한다.

완료 기준:

- runtime diagnostics event에서 activation reason, deferred count, source kind count를 확인할 수 있다.
- config 문서와 runtime evidence의 field 이름이 서로 어긋나지 않는다.

### Wave 3. Spec 025 plugin-provided tool handoff

목표: enabled plugin tool이 core tool로 승격되지 않고 deferrable catalog 후보로만 들어가야 한다는 boundary를 Spec 025로 넘긴다.

작업:

1. Spec 025 PRD 000이 plugin discovery/activation state를 소유한다고 명시한다.
2. Spec 025 PRD 002가 plugin-provided tool registration, source kind, deferrable classification, exclusion diagnostics를 소유한다고 명시한다.
3. 020은 future plugin tool이 `tool_call`로 실행될 때도 004/010/022 gate를 통과해야 한다는 boundary만 유지한다.

완료 기준:

- Spec 025 PRD 000/002가 plugin implementation owner로 연결된다.
- 020 closure gate는 Spec 025 implementation evidence를 요구하지 않는다.

### Wave 4. Scope regression matrix

목표: Tool Search가 권한 확장 경로가 되지 않는다는 기존 invariant를 020 구현 범위에서 고정하고 plugin extension은 Spec 025 handoff로 남긴다.

작업:

1. core tools never defer regression을 유지한다.
2. unknown/unclassifiable tool visible retention regression을 유지한다.
3. MCP `enabledTools` default-deny regression을 유지한다.
4. child registry-only catalog regression을 유지한다.
5. bridge `tool_call`이 visible/core/out-of-scope tool을 거부하는 regression을 유지한다.
6. plugin tool disabled/blocked/not-enabled exclusion regression은 Spec 025 PRD 002로 넘긴다.

완료 기준:

- regression matrix가 config, assembler, bridge, MCP, child scope, replay, diagnostics를 포함한다.
- 어떤 current source kind도 bridge를 통해 current registry scope 밖 tool을 실행할 수 없다.

### Wave 5. Release evidence and docs closure

목표: Spec 020을 구현 완료로 전환할 수 있는 증거를 정리한다.

작업:

1. PRD 000-006 evidence checklist를 하나의 release gate로 묶되 Spec 025 plugin implementation evidence를 요구하지 않는다.
2. user docs는 provider-native beta나 marketplace를 구현 완료처럼 설명하지 않는다.
3. diagnostics bundle 또는 inspect snapshot은 activation reason, deferred count, source kind count, bridge mapping을 포함한다.
4. replay evidence는 destructive underlying tool을 live 재실행하지 않는다.

완료 기준:

- PRD 006 구현 상태 섹션에 실제 파일, 테스트, cargo 검증 명령을 기록할 수 있다.
- Spec 020 완료 기준의 모든 020-owned bullet이 evidence ref로 연결된다.

## 전체 완료 기준

- PRD 000-006 전체가 020-owned 구현 상태와 검증 증거를 가진다.
- Tool Search activation과 pass-through 이유를 사용자가 로컬에서 이해할 수 있다.
- plugin-provided tool 구현은 Spec 025로 handoff되고, 020은 core tool 승격 금지와 deferrable boundary를 문서화한다.
- Tool Search는 core, MCP, plugin, subagent scope 어디에서도 권한 확장 경로가 아니다.
- 관련 Rust 변경은 해당 crate 기준 `cargo fmt --manifest-path crates/shacs-core/Cargo.toml -- --check`, `cargo clippy --manifest-path crates/shacs-core/Cargo.toml --all-targets -- -D warnings`, `cargo test --manifest-path crates/shacs-core/Cargo.toml`처럼 manifest path를 명시해 통과한다.
