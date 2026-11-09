# PRD 030. 알아듣기 쉬운 권한 오류

Status: Complete (Scoped)

Depends on: [PRD 020](020-setup-script-and-container-home.md)의 결과(고치는 방법에 적는 `scripts/docker-setup.sh`와, 그 스크립트가 저장소 `.env`에 적는 `SHACS_UID`·`SHACS_GID`). [PRD 000](000-provider-status-and-gateway-hint.md)의 명령 꼴 판정(`CommandForm`, 023 official-container 증거)을 같이 쓴다. 뒤 번호 PRD에 의존하지 않는다. PRD 030의 종료 확인은 PRD 020이 닫힌 뒤에 한다.

## Goal

data 폴더에 쓰는 명령이 시작할 때 그 폴더에 쓸 수 있는지 먼저 본다. 쓸 수 없으면 원시 오류(`CLI I/O failed: Permission denied (os error 13)`, `config I/O failed: …`, `durable child recovery admission failed: …`) 대신 폴더 경로, 그 주인과 실행 번호, 고치는 방법을 알리고 멈춘다.

## 지금 동작(2026-11-09 확인)

`onboard`로 채운 data 폴더에서 쓰기 권한을 모두 뺀 뒤(다른 사용자의 폴더와 같은 상태) debug binary로 명령을 돌린 결과다.

- 권한 오류로 끝남: `onboard`, `run`, `serve`, `gateway`, `web`, `ask`, `agent`, `runtime start`·`stop`·`restart`·`recover`·`update`, `runtime migrate --apply`·`--resume`, `provider codex import-token`, `provider copilot import-token`, `provider <id> import-key`, `channels connect discord`, `memory index`, `session create`, `permissions revoke`(저장소 I/O 오류). 오류 글은 명령마다 다르고, 어느 폴더인지·누구 것인지·어떻게 고치는지 알리지 않는다.
- 지금처럼 돎: `status`, `tasks --json`, `runtime inspect`, `runtime trusted-runtime`, `runtime diagnostics`, `runtime migrate`(dry run), `runtime config-migrate`(dry run, 고칠 것 없는 `--apply`), `permissions auto-mode reset --yes`(되돌릴 것 없음), 목록·조회 명령(`session list`, `skills list`, `plugins list`, `hooks list`, `channels list`·`status`, `context files list`, `context refs resolve`, `permissions list`), `memory search`(색인을 못 열면 글자 검색으로 답함), `provider status`, `channels connect discord --status`.
- 빈 data 폴더에 쓰기 권한이 없으면 `runtime inspect`도 `CLI I/O failed: Permission denied`로 끝난다.
- 코드로 본 것: `plugins enable`·`disable`과 `permissions auto-mode reset`·`setup`은 config를, `permissions revoke`는 data 폴더의 `permissions.json`을, `memory model install`·`index`는 data 폴더의 memory 검색 자리를, `tasks`의 `recovery` action(`RuntimeRecover`)은 `--workspace`가 없으면 config의 data 폴더를 쓴다. `tasks`의 `goal` action(pause·resume)은 workspace를 쓴다. `session` 쓰기와 `goal`·`work`(`--workspace` 필수)·`trajectory record`(`--workspace`·`--store` 필수)·`improve`(`--root`)는 workspace나 명령에 준 폴더를 쓴다.

## Scope

