# PRD 010. 변화·확인 실패·회복·공백 알림

Status: Complete (Scoped)

Depends on: [PRD 000](000-web-page-observation-and-condition-decision.md)과 [036](../../036-entrusted-work-continuity-and-change-notification/SPEC.md) 완료 사실. 뒤 번호 PRD에는 의존하지 않는다.

## Goal

웹 관측의 결과를 사용자에게 알리되, 알림이 없다면 **기준 시간을 넘는 확인 공백 없이 확인했고 의미 있는 변화도 없었다**는 뜻이 되게 한다. 조건 결과가 바뀐 변화, 기준 시간을 넘은 확인 실패와 그 회복, 어시스턴트가 꺼져 있던 공백을 각각 한 번씩 알린다.

## Scope

1. **042 알림 설정:** 위임마다 단일 `email:<address>` 알림 대상과 실패 알림 기준 시간을 042 알림 설정 revision으로 저장하고 CLI/API로 설정·조회한다. 이 revision은 관측 revision과 따로이며 바꿔도 비교 기준이 다시 잡히지 않는다. 036의 `linked_targets`를 발송 대상으로 재해석하지 않고 036 작업의 owner-Email 규칙을 바꾸지 않는다. Email 설정이 없거나 지원하지 않는 대상은 거절한다.
2. **변화 알림:** PRD 000의 `notify_candidate`만 036의 durable route와 Email 경로로 보낸다. 이전/새 값, 조건과 충족 여부, query·fragment를 뺀 URL, 두 관측의 읽은 시각, 근거 digest, 확인 한계, 추적 ID를 결정적으로 렌더링한다.
3. **확인 실패 알림:** PRD 000의 실패 구간이 `runtime_gap`을 뺀 길이로 기준 시간 이상 이어지면 구간마다 한 번, 실패 분류·구간 시작 시각·마지막 성공 시각·다음 확인 시각을 담아 알린다. 기준 시간의 기본값은 확인 주기의 2배와 6시간 중 긴 시간이며, 확인 주기의 2배 이상 7일 이하에서 바꿀 수 있다. 기준 시간 전에 닫힌 구간은 알림도 회복 알림도 만들지 않는다. 알림 설정 revision이 바뀌어도 열린 실패 구간은 이어지며 새 기준 시간으로 다시 판단하고, 이미 보낸 실패 알림은 다시 보내지 않는다.
4. **회복 알림:** 실패 알림을 보낸 구간이 성공으로 닫히면 한 번 알린다. 그 성공이 `notify_candidate`이면 회복 사실을 담은 변화 알림 한 건만 보낸다.
5. **공백 알림:** PRD 000이 기록한 활성 위임의 `runtime_gap`이 기준 시간 이상이면 공백 기간을 담아 한 번 알린다. 공백은 실패 구간의 길이에 넣지 않으므로 같은 공백 때문에 실패 알림이 따라오지 않는다. pause/done/clear 상태의 위임에는 공백 알림이 없다.
6. **중복과 오래된 알림:** 알림 종류와 사건별로 중복 enqueue를 막는다. enqueue 직전과 소비 직전에 036과 같은 owner·goal·동의·관측 revision과 042 알림 설정 revision을 다시 확인해 pause/done/clear나 설정 변경 뒤의 오래된 알림을 억제한다. 확인 성공·판정·enqueue·전달 사실을 분리해 조회한다.

## Non Scope

- 새 channel transport와 Email 이외 대상으로의 자동 발송, 결정 마감 알림([020](020-decision-deadline-reminder.md)), 대화형 요약([030](030-conversational-entrustment-summary-and-confirmation.md)).
- 모델이 만든 알림 문구나 중요도 재평가, 036 정기 요약 opt-in의 의미 변경.
- 원격 수신 ACK, exactly-once 전달, 이미 보낸 메시지의 회수.

## Parent Requirement Mapping

- [042 SPEC](../SPEC.md)의 소유 범위 5·6과 수용 조건 4·5를 소유한다.

## Required Contract

