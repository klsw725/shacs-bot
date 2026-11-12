# PRD 020. 로그인 유지

Status: Complete (Scoped)

Depends on: PRD [000](000-conversational-choice.md)·[010](010-takeover-and-return.md)의 닫힌 결과(브라우저 닫기 경로와 닫힘 알림, 넘겨받기 회차). [049](../../049-container-browser-computer-use/SPEC.md) PRD 010의 process 수명과 임시 폴더, 048의 비밀 파일 규칙을 소비한다. 051의 구현에 의존하지 않는다.

## Goal

사용자가 `tools.browser.persistLogin`을 켜면 한 번 로그인한 사이트가 다음 브라우저에서도 로그인된 상태다. 로그인 기록은 data 폴더의 `browser/profile/`에 있고, 어시스턴트의 목록·검색에 나오지 않으며 읽으려 하면 매번 묻고 고치지 못한다. 사용자는 `shacs-bot browser forget`으로 지운다.

## 지금 상태

- 049는 generation마다 `/tmp/shacs-browser-*`(0700) 아래 `profile`을 만들고 닫을 때 그 폴더를 통째로 지운다. Chromium 인자의 `--user-data-dir`는 그 안이다(`crates/shacs-cli/src/browser/browser_process.rs`).
- `child_environment_is_confined`는 자식의 환경 변수(`HOME`, `XDG_*`, `TMPDIR`, `XAUTHORITY` 등)가 임시 폴더 안인지만 본다. 실행 인자의 경로는 보지 않는다.
- 닫기는 process group에 SIGKILL을 보내는 것 하나다.
- 048 비밀 파일은 data 폴더의 `auth/` 아래 전부와 data 폴더 바로 아래의 정해진 이름들이다(`crates/shacs-core/src/runtime/auto_mode/secret_file.rs`의 `is_secret_path`, `names`). hard link 확인은 파일 4,096개, 깊이 16까지 훑고 넘으면 비밀로 본다(`store_holds`, `hard_link`).
- `shacs-bot browser` CLI 명령은 없다. 확인을 받는 명령은 `--yes`·`-y`로 건너뛰고, terminal이 아니면 `--yes` 없이 거절한다(`permissions auto-mode reset`).
- gateway와 api role은 같은 data 폴더(`~/.shacs-bot`)를 volume으로 나눠 쓴다(`docker-compose.yml`). 두 role의 container는 hostname이 다르다.

## 설계 결정과 근거 (2026-11-11)

Chromium 근거는 2026-11-11에 bookworm이 싣는 `chromium 156.0.8078.79-1~deb12u1`과 같은 판의 tag `156.0.8078.79` source에서 줄 번호까지 읽었다. Debian의 patch가 바꾼 것이 있을 수 있으므로 구현은 공식 image의 Chromium에서 아래 인자를 다시 확인하고, Scope 10의 Docker 확인이 마지막 증거다.

1. **HTTP cache를 어디에 두는가.** 고른 것: HTTP cache를 끄지 않는다. cache 폴더를 프로필 밖 generation 임시 폴더로 옮기고(`--disk-cache-dir=<generation root>/cache`), 닫을 때 049의 임시 폴더 지우기로 함께 지운다. 크기 인자는 주지 않는다. SPEC 050의 2026-11-11 고침(소유 범위 7, "반드시 지킬 것", 수용 조건 4)을 따른 것이다.
    - 근거: 이 판의 Chromium은 보통 프로필에서 HTTP cache를 끌 수 없다. `chrome/browser/net/profile_network_context_service.cc`:1356–1357이 "Always enable the HTTP cache."라는 주석과 함께 `http_cache_enabled = true`를 고정한다. cache 위치는 `--disk-cache-dir`가 pref `DiskCacheDir`로 정한다(`chrome/browser/prefs/chrome_command_line_pref_store.cc`:71).
    - 다른 길 A: `--disk-cache-size=1`(이 PRD를 처음 쓸 때 고른 것). 끄는 것이 아니다. 이 switch는 pref `DiskCacheSize`를 채울 뿐이고(같은 파일 :106), simple backend는 파일 하나의 한도를 `max(전체 한도 / 비율, 5 MiB)`로 잡는다(`net/disk_cache/simple/simple_backend_impl.cc`:64의 `kMinFileSizeLimit`, :284–291의 `MaxFileSize`). 5 MiB까지의 응답은 그대로 디스크에 쓰인다. 아주 작은 전체 한도에서 Chromium이 언제 비우는지는 정해진 계약이 아니다. 쓰지 않는다.
    - 다른 길 B: enterprise policy `DiskCacheSize`·`DiskCacheDir`. 같은 pref를 채울 뿐이고 정책 설명도 이 값을 "hint"라고 적는다. 끄지 못한다.
    - 다른 길 C: cache 폴더를 memory 파일 시스템에 둔다. `/dev/shm`은 Chromium 자신이 쓰는 256 MB 공유 memory다(`docker-compose.yml`의 `shm_size`. 049 PRD 010은 `--disable-dev-shm-usage`를 쓰지 않는다). cache가 그것을 채우면 renderer가 죽는다. cache만을 위한 tmpfs를 따로 mount하려면 `docker-compose.yml`과 container 구성을 바꿔야 하고(049 계약 변경), swap이 있으면 그래도 디스크에 닿는다. 사용자 결정이 있어야 한다(남은 제한).
    - 다른 길 D: `--incognito`. cookie도 memory에만 있어 로그인이 남지 않는다.
    - 다른 길 E: DevTools `Network.setCacheDisabled`. target마다 걸어야 하고 놓친 target(새 탭, service worker)의 응답은 쓰인다.
    - 결과: HTTP cache의 bytes는 브라우저가 떠 있는 동안 container의 `/tmp` 아래 0700 임시 폴더에 있다. 정상으로 닫히면 없어지고, 강제로 끝나면 남을 수 있다(Scope 2). 이 PRD는 어디에서도 "cache를 끈다"고 말하지 않는다.
2. **cookie 암호화 key.** 고른 것: `--password-store=basic`. container에는 keyring이 없고, 명시하면 keyring을 찾는 시도가 없으며 container를 새로 만들어도 같은 프로필을 읽는다(`components/password_manager/core/browser/password_manager_switches.cc`의 값 목록). cookie는 사실상 평문이므로 지키는 것은 폴더 권한과 048 규칙이다(남은 제한).
3. **닫는 방법.** 고른 것: 로그인 유지 generation은 DevTools `Browser.close`를 먼저 보내고 5초를 기다린 뒤 049의 SIGKILL로 간다.
    - 다른 길: 049처럼 바로 SIGKILL한다. Chromium은 cookie를 30초마다 또는 512건마다 쓴다(`net/extras/sqlite/sqlite_persistent_cookie_store.cc`의 `kCommitInterval`, `kCommitAfterBatchSize`). 로그인 직후 닫으면 그 cookie가 없어진다. `SingletonLock`도 남는다.
    - 근거: `Browser.close`는 이미 있는 DevTools 연결로 가고 새 signal 경로가 필요 없다.
4. **프로필을 두 process가 함께 쓰지 못하게 하기.** 고른 것: shacs-bot이 `browser/profile.lock`에 배타 `flock`을 걸고 generation 동안 쥔다.
    - 다른 길: Chromium의 `SingletonLock`에만 기댄다. 다른 hostname의 lock을 만나면 Chromium은 오류 창을 띄우는데(`chrome/browser/process_singleton_posix.cc`의 `DisplayProfileInUseError`), shacs-bot은 그 창을 읽지 못하고 시작 시간 초과로만 본다.
    - 근거: gateway와 api가 같은 volume을 쓴다. `fs2`의 `try_lock_exclusive`는 runtime 소유 lock에서 이미 쓴다(`crates/shacs-cli/src/lib.rs`).
5. **남은 `SingletonLock`.** 고른 것: 읽기만 하고 판단한다. 같은 hostname이고 그 pid가 Chromium으로 살아 있지 않으면 띄운다(Chromium이 스스로 지운다: `process_singleton_posix.cc`:831–842의 hostname과 `IsChromeProcess` 확인, :331의 `UnlinkPath`). 그 밖에는 띄우지 않고 이유를 알린다. shacs-bot은 lock을 지우지 않는다. 사용자가 답하지 않아 권장 기본으로 정했다(SPEC 050의 2026-11-11 기록).
    - 다른 길 A: lock이 조금이라도 남아 있으면 거절한다. 같은 container를 `docker restart`한 뒤에도 매번 막힌다.
    - 다른 길 B: 자기 `profile.lock`을 쥔 채 `Singleton*`을 지운다. 편하지만 SPEC 소유 범위 7의 "프로필을 고치거나 지우지 않는다"와 다르다. 사용자 결정이 있어야 한다(SPEC을 고쳐야 한다).
6. **`browser forget`이 떠 있는 브라우저를 닫는 방법.** 고른 것: 프로필을 어디에 두든 모든 generation이 따르는 lock 파일 둘이다.
    - 모든 generation(로그인 유지든 임시 프로필이든)은 떠 있는 동안 `browser/active.lock`에 공유 `flock`을 쥐고, `maintenance`가 1초마다 `browser/forget.lock`을 본다. forget은 `forget.lock`을 배타로 쥔다(닫으라는 신호). 그리고 `active.lock`을 배타로 잡았을 때, 곧 그 data 폴더로 뜬 브라우저가 하나도 없을 때만 지운다. `profile.lock`(결정 4)은 로그인 유지 generation만 쥐고, 한 프로필을 두 브라우저가 쓰지 못하게 하는 데만 쓴다.
    - 다른 길 A: 이 PRD를 처음 쓸 때처럼 로그인 유지 generation만 `profile.lock`을 쥐고 `forget.lock`을 본다. 로그인 유지를 끄고 띄운 브라우저와 저장된 프로필 없이 뜬 브라우저를 forget이 닫지 못한다. SPEC 소유 범위 7의 "브라우저가 떠 있으면 닫은 뒤"와 다르다.
    - 다른 길 B: 요청 파일을 쓴다. forget이 죽으면 요청이 남아 다음 브라우저가 닫힌다.
    - 다른 길 C: signal이나 API. forget은 다른 container(`docker compose exec`, 다른 role)에서 돌 수 있어 닿지 않는다.
    - 근거: `flock`은 process가 죽으면 풀린다. 남는 상태가 없다. 공유 lock이라 gateway와 api가 저마다 임시 프로필 브라우저를 띄워도 서로 막지 않는다(`fs2`의 `try_lock_shared`. 이미 `Cargo.lock`에 있다).
    - 대가: 로그인 유지를 꺼도 브라우저를 띄우면 data 폴더에 `browser/`(0700)와 빈 lock 파일 둘(`active.lock`, `forget.lock`)이 생긴다. `browser/profile/`은 건드리지 않는다. lock 파일은 프로필도 내려받기 위치도 아니므로 049의 "data 폴더를 프로필·내려받기 위치로 쓰지 않는다"는 그대로다.
7. **lock 파일이 바뀌어도 forget이 떠 있는 브라우저를 놓치지 않게 하는 방법.** lock 파일은 지우거나 바꿔 놓을 수 있는 보통 파일이다. 어시스턴트가 `active.lock`이나 그 위 폴더를 바꿔 놓으면 forget은 새 파일을 배타로 잡고 "뜬 브라우저가 없다"고 본다. 그래서 lock만으로는 브라우저가 닫혔다는 증거가 되지 못한다. 고른 것은 세 겹이다.
    - 첫째, 어시스턴트의 쓰기를 막는다. data 폴더의 `browser` 폴더 자신과 lock 파일 셋을 "브라우저 lock 경로"로 두고, 이 경로를 쓰거나 지우거나 옮기거나 바꿔 놓는 action을 모든 mode에서 거절한다(`bypass_permissions` 포함, 사용자 규칙으로 풀 수 없음. Scope 6). SPEC 소유 범위 8과 "앞선 스펙에서 바꾸는 것"에 더했다.
    - 둘째, 브라우저가 스스로 확인한다. generation은 띄울 때 `browser/`와 자기가 쓰는 lock 파일의 device·inode를 적어 두고, `maintenance`가 1초마다 그 경로가 여전히 같은 파일인지 본다. 다르거나 없으면 forget의 신호를 받은 때처럼 닫는다(Scope 3). 첫째가 잡지 못하는 길(경로를 적지 않는 스크립트, data 폴더를 통째로 옮기는 명령)로 바뀌어도 그 data 폴더의 브라우저는 1초와 닫는 시간 안에 없어진다.
    - 셋째, forget은 lock과 따로 프로필 자신을 본다. `SingletonLock`이 있으면 Scope 3의 "띄워도 되는 lock"일 때만 지운다. 다른 hostname을 가리키는 lock이면 다른 container의 브라우저가 그 프로필을 쓰는 중일 수 있으므로 지우지 않는다(SPEC 결정 기록 (나)와 같은 판단이다). forget은 또 lock 셋을 쥔 뒤 그 경로의 파일이 자기가 쥔 파일과 같은지 다시 본다(Scope 5).
    - 다른 길 A: lock 파일 셋을 048 비밀 파일 목록에 넣는다. 비밀 파일의 쓰기 판정은 적힌 경로 자신만 본다(`permission_materials.rs`의 `secret_write`). `browser/` 폴더를 옮기는 명령과 glob으로 지우는 명령을 잡지 못하고, 빈 파일을 읽을 때마다 묻게 된다.
    - 다른 길 B: 048 PRD 050의 `UserOnlyFiles`에 더한다. 폴더 자신·glob·code 글까지 보지만, auto mode의 marker 둘만 다루고 auto mode에서만 거절한다(`user_only_file.rs`:10–15, :34–45, `auto_decision.rs`의 `user_only_write`). 다른 mode와 `bypass_permissions`에서는 막지 못한다.
    - 다른 길 C: 쓰기 보호만 두고 둘째·셋째를 두지 않는다. 048의 판정은 file tool과 명령 분석이 경로를 찾은 명령에서만 듣는다. 그 밖의 길로 바뀌면 forget이 다시 lock만 믿게 된다.
    - 대가: container를 새로 만든 뒤 남은 `SingletonLock`(다른 hostname)은 브라우저 띄우기와 forget을 모두 막는다. 사용자가 다른 브라우저가 없는 것을 확인하고 `Singleton*`을 직접 지워야 한다(Scope 8, 남은 제한).

