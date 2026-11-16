# PRD 010. 대화형 등록과 로그인 감시

Status: Implemented (independent review approved 2026-11-12)

Depends on: PRD [000](000-rendered-observation.md)의 닫힌 결과(읽기 방식·검증된 rendered reader·실제 worker 배선·한 자리 점유·실패와 알림 한계). [042](../../042-conversational-web-watch-and-trustworthy-silence/SPEC.md) PRD 030의 요약·확인과 [050](../../050-browser-takeover-and-saved-login/SPEC.md) PRD 020의 저장 로그인·잠금·forget, PRD 010의 takeover를 소비한다. 뒤 번호 스펙에 의존하지 않는다.

## Goal

대화에서 정적으로 읽을 수 없는 페이지를 브라우저로 읽어 감시하도록 제안하고, 사용자가 로그인 사용 여부까지 확인한 요약대로만 등록한다. 로그인 감시는 050의 저장 프로필을 잠그고 쓰며, 값을 못 읽으면 기존 실패 알림에 다시 로그인하는 방법을 적는다. 실제 Docker의 그려진 페이지·로그인 페이지와 로컬 SMTP까지 이어지는 확인을 이 PRD가 소유한다.

## 지금 상태

- `crates/shacs-core/src/tools/propose_web_watch.rs`의 `WebWatchProposalDraft`에는 읽기 방식과 로그인 선택이 없다. `web_watch_conversation_summary.rs::build_summary`가 결정적으로 요약을 만들고, `read_target`은 `preview_page`의 첫 후보를 고른다. 빈 `WebPreview.candidates`를 받으면 "the value was not found" 문제를 만들지만, `web_watch_extract.rs::first_number`와 `push`는 숫자 형식·부호·범위 값·추출 검증 실패도 후보 없이 끝낸다. 따라서 빈 목록은 실제 값 부재의 증거가 아니다. 기존 `spec042_web_watch/failures.rs::signed_fractional_or_range_numbers_are_not_offered_as_one_value`가 이 반례를 확인한다.
- `web_watch_conversation.rs`의 `PendingWebWatchSummary`·`WebWatchSetup`·`SummaryRead`가 요약과 token을 저장한다. 확인은 `agent_loop_web_watch.rs`와 `web_watch_conversation_setup.rs::set_up`을 지나며, `reread`에서 다시 읽고 모든 설정이 성공한 사본만 session에 반영한다. `unchanged_since`는 관측·통지·마감 revision을 모두 본다.
- `web_watch_surface.rs::store_confirmed_watch`는 관측 revision을 증가시키고 baseline을 새로 쓴다. 로그인한 화면인지 구분할 값은 없다. PRD 000은 여기에 읽기 방식만 더한 상태로 닫힌다.
- 050의 `browser_lock.rs::acquire`는 `ProfileUse::Saved`에서 `profile.lock`뿐 아니라 프로필 폴더 descriptor 자체의 배타 flock과 잠금 뒤 identity 확인을 수행한다. 모든 generation은 마지막에 `browser/` descriptor 공유 flock을 가진다. `browser_forget.rs::remove`는 같은 잠금들·identity·`SingletonLock`을 확인한 뒤 열린 descriptor를 통해 지운다.
- `browser_manager.rs::quit`는 저장 프로필 generation에 DevTools `Browser.close`를 보내고 최대 5초 뒤 강제로 닫는다. `browser_process.rs::BrowserEnvironment::chromium_on`은 `--user-data-dir`, 프로필 밖 `--disk-cache-dir`, `--password-store=basic`을 쓴다. HTTP cache를 끄지 않는다.
- 042 시험 profile(`crates/shacs-cli/src/web_watch_profile.rs`, Cargo feature `spec042-test-profile`)은 숫자 주소 `127.0.0.1/32`와 파일 시계만 지원한다. 050 Docker 스크립트의 별도 page container `172.30.250.2`는 사용자의 `ssrfWhitelist`로 열지만 051 감시는 그 whitelist를 소비하면 안 된다.

## 설계 결정과 근거 (2026-11-12)

1. **자동 제안의 시점.** 정적 읽기의 후보 생성이 보존한 사유가 실제 값 부재(`value_absent`)일 때만 browser 미리보기를 한 번 더 한다. 빈 후보 목록이나 지금의 일반적인 "값을 찾지 못함" 안내만으로 판단하지 않는다(SPEC의 같은 날짜 보완). 숫자 형식·부호·범위 값·추출 검증 실패와 HTTP 오류·429·guard 거절·모호한 추출을 브라우저로 우회하지 않는다.
    - 다른 길: 어떤 읽기 실패든 브라우저로 재시도한다. 접근 제한과 `Retry-After`를 우회할 수 있고, 사용자가 정한 위치와 다른 값을 고를 수 있다.
    - 이 추가 읽기는 등록 미리보기에서만 한다. 저장된 감시의 주기 확인은 정해진 방식 하나만 쓰고 자동 전환하지 않는다.
2. **로그인 선택은 명시적이다.** `persistLogin=true`는 저장 프로필을 쓸 수 있는 조건이지 감시마다 쓴다는 뜻이 아니다. 새 제안은 로그인 미사용이고, 사용자가 "로그인한 상태로 보기"를 고른 때만 저장 프로필로 미리본다.
    - 다른 길: 브라우저로 읽기 자동 제안에서 먼저 저장 프로필을 써 본다. 계정 내용에 접근한 뒤에야 사용자에게 고르게 되어 SPEC 소유 범위 6의 순서와 다르다.
3. **계정별 프로필을 새로 만들지 않는다.** 사용자가 고른 `use_saved_login` 하나를 계획·token·digest·요약에 결속한다. true는 050의 공용 `browser/profile/`을 사용한다는 뜻이며 계정 식별자나 로그인 성공 증명은 아니다.
    - 다른 길: cookie를 감시별로 복사하거나 별도 프로필을 만든다. 050의 한 프로필 잠금·forget·비밀 파일 경계와 다른 제품이다. 이 PRD는 하지 않는다.
