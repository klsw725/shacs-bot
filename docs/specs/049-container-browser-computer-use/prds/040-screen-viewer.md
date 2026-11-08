# PRD 040. 브라우저 화면 보기

Status: Planned

Depends on: PRD [000](000-tool-result-images.md)·[010](010-container-browser-and-boundary.md)·[020](020-browser-tool.md)·[030](030-decision-wiring.md)의 닫힌 결과. 048의 `.env.json`·`${이름}` 해석·값 가리기를 소비한다. 050·051의 구현에 의존하지 않는다.

## Goal

사용자가 Docker가 도는 컴퓨터에서 브라우저 창을 보기만 한다. 비밀번호 없는 화면은 열리지 않고, client를 고쳐 보내는 입력도 서버에서 막는다. viewer가 실패해도 어시스턴트의 browser는 계속 쓴다.

## 지금 상태

- runtime image 설치 목록에는 `x11vnc`, `novnc`, `websockify`가 없고 `EXPOSE`는 18790·8900·8765뿐이다(`Dockerfile`의 `runtime` stage). Compose의 gateway·api는 각각 WebSocket·API port만 loopback에 publish한다(`docker-compose.yml`의 두 service).
- `ToolsConfig`에는 browser/view 설정이 없다(`crates/shacs-config/src/lib.rs`의 `ToolsConfig`). `onboard_config_defaults`와 `merge_missing_json_defaults`가 기본 stub을 기존 값에 덧붙인다(`crates/shacs-cli/src/lib.rs`).
- `runtime_inspect_with_owner`가 containment·lifecycle·소유 상태 등을 묶고 `diagnostics_snapshot_from_runtime_inspect`가 이를 진단으로 바꾼다(`crates/shacs-cli/src/spec035_cli_media/runtime_inspect.rs`, `crates/shacs-cli/src/lib.rs`). viewer 전용 필드는 없다.
- `mcp_startup_gate`는 process 시작 action·envelope·containment proof를 만들어 주며 실행을 tool의 매 동작과 분리한다(`crates/shacs-cli/src/lib.rs`). viewer는 010의 browser 시작 묶음에 참여해야 하며 새로운 tool process adapter로 등록할 대상이 아니다.

## Scope

1. **image와 port:** Debian runtime package `x11vnc`, `novnc`, `websockify`를 더한다. noVNC 정적 파일 root는 `/usr/share/novnc`다. Dockerfile에 `EXPOSE 6080`을 추가하고 Compose gateway에 `127.0.0.1:6080:6080`, api에 `127.0.0.1:6081:6080`만 더한다. 5900은 EXPOSE/publish하지 않는다. viewer web listener는 container의 `0.0.0.0:6080`이고 VNC는 `127.0.0.1:5900`이다. host의 다른 interface에는 publish하지 않는다.
2. **설정과 비밀번호:** `tools.browser.view.enable`은 기본 false, `tools.browser.view.password`는 기본 빈 글이다. onboard는 기존 값을 보존하고 이 stub을 더한다. 권장 config는 `password: "${SHACS_BROWSER_VIEW_PASSWORD}"`, 값은 data 폴더의 `.env.json`에 둔다. 기존 config resolver가 비어 있지 않은 process 환경 변수를 먼저 보고 다음 `.env.json`을 보는 순서를 유지한다. runtime이 `.env.json` 값을 process 환경에 내보내지 않는다.
    - 켰는데 값이 없거나 8자 미만이면 viewer만 unavailable로 만들고 browser를 계속 시작한다. x11vnc 파일 형식으로 표현할 수 없는 NUL·CR·LF, 줄의 앞뒤 공백, 주석(`#`로 시작)을 거절한다. `__SKIP__`, `__COMM__`, `__EMPTY__`, `__BEGIN_VIEWONLY__`는 비밀번호가 아니라 제어 지시로 해석될 수 있으므로 어느 위치에라도 이 문자열을 포함한 값은 `password_format_invalid`로 거절한다. 값이나 변수 이름 없이 사유 코드만 진단에 남긴다. 권장값은 8–128자의 무작위 ASCII이며 최소 계약은 8자다. 길어도 VNC가 앞 8자만 확인함을 설정 문서에 적는다.
    - 비밀번호 원문은 tool schema·action 설명·모델 요청·channel·inspect·diagnostics에 넣지 않는다. 설정 직렬화와 migration은 기존 password key의 비밀 취급을 따른다. 빈 기본값을 채우려고 비밀을 새로 생성하거나 user config를 바꾸지 않는다.
