# PRD 060. Codex 모델·오류와 강제 종료 뒤 시작

Status: Implemented (전체 workspace 시험·종료 확인 전)

Depends on: [PRD 020](020-setup-script-and-container-home.md)(설정 스크립트의 로그인 단계와 Docker 설명 문서)과 [PRD 040](040-empty-provider-environment-and-saved-credentials.md)(로그인 저장 때 적는 fingerprint)의 닫힌 결과. 010 Codex 로그인, 030 PRD 003의 OAuth credential 꼴과 갱신, 003 Codex 응답 해석, 019·034 Codex 실패 응답의 보안 계약, 029·030 owner lease와 lock-aware 인계를 그대로 쓰고, SPEC "앞선 스펙에서 바꾸는 것"의 060 행만 바꾼다. 뒤 번호 PRD에 의존하지 않는다.

## Goal

ChatGPT 계정으로 로그인하면 그 계정이 실제로 쓸 수 있는 Codex 모델을 고르고, Codex가 모델을 거절하면 봇이 그 까닭과 할 일을 답하며, 로그인이 계정 ID를 실제 token 꼴에서 읽고, 강제로 꺼진 gateway 컨테이너가 다시 뜰 때 heartbeat가 지나기를 기다리지 않고 시작하게 한다(SPEC 소유 범위 11).

## 지금 동작(2026-11-09 확인)

- 서버(ka1rm31, Codex 로그인, plan `prolite`):
  - `provider codex login`이 적은 `gpt-5.4`로 보낸 Discord 메시지 두 개가 모두 `HTTP 400: provider error`로 답했다.
  - 같은 token으로 최소 요청을 보내니 `gpt-5.4`는 400(`{"error":{"type":"invalid_request_error","code":"model_not_found","message":"The 'gpt-5.4' model is not supported when using Codex with a ChatGPT account."},"detail":…}`)이었다. `gpt-6-sol`은 계정 ID header가 있든 없든 200이었다.
  - `GET https://chatgpt.com/backend-api/codex/models?client_version=1.0.0`(token, 계정 ID header)은 200 `{"models":[…]}`이었다. 모델마다 `slug`, `visibility`(`list`·`hide`), `priority` 등 54개 칸이 있었다.
  - 목록 순서는 `gpt-6.1-sol`(1), `gpt-6-astra`(2), `gpt-6.1-terra`(3), `gpt-6-sol`(4), `gpt-reserve`(5, hide), `gpt-6-luna`(6), `gpt-5.6-sol`·`gpt-5.6-terra`·`gpt-5.6-luna`(7–9), `codex-auto-review`(10, hide)이었다. `gpt-5.4`는 없었다.
- 서버의 access token claim에는 맨 위 `chatgpt_account_id`가 없다. 계정 ID는 `https://api.openai.com/auth` 객체 안의 `chatgpt_account_id`에 있다. 그래서 `auth.json`의 Codex 항목에 `accountId`가 없었다.
- 서버의 gateway 컨테이너(PID 1)는 종료 요청 3초 뒤 SIGKILL로 꺼졌다(exit 137). 다시 만든 컨테이너의 runtime(PID 1)은 남은 marker(pid 1, heartbeat 유효)를 보고 `runtime mutation blocked by stale runtime ownership owner-1-1794240048742 pid=1`로 8번 끝났고, 약 30초 뒤 heartbeat가 지나 시작했다.
- 코드(`crates/shacs-cli/src/lib.rs`):
  - `save_codex_login_token`은 언제나 `agents.defaults.provider = "openai_codex"`, `agents.defaults.model = CODEX_DEFAULT_MODEL("gpt-5.4")`로 적는다. `import_codex_token --select`도 같은 값을 적는다.
  - `token_response_from_json`은 `extract_codex_account_id`(id token, 그다음 access token)로 계정 ID를 읽는다. 이 함수는 맨 위 `chatgpt_account_id`, 점이 든 한 칸 이름 `https://api.openai.com/auth.chatgpt_account_id`, `organizations`의 첫 `id`만 본다. 시험도 그 꼴의 가짜 token을 쓴다.
  - `lock_aware_ownership_can_be_replaced`는 lock protocol이고 `!pid_alive || now > expires_at_ms`일 때만 참이다. marker의 PID가 시작하는 runtime 자신이면 `pid_is_alive`가 언제나 참이다(자기 PID 지름길). `process_started_after_marker`가 참이어도(분류는 "owner pid was reused after marker creation"인 stale) 인계하지 않는다. 시험 `fresh_lock_aware_heartbeat_blocks_pid_namespace_takeover`(PID 1, 살아 있음, 뒤에 시작함, heartbeat 유효 → 바꾸지 않음)가 이것을 고정한다.