1. 변화 알림은 저장된 판정만 소비하며 중요도를 다시 판단하지 않는다. 확인 실패 중에는 변화 알림을 만들지 않는다.
2. 실패·회복·공백 알림은 PRD 000의 상태 사실(마지막 성공 시각, 실패 구간, `runtime_gap`, 다음 확인)만으로 결정적으로 만든다.
3. 같은 사건은 재시작·중복 wake·retry에도 한 번만 enqueue한다. 소비를 claim한 뒤 crash하면 재발송 대신 `unknown`을 보존한다.
4. 알림 payload는 제한된 값·근거만 담고 응답 본문, URL query·fragment, credential을 담지 않는다.

## Acceptance Criteria

1. 실제 runtime, `127.0.0.1/32`만 허용한 시험용 profile의 로컬 HTTP 서버, 로컬 SMTP 수신기, 시험용 시계로 A → A → 무관 변화 → B를 실행해 B의 변화 알림만 한 번 받는다.
2. 기준 시간을 넘긴 실패 구간에서 실패 알림 한 건, 회복 뒤 회복 알림 한 건을 받는다. 실패 중의 값 변화는 알림을 만들지 않고, 기준 시간 전에 회복한 구간은 아무 알림도 만들지 않는다. 회복 확인이 조건 결과 변화이면 회복 사실이 담긴 변화 알림 한 건만 받는다.
3. runtime을 기준 시간 넘게 멈췄다가 다시 시작하면 공백 알림 한 건을 받고 바로 확인이 이루어진다. 공백 뒤 확인이 실패해도 실패 알림이 바로 따라오지 않는다. 기준 시간 안의 재시작과 pause 상태의 위임은 공백 알림을 만들지 않는다.
4. 같은 사건의 중복 wake·재시작, enqueue 뒤 pause·설정 변경 경합에서 중복되거나 오래된 알림이 나가지 않는다. SMTP 실패는 확인 성공과 별개로 기록된다.

## Closure Evidence

1. 실제 runtime·로컬 HTTP 서버·로컬 SMTP·시험용 시계의 transcript와 수신 메시지, 알림 사실 조회 결과.
2. 기준 시간 경계, 회복과 변화 알림의 합침, 공백과 실패 구간의 구분, 중복 억제, 오래된 알림 억제 테스트.
3. 사용자 문서에 알림 종류·기준 시간·비보장 범위를 기록.

PRD 010은 PRD 000의 닫힌 관측·상태 사실만 소비한다. 알림 조건을 모두 충족하면 마감 알림이나 대화형 확인 없이 종료한다.

## 구현 메모 (2026-10-26)