1. **확인하는 명령(정확한 목록):** `run_command`가 명령을 실행하기 전에 아래 명령만 확인한다. config의 data 폴더에 쓰는 명령이다.
   - `onboard`(`--wizard` 포함).
   - runtime을 띄우는 명령: `run`, `serve`(`api serve`), `gateway`, `web`, `ask`, `agent`.
   - `runtime start`, `stop`, `restart`, `recover`, `update`, `migrate --apply`·`--resume`, `config-migrate --apply`·`--recover`.
   - provider 인증 명령: `provider codex login`, `provider codex import-token`, `provider copilot import-token`, `provider <id> import-key`.
   - `channels connect discord`(`--status` 빼고).
   - `plugins enable`·`disable`, `permissions revoke`, `permissions auto-mode reset`·`setup`.
   - `memory model install`, `memory index`.
   - `--workspace`와 `--data-dir` 없이 `recovery` action(`RuntimeRecover`)을 준 `tasks`. `--data-dir`만 주면 `tasks`가 쓰기 전에 그 조합을 거절하므로 확인하지 않는다.

   그 밖의 명령은 확인하지 않고 지금과 같다. 읽기 명령은 쓸 수 없는 폴더에서도 지금 돌기 때문이다. workspace나 명령에 준 폴더에 쓰는 명령(`session` 쓰기, `goal`, `work`, `trajectory record`, `improve`, `tasks`의 `goal` action, `--workspace`를 준 `tasks`, `runtime diagnostics --bundle`)은 data 폴더를 보지 않으므로 이 PRD에서는 그대로다(남은 제한).
2. **data 폴더:** 명령의 config 경로(`--config`, 없으면 기본 `~/.shacs-bot/config.json`)로 runtime과 같은 함수(`shacs_config::config_context`)가 정하는 `data_dir`이다. 상대 경로는 지금 폴더 기준의 절대 경로(`std::path::absolute`)로 바꿔 보고 알린다. `--workspace`로 data 폴더 밖에 둔 workspace는 보지 않는다.
   - **기본 data 폴더:** `config_context(None, None).data_dir`(`$HOME/.shacs-bot`)와 절대 경로가 같은 폴더다. 이 폴더만 shacs-bot 전용으로 보아 주인 바꾸기 명령을 알린다(범위 4).
3. **확인:** 아무것도 만들거나 쓰지 않는다.
   - `symlink_metadata(data 폴더)`:
     - 폴더면 그 폴더를 묻는다.
     - symlink면 따라간 `metadata`가 폴더일 때만 그 폴더를 묻고, 글에 link와 그 대상(`fs::canonicalize`)을 적는다. 대상이 없거나 폴더가 아니면 확인을 지나간다.
     - 없으면(`NotFound`) 위로 올라가 처음 있는 것을 찾는다. 따라간 `metadata`가 폴더면 그 폴더를 묻고(data 폴더는 그 아래에 만들어진다), 아니면 지나간다.
     - 폴더가 아닌 것, 그 밖의 오류는 지나간다.
   - 묻기는 `libc::faccessat(AT_FDCWD, path, W_OK | X_OK, AT_EACCESS)`다. 실제 쓰기처럼 실효 번호로, 보조 그룹과 ACL까지 본다. 실행 번호는 `geteuid`·`getegid`, 주인과 mode는 따라간 `metadata`다.
   - 답이 `EACCES`일 때만 쓸 수 없다고 본다. 그 밖의 답(`EROFS`, 경로에 NUL 등)은 지나가고 명령이 지금처럼 돈다. 그때의 오류도 지금과 같다.
   - 확인을 지나도 실제 쓰기는 지금처럼 실패할 수 있고, 그 오류는 지금과 같다(SPEC "반드시 지킬 것"). 확인과 쓰기 사이의 바뀜, NFS의 root squash처럼 서버가 따로 정하는 경우가 그렇다.
