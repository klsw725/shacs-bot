# PRD 006. audit, diagnostics, replay, and contract matrix

## 구현 상태

Status: Implemented.

구현 증거는 `crates/shacs-core/src/runtime/permission_audit.rs`, `crates/shacs-core/src/runtime/permission_replay.rs`, `crates/shacs-core/src/runtime/permission_policy.rs`, `crates/shacs-core/tests/permission_policy.rs`, `crates/shacs-core/tests/runtime_loop.rs`다. 대표 테스트는 `minimal_audit_record_is_redacted_and_has_decision_evidence`, `permission_audit_diagnostics_count_decisions_and_failure_reasons`, `permission_replay_invariants_are_fail_closed_for_old_denies`, `permission_contract_matrix_declares_required_release_evidence_buckets`, `replay_runner_executes_selected_cases_only_and_never_dispatches_live_tools`다.

이 closure는 redacted typed audit record, diagnostics summary, replay invariant, release evidence bucket을 닫는다. Physical storage backend, diagnostics UI layout, provider-specific trace format은 이 PRD가 구현했다고 주장하지 않는다. PRD 007의 recent classifier denial summary는 이 PRD의 redaction 기준을 소비한다.

## 목표

이 문서는 Spec 022의 permission decision을 audit, diagnostics, replay, contract matrix로 닫는 최종 PRD다.

목표는 auto-approved, asked, denied action이 모두 설명 가능하고, raw secret을 남기지 않으며, replay와 release gate에서 회귀를 잡을 수 있게 하는 것이다.

## SPEC 입력

1. 주관 spec은 `docs/specs/022-auto-approval-permissions/SPEC.md`다.
2. PRD 000부터 PRD 005까지의 타입과 decision을 모두 소비한다.
3. diagnostics와 inspection surface는 `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`를 따른다.
4. release gate와 test family는 `docs/specs/016-verification-matrix-and-release-gates/SPEC.md`를 따른다.
5. replay/evaluator evidence language는 `docs/specs/018-evaluation-automation-and-self-improvement/SPEC.md`를 따른다.

## Dependency Cut

1. 014는 diagnostics rendering, inspection, redaction surface를 계속 소유한다.
2. 016은 release evidence family와 gate 의미를 계속 소유한다.
3. 018은 evaluator ledger와 replay run language를 계속 소유한다.
4. 022는 permission decision audit record, replay invariant, contract matrix를 소유한다.
5. 이 PRD는 실제 UI와 storage backend를 세부 구현하지 않는다.

## 범위

1. Permission audit record field.
2. Redaction requirement.
3. Diagnostics explanation questions.
4. Replay input and invariant.
5. Contract test matrix.
6. Release evidence checklist.
7. Failure evidence classification.

## 범위 제외

1. Diagnostics UI layout.
2. Zip bundle physical format.
3. Provider-specific trace format.
4. Full replay runner implementation.
5. 원격 배포 승인 절차.
6. 여러 설치본을 한꺼번에 평가하는 상태 기준.

## 구현 요구사항

1. 모든 permissioned action은 final decision과 관계없이 audit record를 남겨야 한다.
2. Audit record는 action id, session id, turn id, tool name, capabilities, target summary, argument digest, mode, decision, decision reason, evaluator ref, approval ref, containment summary, created at을 포함해야 한다.
3. Audit record는 raw secret, full env, raw token, unredacted private URL credential을 저장하면 안 된다.
4. Diagnostics는 왜 action이 자동 승인됐는지, 왜 사용자 확인으로 넘어갔는지, 왜 거절됐는지 설명할 수 있어야 한다.
5. Diagnostics는 mode snapshot, containment snapshot, evaluator failure, low confidence, protected target match를 redacted summary로 보여 줄 수 있어야 한다.
6. Replay input은 frozen `PermissionedAction`, permission mode snapshot, containment snapshot, static rule version, evaluator version 또는 recorded output, approval decision ref를 포함해야 한다.
7. Replay는 실제 tool을 실행하면 안 된다.
8. 같은 snapshot과 같은 rule version이면 같은 final decision이 나와야 한다.
9. 더 엄격한 rule replay는 old allow를 deny로 바꿀 수 있다.
10. 더 느슨한 rule replay는 old deny를 자동 allow로 바꾸면 안 된다.
11. Contract matrix는 Spec 022의 mode, capability, evaluator, approval, inheritance, audit cases를 모두 포함해야 한다.
12. Release evidence는 docs, unit, integration, diagnostics, redaction, replay buckets로 나뉘어야 한다.
13. Recent classifier denial summary는 audit보다 좁은 inspect surface이며, raw command, raw arguments, raw prompt, raw classifier response, env/config/secret/host path를 저장하면 안 된다.

## 데이터/상태 모델

1. `PermissionAuditRecord`: action id, session id, turn id, tool name, capabilities, target summary, argument digest, mode, decision, reason, refs, containment, created at.
2. `PermissionDiagnosticsSummary`: decision counts, auto approval reasons, asks, denies, evaluator failures, containment warnings.
3. `PermissionReplayInput`: frozen action, snapshots, static rule version, evaluator output, approval refs.
4. `PermissionReplayOutcome`: same decision, stricter deny, mismatch, invalid replay, redaction failure.
5. `PermissionContractCase`: case id, setup, action, expected decision, evidence bucket.
6. `PermissionReleaseEvidence`: docs refs, test refs, diagnostics refs, replay refs, redaction refs.

## 정상 시퀀스

1. Runtime policy가 final decision을 만든다.
2. Audit record를 redacted form으로 저장한다.
3. Diagnostics summary가 action decision reason을 참조한다.
4. Replay input이 frozen snapshot을 보존한다.
5. Release gate가 contract matrix test evidence를 확인한다.
6. 사용자는 auto-approved action도 final summary 또는 diagnostics에서 확인할 수 있다.

## 실패 시퀀스

1. Audit record에 raw token이 포함되려 한다.
2. Redaction failure로 audit persistence를 중단하고 denied 또는 blocked evidence를 남긴다.
3. Replay가 actual tool execution을 시도한다.
4. Replay runner가 invalid replay로 중단한다.
5. Looser rule replay가 old deny를 allow로 바꾸려 한다.
6. Replay invariant violation으로 release gate를 막는다.

## 검증 관점

1. Allow, ask, deny 모두 audit record가 남는지 확인한다.
2. Audit record가 raw token과 full env를 포함하지 않는지 확인한다.
3. Diagnostics가 auto-approved, asked, denied reason을 설명하는지 확인한다.
4. Evaluator failure와 containment unknown이 diagnostics에 보이는지 확인한다.
5. Replay가 실제 tool을 실행하지 않는지 확인한다.
6. Same snapshot replay가 같은 decision을 내는지 확인한다.
7. Stricter rule replay가 old allow를 deny로 바꿀 수 있는지 확인한다.
8. Looser rule replay가 old deny를 automatic allow로 바꾸지 않는지 확인한다.
9. Spec 022 contract matrix의 모든 필수 row가 테스트로 연결되는지 확인한다.
10. Recent classifier denial summary가 redaction 기준을 위반하지 않는지 확인한다.

## 완료 기준

1. Permission audit record가 모든 permissioned action에 대해 생성된다.
2. Redaction 실패가 raw persistence로 이어지지 않는다.
3. Diagnostics가 allow, ask, deny, evaluator failure, containment unknown, protected target denial을 설명한다.
4. Replay invariant가 반복 가능한 테스트로 고정된다.
5. Contract matrix가 release evidence에 포함된다.
6. 이 PRD를 마지막으로 Spec 022 close 판단에 필요한 evidence buckets가 정의된다.
