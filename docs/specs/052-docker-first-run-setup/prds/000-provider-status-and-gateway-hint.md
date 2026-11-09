# PRD 000. provider 확인과 gateway 안내

Status: Complete (Scoped)

Depends on: 없음. 003·030·031·048의 닫힌 계약만 쓴다. 뒤 번호 PRD에 의존하지 않는다.

## Goal

runtime이 쓸 provider와 그 credential이 runtime이 실제로 쓸 자리에 있는지 알리는 `shacs-bot provider status`를 만들고, 없으면 gateway 시작 출력에 할 일을 한 줄로 적는다. provider 고르기와 credential 자리 찾기를 runtime 해석에서 실행·갱신·기록 없는 함수로 떼어 내, runtime과 두 표면이 같은 함수를 쓴다.

## Scope

1. **고르기와 자리 찾기(`shacs-core` `ProviderCredentialRuntime`):** 아래 두 함수를 두고, 지금 runtime 해석(`resolve.rs`, `local.rs`)의 순서와 규칙을 그대로 옮긴다. runtime과 `provider status`, 시작 안내가 같은 두 함수를 부른다.
   - **고르기(지금 `providers_for_selection` + `match_provider`):** 인증 저장소를 한 번 읽어(잠금 없이, 읽지 못하면 지금처럼 그 후보 없이) config의 provider 항목, 그 저장소의 provider, runtime에 설정된 override·선언의 provider를 후보로 하고, `apiKey`가 없는 후보에 자리 표시 값을 넣어 `ProviderRegistry::match_provider(agents.defaults.provider, model, 후보)`로 고른다. 이 읽기를 하지 못했는지도 돌려준다. 이미지 생성 경로는 지금처럼 같은 후보로 `resolve_image_generation_provider`를 쓰고 이 고르기를 쓰지 않는다.
   - **선언:** 지금처럼 runtime에 설정된 선언, 없으면 config 항목의 `credential_declaration`(registry가 OAuth provider로 적으면 OAuth, 아니면 API key, 기본 환경 변수는 registry의 `env_key`).
   - **자리 찾기(지금 `resolve_credential`의 순서):** 고른 provider에 대해 아래 순서로 첫 자리를 고른다. 인증 저장소가 필요하면 고르기와 따로 한 번 읽는다(잠금 없이, 지금 `resolve_local`의 읽기와 같다).
     1. runtime override: 호출의 override, 없으면 runtime에 설정된 그 provider의 override. 선언의 종류(API key, OAuth)와 맞을 때만이다.
     2. 선언된 환경 변수: runtime에 설정된 값(시험용 `with_environment`), 없으면 process 환경에 그 이름이 있으면 이 자리다. 값이 비어 있어도 이 자리다(지금과 같다).
     3. 선언이 인증 저장소를 허용하면: 저장소를 읽지 못했으면 "인증 저장소를 읽지 못함" 자리다(지금 runtime은 여기서 실패한다). 그 provider의 항목이 없으면 건너뛴다(`NoEntry`). 종류가 선언과 다르면(API key 선언과 `oauth` 항목, OAuth 선언과 `apiKey` 항목) 건너뛴다(`KindMismatch`). 종류가 맞으면 저장소의 그 provider fingerprint가 없거나 지금 선언의 것과 같을 때 이 자리이고, 다르면 건너뛴다(`FingerprintStale`).
     4. credential command 선언이 있으면 이 자리다. 실행하지 않는다.
     5. config `apiKey`의 앞뒤 공백을 뺀 값이 비어 있지 않으면 이 자리다.
     6. 그 밖에는 자리가 없다.
   - **자리의 사실:** 판정과 안내에 쓰는 사실만 담는다. 환경 변수의 이름과 값이 비었는지, 인증 저장소 API key가 비었는지, OAuth 항목이 비었는지·만료됐는지(`expires`가 있고 지금 이하, 지금 `unexpired`와 같다)·갱신 token이 있는지, 3번에서 건너뛴 까닭, 선언이 인증 저장소를 허용하는지, 그리고 저장소에 그 provider의 fingerprint가 있고 지금 선언의 것과 다른지(항목 종류와 상관없이. 로그인과 `import-key`는 이것을 지우지 않는다). "비었다"는 앞뒤 공백을 뺀 값이 빈 것이다. 값 자체는 runtime 해석 안에서만 쓰고 공개 결과와 그 `Debug`에 넣지 않는다.
   - **공개 함수:** `plan_provider_credential(request)`는 고르기와 자리 찾기만 하고, `agents.defaults.provider`가 알 수 없는 이름인지(`auto`가 아니고 `registry.find_by_name`이 없음. `auto`는 registry에 없어도 알 수 없는 이름이 아니다), 고른 provider와 model(없으면 없음), 자리와 그 사실, 인증 저장소를 두 읽기 중 하나라도 읽지 못했는지를 돌려준다. 호출의 override는 받지 않는다. 명령 실행, OAuth 갱신, 잠금 파일, fact 기록, 파일 쓰기가 없다.
