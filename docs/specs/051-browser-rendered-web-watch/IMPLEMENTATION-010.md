# PRD 010 구현 검증 기록 — 2026-11-12

대상은 Spec 051 PRD 010이다. 승인된 PRD 000의 미커밋 구현 위에 추가했다.
구현과 검증을 마쳤으며 상태는 `Implemented (independent review pending)`이다. 독립 검토는 별도다.

## 2026-11-12 독립 검토 후 Docker fixture 수정

독립 deep-high 검토는 Rust 결함 없이 Cargo 검증을 통과했지만, Docker fixture의
P2-1·P2-2 때문에 PRD 010을 반려했다. 아래 수정은 Python fixture와 이 기록에 한정한다.
Rust 코드와 기존 보호 장치는 바꾸지 않았고, PRD 상태도
`Implemented (independent review pending)`으로 유지한다. 수정 후 독립 재검토는 아직 남아 있다.

- **P2-1:** `smtp.py`는 같은 수신 폴더의 `.tmp-<uuid>.part`에 JSON을 쓴 뒤
  `flush`·`os.fsync`하고 `os.replace`로 최종 `.json` 이름을 게시한다.
  checker의 `*.json` 검색에는 작성 중 파일이 잡히지 않으며, 게시 뒤에만 SMTP 성공을 응답한다.
- **P2-2의 기존 연결 근거:** `crates/shacs-core/src/runtime/agent_loop_web_notification.rs:96`
  이 실제 발송 본문 끝에 `Tracking: <correlation_id>`를 붙인다.
  `crates/shacs-core/src/runtime/web_watch_notify.rs:193`의 상태에는 사건 ID와
  correlation ID가 함께 있고, `crates/shacs-cli/src/lib.rs:14501`의 Email 발송 기록에는
  같은 correlation ID·정확한 수신 주소·전체 본문의 `content_sha256`가 있다.
  SMTP Message-ID를 사건 ID로 임의 생성하거나 추정하지 않는다.
- `driver.py`는 기존 `/check-data/runtime/channels/worker-metadata/email-imap.json`을
  읽어 실제 발송 기록을 제공한다. checker는 SMTP 수신 후 완료된 발송 기록을 기다리고,
  사건마다 `pending`→`sent` 한 쌍과 실제 수신 메일 정확히 한 통을 연결한다.
- 수신 envelope의 주소를 파싱해 확인된 수신 주소 하나와 정확히 비교한다.
  MIME 해제한 **본문 전체**의 SHA-256을 production 발송 기록과 비교하므로 같은 가격이더라도
  revision·관측 시각·본문 일부가 다르면 통과하지 못한다. SMTP의 CRLF와 마지막 구분용
  개행 하나만 정규화하며 공백이나 본문 내용을 임의로 잘라 비교하지 않는다.
- 재시작 뒤에는 사건별 수신 수, correlation ID, 발송 기록 ID, 수신 주소, 전체 본문과
  본문 해시, 실제 수신 Message-ID가 최초 검증 결과와 모두 같은지 확인한다.
  Message-ID가 없는 메일도 기존 Tracking과 발송 본문 해시로 연결하며 새 식별자를 만들지 않는다.
- `test_receipts.py`의 로컬 회귀 검사 2개는 원자적 게시 경계와 잘못된 수신자·추가 수신자·
  다른 revision/관측 시각·다른 Tracking·중복 수신·재시작 후 Message-ID 변경·추가 발송 시도를
  확인한다. Docker·소켓·외부 서비스·Rust 실행 없이 통과했다.

실행 명령은 아래와 같고 모두 종료 코드 0이다.

```sh
bash -n scripts/spec051-web-watch-check.sh
python3 -I -m py_compile scripts/spec051-web-watch-fixtures/*.py
python3 -I -B scripts/spec051-web-watch-fixtures/test_receipts.py
```

회귀 검사 출력: `{"fixture_regressions": 2, "status": "passed"}`.
Docker와 Cargo는 실행하지 않았다. 실제 Chromium·SMTP·컨테이너 간 잠금의 기존 미검증
범위는 그대로이며, 이 결과를 실제 Docker 수용 조건의 성공으로 기록하지 않는다.

## Required Contract 대응

