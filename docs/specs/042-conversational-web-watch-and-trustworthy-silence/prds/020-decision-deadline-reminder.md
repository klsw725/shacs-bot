# PRD 020. 결정 마감 알림

Status: Complete (Scoped)

Depends on: [PRD 010](010-change-failure-recovery-and-gap-notification.md)의 042 알림 대상과 전달 경로, [036](../../036-entrusted-work-continuity-and-change-notification/SPEC.md) 완료 사실. [PRD 000](000-web-page-observation-and-condition-decision.md)의 최신 관측 사실이 있으면 함께 보여주되 없어도 동작한다. 뒤 번호 PRD에는 의존하지 않는다.

## Goal

사용자가 정한 결정 마감이 다가오면 **변화가 없어도 한 번 알린다.** 마감을 정하거나 바꿔도 관측의 비교 기준은 그대로다.

## Scope

1. **마감 revision:** 결정 마감 시각과 알림 시점(기본값: 마감 24시간 전)을 관측 revision과 분리된 `deadline_revision`으로 저장하고 CAS한다. 사용자가 입력한 시간대 표기와 절대 시각을 함께 보존한다.
2. **검증:** 마감 자체가 지났으면 거절한다. 마감은 미래지만 알림 시점이 이미 지났다면 "확인 직후 즉시 알림"을 명시적으로 선택했을 때만 받는다.
3. **알림:** 029 durable work로 알림 시점에 한 번 wake한다. 위임 목표, 마감, 남은 시간, 조건, 마지막 성공 확인 시각과 최신 값(있다면)을 결정적으로 렌더링해 PRD 010의 알림 대상과 전달 경로로 보낸다.
4. **취소와 재개:** 마감·알림 시점 변경, pause/done/clear, 동의 철회 뒤에는 이전 예약을 취소하고 알리지 않는다. resume 때 알림 시점이 아직 미래면 다시 예약하고, 이미 지났으면 소급해 보내지 않는다. 마감이 지난 뒤에도 goal이 active이면 관측은 계속하고 마감 알림을 더 만들지 않는다.
5. CLI/API로 마감 설정·조회·삭제를 제공한다.

## Non Scope

- 마감 자동 추정, 반복 리마인더, 마감 이후의 자동 중단이나 자동 행동.
- 웹 관측과 실패 알림([000](000-web-page-observation-and-condition-decision.md), [010](010-change-failure-recovery-and-gap-notification.md))의 의미 변경, 대화형 요약([030](030-conversational-entrustment-summary-and-confirmation.md)).

## Parent Requirement Mapping

- [042 SPEC](../SPEC.md)의 소유 범위 7과 수용 조건 6을 소유한다.

## Required Contract

1. 마감 알림은 변화 판정과 독립적이며 `notify_candidate`를 만들거나 비교 기준을 바꾸지 않는다.
2. `deadline_revision` 하나에 알림은 한 번이다. 재시작·중복 wake에도 다시 보내지 않는다.
3. 마감 알림 의도는 `deadline_revision`과 PRD 010의 알림 설정 revision에 함께 결속된다. enqueue와 소비 직전에 036과 같은 owner·goal·동의 확인과 두 revision 확인을 모두 거치며, 알림 대상이 바뀌었으면 이전 대상으로 보내지 않는다.

## Acceptance Criteria

1. 실제 runtime, 로컬 SMTP, 시험용 시계로 변화가 없는 위임의 알림 시점에 마감 알림 한 건을 받는다.
2. 마감을 바꾸면 이전 시점에는 알림이 없고 새 시점에 한 건을 받는다. 마감 변경 전후의 관측 판정 기준이 이어진다.
3. 알림 시점 전 pause는 알림을 만들지 않는다. 알림 시점이 지난 뒤 resume하면 알림이 없고, 지나기 전 resume하면 그 시점에 한 건을 받는다. done/clear 뒤에는 알림이 없다.
4. 알림 시점 직전·직후의 재시작과 중복 wake에서 알림이 한 건만 남는다. 지난 마감은 거절되고, 남은 시간이 기본 알림 간격보다 짧으면 즉시 알림을 명시적으로 선택했을 때만 설정된다.

## Closure Evidence

1. 실제 runtime·로컬 SMTP·시험용 시계의 transcript와 수신 메시지.
2. 시간대·경계 시각·취소·재개·중복 억제·재시작·비교 기준 유지 테스트.
3. 사용자 문서에 마감 설정 방법과 비보장 범위를 기록.

PRD 020은 PRD 010의 알림 경로만 소비해 마감 알림 경험을 닫는다. 대화형 확인의 구현을 기다리지 않는다.

## 구현 메모 (2026-10-26)