4. **잠금은 복제하지 않는다.** PRD 000의 generation 시작 경로에서 프로필 선택만 `Saved`로 바꾸고 `browser_lock::acquire`를 그대로 호출한다. 열기·maintenance·정리까지 같은 `BrowserLockHold`를 유지한다. 잠금 순서를 축약하거나 프로필 경로만 `start_on`에 직접 넘기는 별도 reader는 허용하지 않는다.
    - SPEC의 2026-11-12 저장 예외 결정에 따라 이 PRD는 저장 프로필의 cookie 생성·저장·전송과 사이트 자료의 예외를 소유한다. 임시 프로필의 익명 cookie와 generation HTTP cache 예외는 PRD 000의 계약을 소비한다. 감시만을 위한 cookie/cache 제한은 추가하지 않는다.
5. **Docker의 로컬 주소는 시험 빌드에만 둔다.** 새 Cargo feature `spec051-test-profile`은 `spec042-test-profile`의 파일 시계를 소비하고, 고정 test page 주소 `172.30.250.2/32`만 추가 허용하는 분기를 만든다. production과 042 전용 profile은 그대로다.
    - 다른 길: `tools.ssrfWhitelist`를 감시에 전달하거나 production 환경 변수로 사설 주소를 연다. SPEC의 공개 주소 계약이 깨진다. 시험 feature 없이 같은 환경 변수를 주어도 허용되지 않아야 한다.
    - 별도 page container의 숫자 주소는 049의 Docker 구성과 맞추기 위한 것이며 사용자 설정으로 주소·대역을 늘리는 표면은 만들지 않는다.

## Scope

1. **저장과 입력:** PRD 000의 `reading_mode` 옆에 `use_saved_login: bool`을 더한다. `WebWatchPlan`·`WebWatchPlanView`·`WebPreviewRequest`·`ConfirmWebWatchRequest`·`WebPreview`·`CandidateToken`·`WebWatchSetup`·`WebWatchProposalDraft`·`WebNotifyIntent`가 같은 뜻으로 소비한다. 내부 실행에서는 `ProfileUse` enum으로 바꾼다.
    - 누락은 false이고 false는 기존 계획 JSON에서 생략한다. PRD 000의 browser 계획 digest와 기존 fetch 예약은 그대로다. 새 token은 version 3으로 로그인 여부까지 넣고, version 1·2 token은 로그인 미사용으로만 읽는다. 예전 token으로 저장 프로필 읽기를 승인하지 않는다.
    - `fetch + true`는 잘못된 입력으로 거절한다. browser의 false/true는 URL과 추출 문구가 같아도 서로 다른 계획이다. `confirmation_candidate`·`decode_token`에서 URL·방식·로그인 여부·추출 버전을 함께 확인한다. API body를 고쳐 token과 다른 로그인 선택으로 실행할 수 없다.
    - CLI의 `work web-preview`·`work web-watch`에 `--use-saved-login`(생략 false), API preview·confirm에 `use_saved_login`을 더한다. browser 방식에서만 받는다. preview에 쓴 선택을 confirm에서도 명시해야 하며 불일치면 새 preview를 요구한다.
    - `persistLogin`이 꺼진 실행에서 true 요청은 입력을 false로 낮추지 않고 `browser_unavailable`의 `persist_login_disabled`로 실패한다. 꺼진 설정은 선택지를 숨기지만 이미 등록된 로그인 감시를 비로그인 감시로 바꾸지는 않는다.
2. **대화형 미리보기:** `WebWatchProposalDraft`의 새 선택을 `build_summary`·`new_setup`·`change_setup`과 `read_target`·`current_read`까지 전달한다. 페이지 글은 후보의 값과 추출 위치로만 쓰고 요약의 URL·조건·주기·알림 대상·로그인 선택을 바꾸는 재료로 쓰지 않는다.
    - **후보 부재의 원인 보존:** `web_watch_extract.rs::candidates`의 내부 결과를 후보가 있는 결과와 사유가 있는 부재 결과로 구분한다. 신설할 `CandidateSearchResult`는 `Candidates`(한 개 이상) 또는 `Unavailable(CandidateAbsenceReason)`이고, 사유는 `ValueAbsent`, `UnsupportedNumber`, `AmbiguousValue`, `InvalidExtraction`, `SearchLimit`이다. `first_number`의 숫자 없음과 형식 추론/파싱 실패·지원하지 않는 부호를 구분하고, `continues_number`의 범위 값 거절과 `push`의 추출 재검증 실패도 버리지 않고 전달한다. 후보 중복 제거는 실패 사유가 아니다.
    - `ValueAbsent`는 잘리지 않은 정적 글을 필요한 범위까지 조사해 요청한 label/문구 또는 그 위치의 숫자 자체가 없음을 확인한 경우에만 낸다. 숫자 모양은 있지만 읽을 수 없는 경우는 `UnsupportedNumber`, 고정 문구를 만들 수 없거나 재검증이 맞지 않는 경우는 `InvalidExtraction`, 개수/위치의 모호함은 `AmbiguousValue`다. 응답 잘림이나 검색 창·구간 길이 한도로 부재를 확정할 수 없으면 `SearchLimit`다. 문구가 지정 구간에 없다는 것을 정상적으로 읽은 `present: false` 후보는 부재 실패가 아니다.
    - 여러 label 위치에서 후보를 못 만들었을 때도 사유를 합쳐 지킨다. 하나라도 지원하지 않는 값·검증 실패·모호함·한도에 걸렸으면 `ValueAbsent`로 낮추지 않는다. 복수 사유의 대표값은 `UnsupportedNumber` → `AmbiguousValue` → `InvalidExtraction` → `SearchLimit` 순서로 고정하고, 이 중 어느 것도 없이 실제 부재만 확인됐을 때 `ValueAbsent`다. 유효 후보가 있으면 기존 후보 선택 규칙을 유지한다.
    - `web_watch_surface.rs::preview_page`가 이 결과를 `WebPreview.candidates`와 새 optional `no_candidate_reason`으로 전달한다. 후보가 있으면 사유 없음, 후보가 없으면 해당 enum의 snake_case 사유가 있어야 한다. `read_target`·`current_read`까지 이를 보존하며, 과거 결과처럼 사유가 없거나 알 수 없는 빈 목록은 부재로 추정하지 않는다. 공개 API는 기존 후보 목록을 유지하되 원문 없는 사유만 더한다. 재읽기의 `static_content_unavailable`도 이 원인 구분을 통과한 실제 부재일 때만 자동 제안에 쓴다.
    - 기본은 fetch, 로그인 미사용이다. 정적 성공 응답의 `no_candidate_reason == value_absent`일 때만, 브라우저를 쓸 수 있는 구성에서 같은 URL·target으로 비로그인 browser 미리보기를 한 번 한다. 다른 label·구간·URL을 생성하지 않는다. 그 밖의 사유와 `ambiguous_value`, `format_mismatch`, network/HTTP/guard 오류에서는 자동 browser 읽기가 0회다.
    - "브라우저로 읽어 줘"처럼 사용자가 처음부터 방식을 정하면 fetch를 먼저 호출하지 않는다. 사용자가 fetch로 한정했으면 자동 browser 제안을 하지 않고 정적 읽기의 한계를 알린다.
    - 실제 gate는 읽기 시작 때 판정한다. 설정상 가능해도 gate 거절·busy·시작 실패이면 값이나 확인 가능한 성공 요약을 만들지 않고 이유와 다시 시도하는 방법을 알린다. busy 때 기다리거나 대화 브라우저를 닫지 않는다.
    - 자동 browser 후보를 얻었으면 runtime이 렌더링하는 요약에 `읽기 방식: 브라우저로 읽기 (rendered_text_v1)`와 읽은 값을 명시한다. 사용자가 그 요약을 확인하기 전에는 반복 wake가 없다.
    - `persistLogin`이 켜져 있으면 요약 또는 후보를 못 얻은 안내에 `로그인한 상태로 보기`와 `로그인 없이 보기`를 이름으로 제시한다. 사용자가 로그인 사용을 요청하기 전의 미리보기는 저장 프로필을 열지 않는다. 후보가 없는 상태에서도 사용자가 로그인 읽기를 요청해 새 제안을 만들 수 있어야 한다.
    - 선택은 다음 사용자 메시지로 받는다. 모델은 그 요청을 새 `propose_web_watch`의 방식·로그인 field로 제안하고 runtime은 다시 미리보고 새 요약을 낸다. 이 선택 메시지는 등록 확인을 대신하지 않는다. `ask_user`의 임의 답을 감시 등록 동의로 승격하지 않는다.
