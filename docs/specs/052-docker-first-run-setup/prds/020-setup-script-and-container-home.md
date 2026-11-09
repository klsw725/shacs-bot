# PRD 020. 처음 설정 스크립트와 컨테이너 HOME

Status: Planned

Depends on: [PRD 000](000-provider-status-and-gateway-hint.md)(`provider status`, gateway 안내)과 [PRD 010](010-onboard-and-discord-connect.md)(migration 중의 `onboard`, `channels connect discord`)의 닫힌 결과. 뒤 번호 PRD에 의존하지 않는다.

## Goal

저장소를 받은 사용자가 `./scripts/docker-setup.sh`를 한 번 실행하면, data 폴더와 사용자 번호를 맞추고 image를 만들고 처음 설정·ChatGPT 계정 로그인·Discord 연결을 물어 한 뒤 gateway를 띄운다. 공식 image의 HOME은 어느 사용자 번호로 돌아도 쓸 수 있게 둔다. README와 USAGE의 Docker 설명을 이 스크립트 중심으로 바꾼다.

## Scope

1. **스크립트 위치와 실행 환경:** `scripts/docker-setup.sh`(실행 권한 있음). POSIX `sh`만 쓴다(`local`, `[[ ]]`, 배열, `pipefail` 없음, `jq`·`python` 없음). macOS의 `/bin/sh`(bash 3.2 POSIX mode)와 Debian `dash`에서 같게 돈다. 출력은 영어다. `set -u`를 쓰고, 실패는 명령마다 직접 본다(`set -e` 없음).
   - 저장소 루트는 스크립트 폴더의 부모다(`CDPATH= cd -- "$(dirname -- "$0")/.." && pwd`). 어느 폴더에서 불러도 루트로 옮겨 그 `docker-compose.yml`을 쓴다.