2. **runtime 해석 바꾸기:** `resolve_provider_client_for_invocation`은 범위 1의 고르기로, `resolve_provider_config_for_invocation`(이미지 생성 경로 포함)은 받은 provider로 범위 1의 자리 찾기를 한 뒤 그 자리에서만 값을 얻는다.
   - override, 환경 변수, config 값: 그 값 하나로 `CredentialSourceDeclaration::resolve`를 불러 지금과 같은 결과(source, fingerprint 상태)를 만든다.
   - 인증 저장소 API key: 읽은 항목의 값과 fingerprint로 `resolve`(지금 `resolve_local`과 같다).
   - 인증 저장소 OAuth: 지금처럼 `resolve_oauth`(잠금, 다시 읽기, 만료면 갱신과 저장)를 부르고, 쓰기 없는 runtime(`without_local_auth_writes`)이면 `read_oauth`를 부른다. 다시 읽은 fingerprint가 달라 그 함수가 fingerprint 오류를 내면(처음 읽은 뒤 바뀜) 지금처럼 인증 저장소를 건너뛰고 범위 1의 4번부터 다시 고른다.
   - 인증 저장소를 읽지 못함: 지금과 같은 상태(Missing, source LocalAuthStore, fingerprint Unavailable)로 실패한다.
   - credential command: 지금처럼 cache를 보고 실행하며, 실패하면 config 값으로 넘어가지 않는다.
   - 자리가 없음: 지금과 같은 Missing 상태로 실패한다.
   - 상태 기록(`record_credential_status`), Codex의 `ChatGPT-Account-Id` header, `ProviderError::AuthRequired`는 지금과 같다.