4. **글(영어, 종료 코드 1):** 새 `CliError::DataFolder(String)`이 머리말 없이 그대로 보인다. 경로, 번호, mode만 있고 비밀 값은 없다.
   - 첫 줄:
     - 폴더가 있을 때: `cannot write to the data folder <path>: it belongs to <uid>:<gid> (mode <0755>) and this command runs as <uid>:<gid>`. symlink면 `<path>` 뒤에 `, a link to <target>`.
     - 없을 때: `cannot create the data folder <path>: <처음 있는 위 폴더> belongs to <uid>:<gid> (mode <0755>) and this command runs as <uid>:<gid>`.
   - 둘째 줄(있으면 하나):
     - mode가 이미 허용할 때(실행 사용자에게 해당하는 칸이 `w`와 `x`를 가짐. 실효 UID가 주인이면 주인 칸, 아니고 실효 GID나 보조 그룹이 주인 그룹이면 그룹 칸, 아니면 나머지 칸): `Its mode lets this user write, so an ACL or a security policy such as SELinux blocks it.`
     - 아니고 폴더가 있으며 주인이 root(UID 0)일 때: `A folder owned by root is usually one Docker made because it did not exist when a container mounted it.`
   - 마지막 줄은 `fix: ` 뒤에 아래 글이다. 공식 container 안인지는 PRD 000 `CommandForm`과 같은 판정(공식 package 표시와 container 표시가 있고 위험 표시가 없음)이다. 이 판정에 config가 주는 것은 backend 뒤에 붙는 sandbox 이름뿐이라 config 없이 판정한다.
     - mode가 이미 허용할 때: 밖이면 `check the folder's ACL (ls -le on macOS, getfacl on Linux) and its security labels`, 안이면 `on the host, check the ACL and security labels of the folder mounted at <path>; with SELinux, add :z to that volume in docker-compose.yml`.
     - 기본 data 폴더(symlink 아님):

       | 경우 | container 밖 | 공식 container 안 |
       |---|---|---|
       | 있음, 주인이 실행 사용자 | `chmod u+rwx <path>` | `on the host, run chmod u+rwx ~/.shacs-bot` |
       | 있음, 주인이 root | `sudo chown -R <실행 uid>:<실행 gid> <path> && chmod u+rwx <path>` | `on the host, run sudo chown -R "$(id -u):$(id -g)" ~/.shacs-bot && chmod u+rwx ~/.shacs-bot, then ./scripts/docker-setup.sh in the shacs-bot repository; it sets SHACS_UID and SHACS_GID in the repository .env so the containers run as you` |
       | 있음, 다른 주인 | root와 같음 | `on the host, run ./scripts/docker-setup.sh in the shacs-bot repository as the user who owns ~/.shacs-bot (<주인 uid>:<주인 gid>), or set SHACS_UID=<주인 uid> and SHACS_GID=<주인 gid> in the repository .env; then run this again` |
       | 없음 | `sudo mkdir -p <path> && sudo chown <실행 uid>:<실행 gid> <path> && chmod u+rwx <path>` | `on the host, run ./scripts/docker-setup.sh in the shacs-bot repository; it creates ~/.shacs-bot as your user and sets SHACS_UID and SHACS_GID in the repository .env` |

     - 그 밖의 폴더(`--config`로 준 다른 폴더나 symlink): 주인을 바꾸는 명령을 알리지 않는다(`/etc` 같은 폴더를 바꾸게 하지 않기 위해). 밖이면 있을 때 `make <path 또는 link 대상> writable for <uid>:<gid>, or use --config with a path in a folder you own (the default data folder is ~/.shacs-bot)`, 없을 때 `create <path> for <uid>:<gid>, or use --config with a path in a folder you own (the default data folder is ~/.shacs-bot)`. 안이면 있을 때 `on the host, make the folder mounted at <path> writable for <uid>:<gid>, or run the containers as its owner with SHACS_UID and SHACS_GID in the repository .env`, 없을 때 `use --config with a path in a folder this container can write, such as the mounted data folder`.
   - 명령으로 알리는 경로는 shell에 그대로 붙여 쓸 수 있게, 글자·숫자와 `_-./:@%+=,` 밖의 글자가 있을 때만 작은따옴표로 감싼다(`'`는 `'\''`로).
   - container 안의 `~/.shacs-bot` 글은 공식 Compose가 host의 `~/.shacs-bot`을 기본 data 폴더에 mount한다고 보고 적는다.
5. **문서:** USAGE Docker Compose 절의 "Permission denied가 계속 나면 …" 문장을 이 동작으로 바꾼다(어느 명령이 확인하는지, 글이 알리는 것, container 안과 밖의 고치는 방법, 다른 폴더와 ACL·SELinux의 경우). README Docker 문단에 한 문장을 더한다.

