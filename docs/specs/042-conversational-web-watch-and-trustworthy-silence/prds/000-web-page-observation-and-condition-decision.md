# PRD 000. 웹페이지 관찰과 조건 판정

Status: Complete (Scoped)

Depends on: [036](../../036-entrusted-work-continuity-and-change-notification/SPEC.md) 완료 사실. 뒤 번호 PRD에는 의존하지 않는다.

## Goal

사용자가 지정한 공개 웹페이지 한 곳에서 **사용자가 확인한 기준으로 값 하나를 읽고, 근거와 함께 기록하고, 함께 정한 조건으로 판정**한다. 확인에 실패해도 확인을 멈추지 않고 실패 구간을 남긴다. **알림 없이 CLI/API만으로 동작하고 완료 판정할 수 있어야 한다.**

## Scope

1. **Source:** 036 위임에 `web_page` 관측 source를 설정한다. http/https URL만 받고 userinfo가 있는 URL은 거절한다. [010](../../010-host-safety-permissions-and-secrets/SPEC.md)의 network guard와 redirect·크기·시간 제한을 그대로 적용하고 최종 URL도 같은 검사를 거친다. production에서는 `tools.ssrfWhitelist`와 관계없이 공개 주소만 허용하며, 로컬 주소 허용은 production 빌드에서 쓸 수 없는 종료 시험용 profile에만 둔다. cookie·인증 header·credential은 보내거나 저장하지 않는다. 관측 계획은 확인한 전체 URL을 보관하되 조회 출력에서는 query·fragment를 가린다.
2. **등록 후보:** 설정 전에 페이지를 한 번 읽어 볼 값 후보, 지금 값, 위치 설명을 보여준다. 후보는 결정적 파서로 만들거나, 모델을 쓰는 경우 도구가 꺼진 구조화 단계에서 값·위치 후보만 출력하게 한다. 이 단계는 URL·조건·주기·알림 대상·도구 호출을 만들 수 없다.
3. **v1 추출 기준:** `normalized_static_text_v1`은 script·style을 제외하고 HTML 태그를 제거한 뒤 entity를 해석하고, Unicode NFC로 맞추고, 연속 공백을 공백 하나로 줄인 텍스트다. 대소문자는 유지하고, 크기 제한으로 잘린 응답은 잘림 사실을 기록한다. 추출 기준은 이 텍스트에서 앞뒤 고정 문구와 occurrence로 지정한 단일 영역이다. 숫자는 확인된 소수점·천 단위 문자, 통화나 단위, 고정소수 값으로 저장한다. 범위 표기, 후보 여러 개, 형식이나 통화 불일치는 `unknown`이다. 문구 조건의 관측값은 확인된 영역 안에 사용자가 확인한 문구가 정확히 있는지를 나타내는 boolean이다. 잘린 범위까지 포함해 정적 내용에서 기준을 찾지 못하면 원인을 로그인이나 JavaScript라고 단정하지 않고 `static_content_unavailable`로 기록한다.
4. **첫 기준:** 사용자가 후보를 확인하면 같은 URL·기준으로 다시 읽는다. 보여준 값과 같으면 그 결과를 첫 `baseline`으로 저장하고 반복 wake를 만드는 일을 원자적으로 처리한다. 다르면 새 후보로 다시 확인받는다. 확인 전 미리보기는 관측 사실이 아니다.
5. **주기 확인과 판정:** 036·029의 wake로 확인한다. provider·agent turn·범용 도구 호출 없이 watcher 전용 fetch·parser만 사용한다. 조건은 숫자의 `at_most`·`at_least`, 문구의 `text_present`·`text_absent`이며, 036 PRD 020의 `baseline`·`suppress`·`notify_candidate`·`unknown`·`blocked` 의미와 기준 갱신 규칙을 따른다. 생명주기 전이도 036 계약을 따르며, `resume` 뒤에는 다음 비교 가능한 관측이 새 `baseline`이다.
6. **근거와 저장 범위:** 관측마다 query·fragment를 뺀 요청 URL과 전체 URL digest, 읽은 시각, 추출 기준 버전을 남긴다. 응답을 받은 경우에만 같은 방식으로 가린 최종 URL, HTTP 상태, 정규화 텍스트의 digest, 잘림 여부, 추출 값 또는 boolean을 남긴다. 응답 본문과 주변 문구는 영속 저장하지 않으며, 모든 조회·진단 출력은 query·fragment를 가린 URL만 보여준다.
7. **실패 구간과 계속 확인:** 실패를 `network_error`, `guard_blocked`(실행 중 network guard가 막은 주소나 redirect), `http_error`, `rate_limited`, `static_content_unavailable`, `ambiguous_value`, `format_mismatch`로 분류한다. 성공한 확인 뒤 첫 실패에서 실패 구간이 시작되고 성공한 추출만 구간을 닫는다. runtime이 멈춰 예정 확인이 확인 주기 이상 밀린 기간은 실패가 아니라 `runtime_gap`으로 기록하며 실패 구간을 닫거나 나누지 않는다. 재시작 때 밀린 확인은 바로 실행한다. 실패해도 다음 확인을 예약한다. 한 번의 확인에서 redirect 경로 하나만 요청하고 즉시 재시도하지 않으며, `Retry-After`와 429·503 backoff를 확인 주기보다 우선한다. goal·동의·stale gate의 `blocked`는 실패 구간에 넣지 않는다. pause/done/clear와 관측 revision 변경은 열린 구간을 알림 없이 닫는다.
8. **상태 조회와 주기:** CLI/API로 등록, 관측·판정 이력, 마지막 성공 확인 시각, 진행 중인 실패 구간(시작 시각·횟수·분류), 기록된 `runtime_gap`, 다음 확인 시각을 조회한다. production 확인 주기는 10분 이상 1일 이하이고, 같은 위임의 확인은 기존 029 lease로 겹치지 않게 한다.