2. **순서와 멈춤:** 아래 순서로 한다. 1–5와 7–8은 실패하면 그 출력을 보이고 뒤 명령 없이 종료 코드 1로 멈춘다. 6의 로그인과 Discord 연결은 실패해도 그 출력을 보이고 다음으로 간다.
   1. **실행 사용자:** `id -u`가 0이면 아무것도 바꾸지 않고 멈춘다(`.env`에 0이 적혀 컨테이너가 관리자로 돌게 되므로).
   2. **도구:** `docker`가 없으면(`command -v docker`), Compose v2가 없으면(`docker compose version`) 그 사실을 알리고 멈춘다.
   3. **data 폴더:** `$HOME/.shacs-bot`(Compose가 mount하는 `~/.shacs-bot`). 있는데 폴더가 아니거나 실행한 사용자가 쓸 수 없으면(`[ -w ]`) 아무것도 바꾸지 않고, 주인 번호(`ls -ldn`)와 고치는 명령 `sudo chown -R "$(id -u):$(id -g)" ~/.shacs-bot`을 알리고 멈춘다. 없으면 `mkdir -p`로 만든다. 스크립트는 `sudo`를 부르지 않는다.
   4. **사용자 번호:** 저장소 루트 `.env`에 `SHACS_UID=<id -u>`, `SHACS_GID=<id -g>`를 둔다.
      - `.env`가 없으면 `umask 077`로 0600 파일을 만들어 두 줄만 쓴다.
      - 있으면 같은 폴더에 고유한 임시 파일(`mktemp "$root/.env.docker-setup.XXXXXX"`)을 만들고 `cp -p`로 `.env`의 내용과 권한을 옮긴 뒤 새 내용을 그 안에 쓰고 `mv`로 바꾼다. `SHACS_UID=`·`SHACS_GID=`로 시작하는 첫 줄은 새 값으로 바꾸고 같은 이름의 뒤 줄은 지운다. 그 밖의 줄(주석 `# SHACS_UID=…` 포함)은 순서와 내용을 그대로 둔다. 없던 이름은 끝에 더한다. 값이 바뀌면 옛 값과 새 값을 알린다. 두 값이 이미 맞고 중복 줄도 없으면 임시 파일을 만들지 않고 쓰지 않는다.
      - 임시 파일은 정상 끝, 오류, INT·TERM 어느 경로에서도 지운다(범위 4의 정리 함수). 강제로 끝나 남을 수 있으므로 `.gitignore`에 `.env.docker-setup.*`를 더한다.
      - 실행한 shell이 다른 값의 `SHACS_UID`나 `SHACS_GID`를 내보내고 있으면(Compose는 그 값을 `.env`보다 먼저 쓴다) 지우라고 알린다. 스크립트는 맞는 두 값을 `export`해 이 스크립트의 Compose 명령이 맞는 값을 받게 한다.
   5. **image와 처음 설정:** `docker compose build`, 그다음 `docker compose run --rm -T shacs-cli onboard`. 끊긴 migration이면 `onboard`가 멈추므로(PRD 010) 스크립트도 그 출력과 함께 멈춘다.
   6. **provider와 Discord:**
      - `docker compose run --rm -T shacs-cli provider status`의 출력 첫 줄이 `provider: ready`면 묻지 않는다. 아니면 출력을 보이고, terminal이면 고르게 한다: `1) Sign in with a ChatGPT account (Codex)`, `2) Skip for now`. 1이면 `docker compose run --rm -T shacs-cli provider codex login --headless`를 하고, 성공하면 `provider status`를 다시 보아 아직 `ready`가 아니면 그 출력을 보인다. 2나 입력 끝이면 나중에 할 명령을 알린다. API key provider는 USAGE의 손 설정을 가리킨다.
      - `docker compose run --rm -T shacs-cli channels connect discord --status`의 첫 줄이 `discord: connected`면 묻지 않는다. 아니면 terminal이면 `Connect a Discord bot now? [y/N]`를 묻는다. `y`면 토큰을 숨긴 입력으로, 사용자 ID를 보이는 입력으로 받는다. 사용자 ID를 비우면 "봇이 볼 수 있는 누구나 봇을 쓰고 gateway는 명령 실행·파일 쓰기 도구를 켠 채 돈다"를 알리고 `Continue without a user ID? [y/N]`를 묻는다. `y`가 아니면 사용자 ID를 다시 묻는다. 그다음 `printf '%s\n' "$token" | docker compose run --rm -T shacs-cli channels connect discord --token-stdin [--allow-from <id>]`로 넘기고 `token` 변수를 비운다. 거절이나 입력 끝이면 나중에 할 명령을 알린다. 다른 channel은 USAGE의 손 설정을 가리킨다.
      - 두 확인 명령(`provider status`, `--status`)이 0이 아닌 종료 코드면 멈춘다.
   7. **시작:** `docker compose ps --status running --services`로 `shacs-gateway`가 돌고 있는지 먼저 보고(이 명령이 실패하면 멈춘다), `docker compose up -d shacs-gateway`를 한다. 돌고 있었고 6에서 로그인이나 Discord 연결이 성공했으면 `docker compose restart shacs-gateway`를 한다.
   8. **끝:** 로그 보기(`docker compose logs -f shacs-gateway`)와 상태 확인(`docker compose run --rm shacs-cli provider status`, `… runtime inspect`) 명령, 그리고 하지 않은 일(건너뛴 로그인, 연결하지 않은 Discord)의 명령을 알린다.
3. **terminal이 아닐 때:** stdin이 terminal이 아니면(`[ -t 0 ]`가 거짓) 아무것도 묻지 않는다. 1–5, 두 확인, 7–8만 하고, 로그인과 Discord 연결은 할 일로 알린다.
4. **정리, 비밀 값과 terminal:** 스크립트 처음에 정리 함수 하나를 `trap`으로 건다: EXIT에서 정리, INT에서 정리 뒤 130, TERM에서 정리 뒤 143으로 끝낸다. 정리는 남은 `.env` 임시 파일을 지우고, 입력 표시를 껐던 중이면 `stty echo`와 줄바꿈으로 되돌린다.
   - 토큰은 `stty -echo`(꺼짐 표시를 먼저 켬)로 숨긴 `read -r`로 shell 변수에 받고, 읽은 뒤 `stty echo`로 되돌리고 표시를 끈다.
   - 토큰은 `printf`(shell 내장)로 `docker compose run -T`의 stdin에만 넘기고, 출력, `.env`, 명령줄 인자, `export`한 변수에 두지 않는다. 넘긴 뒤 변수를 비운다.