## Non Scope

- 목록 밖 명령의 권한 오류, data 폴더 안 파일·하위 폴더의 다른 주인, data 폴더 밖 workspace, 읽기 전용 파일 시스템의 글.
- 주인을 대신 바꾸기(`sudo` 부르기), 관리자로 켜져 고치는 entrypoint.

## Parent Requirement Mapping

- [052 SPEC](../SPEC.md)의 소유 범위 7과 그 문서(소유 범위 8 중 권한 오류 부분), "반드시 지킬 것"의 "시작할 때의 쓰기 확인은 미리 알리는 것뿐이다"와 "`Cargo.lock`에 새 crate를 더하지 않는다", 수용 조건 8과 이 PRD 변경에 대한 수용 조건 9, 10을 소유한다.

## Required Contract

1. 확인은 범위 1의 명령에서만, 명령이 무엇을 읽거나 쓰기 전에 한 번 돈다.
2. 쓸 수 있는 data 폴더(없으면 만들 수 있는 위 폴더)에서는 명령의 동작과 출력이 지금과 같다.
3. 확인은 파일을 만들거나 바꾸지 않는다. `EACCES` 밖의 결과는 지금 동작으로 넘긴다.
4. 주인 바꾸기 명령(`chown -R`)은 symlink가 아닌 기본 data 폴더에만 알린다.
5. 글에는 경로, 번호, mode만 있다.
6. `Cargo.lock`에 새 crate가 없다(`libc`는 이미 `shacs-cli`의 의존이다).

## 구현

- `shacs-cli` `data_folder_access.rs`(새 파일):
  - `write_target(&CliCommand) -> Option<PathBuf>`: 범위 1의 목록. 확인할 config 경로를 돌려준다.
  - `check(config_path)`: 실제 `faccessat`, 실효 번호와 보조 그룹, `CommandForm::observed()`로 아래 두 함수를 부르고, 막히면 `CliError::DataFolder`.
  - `probe(data_folder, default_folder, running, groups, access)`: 범위 3. 묻기 함수를 받아 시험이 경로마다 답을 정한다. 막히면 `Denied`(data 폴더, 물은 폴더, 있음·없음, link 대상, 주인, mode, mode가 이미 허용하는지, 기본 폴더인지).
  - `render(&Denied, running, form) -> String`: 범위 4.
- `lib.rs`: `mod data_folder_access;`. `run_command`의 첫 줄에서 `write_target`이 있으면 `check`. `CliError::DataFolder`와 그 `Display`. `RuntimeContainmentEvidence::detect`를 config 없는 `detect_process`와 그 위의 sandbox 이름으로 나눈다(결과는 같다).
- `provider_status_cli.rs`: `CommandForm`과 `of`를 `pub(super)`로 열고, config 없이 판정하는 `CommandForm::observed()`를 더한다.

## Acceptance Criteria

시험은 `shacs-cli`에서 돌고 `Given`/`When`/`Then` 주석을 쓴다.