## Non Scope

- 사용자 알림과 042 알림 대상([010](010-change-failure-recovery-and-gap-notification.md)), 결정 마감([020](020-decision-deadline-reminder.md)), 대화형 요약과 확인([030](030-conversational-entrustment-summary-and-confirmation.md)).
- JavaScript 실행, 로그인 세션, 사설 주소의 페이지, 여러 값·여러 페이지, 사이트별 전용 연동, robots·접근 차단 우회.
- 036의 로컬 파일 관측과 정수 비교 의미 변경.

## Parent Requirement Mapping

- [042 SPEC](../SPEC.md)의 소유 범위 2·3·4와 수용 조건 2·3을 소유한다.

## Required Contract

1. 주기 확인은 확인된 추출 기준만 사용한다. 기준이 맞지 않으면 다른 위치의 값을 대신 고르지 않는다.
2. 관측 revision(URL·추출 기준·조건·주기)은 036의 CAS와 연결 대상 경계를 따른다. revision이 바뀌면 첫 기준 절차를 다시 거치고, 이전 revision의 값을 새 조건의 변화로 해석하지 않는다.
3. pause/done/clear, 철회된 동의, stale goal binding의 wake는 새 관측을 남기지 않는다. 036 PRD 010과 같은 gate를 사용한다.
4. 페이지 내용은 신뢰하지 않는 자료다. 주기 확인의 provider 호출 수는 0이고, 등록 후보 단계의 출력은 값·위치 후보로 제한된다.
5. 실패는 성공이나 무변화로 합성하지 않는다. `unknown`과 `blocked`는 기준을 갱신하지 않는다.

## Acceptance Criteria

