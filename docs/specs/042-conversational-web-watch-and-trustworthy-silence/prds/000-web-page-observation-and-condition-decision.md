# PRD 000. 웹페이지 관찰과 조건 판정

Status: Planned

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