- **설정:** 위임마다 하나인 `email:<address>` 대상과 실패 기준 시간을 owner session의 `web_watch` metadata(`notify`)에 위임 ID별로 저장한다. 바꿀 때마다 알림 설정 revision이 오르고 관측 revision·계획·첫 기준은 그대로이며, 계획을 다시 확인해도 설정은 남는다. 확인된 웹페이지 계획이 있어야 하고, 대상은 CLI에서는 `--config`(명령 앞의 전역 값, 명령 뒤에 쓰면 그 값)의 Email 채널이, API에서는 adapter가 판단한 Email 채널이 runtime이 Email 전송을 시작하는 규칙과 같이 켜져 있고(`enabled`) `consentGranted`와 `smtp`로 보낼 수 있을 때만 받는다. 기준 시간은 기본 `max(주기×2, 6시간)`, 범위 `주기×2`~7일이다. 주기가 바뀌어 저장한 값이 범위 아래가 되면 `주기×2`를 쓴다. `--off`는 대상을 지운 새 revision이다.
- **사건:** worker는 확인을 기록할 때 대상이 있는 현재 알림 설정 revision을 함께 남긴다. 알림은 현재 revision으로 기록된 확인에서만 나온다. 단, 열린 실패 구간은 마지막 실패 확인이 어떤 revision으로(또는 알림이 꺼진 채로) 기록됐든 그 확인까지의 구간 상태를 현재 설정으로 다시 판단하므로, 기준 시간을 낮추거나 알림을 켜면 다음 확인을 기다리지 않고 실패 알림이 나간다. 각 확인이 반영되는 순간의 실패 구간 상태(구간 안 `runtime_gap`은 `excluded_gap_ms`로 따로 셈)를 잡아 두므로 기록 보관 개수와 관계없이 결정적이다. 변화는 `notify_candidate`, 실패는 공백을 뺀 구간 길이가 기준 이상인 구간이다(구간당 한 번). 구간 길이는 첫 실패부터 열린 구간은 마지막 실패 확인까지, 성공으로 닫힌 구간은 그 성공 확인까지이며, 그 성공 확인에 딸린 `runtime_gap`도 뺀다. 열린 구간 중 hook이 막은(`blocked`) 확인에 딸린 공백은 마지막 실패 뒤의 일이므로 따로 모았다가, 다음 실패가 구간을 늘리거나 성공이 닫을 때 뺀다. 그래서 회복을 확인한 순간 기준에 이른 구간도 그 성공에서 실패 알림 뒤 회복 알림을 보낸다. 실패 알림의 마지막 성공 시각은 현재 계획의 확인 기준과 같은 계획의 성공 확인 중 최신 값이다. 회복은 실패 알림이 나간 구간(그 알림이 현재 설정으로 queue에 들어갔거나 보내기가 시작됐고 억제되지 않음)을 닫은 성공이다. 구간을 닫은 성공 확인이 기록될 때의 설정으로 그 구간이 기준을 넘었는데 queue에 든 실패 알림이 없으면(설정을 바꾼 직후 그 판단보다 먼저 확인이 성공한 경우, 대상 변경으로 실패 알림이 억제된 경우) 실패 알림을 먼저 만들고 이어서 회복을 알린다. 구간이 닫힐 때 알림이 꺼져 있었다면 실패도 회복도 알리지 않는다. 실패 알림의 queue 작업이 보내지 못하고 끝났으면(전달 시도 소진·취소, 전달 `failed`) 회복을 만들지 않고, 이미 queue에 든 회복은 보내기 직전에 그 실패 알림의 보내기가 시작됐는지 확인해 아니면 억제한다. 회복을 담은 변화 알림은 같은 확인에서 실패 알림이 나가지 않았으면 변화만 보내고 회복 문구를 뺀다(그래서 회복 정보에 실패 구간 ID를 둔다). 실패 알림의 보내기가 시작됐으면 그 뒤 SMTP가 실패해도 실패 알림이 나간 것으로 본다. 마지막 실패가 429·503 대기나 `Retry-After`로 다음 확인을 주기보다 늦게 잡았다면 그 확인 전에는 회복할 수 없으므로, 구간 길이를 그 다음 확인 시각까지로 보고 그 실패에서 바로 알린다(다음 확인 시각과 지금 보내는 이유를 담는다). 보통 주기의 실패는 관측한 길이만 쓴다(그 성공이 `notify_candidate`면 변화 알림에 회복 사실을 넣고 회복 알림은 만들지 않는다). 공백은 기준 이상인 `runtime_gap`이다. 같은 종류·사건 ID가 어떤 revision으로든 이미 enqueue되고 억제되지 않았으면 다시 만들지 않는다. 그래서 기준 시간을 바꿔도 보낸 실패 알림은 다시 나가지 않고, 회복은 그 실패 알림을 따른다. 대상이 바뀌어 억제된 실패 알림은 구간이 이어지는 동안이나 닫힐 때 새 설정으로 다시 만들어진다. 기록됐지만 queue에 들어가지 못한 알림은 사건 보관 한도와 관계없이 저장된 의도로 다시 시도한다.
- **경로와 사실:** durable event `web_watch.notification_fact`가 알림 의도(대상·설정 revision·사건·관측 wake·가린 URL·제한된 내용)와 단계(`enqueue_pending`, `enqueue_failed`, `enqueued`, `suppressed_stale`, `consuming`, `outbound_pending`, `delivery_failed`, `sent_hint`)를 남긴다. worker는 036 관측·알림과 웹페이지 확인 뒤, owner session lock 아래에서 enqueue한다. enqueue가 실패하면 시도 시각을 담은 `enqueue_failed`를 남기고 worker와 다른 알림은 계속 진행한다. 실패한 알림은 저장된 의도로 1분 뒤부터 처음 기록된 순서대로 다시 시도하며 모두 5번까지 시도한다(`enqueue_failures`). inbound는 대상 session(`email:<address>`)으로 들어가고 work ID와 dedupe hint가 `web-watch-notification:` correlation이다. 029 durable dispatcher가 due work를 work ID 순서가 아니라 enqueue 순서로 넘기고, 앞선 inbound가 재시도를 기다리는 동안 같은 session의 뒤 inbound도 기다리도록 바꿨으므로(session마다 한 번에 하나, `/stop`·`/restart`·`/status` priority 명령은 예외), 같은 대상의 알림은 queue에 들어간 순서대로 나간다(한 단계에서 실패 알림 뒤 회복 알림을 만들면 그 순서). 알림의 queue 작업이 보내기 전에 끝나면(소진·취소) 전달은 `failed`다. durable dispatcher가 이를 owner-accepted notification으로 표시하면, agent loop가 provider 없이 owner session을 다시 확인하고 `consuming`을 남긴 뒤 메일을 보낸다. 전달 사실은 036과 같은 correlation metadata로 기존 SMTP 경로가 기록한다(접두사로 042 사실에 들어간다). 036 `linked_targets`와 owner-Email 규칙은 바꾸지 않는다. 메일은 Email transport를 실행하는 channel runtime(`run`, `runtime start`)이 보낸다. `serve`의 worker도 확인·enqueue는 하지만 Email transport를 띄우지 않으므로 그동안 알림은 `pending`이고, 같은 data/workspace의 `runtime start`가 소비 직전 확인을 거쳐 보낸다(036과 같음).
- **오래된 알림:** enqueue 직전과 소비 직전에 알림의 관측 wake가 현재 lineage·계획(goal·동의·revision·전이·계획 digest)과 같은지, 알림 설정 revision과 대상이 그대로인지 확인한다. 다르면 `suppressed_stale`로 남기고 보내지 않는다. 이전 revision으로 queue에 남은 알림도 같은 규칙으로 억제된다. queue에 들어갔지만 보내기 전인 알림은 worker가 새 알림을 만들기 전에 owner session lock 아래에서 같은 확인으로 먼저 억제하므로, 설정을 바꾼 뒤의 실패·회복 판단은 그 알림을 보낸 것으로 보지 않는다(새 설정으로 실패 알림 뒤 회복을 다시 만든다). `consuming` 뒤에 멈추면 다시 보내지 않고 전달은 `unknown`으로 남는다.
- **렌더링:** 036과 같은 형식의 결정적인 영어 문장이다. 종류별 제목, query·fragment를 뺀 URL과 SHA-256, 값·조건(문구 조건은 확인한 문구)·읽은 시각(UTC와 ms)·텍스트 digest·HTTP 상태, 실패 분류·시작·마지막 성공·다음 확인, 공백 기간, 한계 문구, 추적 ID를 담는다. 응답 본문·URL query·credential은 담지 않는다.