1. `127.0.0.1/32`만 허용한 시험용 profile과 숫자 IP URL의 로컬 HTTP 서버로 미리보기 A를 확인하면, 확인 순간 다시 읽은 A가 `baseline`이 된다. 이어 시험용 시계로 실제 worker의 due마다 A → 조건 무관 변화 → B를 만들면 `suppress`·`suppress`·`notify_candidate`로 조회되고 근거가 남는다. 사용자 메시지는 생기지 않는다. 확인 순간 값을 바꾸면 기준을 저장하지 않고 다시 확인을 요구한다.
2. 숫자 형식(천 단위 구분·소수·통화), 통화 불일치, 후보 중복, 구조 변경, 잘린 응답, 404, timeout, redirect 초과, `Retry-After`가 붙은 429를 만든다. 각각 정해진 분류로 남고, 실패 구간이 시작·유지·종료되며, 다음 확인은 backoff를 지켜 계속 예약된다. 회복 뒤 판정은 036 규칙과 같다.
3. 시험용 profile 없이, `tools.ssrfWhitelist`에 사설 대역을 넣은 config에서도 내부 주소, 사설 주소로의 redirect, userinfo URL, http/https 이외 scheme이 설정 단계에서 거절된다. 실행 중 사설 주소로 바뀐 redirect는 `guard_blocked`로 기록된다. 페이지에 넣은 지시 문구는 등록 후보·설정·판정에 영향을 주지 않고, 주기 확인 동안 provider 호출이 없다.
4. 재시작 뒤 같은 위임 ID로 추출 기준·기준값·마지막 성공 시각·실패 구간·다음 확인이 복원된다. 확인 주기 이상 멈췄다가 다시 켜면 `runtime_gap`이 기록되고 밀린 확인이 바로 실행되며, 열린 실패 구간은 이어진다. pause/done/clear 뒤의 wake는 관측을 남기지 않고 열린 실패 구간을 닫는다.

## Closure Evidence

1. 로컬 HTTP 서버와 시험용 시계를 쓴 실제 worker 실행의 관측·판정 transcript와 CLI/API 조회 결과.
2. 정규화 규칙·추출 기준·숫자 해석·실패 분류·backoff·network guard 거절·재시작 복원·stale wake 차단 테스트.
3. 사용자 문서에 지원 범위와 비보장 범위(JavaScript·로그인·사설 주소·우회 미지원, 저장하지 않는 자료)를 기록.

PRD 000은 **웹페이지 한 곳을 정해진 기준으로 계속 확인하고 판정하는 경험**만으로 종료된다. 알림이나 대화형 확인의 구현을 기다리지 않는다.

## 구현 메모 (2026-10-26)

