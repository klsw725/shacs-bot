# PRD 040. 빈 provider 환경 변수와 다시 저장한 인증

Status: Complete (Scoped)

Depends on: [PRD 000](000-provider-status-and-gateway-hint.md)의 닫힌 결과(runtime과 `provider status`·gateway 안내가 같이 쓰는 자리 찾기 함수 `ProviderCredentialRuntime::choose_slot`, 선언 함수 `declaration_for`, `provider status`의 판정). 뒤 번호 PRD에 의존하지 않는다.

## Goal

Docker Compose가 넘기는 빈 provider 환경 변수(`ANTHROPIC_API_KEY=` 등)가 인증 저장소와 config(`.env.json`으로 푼 값 포함)에 둔 key를 가리지 않게 한다. 로그인하거나 key를 가져오면 그때 선언의 fingerprint를 함께 적어, 선언이 바뀌어 건너뛰던 provider도 다시 로그인·가져오기로 다시 쓰이게 한다. 다만 지금 선언이 인증 저장소를 허용하지 않으면(`localAuth: false`) 다시 저장해도 runtime이 읽지 않으므로, 그 허용을 되돌리거나 선언된 환경 변수·config 값을 써야 한다(SPEC 소유 범위 9, 사용자 결정 2026-11-09).

## 지금 동작

- `choose_slot`은 선언된 환경 변수가 process 환경(또는 runtime에 넣은 환경)에 있으면 값이 빈 글이어도 그 자리를 고른다. 그래서 인증 저장소의 key나 config `apiKey`가 있어도 빈 key로 요청한다(SPEC "확인한 것"). `provider status`는 이 경우를 `missing`과 "is set but empty"로 알린다.
- 인증 저장소의 `_sourceFingerprints`는 OAuth 갱신(`LocalAuthStore::resolve_oauth`)만 적는다. `provider codex login`, `provider codex import-token`, `provider copilot import-token`, `provider <id> import-key`는 항목만 바꾸고 fingerprint를 그대로 둔다. 그래서 갱신 뒤 선언(`credentialSource`, `apiKeyRef`)을 바꾼 provider는 다시 로그인해도 계속 건너뛰고, `provider status`는 선언을 되돌리라고만 알린다.

## Scope

1. **빈 환경 변수(SPEC 소유 범위 9 첫 항목):** `choose_slot`에서 선언된 환경 변수는 값이 빈 글(`""`)이 아닐 때만 그 자리다. 빈 글이면 없는 것으로 보고 인증 저장소, credential command, config 값 순서로 간다. 048의 `${이름}` 풀기와 같은 뜻이라 공백만인 값은 값으로 본다(지금처럼 그 자리이고 `provider status`는 "is set but empty"로 알린다). 비어 있지 않은 값은 지금처럼 먼저 쓰인다.
   - runtime에 넣은 환경(`with_environment`)에 빈 글이 있으면 그 이름은 없는 것이다(process 환경으로 내려가 찾지 않는다. 시험이 process 환경에 기대지 않게 하는 지금 규칙 그대로).
   - 이 함수 하나만 바꾼다. runtime 해석(`resolve_credential_observed`), `provider status`, gateway 안내가 같이 바뀐다. `CredentialSourceDeclaration::resolve`(고른 자리의 값만 받는 shacs-config 함수)와 `runtime inspect`의 `provider_auth`는 바꾸지 않는다.
