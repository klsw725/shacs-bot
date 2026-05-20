# PRD 006. trajectory replay and provider routing

## 목표

이 문서는 redacted trajectory, replay record, provider/model snapshot, auxiliary judge routing, local quality regression dataset을 완전 구현하기 위한 기준이다. 목적은 public training data 생산이 아니라 self hosted 사용자가 로컬에서 품질 회귀와 실패 재현을 할 수 있게 만드는 것이다.

## SPEC 입력

- 주관 spec: `docs/specs/018-evaluation-automation-and-self-improvement/SPEC.md`
- 선행 PRD:
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/000-evaluator-foundation-and-ledger.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/003-task-outcome-and-scheduled-automation.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/005-self-improvement-app-and-mcp-integration.md`
- 교차 의존:
  - `docs/specs/003-provider-runtime/SPEC.md`
  - `docs/specs/008-configuration-profiles-and-runtime-layout/SPEC.md`
  - `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`
  - `docs/specs/016-verification-matrix-and-release-gates/SPEC.md`

## Dependency Cut

- 000은 frozen snapshot, redaction baseline, ledger foundation을 제공한다.
- 003은 automation run과 task outcome source를 제공한다.
- 005는 improvement verification과 rollback record를 제공한다.
- Spec 003 provider runtime은 provider invocation과 late result policy를 소유한다.
- 008은 profile, runtime layout, provider config를 소유한다.
- 014는 diagnostics bundle과 inspect surface를 소유한다.
- 016은 release regression gate를 소유한다.
- 018은 evaluator와 automation replay에 필요한 trajectory 의미만 소유한다.

## 범위

- redacted trajectory record
- replay record와 destructive effect 차단
- provider/model snapshot
- auxiliary judge model routing과 fallback
- local quality regression dataset
- replay 결과 비교와 diagnostics 연결
- evaluator route와 main provider route의 분리

## 범위 제외

- provider adapter 구현
- cloud training pipeline
- public benchmark upload
- destructive tool replay
- 외부 조직용 평가 dashboard
- model vendor별 가격 최적화 정책

## 구현 요구사항

- trajectory는 model call, tool request, tool outcome, evaluator verdict, provider snapshot, timing, token 또는 tool stat을 redacted record로 연결해야 한다.
- provider/model snapshot은 provider id, model id, profile ref, routing reason, fallback chain, evaluator role을 포함해야 한다.
- auxiliary judge model은 goal completion, capability, task outcome, replay judge 중 어떤 역할인지 명시해야 한다.
- judge routing fallback은 기본 provider 실패, model unavailable, budget exceeded, policy denied를 구분해야 한다.
- replay는 destructive effects를 실행하지 않아야 한다. tool call은 recorded outcome 또는 safe mock outcome으로만 재생한다.
- local quality regression dataset은 사용자가 선택한 trajectory와 expected verdict, expected outcome, redacted evidence를 묶어야 한다.
- replay 결과는 pass, regression, inconclusive, invalid fixture 중 하나로 분류해야 한다.
- replay dataset은 diagnostics bundle에 포함 가능해야 하지만 raw secret이나 unredacted private data를 포함하면 안 된다.
- evaluator routing은 main assistant provider routing과 분리 가능해야 하며, fallback 결과를 ledger에 남겨야 한다.

## 데이터/상태 모델

- `TrajectoryRecord`: trajectory id, session ref, event refs, model calls, tool refs, evaluator refs, redaction profile, stats.
- `ProviderModelSnapshot`: snapshot id, provider id, model id, profile ref, role, routing reason, fallback chain.
- `ReplayRecord`: replay id, trajectory id, mode, safe effects policy, started at, result.
- `QualityRegressionCase`: case id, trajectory ref, expected verdict, expected outcome, evidence refs, owner note.
- `JudgeRoutingDecision`: evaluator kind, preferred model, selected model, fallback reason, denied reason.
- `ReplayResult`: pass, regression, inconclusive, invalid fixture, diff summary ref.

## 정상 시퀀스

1. runtime이 session turn과 automation run의 redacted trajectory를 기록한다.
2. provider/model snapshot이 main call과 evaluator call에 각각 붙는다.
3. 사용자가 trajectory를 local regression case로 승격한다.
4. replay runner가 destructive effect를 막고 recorded tool outcome을 사용한다.
5. auxiliary judge route가 선택되고 필요 시 fallback이 기록된다.
6. replay result가 expected verdict와 비교된다.
7. diagnostics와 release gate가 replay result를 읽는다.

## 실패 시퀀스

1. trajectory redaction이 실패하면 dataset 승격을 막는다.
2. selected judge model이 unavailable이면 fallback chain을 시도하고 결과를 기록한다.
3. fallback도 실패하면 replay result를 inconclusive로 닫는다.
4. replay가 destructive tool 실행을 요구하면 safe effects policy가 차단하고 invalid fixture로 표시한다.
5. provider snapshot이 없으면 regression case를 invalid fixture로 분류한다.

## 검증 관점

- trajectory record가 raw secret 없이 필요한 replay evidence를 보존하는지 확인한다.
- replay가 실제 tool destructive effect를 실행하지 않는지 확인한다.
- judge routing fallback reason이 ledger에 남는지 확인한다.
- local regression case가 expected verdict와 actual replay result를 비교하는지 확인한다.
- diagnostics bundle이 redacted replay evidence만 포함하는지 확인한다.

## 완료 기준

- redacted trajectory와 replay record가 evaluator, provider, task ledger와 연결된다.
- auxiliary judge routing과 fallback이 구현 요구사항과 fixture로 닫힌다.
- local quality regression dataset이 destructive effect 없이 실행된다.
- replay 결과가 016 release gate에서 소비 가능한 evidence로 남는다.