- 코드(`crates/shacs-providers/src/clients/codex/`):
  - `transport.rs`의 `read_codex_error_body_bounded`는 실패 응답의 본문을 32 MiB까지 읽고 버린다. `CodexHttpStreamResponse.body`는 빈 글이다.
  - `response.rs`의 `codex_error_response`는 429면 사용량 문장, 아니면 `HTTP <상태>: provider error`다.
  - 시험 `codex_error_security.rs`가 공급자 글이 답에 들어가지 않는 것을 고정한다. 이미지·media 경로(`media.rs`)는 실패 응답의 본문을 보지 않고 고정 오류를 낸다.

## Scope

1. **로그인이 고르는 모델(`crates/shacs-cli/src/lib.rs`):**
   - 상수:
     - `CODEX_DEFAULT_MODEL`을 `"gpt-6.1-sol"`로 바꾼다(목록을 읽지 못할 때와 `import-token --select`에만 쓴다).
     - `CODEX_MODELS_CLIENT_VERSION = "1.0.0"`을 더한다(목록 요청의 `client_version`, 2026-11-09에 확인한 값).
     - `CODEX_API_BASE = "https://chatgpt.com/backend-api"`를 더하고, `codex_provider_config`도 이것을 쓴다.
   - `CodexAuthTransport`에 `get_json(&self, url, headers: &[(String, String)]) -> Result<CodexAuthHttpResponse, CliError>`를 더한다. `UreqCodexAuthTransport`는 그 header로 GET을 보내고 `codex_auth_response`로 읽는다(JSON이 아니면 지금처럼 오류). 시험 transport `MockCodexAuthTransport`는 받은 URL·header를 남기고 다음 응답을 낸다.
   - `codex_account_models(transport, api_base, access, account_id) -> Result<Vec<CodexAccountModel>, String>`:
     - `GET <api_base 끝의 / 뗌>/codex/models?client_version=<상수>`를 보낸다. header는 `Authorization: Bearer <access>`, `originator: shacs-bot`, `Accept: application/json`이고, 계정 ID가 비지 않았으면 `ChatGPT-Account-Id`도 넣는다.
     - 2xx가 아니면 `Err("HTTP <상태>")`이다. 전송 오류는 그 오류 글이다(token은 header에만 있어 글에 없다). `models`가 배열이 아니면 `Err("the response has no model list")`이다.
     - 항목마다 `slug`, `listed`(`visibility == "list"`), `priority`(정수, 없으면 `i64::MAX`)를 읽는다.
     - `slug`는 앞뒤 공백을 뺀 값이 1–100자이고 영문자·숫자·`.`·`-`·`_`·`:`·`/`로만 되어 있어야 한다. 또 보낸 access token과 (비지 않았으면) 계정 ID를 담지 않아야 한다. 아니면 그 항목을 건너뛴다. 응답이 받은 값을 되돌려도 config와 출력에 남지 않게 하기 위해서다.
   - `choose_codex_model(configured: Option<&str>, models: Result<Vec<CodexAccountModel>, String>) -> (String, CodexModelCheck)`:
     - `configured`는 config가 Codex일 때의 모델이다(비면 `None`). 이름은 `openai-codex/`·`openai_codex/`·`openai/`를 뗀 이름으로 비교한다(provider client의 `strip_codex_model_prefix`와 같은 앞). 그대로 둘 때는 적힌 그대로 둔다.
     - 목록을 읽었고 `configured`가 목록(보임·숨김 상관없이)에 있으면 `(configured, KeptInAccountList)`다.
     - 아니면 `listed`인 것 중 `priority`가 가장 작은 것(같으면 목록 순서)으로 `(slug, FromAccountList)`다.
     - 보이는 모델이 없으면 `Err("the account offers no listed model")`와 같이 다룬다.
     - 목록을 읽지 못했으면 `configured`가 있을 때 `(configured, KeptUnchecked(까닭))`, 없을 때 `(CODEX_DEFAULT_MODEL, DefaultUnchecked(까닭))`이다.
   - `save_codex_login_token(config_path, token, account_models)`. `account_models`는 `impl FnOnce(&str) -> Result<Vec<CodexAccountModel>, String>`이고 `api_base`를 받는다.
     - 지금처럼 config를 읽고 Codex provider 항목을 둔다.
     - 그다음 `configured`(바꾸기 전의 provider가 Codex일 때의 모델)와 `api_base`(config `providers.openai_codex.apiBase`, 앞뒤 공백을 빼고 `http`로 시작하지 않으면 `CODEX_API_BASE`)를 정한다.
     - `account_models(api_base)`와 `choose_codex_model`로 모델을 고른 뒤 provider를 Codex로, 모델을 고른 값으로 적는다.
     - 인증 저장·fingerprint·config 저장은 지금 그대로다.
     - `codex_login_with_transport`는 `|api_base| codex_account_models(transport, api_base, &access, account_id)`를 넘긴다(token의 access와 계정 ID 사본).
   - `CodexLoginOutcome`에 `model_check: CodexModelCheck`를 더한다. `format_codex_login_outcome`은 `Selected model:` 줄 다음에 `Model check: <글>` 한 줄을 넣는다. 글은 아래 넷 중 하나다.
     - `picked the highest-priority model your ChatGPT account lists`
     - `kept the configured model; your ChatGPT account offers it`
     - `could not read your ChatGPT account's model list (<까닭>); kept the configured model`
     - `could not read your ChatGPT account's model list (<까닭>); used the default model`
   - `import_codex_token`은 바꾸지 않는다(새 상수 값을 적을 뿐이다).