3. **요약의 고정 안내:** 모든 요약에 방식과 `로그인 사용: 예/아니오`를 적는다. 로그인 사용 요약에는 아래 뜻의 runtime 고정 글을 값·조건·주기·대상·기준 시간·마감과 함께 넣는다. 모델이 페이지 글로 이 안내를 줄이거나 바꾸지 못한다.

   ```text
   로그인 사용: 예 — 저장된 브라우저 프로필의 로그인 상태로 읽습니다.
   확인한 값과 근거는 감시 기록에 저장되고, 확인한 알림 대상으로 보내집니다.
   페이지 글 전체·화면·cookie·로그인 정보는 감시 사실·알림·상태 조회·진단에 남기거나 보내지 않습니다.
   로그인 기록 폴더 browser/profile/에는 cookie와 방문 기록·사이트 자료가 남고, 페이지 내용을 담을 수 있습니다. 폴더 전체는 비밀 파일로 다룹니다.
   HTTP cache는 저장 프로필 밖의 임시 폴더에 있고, 그 폴더는 비밀 파일 보호 대상이 아닙니다. 정상으로 닫히면 지워지지만 강제 종료 때는 남을 수 있습니다.
   로그인이 풀려 값을 읽지 못하면 확인 실패로 기록합니다.
   ```

    - 알림 대상을 설정하지 않았다면 보내지 않는다는 현재 상태를 함께 보인다. 발송 대상으로는 사용자가 확인할 `email:<address>` 하나를 적는다.
    - 로그인 미사용 browser에는 저장 로그인 프로필을 쓰지 않고 확인마다 임시 프로필로 읽으며, 그동안 익명 cookie를 생성·저장·전송할 수 있다고 적는다. 임시 프로필과 임시 cache는 저장 프로필의 비밀 파일 보호 밖이고 정상 종료 때 지우지만 강제 종료 뒤 남을 수 있다. "cookie가 전혀 없다"고 적지 않는다.
    - 기존 요약 확인 경계(같은 대화·발신자, 최신 요약 또는 ID/revision 명시, TTL 30분, 재시작 epoch, 중간 다른 응답, page/tool 내용이 제안에 끼어든 turn 거절)를 유지한다. 선택지가 추가됐다는 이유로 모델이 "동의한 것 같다"고 판단하게 하지 않는다.
4. **확인과 수정:** `web_watch_conversation_setup.rs::set_up`·`reread`는 요약의 방식·로그인 여부로 다시 읽는다. preview를 읽은 generation은 이미 닫혔고 확인은 새 generation이다.
    - 같은 값이면 그 확인 순간 결과를 baseline으로, 요약의 선택을 계획으로 저장한다. 값이 달라졌으면 `SetupOutcome::Revised`로 새 값을 보여 다시 확인받는다. busy·gate 거절·로그인 설정 꺼짐·프로필 잠김이면 아무것도 저장하지 않고 다시 확인할 요약을 요청한다. 사본 전체를 적용하는 042 원자성을 유지해 위임만 만들어지고 관측이 없는 상태를 만들지 않는다.
    - 기존 감시의 방식 또는 로그인 여부가 바뀌면 `change_setup`의 관측 변경 판단에 포함한다. `read: None`으로 기존 관측을 유지하는 통지/마감 변경 갈래에 섞지 않는다. 새 미리보기→요약→확인→revision 증가와 baseline 재설정 순서다.
    - 알림 대상·실패 기준 시간·마감만 바꾸면 기존 방식·로그인 여부·비교 기준을 유지한다. 읽기 도중 pause/done/clear나 revision 변경이 들어온 경우에는 042의 확인/저장 및 발송 직전 검사로 오래된 결과를 버린다.
