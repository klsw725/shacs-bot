# PRD 003. video analyzer capability and evidence

Status: Planned

## Goal

Video analyzer capability와 bounded metadata/subtitle/transcript/keyframe/scene evidence를 media-domain state로 만들고 safe projection input을 제공한다.

## Scope

1. Analyzer injection/source/trusted-code/status contract.
2. Missing, unsupported codec, failed, duration-cap, truncated outcomes.
3. Bounded video evidence and inbound/generated provenance distinction.
4. Sandbox scope, credential status, data disclosure, snapshot reference projection inputs.

## Non Scope

1. Built-in ffmpeg, full codec coverage, complete video understanding을 요구하지 않는다.
2. Bounded extraction을 privacy redaction이나 민감정보 제거로 표현하지 않는다.
3. Media-root admission을 analyzer process containment로 표현하지 않는다.

## Parent Requirement Mapping

- Owned scope 6-8
- Invariants 7-9, 11-13
- Primary Must Have: 8-9
- Primary Acceptance Criteria: 7-8, 11-12

## Acceptance Criteria

1. Analyzer configured/missing/unsupported/failed/truncated states are deterministic.
2. Evidence obeys duration/byte/item bounds and records truncation.
3. Projection input includes safe analyzer source/trust, sandbox scope/status, disclosure, artifact refs.
4. 027 inbound attachment and generated artifact provenance never collapse into one kind.

## Closure Evidence

1. Analyzer capability/status matrix.
2. Codec/duration/truncation fixtures.
3. 030/031 owner-fact and 035 projection audits.
4. Bounded-evidence non-guarantee review.