5. **Compose 호출:** 컨테이너 명령은 모두 `docker compose run --rm -T shacs-cli …`다(pseudo-TTY 없이 stdin을 그대로 넘기고, 읽는 출력에 terminal 글자가 섞이지 않게). 명령을 다시 실행해도 결과가 같다.
6. **컨테이너 HOME:** `Dockerfile` runtime 단계의 사용자 만들기 뒤에 `chmod 1777 /home/shacs`를 더한다. 주인(1000), `USER shacs`, `ENV HOME`, `SHACS_RUNTIME_PACKAGE`, data 폴더 mount 자리는 그대로다.
7. **실제 Docker 확인 스크립트(수용 조건 6):** opt-in `docs/scripts/spec052-docker-setup-smoke.sh`(bash 가능, 023 smoke와 같은 위치·방식).
   - 시작 전에 `shacs-gateway` 컨테이너가 있으면 아무것도 하지 않고 멈춘다. image 이름 `shacs-bot:local`을 저장소 사본에서 다시 만든다고 알리고 `--yes` 없이는 멈춘다.
   - `git worktree add --detach`로 임시 폴더에 지금 commit의 사본을 만들고, 임시 HOME(빈 data 폴더)으로 그 사본의 `scripts/docker-setup.sh`를 stdin 없이(`</dev/null`) 돌린다. HOME을 바꾸면 Docker CLI가 설정·context·credential을 임시 HOME에서 찾으므로, 원래 HOME을 기억해 `DOCKER_CONFIG="${DOCKER_CONFIG:-$원래_HOME/.docker}"`를 내보낸 채 `HOME`만 바꾼다. 사전 확인, setup, 확인, 정리의 모든 `docker`·Compose 호출이 같은 `HOME`·`DOCKER_CONFIG`를 쓴다.
   - 확인: 종료 코드 0, `docker compose ps`에 `shacs-gateway`가 running, data 폴더 파일의 주인이 실행한 사용자, `docker compose run --rm -T --entrypoint /bin/sh shacs-cli -c 'mkdir -p "$HOME/.cache/spec052" && echo ok'`이 성공(image의 entrypoint가 `shacs-bot`이라 shell로 바꾼다), `runtime inspect` 출력에 official-container 증거, gateway 로그에 `provider: missing` 줄.
   - 끝나면(실패해도) 사본에서 `docker compose down`, `git worktree remove --force`, 임시 폴더를 지운다. 사용자의 `~/.shacs-bot`과 저장소의 `.env`는 쓰지 않는다.
   - Cargo 시험이 아니며, 사용자 승인 뒤 UID가 1000이 아닌 사용자로 돌린다.
8. **문서:** README의 Docker 문단과 `docs/USAGE.md` Docker Compose 절을 스크립트 중심으로 바꾼다. 처음 실행은 `git clone` → `./scripts/docker-setup.sh`, 질문과 그 뜻, 다시 실행해도 된다는 것, 끝난 뒤 로그·상태 명령. `SHACS_UID`·`SHACS_GID`와 저장소 `.env`의 뜻, Compose를 스크립트보다 먼저 돌려 data 폴더가 `root`로 생긴 경우와 고치는 법, root로 실행하지 않는 까닭, rootless Docker·Podman·Docker Desktop·Windows(WSL) 안내, 컨테이너 HOME이 1777이라 HOME 권한을 엄격히 보는 도구가 경고할 수 있다는 것. 지금의 손 순서는 "스크립트 없이" 절로 남긴다(PRD 000·010이 더한 `provider status`, `channels connect discord`, migration 중의 `onboard` 포함).
   - README와 USAGE 각각에 Docker에서 API key provider를 쓰는 세 길(Compose `.env`의 provider 환경 변수, config의 provider 항목, key를 stdin으로만 넘기는 `printf '%s\n' "$PROVIDER_KEY" | docker compose run --rm -T shacs-cli provider <id> import-key --token-stdin`. `-T` 없이 terminal에 붙이면 `import-key`는 입력을 숨기지 않아 붙여넣은 key가 화면에 보인다)과, 지금(PRD 040 전)은 Compose가 넘기는 빈 provider 환경 변수가 뒤의 두 길을 가린다는 것, `provider status`로 확인한다는 것을 적는다. 이 경고는 PRD 040이 고친 동작으로 바꾼다.