2. **Codex가 거절한 까닭(`crates/shacs-providers/src/clients/codex/`):**
   - `transport.rs`: `read_codex_error_body_bounded`가 읽은 byte를 돌려준다. 실패 응답이면 그 byte가 64 KiB(`CODEX_ERROR_CODE_BODY_MAX_BYTES`) 이하이고 `error.code`가 정해 둔 목록에 있을 때만 `CodexHttpStreamResponse.body`를 그 코드만 담은 JSON(`{"error":{"code":"model_not_found"}}`)으로 둔다. 아니면 지금처럼 빈 글이다. 그래서 공급자 글은 transport를 넘지 않고, 이 구조체의 `Debug`·직렬화에도 나오지 않는다. 32 MiB 넘는 본문의 transport 오류와 그 밖의 transport 동작은 그대로다.
   - `mod.rs`: `chat_stream_bounded`가 transport로 넘기기 전에 요청 본문의 `model`(앞을 뗀 이름)을 읽어 두고, 실패 응답이면 `parse_codex_stream_http_response(response, model, on_event)`로 넘긴다.
   - `response.rs`:
     - `codex_error_response(status, headers, body, model)`은 429면 지금 문장이다.
     - 아니면 `known_codex_error(body, model)`이 `Some`이면 `HTTP <상태>: <그 글>`, 아니면 지금처럼 `HTTP <상태>: provider error`다.
     - `known_codex_error`는 본문이 64 KiB 이하이고 JSON 객체이며 `error.code`가 글일 때만 코드를 본다. `model_not_found`면 `the model '<모델>' is not available for this ChatGPT account (model_not_found). Set agents.defaults.model to a model the account offers, or run shacs-bot provider codex login again to pick one.`이다.
     - `<모델>`은 넘겨받은 이름이 비지 않고 100자 이하이며 영문자·숫자·`.`·`-`·`_`·`:`·`/`로만 됐을 때 그 이름이다. 아니면 문장 앞부분 `the model '<모델>'`을 `the configured model`로 바꾼다(`the configured model is not available for this ChatGPT account (model_not_found). …`).
     - 다른 코드는 `None`이다. 본문의 `message`·`detail`·`type` 같은 글은 읽어도 쓰지 않는다.
     - `error_status_code`, `error_retry_after_s`, `error_should_retry`, `finish_reason`은 지금 그대로다.
   - 이미지·media 경로(`media.rs`)의 실패 처리, 다른 provider client는 바꾸지 않는다.
3. **계정 ID(`crates/shacs-cli/src/lib.rs`):**
   - `extract_codex_account_id`를 둘로 나눈다.
     - `nested_codex_account_id(token)`은 claim의 `https://api.openai.com/auth` 객체 안 `chatgpt_account_id`(비지 않은 글)를 본다.
     - `legacy_codex_account_id(token)`은 지금 세 칸을 지금 순서로 본다.
   - `token_response_from_json`의 순서는 아래와 같다. 로그인과 갱신이 모두 이 함수를 쓰므로 같이 바뀐다. 저장과 header 사용은 그대로다.
     1. id token의 nested
     2. access token의 nested
     3. id token의 legacy
     4. access token의 legacy