2. **로그인·가져오기 때 fingerprint(소유 범위 9 셋째 항목):** 네 명령이 인증 저장소에 새 인증을 넣을 때, 같은 저장(같은 `save_auth_store_to_path` 한 번)에서 그 provider의 `_sourceFingerprints`를 지금 선언의 fingerprint로 적는다. 있던 다른 fingerprint도 이 값으로 바뀐다. 다른 provider의 항목과 fingerprint는 그대로다.
   - 지금 선언은 runtime과 같은 함수로 정한다. `shacs-core`에 `ProviderCredentialRuntime::declaration_fingerprint(provider_id, config, spec)`(안에서 `declaration_for(...).fingerprint()`)를 공개로 더하고, CLI는 gateway처럼 만든 runtime(`ProviderCredentialRuntime::new(auth_path, workspace, …)`)으로 부른다.
   - 선언에 쓰는 provider 항목은 명령이 저장할 config의 그 항목이다(없어서 명령이 더하는 기본 항목 포함). runtime은 config를 `${이름}`을 풀어 읽으므로, 그 항목의 선언 칸(`apiKeyRef`, `credentialSource`)만 담은 임시 config를 runtime과 같은 함수(`resolve_config_with_env_file`, process 환경과 `.env.json`)로 풀어 쓴다. 다른 칸(`apiKey`, 다른 provider, 다른 설정)의 `${이름}`이 없어도 영향이 없다. 선언 칸 자체의 `${이름}`을 풀지 못할 때만 적힌 그대로의 선언 칸을 쓴다(이때 runtime도 config를 읽지 못해 시작하지 못한다). 저장하는 config는 지금처럼 풀지 않은 것이다.
   - spec은 `ProviderRegistry`에서 찾는다(`openai_codex`, `github_copilot`, `import-key`는 이미 찾은 spec).
   - 저장 방식(잠금 없이 `load_auth_store` → 바꿈 → `save_auth_store_to_path`)과 명령의 출력은 그대로다.
3. **`provider status` 안내(소유 범위 9 넷째 항목):** fingerprint가 달라 건너뛴 인증(`slot`이 없고 `fingerprint_differs`)의 `next:`:
   - 지금 선언이 인증 저장소를 허용하면(`local_auth_allowed`): `restore providers.<id>.credentialSource and apiKeyRef to what they were when it was saved, or save it again: <다시 저장하는 명령>`. 다시 저장하는 명령은 지금 `add_credential`(Codex는 로그인, Copilot은 `import-token`, API key provider는 `import-key`)이고 container 안이면 `docker compose run --rm shacs-cli …` 꼴이다.
   - 허용하지 않으면(`localAuth: false`) 다시 저장해도 runtime이 읽지 않으므로 다시 저장을 알리지 않는다: `restore providers.<id>.credentialSource and apiKeyRef to what they were when it was saved, or <인증 저장소를 허용하지 않을 때의 지금 안내>`(선언된 환경 변수가 있으면 `set <이름> or providers.<id>.apiKey`, 없으면 `set providers.<id>.apiKey or a credential command`).
   - migration marker가 있으면 지금처럼 `--recover`만 알린다. `reason:`은 그대로다.
   - `PlannedProvider::fingerprint_differs`의 문서("Logging in or importing again does not clear it")와 `choose_slot`의 문서("present even if empty")를 새 동작으로 고친다.
4. **문서(소유 범위 9 마지막 항목):** 빈 환경 변수 경고를 고친 동작으로 바꾼다. 비어 있지 않은 provider 환경 변수는 여전히 저장한 key보다 먼저 쓰인다는 것, 공백만인 값도 값이라는 것, 로그인·가져온 인증(가져온 API key 포함)이 이제 그때 선언의 fingerprint를 가져 그 뒤 선언을 바꾸면 건너뛴다는 것과, 선언이 인증 저장소를 허용하면 다시 로그인·가져오기로 다시 쓰인다는 것(허용하지 않으면 그 허용을 되돌리거나 환경 변수·config 값을 쓴다는 것)을 적는다.
   - README Docker 문단(빈 값 경고와 "지금은 빈 환경 변수 때문에 뒤의 두 길이 가려집니다").
   - USAGE "provider 인증 확인"의 자리 순서 항목("환경 변수는 비어 있어도 그 자리를 차지합니다")과 fingerprint 항목("다시 로그인해도 이 표시는 지워지지 않으므로"), Docker provider secret 문단(빈 값 경고와 그 해결책).
   - `.env.example` 머리 주석(빈 provider 변수가 key를 가린다는 문단).