## Non Scope

- 권한 오류의 알아듣기 쉬운 글(PRD 030), 빈 provider 환경 변수와 fingerprint(PRD 040).
- API key provider 설정 질문, 다른 channel 연결, Telegram 보낸 사람 제한.
- 게시된 image, 관리자로 켜지는 entrypoint, data 폴더 주인 바꾸기.

## Parent Requirement Mapping

- [052 SPEC](../SPEC.md)의 소유 범위 5, 6과 소유 범위 8 중 Docker 설명 문서, "반드시 지킬 것"의 root·`sudo`·비밀 값·`.env`·다시 실행·가짜 `docker` 시험 항목, 수용 조건 5, 6, 7과 이 PRD 변경에 대한 수용 조건 9, 10을 소유한다.

## Required Contract

1. 스크립트는 root로 돌지 않고 `sudo`를 부르지 않으며 data 폴더 주인을 바꾸지 않는다.
2. `.env`에는 `SHACS_UID`·`SHACS_GID` 두 줄만 쓰고 다른 줄과 권한을 지킨다. 새 `.env`는 0600이다.
3. 토큰은 컨테이너 명령의 stdin에만 간다. 명령줄 인자, 환경 변수, 출력, `.env`에 없다.
4. 숨긴 입력이 어떻게 끝나도 terminal의 입력 표시가 돌아온다.
5. 같은 상태에서 두 번 실행하면 같은 결과다.
6. Cargo 시험은 사용자의 data 폴더, 실제 저장소의 `.env`, 실제 `docker`를 쓰지 않는다. `Cargo.lock`에 새 crate를 더하지 않는다.

## Acceptance Criteria

시험은 `shacs-cli` `tests/it/spec052_setup_script.rs`(새 module, `tests/it/main.rs`에 등록)에서 돈다. 시험마다 임시 폴더에 저장소 꼴(`scripts/docker-setup.sh`와 `docker-compose.yml` 사본)과 임시 HOME을 만들고, 시험이 쓴 가짜 `docker`(호출마다 인자 한 줄, `SHACS_UID`·`SHACS_GID`와 전체 환경, 토큰 명령의 stdin을 기록하고 정한 출력·종료 코드를 돌려준다)를 PATH 맨 앞에 두며, `env_clear()` 뒤 `HOME`·`PATH`만 넣고 `/bin/sh`로 스크립트를 돌린다. 실제 저장소의 `.env`가 시험 전후 같은지 본다.

- 기대하는 번호는 스크립트를 돌린 process의 실제 번호다. 시험 process가 root면(`geteuid() == 0`) 임시 폴더 전체를 1001:1001로 바꾸고 스크립트를 `CommandExt::uid(1001)`·`gid(1001)`로 돌려, 권한 확인(`[ -w ]`)과 만든 폴더의 주인이 실제 일반 사용자의 것이 되게 한다. 아니면 그 사용자의 번호 그대로 돌린다.
- 가짜 `id`(0을 돌려준다)는 root 거절 시험에만, 실패하는 가짜 `mv`는 임시 파일 정리 시험에만 PATH에 더한다.