5. **로그인 generation과 잠금:** 감시의 `use_saved_login=true`이고 실행 설정의 `persistLogin=true`일 때만 `ProfileUse::Saved`다. 로그인 미사용이면 config가 true여도 `Temporary`다. 실제 process gate와 공개 주소 proxy는 PRD 000 그대로다.
    - `browser_lock.rs::acquire`의 현재 시작 순서를 그대로 소비한다.

      | 순서 | 현재 코드의 검사·소유 |
      |---|---|
      | 1 | `BrowserFolder`의 `browser/` 확인·identity 기록. `active.lock` 공유 flock을 최대 500 ms 기다려 잡는다 |
      | 2 | `forget.lock` 공유 flock을 최대 500 ms 확인하고 identity를 기록한 뒤 놓는다. forget이 배타로 잡았으면 시작하지 않는다 |
      | 3 | 저장 profile이 없을 때만 만들고 사용 가능 여부를 확인한다. `profile.lock` 배타 flock을 최대 500 ms 기다린다 |
      | 4 | `browser_profile_directory::open`이 `O_DIRECTORY | O_NOFOLLOW`로 연 profile descriptor에 배타 flock을 최대 500 ms 기다린다. 잡은 **직후** descriptor metadata와 현재 경로의 symlink를 따르지 않는 metadata를 비교한다. 없거나 inode/device가 다르면 `ProfileUnreadable`, child 0이며 다시 만들지 않는다 |
      | 5 | 열린 descriptor의 `singleton_open`으로 050 결정 (나)를 적용한다 |
      | 6 | `hold_browser_directory`가 `browser/` descriptor의 공유 flock을 최대 500 ms 기다린다. 처음 기록한 identity, 열린 descriptor, 현재 경로가 같아야 한다. 여기까지 성공한 뒤에만 child를 띄운다 |

    - 결정 (나): `SingletonLock::Absent` 또는 같은 hostname에서 해당 pid가 Chromium으로 살아 있지 않은 `Startable`만 시작할 수 있다. 같은 hostname의 살아 있는 Chromium, 다른 hostname, 해석/읽기 불가능이면 시작하지 않는다. shacs-bot은 `Singleton*`을 지우지 않고, startable의 정리는 Chromium에 맡긴다.
    - `BrowserLockHold`는 `Generation`에서 process·document·임시 폴더보다 뒤에 해제한다. 저장 profile 파일·폴더 배타 lock, active 공유 lock, browser 폴더 공유 lock을 관측 전 과정에 걸쳐 유지한다. `browser/` 공유 lock을 먼저 잡은 뒤 profile lock을 기다리도록 순서를 뒤집지 않는다.
    - `BrowserProcesses::start_on`과 `BrowserEnvironment::chromium_on`을 재사용한다. `--user-data-dir=<data>/browser/profile`, `--disk-cache-dir=<generation root>/cache`, `--password-store=basic`이며 `HOME`·`XDG_*`는 임시 폴더다. 저장 프로필에서 cookie를 생성·저장·전송하고 local storage·IndexedDB·service worker·Cache Storage·방문 기록도 남을 수 있다. 저장 프로필 전체는 050의 비밀 파일 보호를 받는다. HTTP cache는 그 저장 프로필 밖의 generation 임시 폴더에 계정 응답을 저장할 수 있고 050의 비밀 파일 보호 밖이다. cache를 끈다거나 그 크기를 1로 둔다고 쓰지 않는다. 비로그인 임시 프로필·cache도 그 보호 밖이며, 042 무저장 계약에서 이 세 저장 영역의 예외와 owner는 SPEC 표를 따른다.
    - 정상 완료·실패·취소·forget·shutdown에서 `BrowserManager::quit`의 `Browser.close` 후 최대 5초, 뒤의 강제 종료와 process group 정리, 임시 폴더 삭제를 따른다. 잠금을 놓은 뒤 Chromium 종료를 기다리는 순서는 금지한다. 저장 profile은 남기고 cache 폴더만 generation과 함께 없어진다.
6. **대화·takeover·forget·maintenance 경합:** PRD 000의 한 자리와 generation 취소 신호를 로그인 감시도 그대로 쓴다. 대화 브라우저나 050 user takeover 중이면 profile lock을 기다리기 전에 busy다. 다른 process가 저장 profile을 쓰면 `browser_unavailable`의 `profile_in_use`이고, 같은 process의 감시 점유와 혼동하지 않는다.
    - 감시 점유 중 대화 호출과 takeover는 즉시 busy다. 로그인 만료 안내가 takeover 방법을 적더라도 실행 중인 감시를 넘겨주지 않는다. 감시가 닫힌 다음 사용자가 요청해야 한다. viewer는 읽기 전용이다.
    - `maintenance`가 slot을 잡지 못해도 `WatchedLocks::check`는 1초마다 돈다. forget의 배타 `forget.lock`, lock 파일이나 `browser/` identity 교체를 보면 그 감시 generation을 취소한다. 읽기 성공을 얻었더라도 종료 요청을 받은 generation의 미완료 관측은 버린다. 성공 fact 기록 전에 042의 lineage 검사도 남는다.
    - `browser_forget.rs::remove`는 그대로다. forget 배타 lock → active 배타 lock → profile 파일 배타 lock → browser 폴더 배타 lock 순서이며, active 대기 시작부터 **한 15초 예산**을 나눠 쓴다. profile 파일 대기는 남은 시간과 500 ms 중 작은 값이다. 마지막 대기 후 모든 경로 identity를 다시 확인한다.
    - 그 뒤 profile이 없으면 종료하고, 있으면 열린 browser descriptor에서 profile을 열어 `singleton_open`의 결정 (나)를 확인한다. profile 폴더 배타 flock은 기다리지 않고 시도한다. 실패하면 `profile_directory_busy`로 보존한다. `browser_profile_directory::remove`는 descriptor 기준 `openat`·`unlinkat`으로 symlink를 따라가지 않고 지우며 마지막 profile 이름의 identity를 재확인한다.
    - active/profile 파일을 교체한 시작 경합도 보장한다. 이미 profile 폴더 lock을 가진 시작은 forget의 삭제를 막고, flock을 기다리던 사이 forget이 삭제했으면 잠금 직후 identity 재검사가 시작을 막는다. active 파일만 교체한 임시 감시도 browser 폴더 공유 lock이 남아 있으므로 forget이 살아 있는 브라우저를 놓치지 않는다.
    - 경로 교체 뒤 기존 050의 `browser_lock_replaced` 진단을 유지한다. 프로필 폴더 자체의 이름을 외부 작업이 Chromium 시작/삭제 도중 바꾸는 경우와 directory flock을 전달하지 않는 파일 공유 구성은 050의 보장 밖이다. 이 한계를 lock 파일 교체의 보호까지 축소하는 이유로 쓰지 않는다.