4. **강제 종료 뒤 시작(`crates/shacs-cli/src/lib.rs`):**
   - `lock_aware_ownership_can_be_replaced(ownership, now_ms, own_pid)`로 넓힌다. 조건은 lock protocol이 `exclusive_file_v1`이고, 아래 셋 중 하나다.
     - `!pid_alive`
     - `now_ms > expires_at_ms`
     - `marker.pid == own_pid && process_started_after_marker`
   - 호출하는 곳(`acquire_runtime_ownership_marker`, process lock을 잡은 뒤)은 `std::process::id()`를 넘긴다. 바꿀 때의 lifecycle 기록(`startup_replaced_released_owner`)과 순서는 그대로다.
   - 다른 상태(Active, lock protocol이 없는 stale)는 지금처럼 막는다. `runtime recover`의 `remove_stale_runtime_ownership_marker_locked`, `runtime stop`·`restart`는 바꾸지 않는다.
   - 시험을 위해 marker를 읽는 곳에 procfs 자리를 받는 변형을 둔다. 이 기계(macOS)에는 `/proc`가 없어 뒤에 시작한 증거를 만들 수 없기 때문이다.
     - `read_runtime_ownership_marker_with_proc_root(path, proc_root)`: `pid_is_alive_with(pid, proc_root, "kill")`과 `owner_process_started_after_marker(marker, proc_root)`를 쓴다.
     - `inspect_runtime_ownership_with_proc_root(data_dir, now_ms, proc_root)`
     - `acquire_runtime_ownership_marker_with_proc_root(data_dir, path, marker, request_path, proc_root)`
     - 지금 함수들은 `/proc`로 이 변형을 부르고 동작은 같다. 지금의 `pid_is_alive_with`, `owner_process_started_after_marker_with_ticks` 꼴과 같다.
5. **문서:**
   - USAGE "Codex provider 인증":
     - login이 계정의 모델 목록으로 모델을 고르는 규칙과 `Model check:` 줄.
     - 목록을 읽지 못할 때의 동작.
     - `gpt-5.4` 두 문장을 지금 사실로 바꾼다.
     - `accountId`를 token의 실제 꼴에서 읽는다는 것.
     - Codex가 `model_not_found`로 거절했을 때 봇이 하는 답과 고치는 법.
   - USAGE runtime 시작 문단: lock-aware 인계에 자기 PID인 marker의 예외를 더한다.
   - USAGE Docker Compose 절:
     - 설정 스크립트 5번(로그인)에 모델 고르기 한 문장.
     - `~/.shacs-bot/config.json`·`.env.json`을 고친 뒤 `docker compose restart shacs-gateway`로 다시 띄워야 반영된다는 것과, 저장소 `.env`·`docker-compose.yml`을 고쳤으면 `docker compose up -d shacs-gateway`.
   - README Docker 문단: config를 고친 뒤 gateway를 다시 시작한다는 한 문장.

## 앞선 PRD에서 바꾸는 것(052 안)

| PRD | 지금 | 바뀐 뒤 |
|---|---|---|
| 040 | 로그인 저장(`save_codex_login_token`)은 provider·모델(`gpt-5.4`)·인증·fingerprint를 적는다 | 모델은 계정 목록으로 고른다(범위 1). 인증과 fingerprint를 적는 것은 그대로다 |
| 020 | USAGE 설정 스크립트 5번은 로그인 방법만 적는다 | 로그인이 계정 목록으로 모델을 고른다는 한 문장을 더한다. 스크립트는 바꾸지 않는다 |

PRD 020·040 문서는 닫힌 기록이라 고치지 않는다.

## Non Scope

- `provider status`나 gateway 시작이 모델이 계정에 있는지 확인하기(네트워크가 필요하다), 모델을 고르는 명령(`provider codex models` 같은 것).
- `model_not_found` 밖의 Codex 오류 코드, 다른 provider의 오류 문장.
- `provider codex import-token`의 네트워크 사용.
- gateway 정상 종료 시간, Compose `stop_grace_period`.
- 여러 기계가 한 data 폴더를 나눠 쓰는 모양(015가 지원하지 않는다).

## Parent Requirement Mapping

- [052 SPEC](../SPEC.md)의 소유 범위 11과 그 문서, 수용 조건 13과 이 PRD 변경에 대한 수용 조건 9, 10을 소유한다. "앞선 스펙에서 바꾸는 것"의 060 행 넷(010, 010·030, 003·019·034, 029·030)을 구현한다.

## Required Contract

1. 로그인은 모델 목록을 한 번 묻고, 목록을 읽지 못해도 로그인 저장은 지금처럼 끝난다. token과 계정 ID 값은 출력에 없다.
2. 정해 둔 코드가 아니면 Codex 실패 답은 지금 문장 그대로이고, 어느 경우든 공급자가 쓴 글은 답·오류·로그에 없다.
3. 인계는 process lock을 잡은 뒤이고, marker의 PID가 자기 PID이며 자기가 뒤에 시작했다는 증거가 있을 때만 heartbeat가 유효해도 한다. 다른 PID의 유효한 heartbeat와 lock protocol이 없는 marker는 지금처럼 막는다.
4. 시험은 실제 Codex나 사용자 인증을 쓰지 않는다(가짜 transport, 로컬 HTTP 서버).
5. `Cargo.lock`에 새 crate가 없다.