3. **시작 인자:** 010의 user-local enable 기반 시작 gate가 허락한 closure 안에서 Xvfb 준비·Chromium 시작 뒤 같은 display·Xauthority를 쓰는 viewer participant를 시작한다. mode·대화형 여부를 viewer process 시작의 추가 조건으로 삼지 않는다. 실행은 다음과 같다.

   ```text
   x11vnc -norc -noremote -display :<번호> -auth <Xauthority>
     -rfbport 5900 -listen 127.0.0.1 -localhost
     -viewonly -nosel -noclipboard -forever -shared
     -passwdfile rm:<임시 password 파일>
   websockify --web=/usr/share/novnc 0.0.0.0:6080 127.0.0.1:5900
   ```

   일반 noVNC의 `vnc.html`을 쓴다. URL에 비밀번호를 넣지 않고 사용자가 client의 비밀번호 칸에 친다. `-viewonly`가 client의 PointerEvent·KeyEvent를 서버에서 거절한다. selection과 clipboard는 양방향 끄고 file transfer·원격 제어도 열지 않는다. client 설정의 view-only 표시만으로 강제했다고 보지 않는다. `-norc`는 home 설정으로 인자가 바뀌지 않게 한다.
4. **비밀번호 전달:** browser의 0700 임시 폴더 안에 예측할 수 없는 이름의 보통 파일을 `create_new`·0600으로 만들고 비밀번호 한 줄만 쓴 뒤 닫는다. secret은 `-passwd`·`-env`·shell command에 넣지 않는다. `-passwdfile rm:<path>`는 x11vnc가 읽은 뒤 지우는 방식이다. 시작 성공은 VNC listener 준비와 파일 부재를 모두 확인한 뒤에만 보고한다.
    - viewer 시작 한도는 고정 10초이고 010 전체 시작 한도의 남은 시간이 더 짧으면 그것을 쓴다. x11vnc가 파일을 읽기 전 죽거나 timeout이면 host가 파일을 지운다. spawn 실패·websockify 실패도 같은 cleanup이다. 열린 파일 핸들을 불필요하게 보존하지 않는다.
    - x11vnc·websockify 환경은 010 Scope 4의 모든 generation 자식에 대한 `env_clear`·allowlist 규칙으로 만든다. viewer만의 별도 상속 규칙을 만들지 않으며 `${NAME}`의 원본 환경 변수나 provider credential을 물려주지 않는다. stdout/stderr는 모델·channel로 보내지 않고 시작/종료의 사유 코드만 진단한다. argv에는 파일 경로만 있고 값이 없다.
    - 근거는 Debian x11vnc 0.9.16 manual의 `-passwdfile`: `rm:` 파일은 읽은 뒤 삭제하고 VNC 인증은 앞 8자만 쓴다. 구현 종료 때 실제 package의 같은 동작을 opt-in 시험으로 확인한다.
5. **수명:** viewer enable과 유효 password는 browser generation 시작 시 snapshot으로 읽는다. 010 lifecycle participant의 start/stop/failure 보고만 구현하고 별도 browser 자리나 singleton을 더 만들지 않는다. VNC 준비 뒤 websockify를 열고, 종료는 websockify→x11vnc 순서다. 중간 실패면 시작한 viewer 자식을 모두 정리하고 browser는 유지한다. 브라우저·Xvfb 종료나 idle close·process 종료에서는 둘 다 끝나고 파일도 없다.
    - x11vnc 또는 websockify 하나만 죽어도 viewer 쌍을 닫고 `failed`로 기록한다. 그 generation에서는 자동 재시작하지 않고 browser를 계속 쓴다. 다음 browser generation이 시작될 때 다시 시도한다. viewer 시작에 대한 gate 판정을 tool 호출마다 새로 하지 않는다.
    - viewer 연결자가 있는지와 무관하게 browser 동작·timeout·판정은 같다. 연결은 승인이나 session 소유권을 만들지 않는다. 050을 위한 입력 전환·clipboard 전송·takeover 자료형은 미리 넣지 않는다.