- 단위 시험(`data_folder_access_tests.rs`, `data_folder_access.rs`의 `#[cfg(test)]` module):
  1. 글 표: 기본 폴더에서 (있음·없음) × 주인(root, 다른 사용자, 실행 사용자) × (container 밖·안)의 첫 줄, 둘째 줄(root일 때만), `fix:` 줄이 범위 4와 같다. 주인 번호를 넣어 만든다(SPEC 수용 조건 8의 root 경우). 더해서 mode가 이미 허용하는 경우(밖·안, root 줄 대신 ACL 줄), 그 밖의 폴더(`/etc`처럼 root 소유인 다른 폴더: `chown`·`sudo`가 없음, 밖·안·없음), symlink(대상이 적히고 `chown`이 없음). 빈칸이 든 기본 폴더 경로는 작은따옴표로 감싸지고, `'`가 든 경로(`/home/o'neil/.shacs-bot`)는 `'/home/o'\''neil/.shacs-bot'`가 된다.
  2. 판정 표(임시 폴더, 경로마다 답을 정한 가짜 묻기, 넣은 실행 번호): 쓸 수 있음은 지나감. data 폴더 `EACCES`는 있음. data 폴더가 없고 위 폴더 `EACCES`는 없음(위 폴더). 두 단계 없음은 처음 있는 위 폴더. data 경로가 파일이면 지나감. `EROFS`는 지나감. 폴더를 가리키는 symlink는 대상으로 묻고 link를 적음. 대상 없는 symlink는 지나감. 상대 config 경로는 절대 경로로 알림. 주인 칸·그룹 칸·나머지 칸 각각에서 mode가 허용하는지(넣은 실효 번호와 보조 그룹으로).
  3. 목록 표: `parse_cli_args`로 만든 명령마다 확인하는지와 그 config 경로가 범위 1·2와 같다(기본 경로, 명령의 `--config`, 앞에 둔 `--config`). 확인하지 않는 명령: `provider status`, `channels connect discord --status`, `runtime config-migrate`·`runtime migrate`의 dry run, `runtime inspect`, `status`, `channels list`, `memory search`, `session create`, `plugins list`, `permissions list`, `tasks`의 `goal` action(`--workspace` 없이도), `--workspace`를 준 `tasks`의 `recovery` action, `--data-dir`만 준 `tasks`의 `recovery` action, `--help`. 확인하는 쪽에는 `--workspace`와 `--data-dir`이 없는 `tasks`의 `recovery` action이 있다.
  - 실효 번호와 실제 번호가 다른 경우(setuid)는 시험 process로 만들 수 없어 코드로만 본다. 시험 process에서는 둘이 같다.
- binary 시험(`tests/it/spec052_data_folder_access.rs`, 새 module을 `tests/it/main.rs`에 등록. `env_clear` 뒤 `HOME`·`PATH`만, stdin 없음):
  4. 쓸 수 없는(0555) 기본 data 폴더에서 범위 1의 명령마다(확인이 빠지면 네트워크로 가는 `provider codex login`과 `memory model install`은 목록 표로만 본다) 종료 코드 1, 첫 줄이 `cannot write to the data folder <path>`이고 주인·실행 번호와 `fix: chmod u+rwx`가 있으며 `os error 13`이 없다. `--config`로 준 다른 쓸 수 없는 폴더에서는 `chown`이 없는 글이다. 확인하지 않는 읽기 명령 둘(`provider status`, `channels connect discord --status`)은 같은 폴더에서 지금처럼 종료 코드 0이다.
  5. 기본 data 폴더가 없고 그 위(`HOME`)가 0555면 `onboard`가 `cannot create the data folder`와 `sudo mkdir -p`로 끝난다.
  6. 쓸 수 있는 폴더에서 `onboard`가 지금처럼 종료 코드 0이고 config를 만든다.
  7. 시험 process가 root면(root는 권한 확인을 지나가므로) 임시 폴더 전체를 1001:1001로 바꾸고 binary를 그 안에 복사해 `CommandExt::uid(1001)`·`gid(1001)`로 돌린다. 명령마다 정한 시간(60초) 안에 출력이 끝나지 않으면 죽이고 기다린 뒤 실패로 본다(확인이 돌지 않아 서버가 뜬 경우). 0555로 만든 폴더는 `Drop`에서 권한을 되돌려 실패해도 임시 폴더가 지워진다.
- 8. 문서가 범위 5와 같다.
- 9. 검증은 순서대로: Google 옵션을 붙인 `cargo fmt --all -- --check`, workspace `cargo clippy --all-targets -- -D warnings`, `cargo test --manifest-path crates/Cargo.toml -p shacs-cli -q`. `Cargo.lock`이 바뀌지 않는다. 전체 workspace 시험은 사용자 승인 없이 돌리지 않는다.

## Closure Evidence

1. 새 파일과 바뀐 파일, 고친 기존 시험과 까닭.
2. binary로 본 글 예(container 밖, 있음·없음).
3. Cargo 검증 기록과 `Cargo.lock` 확인.