7. **실패 알림과 저장 범위:** 로그인 감시의 `rendered_content_unavailable`인 실패 알림에 아래 글을 붙인다.

   ```text
   로그인이 풀렸을 수 있습니다. 감시 확인이 끝난 뒤 /browser takeover로 브라우저를 넘겨받아 다시 로그인하세요. 로그인 뒤 돌려주고 /browser close로 닫으면 다음 예정 확인에서 다시 읽습니다. 비밀번호나 인증 코드는 대화로 보내지 마세요.
   ```

    - `/browser return`만 하면 050의 대화 브라우저가 계속 열려 감시가 busy이므로 닫는 단계까지 안내한다. slash 명령이 안 되는 channel에서는 "브라우저 넘겨줘", "브라우저 닫아"라는 기존 대화 경로도 안내한다.
    - 로그인 만료를 판정한 것처럼 쓰지 않는다. HTTP 401/403은 `http_error`, 숫자 모호함/형식 불일치는 각각의 기존 실패이고, 이를 모두 로그인 만료로 바꾸지 않는다. 실패 기준 시간·구간당 한 번·회복·중복 억제는 042 그대로다.
    - 로그인 여부는 모든 종류의 `WebNotifyIntent`에 저장해 `render_web_notification`이 저장된 방식과 로그인 여부로 한계 글을 낸다. 현재 config의 `persistLogin`이나 현재 profile 존재 여부를 읽어 과거 알림의 설명을 바꾸지 않는다.
    - `WebWatchSnapshot`·`plan_view`·대화 상태(`web_watch_conversation_status.rs`)·CLI/API·diagnostics에서 방식·로그인 선택을 확인할 수 있게 한다. `browser_state.rs`의 050 login profile 상태는 프로필의 존재/사용 가능성이지 이 감시가 로그인 사용을 확인받았다는 증거가 아니다.
    - 확인한 값·추출 근거는 042 저장 범위다. 주변 원문·화면·cookie·header를 fact·알림·조회·diagnostics에 추가하지 않는다. URL query/fragment는 기존 가리기를 유지한다. profile 전체에 적용되는 `crates/shacs-core/src/runtime/auto_mode/secret_file.rs`의 비밀 파일 보호와 browser lock 경로 쓰기 금지를 그대로 소비한다.
8. **운영 연결과 사용 문서:** 로그인 선택은 PRD 000의 `RenderedWebReader` 요청을 통해 manager까지 간다. `ProductionTooling`·adapter·`loop_config_for`·gateway worker·CLI/API가 같은 값과 같은 manager를 쓰는지 확인한다. 대화에서는 true인데 worker가 기본 false로 읽는 배선을 허용하지 않는다.
    - `docs/USAGE.md`와 README에 브라우저 자동 제안, 명시적인 로그인 선택, 계정 내용의 저장·Email 발송, 프로필과 HTTP cache의 구분, 로그인 만료의 불확실성, 재로그인 후 닫기, `persistLogin`을 껐을 때 기존 감시가 실패한다는 것, 모든 공유 경합을 적는다.
    - profile의 계정 identity를 고정하거나 자동 로그인·비밀번호 입력·takeover를 자동 시작하지 않는다. 사용자가 로그인 상태를 바꾼 뒤에는 같은 true 계획이 달라진 계정 내용을 볼 수 있음을 적는다.

## Non Scope

- 계정별 프로필, cookie 복사·내보내기, 자동 로그인/갱신, 만료 탐지기, 비밀번호를 대화로 받는 기능.
- 기존 050의 takeover 회차·UTF-8 붙여넣기·비밀 파일 판정·forget 절차의 재설계.
- 클릭해야 나타나는 값, 접근 차단 우회, 사용자 요청 없는 계정 감시.
- production의 사설 주소 허용, 짧은 주기/실패 기준 시간, 임시 HTTP cache의 tmpfs 이전이나 강제 종료 잔여 폴더 청소.

## Parent Requirement Mapping

| SPEC 051 기준 | 이 PRD의 계약 | 수용 조건 |
|---|---|---|
| 소유 범위 1 중 로그인 저장·token·digest·revision·대화 재확인 | Scope 1–4 | 1·2·3 |
| 소유 범위 3의 로그인 감시, 수용 조건 2의 저장 프로필 | Scope 5·6·8 | 4·5·6·8 |
| 소유 범위 5 중 로그인 실패 안내, 수용 조건 1의 로그인 한계 글 | Scope 7 | 7 |
| 소유 범위 6, 수용 조건 4 전체 | Scope 2–4·7·8 | 1·2·3·7·8 |
| 소유 범위 7 중 로그인 표면 | Scope 7·8 | 7·8 |
| 수용 조건 5 전체 | Docker Check | 9 |
| 수용 조건 6 | 구현 후 Cargo 검증·문서 | 10 |

수용 조건 1·2·3의 비로그인 기반은 PRD 000의 닫힌 결과다. 이 PRD는 로그인 추가로 달라지는 경계를 확인하고 그 기반을 다시 소유하지 않는다.

## Required Contract

1. 사용자 선택 전 저장 profile로 미리보지 않는다. 자동 browser 제안은 비로그인이고, 후보 생성 단계부터 보존한 `value_absent` 사유에만 한 번이다. 빈 후보 목록만으로 전환하지 않는다.
2. 요약·token·확인 재읽기·계획·worker·알림에 방식과 로그인 여부가 일치한다. 바꾸면 새 요약 확인과 baseline 재설정이다.
3. 로그인 설정 꺼짐·프로필 잠김·busy·확인 중 값 변경을 성공이나 다른 방식으로 바꾸지 않는다.
4. 로그인 감시도 실제 process gate와 감시 전용 공개 주소 proxy를 지난다.
5. 050의 파일 잠금 셋·프로필 폴더 배타 flock과 잠금 뒤 identity 재검사·browser 폴더 공유 flock·1초 maintenance·결정 (나)·프로필 밖 HTTP cache를 모두 지킨다.
6. forget은 사용 중인 profile을 삭제하지 못하고, 시작은 삭제되거나 바뀐 profile identity를 사용하지 못한다. 모든 정리 뒤에만 잠금과 자리를 놓는다.
7. 계정 값의 저장·발송과 profile에 남는 자료를 요약에서 구분한다. 실패 알림은 로그인 만료를 추측으로만 안내한다.
8. 새 crate와 queue를 만들지 않는다. Cargo는 가짜 DevTools·실행기·provider·시계, 실제 Docker는 승인한 일회용 환경에서만 실행한다.

