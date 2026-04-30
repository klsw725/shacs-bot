# PRD 000. config discovery and runtime root

## 목표

이 문서는 `docs/specs/008-configuration-profiles-and-runtime-layout/SPEC.md`의 하위 실행 문서다. 목표는 config discovery, precedence, profile 구성, secrets 분리, runtime root와 하위 디렉터리 초기화를 구현 가능한 작업 단위로 고정하는 것이다.

- 전역과 workspace 설정을 예측 가능한 discovery 순서로 읽는다.
- config와 secrets를 분리하고 precedence를 키 단위로 적용한다.
- runtime root와 하위 디렉터리 레이아웃을 부트스트랩 시 일관되게 초기화한다.

## SPEC 입력

- 주관 spec: `docs/specs/008-configuration-profiles-and-runtime-layout/SPEC.md`
- 교차 의존:
  - `docs/specs/003-provider-runtime/SPEC.md`
  - `docs/specs/005-skill-system/SPEC.md`
  - `docs/specs/006-session-store/SPEC.md`
  - `docs/specs/007-main-orchestrator-policy/SPEC.md`

## Dependency Cut

이 PRD는 설정 탐색과 runtime layout을 구현 대상으로 삼는다. GUI 편집기, 원격 sync, cloud secret manager 연동은 범위 밖이다. 로컬 self-hosted 사용자가 파일만 보고 상태를 이해할 수 있는 수준이 완료 기준이다.

## 범위

- built-in, user-global, workspace-local, explicit override discovery
- config와 secrets의 별도 로딩 경로
- key 단위 precedence 적용
- provider, permission, runtime profile 로딩 골격
- runtime root와 하위 디렉터리 생성 규약
- schema version과 migration 진입점
- provider/auth family를 OpenAI-compatible, Anthropic auth, Codex auth(OpenAI auth style) 세 종류로 제한하는 로딩/검증 규칙

## 범위 제외

- 웹 기반 설정 편집기
- 원격 config 배포
- 멀티유저 정책 계층
- 모든 provider 벤더 옵션 완전 노출
- OpenAI-compatible, Anthropic auth, Codex auth(OpenAI auth style) 바깥의 추가 provider/auth family 지원

## 현재 구현 상태

### 이미 반영된 것

- built-in, user-global, workspace-local, explicit override config discovery와 precedence merge가 구현돼 있다.
- config와 secrets는 별도 snapshot으로 로드되며, provider secret은 `api_key_ref`를 통해 참조된다.
- provider/permission/runtime profile, runtime root layout bootstrap, unsupported schema/malformed layer diagnostics가 검증된다.
- CLI `provider codex login`은 OpenCode/Nanobot과 같은 Codex/ChatGPT browser OAuth를 기본값으로 제공한다. headless 환경은 device-code 계열 fallback을 쓰는 방향으로 남겨 두며, 사용자가 이미 확보한 bearer token 저장은 명시적 `provider codex import-token` fallback으로 둔다.
- Provider auth material은 `secrets.toml`에만 저장하고, 대응되는 `codex_auth` provider profile과 non-secret `api_key_ref`/session reference만 `config.toml`에 기록한다. 현재 Codex browser OAuth는 provider auth session JSON을 저장하고, 이 session bundle 구조는 향후 Anthropic 등 다른 provider auth family가 같은 저장 경계를 재사용할 수 있게 provider-neutral 형태를 유지한다.
- 만료된 Codex OAuth session은 provider adapter 생성 전에 refresh한다. refresh 가능한 session bundle은 새 access token과 rotated refresh token을 같은 `secrets.toml` entry에 atomic하게 저장한 뒤 갱신된 secrets snapshot으로 adapter를 만든다. raw imported bearer token은 refresh 대상이 아니며, refresh token이 없는 만료 session은 model network 호출 전에 재로그인을 요구한다.
- `provider codex login`의 browser OAuth와 `provider codex import-token` fallback이 구현돼 있다. `provider codex login --headless` device-code fallback은 아직 명시적 미구현 상태다.
- config source-origin evidence와 profile snapshot 출처 추적이 검증 증거로 연결돼 있다.
- Spec016 matrix에서 Unit, Integration, PackagingUpgrade가 FullSpec verified evidence로 승격돼 있다.

### 아직 남은 것

