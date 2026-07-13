# PRD 002: verifier and adversarial review

## 목표

Verifier graph와 adversarial review 결과가 workflow success를 fail-closed로 제어하도록 runtime contract를 고정한다. Verifier는 장식 단계가 아니라, required verifier가 누락되거나 fail/uncertain verdict를 반환하면 synthesis success를 막는 release-critical gate다.

## 범위

- verifier verdict kind
- verifier verdict target child binding
- missing verifier detection
- fail/uncertain verdict가 target child failure로 표면화되는 verification gate
- merge policy의 `require_verifier_pass`와 synthesis integration

## 비범위

- verifier child 실행 방식
- verifier prompt/rubric generation
- external source lookup implementation
- reviewer pool 또는 조직 승인 workflow

## 구현 매핑

- `crates/shacs-workflow/src/lib.rs`
  - `WorkflowVerifierVerdictKind`
  - `WorkflowVerifierVerdict`
  - `WorkflowVerificationGate`
  - `workflow_verification_gate`
  - `workflow_synthesis_outcome`
- `crates/shacs-workflow/tests/workflow.rs`
  - `workflow_barrier_verifier_and_synthesis_fail_closed`

## SPEC 입력

1. 주관 spec은 `docs/specs/024-dynamic-workflows-and-harness-orchestration/SPEC.md`다.
2. PRD 001의 child graph와 synthesis outcome을 소비한다.
3. subagent execution boundary는 011을 소비한다.
4. diagnostics/evidence는 014/018을 소비한다.

## Dependency Cut

1. Verifier는 child graph와 분리된 independent review node다.
2. Verifier verdict 없이 required verification workflow를 success로 닫으면 안 된다.
3. Verifier는 session truth를 직접 수정하지 않는다.
4. 비용만 쓰는 장식 review 단계는 비목표다.

## 데이터/상태 모델

1. `WorkflowVerifierNode`: target child/output, rubric, required evidence, timeout/budget snapshot을 가진다.
2. `VerifierVerdict`: pass, fail, inconclusive, timed_out를 구분한다.
3. `VerifierRubric`: goal match, constraints, tests, safety, documentation checks를 가진다.
4. `VerifierEvidence`: independent observation refs와 redaction status를 가진다.

## 정상 시퀀스

1. child output이 verifier input으로 전달된다.
2. verifier가 별도 context와 rubric으로 실행된다.
3. verdict와 evidence가 workflow event로 기록된다.
4. synthesis가 verifier pass를 확인한 뒤 final success를 허용한다.

## 실패 시퀀스

1. verifier fail/inconclusive/timeout은 final success를 막는다.
2. verifier가 child evidence를 그대로 복사만 하면 independent evidence 부족으로 blocked된다.
3. stale verifier result는 synthesis에 섞이지 않는다.

## 검증 관점

1. verifier failure가 final success를 막는 test를 먼저 둔다.
2. timeout/inconclusive verdict가 blocked로 표시되는지 확인한다.
3. independent evidence requirement를 snapshot으로 검증한다.

## Cargo 검증

1. `cargo fmt --manifest-path crates/Cargo.toml -p shacs-workflow -- --check`
2. `cargo clippy --manifest-path crates/Cargo.toml -p shacs-workflow --all-targets -- -D warnings`
3. `cargo test --manifest-path crates/Cargo.toml -p shacs-workflow workflow_verifier`

## 완료 기준

- plan의 required verifier verdict가 없으면 verification gate는 blocked다.
- verifier가 pass가 아닌 verdict를 반환하면 gate는 failed다.
- failed gate는 target child id를 잃지 않는다.
- `require_verifier_pass`가 true인 merge policy에서는 verifier pass 전 final success가 불가능하다.
- verifier failure나 missing verifier를 completed child result로 덮어쓸 수 없다.
