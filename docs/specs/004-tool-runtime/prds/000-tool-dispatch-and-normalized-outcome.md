# PRD 000. tool dispatch and normalized outcome

## 목표

이 문서는 `docs/specs/004-tool-runtime/SPEC.md`의 하위 실행 문서다. 목표는 tool registry 조회, 실행 envelope 검증, dispatch, permission snapshot 준수, normalized outcome 재진입까지 포함한 tool runtime 구현 계획을 고정하는 것이다.

- 오케스트레이터가 승인한 `Effect::RunTool`만 실행되게 한다.
- tool 결과를 tool별 제멋대로가 아닌 공통 outcome envelope로 정규화한다.
- permission snapshot 불일치, timeout, cancellation, late result를 명시적으로 처리한다.

## SPEC 입력

- 주관 spec: `docs/specs/004-tool-runtime/SPEC.md`
- 교차 의존:
  - `docs/specs/001-session-kernel/SPEC.md`
  - `docs/specs/002-command-event-effect/SPEC.md`
  - `docs/specs/007-main-orchestrator-policy/SPEC.md`
  - `docs/specs/008-configuration-profiles-and-runtime-layout/SPEC.md`

## Dependency Cut

이 PRD는 tool runtime 경계와 normalized outcome에 집중한다. 개별 tool 알고리즘, sandbox 강화, host safety taxonomy의 전체 문서는 별도 범위다. 여기서는 실행 계약과 재진입 결과 표준화를 완성한다.

## 범위

- tool registry 조회 모델과 dispatch entrypoint
- `RunTool` envelope 검증
- permission snapshot 확인과 실행 가드
- 완료, 실패, timeout, cancellation outcome 정규화
- 구조화 결과와 텍스트 결과의 공통 envelope
- 재진입 command 변환과 late result 방어

## 범위 제외

- 개별 tool 구현 세부 알고리즘
- 원격 plugin marketplace
- shell sandbox의 완전한 보안 설계
- 멀티세션 리소스 스케줄링

## 현재 구현 상태

### 이미 반영된 것

- tool definition, registry, builtin executors, permission/resource checks는 `crates/shacs-core/src/tools/`와 `crates/shacs-core/src/runtime/tool_execution.rs`에 구현돼 있다.
- provider tool 요청은 runtime runner의 tool loop로 전환되고 normalized result/progress event로 provider context와 observability hook에 연결된다.
- tool reentry는 `ToolPending + RunTool`일 때만 수용되며, 성공/실패 결과는 `ToolOutcomeRecorded`로 기록되고 replay에 반영된다.
- concrete filesystem `read` executor와 `proc_exec` executor가 runtime adapter 계층에 연결돼 있으며, configured provider submit 경로에서 read tool roundtrip이 검증된다.
- permission snapshot, working directory, network/secret scope, artifact ref safety가 dispatch 전후 boundary로 검증된다.
- `input_schema_ref`는 알려진 schema ref만 통과시키며 malformed JSON, shape mismatch, unknown schema ref는 executor 진입 전 normalized failure로 거절한다.
- `RunToolEffect.resource_limits`는 `max_output_bytes=N` 형식으로 텍스트 결과 상한을 적용하고 truncation observation을 남긴다.
- safe runtime artifact list는 결과로 통과하고, runtime artifact root 밖의 binary/list reference는 normalized failure로 거절한다.
- cancelled tool outcome은 `ToolCallCancelled` reentry로 정규화되고, 오케스트레이터가 outcome 기록 후 턴 abort를 결정한다.

### 아직 남은 것

- shell write, 원격 network tool, secret read 같은 추가 tool family는 현재 기본 runtime path가 아니다.
- proc 실행 sandbox와 cancellation은 self-hosted 최소 범위의 structured argv/timeout 경계에 머물며, OS별 강화 sandbox는 별도 확장 범위다.
- 실행 중인 외부 프로세스에 대한 out-of-band cancellation signal 전달은 현재 FullSpec slice 밖이다. 이 문서의 cancellation 증거는 normalized cancelled outcome과 late-result/abort 경계에 한정한다.

### 로컬 근거