- **설정:** 위임마다 하나인 마감을 owner session의 `web_watch` metadata(`deadlines`)에 마감 revision과 함께 저장한다. 시각은 오프셋이 있는 64바이트 이하의 RFC 3339만 받고(초 소수부를 길게 늘인 입력이 마감 기록의 64 KiB inline 한도를 넘지 않게) 입력 표기(`at_text`)와 절대 시각(`at_ms`)을 함께 남긴다. 알림 시점은 `마감 - remind_before`(기본 24시간, 60초~30일)이다. 그 시점이 이미 지났으면 `remind_now`를 명시했을 때만 받고 알림 시점을 설정 시각으로 둔다. 확인된 웹페이지 계획과 PRD 010 Email 대상이 있어야 하며(지우기는 예외) `expected_revision` CAS가 맞아야 한다. 관측 revision·계획·첫 기준은 바꾸지 않는다. CLI `work web-deadline`, API `POST .../web-watch/deadline`, `web-status`·`GET`의 `deadline`으로 설정·조회·삭제한다.
- **wake:** worker는 웹페이지 확인 뒤, 알림 단계 전에 마감 단계를 실행한다. 현재 마감이 있고 goal이 active이며(036과 같은 동의·lineage gate), 알림 시점이 현재 goal lineage가 시작된 뒤(resume 시각 이후)이고 마감 전이면 029 durable work(`web_watch.deadline`)로 알림 시점에 한 번 깨운다. work ID는 위임, 마감 revision, goal 전이 ID의 digest로 만든다. 깨어나면 owner session lock 아래에서 같은 마감 revision·알림 시점·lineage인지, 마감 전인지 다시 확인한다. 맞으면 목표·마감·조건·현재 계획의 최신 읽기(같은 계획의 마지막 성공 확인과 확인 순간의 기준 중 최신)를 `web_watch.deadline.v1` terminal 사실로 남기고, 맞지 않는 예약은 취소한다. 목표는 비밀값을 가리고(`redact_string`. 공용 redaction은 이번에 Bearer뿐 아니라 scheme과 관계없이 `Authorization:`·`Authorization=` 값의 credential을, 알려진 scheme 이름 뒤 전부 또는 값 전체를 닫는 따옴표나 줄 끝까지 가리게 했다. `bearer` 같은 민감 key 이름의 값도 가린다) URL마다 scheme·host·path만 남긴 뒤(해석할 수 없는 URL은 통째로 가림. scheme 없이 적은 주소는 사용자 정보가 있으면 통째로 가리고 query·fragment는 자름) 한 줄 240자로 잘라, 메일과 inline 사실에 비밀값이나 URL query·fragment가 남지 않고 크기도 제한되게 한다. 앞쪽 1,024자만 읽어 그 안의 온전한 낱말만 쓰고 256자를 넘는 낱말은 가리므로, 목표가 아무리 길어도 처리하는 양은 1,024자를 넘지 않는다. 문구 조건의 문구도 같은 방식으로(64자) 가려 마감 기록에 넣고, PRD 010 변화 알림의 문구도 같은 함수를 쓴다. 관측 계획과 미리보기 후보의 불투명 token에는 원문을 두고, 미리보기 후보·확인 응답·`web-status`·API·`session export`는 문구와 앞뒤 고정 문구·후보 표시(160자)를 같은 방식으로 가려 보여준다. 마감 기록에는 그 시점의 가린 페이지 주소와 digest, 조건 revision도 남겨 알림에 쓴다.
- **알림:** PRD 010 알림 단계가 현재 마감 revision·현재 lineage의 기록에서 `deadline` 알림 하나를 만든다. 마감 전이어야 하고, 같은 마감 revision의 마감 알림이 (어떤 알림 설정 revision으로든, 억제되지 않은 채) queue에 있으면 만들지 않는다. 알림은 `deadline_revision`과 알림 설정 revision에 함께 묶인다. enqueue 직전과 소비 직전에 lineage(owner·goal·전이·동의·active), 마감 revision, 알림 설정 revision, 대상과 마감 전인지(worker와 agent loop의 시계로)를 확인하며 계획 digest는 보지 않는다. 그래서 마감 전에 queue에 들어간 알림도 마감 뒤에 소비되면 보내지 않는다. 대상이 바뀌어 억제되면 마감 전인 동안 새 설정으로 다시 만든다. 렌더링은 결정적인 영어 문장이다(목표, 마감 UTC와 입력 표기, 남은 시간, 계획한 알림 시점, 조건, 최신 값, 한계, 추적 ID).

### 검증

