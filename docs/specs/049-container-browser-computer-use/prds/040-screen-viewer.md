# PRD 040. 브라우저 화면 보기

Status: Complete (Scoped)

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
    - 켰는데 값이 없거나 8자 미만이면 viewer만 unavailable로 만들고 browser를 계속 시작한다. x11vnc 파일 형식으로 표현할 수 없는 NUL·CR·LF, 줄의 앞뒤 공백, 주석(`#`로 시작)을 거절한다. `__SKIP__`, `__COMM__`, `__EMPTY__`, `__BEGIN_VIEWONLY__`는 비밀번호가 아니라 제어 지시로 해석될 수 있으므로 어느 위치에라도 이 문자열을 포함한 값은 `password_format_invalid`로 거절한다. 값이나 변수 이름 없이 사유 코드만 진단에 남긴다. 비밀번호는 8–128자의 인쇄 가능 ASCII(`0x21`–`0x7E`, 공백 없음)만 받는다. 8자 미만은 `password_too_short`, 128자 초과는 `password_too_long`, 그 밖의 문자는 `password_format_invalid`이고 위 제어 문자열·주석 제외는 그대로다(이유는 구현 결정). `${이름}` 참조의 값이 어디에도 없으면 config load를 막지 않고 `password_missing`으로 본다. 무작위 값을 권장한다. 길어도 VNC가 앞 8자만 확인함을 설정 문서에 적는다.
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
    - viewer는 browser core(proxy·Xvfb·Chromium·DevTools 초기화)가 010 시작 한도 안에서 끝나고 검증된 뒤 시작하며, 시작 한도는 그와 별도인 고정 10초다. 그래서 첫 호출의 시작은 최대 `startupTimeoutSeconds` + 10초가 걸린다. 이 PRD는 010의 시작 한도를 core에만 적용하는 것으로 셈하고, viewer의 시간은 그 한도에 넣지 않는다(010 문서는 바꾸지 않는다). viewer timeout은 viewer만 실패로 만든다. x11vnc가 파일을 읽기 전 죽거나 timeout이면 host가 파일을 지운다. spawn 실패·websockify 실패도 같은 cleanup이다. 열린 파일 핸들을 불필요하게 보존하지 않는다.
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

## 구현 결정 (2026-11-11)