아래 `core 등록 시험`은 `crates/shacs-core/tests/it/spec051_registration_and_login.rs`,
`CLI 로그인 시험`은 `crates/shacs-cli/tests/it/spec051_login_watch.rs`,
`CLI 연결 시험`은 `crates/shacs-cli/tests/it/spec051_web_watch_cli.rs`를 뜻한다.

| 번호 | 구현 | 시험 |
|---|---|---|
| 1. 선택 전 저장 프로필 금지, 실제 부재에만 자동 전환 | `crates/shacs-core/src/runtime/web_watch_extract.rs:125`, `crates/shacs-core/src/runtime/web_watch_conversation_summary.rs:743` | core 등록 시험 `:177`, `:227`, `:303` |
| 2. 요약·token·계획·worker·알림의 선택 결속 | `crates/shacs-core/src/runtime/web_watch_surface.rs:143`, `:211`, `:349`, `crates/shacs-core/src/runtime/rendered_web_reader.rs:86`, `crates/shacs-core/src/runtime/web_watch_notify_events.rs:75` | core 등록 시험 `:85`, `:303`, `:357`; CLI 연결 시험 `:9`, `:147` |
| 3. 꺼짐·잠금·busy·값 변경을 성공으로 바꾸지 않음 | `crates/shacs-cli/src/browser/browser_manager.rs:2181`, `crates/shacs-core/src/runtime/web_watch_conversation_setup.rs:42` | core 등록 시험 `:357`; CLI 로그인 시험 `:166`, `:316` |
| 4. 실제 process gate·공개 주소 proxy 유지 | `crates/shacs-cli/src/browser/browser_manager.rs:1455`, `crates/shacs-cli/src/web_watch_profile.rs:9` | 기존 PRD 000의 `watch_start_uses_real_process_gate_without_provider`, `watch_uses_public_proxy_temporary_profile_and_closes_popups`; CLI 연결 시험 `:119` |
| 5. 050 잠금·identity·maintenance·Singleton·프로필 밖 cache | `crates/shacs-cli/src/browser/browser_lock.rs:409`, `:422`, `crates/shacs-cli/src/browser/browser_manager.rs:856`, `:910` | CLI 로그인 시험 `:62`, `:166`, `:253`, `:316` |
| 6. forget 경합과 정리 후 잠금·자리 해제 | `crates/shacs-cli/src/browser/browser_manager.rs:1199`, `crates/shacs-cli/src/browser/browser_forget.rs:199` | CLI 로그인 시험 `:62`, `:253`, `:316`, `:432`, `:466` |
| 7. 계정 값·프로필 저장 범위와 추측형 재로그인 안내 | `crates/shacs-core/src/runtime/web_watch_conversation_summary.rs:977`, `crates/shacs-core/src/runtime/web_watch_notify_render.rs:16`, `:126` | core 등록 시험 `:303`, `:403`, `:433` |
| 8. 기존 crate/queue 소비, 가짜 실행기·시계, Docker 승인 경계 | 기존 registry→adapter→worker 유지, `scripts/spec051-web-watch-check.sh:4` | core·CLI·API Cargo 시험, 셸 및 Python AST 문법 검사. Docker 실행 없음 |

## Acceptance Criteria 대응