- `shacs-core --test it spec042_web_watch::deadline::` 12개와 `--lib web_watch_redact` 4개: 변화 없이 알림 시점에 마감 알림 한 통(입력한 +09:00 표기와 절대 시각 보존, 목표·남은 시간·조건·최신 값), 직전 실행에는 없고 재시작해도 다시 없으며 관측 revision과 기준이 유지됨. 마감을 하루 늦추면 이전 시점에는 없고 새 시점에 한 통, 오래된 revision의 CAS는 거절. queue에 든 마감 알림은 마감이 바뀌면 보내지 않고 새 마감이 한 통. pause는 알림 없음, 알림 시점 전 resume은 한 통, 지난 뒤 resume은 없음, done·clear는 없음. 지난 마감·오프셋 없는 시각·초 소수부가 7만 자리인 시각·지난 알림 시점에 즉시 알림 미선택·미래 알림 시점에 즉시 알림·30일 초과·60초 미만(0초, 59초)·대상 없음은 거절, 즉시 알림은 바로 한 통(남은 2시간), 지우기는 새 revision. 마감 뒤 처음 실행되면 알림 없음, 마감 전에 queue에 든 알림을 마감 뒤에 소비하면 억제. 매우 긴 목표(약 8만 자)도 알림이 가고 목표 줄은 240자로 잘림. 목표에 넣은 URL 사용자 정보·query·fragment와 Bearer·Basic credential은 메일과 durable 기록 어디에도 남지 않고 URL은 scheme·host·path만 보임. URL인 문구 조건도 변화 메일과 마감 메일 모두 `https://x.test/a`로만 보이고 durable 기록에 나머지가 없음. 단위 시험은 저장되는 값 자체(사용자 정보·query·fragment·Bearer·Basic credential·경로 안 URL·scheme 없는 `://`·줄바꿈), scheme 없이 적은 주소의 사용자 정보·query·fragment(`shop.example/item?…`, `u:pw@host/…`, `//u:pw@host`, `//alice@host/…`, `bob@host/path`; `why?`·`C#`·`#tag`·메일 주소는 그대로), 한글 바로 뒤 URL의 panic 없는 처리, 아무리 긴 입력도 앞쪽 1,024자만 읽는 것을 확인. 알림 시점이 설정 시각과 같으면 즉시 알림 선택 없이 설정되어 바로 한 통. 대상이 바뀌면 이전 대상은 억제되고 새 대상이 한 통.
- `shacs-cli --test it spec042_web_watch_cli::` 4개 중 `web_deadline_is_set_checked_and_cleared_from_the_command_line`: 설정(입력 표기 보존, 1시간 전 알림), 오래된 revision과 오프셋 없는 시각 거절, 지우기. `shacs-api --test it spec042_web_watch_api::` 1개: 대상 없는 마감 400, 지우기 200, 상태의 `deadline`. `spec042_web_watch::observation::urls_in_the_phrase_and_its_anchor_are_shown_reduced_but_stored_as_matched`: URL인 문구와 query가 있는 링크가 든 앞 고정 문구가 미리보기 후보·확인 응답·상태·내보내기용 metadata에서 scheme·host·path로만 보이고, 저장된 기준으로 다음 확인이 같은 값을 읽음. `shacs-redaction` 단위 시험: Basic, scheme 없는 값, 모르는 scheme, Digest 매개변수(따옴표 안의 escape 포함), 따옴표 key의 `Authorization` credential을 가리고 줄 바꿈 뒤와 닫는 따옴표 뒤는 남기며, 두 번 가려도 같음. Bearer 토큰은 `~`·`+`·`/`·`=`가 섞여도 전부 가림. 따옴표 안 비밀값은 escape된 따옴표에서 끝나지 않고, RSA·EC·OpenSSH·암호화·PGP private key 블록도 통째로 가림(인증서는 그대로). `Authorization=`(따옴표 유무 무관), 문자열 안 JSON의 `\"authorization\": \"Basic …\"`, 탭 뒤의 Bearer 토큰, `bearer:` key, `x-authorization-token:` key의 값도 가림. 회귀: `spec042_web_watch::` 전체, `spec036_notification`, `spec036_observation`, `durable_dispatch`, `runtime_loop`, `permission_action`, `shacs-session`·`shacs-redaction`·`shacs-projection` 전체 통과.
- 실제 표면: `.omo/evidence/spec042/prd020/qa-run/qa/transcript.md`. `cargo clean`부터 빌드한 시험 profile binary로 실제 channel `runtime start`(Email 채널 SMTP를 로컬 수신기로), 바뀌지 않는 로컬 페이지, 시험용 시계를 실행해 22개 확인이 모두 통과했다. 확인 주기 1일, 실패 기준 7일로 두어 시계 이동이 공백 메일을 만들지 않게 했다. 확인한 내용은 다음과 같다. 마감은 +09:00 표기와 절대 시각을 함께 남기고 알림 시점은 하루 전이다. 그 1분 전에는 메일이 없고, 그 시점에 목표·입력한 마감·남은 시간·조건·최신 값·추적 ID를 담은 메일이 한 통 가며, 재시작해도 다시 가지 않는다. 마감을 두 번 바꾸면 바뀌기 전 시점에는 없고 새 시점에 한 통이며, 관측 revision과 기준은 그대로다. pause 중 알림 시점이 지나고 그 뒤 resume하면 메일이 없다. 알림 시점 전에 resume하면, 알림 시점 직전에 runtime을 멈췄다가 지난 뒤 켜도 한 통 가고, 다시 재시작해도 한 통뿐이다. 지난 마감과 오프셋 없는 시각은 거절되고, 남은 시간이 기본 알림 간격보다 짧으면 즉시 알림을 명시했을 때만 설정되어 바로 한 통 간다. done 뒤에는 메일이 없다. 모든 메일은 `user@example.test`로 간 마감 알림이고 URL query를 담지 않는다.
- 남은 제한: 알림은 Email 대상 하나로만 가고 메일은 channel runtime만 보낸다. 원격 수신 확인과 exactly-once는 없다. 반복 리마인더와 마감 자동 추정, 마감 뒤 자동 행동은 없다. 알림 시점에 어시스턴트가 꺼져 있었다면 마감 전에 다시 켜졌을 때 늦게 한 통 가고, 마감이 지났으면 가지 않는다. 비밀값 가리기는 알아볼 수 있는 형식(URL 사용자 정보·query·fragment, `Authorization` 값, Bearer 토큰, 공용 redaction이 아는 민감 key 이름과 token 접두사, private key 블록)만 다룬다. 그 밖의 key 이름(`pwd` 등)이나 문장 속에 그냥 적은 비밀값은 가리지 못한다.