### 검증

- `shacs-core --test it spec042_web_watch::notify::` 22개: A·A·무관 변화·B에서 변화 메일 한 통, 기준 전 회복은 메일 없음, 기준을 넘은 실패 한 통과 회복 한 통, 기준 20분에서 10분 뒤 회복은 메일 없음·0분과 10분 실패 뒤 20분 회복은 실패 메일 뒤 회복 메일·실패 뒤 30분 공백을 지나 회복하면 공백 메일만, 기준 30분에서 실패 뒤 30분 공백·재시작 첫 확인은 hook이 막음·다음 확인 성공이면 공백 메일만이고 닫힌 구간이 공백 30분을 뺌, 실패 메일의 queue 작업이 보내지 못하고 끝나면 회복 전이든 회복이 그 뒤에 queue에 든 뒤든 메일 없음·실패 메일 전달 `failed`·회복은 만들지 않거나 억제, 같은 경우 회복이 조건 변화와 겹치면 변화 메일만 회복 문구 없이 감, 보내기 전인 실패 메일이 queue에 있는 채 대상을 바꾸고 조건 변화로 회복하면 새 대상으로 실패 메일 뒤 회복 사실을 담은 변화 메일, 회복 확인의 실패·회복 메일이 둘 다 queue에 못 들어가면 1분 뒤 실패 메일부터 다시 넣어 그 순서로 감, 회복이 조건 결과를 바꾸면 회복 사실을 담은 변화 메일 한 통, 알림이 꺼진 채 닫힌 구간은 알림을 켜도 메일 없음, 대상 변경으로 실패 메일이 억제된 구간이 조건 변화로 닫히면 새 대상으로 실패 메일 뒤 회복 사실을 담은 변화 메일, 기준을 1시간에서 20분으로 낮추거나 알림을 새로 켠 직후 다음 확인이 먼저 성공해도 실패 메일 뒤 회복 메일이 그 순서로 한 통씩이고 다시 실행해도 추가 없음, 대상 변경으로 억제된 실패 메일은 실패가 이어지면 새 대상으로 한 통, 실패 메일의 마지막 성공 시각은 다시 확인한 현재 계획의 확인 기준, 기준보다 긴 `Retry-After`는 첫 429에서 실패 메일, 실패 중 기준 시간 변경 뒤 구간 유지·실패 메일 재발송 없음·회복 한 통, 30분째 실패 중 기준을 1시간에서 20분으로 낮추면 새 확인 없이 실패 메일 한 통·다시 실행(새 dispatcher)해도 추가 없음·회복 한 통, 주기 1배 공백은 메일 없음과 3배 공백 한 통·뒤이은 실패 메일 없음, enqueue 뒤 pause면 소비 억제·중복 enqueue 없음·재소비 없음, 대상 변경 뒤 이전 알림 억제와 소비 claim 뒤 재발송 없음, 설정 검증(계획 없음·Email 미준비·대상 형식·기준 범위)과 관측 revision·기준 유지, SMTP 실패가 확인 성공과 따로 기록됨, 같은 worker dispatcher에서 work queue가 첫 enqueue를 실패시켜도 오류 없이 두 알림(공백·변화)을 `failed`로 남기고 1분 전에는 다시 시도하지 않으며 1분 뒤 두 메일이 가고 다음 확인과 그 변화 메일이 이어짐, 계속 실패하면 5번에서 멈추고 queue가 돌아와도 보내지 않음. agent loop는 provider를 부르면 실패하는 시험 provider로 실행한다.
- `shacs-api --test it spec042_web_watch_api::` 1개: `notify` route, Email 미준비 거절, `off`. `shacs-cli --test it spec042_web_watch_cli::` 3개 중 `web_notify_needs_an_enabled_email_channel_that_can_send`: 동의·SMTP가 있어도 `enabled: false`인 config는 명령 앞(전역)이든 뒤든 거절, 켜진 config는 명령 앞에 써도 revision 1, 앞에 꺼진 config와 뒤에 켜진 config를 함께 쓰면 뒤의 값으로 revision 2. 036 회귀: `shacs-core --test spec036_notification` 13개, `--test spec036_observation` 33개 통과. 029 dispatcher: `shacs-core --test durable_dispatch` 22개(work ID 순서가 반대인 두 inbound를 한 session에 넣으면 먼저 넣은 것부터 넘김, bus가 가득 차 첫 inbound가 재시도를 기다리면 그 시각 전에는 뒤 inbound도 넘기지 않음 포함), `shacs-session --test durable_work` 10개 통과.
- 실제 표면: `.omo/evidence/spec042/prd010/qa-run/qa/transcript.md`. `cargo clean`부터 빌드한 시험 profile binary로 실제 channel `runtime start`(Email 채널 SMTP를 로컬 수신기로), 로컬 페이지 서버, 시험용 시계를 실행해 21개 확인이 모두 통과했다. 확인한 내용은 다음과 같다. Email을 보낼 수 없는 config와 `email:`이 아닌 대상은 거절된다. 설정 revision은 관측 revision·기준을 바꾸지 않는다. A·A·무관 변화·B에서 메일은 변화 한 통이며 이전/새 값·조건·query를 뺀 URL·읽은 시각·추적 ID를 담는다. 기준 전 회복은 없고, 기준을 넘은 실패 한 통·다음 실패 없음·회복 한 통이 간다. 회복+변화는 회복 사실을 담은 변화 한 통이다. runtime을 세 주기 멈췄다 켜면 밀린 확인이 바로 실행되고 공백 메일 한 통이 가며, 공백 뒤 실패와 그 10분 뒤 회복은 메일이 없다. 한 주기 안의 재시작과 pause된 위임은 공백 메일이 없다. SMTP가 받는 사람을 거절하면 전달 실패가 성공한 확인과 따로 기록된다. 같은 data/workspace로 `serve`만 실행하면 확인과 enqueue는 되지만 메일은 나가지 않고 `pending`이며, `serve`를 멈추고 `runtime start`를 실행하면 그 변화 메일이 한 통 간다. 모든 메일은 `user@example.test`로 가고 URL query를 담지 않는다. 알림 사실은 변화·실패·회복·실패·변화·공백·변화·변화 순서다.
- 남은 제한: 알림은 Email 대상 하나로만 가고 메일은 channel runtime만 보낸다(`serve`만 실행 중이면 `pending`). 원격 수신 확인과 exactly-once는 없고, 보내기 시작 뒤 중단되면 `unknown`이다. queue에 다섯 번 넣지 못한 알림은 `failed`로 남는다. 036 알림의 enqueue 오류는 036 동작대로 worker를 멈춘다. QA 중 `runtime stop`이 실행 중 runtime의 heartbeat와 겹쳐 "runtime ownership is being mutated by another process"로 한 번 거절된 적이 있다. 이것은 기존 실행 관리 동작이라 이번 범위에서 바꾸지 않았고, 다시 요청하면 멈춘다.