3. **`provider status`:** `shacs-bot provider status [--config <path>]`(전역 `--config`도 받는다).
   - config를 `${이름}`까지 풀어 읽고 쓰지 않는다(`load_config_with_env`, `resolve_env: true`, `write_back_migrations: false`, process 환경). 031·048 marker가 있어도 멈추지 않고, marker가 있는지만 본다.
   - credential runtime은 gateway가 쓰는 생성 함수(`local_provider_credential_runtime`)로 만든다. fact store는 메모리에만 있는 새 것을 준다(판정은 fact를 쓰지 않는다). `agents.defaults.provider`·`model`과 config의 provider 항목으로 범위 1을 부른다.
   - 인증 저장소를 읽지 못했으면 그 파일 경로만 적은 오류로 종료 코드 1이다. config나 `.env.json`을 읽지 못하면 지금 CLI 오류로 종료 코드 1이다.
   - 판정: 자리가 있고 그 값이 비어 있지 않으면 `ready`. credential command 자리는 `ready`. 만료된 OAuth 항목은 갱신 token이 있으면 `ready`, 없으면 `missing`. 자리의 값이 비었거나 자리가 없거나 provider를 고르지 못하면 `missing`.
   - 출력: 첫 줄 `provider: ready <provider>` 또는 `provider: missing <provider 또는 none>`. 둘째 줄 `credential: <자리>`(`ready`) 또는 `reason: <까닭>`(`missing`). 셋째 줄 `next: <할 일>`은 `missing`이거나 marker가 있을 때만 있다. 종료 코드는 둘 다 0이다.
   - 까닭과 할 일(위에서부터 처음 맞는 줄. 로그인 명령은 Codex면 `provider codex login`, Copilot이면 `provider copilot import-token --token-stdin`):

     | 경우 | `reason` | `next` |
     |---|---|---|
     | marker 있음(판정과 상관없이) | 판정대로(`ready`면 없음) | `runtime config-migrate --recover`만 |
     | `agents.defaults.provider`가 알 수 없는 이름(`auto` 아님) | 그 이름이 알려진 provider가 아니다 | config의 `agents.defaults.provider`를 알려진 provider나 `auto`로 고치기 |
     | provider를 고르지 못함 | config와 인증 저장소의 provider 중 `agents.defaults.model`에 맞는 것이 없다 | Codex 로그인 명령, 또는 USAGE의 API key 설정 |
     | 환경 변수 자리의 값이 빔 | 그 이름이 비어 있고 runtime이 그것을 먼저 쓴다 | 밖: 그 변수에 key를 넣거나 지우기. 안: `docker-compose.yml` 옆 `.env`에 그 변수의 key를 적고 `docker compose up -d shacs-gateway` |
     | 인증 저장소 자리의 값이 빔, 만료되고 갱신 token 없음 | 그 사실 | OAuth provider면 로그인 명령, 아니면 `provider <id> import-key --token-stdin` |
     | 자리 없음, 저장소의 fingerprint가 지금 선언과 다름 | 저장할 때의 credential 선언(`providers.<id>.credentialSource`·`apiKeyRef`)과 지금이 다르다 | 그 선언을 저장할 때로 되돌리기(로그인과 `import-key`는 그 표시를 지우지 않아 알리지 않는다) |
     | 자리 없음, 선언이 인증 저장소를 허용하지 않음 | 그 사실 | 선언한 환경 변수(있으면)나 `providers.<id>.apiKey`에 credential 주기 |
     | 자리 없음(`NoEntry`, `KindMismatch`) | credential이 없다(종류가 다르면 저장된 항목의 종류가 맞지 않는다) | OAuth provider면 로그인 명령, 아니면 `provider <id> import-key --token-stdin`(밖이고 기본 환경 변수가 있으면 그 이름도) |

   - 명령 꼴: 공식 container 증거가 있으면 `docker compose run --rm shacs-cli <인자>`, 아니면 `shacs-bot <인자>`다. 증거는 `runtime_containment_inspect`의 결과가 `contained == Some(true)`이고 backend를 `+`로 나눈 첫 조각이 `official-container`인 것이다(`official-container+<sandbox>` 포함. `runtime_permission_rule_containment_from_snapshot`과 같은 읽기). container 꼴의 Codex 로그인은 `--headless`를 붙인다.
   - 자리 설명(`credential`): 환경 변수 이름, 인증 저장소의 API key, 인증 저장소의 로그인(만료됐으면 첫 호출에서 갱신한다고 적는다), credential command(실행하지 않았다고 적는다), `providers.<id>.apiKey`. 값, 값의 길이나 앞부분을 보이지 않는다.
4. **gateway 시작 안내:** `run`과 `runtime start`(같은 `run_runtime_with_mode`)가 runtime을 띄울 때, 채널 plan을 stderr에 적은 바로 뒤에 runtime이 읽은 bundle(이미 `${이름}`을 푼 config)과 범위 3과 같은 판정·까닭·할 일·명령 꼴로 아래를 적는다.
   - `missing`이면 한 줄: `provider: missing <provider 또는 none> (<까닭>); next: <할 일>`.
   - 인증 저장소를 읽지 못하면 한 줄: `provider: not checked (the auth store could not be read); next: <명령 꼴> provider status`.
   - `ready`면 적지 않는다.
   - 판정과 그 실패는 runtime 시작을 막지 않는다. marker가 있으면 runtime 시작이 이미 멈추므로 이 안내에 오지 않는다. 띄울 process가 없어 plan만 돌려주는 경우(`runtime_needs_process`가 거짓)는 바꾸지 않는다.
