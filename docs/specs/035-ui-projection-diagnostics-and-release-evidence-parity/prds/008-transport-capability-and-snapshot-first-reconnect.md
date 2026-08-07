# PRD 008. transport capability and snapshot-first reconnect

Status: Planned

## Goal

기존 schema-version fail-closed gate를 보존하면서 실제 transport capability gap만 협상하고, reconnect 시 owner snapshot을 먼저 확정한 뒤 동일 generation의 delta를 적용한다.

## Scope

1. CLI, daemon, worker, local API/WebSocket 사이의 최소 capability handshake matrix.
2. Unsupported mutation의 side effect 전 거부와 user-visible reason.
3. Opaque generation/sequence를 사용하는 snapshot-first reconnect ordering.
4. Connection-local backpressure, coalescing, drop accounting과 reconnect gap의 결합.

## Non Scope

1. 완료된 schema-version rejection을 재구현하지 않는다.
2. Prime kernel/session store를 canonical truth로 도입하지 않는다.
3. Exactly-once delivery, durable network acknowledgement, universal protocol negotiation을 보장하지 않는다.

## Required Contract

1. Handshake는 지원하는 schema와 mutation capability만 교환하며 permission, approval, sandbox proof를 만들지 않는다.
2. Unsupported mutation은 runtime effect가 시작되기 전에 `unsupported` 또는 `blocked`로 끝나야 한다.
3. Reconnect client는 owner snapshot과 generation을 확정하기 전 delta를 적용하지 않는다.
4. Snapshot 이후에는 같은 generation에서 monotonic하게 관찰된 delta만 적용한다. Gap, stale generation, duplicate sequence는 명시적으로 표시한다.
5. Snapshot은 connection bootstrap evidence이며 Spec 031 execution snapshot이나 session truth가 아니다.

## Acceptance Criteria

1. Supported/unsupported 조합의 handshake matrix가 deterministic test로 고정된다.
2. Unsupported mutation이 side effect 전에 거부되고 CLI/API/TUI에 같은 reason으로 표시된다.
3. Reconnect test가 snapshot-before-delta, stale generation rejection, duplicate/gap accounting을 검증한다.
4. Slow consumer와 dropped progress가 final outcome delivery와 독립적으로 남는다.

## Closure Evidence

1. Capability matrix와 protocol transcript.
2. Snapshot-first reconnect ordering test와 failure injection artifact.
3. CLI/API/WebSocket real-surface transcript와 cleanup receipt.
4. 기존 Specs 002, 015, 029, 035 owner truth를 재소유하지 않는 read audit.