- **저장과 결속:** 웹페이지 계획은 036 `EntrustedWork` 형식을 바꾸지 않고 같은 owner session의 별도 metadata `web_watch`에 위임 ID별로 저장한다. 계획에는 확인한 전체 URL, 추출 기준, 조건, 주기, 첫 확인 시각이 있고, 확인 순간 다시 읽은 결과가 첫 기준으로 함께 저장된다. 확인은 036 `current_revision` CAS를 거쳐 revision을 하나 올리고(조건 문구·연결 대상은 그대로 복사) 계획·첫 기준과 같은 session 쓰기 한 번에 저장된다. 반복 wake는 저장된 계획에서 파생되어 worker가 등록하므로 첫 기준과 wake 생성이 나뉘지 않는다. 관측 lineage는 036과 같이 (revision, 최신 goal 전이)다. 계획을 바꾸려면 미리보기와 확인을 다시 거치고, 일반 `work revise`나 resume으로 lineage가 바뀌면 다음 성공 확인이 새 기준(`lineage_reset`)이 된다. 036 로컬 파일 계획이 있는 위임에는 웹페이지를 설정할 수 없고, 웹페이지가 있는 위임에는 036 `observe`가 거절된다.
- **읽기 권한의 근거:** 주기 확인은 확인된 위임(036 동의 근거, 활성 goal, wake의 계획 digest)이 허용하는 watcher 전용 GET이다. `RuntimeToolExecutor`나 `web_fetch` 도구 권한을 거치지 않는다(기본 permission mode는 headless `net_outbound`를 거절하며 위임 확인은 tool permission grant가 아니다). 036 PRD 010과 같이 031 실행 snapshot provenance와 030 `tool:before` hook(`web_watch_fetch`, 인자는 query를 뺀 주소)을 거치며, hook 거절은 `blocked`로 기록하고 다음 주기에 다시 확인한다.
- **네트워크 경계:** 설정 단계는 http/https, userinfo 없음, 공백·제어 문자 없음, 2,048자 이하, 010 network guard를 확인한다. 실행 중에는 첫 요청을 포함한 매 요청 직전에 주소·scheme·userinfo를 다시 검사하며 redirect를 직접 따라간다(최대 5회). 한 번의 확인 전체가 20초 한도 하나를 나눠 쓴다. 환경 변수의 프록시(`ALL_PROXY`·`HTTPS_PROXY`·`HTTP_PROXY`)는 쓰지 않는다. 프록시를 쓰면 guard가 검사한 주소 대신 프록시에 연결하고 프록시 인증 정보를 보낼 수 있기 때문이다. redirect 주소는 010의 공용 `resolve_redirect_url`로 푼다. 이 helper가 query만 있는 `Location`(`?page=2`)을 원래 경로를 버린 주소로, 대문자 scheme이나 http/https 이외 scheme의 절대 주소(`HTTP://…`, `ftp://…`)를 같은 사이트의 상대 경로로 풀던 문제를 함께 고쳤다. 이제 RFC 3986 scheme 문법으로 절대 주소를 먼저 알아보고 http/https(대소문자 무관, `//` 필요)가 아니면 거절한다(웹페이지 확인은 `http_error`). fragment만 있는 주소는 원래 path와 query를, query만 있는 주소는 원래 path를 유지하고, 상대 경로는 원래 주소의 query를 빼고 푼다. `../`는 이전처럼 그대로 붙여 서버에 맡긴다. 같은 helper를 쓰는 `web_fetch` 도구와 생성 미디어 원격 출력도 같은 규칙을 따른다. 제품 surface(CLI, API, worker)는 항상 `NetworkGuard::default()`를 쓰고 config의 `tools.ssrfWhitelist`를 읽지 않는다. `shacs-cli`의 `spec042-test-profile` feature로 빌드한 binary만 `SHACS_SPEC042_ALLOW_LOOPBACK=1`일 때 `127.0.0.1/32`를 허용하고 `SHACS_SPEC042_CLOCK_FILE`의 ms를 현재 시각으로 쓴다(036 CLI 시각 포함). 이 feature를 켜는 workspace 의존성은 없다.
- **`normalized_static_text_v1`:** HTML tokenizer처럼 왼쪽부터 한 번에 읽으며 주석과 `script`·`style` 요소(닫는 태그가 없으면 끝까지)를 지우고, 블록 태그(`p`, `div`, `li`, `td`, `tr`, `h1`–`h6`, `br`, `title` 등 정해진 목록)는 공백 하나로, 나머지 태그와 `<!…>`·`<?…>`는 빈 문자열로 바꾼다. 태그 이름은 공백·`/`·`>`에서 끝나므로 `<script-widget>`·`<div-card>` 같은 사용자 정의 태그는 `script`나 블록 태그가 아닌 일반 태그다. 태그(`script`·`style` 시작 태그 포함)의 `=` 바로 뒤 따옴표 값 안에 있는 `>`, `<!--`, `</script>`는 태그를 끝내거나 주석·요소를 열지 않는다. 그 뒤 `html-escape`로 `;`로 끝나는 이름·숫자 문자 참조를 풀고 `unicode-normalization`으로 NFC를 적용하며 Unicode 공백 연속을 공백 하나로 줄이고 앞뒤를 자른다. 응답은 1 MiB까지 읽고(잘림 기록), 문자 집합은 MIME 종류와 관계없이 HTTP `charset`만 따른다. ureq는 `text/*`만 바꾸므로 그 밖의 형식은 같은 이름 규칙(`encoding_rs`의 `for_label`)으로 직접 바꾼다. charset이 없거나 모르는 이름이면 손실 허용 UTF-8로 읽는다.
- **후보와 추출 기준:** 숫자 후보는 `--label` 일치 뒤 80자 안의 첫 숫자다. 그 숫자를 읽을 수 없으면 후보가 없고 더 뒤의 숫자를 대신 고르지 않는다. 범위나 이어지는 숫자는 한 값으로 내지 않는다. 값 다음 단어에 숫자가 있거나, 값 뒤에 범위 기호(`~`, `～`, `〜`, `-`, `−`, `–`, `—`)나 범위 단어(`to`, `부터`, `에서`)와 다른 숫자가 이어지면(사이에 띄어 쓴 단위 단어 하나, 다른 숫자 앞의 통화·부호·앞 소수점 허용) 후보가 아니다. 확인 때 token의 뒤 고정 문구도 같은 규칙으로 검사하고, 확인 순간과 주기 확인의 추출도 뒤 고정 문구부터 이어지는 텍스트에 같은 규칙을 적용한다. 그래서 `가격 12,000 원 배송`이 `가격 10,000 원 ~ 13,000 원 배송`으로 바뀌면 `10,000`이 아니라 `ambiguous_value`다. 뒤 고정 문구 바로 뒤에 다른 숫자와 같은 뒤 고정 문구가 다시 오면(`10,000 원 13,000 원`, 취소선으로 남긴 옛 가격) 같은 자리의 두 번째 값으로 보고 역시 `ambiguous_value`다. 다른 단어 뒤의 숫자(`배송비 3,000원`)는 다른 항목이라 막지 않는다. 앞 고정 문구는 label 시작부터 값 직전까지, 뒤 고정 문구는 값 다음 단어(앞 공백 제외, 최대 24자)다. 영역 경계의 공백은 앞 고정 문구에만 두고 읽을 때 영역 앞뒤 공백을 자르므로, 값이나 문구만 지워지면 빈 영역을 찾는다(숫자는 `format_mismatch`, 문구는 `present=false`). 문구만 주면 지금 그 문구가 있는 자리의 앞 단어(최대 32자)와 뒤 단어가 고정 문구다. 문구와 함께 `--label`·`--until`(API `label`·`until`)을 주면 두 문구 사이 영역마다 지금 문구가 있는지를 후보로 낸다. 그래서 아직 없는 문구도 `present=false`로 확인해 나타남을 볼 수 있다. 추출은 앞뒤 고정 문구 사이 영역(최대 120자)을 왼쪽부터 겹치지 않게 찾고, 일치 개수가 확인 때와 같을 때만 그 occurrence를 읽는다. 숫자 형식은 미리보기에서 정해 보여준다. 구분 문자가 두 종류면 뒤의 것이 소수점이고, `.`·`,` 하나 뒤에 정확히 세 자리가 오면 선호 소수점(기본 `.`, `--decimal-comma`이면 `,`)에 따른다. 구분 문자가 없으면 관례 쌍(`.`/`,` 또는 `,`/`.`)을 쓴다. 단위는 숫자에 바로 붙은 문자·통화 기호·`%`(8자 이하)다. v1은 부호 없는 수 하나만 읽으므로 `-`, `−`, `+`, `＋`, `±`, `.`, `,`와 범위 기호 바로 뒤의 숫자(앞 단위 포함)는 후보로 내보내지 않고, 앞 고정 문구가 그 문자로 끝나는 token도 받지 않는다. 값은 앞자리 0과 뒤쪽 소수 0을 뺀 고정소수 문자열로 저장하고 비교는 자릿수 기준으로 한다. 후보는 같은 텍스트에서 실제로 같은 값이 다시 추출될 때만 내보낸다. 후보 token은 URL digest·추출 기준·보여준 값을 담은 base64url JSON이며 확인 때 형식을 다시 검사한다.
- **판정:** 036의 verdict 어휘를 쓴다. 비교 대상은 같은 lineage·같은 계획의 마지막 성공 기록, 없으면 같은 lineage의 확인 기준이다. 조건 충족 여부가 바뀌면 `notify_candidate`, 같으면 값이 같을 때 `equivalent_value`, 다르면 `same_condition_outcome`이다. 실패는 `unknown/check_failed`, hook·snapshot 거절은 `blocked`이며 둘 다 기준을 바꾸지 않는다.
- **실패·대기·공백:** 분류는 guard 거절 → `guard_blocked`, 이름 풀이·연결·시간 초과(20초) → `network_error`, 429 → `rate_limited`, 그 밖의 2xx 이외 응답·redirect 초과·잘못된 redirect → `http_error`, 앞뒤 고정 문구 없음 → `static_content_unavailable`, 일치 개수 변화·영역의 여러 숫자·뒤 고정 문구 뒤로 이어지는 범위나 같은 자리의 두 번째 값 → `ambiguous_value`, 숫자 없음·단위나 구분 문자 불일치 → `format_mismatch`다. 다음 확인은 항상 예약한다. 429와 503에는 `max(주기×2^(연속 횟수−1)` (최대 하루), `Retry-After`(최대 7일))를 쓴다. due보다 주기 이상 늦게 실행한 확인에는 `runtime_gap {from: due, to: 실행 시각}`을 남긴다. 새 lineage의 첫 확인 due는 `max(첫 확인 시각, goal 전이 시각, revision 변경 시각)`이다. 원래 첫 확인 시각이 이미 지났다면 resume·revise 직후 확인하고, 그 due보다 주기 이상 늦게 실행되면 꺼져 있던 시간이 공백으로 남는다. 실패 구간은 lineage 안에서 성공 뒤 첫 실패로 열리고 성공으로 닫히며, 조회 때 lineage가 바뀌었거나 goal이 비활성이면 닫힌 것으로 표시한다(닫힌 시각 없음).
- **조회:** CLI `work web-preview|web-watch|web-status`, API `POST|GET /v1/sessions/<session>/entrusted-work[/<owner>]/<work>/web-watch[/preview]`. 모든 출력의 URL은 query·fragment를 뺀 주소와 전체 URL의 SHA-256이다. `session export`(JSON·JSONL)도 저장된 계획 URL을 같은 형태(`url`, `url_digest`)로 바꿔 내보낸다. 029 durable work 종류는 `web_watch.observation`이며 channel 재시작 outbound 투영에서 036 관측처럼 제외한다. worker는 036 관측과 036 알림 enqueue를 마친 뒤 웹페이지 확인을 실행해 웹페이지 쪽 오류가 036 알림을 막지 않게 한다.