8. **시작 중 lock 파일 교체와 프로필 폴더의 소유권 (2026-11-12 구현 중 고침).** 실제 manager의 첫 process 실행 직전에 가짜 실행기를 멈추고 `active.lock`·`profile.lock`을 교체한 시험에서, 기존 forget이 `SingletonLock`이 없다는 이유로 시작 중인 프로필을 삭제했다. 경로의 inode를 한 번 더 확인하거나 1초 점검을 빠르게 하는 것으로는 확인 다음의 틈을 없앨 수 없다.
    - 저장 generation은 Scope 3의 `profile.lock` 다음에 `browser/profile` 폴더를 `O_DIRECTORY | O_NOFOLLOW`로 열어 그 descriptor에도 배타 `flock`을 잡는다(500 ms). 실패하면 `browser_profile_in_use`이고 어떤 process도 띄우지 않는다. 이 descriptor는 Chromium과 임시 폴더 정리가 끝난 뒤에 놓는다. 프로필 안에 새 파일을 쓰지 않는다.
    - forget은 기존 lock 셋과 inode 확인을 그대로 거친다. 열린 `browser/` descriptor에서 `profile`을 열어 그 폴더의 `SingletonLock`을 `readlinkat`으로 확인한다. 결정 (나)는 그대로다. 이어서 그 폴더 descriptor의 배타 lock을 **기다리지 않고** 잡는다. 잡히지 않으면 `profile_directory_busy`와 기존 "브라우저를 닫지 못해" 글로 끝나며 아무것도 지우지 않는다. 기존 15초 닫기 기다림을 늘리지 않는다.
    - 삭제는 그 descriptor에서 `openat`·`unlinkat`으로 내려가며 symlink를 따라가지 않는다. profile 이름이 바뀌어도 새 경로로 다시 내려가지 않는다. 마지막 빈 profile 폴더를 없애기 전에는 이름과 descriptor가 같은지 다시 확인한다. 바뀌면 성공으로 보고하지 않는다.
    - 같은 kernel과 inode에 대한 `flock`이므로 같은 volume을 쓰는 Docker container도 이 경계에 참여한다. 파일 공유 계층이 directory flock을 전달하지 않는 구성은 보장하지 않으며 Scope 10을 실행하지 않은 환경에서 작동한다고 주장하지 않는다.
    - **보장 경계의 좁은 명시:** lock 파일 셋이나 그것들을 담은 폴더 이름의 교체 때문에, 그대로 남은 사용 중 프로필을 forget이 지우는 것은 막는다. 외부 작업이 **프로필 폴더 자체의 이름을 Chromium 시작 또는 삭제 도중 바꾸는 것**은 지원하지 않는다. Chromium은 Scope 2의 절대 경로를 다시 열고, POSIX에는 "이 inode일 때만 rmdir" 하는 원자적 연산이 없다. descriptor를 쥐는 것만으로 이 두 이름 재해석을 막지는 못한다. 삭제가 순회하는 내용은 열린 폴더로 한정되지만, 마지막 검사와 비재귀 rmdir 사이에 바뀐 빈 폴더까지 같은 identity라고 보장하지 않는다. 이미 시작된 프로필을 밖으로 옮긴 뒤 그 자리에 둔 새 프로필을 forget으로 지우는 경우에는 옮긴 프로필의 내용이 보존되는 시험이 있다. 이 한계를 Required Contract 5의 전제로 적는다. 프로필을 fd 별칭으로 실행하는 계약 변경이나 강제 삭제 옵션은 더하지 않는다.

9. **독립 검토의 P1·P2 보완 (2026-11-12).** 앞선 구현은 Cargo 검증을 통과했지만 독립 검토에서 거절됐다. 두 순서를 실제 manager와 시험용 시계로 재현한 뒤 고쳤다.
    - P1: 프로필 폴더를 연 뒤 flock을 기다리는 동안 forget이 그 폴더를 지울 수 있었다. 기다림 뒤 잠금이 잡혀도 열린 inode는 이미 지워진 것이었다. 이제 배타 잠금을 잡은 직후 descriptor의 metadata(fstat)와 현재 `browser/profile`의 symlink를 따르지 않은 metadata(lstat)를 비교한다. 경로가 없거나 다른 inode면 `browser_profile_unreadable`로 거절하고 어떤 child도 띄우지 않는다. 다시 만들거나 재시작하지 않는다.
    - P2: 모든 generation이 프로필 mode와 관계없이 `browser/` 폴더 descriptor에 공유 flock을 쥔다. 파일 이름 `active.lock`만 교체해도 이 잠금은 같은 폴더 inode에 남는다. 살펴보기는 active 파일과 이 폴더를 모두 배타로 잡아 보고 바로 놓는다. 따라서 저장 프로필이 없어도 살아 있는 임시 브라우저를 보지 못하고 성공하는 길이 없다.
    - 시작 순서: 기존 active·forget 확인, 저장 mode면 profile 파일·폴더 잠금과 P1·SingletonLock 확인, 마지막으로 browser 폴더 공유 잠금(500 ms)이다. 마지막 잠금 뒤에도 폴더 descriptor와 현재 경로가 처음 확인한 inode인지 본다. 잠금이 바쁘면 `browser_forget_in_progress`, 폴더 identity가 달라졌으면 `browser_state_unavailable`이다. 이 마지막 단계 전에는 어떤 child도 없다. 임시 mode는 프로필을 열거나 만들지 않는다.
    - forget 순서: forget 파일 배타 잠금으로 닫기를 요청하고 active 파일, profile 파일, browser 폴더의 배타 잠금을 순서대로 잡는다. active 기다림을 시작할 때 잡은 **한 15초 예산**을 profile·browser 폴더 기다림도 나눠 쓴다(profile 파일은 남은 시간과 500 ms 중 작은 값). browser 폴더를 못 잡으면 `browser_directory_busy`와 기존 "브라우저를 닫지 못해" 글로 실패하며 `브라우저를 닫았습니다.`를 쓰지 않는다. 마지막 기다림 뒤 기존 경로 identity 확인을 한다. 그 뒤에만 프로필 없음으로 성공하거나 SingletonLock·프로필 폴더 잠금·삭제로 간다.
    - 순서의 이유: 저장 generation이 browser 폴더를 쥔 채 아직 profile 잠금을 기다리지 않으므로 forget과 잠금 순서가 뒤집히지 않는다. 기존 파일이 교체된 경우에도 저장 generation의 프로필 폴더 배타 잠금이 삭제를 막는다. 프로필 폴더를 아직 잠그지 않은 때 forget이 먼저 삭제했다면 P1이 시작을 막는다. forget이 browser 폴더를 먼저 잡으면 뒤의 generation은 그 공유 잠금을 잡기 전까지 child를 띄우지 않는다.
    - 수명: 새 descriptor는 기존 `BrowserLockHold`의 마지막 필드다. 정상 닫기·shutdown·cancel은 기존 `end`와 `Generation`의 정리 순서를 그대로 타고 프로세스·임시 폴더가 사라진 뒤 놓는다. 시작 오류나 panic unwinding도 내부 `BrowserProcesses`가 먼저 정리되고 바깥의 lock hold가 나중에 버려진다. lock 오류를 정상 결과로 바꾸지 않으며 custom Drop·catch_unwind·새 thread는 없다. PRD 000의 `Live`·닫기 예약·알림 확인과 PRD 010의 회차·재개 순서는 바꾸지 않았다. 정리 없이 process가 강제로 죽는 경우는 기존 parent-death와 SingletonLock 규칙 및 미검증 범위를 그대로 따른다.
    - Required Contract 5를 약하게 하지 않는다. 바뀌는 명세는 공통 폴더 잠금과 내부 중단 단계, 그리고 수용 조건 12에서 더 일찍 거절되는 순서뿐이다. browser 폴더 자체까지 교체한 경우의 기존 identity 감시와 SingletonLock·프로필 폴더 잠금은 남는다. 디렉터리 경로 자체를 외부에서 바꾸는 기존 제한을 lock 파일만 교체한 경우에 적용하지 않는다.

## Scope

1. **설정:** `tools.browser.persistLogin`(bool, 기본 `false`)을 더한다(`BrowserConfig`). onboard stub에 더하고 기존 값은 보존한다. generation을 띄울 때의 값을 그 generation이 닫힐 때까지 쓴다.
2. **프로필 폴더와 실행 인자:** 켜져 있으면 `<data 폴더>/browser/profile/`을 없을 때만 0700으로 만든다(`browser/`는 Scope 3의 공통 확인이 만든다). 있는 폴더의 권한은 바꾸지 않는다. Chromium 인자는 049 인자에서 `--user-data-dir` 값을 바꾸고 두 개를 더한 것이다. `--disk-cache-size`는 넣지 않는다(결정 1).

   ```text
   --user-data-dir=<data 폴더>/browser/profile
   --disk-cache-dir=<generation root>/cache
   --password-store=basic
   ```

    - 그 밖의 인자(`--no-sandbox`, `--remote-debugging-pipe`, proxy, `--disable-background-networking` 등), 환경 변수, 임시 폴더(0700), Xvfb, 화면 보기는 049와 같다. `HOME`과 `XDG_*`는 임시 폴더에 그대로 두므로 `child_environment_is_confined`는 바뀌지 않는다.
    - HTTP cache는 꺼지지 않는다. cache 파일은 `<generation root>/cache/` 아래에 생기고, 브라우저가 떠 있는 동안 페이지 응답(로그인한 계정의 내용 포함)을 담는다. generation root는 049의 `/tmp/shacs-browser-*`(0700)이다. 프로필 안에는 HTTP cache 폴더가 생기지 않는다.
    - 닫을 때: 모든 닫기 길(Scope 4)에서 임시 폴더를 049대로 통째로 지우므로 `cache/`도 함께 없어진다. `browser/profile/`은 지우지 않는다.
    - 강제로 끝난 때: shacs-bot process가 정리하지 못하고 죽으면(SIGKILL, OOM, `docker kill`, 전원) generation root가 통째로 남고 그 안의 `cache/`에 HTTP cache가 있다. 049에는 남은 `/tmp/shacs-browser-*`을 시작할 때 지우는 길이 없고 이 PRD도 더하지 않는다. 남은 폴더는 container의 쓰기 layer에 있어 `docker restart` 뒤에도 있고, container를 새로 만들면(`docker compose down` 뒤 `up`, image 갱신) 없어진다. data 폴더 밖이라 048 비밀 파일이 아니다. 0700이라 다른 사용자는 읽지 못하지만 048 규칙이 이 폴더를 막지는 않는다. 049의 임시 프로필이 강제 종료 때 남는 것과 같은 종류이고, 로그인 유지를 켜면 그 안에 로그인한 계정의 응답이 있을 수 있다는 점이 다르다(남은 제한).
    - 꺼져 있으면 049의 Chromium 인자가 글자 하나 바뀌지 않는다. 남아 있는 `browser/profile/`은 읽지도 쓰지도 지우지도 않고 `profile.lock`도 잡지 않는다. Scope 3의 공통 확인(`active.lock`, `forget.lock`, `browser/` 폴더 공유 잠금)은 한다.