| 번호 | 코드·시험 근거 |
|---|---|
| 1 | `web_watch_extract.rs:125` → `web_watch_surface.rs:95` → `web_watch_conversation_summary.rs:743`. 실제 AgentLoop의 `static_missing_candidate_offers_rendered_summary_before_confirmation`(core 등록 `:177`)와 `unsupported_static_candidates_never_trigger_browser_fallback`(`:227`). 명시 browser의 fetch 0회, 명시 fetch·429·guard·지원하지 않는 숫자의 browser 0회, 확인 전 등록 없음 |
| 2 | `rendered_web_reader.rs:86`, `web_watch_surface.rs:211`, `:529`, summary `:977`. `saved_login_requires_explicit_selection_and_matching_confirmation`(`:303`): 설정별 선택지, 최초 false 읽기, 선택 후 true, 별도 확인, v2/선택 불일치의 읽기 전 거절 |
| 3 | summary `:218`, setup `:42`, surface `:349`. `login_change_reconfirms_and_resets_only_observation_baseline`(`:357`), `login_change_discards_inflight_observation_and_lease_redelivery`(`:85`): 양방향 선택 변경, 값 변경 재확인, 통지만 변경할 때 baseline 보존, 읽는 중 revision 변경, 재전달. 기존 042의 발신자·TTL·epoch·끼어든 응답 경계는 유지 |
| 4 | manager `:910`, `:1199`, `:1455`, `:2181`. `saved_watch_uses_locked_profile_and_ephemeral_http_cache`(CLI 로그인 `:62`): 실제 manager, profile/cache 인자, 정리 중 잠금과 busy, Browser.close와 5초 예산, profile 보존. `:316`은 실패·취소·shutdown·panic 정리도 확인 |
| 5 | `browser_lock.rs:409–429`. `login_watch_obeys_singleton_and_profile_identity_checks`(`:166`), `watch_start_loses_profile_during_directory_lock_wait`(`:253`): 파일/폴더 소유자, 외부/같은 호스트 live/해석 불가/orphan Singleton, directory flock 대기 중 삭제·재생성의 child 0 |
| 6 | manager `:856`, `:1199`, 기존 forget. CLI 로그인 `:316`, `:432`, `:466`: maintenance 취소, lock 교체, 실제 forget, 공통 15초, 시작 중 profile 보존. 실제 외부 ingress takeover는 `crates/shacs-cli/src/spec051_gateway_fixture.rs:109`를 경유하며 감시 읽기를 놓기 전에 busy 응답을 확인 |
| 7 | `web_watch_notify_events.rs:75`, `web_watch_notify_render.rs:126`, `web_watch_conversation_status.rs:34`. core 등록 `:403`, `:433`: 원문·cookie·query 없는 공개 상태/관측, 저장된 true 알림 5종, 재로그인·돌려주기·닫기, 401/403의 원래 분류. false 알림 5종은 기존 PRD 000 시험 |
| 8 | 실제 production registry와 adapter/worker 배선 유지. CLI 연결 `:9`는 실제 from_bundle 대화 API 등록→동일 manager→gateway worker, `:147`은 확인된 true 계획의 worker, `:157`/`:167`은 CLI 입력. `crates/shacs-api/tests/it/spec051_web_watch_api.rs:83`은 preview/confirm/status transport. API crate는 adapter 경계를 시험하고 실제 manager/worker와 대화 API의 결합은 순환 의존성을 만들지 않고 CLI 통합 시험에서 확인 |
| 9 | `scripts/spec051-web-watch-check.sh`와 `scripts/spec051-web-watch-fixtures/{compose,driver,pages,smtp,check}.py`. 사용자가 Docker 실행을 금지했으므로 문법만 검사했다. 실제 Docker 수용 조건을 PASS로 기록하지 않는다 |
| 10 | 아래 Cargo 기록, README·USAGE 갱신. 새 crate·의존성·Cargo.lock 변경 없음. production guard는 CLI 연결 `:119`에서 시험 환경 변수를 넣은 별도 process로 확인 |

짧게 쓴 core 구현 파일은 `crates/shacs-core/src/runtime/`, manager와 lock/forget은
`crates/shacs-cli/src/browser/` 아래다.

## 동시 실행 경계

| 끼어드는 작업 | 유지하는 경계 | 결정적 시험 |
|---|---|---|
| 대화 및 takeover | 같은 slot·live 점유, 감시 중 즉시 busy | CLI 로그인 `:316`, 기존 PRD 000 양방향 busy 시험, 실제 gateway 외부 ingress |
| 다른 process의 forget | active→profile→browser 폴더가 한 15초 예산 사용 | CLI 로그인 `:316`, `:432` |
| 시작 중 active/profile 파일 교체 | profile 폴더 flock이 삭제를 막음 | CLI 로그인 `:466` |
| profile flock 대기 중 삭제·재생성 | 잠금 직후 현재 pathname identity 재확인 | CLI 로그인 `:253` |
| 진행 중 lock 파일 교체 | slot 밖 1초 maintenance가 generation 신호 취소 | CLI 로그인 `:316` |
| close·shutdown·panic | 정상/실패 모두 process·cache 정리 뒤 잠금/점유 해제 | CLI 로그인 `:62`, `:316` |
| lease 재전달·읽는 중 revision 변경 | 기존 durable lineage 검사를 유지, 오래된 관측 폐기 | core 등록 `:85` |

## 보호 장치 실패 확인