6. **네트워크 경계:** VNC loopback listener와 websockify target은 고정이고 사용자 config로 바꾸지 않는다. browser 연결은 010 proxy를 거치므로 기본 guard가 loopback·같은 container의 viewer/API port를 막는다. 별도 test-page container `/32`만 여는 020 Docker 설정은 이것을 풀지 않는다. 사용자가 loopback/container 주소 전체를 whitelist하면 port 단위 보호가 풀릴 수 있다는 010의 한계도 그대로 쓴다. noVNC 정적 HTML이 비밀번호 없이 내려오는 것은 허용하되 framebuffer 연결은 VNC 인증을 통과해야 한다.
7. **inspect와 diagnostics:** 010 browser snapshot 안에 `view`를 더한다. 필드는 `enabled`, `state`(`disabled`, `unavailable`, `starting`, `open`, `failed`, `closed`, `unknown`), `reason`(`password_missing`, `password_too_short`, `password_format_invalid`, `package_missing`, `startup_failed`, `process_exited` 등), `url`이다. 비밀번호·변수 이름·password 파일 내용·원본 stderr는 없다. 열렸을 때 gateway는 `http://127.0.0.1:6080/vnc.html`, api는 `http://127.0.0.1:6081/vnc.html`이다. 기본 container 내부 service port는 둘 다 6080이다. 일회 CLI의 기본 안내는 6080이고 실제 publish는 사용자의 책임이다.
    - 별도 inspect는 010의 현재 owner 검증을 통과한 snapshot만 live 상태로 보인다. owner가 바뀌거나 lease가 끝나면 viewer도 unknown이고 이전 open URL로 현재 열림을 주장하지 않는다. config상 꺼짐·password 미설정·실행 파일 부재 같은 사용 가능 사실과 live 상태를 구별한다. JSON·글 형식과 `diagnostics_snapshot_from_runtime_inspect`를 함께 맞춘다.
8. **실제 Docker 확인:** opt-in `scripts/spec049-viewer-check.sh`를 만들고 사용자 승인 뒤에만 실행한다. 020의 disposable data·별도 정적 test-page container와 그 주소 `/32` 허용 방식을 재사용한다. 사용자 data는 쓰지 않는다. 임시 `.env.json`의 비밀번호는 stdout에 쓰지 않는다.
    - host gateway 6080·api 6081의 올바른 비밀번호로 framebuffer를 받고 틀린 비밀번호는 실패하는지, authenticated client가 직접 보낸 pointer/key/clipboard가 fixture 페이지를 바꾸지 않는지 확인한다. UI의 view-only checkbox만 시험하지 않는다.
    - host에서 VNC 5900에 닿지 않고 Compose publish가 loopback뿐인지, browser에서 viewer/API port로 가지 못하는지, 시작 후 password 파일이 없는지 확인한다. viewer 한 자식을 종료해도 browser look이 되고, browser close 뒤 viewer가 끝나는지 확인한다. 남은 project·임시 자료만 정리한다.
    - 사용자가 거절하면 결정·이유·확인하지 못한 인증/입력 차단/네트워크 실동작을 적어 `Complete (Scoped)`로 닫는다. 실행 실패는 실패로 남기고 거절로 처리하지 않는다.
9. **문서:** `docs/USAGE.md`와 README Docker·browser 설명에 켜는 설정, `.env.json` 예(가짜 값), gateway/api 주소, 보기 전용·clipboard 차단, 앞 8자 제한·비암호화 loopback·viewer 실패 시 browser 유지·자원 비용을 적는다. 원격 접속·터널·로그인·takeover는 구현하지 않는다.

## Non Scope

- viewer에서 입력받기·한글 붙여넣기·로그인·화면 넘겨받기(050), 감시용 viewer 상태(051).
- 외부 interface publish, TLS/reverse proxy/VPN 자동 설정, 임의 VNC target.
- browser tool의 결과 캡처 방식·그림 보관·권한 판정 변경.

## Parent Requirement Mapping

- [049 SPEC](../SPEC.md) 소유 범위 6, 7 중 `view`, 8 중 viewer 상태·자기 문서, 수용 조건 5·7·8을 소유한다.
- SPEC "앞선 스펙에서 바꾸는 것"의 추가 행은 소유하지 않는다. 010이 닫은 030 실행 종류 행·030·022 시작 판정 행과 lifecycle hook·모든 자식의 공통 환경 규칙을 소비한다. 022 plan·비대화형 bypass 행은 030 소유다.
- 050이 나중에 바꿀 보기 전용/clipboard 계약은 여기서 미리 바꾸지 않는다.

## Required Contract

1. viewer가 꺼지거나 비밀번호가 유효하지 않으면 framebuffer를 열지 않는다. browser 사용은 유지한다.
2. 보기 전용과 양방향 clipboard 차단을 x11vnc 서버에서 강제한다.
3. secret은 0600 임시 파일에만 잠깐 쓰고 읽은 뒤 없으며 argv·자식 env·로그·모델·channel·inspect에 없다. 자식 env는 010의 공통 규칙을 따르고 제어 문자열이 든 password는 파일을 만들기 전에 거절한다.
4. VNC는 container loopback에만, host publish는 127.0.0.1의 6080/6081에만 연다.
5. viewer의 실패는 browser를 닫지 않고 browser 종료는 viewer를 반드시 닫는다.
6. Cargo 시험은 가짜 process runner만 쓰고 `Cargo.lock`은 바뀌지 않는다.