3. **띄우기 전 확인:** 모든 generation(로그인 유지든 임시 프로필이든)은 process gate를 지난 뒤, 어떤 process도 띄우기 전에 아래 1–3을 순서대로 본다. 로그인 유지 generation은 이어서 4–6을 본다. 어느 것이든 걸리면 process를 띄우지 않고, 쥔 lock을 놓고, 프로필을 고치거나 지우지 않고, 고정 글을 돌려준다.

   | 순서 | 누가 | 걸린 것 | 결과 글 |
   |---|---|---|---|
   | 1 | 모든 generation | `browser/`를 0700으로 만들 수 없음, 진짜 폴더가 아님(symlink 포함), 주인이 지금 사용자가 아님, lock 파일을 열 수 없음 | 049의 `browser_state_unavailable` |
   | 2 | 모든 generation | `browser/active.lock`의 공유 `flock`을 잡지 못함(forget이 지우는 중) | `browser_forget_in_progress: 로그인 기록을 지우는 중입니다. 잠시 뒤 다시 하세요` |
   | 3 | 모든 generation | `active.lock`을 쥔 채 본 `browser/forget.lock`을 다른 process가 쥐고 있음(forget이 브라우저가 닫히기를 기다리는 중) | 같은 `browser_forget_in_progress` |
   | 4 | 로그인 유지 | `browser/profile/`이 진짜 폴더가 아님(symlink 포함), 주인이 지금 사용자가 아님, 읽고 쓸 수 없음, 만들 수 없음 | `browser_profile_unreadable: 로그인 기록 폴더를 쓸 수 없습니다` |
   | 5 | 로그인 유지 | `browser/profile.lock`의 배타 `flock`을 잡지 못함 | `browser_profile_in_use: 로그인 기록을 다른 곳에서 쓰는 중입니다` |
   | 6 | 로그인 유지 | `browser/profile/SingletonLock`이 있고 아래 "띄워도 되는 lock"이 아님 | `browser_profile_locked: 로그인 기록이 잠겨 있습니다. docs/USAGE.md의 브라우저 로그인 유지를 보세요` |

    - lock 파일 셋(`active.lock`, `forget.lock`, `profile.lock`)은 `browser/` 바로 아래에 있고 비어 있다. `O_NOFOLLOW`, 0600으로 연다.
    - `active.lock`: `fs2::FileExt::try_lock_shared`로 잡고 generation이 닫힐 때까지 쥔다. 프로필 mode와 관계없다. 1번의 폴더나 파일을 마련하지 못하면 브라우저를 띄우지 않는다. PRD 000의 세션 표식 파일과 같은 규칙이다(상태를 남기지 못하는 브라우저는 두지 않는다).
    - `forget.lock`: generation은 쥐지 않는다. 공유로 잡아 보고 바로 놓는다. 잡히지 않으면 forget이 쥔 것이다.
    - `profile.lock`: `try_lock_exclusive`로 잡고 generation이 닫힐 때까지 쥔다. 로그인 유지 generation만 잡는다.
    - 저장 프로필의 폴더 descriptor에도 같은 배타 lock을 잡고 generation 동안 쥔다(설계 결정 8). 잡은 직후 현재 프로필 경로와 descriptor의 device·inode를 비교하고, 없거나 다르면 `browser_profile_unreadable`로 거절한다(P1). sibling lock 파일이 교체돼도 같은 프로필 inode의 사용자는 이 잠금으로 서로 배제된다.
    - 마지막 공통 확인: 모든 generation이 `browser/` 폴더 descriptor를 공유로 잠근다(500 ms). 바쁘면 `browser_forget_in_progress`, 열린 폴더나 현재 경로의 identity가 처음 확인한 것과 다르면 `browser_state_unavailable`이다. 어느 child도 이 확인 앞에서 시작하지 않는다. 이 잠금은 모든 프로세스와 임시 폴더를 정리한 뒤 마지막으로 놓는다(설계 결정 9, P2).
    - 순서의 까닭: generation은 `active.lock`을 먼저 쥐고 `forget.lock`을 본다. forget은 `forget.lock`을 먼저 쥐고 `active.lock`을 기다린다(Scope 5). 그래서 forget이 지우는 동안 뜨는 브라우저도, 뜨는 중인 브라우저를 보지 못하고 지우는 forget도 없다.
    - 같은 파일인지 보기: generation은 lock을 쥔 뒤 `browser/`, `active.lock`, `forget.lock`(로그인 유지면 `profile.lock`도)의 device와 inode를 적어 둔다. `maintenance`는 1초마다 `forget.lock`을 보기 전에 그 경로들을 symlink를 따라가지 않고 다시 `stat`한다. 하나라도 없거나, symlink이거나, device·inode가 다르면 lock 경로가 바뀐 것으로 보고 `forget.lock`이 잡힌 때와 똑같이 닫는다(Scope 5의 "브라우저를 가진 process"). 프로필 mode와 관계없다. 닫힌 까닭은 049의 상태 사유와 inspect에 `browser_lock_replaced`로 남긴다(경로는 넣지 않는다). 그 뒤 새로 뜨는 generation은 그때 경로에 있는 파일로 1–3번을 처음부터 다시 한다.
    - 2·3·5에서 lock을 잡아 보는 것은 500 ms까지 100 ms 간격으로 다시 해 본다. inspect(Scope 7)나 forget의 살펴보기(Scope 5)가 잠깐 잡았다 놓는 것과 겹칠 수 있기 때문이다.
    - 띄워도 되는 lock: `SingletonLock`이 `<hostname>-<pid>` 꼴의 symlink이고, hostname이 이 container의 것이고, 그 pid의 process가 없거나 Chromium이 아닌 경우다. 이때는 그대로 띄우고 Chromium이 스스로 정리한다. symlink가 아니거나, 꼴이 다르거나, hostname이 다르거나, 그 pid가 Chromium으로 살아 있으면 거절이다. hostname과 pid 확인은 `BrowserProcessRunner`가 준다(시험에서는 가짜).
    - 거절 글은 049의 상태 사유와 inspect에도 같은 code로 남는다. 경로나 hostname은 넣지 않는다.
4. **닫기:** 로그인 유지 generation의 모든 닫기(`close` 동작, 쓰지 않아 닫힘, 답 기다리다 닫힘, `/browser close`, `/new`·`/stop`, process 종료 정리, Scope 5의 forget)는 먼저 DevTools `Browser.close`를 보내고 Chromium이 끝나기를 5초까지 기다린다. 끝나지 않거나 연결이 이미 끊겼으면 049의 종료(SIGKILL)로 간다. 그 뒤 나머지 process와 임시 폴더를 049대로 정리하고 `profile.lock`을 놓는다.
    - 넘겨받은 중이면 PRD 010대로 회차를 `closed`로 끝낸 뒤 닫는다.
    - Chromium이 스스로 죽은 경우(049의 `browser_process_exited`)에는 보낼 곳이 없으므로 049와 같다.
    - 임시 프로필 generation은 049 그대로 바로 끝낸다.
    - 어느 generation이든 process와 임시 폴더 정리가 끝난 뒤에 `active.lock`을 놓는다. 그래서 닫기 길을 탄 generation의 lock이 풀렸다는 것은 그 Chromium이 끝났다는 뜻이다. shacs-bot process가 정리 없이 죽으면 lock은 바로 풀리고 Chromium은 049의 parent-death 처리로 끝난다. 그 사이의 짧은 틈은 Scope 5의 `SingletonLock` 확인이 막는다.
5. **`shacs-bot browser forget`:** CLI에 `browser forget [--yes|-y]`를 더한다. 사용자가 실행하는 명령이라 048 규칙을 받지 않는다. `tools.browser.enable`이나 `persistLogin`이 꺼져 있어도 된다.
    - 살펴보기(아무것도 바꾸지 않는다): `browser/profile/`이 있는지와 크기를 보고, `browser/active.lock`과 `browser/` 폴더 descriptor를 배타로 잡아 보고 바로 놓아 떠 있는 브라우저가 있는지 본다(하나라도 잡히지 않으면 있다). active 파일이 없거나 새 파일로 교체돼도 폴더 잠금이 살아 있는 브라우저를 찾는다. 이 단계는 폴더나 파일을 만들지 않는다.
    - 프로필도 없고 떠 있는 브라우저도 없으면 묻지 않고 `지울 로그인 기록이 없습니다.`를 출력하고 0으로 끝난다.
    - 둘 중 하나라도 있으면 아래를 보이고 `계속할까요? [y/N]`을 묻는다. 프로필이 없어도, `persistLogin`이 꺼져 있어도 떠 있는 브라우저는 닫는다.

      ```text
      지울 폴더: <경로> (<크기>) | 없음
      떠 있는 브라우저: 닫습니다 | 없음
      ```

    - `--yes`면 묻지 않는다. terminal이 아닌데 `--yes`가 없으면 닫지도 지우지도 않고 오류로 끝난다. `y`·`yes`가 아니면 `지우지 않았습니다.`로 끝나고 브라우저도 닫지 않는다. 질문은 이미 있는 `crates/shacs-cli/src/consent_prompt.rs`의 `Prompt`를 쓴다(시험은 `ScriptedPrompt`). data 폴더를 쓰는 명령이므로 `data_folder_access.rs`의 명령 목록에 더한다.
    - 확인 뒤에는 아래 순서다. 어느 단계에서 멈추든 쥔 lock을 놓는다.
        1. `browser/`가 없으면 0700으로 만들고 `browser/forget.lock`을 배타로 잡는다. 2초까지 100 ms 간격으로 해 보고(generation이 잠깐 잡아 보는 것과 겹칠 수 있다), 못 잡으면 `다른 browser forget이 실행 중입니다.`로 끝난다(0이 아닌 종료 code).
        2. `browser/active.lock`을 배타로 15초까지 100 ms 간격으로 잡아 본다. 이때 시작한 15초 예산을 3번까지 함께 쓴다. 잡지 못하면 아무것도 지우지 않고 `브라우저를 닫지 못해 지우지 않았습니다. 잠시 뒤 다시 하세요.`로 끝난다(0이 아닌 종료 code). 파일 하나만 잡았다고 모든 브라우저가 닫혔다고 보지는 않는다.
        3. `browser/profile.lock`을 배타로 잡아 본다(500 ms와 남은 예산 중 작은 값). 못 잡으면 2번과 같은 글로 끝나고 지우지 않는다. 이어서 1번에서 연 `browser/` 폴더 descriptor를 남은 예산 안에서 배타로 잡는다. 못 잡으면 같은 글과 `browser_directory_busy`로 끝난다. 파일 셋과 폴더를 모두 쥔 뒤 `browser/`와 lock 파일 셋의 경로를 symlink를 따라가지 않고 `stat`해, 연 폴더와 쥔 파일 셋의 device·inode와 같은지 본다. 하나라도 다르면 `브라우저 lock 파일이 바뀌어 지우지 않았습니다. 다시 실행하세요.`로 끝나고 지우지 않는다(0이 아닌 종료 code).
        4. `browser/profile/`이 있으면 symlink가 아닌 진짜 폴더인지 다시 확인한다. 아니면 지우지 않고 오류로 끝난다. 그 안에 `SingletonLock`이 있으면 Scope 3의 "띄워도 되는 lock"일 때만 다음으로 간다(같은 함수). 이 hostname의 살아 있는 Chromium pid를 가리키면 2번과 같은 글로 끝나고 지우지 않는다. 다른 hostname을 가리키거나, symlink가 아니거나, 꼴이 다르면 `로그인 기록이 다른 곳에서 잠겨 있어 지우지 않았습니다. docs/USAGE.md의 브라우저 로그인 유지를 보세요.`로 끝나고 지우지 않는다(0이 아닌 종료 code). 다른 container의 브라우저가 그 프로필을 쓰는 중인지 이 container에서는 알 길이 없기 때문이다(SPEC 결정 기록 (나)와 같은 판단). lock 파일이 바뀌어 1–3번이 떠 있는 브라우저를 보지 못한 때에도 이 확인이 로그인 유지 브라우저의 프로필을 지킨다. 4번에서 지우지 않고 끝날 때에도 1–2번에서 닫힌 브라우저는 닫힌 채이고, 살펴볼 때 떠 있는 브라우저가 있었으면 `브라우저를 닫았습니다.`를 먼저 출력한다.
        5. 설계 결정 8의 프로필 폴더 lock을 즉시 잡아 본다. 바쁘면 `profile_directory_busy`로 끝나고 지우지 않는다. 잡히면 열린 폴더 identity를 통해 `browser/profile/`만 지운다. `browser/`와 lock 파일은 남긴다.
        6. 출력: 살펴볼 때 떠 있는 브라우저가 있었으면 먼저 `브라우저를 닫았습니다.`를 출력한다. 이어서 프로필을 지웠으면 `로그인 기록을 지웠습니다.`, 프로필이 없었으면 `지울 로그인 기록이 없습니다.`를 출력하고 0으로 끝난다.
    - 브라우저를 가진 process: `maintenance`가 generation이 있는 동안 프로필 mode와 관계없이 1초마다 `forget.lock`을 공유로 잡아 본다(잡히면 바로 놓는다). 잡히지 않으면 닫는다. Scope 3의 "같은 파일인지 보기"가 어긋난 때에도 똑같이 닫는다. 로그인 유지 generation은 Scope 4로, 임시 프로필 generation은 049의 닫기로 닫는다. 답을 기다리는 중(PRD 000)이어도 닫고, 넘겨받은 중이면 PRD 010대로 회차를 `closed`로 끝낸 뒤 닫는다. 그 세션에 PRD 000의 `browser_closed` 알림을 둔다. 동작이 실행 중이면 PRD 000의 닫기 예약을 쓴다. 프로세스와 임시 폴더 정리 뒤 `active.lock`과(쥐고 있었으면) `profile.lock`을 놓고, 마지막으로 `browser/` 폴더 공유 잠금을 놓는다.
    - forget이 도는 동안 새 generation은 프로필 mode와 관계없이 Scope 3의 2·3번에서 `browser_forget_in_progress`로 거절된다.
    - 멈춘 단계: forget을 하는 함수는 출력할 글·종료 code와 함께, 확인 뒤의 순서가 어디서 끝났는지를 값으로 돌려준다. `forget_lock_busy`(1번), `active_lock_busy`(2번), `profile_lock_busy`(3번에서 lock을 잡지 못함), `browser_directory_busy`(3번의 공통 폴더 잠금), `lock_replaced`(3번의 같은 파일인지 보기), `profile_not_directory`·`singleton_live`·`singleton_elsewhere`(4번), `profile_directory_busy`(5번의 폴더 lock), `removed`·`nothing_to_remove`(5–6번)다. 4번 또는 `profile_directory_busy`·`browser_directory_busy`에서 끝난 때에는 그때 쥔 lock 파일 셋의 device·inode도 담는다. 출력과 종료 code는 바뀌지 않고 이 값은 출력하지 않는다. 2번·3번과 4번의 `singleton_live`는 글이 같아서, 시험이 forget이 어디까지 갔는지 가리는 데 쓴다.
