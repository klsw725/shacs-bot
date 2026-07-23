# PRD 004: durable child task recovery

## 목표

Subagent child task의 active/terminal 상태와 correlation identity를 durable하게 보존하고 restart 뒤 inspect, cancel, stale discard를 안전하게 수행한다.

Status: Complete (Scoped). Durable child lifecycle, artifact-backed run/result refs, `subagent.child_run` work ordering, spawned/running restart distinction, cancellation/late/stale/duplicate decisions, parent reentry repair, bounded retention, opaque CLI/session diagnostics가 구현됐다. `shacs-session`/`shacs-core` durable child tests와 실제 `runtime inspect`/`runtime recover` QA, workspace gate, 5-lane review를 통과했다.

## 범위

1. Durable child task record
2. Spawned, running, completed, failed, timed out, cancelled, stale 상태
3. Parent/session/turn/effect/child correlation
4. Restart 뒤 active child inspection과 recovery decision
5. Finished result adoption과 stale/duplicate/late discard
6. Durable cancellation 연결

## 비범위

- remote worker fleet
- arbitrary child process resurrection
- parent session에 correlation 없는 result 주입
- 새로운 merge policy owner

## SPEC 입력

1. 필수 선행 PRD: `002-durable-work-queue-scheduler-retry-and-cancellation.md`
2. Existing child contract: `../../011-subagent-runtime/SPEC.md`
3. Execution outcome contract: `../../028-formal-execution-reentry-and-outcome-contracts/SPEC.md`

## Dependency Cut

1. 기존 four-field correlation은 authoritative invariant로 유지한다.
2. Restart recovery는 child success를 추정하지 않는다.
3. Durable child record는 merge/adoption fact를 보존하지만 parent session patch를 직접 수행하지 않는다.
4. Stale/mismatch result는 session history content로 persist되지 않는다.

## 구현 요구사항

1. Record는 child id, parent session/turn, spawn effect, correlation/idempotency, state, attempt, timestamps, result ref를 가진다.
2. Spawn acceptance와 child record append 순서를 crash-safe하게 정의한다.
3. Restart 시 running child를 `completed`로 바꾸지 않고 recovery-needed 상태로 분류한다.
4. Durable cancellation request와 terminal cancellation outcome을 구분한다.
5. Result adoption은 current parent state와 correlation을 다시 검증한다.
6. Duplicate/late/stale result는 inspectable decision으로 남긴다.
7. Child record retention은 bounded하며 artifact/result raw content를 inline하지 않는다.

## 정상 시퀀스

1. Parent가 spawn을 승인하고 durable child record를 만든다.
2. Durable work가 child execution을 시작한다.
3. Child result가 correlation 검증을 거친다.
4. Accepted fact가 event store에 기록된다.
5. Parent orchestrator가 reentry에서 session 반영 여부를 결정한다.

## 실패 시퀀스

1. Spawn record 후 실행 전 crash는 pending child로 복원한다.
2. Running 중 crash는 recovery-needed로 남긴다.
3. Parent가 종료되었거나 correlation이 맞지 않으면 stale로 남긴다.
4. Cancellation 뒤 late success는 parent truth를 뒤집지 않는다.

## 검증 관점

1. Spawn 전/후, run 전/중/후 crash matrix를 둔다.
2. Active, completed, failed, timed out, cancelled child restart fixture를 검증한다.
3. Wrong parent/turn/effect/child result와 duplicate/late result를 검증한다.
4. Stale result가 session content에 들어가지 않는지 확인한다.
5. CLI/session diagnostics가 raw child payload 없이 recovery 상태를 보여주는지 확인한다.

## 완료 기준

- Child lifecycle이 restart 뒤 inspect 가능하다.
- Correlation/stale invariant가 약화되지 않는다.
- Durable cancellation과 terminal result가 구분된다.