## Acceptance Criteria

아래 이름들은 구현 예정인 테스트다. 새 파일은 기존 `tests/it/main.rs`에 등록하며 별도 통합 테스트 실행 파일을 늘리지 않는다. 기존 fixture를 공유하고 한 계약은 가장 강한 경계에서 한 번 확인한다.

1. `crates/shacs-core/tests/it/spec051_registration_and_login.rs`의 `static_missing_candidate_offers_rendered_summary_before_confirmation`: 실제 `AgentLoop`·가짜 provider로 정적 preview의 실제 부재 사유 `value_absent`가 전달되어 browser를 한 번 읽고 그 값이 담긴 요약이 나오며, 같은 발신자가 확인한 뒤에만 browser 감시가 생기는지 본다. 직접 browser 요청은 fetch 0회, HTTP 429/guard 거절/명시 fetch에는 browser 자동 읽기 0회, 정적·rendered 본문의 지시 문구가 조건·대상·로그인 여부를 바꾸지 못함을 같은 흐름의 사례로 확인한다.
    - 같은 파일의 `unsupported_static_candidates_never_trigger_browser_fallback`: 기존 `crates/shacs-core/tests/it/spec042_web_watch/failures.rs::signed_fractional_or_range_numbers_are_not_offered_as_one_value`의 HTML과 label 반례를 재사용한다. `-5`, `-₩5`, `.5`, `−3`, `+5`, `₩10,000~12,000`, `₩10,000 ~ ₩12,000`, `10,000-12,000원`, `10,000 원 ~ 12,000 원`, `10,000 ~ -12,000`, `$10 to $12` 모두 후보가 없더라도 `unsupported_number`가 preview와 요약 판단까지 남고 실제 AgentLoop의 자동 browser reader 호출은 각각 0회여야 한다. 추출 재검증 실패·응답 잘림/검색 한도·사유 없는 빈 결과, 같은 label의 실제 부재와 지원하지 않는 값이 섞인 경우도 0회다. 정상적인 값 부재 사례만 위 시험처럼 1회다. 기존 042의 거절 assertion을 약하게 하거나 parser를 흉내 낸 가짜 결과로 반례를 대체하지 않는다.
2. 같은 파일의 `saved_login_requires_explicit_selection_and_matching_confirmation`: `persistLogin=false`에서는 로그인 선택지가 없고 true 요청은 실패한다. true 설정에서도 최초 자동 preview는 임시 프로필 요청이며, 사용자의 로그인 선택 뒤에만 저장 profile 요청과 새 요약이 나온다. 고정 안내·대상·값이 있고 "로그인한 상태로 보기" 선택만으로 등록되지 않는지, 최신 요약 확인 뒤 저장 plan과 reader 요청이 모두 true인지 본다. 예전 token/다른 로그인 field로의 확인은 읽기 전에 거절한다.
3. 같은 파일의 `login_change_reconfirms_and_resets_only_observation_baseline`: 기존 browser 감시의 false→true와 true→false 변경이 새 요약을 요구하고 token·digest·revision·baseline을 바꾸는지 본다. 확인 직전에 값이 바뀌면 새 요약이며, busy/설정 꺼짐이면 등록·수정 전체가 반영되지 않는다. 통지 대상/기준 시간/마감만 변경하면 기존 baseline을 보존한다. 다른 발신자·오래된 요약·재시작 뒤 확인은 042처럼 거절한다.
4. `crates/shacs-cli/tests/it/spec051_login_watch.rs`의 `saved_watch_uses_locked_profile_and_ephemeral_http_cache`: 실제 manager와 가짜 실행기로 true 감시는 050 인자·profile lock 및 profile/browser directory flock을 쥐고, false 감시는 `persistLogin=true`에서도 임시 프로필인 것을 확인한다. 정상·실패·취소 시 저장 generation은 `Browser.close`→최대 5초→강제 정리 순서이며 process·cache가 없어진 뒤 잠금이 풀리고 profile은 남는다.
5. 같은 파일의 `login_watch_obeys_singleton_and_profile_identity_checks`: 다른 profile 소유자의 flock, 다른 hostname·살아 있는 같은 hostname·해석 불가 SingletonLock에서 child 0과 profile 보존을 확인하고, 같은 hostname의 고아 lock은 Chromium 시작이 허용되는지 본다. `watch_start_loses_profile_during_directory_lock_wait`: profile descriptor를 열고 flock을 기다리는 사이 실제 forget이 삭제한 경우와 새 폴더가 생긴 경우 모두 `ProfileUnreadable`, child 0, 재생성 없음이다. 기존 050 시험을 watch 시작 경로로 확장해 동일 검사를 우회하지 못함을 확인한다.
6. 같은 파일의 `forget_and_maintenance_close_saved_watch_before_profile_removal`: 읽기 응답을 멈춘 사이 실제 forget이 요청하고, 1초 maintenance가 slot 밖에서 감시를 취소해 정리한 뒤에만 삭제되는지 본다. `active.lock`·`profile.lock` 교체를 시작 경계에 주입해 profile 폴더 lock이 삭제를 막는 경우, browser 폴더 공유 lock이 생존을 증명하는 경우, 15초 공통 예산 소진 실패도 확인한다. 정리 중 새 감시는 child 0이고, 다른 hostname의 SingletonLock은 lock 파일들이 비어 있어도 삭제되지 않는다. 대화 user takeover 중 로그인 watch는 busy, 로그인 watch 중 takeover는 즉시 busy이며 회차·paste 경로가 생기지 않는 경우를 이 파일의 `saved_watch_never_enters_takeover`에서 확인한다.
7. core 파일의 `login_failure_and_all_notification_limits_use_stored_choice`: `rendered_content_unavailable`이 기준 시간을 넘으면 다시 로그인·돌려주기·닫기 안내가 붙은 실패 알림 하나, 회복 뒤 회복 하나를 만들고 401·403·모호한 값은 로그인 만료로 바뀌지 않는다. 변화·회복·공백·마감의 한계 글도 저장된 true/false를 따른다. `login_watch_status_and_facts_exclude_page_and_credentials`: unique 원문·cookie·query 표식을 넣어 실제 fact·알림·status·diagnostics 출력에 없고 허용된 추출 값과 로그인 선택은 있는지 확인한다.
8. `crates/shacs-cli/tests/it/spec051_web_watch_cli.rs`의 `gateway_runs_the_confirmed_login_choice`와 `crates/shacs-api/tests/it/spec051_web_watch_api.rs`의 `saved_login_selection_survives_preview_confirm_and_status`: PRD 000의 운영 배선 시험을 확장해 CLI/API/대화에서 저장한 true/false가 실제 `from_bundle` 구성의 gateway worker에서 같은 manager·profile 선택으로 실행되는지 확인한다. provider는 등록 대화에만 호출되고 예정 확인에서는 추가 호출이 없다. 외부 ingress에서 `/browser takeover`를 보냈을 때 감시를 놓기 전에 busy 응답이 도착해야 한다. getter나 가짜 reader가 받은 인자만 비교하는 시험으로 대신하지 않는다.
9. 사용자 승인 뒤 `scripts/spec051-web-watch-check.sh --approved`가 아래 Docker Check를 실행한다. 미실행을 사용자가 결정하면 그 결정·이유와 실제 Chromium JS·cookie·SOCKS5·SMTP·container 간 flock 미검증을 종료 기록에 적고 scoped로 닫는다. Cargo 성공만으로 이 수용 조건을 통과했다고 하지 않는다.
10. 구현 후 fmt → workspace clippy `-D warnings` → 변경 package 테스트 순서다. 정확한 명령은 PRD 000 Acceptance Criteria 11과 같으며 `--locked`, 테스트 `-q`를 쓴다. 기본 변경 대상은 `shacs-core`, `shacs-cli`, `shacs-api`이고 실제 바뀐 package만 추가한다. 전체 workspace 테스트는 별도 사용자 승인 뒤에만 실행한다. Docker 시험 feature가 없는 production 구성에서 시험 환경 변수로 사설 주소가 열리지 않는지 `crates/shacs-cli/tests/it/spec051_web_watch_cli.rs`의 `spec051_test_profile_does_not_change_production_guard`로 확인한다. 사용 문서와 새 의존성/lockfile 무변경도 확인한다.