6. **비밀 파일과 lock 경로:** `SecretFiles`에 data 폴더의 `browser/profile`과 그 아래 전부를 더한다. `persistLogin` 값과 관계없이 늘 들어 있고 사용자가 뺄 수 없다.
    - `is_secret_path`: data 폴더 기준 상대 경로의 앞 두 조각이 `browser`, `profile`이면 비밀이다(폴더 자신 포함). `browser/active.lock`, `browser/forget.lock`, `browser/profile.lock`은 비밀이 아니다. 읽기는 막지 않고, 쓰기는 아래 "브라우저 lock 경로"로 막는다.
    - `names`: `<data 폴더>/browser/profile`의 절대 경로와 working folder 기준 상대 경로를 더한다. `auth/`와 같은 방식이다. `profile`이라는 이름만으로는 잡지 않는다.
    - 결과는 048 비밀 파일과 같다. 모든 mode에서 목록·검색에 나오지 않고, file tool과 명령 분석이 읽는다고 본 action은 매번 묻고, 쓴다고 본 action은 거절한다(`bypass_permissions` 포함, 사용자 규칙으로 풀 수 없음).
    - hard link 확인: 프로필은 파일이 많아 048의 4,096개 한도를 넘는다. 프로필은 따로 센다(파일 20,000개, 깊이 16). 확인하려는 파일이 프로필과 다른 device에 있으면 프로필을 훑지 않는다. 한도를 넘으면 048처럼 비밀로 본다.
    - 브라우저 lock 경로: data 폴더의 `browser` 폴더 자신과 `browser/active.lock`, `browser/forget.lock`, `browser/profile.lock` 넷이다. 비밀 파일이 아니라 어시스턴트가 바꾸지 못하는 경로다. `PermissionEnvironment`에 `BrowserLockPaths`를 더하고, 048 비밀 파일 쓰기와 같은 자리(`auto_decision.rs`의 `denied_by_user`에서 `secret_write`와 나란히)에서 판정한다. 그래서 `default`·`accept_edits`·`auto`·`bypass_permissions` 모두에서 거절이고, 사용자 allow 규칙과 기억된 허용이 풀지 못한다. `persistLogin` 값과 관계없이 늘 들어 있고 사용자가 뺄 수 없다. 파일이 아직 없어도 경로로 판정한다.
    - 걸리는 action: file tool의 쓰기·고치기·지우기와, 명령 분석이 쓴다고 본 경로(지우기, 옮기기와 덮어쓰기, link 만들기, redirection, `touch`)가 넷 가운데 하나인 경우다. 적힌 경로와 symlink를 푼 경로를 모두 본다. `browser` 폴더 자신을 지우거나, 옮기거나, 다른 것으로 바꿔 놓는 것도 걸린다. glob이 넷 가운데 하나에 맞을 수 있는 쓰기와, 명령이 code로 돌리는 글에 lock 파일 셋의 경로(절대 경로와 working folder 기준 상대 경로)가 적힌 경우도 거절한다. 맞춰 보는 방식은 048 PRD 050의 `UserOnlyFiles`(`written_by`, `pattern_may_write`)와 같다. 다만 그 목록은 auto mode의 marker 둘만 다루고 auto mode에서만 거절하므로 그 목록에 더하지 않고 따로 둔다. `UserOnlyFiles`는 바꾸지 않는다.
    - 거절 사유는 새 `PermissionPolicyReason::BrowserLockWrite`(`browser_lock_write`)이고, 글은 `브라우저 lock 파일은 바꿀 수 없습니다.`다. 읽기와 목록은 막지 않는다. lock 파일은 비어 있고 inspect가 이미 상태를 보인다.
    - 수용 조건이 적은 명령 꼴(`rm`, `mv`, `ln -sf`, `touch`, redirection)은 048의 명령 분석이 이미 대상 경로를 쓰기로 낸다(commit 5fcdea9의 `crates/shacs-core/src/runtime/auto_mode/shell_paths.rs`). `OPERAND_WRITERS`(8–10행)에 `rm`·`rmdir`·`mv`·`touch`·`truncate`·`chmod`·`chown`·`ln`이 있고 그 피연산자가 모두 쓰기 경로가 된다(80–108행). 출력 redirection의 대상도 쓰기 경로가 된다(50–54행). 그래서 명령 분석은 넓히지 않고 048의 그 계약은 그대로다. 이 PRD가 048에서 바꾸는 것은 SPEC의 lock 경로 행 하나다.
    - 이 보호가 듣는 곳은 048과 같다. file tool과, 명령 분석이 경로를 찾은 명령이다. 경로를 적지 않는 스크립트와, data 폴더 자신이나 그 위 폴더를 옮기는 명령은 잡지 못한다(048의 critical folder 확인은 `rm`·`rmdir`만 묻는다). 그 틈은 Scope 3의 "같은 파일인지 보기"와 Scope 5의 `SingletonLock` 확인이 받는다.
    - 브라우저를 띄우는 runtime과 `browser forget`은 file tool을 거치지 않으므로 프로필을 그대로 읽고 쓰고, lock 경로를 그대로 만들고 연다.
7. **inspect와 diagnostics:** `runtime inspect`의 브라우저 항목에 아래를 더한다. 파일 이름이나 내용, 사이트 이름은 넣지 않는다.

   ```text
   login.persist: true | false
   login.profile.exists: true | false
   login.profile.path: <data 폴더>/browser/profile
   login.profile.size_bytes: <수> (login.profile.size_truncated: true | false)
   login.profile.state: ready | absent | in_use | locked | unreadable
   ```

    - 크기는 파일 20,000개까지 더하고 넘으면 `size_truncated`다. symlink를 따라가지 않는다.
    - `state`는 Scope 3의 4–6번을 읽기만 해서 본 값이다. lock은 공유로 잡아 보고 바로 놓으며 쥐지 않는다. 폴더나 lock 파일을 만들지 않는다. 이 process의 generation이 그 프로필을 쓰는 중이거나 `forget.lock`이 잡혀 있으면 `in_use`다. `diagnostics_snapshot_from_runtime_inspect`는 `persist`, `exists`, `state`를 싣는다.
8. **문서:** `docs/USAGE.md`와 README의 브라우저 설명에 `persistLogin`, 프로필에 무엇이 남는지(cookie, 로그인 상태, 방문 기록, 사이트 저장 자료), 브라우저가 닫히면 없어지는 것(기한 없는 session cookie), HTTP cache가 어디에 있는지(꺼지지 않는다. 프로필 밖의 임시 폴더에 있고 브라우저를 닫을 때 지운다. shacs-bot이 강제로 끝나면 container의 `/tmp/shacs-browser-*`에 남을 수 있고, container를 새로 만들면 없어진다), 로그인 기록이 평문에 가깝고 같은 container에 있다는 위험, `shacs-bot browser forget`(Docker에서는 `docker compose exec <service> shacs-bot browser forget`. 로그인 유지를 꺼 둔 브라우저도 닫는다), 잠긴 프로필을 만났을 때 사용자가 할 일(다른 container나 다른 컴퓨터의 브라우저가 그 프로필을 쓰지 않는 것을 확인하고 `browser/profile/Singleton*`을 직접 지운다. 다른 hostname을 가리키는 잠금이 있으면 `browser forget`도 지우지 않는다. 브라우저를 띄웠던 container가 살아 있으면 그 container에서 forget을 다시 한다), 어시스턴트가 `browser/` 폴더와 lock 파일을 바꾸지 못한다는 것과 lock 파일이 바뀌면 브라우저가 스스로 닫힌다는 것, 프로필이 하나라는 것을 적는다.
9. **PRD 000·010과의 관계:** 닫힘 알림, `/browser` 명령, 넘겨받기는 바뀌지 않는다. 넘겨받아 한 로그인은 로그인 유지가 켜져 있을 때만 다음 브라우저에 남는다.
10. **실제 Docker 확인(opt-in):** `scripts/spec050-saved-login-check.sh --approved`와 `scripts/spec050-saved-login-fixtures/`를 더한다. 사용자 승인 뒤에만 돌린다. `--approved`가 없으면 아무것도 하지 않고 끝난다.
    - 준비는 `scripts/spec049-browser-check.sh`와 같다. `mktemp -d`의 일회용 data 폴더와 고유 compose project를 쓰고 사용자의 data 폴더와 `docker-compose.yml`은 건드리지 않는다. 고정 test 페이지 container(`172.30.250.2`, `tools.ssrfWhitelist`에 그 `/32`만)의 `/login`은 `Set-Cookie: spec050_login=<고정값>; Max-Age=86400; Path=/`를 주고 `/whoami`는 받은 cookie를 글로 보인다. `/cached`는 `Cache-Control: max-age=3600`인 고정 응답을 준다(HTTP cache에 쓰이는 응답). provider는 loopback의 fixture다. 끝나면 `down --volumes --remove-orphans --rmi local`로 지우고 임시 data 폴더도 지운다.
    - 확인하는 것(수용 조건 5의 둘째 항목):
        - `persistLogin: true`로 `/login`을 연 뒤 `close`하고 다시 `/whoami`를 열면 cookie 값이 보인다. `docker restart` 뒤에도 보인다.
        - Chromium 실행 인자에 Scope 2의 세 인자가 있고 `--disk-cache-size`가 없다. 시작할 때 탭은 `about:blank` 하나다.
        - HTTP cache의 위치: `/cached`를 연 뒤 브라우저가 떠 있는 동안 `<generation root>/cache/` 아래에 파일이 있고, 프로필 안에 HTTP cache 폴더(`Default/Cache`)가 없다. `close` 뒤에는 generation root가 없고(cache도 없다), `browser/profile/`은 0700으로 남아 있으며 `SingletonLock`이 없다. 이 확인은 cache가 꺼졌다는 증거가 아니라 프로필 밖에 있다가 닫을 때 지워진다는 증거다.
        - `docker kill` 뒤 같은 container를 다시 시작하면 브라우저가 뜬다(같은 hostname의 고아 lock). 이때 앞 generation의 `/tmp/shacs-browser-*`이 남아 있는지와 그 안에 `cache/`가 있는지를 기록한다(강제 종료 때 남는 것의 증거. 남아 있는 것이 예상 결과다).
        - 다른 hostname을 가리키는 `SingletonLock`을 일회용 data 폴더의 프로필에 만들어 두면 `browser_profile_locked`로 거절되고 프로필의 파일 목록과 그 lock이 그대로다. 같은 상태에서 `browser forget --yes`는 Scope 5의 "다른 곳에서 잠겨 있어" 글과 0이 아닌 종료 code로 끝나고 프로필이 그대로다. 그 lock을 직접 지운 뒤의 forget은 지운다.
        - 브라우저가 떠 있는 동안 `docker compose exec … shacs-bot browser forget --yes`가 브라우저를 닫고 `browser/profile/`을 지운다. 그 뒤 `/whoami`에 cookie가 없다.
        - api role container에서 한 `browser forget --yes`가 gateway role의 브라우저를 닫고 지운다(두 container 사이의 `flock`).
        - `persistLogin: false`로 띄우면 `--user-data-dir`가 임시 폴더이고 `--disk-cache-dir`와 `--password-store`가 없으며, 미리 둔 `browser/profile/`의 파일 목록이 그대로다. `browser/active.lock`은 생기고 `browser/profile.lock`은 생기지 않는다.
        - `persistLogin: false`로 브라우저가 떠 있는 동안 한 `browser forget --yes`가 그 브라우저를 닫고 미리 둔 `browser/profile/`을 지운다.
        - 저장된 프로필 없이 브라우저가 떠 있는 동안 한 `browser forget --yes`가 브라우저를 닫고 `브라우저를 닫았습니다.`와 `지울 로그인 기록이 없습니다.`를 출력한다. 그 뒤 세션의 다음 `browser` 호출이 `browser_closed`를 받는다.
        - 브라우저가 떠 있는 동안 `docker compose exec`로(어시스턴트를 거치지 않고) `browser/active.lock`을 지우고 새로 만들면 10초 안에 브라우저가 닫히고 inspect에 `browser_lock_replaced`가 남는다. `browser/`를 다른 이름으로 옮기고 새로 만든 때에도 같다.
        - fixture provider가 `exec`로 `rm <data 폴더>/browser/active.lock`과 `mv <data 폴더>/browser <다른 이름>`을 부르면 `bypass_permissions`에서도 거절되고, `browser/`와 lock 파일의 inode가 그대로이며 브라우저가 닫히지 않는다.

## Non Scope