## 남은 제한

- 목록 밖 명령과 data 폴더 안의 다른 주인 파일·하위 폴더는 지금처럼 원시 오류로 끝난다. 예를 들어 빈 data 폴더에 쓸 수 없으면 `runtime inspect`는 지금처럼 `CLI I/O failed: Permission denied`로 끝난다(읽기 명령이라 목록 밖이고, 채워진 폴더에서는 돈다). workspace를 쓰는 명령도 workspace를 미리 보지 않는다.
- 확인은 미리 알리는 것뿐이다. 확인을 지난 뒤의 쓰기 실패(바뀜, NFS root squash 등)는 지금 글 그대로다.
- container 안의 고치는 방법은 공식 Compose의 mount(`~/.shacs-bot`)를 전제로 적는다. mount를 바꿨으면 그 host 폴더로 읽어야 한다.
- 고칠 것 없는 `runtime config-migrate --apply`와 되돌릴 것 없는 `permissions auto-mode reset`도 쓸 수 없는 폴더에서는 이제 이 글로 멈춘다(쓰는 명령이므로).
- 보조 그룹 목록을 읽지 못하면(실제로는 생기지 않는다) mode 칸 판정이 어림이 되므로, 기본 data 폴더라도 주인 바꾸기 명령을 알리지 않는다.

## 구현 메모 및 검증 근거 (2026-11-09)

실행 기록은 로컬 `.omo/evidence/spec052/`(추적하지 않음)의 `plan-review.md`와 `prd030/`에 있다.

- 구현:
  - `shacs-cli` `data_folder_access.rs`(새 파일, unix에서만): `write_target`(범위 1의 목록, 32갈래), `check`, `data_folder`(runtime의 `config_context`로 정한 data 폴더를 절대 경로로. 명령의 config와 기본 config에 같이 씀), `probe`, `first_existing_folder`, `mode_allows`, `effective_access`(`faccessat` + `AT_EACCESS`), `supplementary_groups`·`read_groups`(`getgroups`에 언제나 한 칸 이상을 주고, 목록이 그 사이 늘면 세 번까지 다시 읽음. 끝내 못 읽으면 `None`이고 주인 바꾸기 명령을 알리지 않음), `render`·`fix`·`other_folder_fix`, `shell_quote`.
  - `lib.rs`: `#[cfg(unix)] mod data_folder_access;`, `CliError::DataFolder`(글 그대로), `run_command` 첫머리의 확인(`#[cfg(unix)]`), `RuntimeContainmentEvidence::detect_process`(config 없는 증거. `detect`는 그 위에 sandbox 이름만 얹음).
  - `provider_status_cli.rs`: `CommandForm`·`of`를 `pub(super)`로, `CommandForm::observed()`(unix).
  - 문서: USAGE "설정 스크립트로 처음 실행"의 새 항목(확인하는 명령, 글이 알리는 것, container 안의 고치는 방법), Docker provider secret 문단의 "Permission denied가 계속 나면" 문장을 이 동작(기본 폴더에만 주인 바꾸기 명령, ACL·SELinux와 `:z`, 확인 뒤의 실패는 원래 오류)으로. README Docker 문단에 한 문장.