## Non Scope

- 공백만인 provider 환경 변수를 없는 것으로 보기(048과 같은 뜻을 지킨다), 비어 있지 않은 환경 변수의 우선순위.
- 인증 명령의 잠금, 다른 저장 경로(OAuth 갱신은 지금처럼 적는다), PRD 040 전에 저장해 fingerprint가 없는 인증(030 규칙대로 지금 선언의 것으로 본다).
- `runtime inspect` readiness의 `provider_auth`, Compose 파일.

## Parent Requirement Mapping

- [052 SPEC](../SPEC.md)의 소유 범위 9와 그 문서, "앞선 스펙에서 바꾸는 것"의 030 두 줄(빈 환경 변수, 로그인·가져오기 때 fingerprint), 수용 조건 11과 이 PRD 변경에 대한 수용 조건 9, 10을 소유한다.

## Required Contract

1. 빈 글인 선언 환경 변수는 runtime, `provider status`, gateway 안내 모두에서 없는 것이다. 공백만인 값과 비어 있지 않은 값은 지금과 같다.
2. 네 명령은 그 provider의 fingerprint만, runtime과 같은 선언 함수의 값으로 적는다. 다른 provider의 항목과 fingerprint는 byte 단위로 같다.
3. 출력과 오류에 비밀 값이 없다.
4. `Cargo.lock`에 새 crate가 없다.

## Acceptance Criteria

시험은 `Given`/`When`/`Then` 주석을 쓴다.

1. **같은 경우 표(`shacs-core` `provider_credential_plan_tests.rs`, PRD 000 시험):** plan과 runtime 해석을 같은 표로 비교한다.
   - 고치는 기존 행: "saved key behind an empty environment variable"는 이제 `LocalApiKey`이고 runtime이 저장한 key를 쓴다(빈 환경 변수의 뜻이 바뀌었기 때문).
   - 더하는 행: config `apiKey`(`.env.json`으로 푼 값과 같은 꼴)와 빈 환경 변수 → `ProviderLiteral`과 그 값. 공백만인 환경 변수와 저장한 key → `Environment { empty: true }`, runtime은 그 공백 값. 비어 있지 않은 환경 변수와 저장한 key → `Environment`, runtime은 환경 변수 값.
   - 나머지 행의 결과는 그대로다.
2. **실제 요청(`shacs-core` `tests/it/spec052_empty_provider_environment.rs`, 새 module을 `tests/it/main.rs`에 등록):** 네트워크 없는 로컬 가짜 Anthropic 서버(시험 안의 `TcpListener`, Anthropic 꼴 답)를 `providers.anthropic.apiBase`로 두고, 인증 저장소에 key를, runtime 환경에 빈 `ANTHROPIC_API_KEY`를 넣어 `resolve_provider_client`로 만든 client의 요청을 받으면 `x-api-key: <저장한 key>`가 실려 있다(SPEC "확인한 것"의 재현).
3. **fingerprint 적기(`shacs-cli` `saved_credential_tests.rs`, 새 단위 시험 파일을 `lib.rs`의 `#[cfg(test)] #[path = "saved_credential_tests.rs"] mod saved_credential_tests;`로 등록해 private 함수를 부른다. 임시 data 폴더, config 경로를 준다):**
   - `save_codex_login_token`(시험 token), `import_codex_token`, `import_copilot_token`, `import_provider_api_key`(`anthropic`)가 각각 그 provider의 fingerprint를 지금 선언의 것으로 적는다. 기대값은 시험이 따로 만든 `CredentialSourceDeclaration`(기본 config는 `compatibility(family, spec의 환경 변수)`)의 fingerprint다.
   - config에 `providers.anthropic.credentialSource`(다른 환경 변수 이름)가 있으면 그 선언의 fingerprint를 적는다. 그 칸이 `${이름}`이고 `.env.json`에 그 이름이 있으면 푼 선언의 fingerprint를 적는다. 다른 칸(그 provider의 `apiKey`, 다른 provider의 칸)의 `${이름}`이 어디에도 없어도 푼 선언의 fingerprint다. 선언 칸 자체의 `${이름}`이 어디에도 없으면 적힌 그대로의 선언으로 적는다.
   - 다른 provider의 항목과 fingerprint가 있던 인증 저장소에서 네 명령 뒤에도 그것들이 같다. 그 provider에 다른 fingerprint가 있었으면 새 값으로 바뀐다.
   - 적은 뒤 gateway처럼 만든 runtime(`local_provider_credential_runtime`)이 그 provider를 `LocalAuthStore`에서 새 값으로 해석한다.