5. **문서:** `docs/USAGE.md`의 provider 인증 절과 Docker Compose 절에 `provider status`(판정 기준, 출력, marker, container에서 `shacs-cli`로 실행하는 까닭)와 시작 안내를 적는다. Docker Compose 절의 "Provider 설정이 없거나 명시적으로 활성화한 선택 도구의 provider 인증이 빠져 있으면 runtime은 시작하지 않으므로" 문장을 확인한 동작으로 고친다. provider 부분은 범위 4대로 적는다. 선택 도구 부분은 코드로 확인한 동작으로 적는다: 이미지 생성(`tools.imageGeneration`)을 켜고 그 provider가 알 수 없는 이름이거나 이미지 생성을 지원하지 않으면 runtime은 채널 plan 전, agent 준비 단계에서 시작하지 못한다(`spec035_cli_media/tool_registry.rs`의 `resolve_image_generation_provider`). 알려진 provider에 credential이 없을 뿐이면 시작하고, 그 도구를 부를 때 인증 오류가 난다(credential은 호출 때 읽는다). README의 명령 소개에 `provider status` 한 줄을 더한다.
   - README와 USAGE의 Docker provider secret 문장(README Docker 문단, USAGE Docker Compose 절과 `.env.example`의 설명)에, 공식 Compose가 정하지 않은 provider 변수를 빈 값으로 넘기고 runtime이 그 빈 값을 인증 저장소와 config(`.env.json`으로 푼 값 포함)의 key보다 먼저 써서 key 없이 요청한다는 것을 적는다. `${이름}` 풀기에서는 빈 값이 `.env.json`을 가리지 않지만(048), provider credential 자리 순서에서는 가린다는 차이를 함께 적고, Docker에서는 Compose `.env`에 key를 적거나 ChatGPT 로그인을 쓰라고 안내한다(SPEC 소유 범위 8, 남은 위험).

## Non Scope

- 빈 환경 변수를 없는 것으로 보기, 다시 로그인하거나 key를 가져올 때 남은 fingerprint 지우기. 사용자 결정을 기다린다(SPEC 남은 위험).
- 환경 변수만 있는 provider를 후보에 넣기, readiness `provider_auth` 판정, `serve`(local API) 시작 출력.
- 실제 provider 호출로 credential이 맞는지 보기.
- migration 중의 `onboard`와 `channels connect discord`(PRD 010), 스크립트와 HOME(PRD 020), 권한 오류(PRD 030).

## Parent Requirement Mapping

- [052 SPEC](../SPEC.md)의 소유 범위 1, 2와 소유 범위 8 중 그 문서(`provider status`, 시작 안내, USAGE의 "Provider 설정이 없거나 …" 문장, README·USAGE의 Docker provider secret 문장과 빈 환경 변수), "반드시 지킬 것"의 같은 코드·쓰지 않음·비밀 값 항목, 수용 조건 1, 2와 이 PRD 변경에 대한 수용 조건 9, 10을 소유한다.

## Required Contract

1. `provider status`, 시작 안내, runtime 해석은 범위 1의 같은 고르기·자리 찾기 함수를 쓴다.
2. 판정은 네트워크, credential command 실행, OAuth 갱신, 잠금 파일, fact 기록, 파일 쓰기 없이 한다.
3. runtime이 고르는 provider와 자리, 쓰는 값, 실패 상태와 그 기록은 지금과 같다. 다른 점은 범위 2의 다시 고르기와 아래 남은 제한의 경합뿐이다.
4. 출력과 오류에 비밀 값과 그 일부가 없다.
5. 시작 안내는 runtime 시작을 막지 않는다.
6. `Cargo.lock`에 새 crate를 더하지 않는다.

## Acceptance Criteria

1. 같은 경우 표(`shacs-core` 시험): 임시 인증 저장소, runtime에 설정한 환경 변수 값(`with_environment`), 호출을 세는 시험 갱신기, 시험 command로 SPEC 수용 조건 1의 경우를 만든다. 실제 process 환경에 기대지 않게 각 경우의 환경 변수는 runtime에 설정하거나 시험에만 쓰는 이름을 선언한다.
   - `ready`: Codex를 고른 config와 fingerprint 없는 Codex OAuth 항목(만료 안 됨, 만료됐고 갱신 token 있음), 기본 config(`auto`, `anthropic/…`)와 fingerprint 없는 Anthropic API key 항목(환경 변수 없음), fingerprint가 지금 선언과 같은 항목, config `anthropic` 항목과 비어 있지 않은 환경 변수, `.env.json`으로 푼 값과 같은 config `apiKey`(환경 변수 없음), credential command 선언.
   - `missing`: 인증 없음(provider 없음), 항목 없이 환경 변수만 있음(provider 없음), 고른 provider가 아닌 provider에만 인증, 인증 저장소 key와 빈 환경 변수, fingerprint가 다른 항목뿐, 만료됐고 갱신 token이 없는 Codex 항목, Codex를 고른 config에 `apiKey` 종류 항목만 있음(fingerprint 없음: `KindMismatch`와 로그인 안내. fingerprint가 다름: 선언 되돌리기 안내).
   - 각 경우 `plan_provider_credential`의 provider와 자리를 `resolve_provider_client`의 결과와 비교한다: `ready`면 같은 provider를 고르고 그 자리의 source(`CredentialSource`)와 값(command는 시험 command 출력, 만료 OAuth는 시험 갱신기 값)을 쓴다. `missing`이면 provider를 고르지 못하거나(`ProviderNotFound`), `AuthRequired`로 실패하거나, 빈 값을 쓴다.
   - command 경우는 같은 runtime으로 두 번 해석해 command가 한 번만 돌고(표시 파일의 줄 수) 두 번 모두 `CredentialSource::Command`인지 본다(cache 유지).
