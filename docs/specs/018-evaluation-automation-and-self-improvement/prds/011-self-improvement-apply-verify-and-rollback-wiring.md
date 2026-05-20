# PRD 011. self improvement apply verify and rollback wiring

## 목표

이 문서는 005에서 정의한 self improvement proposal lifecycle을 실제 approval, checkpoint, apply, verify, record, rollback wiring으로 연결하는 기준이다. runtime은 설정, skill, prompt, tool exposure, app manifest, automation rule 변경을 제안할 수 있지만, 승인과 owner primitive 없이는 적용하거나 권한을 넓히지 못해야 한다.

## SPEC 입력

- 주관 spec: `docs/specs/018-evaluation-automation-and-self-improvement/SPEC.md`
- 선행 PRD:
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/000-evaluator-foundation-and-ledger.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/002-capability-approval-and-checkpoint-gates.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/004-memory-search-skills-and-curator.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/005-self-improvement-app-and-mcp-integration.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/008-runtime-evaluator-enforcement-and-ledger-consumption.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/010-memory-skill-curator-runtime-integration.md`
- 교차 의존:
  - `docs/specs/004-tool-runtime/SPEC.md`
  - `docs/specs/005-skill-system/SPEC.md`
  - `docs/specs/008-configuration-profiles-and-runtime-layout/SPEC.md`
  - `docs/specs/010-host-safety-permissions-and-secrets/SPEC.md`
  - `docs/specs/017-app-operating-environment/SPEC.md`

## Dependency Cut

- 005는 self improvement lifecycle 의미를 제공한다.
- 002는 approval, checkpoint, denied outcome gate를 제공한다.
- 010은 authored skill과 curator proposal runtime evidence를 제공한다.
- Spec 004는 tool dispatch와 normalized tool outcome을 소유한다.
- Spec 005는 skill registry mutation primitive를 소유한다.
- Spec 008은 profile과 runtime layout mutation primitive를 소유한다.
- Spec 010은 host safety, approval, permission guard, MCP default deny primitive를 소유한다.
- Spec 017은 app manifest와 app task boundary를 소유한다.
- 018은 proposal을 owner primitive에 연결하는 orchestration과 evidence만 소유한다.

## 범위

- improvement proposal approval checkpoint 생성과 사용자 승인 대기
- apply 전 checkpoint 확보와 owner primitive routing
- verify 실행과 실패 판정
- record와 rollback evidence 연결
- app task와 proposal 생성 경계
- MCP/tool exposure default deny와 승인 기반 widening

## 범위 제외

- approval UI visual design
- permission guard primitive 구현
- tool runtime dispatch 구현
- skill registry 저장소 재설계
- app manifest schema 재정의
- 승인 없는 자동 코드 수정
- 조직 rollout, 관리자 승인, fleet policy 배포

## 구현 요구사항

- improvement proposal은 승인 전 `proposed` 또는 `approval_pending` 상태에 머물러야 하며 runtime behavior를 바꾸면 안 된다.
- proposal은 target kind, target owner, proposed diff summary, risk summary, expected benefit, evidence refs, checkpoint requirement, rollback plan을 포함해야 한다.
- target kind는 `config_profile`, `skill`, `prompt`, `tool_exposure`, `app_manifest_ref`, `automation_rule`을 구분해야 한다.
- app task는 proposal을 만들 수 있지만 app task 권한만으로 approval, checkpoint, apply, rollback을 완료할 수 없다.
- approval은 010 permission guard와 002 approval gate를 통과해야 하며 approval record ref를 proposal에 연결해야 한다.
- apply 전에는 owner primitive가 제공하는 checkpoint ref가 필요하다.
- checkpoint를 만들 수 없는 target은 `blocked_checkpoint_unavailable` 상태가 되어야 하며 apply를 시도하면 안 된다.
- apply는 target owner primitive를 통해서만 수행해야 하며 018이 config, skill, tool exposure, app registry 저장소를 직접 수정하면 안 된다.
- MCP/tool exposure는 default deny로 projection되어야 하며 승인된 proposal 없이는 exposure scope가 넓어지면 안 된다.
- approval은 proposal에 명시된 scope만 넓힐 수 있으며 wildcard widening은 별도 proposal이 필요하다.
- verify는 proposal의 expected behavior와 safety condition을 확인해야 하며 성공 전에는 `applied_unverified` 상태로 남아야 한다.
- verify 실패 시 runtime은 rollback eligibility를 평가하고 rollback 가능하면 owner rollback primitive를 호출해야 한다.
- rollback이 실행되면 rollback record, checkpoint ref, verify failure ref, final state를 improvement ledger에 기록해야 한다.
- rollback 불가 target은 blocked status와 manual recovery hint를 projection에 제공해야 한다.
- record 단계는 적용 결과, 검증 결과, user approval ref, owner primitive refs를 diagnostics evidence로 남겨야 한다.

## 데이터/상태 모델

- `ImprovementProposal`: proposal id, target kind, target owner, diff summary, risk summary, expected benefit, evidence refs, rollback plan, status.
- `ImprovementApprovalCheckpoint`: checkpoint id, proposal id, approval ref, owner checkpoint ref, permission guard result, created at.
- `ImprovementApplyRecord`: apply id, proposal id, owner primitive ref, input digest, outcome ref, applied at.
- `ImprovementVerifyRecord`: verify id, proposal id, expected behavior refs, result, failure reason, evidence refs.
- `ImprovementRollbackRecord`: rollback id, proposal id, checkpoint ref, owner rollback ref, result, manual recovery hint.

## 정상 시퀀스

1. curator 또는 app task가 skill 변경 proposal을 만든다.
2. runtime이 proposal을 approval pending으로 projection한다.
3. 사용자가 approval surface에서 proposal scope를 승인한다.
4. runtime이 owner skill primitive로 checkpoint를 만든다.
5. runtime이 owner skill primitive로 변경을 적용한다.
6. verify가 expected behavior를 확인한다.
7. improvement ledger가 approval, checkpoint, apply, verify, record refs를 연결하고 projection은 verified status를 보여준다.

## 실패 시퀀스

1. app task가 tool exposure widening proposal을 만든다.
2. approval이 없거나 scope가 맞지 않는다.
3. runtime은 MCP/tool exposure default deny를 유지한다.
4. proposal은 `blocked_approval_required` 또는 `rejected`로 기록된다.
5. apply와 verify는 실행되지 않고 projection은 필요한 사용자 결정을 보여준다.

## 검증 관점

- approval 전 proposal이 config, skill, prompt, tool exposure, app manifest, automation rule을 바꾸지 않는지 확인한다.
- checkpoint가 없는 proposal은 apply되지 않고 blocked 상태가 되는지 확인한다.
- app task가 proposal 생성 외의 approval이나 apply를 단독 완료하지 못하는지 확인한다.
- MCP/tool exposure가 default deny에서 승인된 scope만큼만 넓어지는지 확인한다.
- verify 실패 시 rollback primitive가 호출되고 rollback evidence가 기록되는지 확인한다.
- rollback 불가 상황이 manual recovery hint와 함께 projection과 diagnostics에 남는지 확인한다.

## 완료 기준

- self improvement proposal이 approval, checkpoint, apply, verify, record, rollback 상태를 모두 가진다.
- 모든 mutation은 target owner primitive를 통해서만 수행된다.
- app task와 MCP/tool exposure는 승인 없이 권한을 넓히지 못한다.
- verify 실패와 rollback 결과가 improvement ledger, projection, diagnostics에서 추적된다.
- self hosted 사용자가 로컬 승인 surface에서 변경 범위와 복구 계획을 확인할 수 있다.