## Acceptance Criteria

시험은 `Given`/`When`/`Then` 주석을 쓴다. AGENTS.md대로 바뀐 동작에만, 되도록 기존 시험에 더한다.

- `shacs-cli` 새 단위 시험 파일 `src/codex_login_model_tests.rs`(lib.rs에 `#[cfg(test)] #[path]`로 붙임):
  1. 고르는 규칙 표(`choose_codex_model`, 한 시험):
     - 그대로 둠은 config가 `gpt-6-sol`이고 목록에 있을 때이고, 앞이 붙은 `openai_codex/gpt-6-sol`도 같다. 숨은 모델도 그대로 둔다.
     - 계정 목록에서 고름은 `gpt-5.4`일 때다. priority 1인 숨은 모델은 건너뛰고 priority 2인 보이는 모델을 고른다. priority가 같은 보이는 모델 둘이면 목록에서 앞의 것을 고른다.
     - 목록을 읽지 못했을 때(`Err`): Codex 모델이 있으면 그대로 두고, 없으면 `CODEX_DEFAULT_MODEL`을 쓴다. 보이는 모델이 없는 목록도 같다.
  2. 로그인 저장(`save_codex_login_token`, `account_models`로 `codex_account_models`와 이 파일의 작은 가짜 `CodexAuthTransport`를 넘김):
     - 다른 provider였던 config에서 계정 목록의 모델을 고르고 provider가 Codex가 된다.
     - GET 한 번의 URL이 config `apiBase`(가짜 값)의 `/codex/models?client_version=1.0.0`이고, header에 `Authorization: Bearer <token>`과 `ChatGPT-Account-Id`가 있다.
     - 목록의 priority 0–3 보이는 항목은 slug가 각각 계정 ID, token, 계정 ID를 가운데 담은 이름(`m-<계정 ID>-x`), token을 가운데 담은 이름(`model-<token>-x`)이다. 시험 token과 계정 ID는 짧은 값이라 꼴 검사는 통과하고, 담고 있어서 건너뛴다. 그래서 priority 4 모델을 고르고, config와 출력에 계정 ID와 token이 없다.
     - `format_codex_login_outcome`에 `Model check: picked the highest-priority model your ChatGPT account lists`가 있다.
- `shacs-cli` 새 단위 시험 파일 `src/runtime_ownership_takeover_tests.rs`(lib.rs에 `#[cfg(test)] #[path]`로 붙임):
  3. 강제 종료 뒤 인계(한 시험):
     - 실제 data 폴더에 자기 PID, 1분 전 시작, 지금 갱신(heartbeat 유효), lock protocol인 marker를 쓴다.
     - 가짜 procfs(`stat`의 `btime`, `<자기 PID>/stat`의 시작 tick)로 자기 process가 marker보다 10초 뒤에 시작했다고 만든다.
     - 그다음 `acquire_runtime_ownership_marker_with_proc_root`를 부른다. 결과는 아래와 같다.
       - 성공한다.
       - marker 파일이 새 owner id(같은 PID, 새 시작 시각)가 된다.
       - durable event에 `RUNTIME_OWNER_LIFECYCLE` 기록이 하나 생긴다.
     - 가짜 procfs가 자기 process가 marker보다 먼저 시작했다고 하면(뒤에 시작한 증거 없음, 이 기계와 Linux에서 같은 결과) 지금처럼 막는다(`runtime ownership` 오류). marker 파일은 byte 단위로 같고 lifecycle 기록은 없다.
- `shacs-cli` 고치는 기존 단위 시험(`lib.rs`):
  4. `codex_token_response_extracts_account_id_and_default_expiry`에 두 경우를 더한다.
     - 실제 꼴(`{"https://api.openai.com/auth": {"chatgpt_account_id": …}}`) id token
     - id token에는 옛 꼴 `organizations`만 있고 access token에 실제 꼴이 있을 때: access token의 계정 ID를 쓴다.
     - 지금 경우(id token의 맨 위 칸이 access token의 `organizations`보다 앞)는 그대로다.
  5. `fresh_lock_aware_heartbeat_blocks_pid_namespace_takeover`에 두 가지를 더한다. 자기 PID이고 뒤에 시작했으며 heartbeat가 유효하면 바꿀 수 있다. 자기 PID라도 뒤에 시작한 증거가 없으면 막는다. PID 1 두 경우는 그대로이고, 새 인자에는 `std::process::id()`를 넘긴다.
  6. `save_codex_login_token`을 부르는 기존 시험 둘은 새 인자에 목록을 읽지 못하는 함수(`|_| Err(…)`)를 넘긴다. 기대 값은 그대로다.
     - `codex_login_success_saves_oauth_session_without_config_secret`(`lib.rs`): 기본 config라 결과 모델은 `CODEX_DEFAULT_MODEL`이다.
     - `saved_credential_tests.rs`의 `codex_login`: 빈 config에서 시작해 기본값을 적는다. 이 시험은 모델을 보지 않고(표의 모델 칸은 runtime 해석에 넘기는 요청 모델이다) fingerprint와 인증 출처만 보므로 기대는 그대로다.
     - `MockCodexAuthTransport`(`lib.rs` 시험)는 `get_json`을 구현한다(부른 URL·header를 남기고 다음 응답을 낸다).