## 종료 확인 (2026-10-26)

- 범위: 결정 마감 설정(마감 revision·CAS, RFC 3339 입력 표기와 절대 시각, 알림 시점 60초~30일, 즉시 알림 규칙, CLI/API), 029 wake로 알림 시점에 한 번 깨우는 예약·취소·재개 규칙, PRD 010 경로의 마감 알림 파생·enqueue·소비와 마감 시각 확인(agent loop 시계 포함). 사용자·페이지 문구를 사실과 출력에 남기기 전에 가리는 처리(`safe_text`: 알림, 미리보기 후보, 확인 응답, 상태, API, `session export`)와 공용 redaction 보강(scheme과 관계없는 `Authorization` 값, Bearer 토큰 전체, escape된 따옴표, 모든 private key 블록)도 포함한다. 대화형 확인([030](030-conversational-entrustment-summary-and-confirmation.md))은 포함하지 않는다.
- 최종 검증(코드 커밋 `880496b`, 기록은 `.omo/evidence/spec042/prd020/final-tests/`):
  - 사용자 승인을 받은 전체 workspace 실행(`cargo test --workspace --no-fail-fast`, 1,284초): 3,288 passed / 1 failed / 14 ignored. 유일한 실패는 043과 PRD 010 종료 때 기록한 baseline 실패다(`runtime::generated_media_release::runner::fixture::tests::mismatch_uses_isolated_tree`). git이 추적하지 않는 Spec034 로컬 evidence 파일 `.omo/evidence/spec034/task-12-integration.json`이 없어서 실패하며, 이번 변경과 관계없다.
  - `cargo fmt --all -- --check`, workspace clippy `--all-targets -D warnings`, `shacs-cli --features spec042-test-profile` clippy가 통과했다.
- Oracle 독립 검토: **PASS** (9차, 차단 지적 없음). 1~8차의 차단 지적은 모두 고치고 회귀 테스트와 mutation 확인을 더했다. 고친 항목은 다음과 같다: 마감 뒤에 소비되는 알림, 0초 알림 간격, 긴 목표의 inline 한도, 목표의 비밀값과 URL query, 설정 시각과 같은 알림 시점, 한글 바로 뒤 URL의 panic과 처리량, 문구 조건의 문구, 상태·내보내기·미리보기의 문구와 고정 문구, scheme과 관계없는 `Authorization` 값(`=`, 따옴표, escape 포함)과 Bearer 토큰 문자, escape된 따옴표 안의 비밀값, 모든 종류의 private key 블록, scheme 없이 적은 주소, 마감 입력 길이. 회차별 기록은 `.omo/evidence/spec042/prd020/review/gates.md`에 있다.
- 실제 표면: `.omo/evidence/spec042/prd020/qa-run/qa/transcript.md` 22/22 PASS. QA binary는 코드 커밋 `880496b`의 crates와 같은 소스(QA 전후 작업 트리 digest `182e61bf…`)로 `cargo clean`부터 빌드했다.
- 남은 제한: 위 "검증"의 남은 제한과 같다.
