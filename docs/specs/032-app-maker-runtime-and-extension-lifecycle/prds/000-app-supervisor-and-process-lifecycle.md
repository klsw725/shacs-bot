# PRD 000. AppSupervisor and process lifecycle

Status: Planned

## Goal

Installed app의 start, stop, restart, recover를 `AppSupervisor` typed state와 evidence-producing process boundary로 구현한다.

## Scope

1. App process id/state, lifecycle event, recovery input, shutdown reason.
2. Registry lookup부터 process creation과 final receipt까지의 start flow.
3. Graceful stop, timeout, cancellation, stale marker, interrupted recovery.
4. Spec 030 process-control/sandbox/credential facts와 Spec 031 execution snapshot ref의 소비.

## Non Scope

1. App Maker proposal/apply/install은 PRD 001이 소유한다.
2. Executable activation eligibility나 persistence를 재정의하지 않는다.
3. Process lifecycle isolation을 security sandbox나 side-effect rollback으로 표현하지 않는다.

## Parent Requirement Mapping

- Owned scope 1-3, 7-8
- Invariants 1, 3-7, 9-11
- Primary Must Have: 1-5, 9-10
- Primary Acceptance Criteria: 1-5, 9-10, 15-16

## Required Contract

1. Start는 enabled state, manifest digest, credential status, trusted runtime ref, activation snapshot, execution snapshot ref를 확인한다.
2. Missing credential, untrusted workspace, disabled/stale activation은 process creation 전에 blocked receipt를 남긴다.
3. Stop/recover는 requested와 completed를 구분하고 timeout·cleanup 범위를 기록한다.
4. Receipt에는 raw credential이나 full environment를 저장하지 않는다.
5. Replay는 recorded lifecycle evidence만 읽고 process를 시작하지 않는다.

## Acceptance Criteria

1. Typed transition test가 installed, starting, running, stopping, stopped, failed, recovery-needed 상태를 고정한다.
2. CLI/local API의 start/stop/restart/recover가 같은 owner boundary와 receipt를 사용한다.
3. Failure injection이 stale marker, partial start, timeout, missing receipt를 재현한다.
4. Process-control/sandbox facts는 adapter별로 남고 app `active` 상태로 뭉개지지 않는다.

## Closure Evidence

1. State-machine test와 command locator.
2. Success/blocked/timeout/recover process receipts.
3. CLI/local API real-surface transcript와 cleanup receipt.
4. Specs 030/031 fact consumption read audit.