- `shacs-providers` 통합 시험(`tests/it/codex_error_security.rs`에 더함):
  7. 정해 둔 코드(`CodexClient::chat`, 클로저 transport, 한 시험):
     - `model_not_found` 본문의 `message`·`detail`에 비밀 같은 문자열을 넣는다. 답은 `HTTP 400: the model 'gpt-5.4' is not available …` 문장과 같고, 비밀과 공급자 글이 없다. `error_status_code`는 400이다.
     - 모르는 코드(`invalid_value`)는 `HTTP 400: provider error`다.
     - 64 KiB를 넘는(공백으로 늘린) `model_not_found` 본문도 `HTTP 400: provider error`다.
     - 꼴이 맞지 않는 모델 이름(공백이 든 이름)이면 답이 `HTTP 400: the configured model is not available …`이고 그 이름이 없다.
  8. 로컬 TCP 서버가 400과 `model_not_found` JSON(공급자 글에 비밀)을 준다.
     - `UreqCodexHttpTransport::post_json_stream`의 응답 구조체는 `Debug`·직렬화에 비밀과 공급자 글이 없고, 본문이 `{"error":{"code":"model_not_found"}}`이다.
     - 같은 transport로 부른 `CodexClient::chat`의 답이 7과 같은 문장이다.
- 9. 기존 `codex_error_security` 두 시험, `lock_aware_dead_owner_is_replaced_during_startup`, `prd007_stale_start_blocks_and_retains_marker`, `codex_import_token_writes_auth_without_leaking_token_to_config_or_output`(상수만 바뀜)가 그대로 통과한다.
- 10. 문서가 범위 5와 같다.
- 11. 검증은 순서대로 한다:
  - Google 옵션을 붙인 `cargo fmt --all -- --check`
  - workspace `cargo clippy --all-targets -- -D warnings`
  - `cargo test --manifest-path crates/Cargo.toml -p shacs-cli -q`와 `-p shacs-providers -q`
  - `Cargo.lock`이 바뀌지 않았는지 확인
  - 전체 workspace 시험은 사용자 승인 없이 돌리지 않는다.
- 12. (사용자 승인 뒤, 선택) 실제 Docker: 서버에서 새 image의 gateway 컨테이너가 돌 때, host에서 그 runtime process(`docker inspect -f '{{.State.Pid}}' shacs-gateway`, host 사용자와 같은 번호라 `sudo` 없이)를 `kill -KILL`로 죽인다. `docker kill`·`docker stop`은 손으로 끈 것이라 restart policy가 다시 띄우지 않으므로 쓰지 않는다. 그 뒤 Docker가 `unless-stopped`로 다시 띄우면 `RestartCount`가 1만 늘고, 새 로그에 stale ownership 오류가 없으며, 바로 시작한다. 새 image로 `provider codex login --headless`를 하면 `Model check:` 줄과 계정 목록의 모델이 적힌다(서버에서는 사용자가 로그인해야 한다). 돌리지 않으면 종료 확인에 그 결정과 확인하지 못한 것을 적는다.

## Closure Evidence

1. 바뀐 파일과 새 시험, 고친 기존 시험과 까닭.
2. 값이 없는 출력 예(로그인 출력의 `Model check:` 줄, `model_not_found` 답).
3. Cargo 검증 기록과 `Cargo.lock` 확인.

## 남은 제한