1. 처음 실행(terminal 아님): 없던 data 폴더가 그 사용자 소유로 생기고, 새 `.env`(0600)에 그 사용자의 `SHACS_UID`·`SHACS_GID`가 있다. 가짜 `docker`가 받은 명령이 `compose version` → `compose build` → `onboard` → `provider status` → `channels connect discord --status` → `compose ps` → `compose up -d shacs-gateway` 순서이고, 컨테이너 명령마다 `run --rm -T shacs-cli`다. 질문이 없고 로그인·Discord 연결 명령을 할 일로 알리며 종료 코드 0이다. 같은 상태로 다시 돌리면 `.env`의 inode와 byte가 같고(쓰지 않음) 명령 순서가 같으며, 임시 파일이 남지 않는다.
2. `.env` 지키기(표 하나): 다른 줄·주석과 다른 `SHACS_UID`, 0640 권한이 있던 `.env`에서 다른 줄과 순서와 권한이 그대로이고 두 값만 바뀌며 바꾼다는 글이 있다. `SHACS_UID`가 두 줄이면 첫 줄만 새 값이 되고 뒤 줄은 없어진다. 끝나면 `.env.docker-setup.*`가 남지 않는다. 실패하는 가짜 `mv`로 바꾸기가 실패하면 `.env`가 byte 단위로 그대로이고, 종료 코드 1이며, `.env.docker-setup.*`가 남지 않고 `docker compose build`가 불리지 않는다.
3. 내보낸 번호: shell이 다른 `SHACS_UID`·`SHACS_GID`를 내보내도 가짜 `docker`가 받은 두 값이 실행한 사용자의 것이고, 지우라는 글이 있다.
4. 멈춤: 가짜 `id -u`가 0을 돌려주면(root) data 폴더·`.env`가 생기지 않고 `docker`가 불리지 않으며 종료 코드 1이다. 쓸 수 없는 data 폴더(0555)와 폴더가 아닌 `~/.shacs-bot`(파일)도 `.env`가 생기지 않고 `compose build`가 불리지 않으며, 쓸 수 없는 폴더의 글에 주인 번호와 `sudo chown -R`이 있다. `docker`가 PATH에 없거나 `compose version`이 실패해도 멈춘다.
5. 실패: `compose build`, `onboard`, `provider status`, `--status`, `compose ps`, `up -d` 각각이 실패하면 그 뒤 명령이 없고 종료 코드 1이다. 로그인이나 Discord 연결이 실패하면 `up -d`까지 간다.
6. restart: gateway가 돌고 있었고 로그인이나 연결이 성공했을 때만 `compose restart shacs-gateway`가 더해진다. 돌고 있었지만 둘 다 하지 않았거나 실패했을 때, 돌고 있지 않았을 때는 없다.
7. 질문(`libc::openpty`로 만든 terminal을 stdin으로, 출력은 pipe로 받아 질문을 기다린 뒤 답을 씀):
   - `provider status`가 `ready`이고 Discord가 `connected`면 질문 없이 끝난다.
   - 로그인 고르기는 `provider codex login --headless`를 부르고, 건너뛰기는 부르지 않고 할 일을 알린다.
   - Discord 거절은 연결 명령을 부르지 않는다.
   - 빈 사용자 ID를 확인하지 않으면 다시 묻고, 다음에 준 ID가 `--allow-from`으로 간다. 빈 ID를 확인하면 `--allow-from` 없이 연결한다.
   - 토큰은 terminal(master 쪽)에 표시되지 않고, 가짜 `docker`의 stdin 기록에만 있으며 인자·환경 기록과 스크립트 출력에 없다.
   - 토큰을 숨겨 받는 중에 SIGINT를 받으면 종료 코드 130, SIGTERM이면 143으로 끝나고, 두 경우 모두 terminal의 ECHO가 다시 켜져 있다.
8. image: 기존 시험 `dockerfile_runtime_stage_uses_non_root_user`에 `chmod 1777 /home/shacs`가 있는지 더한다(`USER shacs`, `SHACS_RUNTIME_PACKAGE`는 그 시험이 이미 본다).
9. 실제 Docker(수용 조건 6): 범위 7의 스크립트를 사용자 승인 뒤 UID가 1000이 아닌 사용자로 돌려 그 확인이 모두 통과한다. 사용자가 돌리지 않기로 하면 종료 확인에 그 결정과 확인하지 못한 것을 적고 scoped로 닫는다.
10. 문서가 범위 8과 같다.
11. fmt(Google 옵션), workspace clippy `-D warnings`, `shacs-cli` 시험이 통과한다. `Cargo.lock`이 바뀌지 않는다.