## Docker Check

SPEC 수용 조건 5 전체를 이 PRD가 소유한다. 구현 산출물은 `scripts/spec051-web-watch-check.sh`와 `scripts/spec051-web-watch-fixtures/`이며, 이 문서를 쓴 시점에 존재하거나 실행됐다는 뜻이 아니다.

1. **명시 승인:** `scripts/spec050-saved-login-check.sh`처럼 인자가 정확히 `--approved` 하나일 때만 준비한다. 없거나 잘못됐으면 사용법과 승인 필요를 stderr에 쓰고 exit 2다. 이 검사보다 앞서 Docker·빌드·임시 data 생성·네트워크 생성은 하지 않는다.
2. **격리:** `mktemp -d`의 일회용 data/workspace·고유 compose project·임시 compose 파일을 쓴다. 사용자의 data directory, 실제 cookie, 실제 Email 계정, 저장소 `docker-compose.yml`은 사용하거나 바꾸지 않는다. 설정·provider·SMTP credential은 fixture 값이다. 종료/오류/신호 때 `down --volumes --remove-orphans --rmi local`로 그 project만 정리하고 임시 data를 지운다.
3. **구성:** 공식 browser image의 구성으로 gateway, 별도 page container(`172.30.250.2`), 로컬 SMTP sink를 띄운다. 필요하면 같은 일회용 data volume을 쓰는 exec 전용 service로 cross-container forget을 한다. noVNC를 시험에 열면 host `127.0.0.1`에만 연결한다. SMTP도 외부 relay를 두지 않고 sink가 받은 메시지를 fixture 파일로 보관한다.
    - test image는 명시적으로 `spec051-test-profile`을 켜 Cargo로 만든다. 필요한 build 경로도 스크립트/fixture에 둔다. production Dockerfile의 기본 feature는 바꾸지 않는다. 빌드를 실행할 때 저장소의 clean→build 규칙을 지킨다. 시험 profile은 고정 page IP만 감시 guard에 허용하고 사용자의 `ssrfWhitelist`는 계속 무시한다.
    - 짧은 production 주기를 추가하지 않는다. 최소 600초 주기와 기존 실패 기준 시간을 그대로 두고 042의 파일 시계를 전진시켜 실제 worker를 진행한다. browser의 load/idle/forget 시간은 실제 동작으로 확인한다.
4. **페이지와 로그인 준비:** `/rendered`의 원본 HTML에는 값이 없고 load 뒤 JavaScript 요청이 정해진 가격을 그린다. `/account`도 JavaScript로 그리지만 `spec051_login` cookie가 있어야 지정한 고정 문구와 값이 나온다. cookie가 없으면 고정 문구 없는 로그인 안내다. fixture 제어 경로로 가격을 A→무관 변화→B로 바꾼다.
    - 일회용 profile의 로그인 준비는 050의 대화 브라우저로 test `/login` 응답의 `Set-Cookie: ...; Max-Age=86400; Path=/`를 받은 뒤 닫는다. 실제 서비스 비밀번호를 요구하거나 profile 파일에 cookie를 직접 쓰지 않는다. 대화 브라우저가 page IP를 열 때만 해당 `/32` whitelist를 쓰며, 감시 접근은 시험 profile의 독립 guard로 한다.
    - fixture provider는 대화 등록의 제안과 선택을 정해진 응답으로 돌려준다. 감시 확인 중 provider 요청이 추가되지 않는지 계수를 기록한다.