- 사이트별·계정별·대화방별 프로필. 프로필은 하나다.
- 프로필 암호화, 비밀번호 저장소, 로그인 기록을 고르거나 일부만 지우는 것.
- 잠긴 프로필을 shacs-bot이 스스로 푸는 것.
- 자동화 탐지를 피하는 것.
- HTTP cache를 끄는 것, memory 파일 시스템에 두는 것, 강제 종료 뒤 남은 `/tmp/shacs-browser-*`을 찾아 지우는 것.
- file tool을 거치지 않는 스크립트가 프로필을 읽는 것을 막는 것(048과 같은 한계).
- 049의 임시 프로필 경로와 닫기의 변경.

## Parent Requirement Mapping

- [050 SPEC](../SPEC.md) 소유 범위 7, 8, 9 중 로그인 유지를 소유한다. 수용 조건 4, 5의 둘째 항목, 6(자기 변경)을 소유한다.
- SPEC "앞선 스펙에서 바꾸는 것" 중 049 프로필 행, 048 비밀 파일 목록 행, 048 브라우저 lock 경로 행(2026-11-11 두 번째 검토 뒤 더함), 그리고 이 PRD의 결정으로 더한 한 행(049의 닫기가 SIGKILL 하나인 것)을 구현한다.
- SPEC 소유 범위 7, "반드시 지킬 것", 수용 조건 4의 HTTP cache 줄(2026-11-11 고침: HTTP cache는 프로필 밖 generation 임시 폴더에 두고 닫을 때 지운다. 강제로 끝나면 남을 수 있다)은 Scope 2로 채운다. Chromium 156에는 보통 프로필의 HTTP cache를 끄는 길이 없으므로(결정 1) 이 PRD는 cache를 끈다고 말하지 않는다.
- SPEC 소유 범위 7의 "`browser forget`은 브라우저가 떠 있으면 닫은 뒤"는 프로필 mode와 관계없이 채운다(Scope 3·5). 로그인 유지를 끈 브라우저와 저장된 프로필이 없는 때의 브라우저도 닫는다.
- SPEC 소유 범위 7의 "닫힌 것을 lock 하나로 믿지 않는다"와 "다른 hostname을 가리키는 잠금이 있으면 지우지 않는다", 소유 범위 8의 "브라우저 lock 경로는 어시스턴트가 바꾸지 못한다"(2026-11-11 두 번째 검토 뒤 고침)는 Scope 3의 "같은 파일인지 보기", Scope 5의 3·4번, Scope 6으로 채운다.
- PRD 000의 닫기 경로·닫기 예약·닫힘 알림, PRD 010의 회차 끝내기를 소비한다. 049와 048 문서는 고치지 않는다.

## Required Contract

1. `persistLogin`이 꺼져 있으면 049의 Chromium 실행 인자와 닫기가 그대로이고 `browser/profile/`을 건드리지 않는다. 더해지는 것은 `browser/active.lock`과 `browser/` 폴더의 공유 잠금을 쥐는 것, `forget.lock`을 보는 것, lock 경로가 그대로인지 보는 것뿐이다.
2. 켜져 있으면 프로필은 data 폴더의 `browser/profile/` 하나이고 닫아도 남는다. HTTP cache는 꺼지지 않는다. 프로필 밖 generation 임시 폴더에 있고, 모든 닫기 길에서 그 폴더와 함께 지워진다. 강제로 끝나 남는 경우는 문서와 남은 제한에 적는다.
3. 한 프로필을 두 브라우저가 함께 쓰지 않는다.
4. 잠겼거나 쓸 수 없는 프로필에서는 브라우저를 띄우지 않고, shacs-bot은 프로필 안의 무엇도 고치거나 지우지 않는다. 지우는 것은 사용자의 `browser forget`뿐이다.
5. `browser forget`은 확인을 받고, 그 data 폴더로 뜬 브라우저를 프로필 mode와 관계없이 닫게 하며, 모두 닫힌 것을 확인한 뒤에만 지운다. 확인은 lock 하나에 기대지 않는다. lock 셋과 `browser/` 폴더의 배타 잠금을 쥐고, 쥔 파일이 그 경로의 파일과 같은지 보고, 프로필의 `SingletonLock`이 없거나 "띄워도 되는 lock"인지 본다. 하나라도 확인하지 못하면 지우지 않는다. 다른 hostname을 가리키는 `SingletonLock`이 있으면 지우지 않는다. 지울 프로필이 없어도 떠 있는 브라우저는 닫는다. forget이 도는 동안 새 브라우저는 뜨지 않는다.
   2026-11-12 전제 명시: 외부 작업이 프로필 폴더 자체를 Chromium 시작·삭제 도중 옮기거나 이름을 교체하는 경우는 보장 범위 밖이다(설계 결정 8). lock 파일 교체로 시작 중인 저장 프로필이 지워지는 경우는 폴더 descriptor의 lock으로 막는다.
6. `browser/profile/` 아래 전부는 어떤 mode와 규칙으로도 풀리지 않는 048 비밀 파일이다. data 폴더의 `browser` 폴더 자신과 lock 파일 셋은 어시스턴트가 어떤 mode와 규칙으로도 쓰거나 지우거나 옮기거나 바꿔 놓지 못한다. lock 경로가 바뀐 것을 본 브라우저는 스스로 닫는다.
7. inspect와 diagnostics는 프로필의 위치·크기·상태만 보인다.
8. Cargo 시험은 가짜 DevTools, 가짜 process 실행기, 시험용 시계, 임시 폴더만 쓰고 `Cargo.lock`은 바뀌지 않는다.

## Acceptance Criteria

1. `crates/shacs-cli/tests/it/spec050_browser_saved_login.rs`의 `persist_login_uses_data_profile_and_keeps_it`: 실제 `BrowserManager`, 가짜 process 실행기, 임시 data 폴더로 켠 채 띄우면 Chromium 인자가 Scope 2와 같고(`--disk-cache-dir`가 그 generation의 임시 폴더 안이고 프로필과 data 폴더 밖이며, `--disk-cache-size`가 없다) 환경 변수는 049와 같은지, `browser/`와 `browser/profile/`이 0700으로 생기는지, 닫은 뒤 프로필은 남고 임시 폴더는 없어지는지, 다음 generation이 같은 프로필과 새 cache 폴더로 뜨는지 확인한다.
2. 같은 파일의 `http_cache_stays_outside_profile_and_is_removed_on_close`: 가짜 process 실행기가 `--disk-cache-dir`가 가리키는 폴더에 파일을 쓴다. 닫기의 모든 길(`close` 동작, 쓰지 않아 닫힘, 답 기다리다 닫힘, `/browser close`, `/new`·`/stop`, forget, 종료 정리) 각각에서 닫힌 뒤 그 파일과 generation 임시 폴더가 없고 `browser/profile/` 아래에 그 파일이 생긴 적이 없는지 확인한다. 닫기 길을 타지 못하고 버려진 manager(강제 종료를 흉낸다) 뒤에는 그 폴더가 남고, 같은 data 폴더로 만든 새 manager가 그것을 지우지도 다시 쓰지도 않는지 확인한다(Scope 2에 적은 남는 범위를 그대로 보인다).
3. 같은 파일의 `persist_login_off_leaves_profile_untouched`: 끈 채 띄우면 Chromium 인자가 049와 같고(`--disk-cache-dir`, `--password-store`가 없다), 미리 둔 `browser/profile/`의 파일과 수정 시각이 그대로인지, `browser/active.lock`이 생기고 generation이 떠 있는 동안 다른 handle의 배타 `flock`이 잡히지 않다가 닫힌 뒤 잡히는지, `browser/profile.lock`은 생기지 않는지 확인한다.
4. 같은 파일의 `locked_or_unreadable_profile_refuses_start_without_changes`: 다른 hostname의 `SingletonLock`, 같은 hostname의 살아 있는 Chromium pid, 꼴이 다른 lock, symlink인 프로필 폴더, 다른 handle이 쥔 `profile.lock` 각각에서 process가 뜨지 않고 Scope 3의 글이 돌아오며 프로필이 그대로이고 `active.lock`이 다시 풀려 있는지 확인한다. 같은 hostname의 죽은 pid는 뜨는지 확인한다. 로그인 유지를 켠 때와 끈 때 각각, 다른 handle이 `forget.lock`을 배타로 쥐고 있거나 `active.lock`을 배타로 쥐고 있으면 `browser_forget_in_progress`로 거절되는지, `browser/`가 symlink면 `browser_state_unavailable`인지, 다른 handle이 lock을 300 ms 쥐었다 놓으면(시험용 시계) 거절되지 않고 뜨는지 확인한다.
5. 같은 파일의 `persist_generation_closes_gracefully_before_kill`: 닫기의 모든 길에서 가짜 DevTools가 `Browser.close`를 받은 뒤에 종료가 오는지, Chromium이 끝나지 않으면 시험용 시계로 5초 뒤 SIGKILL로 가는지, 임시 프로필 generation은 `Browser.close` 없이 049대로 끝나는지 확인한다.
6. 같은 파일의 `browser_forget_confirms_closes_and_deletes`: `--yes` 없는 비 terminal 실행은 거절, `n`은 지우지 않고 떠 있는 브라우저도 그대로 둠, `y`와 `--yes`는 지움을 확인한다. 로그인 유지 브라우저가 떠 있을 때 forget이 `forget.lock`을 쥐면 manager가 Scope 4로 닫고 `active.lock`과 `profile.lock`을 놓으며 그 뒤 프로필이 없어지는지, 그 세션의 다음 호출이 `browser_closed`를 받는지, forget이 도는 동안 새 generation이 `browser_forget_in_progress`로 거절되는지, manager가 놓지 않으면 시험용 시계로 15초 뒤 아무것도 지우지 않고 오류인지, 프로필이 symlink이거나 `SingletonLock`이 이 hostname의 살아 있는 Chromium을 가리키면 지우지 않는지, 다른 handle이 `forget.lock`을 쥐고 있으면 둘째 forget이 오류로 끝나는지 확인한다.
7. 같은 파일의 `browser_forget_closes_temporary_profile_browser`: `persistLogin`을 끈 채 브라우저가 떠 있고 미리 둔 `browser/profile/`이 있을 때 forget이 그 브라우저를 닫고(임시 프로필이라 `Browser.close` 없이 049의 닫기) 프로필을 지우는지, 출력이 `브라우저를 닫았습니다.`와 `로그인 기록을 지웠습니다.`인지, 그 세션의 다음 호출이 `browser_closed`를 받는지 확인한다. 답을 기다리는 중인 브라우저와 넘겨받은 중인 브라우저(회차가 `closed`로 끝난다)도 닫히는지, 동작이 실행 중이면 그 동작이 끝난 뒤 닫히는지 확인한다.
8. 같은 파일의 `browser_forget_without_saved_profile_closes_running_browser`: `browser/profile/`이 없고 로그인 유지를 끈 브라우저가 떠 있을 때 forget이 확인 글에 `지울 폴더: 없음`과 `떠 있는 브라우저: 닫습니다`를 보이고, 브라우저를 닫고, `브라우저를 닫았습니다.`와 `지울 로그인 기록이 없습니다.`를 출력하고 0으로 끝나는지 확인한다. 프로필도 브라우저도 없으면 묻지 않고 `지울 로그인 기록이 없습니다.`로 끝나며 `browser/` 폴더를 만들지 않는지 확인한다.
9. 같은 파일의 `inspect_reports_login_profile_without_contents`: inspect의 글·JSON과 diagnostics에 Scope 7의 값이 있고 프로필 안의 파일 이름이 없는지, inspect가 `browser/` 폴더나 lock 파일을 만들지 않는지 확인한다.
10. `crates/shacs-core/tests/it/spec050_login_profile_secret.rs`의 `login_profile_is_secret_in_every_mode`: `default`·`accept_edits`·`auto`·`bypass_permissions` 각각에서 `browser/profile/` 아래 파일이 목록과 검색 결과에 없고, file tool 읽기와 경로를 쓴 명령(`cat`, 상대 경로 포함)이 묻고, 쓰기·지우기가 거절되는지, 사용자 allow 규칙이 풀지 못하는지, 프로필 파일의 hard link가 같은 대우를 받는지, `browser/active.lock`·`browser/forget.lock`·`browser/profile.lock`은 목록에 나오고 읽기가 묻지 않는지(비밀이 아니다. 쓰기는 11번이 본다) 확인한다.
11. 같은 파일의 `browser_lock_paths_cannot_be_changed_in_any_mode`: `default`·`accept_edits`·`auto`·`bypass_permissions` 각각에서 `browser/active.lock`, `browser/forget.lock`, `browser/profile.lock`에 대한 file tool의 쓰기·고치기·지우기와 명령(`rm`, `mv`로 옮기기와 덮어쓰기, `ln -sf`, `touch`, `>` redirection, glob을 쓴 `rm <data 폴더>/browser/*.lock`, lock 파일 경로를 적은 `python -c` 글)이 `browser_lock_write`로 거절되는지 확인한다. `browser` 폴더 자신을 건드리는 명령(`mv <data 폴더>/browser <다른 이름>`, `rm -r <data 폴더>/browser`, `ln -sfn <다른 폴더> <data 폴더>/browser`)과 symlink를 거쳐 같은 곳에 닿는 경로, 상대 경로로 적은 경로도 거절되는지, lock 파일이 아직 없을 때에도 거절되는지, 사용자 allow 규칙과 기억된 허용이 풀지 못하는지 확인한다. 읽기(`cat`, `ls`)는 거절되지 않는지, `browser/` 밖의 같은 이름 파일(`<workspace>/active.lock`)은 걸리지 않는지, 048의 `UserOnlyFiles` 판정이 바뀌지 않았는지 확인한다.
12. `crates/shacs-cli/tests/it/spec050_browser_saved_login.rs`의 `replaced_lock_path_closes_browser_and_forget_keeps_profile`: 실제 `BrowserManager`, 가짜 process 실행기, 시험용 시계를 쓴다. 로그인 유지를 켠 때와 끈 때 각각 generation이 떠 있는 동안 시험이 file tool을 거치지 않고 lock 경로를 바꾼다. 경우는 `active.lock`을 지우고 새로 만들기, `forget.lock`을 다른 파일로 바꿔 놓기, `profile.lock`을 바꿔 놓기(켠 때), `browser/`를 다른 이름으로 옮기고 새 `browser/`를 만들기, `browser`를 symlink로 바꾸기다. 각각 시험용 시계 1초 뒤 `maintenance`가 generation을 닫고(켠 때는 `Browser.close`가 먼저다), 그 세션의 다음 호출이 `browser_closed`를 받고, inspect에 `browser_lock_replaced`가 남는지 확인한다. 바꾸지 않았으면 닫히지 않는지 확인한다. 로그인 유지 브라우저가 떠 있을 때 lock 파일이 바뀐 뒤 `maintenance`가 돌기 전에 한 forget은 아래에서 따로 본다. forget이 `active.lock`을 기다리는 동안 `forget.lock`이 바뀌면 forget이 `브라우저 lock 파일이 바뀌어 지우지 않았습니다. 다시 실행하세요.`로 끝나고 프로필이 그대로인지 확인한다.
    - 같은 hostname의 살아 있는 브라우저(lock 둘을 바꾼 때): 로그인 유지 브라우저가 떠 있을 때 시험이 `active.lock`과 `profile.lock`을 둘 다 지우고 같은 이름의 새 파일을 만든다. `active.lock`만 바꾸면 generation이 바뀌기 전의 `profile.lock`을 배타로 쥐고 있어 forget이 Scope 5의 3번에서 멈추고 `SingletonLock` 확인까지 가지 못하므로 둘을 다 바꾼다. 브라우저는 살아 있고 generation은 바뀌기 전의 두 파일을 쥔 채다. 시험용 시계를 움직이지 않아 `maintenance`가 돌기 전에, 브라우저를 가진 manager와 따로 forget을 한다.
        - forget이 멈춘 단계가 `browser_directory_busy`(Scope 5의 3번)인지 본다. 2026-11-12 독립 검토 뒤 추가한 공통 폴더 잠금이 SingletonLock 검사보다 먼저 살아 있는 브라우저를 확인한다. 기존 오류 글은 그대로이고 중단 단계로 순서를 구별한다.
        - 그때 forget이 쥔 `forget.lock`·`active.lock`·`profile.lock`의 device·inode가 경로에 있는 파일의 것과 같고, `active.lock`·`profile.lock`은 generation이 쥔 옛 파일의 것과 다른지 본다. 새 파일 셋은 잡았지만 살아 있는 generation이 쥔 폴더는 못 잡았다는 뜻이다.
        - `SingletonLock`은 이 hostname과, 가짜 process 실행기가 살아 있는 Chromium이라고 답하는 pid를 가리킨다. 종료 code가 0이 아니고 글이 `브라우저를 닫지 못해 지우지 않았습니다. 잠시 뒤 다시 하세요.`이며 `브라우저를 닫았습니다.`는 나오지 않는다. 살펴보기는 폴더 잠금으로 살아 있는 브라우저를 보았지만 종료 확인은 실패한 것이다. 프로필의 파일 목록·수정 시각과 `SingletonLock`이 그대로다.
        - 견줄 경우: 같은 준비에서 시험이 새 `profile.lock`을 먼저 배타로 쥐고 있으면 멈춘 단계가 `profile_lock_busy`다. forget이 경로의 새 파일을 잡으려 한다는 것을 따로 보인다.
        - 별도로 `browser/` 폴더까지 옮긴 뒤 같은 프로필 inode를 새 폴더 아래로 옮긴 경우도 본다. 새 browser 폴더의 잠금까지 잡히더라도 기존 SingletonLock 검사가 `singleton_live`로 거절하고 프로필을 보존한다. 다른 hostname을 가리키는 잠금의 `singleton_elsewhere` 시험도 그대로 유지한다.