각 보호를 하나씩 임시로 끄고 assertion 실패(종료 코드 101)를 확인한 뒤 원래 bytes로
복원했다. 컴파일 실패는 red 근거로 세지 않았다.

| 번호 | 끈 보호 | 관찰한 실패 |
|---|---|---|
| 1 | 실제 부재에만 자동 browser 전환 | `-5` 반례에서 browser 읽기가 발생 |
| 2 | token의 로그인 선택 결속 | 다른 로그인 선택의 확인이 InvalidInput으로 거절되지 않음 |
| 3 | 확인 선택의 계획 저장 | 확인된 계획의 use_saved_login이 false |
| 4 | worker의 저장 로그인 전달 | 실제 gateway worker가 profile 폴더 잠금을 잡지 않음 |
| 5 | 로그인 감시의 Saved 선택 | Chromium의 profile 경로가 임시 폴더 |
| 6 | Browser.close와 5초 예산 | 종료 기다림이 5000 ms 대신 0 |
| 7 | slot 밖 maintenance의 취소 | forget으로 닫힌 읽기가 성공으로 반환 |
| 8 | profile flock 뒤 identity 재확인 | 삭제한 프로필로 시작이 성공 |
| 9 | forget 공통 15초 예산 | 15000 ms 대신 15400 ms 사용 |
| 10 | 알림의 저장된 로그인 한계 | 로그인 알림에 no saved login profile 안내 |
| 11 | generation별 취소 신호 | 앞 감시 취소가 다음 감시까지 닫음 |
| 12 | 정리 뒤 잠금 해제 | child 정리 중 browser 폴더 잠금을 다른 쪽이 획득 |
| 13 | persistLogin 꺼짐 거절 | 설정이 꺼져도 저장 프로필 읽기가 성공 |
| 14 | 5초 뒤 별도 탭 닫기 대기 금지 | 저장 generation이 Browser.close 뒤 Target.closeTarget을 다시 요청 |

## Cargo와 문법 검사