2. 쓰지 않음(`shacs-core` 시험): `plan_provider_credential` 전후 config, `.env.json`, `auth.json`이 byte 단위로 같고, `auth.json.lock`이 생기지 않고, command가 만들 표시 파일이 없고, 갱신기 호출이 0번이며, fact store의 credential 상태가 바뀌지 않는다.
3. runtime 회귀: 기존 030 시험(`shacs-config`의 `spec030_auth_*`, `shacs-core`의 `spec030_provider_credentials_*`와 `tests/it/spec030_provider_credentials_image.rs`, `shacs-tui`의 `spec030_credential_resolution_surfaces`, `shacs-cli`의 `spec030_provider_credential_*`)이 고치지 않고 통과한다. 더하는 회귀 시험은 둘이다.
   - 처음 읽은 뒤 잠금 안에서 fingerprint가 달라진 OAuth 항목은 다음 자리(config 값)로 넘어간다(자리 찾기 뒤 저장소를 바꾸는 시험 hook).
   - 이미지 생성 client가 `requested_provider: "auto"`와 인증 저장소의 Codex OAuth 항목만으로 지금처럼 `resolve_image_generation_provider`가 고른 Codex를 쓰고 그 항목의 access를 쓴다.
4. CLI 시험(`shacs-cli` `tests/it/`, binary를 `env_clear`한 환경에 `HOME`·`PATH`만 넣어 실행):
   - `ready`인 Codex 항목은 첫 줄 `provider: ready openai_codex`, 둘째 줄 `credential:`, 종료 코드 0이다.
   - 인증이 없는 기본 config(`auto`)는 `provider: missing none`이고, `reason:`은 고를 provider가 없다는 까닭(알 수 없는 이름이 아님)이며, `next:`에 `shacs-bot provider codex login`이 있다.
   - Anthropic key 항목과 빈 `ANTHROPIC_API_KEY`는 `provider: missing anthropic`이고 까닭과 할 일에 `ANTHROPIC_API_KEY`가 있다.
   - `agents.defaults.provider`가 없는 이름이면 `provider: missing none`이고 까닭에 그 이름, 할 일에 `agents.defaults.provider`가 있으며 로그인 안내가 없다.
   - 031 marker와 048 marker 각각이 있으면 첫 줄이 marker 없을 때와 같고, `next:`는 `runtime config-migrate --recover`뿐이며 `login`·`import-key`가 없다.
   - 읽을 수 없는 config, 모양이 틀린 `.env.json`, 읽을 수 없는 `auth.json`은 종료 코드 1이다.
   - 실행 전후 config, `.env.json`, `auth.json`이 byte 단위로 같고 `auth.json.lock`이 없다. stdout·stderr에 시험 key·token(canary)이 없다.
5. 명령 꼴(`shacs-cli` 단위 시험): containment 증거(`RuntimeContainmentEvidence::from_parts`)가 official-container이고 backend가 `official-container`와 `official-container+<sandbox>`인 두 경우 모두 `next:`가 `docker compose run --rm shacs-cli provider codex login --headless` 꼴이고, container 증거만 있거나 없으면 `shacs-bot provider codex login`이다. 빈 환경 변수의 container 꼴 할 일은 `.env`를 가리킨다.
6. gateway 시험(`shacs-cli` `tests/it/`, binary `run`을 빈 port로 띄우고 stderr를 읽는 thread로 받음):
   - 인증이 없는 config: 정한 시간 안에 채널 plan과 그 뒤의 `provider: missing` 한 줄을 받고, 그때 `try_wait()`가 아직 끝나지 않았으며, `runtime stop` 뒤 정한 시간 안에 정상 종료한다. 정해진 sleep으로 기다리지 않는다.
   - `ready`인 config(`custom` provider와 `apiKey`): 채널 plan을 받은 뒤 정상 종료까지 그 줄이 없다.
   - 읽을 수 없는 `auth.json`: `provider: not checked` 한 줄을 받고 그때 process가 살아 있다.
