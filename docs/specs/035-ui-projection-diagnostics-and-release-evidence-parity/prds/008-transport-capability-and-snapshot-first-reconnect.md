# PRD 008. transport capability and snapshot-first reconnect

Status: Open

구현 상태: 구현됨, closure 차단. Capability 협상, owner 접근 전 mutation 거절, snapshot-first reconnect 및 generation/sequence 검사가 CLI/API/TUI와 API WebSocket/SSE 경계에 구현되어 있다. Task7-9 및 todo10의 capability/failure-injection/실제 표면/owner audit 기록은 존재하나 todo11 이후 낡은 역사적 결속이다. [현재 차단 기록](../CLOSURE.md)을 따른다.

CLI Tasks 변경은 `--transport-hello`, API `POST /v1/tasks/actions`는 `transport_hello`를 매 요청에 요구한다. `POST /v1/transport/hello`는 호환성 조회이지 인증·권한 부여가 아니며 API의 loopback mutation opt-in과 owner 검증을 대체하지 않는다. 정확한 JSON과 조회의 URL 인코딩은 [사용법](../../../USAGE.md#tasks-조회와-owner-변경-요청)을 따른다.

## Goal

기존 schema-version fail-closed gate를 보존하면서 실제 transport capability gap만 협상하고, reconnect 시 owner snapshot을 먼저 확정한 뒤 동일 generation의 delta를 적용한다.

## Scope

1. CLI/TUI의 로컬 owner 호출, 기존 runtime/worker 경계와 local API/WebSocket 사이의 최소 capability handshake matrix. 여기서 daemon/worker는 기존 로컬 실행 경계를 뜻하며 새 범용 daemon RPC, fleet control plane이나 자동 reexec 서비스가 아니다.
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

현재 snapshot-first 발행 경로와 순서 검사 모델은 구현되어 있지만 별도 production reconnect client를 제공한다는 뜻은 아니다. F2는 client/session identity 충돌, 이전 generation의 final 관측 덮어쓰기 및 stale 비동기 queue 역방향 사례의 보정을 제한된 재검토 범위에서 확인했다. 과거 F3에서 URL 인코딩된 query가 HTTP 400으로 실패한 뒤 query decoding을 수정했고, 최종 compiled QA에서 encoded client/session으로 snapshot generation 1→2를 확인했다. 이 scoped QA는 전체 의미 증거나 release closure PASS가 아니며 최신 근거는 [CLOSURE](../CLOSURE.md)를 따른다.
