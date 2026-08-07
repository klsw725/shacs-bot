# PRD 003. auth source resolution and credential lifecycle

Status: Planned

## Goal

Prime 방식의 credential source precedence, local persistence, refresh, fingerprint, status projection을 현재 trust model의 auth 계약으로 정의한다.

## Scope

1. Runtime override, environment, local auth store, provider config의 precedence.
2. API key, OAuth access/refresh token, expiry와 provider-specific credential shape.
3. File permission, atomic write 또는 lock, concurrent refresh serialization.
4. Source fingerprint와 stale detection.
5. Status-only inspect와 raw value 비표시.
6. Environment, literal, command-backed config value resolution.

Config/profile source declaration schema와 migration은 Spec 031이 소유한다. 이 PRD는 local auth file permission, raw credential read/write, source resolution, refresh, fingerprint, status projection을 소유한다.

## Invariants

1. Provider transport는 resolved raw credential을 사용할 수 있다.
2. Local auth store는 raw credential을 저장할 수 있으므로 사용자-local sensitive file로 취급한다.
3. Status projection과 diagnostics는 raw value를 표시하지 않는다.
4. Fingerprint는 stale detection용이며 SecretRef, redaction proof, payload integrity proof가 아니다.
5. Command-backed resolution은 실행 command와 failure disclosure를 제공하고 trusted code로 취급한다.

## Acceptance Criteria

1. Source precedence가 provider family별 fixture에서 결정적으로 검증된다.
2. Auth file permission과 concurrent refresh lock이 검증된다.
3. OAuth refresh success, expiry, failure, stale source 전환이 검증된다.
4. Status inspect에 raw API key, bearer token, refresh token이 나타나지 않는다.
5. Session·log·trace는 별도 중앙 secret-safe surface가 아니라는 disclosure를 유지한다.

## Non-goals

- Typed SecretRef
- 중앙 vault
- Raw credential 비지속성
- Complete secret redaction 또는 exfiltration prevention