7. 문서가 범위 5와 같다(SPEC 수용 조건 9 중 이 PRD 몫).
8. fmt(Google 옵션), workspace clippy `-D warnings`, 변경한 package(`shacs-core`, `shacs-cli`, 시험을 고치지 않았는지 보는 `shacs-config`·`shacs-tui`) 시험이 통과한다. `Cargo.lock`이 바뀌지 않는다.

## Closure Evidence

1. 고르기·자리 찾기 함수, runtime 해석 바꾸기, `provider status`, 시작 안내, 문서의 구현과 시험.
2. 고친 기존 시험이 있으면 그 까닭(없어야 한다).
3. 값이 없는 `provider status` 출력 예(`ready`, 빈 환경 변수의 `missing`, marker).
4. Cargo 검증 기록과 `Cargo.lock` 확인.

PRD 000은 PRD 010–030 없이 닫힌다.

## 남은 제한

- 인증 저장소 읽기는 지금처럼 고르기에 한 번, 자리 찾기에 한 번, OAuth 값을 얻을 때 한 번이다. 마지막 읽기는 보통 runtime이면 잠금 안(`resolve_oauth`), 쓰기 없는 runtime이면 잠금 없이(`read_oauth`) 한다. 지금 runtime은 OAuth 항목의 fingerprint를 그 마지막 읽기에서만 보지만, 자리 찾기는 그 앞의 읽기에서도 본다. 그래서 자리 찾기의 읽기에서 달랐다가 마지막 읽기 전에 같아진 경우(다른 process가 그 사이 저장소를 고침)만 두 runtime 모두 지금과 달리 인증 저장소를 건너뛴다. 반대 순서(같았다가 마지막 읽기에서 달라짐)는 범위 2의 다시 고르기로 지금과 같다.
- 판정은 gateway를 띄운 process 환경이 아니라 명령을 실행한 process 환경으로 한다. Docker에서는 `shacs-cli` 서비스로 실행해야 gateway와 같은 Compose 환경을 받는다(SPEC 소유 범위 1).
- SPEC 남은 위험의 빈 환경 변수, 남은 fingerprint, 환경 변수만 있는 provider는 그대로이고 `provider status`는 그것을 그대로 보인다.
- 공식 container 증거는 `runtime inspect`와 같은 판정이라, container 안에 Docker socket이 보이면(unsafe privileged) 명령 꼴이 `shacs-bot …`로 나온다.

## 구현 메모 및 검증 근거 (2026-11-09)

실행 기록은 로컬 `.omo/evidence/spec052/`(추적하지 않음)의 `plan-review.md`와 `prd000/`에 있다.

- 구현:
  - `shacs-core` `runtime/provider_credentials/provider_credential_plan.rs`(새 파일): 공개 `ProviderCredentialPlan`·`PlannedProvider`·`CredentialSlot`·`LocalAuthSkip`과 `plan_provider_credential`. runtime과 같이 쓰는 `select_provider`(지금 `providers_for_selection`과 같은 후보를 만들고 그 읽기의 저장소를 함께 돌려줌), `declaration_for`, `choose_slot`(override → 선언된 환경 변수 → 인증 저장소 → credential command → config 값). `fingerprint_differs`는 고르기 읽기의 저장소로 정해, 앞 자리가 이기거나 선언이 인증 저장소를 막아도 맞게 나온다(두 번째 읽기가 없다).
  - `resolve.rs`: 고르기는 `select_provider`, 선언은 `declaration_for`, 해석은 `resolve_credential_observed`(자리를 고른 뒤 그 자리에서만 값을 읽고, 잠금 안에서 fingerprint가 달라진 OAuth면 인증 저장소 다음부터 다시 고름). `local.rs`에는 OAuth 값 읽기(`resolve_oauth`·`read_oauth`)만 남았다.
  - `shacs-cli` `provider_status_cli.rs`(새 파일): `provider status`(쓰지 않는 config 읽기, 메모리 fact store, 인증 저장소를 읽지 못하면 종료 코드 1, marker가 있으면 recover만 안내), gateway 시작 줄(`startup_line`), 명령 꼴, 까닭·할 일 표. `lib.rs`: `ProviderCommand::Status`, `provider status` 해석, 도움말, `run_runtime_with_mode`의 채널 plan 바로 뒤 출력.
  - 문서: USAGE "provider 인증 확인" 절, Docker Compose 절의 시작 문장과 빈 환경 변수 경고, README의 명령 소개와 Docker 문단, `.env.example`.