## Acceptance Criteria

1. `crates/shacs-cli/tests/it/spec049_browser_viewer.rs`의 `viewer_password_gate_does_not_block_browser`: 가짜 process runner로 꺼짐·없음·짧음·파일형식 오류·package 없음에서 viewer spawn이 없고 browser가 준비되는지 확인한다. 기존 파일형식 오류 표에 `__SKIP__`·`__COMM__`의 앞/중간/끝 삽입과 `__EMPTY__`·`__BEGIN_VIEWONLY__`를 넣어 길이 조건을 충족해도 password 파일이나 viewer 자식이 생기지 않고 `password_format_invalid`인 것을 확인한다. 048 resolver를 통해 `${NAME}`이 `.env.json`에서 풀리되 자식 env에는 안 들어가는지 확인한다.
2. 같은 파일의 `viewer_arguments_and_password_file_cleanup`: argv의 viewonly·loopback·selection/clipboard 끔·norc/noremote와 web root/target을 확인한다. fake x11vnc가 파일을 읽고 지우는 성공 경로와 읽기 전 종료·timeout·websockify 실패의 host cleanup을 확인한다. 파일 0600/상위 0700, 원문 없는 argv/env/log·tool/model/diagnostics도 같은 비밀 표식으로 검증한다. 부모 환경에 provider credential·viewer password source 표식을 둔 채 x11vnc·websockify가 010 공통 allowlist만 받는지 확인하고, 010의 기존 lifecycle 시험이 검증한 xauth·Xvfb·Chromium과 같은 환경 생성 경로를 쓰는지 확인한다.
3. 같은 파일의 `viewer_lifecycle_and_owner_inspect`: 실제 010 lifecycle participant와 fake runner로 browser close·Xvfb 사망의 viewer 종료, viewer-only 사망 뒤 browser 호출 유지, 다음 generation의 재시작, 유효/지난 owner의 viewer snapshot과 inspect JSON·글/diagnostics를 확인한다.
4. `crates/shacs-config/tests/it/spec049_browser_config.rs`에 view 기본값·password key의 비밀 취급 검사를, `crates/shacs-cli/tests/it/spec049_browser_viewer.rs`에 onboard 값 보존·Dockerfile/Compose의 port 계약 검사를 보탠다. 기존 `tests/it/main.rs`에만 등록한다. 단순 getter 시험이나 새 최상위 test binary를 늘리지 않는다.
5. 사용자 승인 뒤 `scripts/spec049-viewer-check.sh`가 Scope 8을 확인한다(부모 수용 조건 7). 미실행 결정이면 이유와 미검증 범위를 명시한다.
6. fmt → workspace clippy `-D warnings` → 변경 package의 `cargo test --manifest-path crates/Cargo.toml -p <package> -q`를 `--locked`로 실행한다. 전체 workspace test는 사용자 승인 뒤에만 한다. 사용 문서와 `Cargo.lock` 무변경을 확인한다.

## Closure Evidence

1. fake process runner의 redacted argv/env, password 파일 생성·소멸·실패 정리 증거와 browser 유지 시험 결과.
2. 비밀 없는 inspect·diagnostics 예, Compose loopback publish와 Dockerfile package/EXPOSE diff.
3. 실제 Docker의 인증·서버 입력 차단·clipboard·종료 결과 또는 사용자 미실행 결정·이유·확인하지 못한 범위.
4. Cargo 검증 기록, 사용 문서와 `Cargo.lock` 확인.

PRD 040은 보기 전용으로 닫힌다. 050의 입력 전환이나 로그인은 종료 조건이 아니다.

## 남은 제한

- VNC 인증은 앞 8자만 쓰고 host와 container 사이 연결은 암호화하지 않는다. 같은 컴퓨터에서 비밀번호를 아는 사람은 화면을 본다.
- 캡처와 달리 viewer는 주소창·팝업까지 그대로 보인다. 화면의 비밀을 별도로 가리지 않는다.
- x11vnc password 파일 문법 때문에 줄바꿈·제어 줄을 비밀번호로 쓸 수 없다. 비밀번호를 잊었다고 runtime이 대신 보여 주지 않는다.
- viewer failure 뒤 같은 generation에서 자동 재시작하지 않는다. 다음 browser 시작까지 viewer만 쓸 수 없다.
- Docker 확인을 거절한 scoped 종료는 실제 client의 입력 차단·인증·port 노출 확인을 대신하지 않는다.