5. **실제 사용자 흐름:** 같은 gateway에서 `/rendered`를 정적으로 미리보면 후보가 없고 browser 값이 담긴 요약이 나온다. 확인 후 비로그인 감시가 등록된다. 별도 owner session에서 `/account`의 로그인 사용을 명시적으로 고르고 안내가 있는 요약을 확인해 로그인 감시를 등록한다. 두 감시는 각각 첫 기준과 A/무관 변화에서 메일 0, 조건 결과가 바뀐 B에서 확인한 Email 대상으로 변화 메일 **각각 한 통**이어야 한다.
    - SMTP sink의 실제 수신 envelope·메시지 본문·사건 식별자를 대조한다. intent 생성이나 outbound enqueue만으로 발송을 통과시키지 않는다. 같은 시각의 추가 worker pass와 gateway 재시작 뒤 동일 사건 메일이 늘지 않아야 한다. 이 시험의 한 통은 exactly-once 전달의 일반 보장이 아니다.
    - 계정 cookie 표식·페이지 전체의 unique 표식은 메일·status·diagnostics·관측 fact에 없고 확인한 값은 있어야 한다. 두 메일의 한계 글과 로그인 여부는 각각 다르다.
6. **프로필과 경합:** 대화 브라우저를 열어 둔 채 due check를 진행해 busy와 다음 wake를 확인하고, 닫은 뒤 정상 확인을 진행한다. 로그인 감시의 응답을 page fixture에서 지연시키고 다른 channel의 takeover가 즉시 busy인지, 다른 container의 `browser forget --yes`가 읽기를 중단해 process·cache를 정리한 뒤 profile을 지우는지 확인한다. profile 삭제 뒤 다음 로그인 감시는 로그인 문구 부재의 실패이며 값·로그인 성공을 합성하지 않는다.
    - 로그인 감시가 사용하는 Chromium 인자의 profile/cache 경로, 종료 뒤 cache 부재와 profile 유지(별도의 forget 전), lock 해제 순서를 기록한다. 050의 모든 화면 조작 시험을 다시 실행하는 것은 이 PRD의 조건이 아니다.
7. **기록:** 사용한 image/feature·실행 명령·페이지 값 전환·요약과 확인·계획·wake/fact·메일 수와 내용·provider 호출 수·busy/forget 순서·정리 결과를 남긴다. fixture cookie 값도 공개 증거에는 가리고, 사용자의 실제 data를 쓰지 않았음을 경로로 보인다. 어느 검사가 실패하면 스크립트는 nonzero로 끝나며 cleanup 성공을 제품 성공으로 대신하지 않는다.

## Closure Evidence

2026-11-12 구현과 검증을 마쳤다. [IMPLEMENTATION-010.md](../IMPLEMENTATION-010.md)에
Required Contract 8개와 Acceptance Criteria 10개를 코드·시험 위치로 대응했다.
실제 AgentLoop·production registry·gateway worker의 저장 로그인 선택, 외부 ingress의
즉시 busy, 050 잠금과 정리, 후보 부재 사유, 알림과 상태 표면을 확인했다.
14개 보호를 잠시 해제한 실패 검사 뒤 원복했고, 최종 동시 실행 그룹을 각각
10회 연속 통과시켰다. core 전체 검증 중 발견한 요약 회귀와 실행 중 문서 변경에 따른
출시 증거 digest 실패를 고친 뒤 실패·미실행 대상을 모두 확인했다. CLI/API 전체
명령은 종료 코드 0이다. 정확한 명령과 최초 실패까지 구현 기록에 남겼다.
사용자의 2026-11-12 지시에 따라 Docker는 실행하지 않고 스크립트·fixture의 문법만
검사했다. 실제 Chromium JS·cookie·SOCKS5·SMTP·container 간 flock은 미검증이며
수용 조건 9의 실제 Docker 성공으로 기록하지 않는다. 독립 검토는 아직 남아 있다.

1. 실제 AgentLoop의 정적 실패→browser 요약→확인과 명시 로그인 선택→새 요약→확인 기록. 확인 전 저장 profile 읽기가 없다는 증거.
2. token·plan·revision·baseline·worker 요청의 로그인 선택 일치와 변경/취소/오래된 확인 거절 결과.
3. 실제 manager의 잠금·profile identity·SingletonLock 거절·1초 maintenance·정리 순서와 forget의 보존/삭제 결과.
4. 모든 알림 종류의 저장된 로그인 한계 글, 만료 가능성 안내와 실제 값/자격정보 저장 범위 결과.
5. Docker 스크립트의 두 감시와 로컬 SMTP 기록 또는 사용자의 미실행 결정·이유·미검증 범위, Cargo 검증과 사용 문서/lockfile 확인.

PRD 010이 닫히면 051의 두 PRD가 모두 닫힌다. 별도의 후속 스펙이나 종료 PRD를 요구하지 않는다.

## 남은 제한

- 저장 프로필의 어느 계정인지 확인하거나 고정하지 않는다. 사용자가 다른 계정으로 다시 로그인하면 같은 감시가 그 계정 내용을 읽을 수 있다. 계정을 바꿨다면 감시를 중지하고 새 요약으로 다시 확인하는 사용 방법을 적는다.
- 로그인 화면에 같은 고정 문구가 있으면 추출이 성공할 수도 있다. `rendered_content_unavailable` 안내는 만료 탐지나 만료의 증거가 아니다.
- 로그인된 페이지 접속 자체가 사이트에 기록되며 JavaScript가 부수 요청을 보낼 수 있다. Chromium sandbox 꺼짐, 같은 container의 profile, 사실상 평문 cookie, 비밀 경로를 적지 않는 스크립트의 접근은 049·050의 기존 제한이다.
- 저장 profile에는 cookie·사이트 자료·페이지 내용이 남으며 전체가 050의 비밀 파일 보호를 받는다. 비로그인 임시 profile과 HTTP cache는 042 무저장 계약의 별도 예외이고 그 보호 밖이다. 로그인 HTTP cache도 저장 profile 밖 generation 임시 폴더에 계정 응답을 저장한다. 임시 자료는 정상 종료 때 지우지만 강제 종료 때 남을 수 있고 새 시작 때 청소하지 않는다.
- directory flock을 지원하지 않는 공유 계층이나 프로필 폴더 자체의 외부 이름 교체를 보장하지 않는다. 다른 hostname의 SingletonLock은 자동 복구하지 않는다.
- 실제 Docker를 실행하지 않기로 결정하면 실제 Chromium의 렌더링·cookie 유지, network idle의 실제 event 순서, SMTP 전달, container 간 flock은 미검증으로 남는다.