| 명령 | 결과 |
|---|---|
| `cargo check --manifest-path crates/Cargo.toml -p shacs-core -p shacs-cli -p shacs-api --all-targets --locked` | 성공. 당시 시험 import 경고는 이후 제거 |
| `cargo fmt --manifest-path crates/Cargo.toml --all -- --config imports_granularity=Module,format_macro_matchers=true` | 성공 |
| `cargo fmt --manifest-path crates/Cargo.toml --all -- --check --config imports_granularity=Module,format_macro_matchers=true` | 최종 성공 |
| `cargo clippy --manifest-path crates/Cargo.toml --workspace --all-targets --locked -- -D warnings` | 최종 성공. 새 시험 helper의 unwrap 경고를 expect 문맥으로 고친 뒤 통과 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-core --test it -q --locked spec051_` | 14개 통과 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-cli --test it -q --locked --features spec031-test-fixture spec051_` | 20개 통과 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-core --test it -q --locked spec051_registration_and_login::` | 7개 × 10회 연속 통과 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-cli --test it -q --locked spec051_login_watch::` | 6개 × 10회 연속 통과 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-cli --test it -q --locked --features spec031-test-fixture,spec051-test-profile spec051_web_watch_cli::` | 4개 × 10회 연속 통과 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-api --test it -q --locked spec051_web_watch_api::` | 2개 × 10회 연속 통과 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-core -q --locked` | 최초 종료 101: 단위 504 통과, it 646 통과·1 실패·2 기존 ignored. 실패 수정 및 미실행 대상의 후속 결과는 아래 기록 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-core --doc -q --locked` | 2개 통과, 종료 0 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-cli -q --locked` | 종료 0. 단위 472개·it 203개 및 나머지 대상 통과, 기존 opt-in 3개 ignored 유지 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-api -q --locked` | 종료 0. 단위 104개·it 24개 및 나머지 대상 통과 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-cli --test it -q --locked --features spec031-test-fixture spec051_rendered_browser::` | 양방향 로그인 busy를 보완한 10개 × 10회 연속 통과 |
| `bash -n scripts/spec051-web-watch-check.sh` | 성공 |
| Python fixture 5개 파일의 `ast.parse` | 성공 |

반복 실행은 같은 명령을 `set -e; for repeat in {1..10}; do ...; done`으로 차례대로
실행했다. 직접 시작한 Cargo는 모두 전경으로 하나씩 실행했다. 도구의 전경 대기 상한으로
분리된 명령은 완료 통지를 받은 다음에만 다음 Cargo를 시작했다. 기존 시험 내부의 자식
Cargo는 사용자가 허용한 범위다. 전체 workspace 테스트·Docker·Git 상태 변경·commit은
실행하지 않았다. `CARGO_TARGET_DIR`를 설정하지 않았다.

## 구현 범위·보완·미검증

- 새 `RenderedWebRequest`로 URL·한도·로그인 선택을 묶어 같은 production reader에 전달한다.
  내부 manager는 `ProfileUse`로 바꾸고 기존 `browser_lock::acquire`를 그대로 소비한다.
- token v3에 로그인 선택을 결속한다. v1/v2는 false만 허용하고, false 저장 계획은 새 필드를
  생략하므로 PRD 000 browser digest와 기존 fetch digest를 유지한다.
- 제안 입력은 미지정과 명시 false를 구별해야 기존 감시의 알림만 바꿀 때 로그인 선택을
  유지할 수 있다. 새 제안의 미지정은 false다.
- 기존 감시 종료의 빠른 정리 경로에 050의 `quit`를 연결했다. 저장 프로필에서는
  Browser.close→최대 5초→강제 정리, 임시 프로필에서는 기존 빠른 정리다.
  마지막 점검에서 저장 generation에 남아 있던 별도 탭 닫기 요청을 제거했다.
  5초 뒤 추가 1초를 기다릴 수 있는 길을 없앤 것으로, 050 계약을 바꾼 것이 아니다.
  이 경로를 다시 넣은 14번째 red 검사와 최종 CLI·gateway 그룹 각각 10회를 통과했다.
- PRD 000의 대화형 수정 임시 제한 안내를 SPEC 051·README·USAGE에서 제거했다.
  001–050 문서는 변경하지 않았다.
- 2026-11-12 SPEC의 시험 배치 보완에 따라 API route 계약은 API package에서,
  실제 from_bundle·대화 API·manager·gateway worker 결합은 CLI package에서 검증했다.
  새 의존성이나 Cargo.lock 변경을 만들지 않았으며 API의 가짜 adapter 시험만으로
  운영 배선을 통과했다고 보지 않는다. 제품 계약을 바꾼 보완은 없다.
- 사용자 지시(2026-11-12)에 따라 Docker는 실행하지 않았다. 실제 Chromium의 JavaScript
  렌더링·cookie 유지·network idle event 순서·SOCKS5 통신·SMTP 전달·container 간 directory
  flock은 미검증이다. Cargo 성공을 이 실제 환경의 성공으로 대신하지 않는다.
- 050이 보장하지 않는 profile 폴더 자체의 외부 이름 교체와 directory flock을 전달하지
  않는 공유 파일 시스템은 여전히 보장 밖이다.
- clippy 종료 코드는 0이지만 기존 의존성 `imap-proto 0.10.2`의 future-incompatibility
  안내는 남는다.

## 실패 검증의 정확한 Cargo 명령

```sh
# 1. 실제 부재에만 자동 browser 전환
cargo test --manifest-path crates/Cargo.toml -p shacs-core --test it -q --locked spec051_registration_and_login::unsupported_static_candidates_never_trigger_browser_fallback
# 2. token의 로그인 선택 결속
cargo test --manifest-path crates/Cargo.toml -p shacs-core --test it -q --locked spec051_registration_and_login::saved_login_requires_explicit_selection_and_matching_confirmation
# 3. 확인 선택의 계획 저장
cargo test --manifest-path crates/Cargo.toml -p shacs-core --test it -q --locked spec051_registration_and_login::saved_login_requires_explicit_selection_and_matching_confirmation
# 4. worker의 저장 로그인 선택 전달
cargo test --manifest-path crates/Cargo.toml -p shacs-cli --test it -q --locked --features spec031-test-fixture spec051_web_watch_cli::gateway_runs_the_confirmed_login_choice
# 5. 로그인 감시의 프로필 선택
cargo test --manifest-path crates/Cargo.toml -p shacs-cli --test it -q --locked spec051_login_watch::saved_watch_uses_locked_profile_and_ephemeral_http_cache
# 6. 저장 프로필 종료 요청과 5초 예산
cargo test --manifest-path crates/Cargo.toml -p shacs-cli --test it -q --locked spec051_login_watch::saved_watch_uses_locked_profile_and_ephemeral_http_cache
# 7. slot 밖 maintenance의 즉시 취소
cargo test --manifest-path crates/Cargo.toml -p shacs-cli --test it -q --locked spec051_login_watch::forget_and_maintenance_close_saved_watch_before_profile_removal
# 8. 프로필 flock 뒤 identity 재확인
cargo test --manifest-path crates/Cargo.toml -p shacs-cli --test it -q --locked spec051_login_watch::watch_start_loses_profile_during_directory_lock_wait
# 9. forget의 공통 15초 예산
cargo test --manifest-path crates/Cargo.toml -p shacs-cli --test it -q --locked spec051_login_watch::saved_watch_forget_shares_fifteen_second_budget
# 10. 알림의 저장된 로그인 한계 안내
cargo test --manifest-path crates/Cargo.toml -p shacs-core --test it -q --locked spec051_registration_and_login::login_failure_and_all_notification_limits_use_stored_choice
# 11. 감시 generation별 취소 신호
cargo test --manifest-path crates/Cargo.toml -p shacs-cli --test it -q --locked spec051_login_watch::forget_and_maintenance_close_saved_watch_before_profile_removal
# 12. 정리 뒤 잠금 해제
cargo test --manifest-path crates/Cargo.toml -p shacs-cli --test it -q --locked spec051_login_watch::saved_watch_uses_locked_profile_and_ephemeral_http_cache
# 13. persistLogin 꺼짐 거절
cargo test --manifest-path crates/Cargo.toml -p shacs-cli --test it -q --locked spec051_login_watch::login_watch_obeys_singleton_and_profile_identity_checks
# 14. 5초 뒤 별도 탭 닫기 대기 금지
cargo test --manifest-path crates/Cargo.toml -p shacs-cli --test it -q --locked spec051_login_watch::saved_watch_uses_locked_profile_and_ephemeral_http_cache
```

## 전체 검증 중 발견해 수정한 회귀

`cargo test --manifest-path crates/Cargo.toml -p shacs-core -q --locked`의 첫 실행에서
단위 시험 504개와 앞선 통합 대상은 통과했다. `it`는 646개 통과·2개 기존 ignored·
1개 실패였다. 실패는
`spec042_web_watch::conversation::a_confirmed_summary_sets_up_the_work_watch_notification_and_deadline_at_once`로,
메일 대상의 새 `email:` 표시가 기존 요약의 `email to <address>` 표현을 깨뜨린 경우였다.
기존 표현 뒤에 확인된 `email:<address>`를 괄호로 함께 적도록 production 요약을 고쳤다.
시험 기대값은 바꾸지 않았다.

다음 명령은 수정 뒤 1개 통과, 종료 코드 0이었다.

```sh
cargo test --manifest-path crates/Cargo.toml -p shacs-core --test it -q --locked spec042_web_watch::conversation::a_confirmed_summary_sets_up_the_work_watch_notification_and_deadline_at_once
```

동작이 바뀌지 않은 504개 단위 시험을 다시 돌리지 않는 저장소 규칙에 따라, 영향받는
새 core 그룹은 최종 소스에서 다시 10회 실행했고, 첫 전체 명령이 멈춰 아직 실행하지
못한 `it` 뒤의 60개 통합 target과 doctest를 별도 명령으로 이어서 모두 검사했다.
따라서 첫 전체 명령 자체가 종료 코드 0이었다고 기록하지 않는다.

후보 원인 보존의 마지막 점검에서는 검색창의 마지막 글자에서 **완전히 끝난 숫자**도
SearchLimit로 거절하던 회귀를 같은 반례 시험에 추가해 먼저 재현했다. 숫자가 실제로
검색창 밖에서 이어질 때만 한도로 분류하도록 고쳐 기존 정상 후보를 유지했다.
`cargo test --manifest-path crates/Cargo.toml -p shacs-core --test it -q --locked spec051_registration_and_login::unsupported_static_candidates_never_trigger_browser_fallback`
의 수정 전 결과는 1개 실패(종료 101)였고, 수정 뒤 최종 등록 그룹 7개가 10회 연속
통과했다. 영향받는 기존 추출 시험도 다음 명령으로 7개 통과·기존 subprocess probe
1개 ignored, 종료 0을 확인했다. 이 probe는 상위 시험이 별도 process로 호출한다.

```sh
cargo test --manifest-path crates/Cargo.toml -p shacs-core --test it -q --locked spec042_web_watch::failures::
```

이 보완 뒤 지정된 fmt check와 workspace clippy도 다시 통과했다. 이미 통과했고
영향받지 않는 다른 패키지 시험은 중복 실행하지 않았다.

## 변경 파일

- `README.md`
- `docs/USAGE.md`
- `docs/specs/051-browser-rendered-web-watch/SPEC.md`
- `docs/specs/051-browser-rendered-web-watch/prds/010-registration-and-login-watch.md`
- `docs/specs/051-browser-rendered-web-watch/IMPLEMENTATION-010.md`
- `crates/shacs-core/src/runtime/mod.rs`
- `crates/shacs-core/src/runtime/rendered_web_reader.rs`
- `crates/shacs-core/src/runtime/web_watch.rs`
- `crates/shacs-core/src/runtime/web_watch_conversation.rs`
- `crates/shacs-core/src/runtime/web_watch_conversation_setup.rs`
- `crates/shacs-core/src/runtime/web_watch_conversation_status.rs`
- `crates/shacs-core/src/runtime/web_watch_conversation_summary.rs`
- `crates/shacs-core/src/runtime/web_watch_extract.rs`
- `crates/shacs-core/src/runtime/web_watch_fetch.rs`
- `crates/shacs-core/src/runtime/web_watch_notify.rs`
- `crates/shacs-core/src/runtime/web_watch_notify_events.rs`
- `crates/shacs-core/src/runtime/web_watch_notify_render.rs`
- `crates/shacs-core/src/runtime/web_watch_read.rs`
- `crates/shacs-core/src/runtime/web_watch_record.rs`
- `crates/shacs-core/src/runtime/web_watch_surface.rs`
- `crates/shacs-core/src/tools/propose_web_watch.rs`
- `crates/shacs-core/src/runtime/durable_filtered_reads_tests.rs`
- `crates/shacs-core/tests/it/main.rs`
- `crates/shacs-core/tests/it/spec042_web_watch.rs`
- `crates/shacs-core/tests/it/spec042_web_watch/conversation.rs`
- `crates/shacs-core/tests/it/spec042_web_watch/failures.rs`
- `crates/shacs-core/tests/it/spec042_web_watch/support.rs`
- `crates/shacs-core/tests/it/spec051_rendered_observation.rs`
- `crates/shacs-core/tests/it/spec051_registration_and_login.rs`
- `crates/shacs-cli/Cargo.toml`
- `crates/shacs-cli/src/browser/browser_manager.rs`
- `crates/shacs-cli/src/spec042_web_watch_cli.rs`
- `crates/shacs-cli/src/spec051_gateway_fixture.rs`
- `crates/shacs-cli/src/web_watch_profile.rs`
- `crates/shacs-cli/tests/it/main.rs`
- `crates/shacs-cli/tests/it/spec042_web_watch_cli.rs`
- `crates/shacs-cli/tests/it/spec050_browser_takeover.rs`
- `crates/shacs-cli/tests/it/spec051_rendered_browser.rs`
- `crates/shacs-cli/tests/it/spec051_web_watch_cli.rs`
- `crates/shacs-cli/tests/it/spec051_login_watch.rs`
- `crates/shacs-api/src/spec042_web_watch.rs`
- `crates/shacs-api/tests/it/spec051_web_watch_api.rs`
- `scripts/spec051-web-watch-check.sh`
- `scripts/spec051-web-watch-fixtures/compose.py`
- `scripts/spec051-web-watch-fixtures/driver.py`
- `scripts/spec051-web-watch-fixtures/pages.py`
- `scripts/spec051-web-watch-fixtures/smtp.py`
- `scripts/spec051-web-watch-fixtures/check.py`

## core 후속 검증과 재현 명령

첫 후속 명령에서 앞의 44개 target은 통과했고 `spec033_release_closure`의 6개 시험은 DigestMismatch로 실패했다. 실행 중 증거 문서를 변경하여 저장소 source manifest가 달라진 상태였다. 이후 저장소 파일을 고정하고 해당 target부터 남은 16개를 모두 다시 실행해 종료 코드 0을 확인했다. 출시 시험이나 검사 조건을 변경하지 않았다. core의 기본 단위·통합·doctest 대상은 이 실행들과 실패 1개 수정 후 재실행을 합쳐 모두 확인했다.

```sh
# 첫 후속 명령: 종료 101 (출시 증거의 source manifest 변경)
cargo test --manifest-path crates/Cargo.toml -p shacs-core -q --locked --test permission_action --test permission_policy --test plugin_runtime --test runtime --test runtime_agent --test runtime_loop --test runtime_workflow --test spec030_classifier_baseline --test spec030_diagnostics_aggregate --test spec030_exec_config --test spec030_fact_store --test spec030_javascript_tool_before_exec --test spec030_local_provider --test spec030_process_controlled_child --test spec030_process_fact_scope --test spec030_provider_credentials_command --test spec030_provider_credentials_oauth --test spec030_provider_credentials_transport --test spec030_resource_load_checks --test spec030_resource_selection --test spec030_sandbox_adapter --test spec030_sandbox_invalid_plan_fallback --test spec030_startup_facts --test spec030_tool_before --test spec030_tool_before_confirmation --test spec030_tool_before_interaction --test spec030_tool_before_preparation --test spec030_tool_before_timeout --test spec030_trace_disclosure --test spec030_trusted_runtime --test spec031_activation_execution --test spec031_activation_store --test spec031_context_projection --test spec031_execution_snapshot --test spec031_sequential_integration --test spec033_automation_dispatch --test spec033_automation_gates --test spec033_automation_rejection --test spec033_automation_results --test spec033_automation_service --test spec033_goal_accounting --test spec033_local_improvement_process --test spec033_local_improvement_support --test spec033_production_local_gate --test spec033_release_closure --test spec033_release_edges --test spec033_release_runner --test spec033_self_improvement_live --test spec033_snapshot_replay --test spec033_snapshot_replay_support --test spec034_image_edit --test spec034_persistence_operations --test spec034_sequential_integration --test spec035_tasks_assembler --test spec036_action_decision --test spec036_entrusted_work --test spec036_entrusted_work_compat --test spec036_notification --test spec036_observation --test tools
# 파일 고정 뒤 마지막 16개 대상: 종료 0
cargo test --manifest-path crates/Cargo.toml -p shacs-core -q --locked --no-fail-fast --test spec033_release_closure --test spec033_release_edges --test spec033_release_runner --test spec033_self_improvement_live --test spec033_snapshot_replay --test spec033_snapshot_replay_support --test spec034_image_edit --test spec034_persistence_operations --test spec034_sequential_integration --test spec035_tasks_assembler --test spec036_action_decision --test spec036_entrusted_work --test spec036_entrusted_work_compat --test spec036_notification --test spec036_observation --test tools
# doctest: 2개 통과, 종료 0
cargo test --manifest-path crates/Cargo.toml -p shacs-core --doc -q --locked
```

최종 소스의 연속 반복은 core 등록 7개, CLI 로그인 6개, gateway 연결 4개, API 2개, 기존 공유 브라우저 10개 그룹 각각 10회다. 보완 전 반복까지 포함한 반복 루프 누계는 각각 40·20·20·10·10회다. 이 수에는 개발 중 단독 실행과 red 검사를 더하지 않았다.

새 요청/fixture를 연결하는 동안 발생한 컴파일 실패(필드 누락, 타입 추론, 실제 실행 인자 필드명), shutdown 시험의 동기 호출 경합, API session 접두를 중복한 시험 조회, helper의 unwrap 경고는 원인별로 수정한 뒤 대상 시험과 최종 clippy로 확인했다. 시험을 삭제·skip하거나 실패 assertion을 약하게 바꾸지 않았다.

## Cargo.lock과 Git 상태

다음 읽기 전용 명령은 종료 코드 0이었고 HEAD는 `c9025d0`이었다. index 갱신 잠금도 끈 상태로 확인했으며 stage·commit·reset 등 상태 변경 명령은 실행하지 않았다.

```sh
GIT_MASTER=1 GIT_OPTIONAL_LOCKS=0 git diff --exit-code c9025d0 -- crates/Cargo.lock
GIT_MASTER=1 GIT_OPTIONAL_LOCKS=0 git rev-parse --short HEAD
```
