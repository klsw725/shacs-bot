# PRD 004. memory search skills and curator

## 목표

이 문서는 evaluator와 automation이 사용할 bounded memory evidence, frozen session search snapshot, skill progressive disclosure, agent authored skill lifecycle, curator 흐름을 완전 구현하기 위한 기준이다. memory와 skill은 evaluator의 증거 입력일 뿐, 자동 삭제나 무단 활성화를 수행하지 않는다.

## SPEC 입력

- 주관 spec: `docs/specs/018-evaluation-automation-and-self-improvement/SPEC.md`
- 선행 PRD: `docs/specs/018-evaluation-automation-and-self-improvement/prds/000-evaluator-foundation-and-ledger.md`
- 교차 의존:
  - `docs/specs/005-skill-system/SPEC.md`
  - `docs/specs/006-session-store/SPEC.md`
  - `docs/specs/009-context-assembly-and-compaction-input/SPEC.md`
  - `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`
  - `docs/specs/017-app-operating-environment/SPEC.md`

## Dependency Cut

- 000의 frozen snapshot과 evidence ref를 사용한다.
- 005는 skill discovery, parsing, active registry, injection primitive를 소유한다.
- 006은 session event log와 replay 가능한 session truth를 소유한다.
- 009는 context assembly, compaction, budget cut primitive를 소유한다.
- 014는 diagnostics와 inspect를 소유한다.
- 017은 app이 제공하는 skill reference와 app task projection을 소유한다.
- 018은 evaluator가 읽을 memory evidence와 skill lifecycle 통합 의미를 소유한다.

## 범위

- bounded memory evidence selection
- frozen session search snapshot
- evaluator용 summarization record
- skill progressive disclosure list, view, reference 의미
- agent authored skill lifecycle: draft, dry run, approval, active, stale, archived
- curator review flow와 no auto delete rule
- diagnostics와 ledger에서 evidence lineage 확인

## 범위 제외

- vector database 제품 선택
- public memory sync
- marketplace skill publishing
- 원격 팀 지식베이스
- 자동 memory 삭제
- agent가 승인 없이 skill을 활성화하는 흐름
- Python reference runtime 채택

## 구현 요구사항

- evaluator input에 들어가는 memory는 bounded evidence set이어야 하며, query, cutoff, budget, redaction profile, result digest를 기록해야 한다.
- session search는 evaluator 호출 전에 frozen snapshot으로 고정해야 하며, 이후 검색 결과 변화가 같은 evaluator run에 영향을 주면 안 된다.
- summarization은 source refs, omitted reason, confidence, redaction status를 가져야 한다.
- skill progressive disclosure는 list, view, reference 단계로 나뉘어야 한다.
- list는 skill name, source, status, short description, digest만 제공한다.
- view는 사용자가 선택한 skill body를 redacted 형태로 제공한다.
- reference는 evaluator 또는 context builder가 특정 skill digest를 증거로 인용할 때 사용한다.
- agent authored skill은 draft 상태로만 시작하며, dry run과 approval을 통과해야 active 후보가 된다.
- stale skill은 사용 중지 후보일 뿐이며 curator가 자동 삭제하면 안 된다.
- archived skill은 active injection 대상에서 빠지지만 audit과 replay evidence로 남아야 한다.
- curator는 중복, 충돌, 오래된 skill, low value memory를 제안할 수 있지만 삭제나 활성화는 사용자 승인 없이 수행하지 않는다.

## 데이터/상태 모델

- `MemoryEvidenceSet`: evidence id, query, source scope, budget, result refs, summary ref, redaction profile.
- `FrozenSessionSearchSnapshot`: snapshot id, search input digest, matched event refs, created at, result digest.
- `EvaluatorSummaryRef`: summary id, source refs, omitted refs, summary digest, confidence.
- `SkillDisclosureRecord`: skill id, status, list digest, view digest, referenced by.
- `AuthoredSkillLifecycle`: draft, dry run, approval pending, active, stale, archived.
- `CuratorRecommendation`: recommendation id, target kind, action proposed, reason, evidence refs, requires approval.

## 정상 시퀀스

1. evaluator가 memory evidence를 요청한다.
2. context owner가 budget과 redaction profile에 맞춰 bounded evidence set을 만든다.
3. session search result가 frozen snapshot으로 고정된다.
4. summarizer가 source refs를 가진 요약을 만든다.
5. evaluator가 evidence refs만 사용해 verdict를 만든다.
6. agent가 skill 개선을 제안하면 draft로 저장된다.
7. dry run과 사용자 approval 이후 active registry 후보가 된다.

## 실패 시퀀스

1. memory result가 budget을 넘으면 omitted reason과 함께 잘라내고 raw payload를 넣지 않는다.
2. session search snapshot이 stale이면 evaluator verdict 적용을 막는다.
3. skill body redaction에 실패하면 view와 reference를 제공하지 않는다.
4. dry run이 실패한 authored skill은 active 후보가 되지 않는다.
5. curator가 삭제를 제안해도 approval 없이는 archived 또는 deleted 상태로 바뀌지 않는다.

## 검증 관점

- bounded evidence가 budget과 redaction profile을 지키는지 확인한다.
- frozen session search snapshot이 재실행마다 같은 digest를 제공하는지 확인한다.
- skill list와 view가 서로 다른 disclosure 수준을 갖는지 확인한다.
- authored skill이 approval 없이 active가 되지 않는지 확인한다.
- curator recommendation이 auto delete를 수행하지 않는지 확인한다.

## 완료 기준

- evaluator memory evidence가 bounded, redacted, replay 가능한 ref로 남는다.
- session search snapshot과 summary가 ledger에서 추적된다.
- skill progressive disclosure가 list, view, reference 단계로 구현된다.
- agent authored skill lifecycle과 curator no auto delete rule이 테스트로 확인된다.