- deep merge 출처 설명과 runtime migration 진입점은 현재 minimum-slice 범위에 맞춰 제한적으로 유지된다.
- OpenAI-compatible, Anthropic auth, Codex auth(OpenAI auth style) 외 provider/auth family는 지원 범위 밖이다.
- 위 항목은 현재 로컬 config discovery/runtime layout FullSpec slice의 blocker가 아니라 후속 migration/provenance 확장 범위다.

### 로컬 근거

- `crates/shacs-core/src/core/config.rs`
- `crates/shacs-core/src/core/lifecycle.rs`
- `crates/shacs-core/tests/config_discovery.rs`
- `crates/shacs-cli/tests/provider_auth_cli.rs`
- `crates/shacs-core/tests/session_store_files.rs`

## TDD 계획

1. 없는 config 파일이 조용히 무시되는 테스트를 작성한다.
2. malformed 상위 계층이 다른 유효 계층까지 무효화하지 않는 테스트를 작성한다.
3. precedence가 `built-in < user-global < workspace-local < explicit override` 순으로 적용되는 테스트를 작성한다.
4. secrets가 config와 분리되어 로드되고 accidental config merge에 섞이지 않는 테스트를 작성한다.
5. runtime root 초기화 시 필수 하위 디렉터리가 생성되는 테스트를 작성한다.

## 구현 웨이브

### Wave 1. discovery 입력과 snapshot 타입 고정

- config snapshot, secrets snapshot, merged runtime config 타입을 정의한다.
- discovery 컨텍스트에 workspace 유무와 explicit override 입력을 포함한다.
- 한 번의 부트스트랩에서 일관된 snapshot을 사용하도록 경계를 만든다.

### Wave 2. discovery와 precedence 구현

- built-in defaults, user-global, workspace-local, explicit override 순으로 config를 읽는다.
- secrets는 별도 채널로 읽고 precedence를 독립 적용한다.
- malformed 계층은 진단하되 다른 유효 계층을 유지한다.

### Wave 3. profile 조립과 secrets 분리

- provider, permission, runtime profile의 최소 구조를 정의한다.
- 비민감 옵션과 민감 값을 분리 결합하되 출처를 추적 가능하게 유지한다.
- config에 민감 값이 직접 섞이지 않도록 검증 경로를 추가한다.

### Wave 4. runtime root 부트스트랩과 migration 진입점

- `artifacts/`, `sessions/`, `checkpoints/`, `logs/`, `cache/`, `tmp/` 디렉터리 규약을 초기화한다.
- session store와 tool runtime이 runtime root를 일관되게 참조하게 연결한다.
- schema version 체크와 향후 migration 진입점을 마련한다.

## Verification Evidence

- discovery precedence 테스트
- malformed 계층 격리 테스트
- secrets 분리 테스트
- structured config source-origin evidence 테스트
- runtime root 디렉터리 생성 테스트
- tool result artifact ref가 runtime-managed artifact 참조만 노출하는지 검증
- profile snapshot 출처 추적 검증
- explicit config/secrets override 파일 precedence 검증
- OpenAI-compatible, Anthropic auth, Codex auth(OpenAI auth style) provider family 로딩 검증
- Codex browser login/import가 token을 stdout/stderr/config에 노출하지 않고 `secrets.toml`에 저장하는지 검증
- Codex provider transport/provider errors가 bearer token을 redaction하는지 검증
- 만료된 Codex OAuth session이 adapter 생성 전에 refresh되고 rotated refresh token을 `secrets.toml`에 저장하는지 검증
- refresh token이 없는 만료 Codex OAuth session이 model network 호출 전에 재로그인을 요구하는지 검증

## Open Risks

- precedence를 깊은 병합으로 과하게 처리하면 값 출처 설명 가능성이 떨어질 수 있다.
- runtime root와 workspace root 경계가 흐리면 artifact와 세션 데이터 위치가 불명확해질 수 있다.
- secrets 분리 규칙이 약하면 사용자가 민감 값을 잘못 커밋할 위험이 남는다.

## 종료 기준

- config discovery와 precedence가 고정된 순서로 동작한다.
- secrets는 config와 별도 경로로 로드되고 병합 출처가 설명 가능하다.
- runtime root와 필수 하위 디렉터리가 일관되게 초기화된다.
- `docs/specs/008-configuration-profiles-and-runtime-layout/SPEC.md`의 경로 규약과 비목표를 침범하지 않는다.