4. **binary(`shacs-cli` `tests/it/spec052_provider_status.rs`, PRD 000 시험):**
   - 고치는 기존 시험: `status_reports_what_is_missing_and_what_to_do`의 빈 `ANTHROPIC_API_KEY`와 저장한 key 경우는 이제 `provider: ready anthropic`과 `credential: API key saved in the auth store`이다(빈 환경 변수의 뜻이 바뀌었기 때문). 그 경우를 따로 떼어 새 시험으로 둔다.
   - 빈 `ANTHROPIC_API_KEY`를 준 채 `import-key --token-stdin`으로 저장하고 띄운 gateway의 시작 출력에 `provider:` 줄이 없다.
   - fingerprint가 다른 Anthropic key만 있으면 `provider status`가 `missing`이고 `next:`에 `credentialSource`를 되돌리기와 `import-key`가 함께 있다. `import-key`로 다시 가져오면 `ready`다. 그 뒤 config의 `providers.anthropic.credentialSource`를 바꾸면 다시 `missing`(fingerprint 다름)이다.
   - 출력에 비밀 값이 없다.
5. **안내 단위 시험(`shacs-cli` `provider_status_cli_tests.rs`):** fingerprint가 달라 건너뛴 Codex·Copilot·API key provider마다 `next:`가 되돌리기와 다시 저장하는 명령을 함께 담고, container 안이면 Compose 꼴이다. 선언이 인증 저장소를 허용하지 않으면(`localAuth: false`, container 밖·안) 되돌리기와 환경 변수·config 안내만 있고 다시 저장하는 명령이 없다.
6. 문서가 범위 4와 같다.
7. 검증은 순서대로: Google 옵션을 붙인 `cargo fmt --all -- --check`, workspace `cargo clippy --all-targets -- -D warnings`, `cargo test --manifest-path crates/Cargo.toml -p shacs-core -q`, `-p shacs-cli -q`(바꾼 package). `shacs-config`는 바꾸지 않는다. `Cargo.lock`이 바뀌지 않는다. 전체 workspace 시험은 사용자 승인 없이 돌리지 않는다.

## Closure Evidence

1. 바뀐 파일과 새 시험, 고친 기존 시험 두 개와 까닭.
2. 값이 없는 출력 예(빈 환경 변수와 저장한 key의 `provider status`, fingerprint가 다른 경우의 안내).
3. Cargo 검증 기록과 `Cargo.lock` 확인.

## 남은 제한

