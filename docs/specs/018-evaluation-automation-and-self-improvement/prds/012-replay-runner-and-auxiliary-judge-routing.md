# PRD 012. replay runner and auxiliary judge routing

## 목표

이 문서는 006에서 정의한 trajectory replay, safe mock, recorded tool outcome, auxiliary judge routing, local regression dataset을 실제 실행 가능한 replay runner 기준으로 구체화한다. 목적은 self hosted 사용자가 로컬에서 평가 회귀를 재현하고 비교할 수 있게 하는 것이며, destructive effect나 public training upload를 만들지 않는 것이다.

## SPEC 입력

- 주관 spec: `docs/specs/018-evaluation-automation-and-self-improvement/SPEC.md`
- 선행 PRD:
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/000-evaluator-foundation-and-ledger.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/006-trajectory-replay-and-provider-routing.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/008-runtime-evaluator-enforcement-and-ledger-consumption.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/009-scheduled-automation-runtime-execution.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/011-self-improvement-apply-verify-and-rollback-wiring.md`
- 교차 의존:
  - `docs/specs/003-provider-runtime/SPEC.md`
  - `docs/specs/004-tool-runtime/SPEC.md`
  - `docs/specs/008-configuration-profiles-and-runtime-layout/SPEC.md`
  - `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`
  - `docs/specs/016-verification-matrix-and-release-gates/SPEC.md`

## Dependency Cut

- 006은 replay record와 provider routing 의미를 제공한다.
- 003 provider runtime은 provider invocation, model routing, fallback primitive를 소유한다.
- 004 tool runtime은 tool schema와 normalized outcome을 소유한다. replay는 destructive tool을 호출하지 않는다.
- 008은 profile과 provider config source를 소유한다.
- 014는 diagnostics bundle과 inspect surface를 소유한다.
- 016은 release regression gate runner와 coverage matrix를 소유한다.
- 018은 replay runner가 소비할 dataset, judge role, comparison result 의미를 정의한다.

## 범위

- local replay dataset loading과 selection
- recorded tool outcome과 safe mock outcome 적용
- destructive effect 차단
- auxiliary judge routing과 fallback decision 기록
- evaluator regression 비교와 diff 생성
- replay run 결과를 diagnostics와 release coverage entry로 연결

## 범위 제외

- provider adapter 구현
- tool runtime dispatch 재구현
- destructive tool dry run engine 구현
- cloud training pipeline
- public benchmark upload
- vendor 비용 최적화 정책
- 관리자용 평가 dashboard

## 구현 요구사항

- replay runner는 redacted trajectory와 local regression dataset만 입력으로 받아야 한다.
- replay dataset item은 dataset id, case id, trajectory refs, expected verdict, expected task outcome, expected projection status, allowed provider roles, redaction profile을 포함해야 한다.
- replay runner는 destructive tool request를 실제 tool runtime으로 전달하면 안 된다.
- tool result는 recorded outcome이 있으면 recorded outcome을 사용하고, 없으면 safe mock outcome이 명시된 case만 실행해야 한다.
- recorded outcome과 safe mock outcome이 모두 없으면 case는 `blocked_missing_replay_outcome`으로 종료해야 한다.
- safe mock outcome은 mock reason, source, expected schema digest, limitations를 포함해야 한다.
- auxiliary judge routing은 `goal_completion_judge`, `capability_judge`, `task_outcome_judge`, `replay_comparison_judge` role을 구분해야 한다.
- auxiliary judge provider selection은 provider snapshot, model id, profile ref, fallback chain, routing reason을 기록해야 한다.
- judge fallback은 primary unavailable, policy denied, budget exceeded, timeout, invalid output을 구분해야 한다.
- fallback이 발생해도 replay result는 사용된 judge route를 명시해야 하며 primary judge 결과처럼 기록하면 안 된다.
- replay comparison은 expected verdict와 actual verdict의 kind, reason class, evidence refs, confidence band, projection status를 비교해야 한다.
- confidence 숫자만 다른 경우와 verdict kind가 다른 경우는 다른 severity로 기록해야 한다.
- replay runner는 session truth나 user config를 mutate하면 안 되며 별도 replay ledger 또는 diagnostics artifact로만 결과를 남겨야 한다.
- local regression dataset은 사용자가 선택한 case만 실행해야 하며 외부 업로드를 기본 동작으로 가져서는 안 된다.

## 데이터/상태 모델

- `ReplayDatasetItem`: dataset id, case id, trajectory refs, expected verdict, expected outcome, expected projection, allowed judge roles, redaction profile.
- `ReplayToolOutcomePolicy`: tool call ref, outcome source, recorded outcome ref, safe mock ref, blocked reason.
- `AuxiliaryJudgeRoute`: route id, judge role, provider id, model id, profile ref, fallback chain, routing reason, final status.
- `ReplayRunRecord`: run id, dataset id, selected cases, started at, completed at, status, diagnostics ref.
- `ReplayCaseResult`: case id, actual verdict, comparison status, diff summary, severity, judge route refs, blocked reason.

## 정상 시퀀스

1. 사용자가 local regression dataset에서 case를 선택한다.
2. replay runner가 redacted trajectory와 expected verdict를 읽는다.
3. runner가 recorded tool outcome 또는 safe mock outcome으로 tool step을 재생한다.
4. auxiliary judge route가 provider snapshot과 fallback chain을 기록하며 evaluator verdict를 만든다.
5. runner가 expected와 actual verdict를 비교한다.
6. replay result가 diagnostics artifact와 release coverage entry 후보로 기록된다.

## 실패 시퀀스

1. replay case에 destructive tool call이 있지만 recorded outcome이 없다.
2. safe mock outcome도 명시되어 있지 않다.
3. runner는 tool을 실행하지 않고 case를 `blocked_missing_replay_outcome`으로 종료한다.
4. replay result는 blocked reason, missing tool call ref, 필요한 fixture 정보를 기록한다.
5. release gate는 이 case를 pass로 계산하지 않는다.

## 검증 관점

- destructive tool call이 replay 중 실제 tool runtime으로 전달되지 않는지 확인한다.
- recorded outcome이 있는 case가 같은 결과로 재생되는지 확인한다.
- safe mock schema digest가 맞지 않으면 case가 blocked되는지 확인한다.
- auxiliary judge primary 실패 시 fallback reason과 사용된 provider가 기록되는지 확인한다.
- verdict kind mismatch와 confidence band mismatch가 다른 severity로 보고되는지 확인한다.
- replay result가 session truth나 runtime config를 변경하지 않는지 확인한다.

## 완료 기준

- replay runner가 local regression dataset을 선택 실행할 수 있다.
- destructive effect는 recorded outcome 또는 safe mock outcome으로만 대체된다.
- auxiliary judge route와 fallback evidence가 replay result에 남는다.
- expected와 actual verdict, outcome, projection status 비교 결과가 diagnostics와 release coverage에 연결된다.
- replay 실행은 외부 업로드와 runtime mutation 없이 로컬 artifact로 끝난다.