- `crates/shacs-core/src/tools/registry.rs`
- `crates/shacs-core/src/tools/`
- `crates/shacs-core/src/runtime/tool_execution.rs`
- `crates/shacs-core/src/runtime/runner.rs`
- `crates/shacs-core/tests/tools.rs`
- `crates/shacs-core/tests/runtime.rs`
- `crates/shacs-core/tests/runtime_agent.rs`

## TDD 계획

1. registry에 없는 tool 실행 요청이 정규화된 실패로 반환되는 테스트를 작성한다.
2. permission snapshot과 실제 요청이 모순될 때 실행이 거부되는 테스트를 작성한다.
3. stdout, stderr, 구조화 데이터가 공통 outcome envelope로 정규화되는 테스트를 작성한다.
4. timeout과 cancellation이 별도 상태로 반환되는 테스트를 작성한다.
5. 이미 닫힌 턴에 대한 tool 결과가 late result로 처리되는 테스트를 작성한다.

## 구현 웨이브

### Wave 1. registry와 envelope 타입 정리

- registry 항목의 최소 필드를 Rust 타입으로 고정한다.
- `RunToolEffect`와 `ToolCallOutcome`를 분리한다.
- envelope validation 실패를 명시적 오류 상태로 돌리는 경로를 만든다.

### Wave 2. dispatch와 executor adapter 연결

- `tool_name` 기준 registry 조회 후 적절한 executor kind로 dispatch한다.
- 실행 기준 경로, timeout, 리소스 제한을 executor 호출에 전달한다.
- runtime이 effect를 다른 effect로 재작성하지 못하도록 경계를 고정한다.

### Wave 3. permission snapshot 및 outcome 정규화

- snapshot과 요청 capability, 경로 범위를 대조하는 실행 가드를 구현한다.
- 텍스트, JSON, 파일 참조 등 결과 표현을 공통 outcome envelope 아래 정리한다.
- 실패 유형을 입력 검증, 조회 실패, I/O 실패, 정규화 실패 등으로 구분한다.

### Wave 4. 재진입 및 late result 처리

- `ToolCallCompleted`, `ToolCallFailed`, `ToolCallTimedOut`, `ToolCallCancelled` 재진입 command를 연결한다.
- 오케스트레이터가 승인하지 않은 tool 결과는 세션 기록으로 승격되지 못하게 한다.
- timeout 또는 취소 후 뒤늦게 온 완료 신호는 late result로 분류한다.

## Verification Evidence

- Integration evidence: `crates/shacs-core/tests/runtime.rs`, `crates/shacs-core/tests/runtime_agent.rs`, and `crates/shacs-core/tests/runtime_loop.rs` cover tool-call execution, result-message mapping, concurrent execution boundaries, checkpointed normalized tool results, throttled result handling, and provider/tool progress forwarding.
- Safety evidence: `crates/shacs-core/tests/tools.rs` covers registry validation, filesystem/exec path restrictions, SSRF allowlist checks, symlink escape rejection, output truncation, and sensitive self-tool path blocking.
- Durability evidence: `crates/shacs-core/tests/runtime_agent.rs` and `crates/shacs-core/tests/runtime_loop.rs` cover checkpoint persistence during tool execution, runtime checkpoint materialization, and session recovery context boundaries.

## Open Risks

- registry 메타데이터와 실제 executor 능력이 어긋나면 permission 의미가 흔들릴 수 있다.
- 결과 정규화를 과도하게 단순화하면 tool 특성별 디버깅 정보가 사라질 수 있다.
- runtime root와 artifact 참조 규칙이 약하면 외부 경로 노출이 섞일 수 있다.
- OS별 강한 sandbox와 out-of-band process cancellation은 아직 product 최소 범위 밖이므로, 이를 요구하는 새 tool family를 추가할 때 별도 spec update가 필요하다.

## 종료 기준

- 승인된 `RunTool` effect만 실행된다.
- 모든 tool 결과는 공통 normalized outcome으로 재진입한다.
- timeout, cancellation, late result, permission 불일치가 각각 구분된다.
- `docs/specs/004-tool-runtime/SPEC.md`의 permission 경계와 금지 패턴이 구현에 반영된다.