- 비어 있지 않은 provider 환경 변수(공백만인 값 포함)는 지금처럼 저장한 key보다 먼저 쓰인다(030 순서).
- 빈 환경 변수로 뒤 자리의 인증을 막던 실행(CI 등)은 이제 저장한 인증·command·config 값을 쓴다(SPEC 남은 위험). 모두 막으려면 그 provider의 저장한 인증, credential command, config `apiKey`를 지운다.
- PRD 040 전에 저장해 fingerprint가 없는 인증은 다시 로그인하거나 가져올 때까지 fingerprint가 없다. 그 사이 선언을 바꿔도 건너뛰지 않는다(030 규칙).
- 가져온 API key도 fingerprint를 가지므로, 가져온 뒤 `apiKeyRef`·`credentialSource`를 바꾸면 그 key는 건너뛰어지고 `provider status`가 되돌리기나 다시 가져오기를 알린다. 바뀐 선언이 인증 저장소를 허용하지 않으면(`localAuth: false`) 다시 가져와도 쓰이지 않으므로, 그때는 되돌리기나 환경 변수·config 값만 알린다.
- 인증 명령은 지금처럼 인증 저장소 잠금 없이 쓴다. 같은 때 OAuth 갱신이 같은 파일을 쓰면 한쪽 쓰기가 덮일 수 있다(지금과 같다).

## 구현 메모 및 검증 근거 (2026-11-09)

실행 기록은 로컬 `.omo/evidence/spec052/`(추적하지 않음)의 `plan-review.md`와 `prd040/`에 있다.

- 구현:
  - `shacs-core` `provider_credential_plan.rs`: `choose_slot`이 선언된 환경 변수를 값이 빈 글이 아닐 때만 고른다(`with_environment`로 넣은 빈 글도 없는 것이고 process 환경으로 내려가지 않는다). 공개 `ProviderCredentialRuntime::declaration_fingerprint`(안에서 `declaration_for(...).fingerprint()`). `fingerprint_differs`·`choose_slot` 문서를 새 동작으로.
  - `shacs-cli` `lib.rs`: `runtime_declaration_fingerprint`(그 provider의 선언 칸만 담은 임시 config를 `resolve_config_with_env_file`로 풀고, 풀지 못하면 적힌 그대로. gateway처럼 만든 runtime의 `declaration_fingerprint`), `registered_provider_spec`. `import_codex_token`, `save_codex_login_token`, `import_copilot_token`, `import_provider_api_key`가 한 번의 `save_auth_store_to_path` 전에 `set_fingerprint`.
  - `shacs-cli` `provider_status_cli.rs`: fingerprint가 다른 인증의 `next:`에 되돌리기와, 선언이 인증 저장소를 허용하면 "save it again: <명령>", 허용하지 않으면 환경 변수·config 안내(`without_auth_store`, 허용하지 않는 경우의 기존 안내와 같은 글).
  - 문서: README Docker 문단(빈 값은 key를 가리지 않음, 값이 있는 변수가 먼저, 가져온 인증의 선언 기록과 `localAuth: false`), USAGE "provider 인증 확인"의 자리 순서·선언 기록 항목과 Docker provider secret 문단, `.env.example` 머리 주석.
- 시험:
  - `shacs-core` 같은 경우 표: 고친 기존 행 1개("saved key behind an empty environment variable"는 이제 `LocalApiKey`와 저장한 key. 빈 환경 변수의 뜻이 바뀌었기 때문), 더한 행 3개(빈 변수 뒤의 config `apiKey`, 공백만인 변수, 값이 있는 변수와 저장한 key). 표는 21행이다.
  - `shacs-core` `tests/it/spec052_empty_provider_environment.rs`(새 module) 1개: 가짜 Anthropic 서버가 받은 요청의 `x-api-key`가 저장한 key다(빈 key면 이 header가 아예 없으므로 바뀌기 전 동작은 이 시험에 실패한다).
  - `shacs-cli` `saved_credential_tests.rs`(새 단위 시험 파일) 2개: 네 명령의 fingerprint(다른 provider의 항목·fingerprint 그대로, 옛 fingerprint 바뀜, gateway처럼 만든 runtime이 빈 `ANTHROPIC_API_KEY`를 넣어도 `LocalAuthStore`에서 해석), 선언 3가지(선언된 이름, 다른 칸의 `${이름}`이 없어도 `.env.json`으로 푼 선언, 풀지 못하는 선언 칸은 적힌 그대로). `${이름}`은 이 process에 없는 이름을 골라 실행 환경에 기대지 않는다.
  - `shacs-cli` `provider_status_cli_tests.rs`: 안내 표 7가지(Codex·Copilot·API key, container 밖·안, `localAuth: false` 두 가지).
  - `shacs-cli` `tests/it/spec052_provider_status.rs`: 고친 기존 시험 1개(`status_reports_what_is_missing_and_what_to_do`에서 빈 변수 경우를 뺌. 이제 `ready`라서 새 시험으로 옮김), 새 시험 2개(binary `import-key`와 빈 변수의 `ready`, 같은 상태 gateway의 `provider:` 줄 없음. fingerprint가 다른 key의 안내 → 다시 가져와 `ready` → `credentialSource`를 바꿔 다시 `missing`). gateway 도움 함수가 환경을 받게 했다.