13. 같은 파일의 `browser_forget_refuses_profile_locked_by_other_host`: 다른 container의 브라우저가 그 프로필을 쓰는 상황을 만든다. `browser/profile/SingletonLock`이 다른 hostname의 pid를 가리키고, 이 data 폴더의 lock 셋은 아무도 쥐고 있지 않다(그 브라우저가 바뀌기 전의 lock 파일을 쥐고 있는 경우와 `flock`이 container 사이에 듣지 않는 경우를 흉낸다). `browser forget --yes`가 Scope 5의 "다른 곳에서 잠겨 있어" 글과 0이 아닌 종료 code로 끝나고, 프로필의 파일 목록·수정 시각과 `SingletonLock`이 그대로인지 확인한다. 이 process에 임시 프로필 브라우저가 떠 있었으면 그것은 닫히고 `브라우저를 닫았습니다.`가 먼저 나오는지, symlink가 아닌 `SingletonLock`과 꼴이 다른 lock에서도 지우지 않는지, 그 lock을 지운 뒤 다시 한 forget은 지우는지 확인한다.
14. `crates/shacs-config/tests/it/spec050_browser_config.rs`의 `persist_login_default_off`: 기본이 `false`이고 onboard가 기존 값을 보존하는지 확인한다. 새 파일은 기존 `tests/it/main.rs`에만 등록한다.
15. 사용자 승인 뒤 `scripts/spec050-saved-login-check.sh --approved`가 Scope 10을 확인한다(부모 수용 조건 5의 둘째 항목). 미실행 결정이면 이유와 미검증 범위(실제 Chromium에서의 cookie 유지, HTTP cache가 실제로 프로필 밖에 생기고 닫을 때 지워지는지, 강제 종료 뒤 남는 범위, `Browser.close` 뒤 lock 정리, container 사이의 `flock`, 로그인 유지를 끈 브라우저의 forget, 다른 hostname 잠금에서의 forget 거절, lock 경로가 바뀐 때 브라우저가 닫히는지, 실제 `exec` 명령에서의 lock 경로 쓰기 거절)를 명시한다.
16. fmt → workspace clippy `-D warnings` → 변경 package의 `cargo test --manifest-path crates/Cargo.toml -p <package> -q`를 `--locked`로 실행한다. 전체 workspace test는 사용자 승인 뒤에만 한다. 사용 문서와 `Cargo.lock` 무변경을 확인한다.
17. (2026-11-12 P1) 같은 CLI 파일의 `start_refuses_profile_deleted_before_directory_flock`: 시험의 별도 descriptor가 프로필 폴더 잠금을 쥐어 시작을 open 뒤 flock 재시도에서 멈춘다. 시계의 wait hook이 그 잠금을 놓고 active·profile 파일을 교체한 뒤 forget을 끝까지 실행한다. 그 뒤 경로가 없는 경우와 새 폴더를 둔 경우 모두 시작이 `browser_profile_unreadable`, child spawn 수가 0이어야 한다. 재생성·재시도하지 않는다.
18. (2026-11-12 P2) 같은 CLI 파일의 `forget_waits_for_temporary_browser_after_active_lock_replacement`: 임시 프로필 브라우저가 열린 뒤 active 파일만 교체하고 maintenance 시계는 움직이지 않은 상태에서 forget을 시작한다. 저장 프로필이 있을 때와 없을 때 모두 닫기를 안내하고, maintenance가 닫고 임시 폴더까지 정리한 뒤에만 성공해야 한다. 새 active 파일에서 5초, browser 폴더에서 나머지 10초를 기다려도 종료 확인을 못 하면 합계 15초에 `browser_directory_busy`로 실패하고 프로필을 보존한다. forget의 폴더 배타 잠금 중에는 두 mode의 새 시작이 500 ms 뒤 `browser_forget_in_progress`, child spawn 0이어야 한다.

## Closure Evidence

1. 켠 때와 끈 때의 Chromium 인자와 프로필·임시 폴더의 전후 목록. HTTP cache 폴더가 임시 폴더 안에 있다가 닫힌 뒤 없어진 기록과, 닫기 길을 타지 못한 때 남은 폴더의 기록.
2. 거절 표(걸린 것 → 결과 글)와 프로필이 바뀌지 않았다는 증거.
3. `Browser.close`가 종료보다 먼저 간 기록과 5초 뒤 강제 종료.
4. `browser forget`의 확인 글, 닫힘, 지워진 폴더, 지우지 않은 경우들(다른 hostname 잠금, 바뀐 lock 파일 포함). 로그인 유지를 끈 브라우저와 저장된 프로필이 없는 브라우저가 닫힌 기록.
5. mode별 비밀 파일 결과 표와 lock 경로 쓰기 거절 표. lock 경로가 바뀐 때 브라우저가 닫힌 기록.
6. inspect·diagnostics 예.
7. Docker 확인 기록 또는 미실행 결정과 미검증 범위, Cargo 검증 기록, 사용 문서와 `Cargo.lock` 확인.

PRD 020이 닫히면 050의 세 PRD가 모두 닫힌다.

### 2026-11-12 구현과 시험 대응표

아래 표는 첫 구현 당시의 줄 번호와 근거다. 독립 검토 뒤 P1·P2의 현재 위치와 검증은 아래 보완 기록에 적는다. 약칭은 다음 파일을 가리킨다.

- `login`: `crates/shacs-cli/tests/it/spec050_browser_saved_login.rs`
- `lock`: `crates/shacs-cli/src/browser/browser_lock.rs`
- `forget`: `crates/shacs-cli/src/browser/browser_forget.rs`
- `directory`: `crates/shacs-cli/src/browser/browser_profile_directory.rs`
- `manager`: `crates/shacs-cli/src/browser/browser_manager.rs`
- `process`: `crates/shacs-cli/src/browser/browser_process.rs`
- `state`: `crates/shacs-cli/src/browser/browser_state.rs`
- `secret`: `crates/shacs-core/src/runtime/auto_mode/secret_file.rs`
- `secret_test`: `crates/shacs-core/tests/it/spec050_login_profile_secret.rs`

| Required Contract | 구현 | 확인 |
|---|---|---|
| 1 꺼진 때의 기존 인자·임시 프로필 | `lock:358`, `process:331`, `manager:888` | `login:1141`, `login:1356` |
| 2 저장 프로필과 임시 HTTP cache | `process:366`, `manager:1151` | `login:977`, `login:1075` |
| 3 한 프로필의 배타 사용 | `lock:358`, `directory:11` | `login:1075`, `login:1201`, `login:1663` |
| 4 쓸 수 없거나 잠긴 프로필 보존 | `lock:202`, `lock:256`, `lock:358` | `login:1201`, `login:2119` |
| 5 확인·닫기·삭제와 시작 중 경합 | `forget:140`, `forget:195`, `directory:146`, `manager:837` | `login:1406`, `login:1663`, `login:1708`, `login:1802`, `login:1915`, `login:1996`, `login:2119`; 설계 결정 8의 전제 |
| 6 비밀 파일·lock 경로 보호 | `secret:143`, `secret:192`, `crates/shacs-core/src/runtime/auto_mode/permission_materials.rs:274`, `crates/shacs-core/src/runtime/auto_mode/auto_decision.rs:284` | `secret_test:71`, `secret_test:314`, `secret_test:415`, `login:1996` |
| 7 읽기 전용 inspect·제한된 diagnostics | `lock:435`, `lock:488`, `state:346`, `crates/shacs-cli/src/lib.rs:4682` | `login:1832`, `login:1976` |
| 8 가짜 실행기·시계와 기존 의존성 | `login:99` 이후 fixture, `secret_test:29` | Cargo 명령 기록, Cargo.lock SHA-256 무변경; Docker 미실행 |