### 검증

- `shacs-core --lib web_watch` 9개(같은 자리의 두 번째 값 판별 포함): 정규화 규칙(주석·`script`·`style`, 블록 공백, 태그 제거 뒤 문자 참조, NFC, 따옴표 속성값의 `>`, 속성값 속 `</script>`·`<!--`·`<script>`가 요소를 끝내거나 열지 않음, `<script-widget>`·`<style-guide>`·`<div-card>`의 내용 유지), 숫자 형식 추론과 확인된 형식의 거절, 값을 잇는 숫자·범위의 판별(`배송비 - 무료`, `/월` 같은 정상 문구는 허용).
- `shacs-core --test it spec042_web_watch::` 16개(자식 프로세스용 무시 테스트 1개 별도): 단위를 띄어 쓴 가격이 범위로 바뀌거나 취소선 옛 가격이 앞에 붙으면 확인 순간과 worker 확인 모두 `ambiguous_value`·`unknown`, `ftp://`로의 redirect가 같은 사이트의 엉뚱한 경로를 읽지 않고 웹페이지 확인에서는 `http_error`, `web_fetch` 클라이언트에서는 오류가 됨, 프록시 환경 변수(인증 정보 포함)를 준 자식 프로세스의 미리보기가 프록시 연결 없이 페이지를 직접 읽음, 따옴표 속성값과 따옴표 속성이 있는 `script` 안에 숨은 가격을 후보로 내지 않는 확인 기준과 A·무관 변화·B의 `suppress`·`suppress`·`notify_candidate`, 확인 사이 값 변화의 거절, 문구 조건(영역이 없어지면 `static_content_unavailable`, 문구만 지워지면 `present=false`), 아직 없는 문구 영역의 확인과 첫 출현(`false → true`), EUC-KR을 선언한 `text/html`·`application/xhtml+xml`의 같은 후보, 실패 11종의 분류·예약·대기·한 구간과 회복 판정, redirect 사슬 전체가 하나의 시간 한도를 나눠 씀, 저장된 계획에 userinfo가 끼어들면 요청 없이 `guard_blocked`, 부호(`-`, `+`)·앞 소수점·범위(기호, `to`, 부호가 붙은 뒷값) 숫자를 후보로 내지 않고 그 자리에 뒤의 숫자도 내지 않음, 설정 단계의 내부 주소·userinfo·scheme·사설 redirect 거절, 재시작 뒤 한 번의 늦은 확인과 공백·이어지는 구간, pause 뒤 관측 없음·구간 닫힘·resume 뒤 새 기준, 036 `observe` 거절.
- `shacs-security --lib validates_redirects_and_detects_internal_urls_in_text`: query만 있는 redirect는 원래 path를, fragment만 있는 redirect는 원래 path와 query를 유지함, 대문자 scheme 절대 주소, 원래 주소 query 속 `/`를 무시한 상대 경로, `ftp://`·`javascript:`·`http:relative` 거절, 기존 `../c` 결과 유지. `shacs-core --test it spec034_remote_output::adversarial::a_redirect_to_another_scheme_is_an_invalid_url`: 생성 미디어 원격 출력은 `ftp://` redirect를 `InvalidUrl`로 거절하고 두 번째 요청을 하지 않음.
- `shacs-api --test it spec042_web_watch_api::` 1개, `shacs-cli --test it spec042_web_watch_cli::` 2개: route·CAS·mutation opt-in·URL 가림, API의 문구 영역(`until`) 미리보기, 제품 CLI의 로컬 주소 거절, 확인된 계획이 있는 세션의 `session export` JSON·JSONL에 query·fragment가 없고 가린 주소와 digest가 있음.
- 실제 표면: `.omo/evidence/spec042/prd000/qa-run/qa/transcript.md`. 독립 검토 2~9차 지적(아직 없는 문구, 문구만 지운 영역, `text/*` 이외 형식의 charset, 환경 변수 프록시, `session export`의 URL, 첫 페이지의 범위 표기, `+` 부호와 부호가 붙은 범위 뒷값, 따옴표 속성값 속 `>`, `script` 시작 태그의 따옴표 속성, `-`가 붙은 사용자 정의 태그, http/https 이외 scheme과 fragment만 있는 redirect, 확인 뒤 뒤 고정 문구 다음에 생긴 범위와 같은 자리의 두 번째 값)을 고친 뒤 `cargo clean`부터 다시 빌드한 제품 binary와 시험 profile binary로 로컬 페이지 서버, 시험용 시계, 실제 `serve` worker를 실행해 44개 확인이 모두 통과했다. 시험 binary의 모든 명령과 `serve`에는 인증 정보가 든 프록시 환경 변수를 주었고, 먼저 `curl -x` 연결을 기록하는 것을 보인 기록용 프록시에 연결이 한 번도 없었다. 따옴표 속성값과 `script`에 숨은 `₩1`·`₩2`가 아닌 후보 하나, 확인 순간 값 변화 거절, 사설 redirect 미리보기 거절, `-5`를 5로도 뒤의 다른 숫자로도 내지 않는 미리보기, `₩10,000~12,000`을 한 값으로 내지 않는 미리보기, `+5`를 내지 않는 미리보기, EUC-KR XHTML 미리보기, `session export` JSON·JSONL의 가린 계획 주소와 digest, A→무관 변화→B, 404·`Retry-After: 3600`의 429·사설 redirect·`ftp://` redirect(같은 사이트의 `/ftp://…` 경로를 요청하지 않음), 회복, worker 프로세스를 종료하고 세 주기 뒤 새 프로세스로 다시 시작했을 때의 공백 한 번, pause 뒤 관측 없음, 다른 세션에서 아직 없는 `재입고` 영역을 `present=false`로 확인한 뒤 실제 worker가 첫 출현을 `notify_candidate`로 판정, 시험 변수를 준 제품 binary와 `tools.ssrfWhitelist`에 사설 대역을 넣은 config의 거절, query 값이 owner session 파일에만 있음, 페이지 요청 21번(재시도 없음, cookie·인증 header 없음)을 확인했다.
- 남은 제한: 주소 확인과 연결 사이의 DNS 변경, HTML `<meta charset>`, 여러 값·여러 페이지, JavaScript·로그인 페이지는 지원하지 않는다. lineage 변경이나 비활성으로 닫힌 실패 구간의 닫힌 시각은 기록하지 않는다.