- **설정 자료형과 비밀 취급:** `BrowserConfig.view: BrowserViewConfig { enable, password }`(`crates/shacs-config/src/browser_config.rs`). `password`는 평문 `String`으로 다른 `password` 키와 같은 길을 쓴다. `${NAME}`은 기존 loader가 process 환경 → `.env.json` 순서로 풀고, `runtime config-migrate`는 `is_sensitive_key("password")`로 `SHACS_TOOLS_BROWSER_VIEW_PASSWORD`에 옮기며, redaction은 key 이름으로 가린다. `Debug`만 직접 구현해 값을 `[REDACTED]`로 바꾼다. 이 key만은 선택적 비밀이다. 권장 config는 값이 아직 없어도 `"${SHACS_BROWSER_VIEW_PASSWORD}"`를 적으므로, `resolve_config_env_refs`(`crates/shacs-config/src/lib.rs`)가 이 필드를 같은 resolver·같은 순서(process 환경 → `.env.json`)로 풀되 값이 없으면 빈 글로 두고 변수 이름을 오류에 넣지 않는다. 다른 모든 필드의 `${NAME}` 누락은 그대로 config 로드 오류다. 그래서 viewer를 꺼 둔 채 참조만 남아 있어도 runtime과 browser는 시작하고, 켜져 있으면 viewer만 `password_missing`이다.
- **검증 위치:** 비밀번호 검증(`validate_view_password`)과 참여자는 `crates/shacs-cli/src/browser/browser_viewer.rs` leaf에 둔다. 규칙은 8–128자(`password_too_short`/`password_too_long`)의 인쇄 가능 ASCII `0x21`–`0x7E`만이다(공백·탭·제어 문자·비ASCII는 어느 위치든 `password_format_invalid`). 128 상한은 Debian x11vnc 0.9.16이 `-passwdfile`을 1024바이트 `fgets` 버퍼로 줄마다 읽기 때문이다. 더 긴 줄은 두 번째 줄로 잘려 두 번째 비밀번호가 되어 함께 인증된다(예: `A`×1023+`B`는 `B`도 통과). ASCII 제한은 noVNC 1.3.0이 DES key를 JS 문자 코드(UTF-16)에서 만들고 파일은 UTF-8이라 비ASCII 비밀번호는 검증은 지나도 인증할 수 없기 때문이다. 공백은 안쪽에서도 받지 않는다(무작위 ASCII 권장값에 필요 없고, UI 입력 칸에서 앞뒤 공백 처리가 client마다 달라 혼동만 준다). 설정 crate는 값을 검증하지 않아 잘못된 비밀번호가 config 오류가 되지 않는다.
- **lifecycle hook 확장:** 010의 `BrowserLifecycleParticipant`에 `view()`를, `BrowserLifecycleFactory`에 `view(runner)`를, `BrowserProcessRunner`에 `viewer_available()`·`viewer_listener(port, owner)`를, `BrowserChild`에 `process_id()`(기본 `None`)를 더했다. 참여자는 generation마다 factory가 만들며 `create()` 때 enable·비밀번호를 snapshot한다. 꺼짐·비밀번호 문제·package 부재는 `start()`가 `Ok`를 돌려주고 `view()`로만 알린다. `Err`는 켜져 있어 실행해야 했으나 실패한 경우만이며 `BrowserProcesses::start_viewer`가 그때 `viewer_failed`로 기록한다. 실패한 참여자는 generation이 끝날 때까지 보관되어(`BrowserProcesses.viewer`를 `None`으로 바꾸지 않는다) `failed` 상태를 계속 보고한다.
- **시작 순서와 viewer 예산:** browser core(proxy → xauth → Xvfb → Chromium → DevTools 초기화)가 010 시작 한도 안에서 먼저 끝나고 검증된 뒤, 같은 gate closure 안에서 `BrowserProcesses::start_viewer`가 viewer를 시작한다. viewer 예산은 고정 10초(`VIEWER_START_LIMIT`)이며 browser 한도의 남은 시간과 무관하다. 처음 쓴 Scope 4의 "남은 시간이 더 짧으면 그것을 쓴다"는 독립 검토 2차 뒤 이 결정에 맞춰 고쳤다. 그 규칙은 `startupTimeoutSeconds: 5`에 viewer가 준비되지 않으면 browser 시작까지 실패로 만들었다(viewer가 공유 deadline을 소진하고 뒤의 DevTools 초기화·경과 시간 검사가 browser 실패로 바뀌었다). 지금은 manager가 core 완료 시각(`core_finished_ms`)으로만 browser 한도를 판정하므로 viewer가 10초를 다 써도 browser는 유지된다. 첫 browser 호출은 그만큼(최대 browser 한도 + 10초) 더 걸릴 수 있다.
- **x11vnc 시작 조건과 listener 소유:** 비밀번호 파일을 만들기 전에 `viewer_listener(5900, None)`이 `Absent`여야 한다. 아니면 파일 없이 `port_in_use`로 viewer만 실패한다. 파일(`<generation root>/view-<32 hex>`, `create_new`·0600, 한 줄, spawn 전 닫음) → x11vnc spawn(Scope 3의 인자 그대로, `rm:` 접두) → 10 ms 간격으로 x11vnc 종료 여부·파일 부재·listener 소유를 확인 → websockify spawn. 성공은 파일이 사라지고 **IPv4 `127.0.0.1:5900` LISTEN socket이 spawn한 x11vnc의 것**일 때만이다. Linux 구현은 `/proc/net/tcp`의 `0100007F:170C` LISTEN 행 inode를 `/proc/<x11vnc pid>/fd`의 `socket:[inode]`와 대조한다(`linux_loopback_listener`). x11vnc 0.9.16은 IPv4 bind가 실패해도 IPv6만 듣고 살아 있을 수 있어, 연결 가능 여부만 보면 websockify가 남의 IPv4 5900 server로 이어질 수 있었다. 다른 process의 listener가 보이면 `Foreign` → `port_in_use`, x11vnc 종료는 `process_exited`, 10초 초과는 `startup_timeout`, spawn 실패는 `startup_failed`다. 어떤 실패에서도 host가 websockify→x11vnc 순으로 멈추고 남은 파일을 지운다. 비Linux에서는 소유를 증명할 수 없어 `Foreign`을 돌려준다(그 환경에서는 runner가 어차피 unavailable). websockify의 즉시 종료는 `failed()` 감시가 `process_exited`로 잡는다.
- **상태 표면:** `BrowserStatus.view: BrowserViewStatus { enabled, state, reason, url }`을 010 snapshot 파일에 함께 쓴다. manager의 `publish`가 live generation의 `processes.view()`를, 없으면 factory의 설정상 사실을 넣고, 브라우저가 `starting`이면 사용 가능한 viewer도 `starting`으로 보인다. `read_browser_snapshot`이 owner 검증에 실패하면 `view.state = unknown`, `url = None`으로 바꾸되 `enabled`·`reason`(설정상 사실)은 남긴다. 별도 inspect는 bundle을 `resolve_env: false`로 읽으므로 `browser_viewer::configured_view`가 같은 `config_env` resolver로 비밀번호만 풀어 판정 코드만 남긴다. 브라우저 수준 `reason: viewer_process_exited`(010)는 그대로 두고 `view.reason: process_exited`를 더했다.
- **URL의 host port:** `process_browser_host`가 현재 process의 ownership marker `mode`를 읽어 `serve`면 6081, 그 밖(`run` gateway, marker 없는 일회 CLI)이면 6080을 factory에 준다. Compose 밖에서 다른 port로 publish한 경우의 안내는 사용자 책임이다.
- **Docker 확인 설계:** `scripts/spec049-viewer-check.sh`는 gateway·api 역할 container 둘과 정적 페이지 container를 별도 project에 띄운다. host port publish가 필요해 network는 `internal`이 아니다(020 확인은 internal). RFB·VNC DES 인증·WebSocket framing은 `scripts/spec049-viewer-fixtures/host_check.py`가 표준 라이브러리로만 구현하고 DES는 FIPS 시험 vector로 자기 검사한다. host의 5900은 macOS 화면 공유처럼 다른 service가 들을 수 있어, 연결되면 두 container의 publish 목록에 5900이 없음을 확인하고 그 사실을 적는다. Debian x11vnc 0.9.16은 Scope 3의 인자 그대로 띄우면 `127.0.0.1:5900`과 함께 IPv6 loopback `[::1]:5900`도 듣는다(`-localhost`의 기본 `-6`). 둘 다 container loopback이고 publish되지 않으므로 인자를 바꾸지 않고 Docker 확인이 두 loopback 주소를 모두 허용한다. gateway 역할 container는 Compose와 같은 `shacs-bot run --websocket-host 0.0.0.0 --allow-remote --verbose --allow-side-effects`로 띄우고 fixture가 WebSocket channel(`{"type":"message",...}` → `message` event)로 browser tool을 부르며 안내 URL이 6080인지 확인한다. api 역할은 `serve`와 local API로 6081을 확인한다. clipboard 차단은 두 방향으로 본다. ClientCutText 뒤 container 안 X PRIMARY/CLIPBOARD owner와 `CUT_BUFFER0`이 바뀌지 않았는지(production image에 package를 더하지 않고 chromium 의존성인 `libX11.so.6`을 Python `ctypes`로 직접 불러 확인), fixture helper가 server 쪽에서 PRIMARY·CLIPBOARD를 소유하고 `CUT_BUFFER0`을 채운 뒤 인증한 client에 3초 안에 ServerCutText가 오지 않는지 확인한다.

