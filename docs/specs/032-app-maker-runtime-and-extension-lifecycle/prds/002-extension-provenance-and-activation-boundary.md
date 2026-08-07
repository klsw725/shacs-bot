# PRD 002. extension provenance and activation boundary

Status: Planned

## Goal

App-owned skill, plugin, hook, command, MCP의 provenance를 연결하고 discovery, install, activation, execution을 분리해 app lifecycle blocker로 투영한다.

## Scope

1. App id, manifest/source/content/dependency digest와 extension declaration linkage.
2. 005 Markdown skill, 025 command-backed surface, 030 trusted executable resource 경계.
3. 030 activation ref/status와 035 persistence/snapshot ref의 app-level consumption.
4. Active, stale, disabled, revoked, removed, untrusted blocker와 historical receipt.

## Non Scope

1. 030 activation eligibility, dependency execution gate, sandbox semantics를 재정의하지 않는다.
2. 035 activation schema/storage/migration을 재소유하지 않는다.
3. Discovery 또는 install만으로 executable surface를 노출하지 않는다.

## Parent Requirement Mapping

- Owned scope 6, 9
- Invariants 6-8, 11-16
- Primary Must Have: 8, 11-15
- Primary Acceptance Criteria: 8, 11-14

## Required Contract

1. Discovery는 descriptor/source/digest를 만들지만 hook, tool, MCP, entrypoint를 시작하지 않는다.
2. App receipt는 activation decision을 만들지 않고 030 activation ref/status와 blocker를 기록한다.
3. Content/dependency/source mismatch는 stale이며 기존 activation을 재사용하지 않는다.
4. Dependency preparation 결과는 030 process/sandbox controls와 연결하고 032는 app-level evidence만 남긴다.
5. Disable/revoke/remove는 historical receipt를 삭제하지 않는다.

## Acceptance Criteria

1. Discovery-only와 installed-only fixture가 executable exposure를 만들지 않는다.
2. Activation status별 app start blocker와 projection input이 deterministic하다.
3. Same-name/different-digest resource가 기존 activation을 재사용하지 않는다.
4. 005/025/030/035 owner handoff read audit가 중복 ownership이 없음을 보인다.

## Closure Evidence

1. Extension provenance and blocker matrix.
2. Digest mismatch/stale/disable/revoke/remove tests.
3. App lifecycle receipt와 031 projection fixture.
4. Specs 005/025/030/035 owner-boundary audit.