## 종료 확인 (2026-10-26)

- 범위: 042 알림 설정(Email 대상·실패 기준 시간·설정 revision, CLI/API), 변화·확인 실패·회복·공백 알림의 파생·enqueue·소비·전달 사실, 029 durable inbound를 session마다 enqueue 순서로 넘기는 dispatcher 변경. 결정 마감([020](020-decision-deadline-reminder.md))과 대화형 확인([030](030-conversational-entrustment-summary-and-confirmation.md))은 포함하지 않는다.
- 최종 검증(코드 커밋 `e7a7ea0`, 기록은 `.omo/evidence/spec042/prd010/final-tests/`):
  - 사용자 승인을 받은 전체 workspace 실행(`cargo test --workspace --no-fail-fast`, 1,314초): 3,268 passed / 1 failed / 14 ignored. 유일한 실패는 043 종료 때 기록한 baseline 실패다(`runtime::generated_media_release::runner::fixture::tests::mismatch_uses_isolated_tree`). git이 추적하지 않는 Spec034 로컬 evidence 파일 `.omo/evidence/spec034/task-12-integration.json`이 없어서 실패하며, 이번 변경과 관계없다.
  - `cargo fmt --all -- --check`, workspace clippy `--all-targets -D warnings`, `shacs-cli --features spec042-test-profile` clippy가 통과했다.
- Oracle 독립 검토: **PASS** (11차, 차단 지적 없음). 1~10차의 차단 지적은 모두 고치고 회귀 테스트를 더했다. 고친 항목은 다음과 같다: 회복 알림의 선행 조건과 마지막 성공 시각, Email 채널 `enabled` 확인, `serve` 설명, enqueue 실패 격리·재시도, 전역 `--config`, 설정 변경 뒤 열린 구간 재판단, 회복 확인 시점의 판단과 durable FIFO, 재시도 중인 선두 inbound, 보내지 못한 실패 알림 뒤의 회복, 회복 확인까지의 구간 길이, hook이 막은 확인의 공백, 변화 메일의 회복 문구, queue에 남은 오래된 알림과 재시도 순서. 회차별 기록은 `.omo/evidence/spec042/prd010/review/gates.md`에 있다.
- 실제 표면: `.omo/evidence/spec042/prd010/qa-run/qa/transcript.md` 21/21 PASS. QA binary는 코드 커밋 `e7a7ea0`의 crates와 같은 소스(QA 전후 digest `57997604…`)로 `cargo clean`부터 빌드했다.
- 남은 제한: 위 "검증"의 남은 제한과 같다.
