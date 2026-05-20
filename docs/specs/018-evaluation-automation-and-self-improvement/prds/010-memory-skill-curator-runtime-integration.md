# PRD 010. memory skill curator runtime integration

## 목표

이 문서는 004에서 정의한 bounded memory evidence, session search, skill progressive disclosure, authored skill, curator 흐름을 실제 owner primitive와 runtime 호출 경로에 연결하는 기준이다. 목표는 evaluator와 automation이 memory와 skill을 증거로 쓰되, skill 활성화나 memory 삭제를 승인 없이 수행하지 않게 하는 것이다.

## SPEC 입력

- 주관 spec: `docs/specs/018-evaluation-automation-and-self-improvement/SPEC.md`
- 선행 PRD:
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/000-evaluator-foundation-and-ledger.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/004-memory-search-skills-and-curator.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/008-runtime-evaluator-enforcement-and-ledger-consumption.md`
  - `docs/specs/018-evaluation-automation-and-self-improvement/prds/009-scheduled-automation-runtime-execution.md`
- 교차 의존:
  - `docs/specs/005-skill-system/SPEC.md`
  - `docs/specs/006-session-store/SPEC.md`
  - `docs/specs/009-context-assembly-and-compaction-input/SPEC.md`
  - `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`
  - `docs/specs/017-app-operating-environment/SPEC.md`

## Dependency Cut

- 004은 memory evidence와 skill lifecycle 의미를 제공한다.
- 005는 skill discovery, parsing, registry, injection primitive를 소유한다.
- 006은 session search와 replay 가능한 session truth를 소유한다.
- 009는 context assembly와 budget cut primitive를 소유한다.
- 014는 diagnostics와 inspect surface를 소유한다.
- 017은 app provided skill reference와 app task boundary를 소유한다.
- 018은 evaluator와 automation이 이 owner primitive를 어떤 순서와 증거 계약으로 소비하는지만 정의한다.

## 범위

- bounded memory evidence request와 owner search primitive 연결
- frozen session search snapshot 생성과 digest 기록
- skill list, view, reference progressive disclosure runtime 호출
- authored skill draft, dry run, approval pending, active candidate 연결
- curator proposal과 no auto delete enforcement
- evaluator input과 diagnostics evidence lineage 연결

## 범위 제외

- vector database 제품 선택
- session store 검색 인덱스 물리 구현
- skill parser 또는 injection engine 재작성
- app manifest schema 변경
- 자동 skill 활성화, 자동 memory 삭제
- 원격 지식베이스, 조직 skill catalog, marketplace publishing

## 구현 요구사항

- evaluator나 automation은 memory를 직접 조회하지 않고 `MemoryEvidenceRequest`를 통해 owner search primitive를 호출해야 한다.
- `MemoryEvidenceRequest`는 session id, query, target evaluator kind, budget, cutoff, redaction profile, caller reason을 포함해야 한다.
- memory result는 bounded evidence set으로 반환되어야 하며 result count, omitted reason, digest, source refs를 기록해야 한다.
- session search snapshot은 evaluator 호출 전에 freeze되어야 하며 같은 evaluator run에서 재검색하면 안 된다.
- context budget 초과로 제외된 evidence는 `omitted_by_budget`, `omitted_by_redaction`, `omitted_by_cutoff`, `omitted_by_relevance` 중 하나의 reason을 가져야 한다.
- skill discovery는 005 registry를 통해 list 단계부터 시작해야 하며, evaluator input에 full skill body를 자동 주입하면 안 된다.
- skill list 단계는 name, source, status, digest, short description만 제공해야 한다.
- skill view 단계는 사용자가 선택하거나 approved runtime이 요청한 skill body의 redacted view만 제공해야 한다.
- skill reference 단계는 evaluator evidence refs에 skill digest와 registry ref만 기록해야 한다.
- authored skill은 draft로 생성되며 005 skill validation과 dry run을 통과해야 approval pending이 될 수 있다.
- authored skill은 approval 전 active registry에 들어가면 안 된다.
- curator는 duplicate, conflict, stale, low value memory, candidate skill을 제안할 수 있지만 owner primitive를 직접 변경하면 안 된다.
- curator proposal은 improvement proposal 또는 approval checkpoint로 연결되어야 하며 silent cleanup을 수행하면 안 된다.
- app provided skill ref는 017 app manifest와 app task boundary를 evidence로 포함해야 한다.
- diagnostics evidence는 memory query, search snapshot digest, skill list digest, viewed skill digest, curator proposal refs를 추적할 수 있어야 한다.

## 데이터/상태 모델

- `MemoryEvidenceRequest`: request id, session id, query, evaluator kind, budget, cutoff, redaction profile, caller reason.
- `MemoryEvidenceSet`: evidence set id, request id, result refs, digest, omitted reasons, created at, frozen at.
- `SkillDisclosureRecord`: disclosure id, skill ref, stage, requester, digest, redaction status, approval ref.
- `AuthoredSkillRuntimeState`: draft, dry_run_pending, dry_run_failed, approval_pending, active_candidate, active, stale, archived.
- `CuratorProposal`: proposal id, target kind, target refs, reason, evidence refs, suggested action, approval ref, final status.

## 정상 시퀀스

1. evaluator coordinator가 goal completion 평가 전에 `MemoryEvidenceRequest`를 만든다.
2. owner session search primitive가 bounded result를 반환한다.
3. coordinator가 result를 frozen evidence set으로 고정하고 digest를 evaluator envelope에 넣는다.
4. skill registry에서 list 단계 정보를 가져와 관련 skill digest만 evidence로 연결한다.
5. 필요한 경우 approved runtime request가 skill view를 요청한다.
6. evaluator는 memory evidence set과 skill refs를 근거로 verdict를 만든다.
7. diagnostics와 projection은 raw content 없이 evidence lineage와 omitted reason을 확인할 수 있다.

## 실패 시퀀스

1. curator가 stale skill 삭제를 제안한다.
2. runtime이 curator proposal을 owner primitive 변경으로 바로 실행하지 않는다.
3. proposal은 approval pending 또는 improvement proposal로 연결된다.
4. approval이 없으면 skill registry는 바뀌지 않는다.
5. ledger와 projection은 curator suggestion이 실행되지 않은 상태와 reason을 보여준다.

## 검증 관점

- evaluator 호출 중 session search 결과가 바뀌어도 같은 run의 evidence digest가 변하지 않는지 확인한다.
- skill list 단계에서 full skill body가 evaluator input으로 들어가지 않는지 확인한다.
- approval 전 authored skill이 active injection 대상에 포함되지 않는지 확인한다.
- curator proposal이 memory 삭제나 skill archive를 직접 실행하지 않는지 확인한다.
- budget과 redaction으로 제외된 evidence가 omitted reason과 함께 diagnostics에서 보이는지 확인한다.
- app provided skill ref가 app manifest와 capability boundary 없이 evidence로 승격되지 않는지 확인한다.

## 완료 기준

- memory, session search, skill, curator 흐름이 각 owner primitive를 통해 호출된다.
- evaluator input에는 bounded, frozen, redacted evidence set과 skill refs만 들어간다.
- authored skill과 curator proposal은 approval 전 runtime behavior를 바꾸지 않는다.
- diagnostics가 evidence lineage와 omitted reason을 재구성할 수 있다.
- 018은 owner primitive를 우회하는 저장소 직접 변경 경로를 만들지 않는다.