- 시험:
  - `shacs-cli` 단위 시험 `data_folder_access_tests.rs` 6개: 글 표 20행과 빈칸 경로, mode 칸 표 7행, `getgroups` 다시 읽기 4가지(빈 목록이 둘로 늘어남, 칸보다 많다는 답, 계속 늘어남, `EINVAL` 밖의 오류), 판정(임시 폴더와 가짜 묻기: 쓸 수 있음, 있음, 기본 폴더 아님, 없음 두 가지, 파일, `EROFS`, 대상 없는 link, 폴더 link, 상대 경로), 상대 config 경로의 data 폴더 3가지, 목록 표(확인 31갈래, `--config` 3가지, 확인 안 함 15갈래).
  - `shacs-cli` 통합 시험 `tests/it/spec052_data_folder_access.rs`(새 module, `#[cfg(unix)]`) 4개: 0555 기본 data 폴더에서 명령 29개의 첫 줄·`fix: chmod u+rwx`·`os error 13` 없음과 읽기 명령 둘의 종료 코드 0, `--config` 폴더의 `chown`·`sudo` 없는 글, 0555 `HOME` 아래 없는 data 폴더의 `cannot create`와 `sudo mkdir -p`, 쓸 수 있는 폴더의 `onboard`. 시험 process가 root면 binary를 복사해 1001로 돈다. 이 컴퓨터(macOS, 일반 사용자)에서는 그 갈래가 돌지 않았다.
  - PRD 020의 `spec052_setup_script` module 등록에도 `#[cfg(unix)]`를 붙였다(같은 까닭).
  - 고친 기존 시험: 없다.
- 값이 없는 글 예(debug binary, 임시 HOME, container 밖):

  ```text
  cannot write to the data folder <HOME>/.shacs-bot: it belongs to 501:20 (mode 0555) and this command runs as 501:20
  fix: chmod u+rwx <HOME>/.shacs-bot
  ```

  ```text
  cannot create the data folder <HOME>/.shacs-bot: <HOME> belongs to 501:20 (mode 0555) and this command runs as 501:20
  fix: sudo mkdir -p <HOME>/.shacs-bot && sudo chown 501:20 <HOME>/.shacs-bot && chmod u+rwx <HOME>/.shacs-bot
  ```

  같은 상태에서 `provider status`는 지금처럼 돈다.
- 검증: Google 옵션을 붙인 `cargo fmt --all -- --check`, workspace `cargo clippy --all-targets -- -D warnings` 통과. 마지막 소스로 `shacs-cli` 553 passed / 0 failed / 3 ignored(로컬 `prd030/cli-tests.log`, PRD 020 시험 포함). `crates/Cargo.lock`은 바뀌지 않았다.
- Oracle 독립 검토: 계획 4차 **PASS**(1차: 실제 번호로 묻는 `access`, `/etc` 같은 폴더에 `chown -R`을 알림, 빠진 쓰는 명령, symlink, 시험 빈틈, 시험 등록·BDD·검증 순서. 2차: `tasks`의 goal action은 workspace를 씀, `'` 따옴표 시험. 3차: `--data-dir`만 준 `tasks`). 구현 3차 **PASS**(1차: 없는 폴더 행과 상대 config 시험, `getgroups` 경쟁, BDD 주석, USAGE의 `tasks`, unix 전용 module 등록. 2차: 0개로 센 뒤 늘어난 목록).
- 종료 확인은 PRD 020이 닫힌 뒤에 한다(Depends on).

## 종료 확인 (2026-11-09)

- PRD 020이 같은 날 닫혔다(그 종료 확인).
- 전체 workspace 시험: 사용자 승인(2026-11-09)을 받아 PRD 020·030·040을 담은 작업 트리에서 `cargo test --manifest-path crates/Cargo.toml --workspace --locked --no-fail-fast`를 돌렸다. suite 211개, 3,752 passed / 0 failed / 8 ignored, exit 0이다(로컬 `.omo/evidence/spec052/closure/workspace-tests-prd020-040.log`). 커밋 `738061c`.
- PRD 020의 실제 Docker 확인(서버 ka1rm31, UID 1001)은 이 확인이 든 image로 돌았다. 쓸 수 있는 data 폴더에서 `onboard`, `provider status`, `channels connect discord --status`, gateway가 지금처럼 돌았다(Required Contract 2). 쓸 수 없는 폴더의 글은 Cargo 시험으로만 확인했다.
- 수용 조건 1–9를 위 시험과 검증으로 맞췄고, 계획 4차·구현 3차 독립 검토가 PASS다. PRD 030은 `Complete (Scoped)`로 닫는다. 전체 출시 seal이 아니다.
- 남은 제한은 위 "남은 제한"과 같다.