| Acceptance Criterion | 구현과 시험 |
|---|---|
| 1 저장 경로·인자·환경·권한·다음 generation | `process:366`, `lock:358` → `login:977` |
| 2 모든 닫기의 cache 정리와 강제 종료 흔적 | `manager:1151`, `process:390` → `login:1075` |
| 3 꺼진 때 저장 프로필 무변경 | `lock:358`, `process:331` → `login:1141` |
| 4 시작 거절과 500 ms 재시도 | `lock:142`, `lock:202`, `lock:358` → `login:1201` |
| 5 정상 종료 먼저, 5초 뒤 강제 종료 | `manager:888` → `login:1356` |
| 6 확인·닫기·삭제·15초·거절 | `forget:140`, `forget:195` → `login:1406` |
| 7 임시·답 기다림·넘겨받기·동작 중 브라우저의 forget | `manager:837`, `manager:1151`, `forget:195` → `login:1708` |
| 8 프로필 없이도 닫기, 모두 없으면 무변경 | `forget:140` → `login:1802` |
| 9 inspect 글·JSON·diagnostics, 파일을 만들지 않음 | `lock:488`, `state:346`, `crates/shacs-cli/src/lib.rs:4682` → `login:1832` |
| 10 모든 mode의 비밀 파일과 hard link | `secret:143`, `secret:192` → `secret_test:71` |
| 11 모든 mode의 lock 쓰기 거절, 기존 marker 유지 | `crates/shacs-core/src/runtime/auto_mode/permission_environment.rs:71`, `crates/shacs-core/src/runtime/auto_mode/permission_materials.rs:274`, `crates/shacs-core/src/runtime/auto_mode/auto_decision.rs:284` → `secret_test:314`, `secret_test:415` |
| 12 경로 교체 뒤 닫기·SingletonLock 보호 | `manager:837`, `forget:195` → `login:1996`; 시작 전 빈틈은 `login:1663`, 열린 identity의 삭제는 `login:1915`, 옮긴 사용 중 프로필 보존은 `login:1951` |
| 13 다른 hostname 잠금 거절 | `lock:256`, `forget:195` → `login:2119` |
| 14 기본 꺼짐과 onboard 보존 | `crates/shacs-config/src/browser_config.rs:24`, `crates/shacs-cli/src/lib.rs:3513` → `crates/shacs-config/tests/it/spec050_browser_config.rs:29`, `login:1832`의 실제 onboard 호출 |
| 15 opt-in Docker | `scripts/spec050-saved-login-check.sh:1`, `scripts/spec050-saved-login-fixtures/check.py:1`; 사용자 지시로 미실행 |
| 16 fmt·workspace clippy·package 시험·문서 | 아래 검증 기록과 `docs/USAGE.md`의 브라우저 로그인 유지, README |

실제 생성 경로: CLI dispatch는 `crates/shacs-cli/src/lib.rs:2937`에서 `browser_cli::run`을 부른다. data 폴더 명령 분류는 `data_folder_access.rs:66`이다. 브라우저 도구 등록은 `spec035_cli_media/tool_registry.rs:54`에서 `manager:2568`의 `process_browser_host`로 가며 `ConfigBrowserStartup`·실제 Linux 실행기·data 폴더를 넣는다. serve는 `lib.rs:11262`, gateway의 외부 channel 없는/있는 갈래는 `lib.rs:12122`, `lib.rs:12140`에서 이어 가기 알림을 설치한다. `lib.rs:17259`의 web adapter는 브라우저 도구를 등록하지 않는다. 이 배선을 시험 전용 생성 경로로 바꾸지 않았다.

### 2026-11-12 실패 확인

각 항목을 하나씩 끄고 아래 시험의 실제 동작 실패(exit 101)를 확인한 뒤 원문으로 복구했다. 컴파일 오류는 실패 증거로 세지 않았다.

| 잠시 끈 부분 | 실패한 시험 | 관찰 |
|---|---|---|
| 프로필 폴더 배타 lock | `forget_cannot_delete_profile_before_chromium_creates_singleton` | 시작 중 프로필이 `Removed`로 삭제됨 |
| 열린 identity 대신 profile 경로를 다시 열어 삭제 | `forget_traverses_only_the_opened_profile_after_path_replacement` | 새 프로필의 cookie 파일이 없어짐 |
| 크기 계산의 최상위 symlink 거절 | `profile_size_does_not_follow_links_and_stops_at_twenty_thousand_files` | 0이어야 할 크기가 외부 폴더의 1546 bytes |
| 프로필이 없을 때 forget lock의 `in_use` | `inspect_reports_login_profile_without_contents` | `InUse` 대신 `Absent` |
| 저장 generation의 정상 종료 요청 | `persist_generation_closes_gracefully_before_kill` | assistant close에서 정상 종료 순서 누락 |
| maintenance의 lock 감시 | `replaced_lock_path_closes_browser_and_forget_keeps_profile` | 닫힘 상태에 도달하지 못함 |
| diagnostics에서 profile 경로·크기 제거 | `inspect_reports_login_profile_without_contents` | 허용된 세 값 외에 경로·크기가 노출됨 |
| 모든 mode의 lock 쓰기 거절 | `browser_lock_paths_cannot_be_changed_in_any_mode` | 쓰기가 승인 질문 또는 실행으로 넘어가며 bypass에서 lock 파일이 바뀜 |
| 프로필 경로의 비밀 파일 판정 | `login_profile_is_secret_in_every_mode` | 폴더 삭제가 승인 질문으로 넘어가고 이동이 실행됨 |
| 설정을 저장 프로필 선택에 연결 | `persist_login_uses_data_profile_and_keeps_it` | 저장 인자 대신 임시 프로필 인자 |
| 다른 hostname SingletonLock 거절 | `browser_forget_refuses_profile_locked_by_other_host` | `SingletonElsewhere` 대신 `Removed` |

같은 시작 경합은 수정 전에도 재현했다. 잠금을 잡은 뒤 첫 fake child 실행에서 channel로 멈추므로 시간 운에 기대지 않는다. lock 쓰기 실패 출력이 길어 그 항목만 전체 로그를 보관하는 명령으로 한 번 더 확인했다. 서로 다른 수정 11개, 실패 확인 실행은 12번이며 수정 전 재현 1번은 별도다.

### 2026-11-12 검증 결과와 미검증 범위

- `cargo fmt --manifest-path crates/Cargo.toml --all -- --config imports_granularity=Module,format_macro_matchers=true`: 성공. 기존 기본값 시험의 JSON에 `persistLogin: false`를 더한 뒤 `cargo fmt --manifest-path crates/Cargo.toml --all -- --check --config imports_granularity=Module,format_macro_matchers=true`도 성공.
- `cargo clippy --manifest-path crates/Cargo.toml --workspace --all-targets --locked -- -D warnings`: 성공(마지막 실행 13.69초). 의존성 `imap-proto v0.10.2`의 기존 future-incompatibility 안내는 남아 있다. lint 오류는 없다.
- `cargo test --manifest-path crates/Cargo.toml -p shacs-cli --test it -q --locked spec050_browser_saved_login`: 최종 코드에서 **연속 10회**, 매번 15 passed. 준비 중에는 미완성 시험의 결과형 두 곳을 바로잡았고, 12개 시험 통과 후 세 시험을 더해 15개로 마쳤다.
- `cargo test --manifest-path crates/Cargo.toml -p shacs-core --test it -q --locked spec050_login_profile_secret`: 최종 코드에서 **연속 10회**, 매번 3 passed.
- `cargo test --manifest-path crates/Cargo.toml -p shacs-config --test it -q --locked spec050_browser_config`: 3 passed.
- `cargo test --manifest-path crates/Cargo.toml -p shacs-config -q --locked`: 최종 전체 성공, 124 passed. 첫 실행은 `spec049_browser_config::browser_defaults_and_limits`의 공개 JSON 기대값에 새 키가 빠져 실패했다. 기존 비교를 유지하고 `persistLogin: false`를 추가해 고쳤다. 049 문서는 고치지 않았다.
- `cargo test --manifest-path crates/Cargo.toml -p shacs-cli -q --locked`: 전체 성공(exit 0). 단위 472, 통합 `it` 186과 나머지 실행 파일이 통과했다. 원래 ignored인 3개는 그대로다.
- `cargo test --manifest-path crates/Cargo.toml -p shacs-core -q --locked`: 첫 실행에서 단위 **503 passed**(654.34초), 앞선 통합 실행 파일들이 통과했다. `it`의 `spec034_remote_output::decision_contract::live_reference_contains_safe_provider_domain_and_future_expiry`, `spec034_remote_output::explicit_future_reference_contains_no_raw_url`가 실패로 표시된 뒤 도구가 실행 세션을 `killed (exit 1)`로 종료했다. 이 작업이 종료 명령을 보내지는 않았다. 실패 상세가 나오기 전의 중단이라 그 원인을 단정하지 않는다. 두 시험과 해당 제품 코드는 수정하지 않았다.
- 중단 후 `cargo test --manifest-path crates/Cargo.toml -p shacs-core --test it -q --locked`: **633 passed, 기존 ignored 2**, exit 0. 앞의 두 시험도 통과했다. 이미 통과한 단위 시험은 반복하지 않았다. 아직 실행하지 않은 60개 통합 실행 파일은 아래 명령으로 모두 성공(exit 0)했고, 원래 ignored인 3개는 그대로다. `cargo test --manifest-path crates/Cargo.toml -p shacs-core --doc -q --locked`도 2 passed다. 따라서 core의 기본 전체 시험 범위는 나누어 마쳤지만, 최초 전체 명령 자체를 성공했다고 기록하지 않는다.
- 모든 Cargo 명령은 한 번에 하나만 실행했고 `--manifest-path crates/Cargo.toml`을 사용했다. 도구가 긴 foreground 호출을 자동 분리한 경우에도 종료 결과를 받은 뒤 다음 Cargo를 실행했다. `CARGO_TARGET_DIR`를 설정하지 않았고 전체 workspace test는 실행하지 않았다.
- `Cargo.lock`의 전후 SHA-256은 `0889a78ceda87f6aa082ec005cd8757a9bd0a1c395f2c1c822081dad07f21256`로 같다. crate와 의존성을 더하지 않았다. git 상태를 바꾸는 명령이나 commit은 하지 않았다. 테스트 내부의 일회용 저장소 작업은 기존 시험의 동작이다.
- `bash -n scripts/spec050-saved-login-check.sh`와 Python fixture 세 파일의 `ast.parse`는 성공했다. 스크립트 자체와 Docker는 사용자 지시로 실행하지 않았다. 실제 Chromium cookie 유지, HTTP cache 실제 위치·정리·강제 종료 흔적, Browser.close 후 SingletonLock 정리, container 사이 directory/file flock과 forget, 임시 프로필·무프로필 forget, 외부 hostname 잠금, lock 교체 뒤 닫기, 실제 exec 거절은 미검증이다. Unix descriptor 삭제는 macOS의 실제 파일 시스템에서 시험했다. Linux 전용 errno 분기와 Miri·sanitizer는 검증하지 않았다.
- PRD 000·010의 보호할 구현은 바꾸지 않았다. 이어받을 때와 비교해 `browser_manager.rs`, CLI `lib.rs`, `permission_policy.rs`의 내용도 그대로이며, 기존 `secret_file.rs`에는 지정 formatter의 공백 정리만 생겼다. 새 진단 배선을 따로 만들지 않고 이미 연결돼 있던 실제 inspect·diagnostics를 시험으로 확인했다.

실패 확인에서 실행한 Cargo 명령은 다음과 같다(각각 exit 101, 이후 코드 복구). 같은 inspect 시험은 서로 다른 두 보호를 끈 상태에서 각각 실행했다.

```sh
cargo test --manifest-path crates/Cargo.toml -p shacs-cli --test it -q --locked forget_cannot_delete_profile_before_chromium_creates_singleton
cargo test --manifest-path crates/Cargo.toml -p shacs-cli --test it -q --locked forget_traverses_only_the_opened_profile_after_path_replacement
cargo test --manifest-path crates/Cargo.toml -p shacs-cli --test it -q --locked profile_size_does_not_follow_links_and_stops_at_twenty_thousand_files
cargo test --manifest-path crates/Cargo.toml -p shacs-cli --test it -q --locked inspect_reports_login_profile_without_contents
cargo test --manifest-path crates/Cargo.toml -p shacs-cli --test it -q --locked persist_generation_closes_gracefully_before_kill
cargo test --manifest-path crates/Cargo.toml -p shacs-cli --test it -q --locked replaced_lock_path_closes_browser_and_forget_keeps_profile
cargo test --manifest-path crates/Cargo.toml -p shacs-core --test it -q --locked browser_lock_paths_cannot_be_changed_in_any_mode
cargo test --manifest-path crates/Cargo.toml -p shacs-core --test it -q --locked login_profile_is_secret_in_every_mode
cargo test --manifest-path crates/Cargo.toml -p shacs-cli --test it -q --locked persist_login_uses_data_profile_and_keeps_it
cargo test --manifest-path crates/Cargo.toml -p shacs-cli --test it -q --locked browser_forget_refuses_profile_locked_by_other_host
```

core의 중단 뒤 남은 실행 파일 명령은 다음과 같다.

```sh
cargo test --manifest-path crates/Cargo.toml -p shacs-core -q --locked --no-fail-fast \
  --test permission_action \
  --test permission_policy \
  --test plugin_runtime \
  --test runtime \
  --test runtime_agent \
  --test runtime_loop \
  --test runtime_workflow \
  --test spec030_classifier_baseline \
  --test spec030_diagnostics_aggregate \
  --test spec030_exec_config \
  --test spec030_fact_store \
  --test spec030_javascript_tool_before_exec \
  --test spec030_local_provider \
  --test spec030_process_controlled_child \
  --test spec030_process_fact_scope \
  --test spec030_provider_credentials_command \
  --test spec030_provider_credentials_oauth \
  --test spec030_provider_credentials_transport \
  --test spec030_resource_load_checks \
  --test spec030_resource_selection \
  --test spec030_sandbox_adapter \
  --test spec030_sandbox_invalid_plan_fallback \
  --test spec030_startup_facts \
  --test spec030_tool_before \
  --test spec030_tool_before_confirmation \
  --test spec030_tool_before_interaction \
  --test spec030_tool_before_preparation \
  --test spec030_tool_before_timeout \
  --test spec030_trace_disclosure \
  --test spec030_trusted_runtime \
  --test spec031_activation_execution \
  --test spec031_activation_store \
  --test spec031_context_projection \
  --test spec031_execution_snapshot \
  --test spec031_sequential_integration \
  --test spec033_automation_dispatch \
  --test spec033_automation_gates \
  --test spec033_automation_rejection \
  --test spec033_automation_results \
  --test spec033_automation_service \
  --test spec033_goal_accounting \
  --test spec033_local_improvement_process \
  --test spec033_local_improvement_support \
  --test spec033_production_local_gate \
  --test spec033_release_closure \
  --test spec033_release_edges \
  --test spec033_release_runner \
  --test spec033_self_improvement_live \
  --test spec033_snapshot_replay \
  --test spec033_snapshot_replay_support \
  --test spec034_image_edit \
  --test spec034_persistence_operations \
  --test spec034_sequential_integration \
  --test spec035_tasks_assembler \
  --test spec036_action_decision \
  --test spec036_entrusted_work \
  --test spec036_entrusted_work_compat \
  --test spec036_notification \
  --test spec036_observation \
  --test tools
```