## 종료 확인 (2026-10-26)

- 범위: 웹페이지 한 곳의 값 관찰(미리보기·확인·주기 확인·판정·실패 구간·공백·상태 조회), 시험용 profile, 공용 redirect 해석 helper 수정. 알림([010](010-change-failure-recovery-and-gap-notification.md)), 결정 마감([020](020-decision-deadline-reminder.md)), 대화형 확인([030](030-conversational-entrustment-summary-and-confirmation.md))은 포함하지 않는다.
- 최종 검증(코드 커밋 `f25f1bc`, 기록은 `.omo/evidence/spec042/prd000/final-tests/`):
  - 사용자 승인을 받은 전체 workspace 실행(`cargo test --workspace --no-fail-fast`, 1,275초): 3,243 passed / 1 failed / 14 ignored. 유일한 실패는 043 종료 때 기록한 baseline 실패다(`runtime::generated_media_release::runner::fixture::tests::mismatch_uses_isolated_tree`). git이 추적하지 않는 Spec034 로컬 evidence 파일 `.omo/evidence/spec034/task-12-integration.json`이 없어서 실패하며, 이번 변경과 관계없다.
  - `cargo fmt --all -- --check`, workspace clippy `--all-targets -D warnings`, `shacs-cli --features spec042-test-profile` clippy가 통과했다.
- Oracle 독립 검토: **PASS** (10차, 차단·비차단 지적 없음). 1~9차의 차단 지적은 모두 고치고 회귀 테스트를 더했다. 고친 항목은 worker 순서, 요청마다의 주소 검사, redirect 전체 시간 한도, 부호·범위 숫자, 아직 없는 문구, charset, 환경 변수 프록시, `session export` URL, 태그 해석, 공용 redirect helper, 확인 뒤 생긴 범위와 두 번째 값이다. 회차별 기록은 `.omo/evidence/spec042/prd000/review/gates.md`에 있다.
- 실제 표면: `.omo/evidence/spec042/prd000/qa-run/qa/transcript.md` 44/44 PASS. QA binary는 코드 커밋 `f25f1bc`와 같은 소스로 `cargo clean`부터 빌드했다.
- 남은 제한: 위 "검증"의 남은 제한과 같다.