- 모델 목록은 공개 문서가 없는 ChatGPT backend 경로다. 꼴이 바뀌면 로그인은 config의 Codex 모델이나 코드의 기본값을 쓰고 까닭을 알린다. 코드의 기본값은 시간이 지나면 다시 계정에 없을 수 있다.
- 이미 계정에 없는 모델을 적어 둔 설치는 사용자가 모델을 바꾸거나 다시 로그인할 때까지 실패하고, 봇의 답이 할 일을 알린다.
- `import-token --select`는 목록을 보지 않고 코드의 기본값을 적는다.
- 자기 PID 인계는 Linux의 `/proc`로 뒤에 시작한 증거를 얻는다. 증거를 얻지 못하는 곳(`/proc`가 없거나 앞 process가 시작한 지 5초 안에 다시 뜸)에서는 지금처럼 heartbeat가 지나기를 기다린다.
- Cargo 시험은 marker 인계 경로를 가짜 procfs로 본다. 그 앞의 process lock과 시작 admission(바뀌지 않은 코드)까지 진짜 `/proc`로 거치는 확인은 수용 조건 12(실제 Docker)뿐이다.

## 구현 메모 및 검증 근거 (2026-11-09)

실행 기록은 로컬 `.omo/evidence/spec052/`(추적하지 않음)의 `plan-review.md`와 `prd060/`에 있다.

- 구현:
  - `shacs-providers` `clients/codex/`:
    - `transport.rs`: `read_codex_error_body_bounded`가 읽은 byte를 돌려준다. `known_error_code_body`는 정해 둔 코드만 담은 JSON(`{"error":{"code":"model_not_found"}}`) 아니면 빈 글을 `CodexHttpStreamResponse.body`에 둔다. 그래서 공급자 글은 transport를 넘지 않는다. 32 MiB 넘는 본문의 transport 오류는 그대로다.
    - `mod.rs`: `CODEX_ERROR_CODE_BODY_MAX_BYTES`(64 KiB)를 둔다. `chat_stream_bounded`가 요청 본문의 `model`(앞을 뗀 이름)을 실패 응답 해석에 넘긴다.
    - `response.rs`: `known_codex_error_code`(64 KiB 이하 JSON의 `error.code`가 `KNOWN_CODEX_ERROR_CODES`에 있을 때)를 transport와 함께 쓴다. `known_codex_error`가 `model_not_found`일 때만 052의 고정 문장을 낸다. `plain_model_name`이 아니면 `the configured model`이다. 429와 다른 실패는 그대로다.
  - `shacs-cli` `lib.rs`:
    - 상수 `CODEX_DEFAULT_MODEL = "gpt-6.1-sol"`, `CODEX_MODELS_CLIENT_VERSION = "1.0.0"`, `CODEX_API_BASE`(`codex_provider_config`도 씀).
    - `CodexAuthTransport::get_json`과 그 ureq 구현.
    - `codex_account_models`: 목록 요청, slug 꼴 검사(`plain_codex_model_name`)와 token·계정 ID를 담은 slug 건너뛰기.
    - `choose_codex_model`(`codex_model_slug`로 앞을 뗀 이름 비교)과 `CodexModelCheck`.
    - `save_codex_login_token(config_path, token, account_models)`, `CodexLoginOutcome.model_check`, `format_codex_login_outcome`의 `Model check:` 줄.
    - 계정 ID: `codex_token_claims`, `nested_codex_account_id`, `legacy_codex_account_id`. `token_response_from_json`은 id token nested, access token nested, id token legacy, access token legacy 순서로 본다(옛 `extract_codex_account_id`는 없앰).
    - ownership: `read_runtime_ownership_marker_with_proc_root`, `inspect_runtime_ownership_with_proc_root`, `acquire_runtime_ownership_marker_with_proc_root`(지금 함수는 `/proc`로 부름). `lock_aware_ownership_can_be_replaced(ownership, now_ms, own_pid)`가 `marker.pid == own_pid && process_started_after_marker`도 받는다. 시작은 `std::process::id()`를 넘긴다.
  - 문서:
    - USAGE runtime 시작 문단(자기 PID marker 인계)
    - USAGE "Codex provider 인증"(목록으로 고르기와 `Model check:`, `gpt-5.4` 문장, `accountId`, `model_not_found` 답, `import-token --select`)
    - USAGE Docker Compose 절(설정 스크립트 5번 한 문장, config·`.env.json`을 고친 뒤 `docker compose restart shacs-gateway`와 `.env`·Compose 파일을 고친 뒤 `docker compose up -d shacs-gateway`)
    - README Docker 문단 한 문장
    - SPEC 052 개정과 `docs/specs/README.md` 행
