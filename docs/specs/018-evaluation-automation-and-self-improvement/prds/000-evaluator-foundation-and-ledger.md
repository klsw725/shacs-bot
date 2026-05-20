# PRD 000. evaluator foundation and ledger

## 목표

이 문서는 `docs/specs/018-evaluation-automation-and-self-improvement/SPEC.md`의 첫 실행 문서다. 목표는 goal, safety, task outcome evaluator가 공유할 envelope, frozen input snapshot, ledger 경계를 먼저 고정해 나머지 PRD가 같은 평가 언어를 쓰게 하는 것이다.

018은 통합 계약을 소유한다. session truth, permission 확정, tool 실행, provider adapter, session store 물리 형식은 각 owner spec이 계속 소유한다.

## SPEC 입력

- 주관 spec: `docs/specs/018-evaluation-automation-and-self-improvement/SPEC.md`
- 선행 기준:
  - `docs/SYSTEM-FOUNDATION.md`
  - `docs/specs/001-session-kernel/SPEC.md`
  - `docs/specs/002-command-event-effect/SPEC.md`
  - `docs/specs/006-session-store/SPEC.md`
  - `docs/specs/007-main-orchestrator-policy/SPEC.md`
  - `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`
  - `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`

## Dependency Cut

- 이 PRD는 018 전체의 foundation이며 모든 후속 018 PRD의 선행 작업이다.
- 006은 event log와 checkpoint 저장의 source of truth를 소유한다. 018 ledger는 evaluation과 automation 의미를 읽기 좋게 나눈 projection 계약이다.
- 007은 `MainOrchestrator`의 권한을 소유한다. evaluator verdict는 조언이며 권한 결정이 아니다.
- 010은 redaction primitive와 secret boundary를 소유한다. 018은 evaluator input, output, ledger, fixture에 적용할 redaction baseline을 요구한다.
- 014는 diagnostics bundle과 inspect surface를 소유한다. 018은 그 surface가 읽을 evaluation record shape를 제공한다.

## 범위

- evaluator 공통 envelope와 verdict authority boundary 정의
- frozen input snapshot 생성 시점, 참조 범위, digest 기준 정의
- goal evaluator, capability evaluator, task outcome evaluator가 공유할 evidence reference 모델 정의
- task ledger와 evaluation ledger의 분리 기준 정의
- evaluator input, output, diagnostics, fixture에 적용할 redaction baseline 정의
- shared test fixtures와 golden evaluator cases 정의

## 범위 제외

- provider adapter 구현
- LLM judge prompt 상세 문구 고정
- session store 파일 형식 재설계
- permission grant engine 구현
- SaaS evaluator, 원격 평가 서비스, 조직 운영 콘솔
- public training dataset 생산

## 구현 요구사항

- `EvaluatorRequestEnvelope`는 evaluator 종류, correlation id, session id reference, turn id reference, triggering source, frozen snapshot digest, redaction profile, caller intent를 포함해야 한다.
- `EvaluatorVerdictEnvelope`는 verdict kind, reason, confidence, evidence references, suggested next action, expiration, redaction status, evaluator version을 포함해야 한다.
- envelope는 tool 실행이나 session mutation을 직접 표현하지 않는다. 모든 실행 권한은 `MainOrchestrator`가 별도 policy input으로 소비한다.
- frozen input snapshot은 evaluator 호출 직전에 확정해야 하며, 이후 session event가 도착해도 동일 evaluator run의 입력은 바뀌면 안 된다.
- snapshot에는 raw secret, full private file content, unredacted tool payload를 넣으면 안 된다. 필요한 경우 owner spec의 redacted artifact reference만 넣는다.
- task ledger는 automation job, background result, delivery, timeout, retry, rollback request를 기록한다.
- evaluation ledger는 evaluator request, frozen input digest, verdict, evidence digest, authority boundary note를 기록한다.
- 두 ledger는 correlation id로 연결하되 서로의 source of truth가 되지 않는다.
- denied, stale, expired, redaction failed 같은 공통 outcome code를 후속 PRD가 재사용할 수 있게 정의해야 한다.
- shared fixtures는 정상 verdict, stale snapshot, redaction failure, low confidence, conflicting evidence, denied capability, task timeout을 포함해야 한다.

## 데이터/상태 모델

- `EvaluatorKind`: `goal_completion`, `capability_safety`, `task_outcome`, `improvement_review`, `replay_judge`.
- `FrozenEvaluationSnapshot`: snapshot id, created at, source event ids, context summary digest, evidence refs, redaction profile, provider snapshot ref.
- `EvaluationLedgerRecord`: record id, evaluator kind, request id, snapshot id, verdict envelope, authority boundary, created at.
- `TaskLedgerRecord`: task id, task source, job id, status, result ref, outcome request id, delivery ref, retry or rollback ref.
- `EvidenceRef`: owner spec, artifact kind, redacted digest, locator, retention hint.
- 상태 전이는 evaluator가 아니라 orchestrator 또는 owner runtime이 수행한다.

## 정상 시퀀스

1. runtime이 evaluator가 필요한 순간을 감지한다.
2. `MainOrchestrator` 또는 owner service가 redacted frozen snapshot을 만든다.
3. evaluator request envelope가 snapshot digest와 함께 생성된다.
4. evaluator가 verdict envelope를 반환한다.
5. evaluation ledger에 request, snapshot, verdict, authority boundary가 기록된다.
6. orchestrator가 verdict를 policy, permission, task 상태와 함께 소비한다.
7. 필요한 task 상태 변경은 task ledger에 별도 기록된다.

## 실패 시퀀스

1. snapshot 생성 중 redaction 실패가 감지되면 evaluator 호출을 막고 `redaction_failed` record를 남긴다.
2. evaluator가 만료된 snapshot으로 verdict를 반환하면 orchestrator는 verdict를 적용하지 않고 stale record를 남긴다.
3. evaluator가 권한 확정처럼 보이는 verdict를 반환하면 boundary violation으로 기록하고 안전한 denied outcome으로 접는다.
4. task ledger 기록이 실패하면 user visible task 상태를 성공으로 표시하지 않는다.
5. evaluation ledger 기록이 실패하면 후속 자동화 action을 실행하지 않는다.

## 검증 관점

- frozen snapshot digest가 같은 입력에서 안정적으로 생성되는지 확인한다.
- redacted snapshot에 secret과 raw private payload가 포함되지 않는지 확인한다.
- evaluator verdict가 session truth를 직접 바꾸지 못하는지 확인한다.
- task ledger와 evaluation ledger가 같은 correlation id로 연결되지만 별도 record로 남는지 확인한다.
- shared fixtures가 모든 evaluator family에서 재사용되는지 확인한다.

## 완료 기준

- 018 후속 PRD가 참조할 공통 evaluator envelope와 ledger 타입 의미가 문서와 구현 요구사항으로 고정된다.
- redaction baseline과 frozen snapshot contract가 fixture로 검증된다.
- task ledger와 evaluation ledger inspect에 필요한 최소 record가 정의된다.
- evaluator 권한 한계가 테스트 이름, fixture, diagnostics 문구에서 반복 가능하게 확인된다.
