# PRD 002. remote output and artifact persistence

Status: Planned

## Goal

Remote URL provider output을 safe local artifact, safe non-persisted reference, rejection 중 하나로 결정하고 metadata/provenance/retention evidence를 저장한다.

## Scope

1. Download/reference/reject policy and user disclosure.
2. Existing network/SSRF guard, redirects, scheme, byte/MIME, credential-forwarding evidence consumption.
3. Artifact metadata, digest, provenance, retention, disclosure status.
4. Snapshot-based replay without live URL or credential resolution.

## Non Scope

1. CDN, public URL, hosted gallery를 제공하지 않는다.
2. 034가 network isolation, credential lifecycle, sandbox를 재소유하지 않는다.
3. Remote reference의 영구 접근성이나 재다운로드 가능성을 보장하지 않는다.

## Parent Requirement Mapping

- Owned scope 4-5, 8
- Invariants 6, 11-12
- Primary Must Have: 5-7, 10
- Primary Acceptance Criteria: 4-6, 9

## Acceptance Criteria

1. Private/link-local/loopback, redirect, scheme, byte/MIME, credential forwarding cases use exact owner evidence and fail closed.
2. Persisted/reference/rejected outcomes remain distinct and user-visible.
3. Metadata records relative path, digest, provider/model, source ids, retention and disclosure status without raw credential/URL payload.
4. Replay reads 031 snapshot and recorded artifact evidence only.

## Closure Evidence

1. Remote-output policy and SSRF handoff matrix.
2. Persistence/provenance/retention tests.
3. Specs 030/031 owner-fact audits.
4. Replay no-live-URL transcript.