- 새 시험:
  - `shacs-cli` `src/codex_login_model_tests.rs`(lib.rs에 `#[cfg(test)] #[path]`):
    - `the_login_model_follows_the_account_list`: 수용 조건 1, 같은 priority 순서 포함.
    - `the_login_saves_a_listed_model_and_never_an_echo_of_its_secrets`: 수용 조건 2. 요청 URL·header, config, `Model check:` 줄을 보고, 계정 ID·token과 그것을 담은 slug가 config·출력에 없는지 본다.
  - `shacs-cli` `src/runtime_ownership_takeover_tests.rs`: `a_fresh_marker_with_this_pid_is_taken_over_only_with_later_start_evidence`(수용 조건 3).
    - 가짜 procfs로 뒤에 시작함·먼저 시작함 두 경우를 본다. 인계와 lifecycle 기록 1개, 막음과 marker byte·lifecycle 기록 0개를 확인한다.
    - 앞 marker의 갱신 시각을 1시간 뒤로 두어, 시험이 늦게 돌아도 heartbeat가 지나지 않는다.
  - `shacs-providers` `tests/it/codex_error_security.rs`:
    - `a_known_codex_error_code_gets_our_fixed_sentence_without_provider_text`: 수용 조건 7. `model_not_found`, 모르는 코드, 64 KiB 넘는 본문, 꼴이 맞지 않는 모델 이름을 본다.
    - `the_http_transport_keeps_only_a_known_error_code_and_the_client_explains_it`: 수용 조건 8. 로컬 TCP 서버(요청 두 번, 요청 본문까지 읽는 `read_request_with_body`)를 쓴다. transport 응답 구조체의 `Debug`·직렬화와 client 답을 본다.
- 고친 기존 시험과 까닭:
  - `codex_token_response_extracts_account_id_and_default_expiry`: 실제 꼴 id token과, id token 옛 꼴·access token 실제 꼴 경우를 더했다(수용 조건 4). 지금 경우는 그대로다.
  - `fresh_lock_aware_heartbeat_blocks_pid_namespace_takeover`: 새 인자(`std::process::id()`)와 자기 PID 두 경우를 더했다(수용 조건 5). PID 1 두 경우는 그대로다.
  - `codex_login_success_saves_oauth_session_without_config_secret`와 `saved_credential_tests.rs`의 `codex_login`: `save_codex_login_token`의 새 인자에 목록을 읽지 못하는 함수를 넘긴다. 기대 값은 그대로다(수용 조건 6).
  - `MockCodexAuthTransport`는 `get_json`을 구현한다(trait에 더한 메서드).
- 시험이 바뀐 동작을 잡는지: `lock_aware_ownership_can_be_replaced`에서 자기 PID 조건을 잠시 빼면 인계 시험이 실패하는 것을 보고 되돌렸다. 소스는 그 전과 byte 단위로 같다.
- 값이 없는 출력 예:

  ```text
  Selected model: gpt-6.1-sol
  Model check: picked the highest-priority model your ChatGPT account lists
  ```

  ```text
  HTTP 400: the model 'gpt-5.4' is not available for this ChatGPT account (model_not_found). Set agents.defaults.model to a model the account offers, or run shacs-bot provider codex login again to pick one.
  ```

- 검증:
  - Google 옵션을 붙인 `cargo fmt --all -- --check`와 workspace `cargo clippy --all-targets -- -D warnings`가 통과했다. 처음 clippy가 새 시험의 `type_complexity`를 잡아 `GetCalls` 별칭으로 고쳤다.
  - 마지막 소스로 `shacs-providers` 204 passed / 0 failed / 0 ignored(`prd060/providers-tests.log`)였다.
  - `shacs-cli`는 578 passed / 0 failed / 3 ignored(`prd060/cli-tests.log`, PRD 040·050과 같은 셈법)였다. 구현 검토 1차 뒤에는 시험 파일 하나(`runtime_ownership_takeover_tests.rs`)만 바뀌어 그 시험만 다시 돌렸다(통과).
  - 구현 검토 1차 뒤 USAGE의 `import-token` 기본 모델 문장(`gpt-5.4` → `gpt-6.1-sol`)도 고쳤다.
  - 바꾼 함수와 타입을 다른 crate가 쓰지 않는 것을 확인했다(`CodexLoginOutcome`, `save_codex_login_token`, `lock_aware_ownership_can_be_replaced`, `parse_codex_stream_http_response`).
  - `crates/Cargo.lock`과 `Cargo.toml`은 바뀌지 않았다.
- Oracle 독립 검토:
  - 계획 3차 **PASS**.
    - 1차 지적: id token 옛 꼴이 access token의 계정 ID를 가림, 목록 slug의 token·계정 ID 되돌림, 인계 경로 시험, owner 표기, 같은 priority와 64 KiB 시험, `Model check:` 글.
    - 2차 지적: 담고 있는 slug 시험, `docker kill`은 restart policy를 거치지 않음, 요약의 owner 표기.
  - 구현 2차 **PASS**.
    - 1차 지적: transport 구조체에 남던 공급자 원문, USAGE `import-token` 기본 모델 문장, 인계 시험의 시간 의존.