### 010 코드에서 바로잡은 것 (2026-11-11)

- **generation 자식은 process 수발의 전용 thread가 spawn한다.** Linux `PR_SET_PDEATHSIG`는 부모 *process*가 아니라 자식을 fork한 부모 *thread*가 끝날 때 신호를 보낸다. gateway(`run`)의 WebSocket turn과 local API turn은 tokio `spawn_blocking` thread에서 도는데, 그 thread가 끝나면(일을 마치고 풀에서 빠질 때) Xvfb·Chromium·x11vnc·websockify가 모두 SIGKILL되고 browser는 `browser_process_exited`로 실패했다. Docker 확인의 gateway 역할(`run`)에서 첫 turn 직후 재현됐고(자식 10개가 모두 zombie), api 역할은 같은 thread가 재사용되는 운으로만 살아 있었다. `LinuxBrowserProcessRunner::spawn`은 이제 `Command`를 process 수발의 전용 thread(`shacs-browser-spawner`, `spawn_from_persistent_thread`)에 넘겨 spawn하므로 자식의 parent-death 기준은 process 종료만이다. 가짜 runner 시험은 이 경로를 거치지 않아 Docker 확인이 유일한 증거다. 010 문서는 고치지 않는다.

### 구현 위치

| 범위 | 구현 |
|---|---|
| 1 | `Dockerfile`(`novnc`·`websockify`·`x11vnc`, `EXPOSE 6080`), `docker-compose.yml`(gateway `127.0.0.1:6080:6080`, api `127.0.0.1:6081:6080`) |
| 2 | config `browser_config.rs`의 `BrowserViewConfig`, CLI `lib.rs`의 onboard stub, `browser_viewer.rs`의 `validate_view_password` |
| 3·4 | `browser_viewer.rs`의 `x11vnc_arguments`·`websockify_arguments`·`create_password_file`·`BrowserViewer::launch`, `browser_process.rs`의 `BrowserEnvironment::child` 재사용 |
| 5 | `browser_viewer.rs`의 `BrowserViewer`(start/failed/stop/view), `browser_process.rs`의 `BrowserProcesses`, `browser_manager.rs`의 maintenance·`publish_or_dispose` |
| 6 | 010 proxy 그대로; `host_check.py`의 SOCKS·`open` 확인 |
| 7 | `browser_state.rs`의 `BrowserViewState`·`BrowserViewStatus`·`read_browser_snapshot`·`inspect`, `browser_manager.rs`의 `view()`·`publish` |
| 8 | `scripts/spec049-viewer-check.sh`, `scripts/spec049-viewer-fixtures/{viewer.html,viewer_check.py,host_check.py}` |
| 9 | `docs/USAGE.md` "브라우저 화면 보기", README browser 단락 |