- 값이 없는 출력 예(debug binary, 임시 HOME, 빈 `ANTHROPIC_API_KEY`, `import-key`로 넣은 시험 key):

  ```text
  provider: ready anthropic
  credential: API key saved in the auth store
  ```

  그 뒤 config에 `providers.anthropic.credentialSource`(`environment: OTHER_ANTHROPIC_KEY`)를 넣으면:

  ```text
  provider: missing anthropic
  reason: the sign-in or key for anthropic was saved under a different credential setting (providers.anthropic.credentialSource or apiKeyRef)
  next: restore providers.anthropic.credentialSource and apiKeyRef to what they were when it was saved, or save it again: shacs-bot provider anthropic import-key --token-stdin, or set OTHER_ANTHROPIC_KEY
  ```

- 검증: Google 옵션을 붙인 `cargo fmt --all -- --check`, workspace `cargo clippy --all-targets -- -D warnings` 통과. 마지막 소스로 `shacs-core` 1,986 passed / 0 failed / 5 ignored(`prd040/core-tests.log`), `shacs-cli` 558 passed / 0 failed / 3 ignored(`prd040/cli-tests.log`, PRD 020·030 시험 포함). `ProviderCredentialRuntime`을 쓰는 다른 crate의 시험(`shacs-tui --test spec030_credential_resolution_surfaces`)도 통과. `crates/Cargo.lock`은 바뀌지 않았다. `shacs-config`는 바꾸지 않았다.
- Oracle 독립 검토: 계획 3차 **PASS**(1차: 설정 전체를 풀면 상관없는 `${이름}`이 선언을 바꿈, `localAuth: false`면 다시 저장해도 소용없음, 새 시험 등록. 2차: 같은 단서를 목표·문서 범위에도). 구현 2차 **PASS**(1차: 실행 환경에 기대는 `${이름}` 시험, README·`.env.example`의 `localAuth: false` 단서, BDD 주석).

## 종료 확인 (2026-11-09)

- 전체 workspace 시험: 사용자 승인(2026-11-09)을 받아 PRD 020·030·040을 담은 작업 트리에서 `cargo test --manifest-path crates/Cargo.toml --workspace --locked --no-fail-fast`를 돌렸다. suite 211개, 3,752 passed / 0 failed / 8 ignored, exit 0이다(로컬 `.omo/evidence/spec052/closure/workspace-tests-prd020-040.log`). 커밋 `738061c`.
- PRD 020의 실제 Docker 확인(서버 ka1rm31)에서 공식 Compose가 빈 provider 환경 변수를 넘기는 container의 `provider status`가 지금처럼 `missing none`(인증 없음)이었다. 빈 변수와 저장한 key가 함께 있는 실제 container 실행은 하지 않았고, 그 경우는 binary 시험과 가짜 Anthropic 서버로 보낸 요청으로 확인했다.
- 수용 조건 1–7을 위 시험과 검증으로 맞췄고, 계획 3차·구현 2차 독립 검토가 PASS다. PRD 040은 `Complete (Scoped)`로 닫는다. 전체 출시 seal이 아니다.
- 남은 제한은 위 "남은 제한"과 같다.