- 계획 밖에서 고친 것: USAGE의 "Runtime startup은 … 만료된 Codex access token을 갱신" 문장. 실제로는 provider를 부를 때 갱신하므로(`client.rs` 호출 때 해석) 그 시점으로 고쳤다. `provider status`의 "첫 호출에서 갱신"과 맞춘 것이다.
- 시험:
  - `shacs-core` 단위 시험 `provider_credential_plan_tests.rs` 5개: 같은 경우 표 17행(ready 8, missing 9. plan의 provider·자리·건너뛴 까닭·fingerprint 다름과 runtime의 provider·source·값을 비교), 알 수 없는 provider와 읽을 수 없는 저장소, 쓰지 않음(`auth.json` byte, 잠금 파일, command 표시, 갱신기 0번, fact 상태), command cache, 잠금 안에서 낡은 로그인의 다음 자리.
  - `shacs-core` `tests/it/spec030_provider_credentials_image.rs`: `auto` 이미지와 Codex 로그인만 있는 회귀 시험 1개.
  - `shacs-cli` 단위 시험 `provider_status_cli_tests.rs` 2개(명령 꼴, container 안내), 통합 시험 `tests/it/spec052_provider_status.rs` 7개(binary를 `env_clear`한 환경으로: ready·missing·알 수 없는 provider·빈 환경 변수, 031·048 marker, 읽을 수 없는 세 파일, gateway 세 경우).
  - 고친 기존 시험: 없다.
- 값이 없는 출력 예(로컬 실행):

  ```text
  provider: missing anthropic
  reason: ANTHROPIC_API_KEY is set but empty, and the runtime reads it before any saved key
  next: set ANTHROPIC_API_KEY to the key, or unset it
  ```

  `ready`는 `provider: ready anthropic` / `credential: API key saved in the auth store`, marker가 있으면 셋째 줄이 `next: finish the interrupted config migration first: shacs-bot runtime config-migrate --recover`다.
- Cargo 검증: Google 옵션을 붙인 `cargo fmt --all -- --check`, workspace `cargo clippy --all-targets -- -D warnings` 통과. 마지막 소스로 `shacs-core` 1,985 passed / 0 failed / 5 ignored, `shacs-cli`+`shacs-tui` 556 passed / 0 failed / 3 ignored(로컬 `prd000/core-tests-final.log`, `cli-tui-tests-final.log`). `crates/Cargo.lock`은 바뀌지 않았다. `shacs-config`는 바꾸지 않았다.
- Oracle 독립 검토: 계획 3차 **PASS**. 구현 4차 **PASS**(1차: 선언이 인증 저장소를 막을 때 fingerprint 다름을 못 봄, 2차: 상태용 두 번째 읽기의 오류를 버림과 gateway 시험의 주기적 확인, 3차: 앞 자리가 이길 때 fingerprint 다름을 못 봄).
## 종료 확인 (2026-11-09)

- 전체 workspace 시험: 사용자 승인(2026-11-09)을 받아 PRD 000과 PRD 010을 담은 작업 트리에서 `cargo test --manifest-path crates/Cargo.toml --workspace --locked --no-fail-fast`를 돌렸다. suite 211개, 3,727 passed / 0 failed / 8 ignored, exit 0이다(로컬 `.omo/evidence/spec052/closure/workspace-tests.log`).
- 수용 조건 1–8을 위 시험과 검증으로 확인했고, 계획 3차·구현 4차 독립 검토가 PASS다. PRD 000은 `Complete (Scoped)`로 닫는다. 전체 출시 seal이 아니다.
- 남은 제한은 위 "남은 제한"과 같다. SPEC 남은 위험의 빈 환경 변수와 다시 로그인해도 남는 fingerprint는 사용자 결정(2026-11-09)으로 이 스펙의 뒤 PRD에서 고친다.