## Closure Evidence

1. 스크립트, Dockerfile, smoke 스크립트, 문서와 시험.
2. 고친 기존 시험과 까닭.
3. 가짜 `docker`로 돌린 처음 실행 출력 예(값 없음).
4. Cargo 검증 기록과 `Cargo.lock` 확인, 실제 Docker 확인 기록 또는 돌리지 않은 결정.

PRD 020은 PRD 030, 040 없이 닫힌다.

## 남은 제한

- 스크립트 없이 `docker compose up -d`만 하면 UID가 1000이 아닌 컴퓨터에서는 지금처럼 쓰기에 실패한다(SPEC 남은 위험). 알기 쉬운 글은 PRD 030이다.
- 사용자 번호를 `/etc/passwd`에 없는 번호로 돌리므로, 사용자 이름을 찾는 도구는 이름 없이 돌거나 경고할 수 있다.
- 컨테이너 HOME을 누구나 쓸 수 있게(1777) 두므로 HOME 권한을 엄격히 보는 도구가 경고할 수 있다(SPEC 남은 위험).
- 숨긴 입력은 SIGKILL처럼 잡을 수 없는 신호로 끝나면 입력 표시가 꺼진 채 남을 수 있다(`stty echo`로 되돌린다).
- `docker compose build`는 처음에 몇 분 걸리고 메모리를 쓴다.

## 구현 메모 및 검증 근거 (2026-11-09)

실행 기록은 로컬 `.omo/evidence/spec052/`(추적하지 않음)의 `plan-review.md`와 `prd020/`에 있다.

- 구현:
  - `scripts/docker-setup.sh`(새 파일, 0755, POSIX `sh`): 범위 1–5를 순서대로 한다. 처음에 정리 함수와 trap(EXIT, INT 130, TERM 143)을 걸고, root 거절과 shell이 내보낸 번호 알림·`export`, `docker`·Compose v2 확인, data 폴더 확인·만들기, `.env`(먼저 판정 → `mktemp`·`cp -p`·다시 쓰기·`mv`, 새 파일 0600, 이미 맞으면 쓰지 않음), `build`·`onboard`, `provider status`와 ChatGPT 로그인 고르기·다시 확인, `channels connect discord --status`와 연결 질문(입력 표시를 먼저 끈 숨긴 토큰, 사용자 ID와 "누구나" 확인, 토큰은 내장 `printf` pipe로만), `compose ps` → `up -d` → 조건부 `restart`, 끝 안내와 할 일.
  - 숨긴 입력이 실패할 때: `stty -echo`가 실패하면 토큰을 묻지 않는다. `stty echo`가 실패하면 토큰을 버리고, 정리 함수가 끝날 때 다시 켠다. 사용자 ID나 "누구나" 질문에서 입력이 끝나면 연결을 그만둔다. 어느 경우든 `The Discord connection did not finish.`를 보이고 gateway 시작으로 간다.
  - `Dockerfile`: runtime 단계의 `chown` 뒤 `chmod 1777 /home/shacs`. `.gitignore`: `.env.docker-setup.*`.
  - `docs/scripts/spec052-docker-setup-smoke.sh`(새 파일, 0755, bash): 범위 7. `--yes`, root·UID 1000 거절, shell의 `SHACS_UID`·`SHACS_GID`를 지워 확인이 스크립트가 쓴 `.env`를 읽게 함, `DOCKER_CONFIG` 고정, 무엇을 만들기 전에 거는 정리 trap(worktree를 만든 뒤에만 `compose down`·`worktree remove`), 임시 HOME으로 하는 사전 확인. 명령 출력은 먼저 받아 둔 뒤 찾는다(`pipefail`에서 `grep -q`가 일찍 끝나 앞 명령이 SIGPIPE로 실패하는 일을 막음).
  - 문서: README Docker 문단(스크립트와 하는 일, root·`sudo`·`chown`, smoke 줄, API key 세 길과 PRD 040 전 경고, HOME 1777), USAGE Docker Compose 절("설정 스크립트로 처음 실행", "스크립트 없이", smoke 문단, API key 세 길, Permission denied 안내).