### 2026-11-12 독립 검토 뒤 P1·P2 보완 결과

Status는 `Implemented (independent review pending)` 그대로다. 이번 변경 package는 `shacs-cli` 하나다. 수정한 Rust 파일은 `src/browser/browser_lock.rs`, `src/browser/browser_forget.rs`, `tests/it/spec050_browser_saved_login.rs`다. 이 PRD·SPEC 050·`docs/USAGE.md`도 함께 고쳤다. 앞선 구현의 다른 산출물과 Docker 스크립트는 바꾸거나 실행하지 않았다.

| 지적 | 수정과 현재 위치 | 새 시험과 관찰 |
|---|---|---|
| P1: 지워진 프로필 descriptor를 잠근 뒤 시작 | `crates/shacs-cli/src/browser/browser_lock.rs:361`의 `acquire`: 프로필 폴더 flock 직후 fstat/lstat identity 확인 | `crates/shacs-cli/tests/it/spec050_browser_saved_login.rs:1663`의 `start_refuses_profile_deleted_before_directory_flock`. open 뒤 flock 재시도의 clock hook에서 forget을 완료한다. 경로가 없는 경우와 새 inode가 있는 경우 모두 `ProfileUnreadable`, child spawn 0. |
| P2: active 파일 교체 뒤 임시 브라우저를 놓침 | `browser_lock.rs:446`의 `hold_browser_directory`, `browser_lock.rs:519`의 살펴보기, `browser_forget.rs:197`의 공통 15초 종료 확인 | 같은 시험 파일 `:1713`의 `forget_waits_for_temporary_browser_after_active_lock_replacement`. 저장 프로필 유무 두 경우 모두 1초 maintenance와 실제 정리 뒤 성공. active에서 5초를 썼으면 폴더에서 10초만 더 기다리고 실패하며, 폴더 배타 잠금 중 두 mode의 새 시작은 거절. |

기존 시험은 약하게 만들지 않고 더 이른 종료 확인에 맞췄다. 첫 spawn 앞의 보호 시험(`:1819`)은 lock 파일만 교체하면 `browser_directory_busy`, browser 폴더까지 교체하고 같은 프로필 inode를 옮기면 기존 `profile_directory_busy`를 확인한다. 기존 lock 교체 시험(`:2168`)도 새 공통 폴더 거절과 별도의 `singleton_live` 거절을 모두 확인한다. 옮긴 프로필 보존 시험(`:2123`)은 그 프로필의 내용 보존에 더해 기존 브라우저가 먼저 닫힌 것도 요구한다.

실패 확인:

| 잠시 비활성화한 보호 | 실행한 새 시험 | 결과 |
|---|---|---|
| P1 잠금 뒤 profile identity 비교 | `start_refuses_profile_deleted_before_directory_flock` | `Err(ProfileUnreadable)` 대신 `Ok(())`, exit 101 |
| P2 generation의 browser 폴더 공유 잠금 | `forget_waits_for_temporary_browser_after_active_lock_replacement` | `BrowserDirectoryBusy` 대신 `NothingToRemove`, exit 101 |
| P2 살펴보기의 browser 폴더 잠금 확인 | 같은 P2 시험 | 1초 종료 기다림 대신 0 ms 성공, exit 101 |
| P2 삭제 전 browser 폴더 배타 잠금 기다림 | 같은 P2 시험 | `BrowserDirectoryBusy` 대신 `NothingToRemove`, exit 101 |

모두 원문으로 복구했다. 수정 전에도 새 두 시험이 각각 같은 경합으로 실패했다. 실패 확인에는 다음 두 명령을 사용했다(P1 1회, P2 3회; 수정 전 재현은 각각 별도 1회).

```sh
cargo test --manifest-path crates/Cargo.toml -p shacs-cli --test it -q --locked start_refuses_profile_deleted_before_directory_flock
cargo test --manifest-path crates/Cargo.toml -p shacs-cli --test it -q --locked forget_waits_for_temporary_browser_after_active_lock_replacement
```

복구한 최종 코드의 검증:

```sh
cargo fmt --manifest-path crates/Cargo.toml --all -- --config imports_granularity=Module,format_macro_matchers=true
cargo fmt --manifest-path crates/Cargo.toml --all -- --check --config imports_granularity=Module,format_macro_matchers=true
cargo clippy --manifest-path crates/Cargo.toml --workspace --all-targets --locked -- -D warnings
cargo test --manifest-path crates/Cargo.toml -p shacs-cli --test it -q --locked spec050_browser_saved_login
cargo test --manifest-path crates/Cargo.toml -p shacs-core --test it -q --locked spec050_login_profile_secret
cargo test --manifest-path crates/Cargo.toml -p shacs-cli -q --locked
```

- fmt와 check 성공. workspace clippy 성공(4.26초); 기존 의존성 `imap-proto v0.10.2`의 future-incompatibility 안내는 남아 있다.
- 저장 로그인 묶음은 **연속 10회**, 매번 **17 passed**. 비밀 파일 묶음도 **연속 10회**, 매번 **3 passed**.
- 변경한 package의 전체 시험은 exit 0: 단위 **472 passed**, `it` **188 passed**, 나머지 실행 파일과 문서 시험도 통과. 기존 ignored 3개는 그대로다. `shacs-core`와 `shacs-config`는 이번에 바꾸지 않아 전체 시험을 반복하지 않았다.
- 새 production test hook·의존성·unsafe·thread·catch_unwind는 추가하지 않았다. 기존 시계의 wait hook과 실제 flock만으로 재현했다.
- `loop_control.rs`, `durable_work.rs`, CLI `lib.rs`, `browser_manager.rs`, `surface_approval_worker.rs`, `automation_worker.rs`, core `tool_execution.rs`, `browser_viewer.rs`, `browser_profile_directory.rs`와 Cargo.lock의 전후 SHA-256이 같다. 따라서 예약 주인·실행 후보·공유 lease owner·ThreadEndSignal·Live·알림 acknowledge·회차와 재개 경로에 직접 변경이 없다.
- Cargo는 한 번에 하나씩 foreground로 요청했다. 전체 시험의 도구 자동 분리 뒤에도 종료를 확인하기 전에 turn을 끝내거나 다른 Cargo를 시작하지 않았다. `CARGO_TARGET_DIR`, Docker, workspace 전체 test, git 변경 명령은 사용하지 않았다. Docker 미실행 범위와 폴더 경로 자체 교체의 기존 제한은 그대로다.

## 남은 제한

- 기한 없는 session cookie는 브라우저가 닫히면 없어진다. 그런 cookie만 쓰는 사이트는 다시 로그인해야 한다.
- cookie는 사실상 평문으로 디스크에 있다. 지키는 것은 폴더 권한과 048 비밀 파일 규칙이다. 049처럼 Chromium sandbox가 꺼져 있어 브라우저를 뚫은 페이지는 읽을 수 있다.
- 브라우저가 강제로 죽으면(container kill, OOM) 마지막 30초 안의 cookie가 없어질 수 있고 `SingletonLock`이 남는다. 같은 container를 다시 시작하면 스스로 풀리지만, container를 새로 만들면 hostname이 달라 `browser_profile_locked`로 거절된다. 이때는 `browser forget`도 지우지 않는다(Scope 5의 4번). 사용자가 다른 브라우저가 그 프로필을 쓰지 않는 것을 확인하고 `Singleton*`을 직접 지워야 한다. compose의 hostname을 고정하는 것과 forget에 강제 선택지를 두는 것은 이 PRD가 하지 않는다(사용자 결정이 있어야 한다).
- 로그인 유지 generation의 닫기는 5초까지 걸릴 수 있다. `/stop`과 `/browser close`의 답이 그만큼 늦을 수 있다.
- macOS Docker Desktop의 bind mount 위에 프로필이 있어 Chromium의 SQLite가 느릴 수 있고, `flock`이 container 사이에서 지켜지는지는 Docker의 파일 공유 방식에 달려 있다. Scope 10이 한 구성에서 확인할 뿐이다.
- HTTP cache는 꺼지지 않는다. Chromium 156에는 보통 프로필에서 끄는 길이 없다(결정 1). 로그인 유지 브라우저가 떠 있는 동안 로그인한 계정의 응답이 container의 `/tmp/shacs-browser-*/cache/`에 있다. 이 폴더는 048 비밀 파일이 아니고 지키는 것은 0700 권한뿐이다.
- shacs-bot이 강제로 끝나면(SIGKILL, OOM, `docker kill`, 전원) 그 폴더가 HTTP cache와 함께 남는다. `docker restart` 뒤에도 있고 container를 새로 만들어야 없어진다. 이 PRD는 남은 폴더를 찾아 지우지 않는다. 시작할 때 지우기와 cache용 tmpfs는 049의 계약(임시 폴더의 수명, compose 구성)을 바꾸므로 사용자 결정이 있어야 한다.
- `--disk-cache-dir`가 옮기는 범위는 tag `156.0.8078.79`의 source와 Scope 10의 Docker 확인으로만 안다. 프로필에는 다른 cache(Code Cache, GPU cache, 사이트의 Cache Storage 등)가 남을 수 있다. 그래서 프로필 전체가 비밀 파일이다.
- `browser` 폴더와 lock 파일 셋은 어시스턴트가 바꾸지 못하지만, 이 보호는 048처럼 file tool과 명령 분석이 경로를 찾은 명령에서만 듣는다. 경로를 적지 않는 스크립트, data 폴더 자신이나 그 위 폴더를 옮기는 명령, 사용자가 직접 한 변경은 잡지 못한다. `browser/` 폴더 자체나 그 상위 경로가 바뀌면 브라우저가 1초마다 하는 확인으로 스스로 닫히지만, 그 1초와 닫는 시간(로그인 유지 generation은 5초까지) 동안 forget은 떠 있는 임시 프로필 브라우저를 보지 못하고 `브라우저를 닫았습니다.` 없이 끝날 수 있다. 그 브라우저는 저장된 프로필을 쓰지 않고 곧 스스로 닫힌다. 로그인 유지 브라우저의 프로필은 폴더 descriptor의 배타 lock과 `SingletonLock` 확인이 함께 지킨다. 프로필 폴더 자체의 동시 이름 교체는 설계 결정 8에 적은 보장 범위 밖이다. lock 파일만 교체한 경우에는 모든 generation의 browser 폴더 공유 잠금이 남으므로 forget이 닫힘 확인을 건너뛰지 못한다(설계 결정 9).
- lock 파일을 열 수 있는 process는 `flock`으로 쥐고 있을 수 있다(읽기로 열어도 된다). `forget.lock`을 쥐면 브라우저가 닫히고 뜨지 않으며, `active.lock`을 쥐면 forget이 지우지 않고 끝난다. 어느 쪽도 떠 있는 브라우저 밑에서 프로필을 지우게 하지는 못한다. 쥔 process가 끝나면 풀린다.
- 다른 container에서 한 forget이 브라우저를 닫았는데 Chromium이 5초 안에 끝나지 않아 강제로 끝났으면 `SingletonLock`이 그 container의 hostname으로 남는다. 그 forget은 "다른 곳에서 잠겨 있어" 글로 끝난다. 브라우저를 띄웠던 container에서 forget을 다시 하면 지워진다.
- 로그인 유지를 꺼도 브라우저를 띄우면 data 폴더에 `browser/`와 빈 lock 파일이 생긴다. 그 폴더를 마련하지 못하면 브라우저가 뜨지 않는다.
- `browser forget`은 그 data 폴더로 뜬 브라우저를 모두 닫는다(gateway와 api 둘 다, 답을 기다리는 중이거나 넘겨받은 중이어도). 브라우저를 두고 프로필만 지우는 길은 없다. 실행 중인 동작이 15초 안에 끝나지 않으면 forget은 지우지 않고 끝나는데, 예약된 닫기는 그 동작 뒤에 그대로 일어난다. 사용자는 forget을 다시 해야 한다.
- 프로필이 매우 크면 hard link 확인이 한도를 넘어, 같은 device의 link가 둘 이상인 파일이 비밀로 다뤄질 수 있다(읽기는 묻고 쓰기는 거절). `browser forget`으로 프로필을 지우면 풀린다.
- 강제로 죽은 뒤 다시 뜬 Chromium은 "복원하시겠습니까" 말풍선을 화면에 보일 수 있다. 페이지 읽기에는 영향이 없다.
- 프로필은 하나라 대화방이 달라도 같은 로그인을 쓴다. 많은 사이트가 자동화된 브라우저의 로그인을 막는다.