수용 시험은 `crates/shacs-cli/tests/it/spec049_browser_viewer.rs`의 `viewer_password_gate_does_not_block_browser`, `viewer_arguments_and_password_file_cleanup`, `viewer_lifecycle_and_owner_inspect`, `onboard_preserves_view_and_images_publish_loopback_viewer_ports`와 `crates/shacs-config/tests/it/spec049_browser_config.rs`의 `view_defaults_and_password_secret_handling`이다. 기존 `tests/it/main.rs`에만 등록했다. 010의 시험 fake(`Runner`·`Facts`·`Startup` 등)는 `pub(super)`로 열어 재사용했고 새 trait method를 구현했다.

### 검증 결과 (2026-11-11, 독립 검토 1차 수정 뒤)

Cargo 명령은 하나씩 실행했고 `CARGO_TARGET_DIR`는 내보내지 않았다. 마지막 Rust 변경(전용 spawner thread) 뒤에 아래 명령을 모두 다시 실행했으며, 검증 도중 소스는 고치지 않았다.

| 명령 | 결과 |
|---|---|
| `cargo fmt --manifest-path crates/Cargo.toml --all -- --check --config imports_granularity=Module,format_macro_matchers=true` | exit 0 |
| `cargo clippy --manifest-path crates/Cargo.toml --workspace --all-targets --locked -- -D warnings` | exit 0. 기존 `imap-proto v0.10.2` future-compatibility 안내만 |
| CLI `--test it -q --locked spec049` | 46 passed / 0 failed / 0 ignored |
| core `--test it -q --locked spec049` | 19 passed / 0 failed / 0 ignored |
| config `--test it -q --locked spec049` | 2 passed / 0 failed / 0 ignored |
| `cargo test -p shacs-config -q --locked --no-fail-fast` | 121 passed / 0 failed / 0 ignored |
| `cargo test -p shacs-cli -q --locked --no-fail-fast` | 623 passed / 0 failed / 3 ignored(자기 재실행 자식 process의 중복 결과 1줄 제외) |
| `git diff --quiet -- crates/Cargo.lock` | 변경 없음 |
| `bash scripts/spec049-viewer-check.sh --approved` | PASS, EXIT=0 (`/tmp/spec049-viewer-check-final.log`) |
| `bash scripts/spec049-browser-check.sh --approved` | PASS, EXIT=0 (`/tmp/spec049-docker-check-final.log`) |

실제 Docker(arm64, Debian bookworm `x11vnc` 0.9.16, `websockify`, `novnc`, Chromium 156)에서 확인한 것:

- gateway 역할은 Compose의 production 명령 `shacs-bot run --websocket-host 0.0.0.0 --allow-remote --verbose --allow-side-effects`로 돌고 WebSocket channel로 browser tool을 불렀다. 별도 `runtime inspect`의 `view.url`은 gateway `http://127.0.0.1:6080/vnc.html`, api(`serve`) `http://127.0.0.1:6081/vnc.html`이다.
- host `127.0.0.1:6080`·`6081` 모두 올바른 비밀번호로 1280×720 framebuffer를 받고, 첫 글자가 다른 비밀번호는 거절되며, 앞 8자만 같은 비밀번호는 인증된다(VNC의 8자 한계).
- 인증한 client의 PointerEvent·KeyEvent·ClientCutText는 fixture 페이지의 event 수와 입력 칸을 바꾸지 않았고(browser `look`), container 안 X PRIMARY/CLIPBOARD owner와 `CUT_BUFFER0`도 그대로였다(PRIMARY는 Chromium 창이 소유, CLIPBOARD 없음, cut buffer 빈 글). fixture helper가 server 쪽에서 PRIMARY·CLIPBOARD를 소유하고 `CUT_BUFFER0`을 채운 뒤 3초 동안 client에 ServerCutText가 오지 않았다(server message 0건). 이 helper는 `SelectionRequest`에 내용을 답하지 않으므로, 밖으로 나가는 쪽의 증거는 `CUT_BUFFER0`의 글과 PRIMARY·CLIPBOARD의 소유 변경까지다. selection 내용의 전달 차단은 Scope 3의 `-nosel -noclipboard` 인자에 기댄다.
- x11vnc argv는 Scope 3 그대로, viewer 자식 두 개의 환경 키는 010 allowlist만, 비밀번호 파일은 시작 뒤 없었다. VNC는 container의 `127.0.0.1`·`::1`에만 듣고 Compose publish는 `127.0.0.1`의 6080/6081뿐이며 host에서 5900은 닿지 않았다. browser proxy는 `127.0.0.1`·container 주소의 6080·5900·8765·8900·9100을 거절했다.
- x11vnc를 죽이면 websockify도 끝나고 `look`은 계속 되며 `runtime inspect`가 `view: failed/process_exited`를 보였다. browser `close` 뒤 viewer 두 process와 임시 폴더가 없고 host 6081은 framebuffer를 주지 않았다.
- 같은 실행에서 PRD 020의 `scripts/spec049-browser-check.sh`도 바뀐 image로 통과했다(policy 적용, 입력·팝업·이미지·proxy·정리).

## 종료 기록 (2026-11-11)

독립 검토 2차 PASS(1차 REJECT: 긴 비밀번호, 남의 5900 listener, viewer timeout이 browser를 끝냄, 없는 비밀번호 참조가 config load를 막음 등 7건을 고침). 2차의 minor 3건은 Scope 2·4 본문을 구현에 맞추고 clipboard 증거의 범위를 좁혀 적어 반영했다. 실제 Docker 확인은 사용자 승인(2026-11-10)으로 일회용 project에서 돌렸고 마지막 tree에서 `scripts/spec049-viewer-check.sh`와 `scripts/spec049-browser-check.sh` 모두 `EXIT=0`이다. 전체 workspace 시험은 돌리지 않았다.

## 남은 제한

- VNC 인증은 앞 8자만 쓰고 host와 container 사이 연결은 암호화하지 않는다.
- 비밀번호는 8–128자의 인쇄 가능 ASCII(공백 없음)만 받는다. x11vnc 파일 형식(1024바이트 줄)과 noVNC의 UTF-16 DES key 때문이며, 한글·공백·기호 외 문자가 든 비밀번호는 `password_format_invalid`로 viewer만 꺼진다.
- container 안 `127.0.0.1:5900`을 다른 process가 쓰고 있으면 viewer는 `port_in_use`로 시작하지 않는다. port는 고정이고 설정으로 바꾸지 않는다.
- viewer 시작은 browser 시작 뒤 최대 10초를 더 쓴다. 첫 browser 호출의 지연이 그만큼 늘 수 있고, 그 사이 다른 호출은 busy를 받는다. 같은 컴퓨터에서 비밀번호를 아는 사람은 화면을 본다.
- 캡처와 달리 viewer는 주소창·팝업까지 그대로 보인다. 화면의 비밀을 별도로 가리지 않는다.
- x11vnc password 파일 문법 때문에 줄바꿈·제어 줄을 비밀번호로 쓸 수 없다. 비밀번호를 잊었다고 runtime이 대신 보여 주지 않는다.
- viewer failure 뒤 같은 generation에서 자동 재시작하지 않는다. 다음 browser 시작까지 viewer만 쓸 수 없다.
- Docker 확인은 selection 내용(PRIMARY·CLIPBOARD에 답하는 owner)이 밖으로 나가지 않음을 직접 재지 않았다. 이는 x11vnc 인자로 막는다.
- Docker 확인을 거절한 scoped 종료는 실제 client의 입력 차단·인증·port 노출 확인을 대신하지 않는다.