- 시험:
  - `shacs-cli` 통합 시험 `tests/it/spec052_setup_script.rs`(새 module) 9개: 처음 실행과 다시 실행(명령 순서, 번호, 0600, inode·byte 같음, 임시 파일 없음, 실제 저장소 `.env` 그대로), `.env` 지키기 표와 실패하는 `mv`, 내보낸 번호, 멈춤(root, 0555 폴더, 파일, `docker` 없음, Compose 없음), 필수 단계 실패 6가지, 질문 없음(ready·connected), 질문 표 10행(로그인·건너뛰기·실패, 로그인 뒤 ready, 빈 ID 다시 묻기·확인, 연결 실패, 토큰·사용자 ID·"누구나"에서 입력 끝. 행마다 restart와 할 일 줄을 봄), 실패하는 `stty` 두 경우, 숨긴 토큰 중 SIGINT 130·SIGTERM 143과 ECHO.
  - 시험 process가 root면 임시 폴더를 1001로 바꾸고 스크립트를 1001로 돌린다. 이 컴퓨터(macOS, 일반 사용자)에서는 그 갈래가 돌지 않았다.
  - 고친 기존 시험: `lib.rs`의 `dockerfile_runtime_stage_uses_non_root_user`에 `chmod 1777 /home/shacs` 확인을 더했다(수용 조건 8). 원래 확인은 그대로다.
- 값이 없는 처음 실행 출력 예(가짜 `docker`, terminal 아님):

  ```text
  Created <HOME>/.shacs-bot
  Wrote SHACS_UID=<uid> and SHACS_GID=<gid> to .env
  Building the image (the first build takes a few minutes)...
  Running the first setup...
  provider: missing openai_codex
  reason: no saved ChatGPT sign-in
  next: docker compose run --rm shacs-cli provider codex login --headless
  discord: not connected
  reason: channels.discord.enabled is not true
  Starting the gateway...

  The gateway is running.
    Logs:            docker compose logs -f shacs-gateway
    Provider status: docker compose run --rm shacs-cli provider status
    Runtime status:  docker compose run --rm shacs-cli runtime inspect
  Still to do (then restart the gateway: docker compose restart shacs-gateway):
    Sign in:         docker compose run --rm shacs-cli provider codex login --headless
    Connect Discord: docker compose run --rm shacs-cli channels connect discord --token-stdin --allow-from YOUR_DISCORD_USER_ID
    API key providers and other channels: see docs/USAGE.md (Docker Compose)
  ```

  `provider`·`discord` 줄은 가짜 `docker`가 돌려준 글이다.
- 검증: 스크립트 문법 `sh -n`·`bash --posix -n`·`dash -n`, smoke `bash -n`. Google 옵션을 붙인 `cargo fmt --all -- --check`, workspace `cargo clippy --all-targets -- -D warnings` 통과. 마지막 소스로 `shacs-cli` 543 passed / 0 failed / 3 ignored(로컬 `prd020/cli-tests.log`). `crates/Cargo.lock`은 바뀌지 않았다.
- 실제 Docker 확인(수용 조건 9): 아직 돌리지 않았다. smoke는 지금 commit의 사본을 쓰므로 커밋한 뒤, 사용자 승인을 받아 UID가 1000이 아닌 일반 사용자로 돌린다.
- Oracle 독립 검토: 계획 4차 **PASS**. 구현 3차 **PASS**(1차: `stty` 실패를 보지 않음, 사용자 ID 질문에서 입력이 끝나면 끝나지 않음, smoke의 정리 trap 순서와 사전 확인의 HOME, 시험의 `fcntl`·`kill` 결과. 2차: smoke의 HOME 쓰기 확인이 명령 실패를 버림).
