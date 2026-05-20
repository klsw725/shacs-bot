# PRD 005. self improvement app and mcp integration

## 목표

이 문서는 autonomous improvement flow와 app task, MCP/tool exposure 통합을 완전 구현하기 위한 기준이다. runtime은 자기 설정, skill, prompt, tool exposure, app manifest, automation rule 변경을 제안할 수 있지만, 승인 전에는 아무 효과도 만들 수 없다.

## SPEC 입력

- 주관 spec: `docs/specs/018-evaluation-automation-and-self-improvement/SPEC.md`
- 선행 PRD:
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/000-evaluator-foundation-and-ledger.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/002-capability-approval-and-checkpoint-gates.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/004-memory-search-skills-and-curator.md`
- 교차 의존:
  - `docs/specs/004-tool-runtime/SPEC.md`
  - `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`
  - `docs/specs/017-app-operating-environment/SPEC.md`

## Dependency Cut

- 000은 evaluator envelope와 ledger foundation을 제공한다.
- 002는 approval, checkpoint, denied outcome gate를 제공한다.
- 004는 authored skill draft와 curator evidence를 제공한다.
- Spec 004 tool runtime은 tool dispatch와 normalized outcome을 소유한다.
- 010은 MCP default deny와 permission guard를 소유한다.
- 017은 app manifest, app registry, app task boundary를 소유한다.
- 018은 proposal에서 rollback record까지 이어지는 self improvement 통합 흐름을 소유한다.

## 범위

- improvement proposal, approval, checkpoint, apply, verify, record, rollback lifecycle
- app task와 self improvement proposal 연결
- MCP/tool exposure projection의 default deny 기준
- silent self modification 금지
- improvement ledger와 task/evaluation ledger 연결
- failed verify와 rollback 후 사용자 보고

## 범위 제외

- app marketplace
- organization policy rollout
- fleet wide MCP exposure
- remote evaluator SaaS
- approval 없는 자동 코드 수정
- Hermes Python runtime 또는 코드 복사

## 구현 요구사항

- improvement proposal은 target kind, proposed diff summary, risk summary, evidence refs, expected benefit, rollback plan을 포함해야 한다.
- proposal은 `proposed`, `approval_pending`, `approved`, `checkpointed`, `applied`, `verified`, `recorded`, `rolled_back`, `rejected`, `failed` 상태를 가져야 한다.
- approval 전 proposal은 runtime behavior를 바꾸면 안 된다.
- approval 후에도 002의 checkpoint gate가 통과해야 apply가 가능하다.
- apply는 owner spec의 primitive를 호출해야 하며, 018이 tool, app, skill 저장소를 직접 우회하면 안 된다.
- verify는 적용된 변경의 expected behavior를 확인하고, 실패하면 rollback eligibility를 평가해야 한다.
- rollback은 checkpoint ref와 owner rollback primitive가 있을 때만 실행한다.
- app task가 improvement proposal을 만들 수 있지만 app 권한만으로 승인이나 apply를 완료할 수 없다.
- MCP/tool exposure는 기본적으로 deny projection이어야 하며, proposal과 approval 없이는 노출 범위를 넓히지 않는다.
- 모든 self improvement action은 evaluation ledger, task ledger, improvement record에서 같은 correlation id로 inspect 가능해야 한다.

## 데이터/상태 모델

- `ImprovementProposal`: proposal id, target kind, diff summary ref, evidence refs, risk summary, rollback plan, status.
- `ImprovementApproval`: approval request ref, decision ref, approved scope, expiration, actor local user.
- `ImprovementCheckpoint`: checkpoint ref, target digest before, inspect ref, rollback capability.
- `ImprovementApplyRecord`: apply id, proposal id, owner spec, action ref, outcome ref.
- `ImprovementVerification`: verification id, expected behavior, observed result ref, pass or fail, next action.
- `McpExposureProjection`: tool or resource id, requested exposure, current exposure, default deny reason, approval ref.

## 정상 시퀀스

1. evaluator 또는 app task가 improvement proposal을 만든다.
2. proposal이 redacted diff summary와 rollback plan을 사용자에게 보여준다.
3. 사용자가 승인하면 approval correlation을 확인한다.
4. checkpoint gate가 target snapshot을 만든다.
5. owner primitive를 통해 변경을 적용한다.
6. verification이 expected behavior를 확인한다.
7. record가 ledger에 남고 projection이 새 상태를 보여준다.

## 실패 시퀀스

1. approval 전 apply 시도가 감지되면 denied outcome으로 닫는다.
2. checkpoint 생성 또는 inspect 실패 시 apply하지 않는다.
3. owner primitive가 실패하면 proposal을 failed로 표시하고 partial state를 기록한다.
4. verification이 실패하면 rollback 가능 여부를 판단하고 사용자에게 보고한다.
5. MCP exposure가 default deny를 벗어나려 하지만 approval이 없으면 노출하지 않는다.

## 검증 관점

- proposal이 approval 전 runtime behavior를 바꾸지 않는지 확인한다.
- approval, checkpoint, apply, verify, record, rollback 순서가 건너뛰어지지 않는지 확인한다.
- app task가 proposal은 만들 수 있지만 승인과 apply를 독점하지 못하는지 확인한다.
- MCP/tool exposure projection이 default deny로 시작하는지 확인한다.
- verification 실패가 rollback 또는 explicit failed record로 이어지는지 확인한다.

## 완료 기준

- self improvement flow 전체가 proposal에서 rollback까지 구현 기준과 테스트를 가진다.
- no silent self modification 원칙이 approval 없는 모든 변경 경로에서 확인된다.
- app task와 MCP/tool exposure가 018 ledger와 010 safety boundary를 함께 따른다.
- 실패한 improvement가 숨겨지지 않고 inspect, diagnostics, user projection에 남는다.
