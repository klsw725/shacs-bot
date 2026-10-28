# shacs-bot 사용 가이드

## 명시적으로 맡긴 작업

먼저 분리된 workspace에서 `onboard`와 `session create --session cli:direct --workspace /tmp/shacs-ws`를 실행하세요. `/work create <목표>`는 대화 중 사용자가 직접 지속 위임할 때 사용합니다. 일반 질문이나 모호한 요청은 자동으로 위임으로 저장하지 않습니다. 어시스턴트가 `propose_entrusted_work` 도구로 제안한 경우에도 같은 대화·발신자에게 표시된 `/work accept <proposal-id>`를 명시적으로 보내야 생성됩니다. `/work decline <proposal-id>` 또는 일반적인 `yes`는 위임 동의가 아닙니다. 대화에서는 `/work read <work-id>`, `/work edit <work-id> <expected-revision> <instructions>`, `/work pause|resume|done|clear <work-id>`를 사용할 수 있습니다. 대화의 `edit`는 이전 예산·결정·남은 질문을 보존하고 지시 내용만 교체합니다.

CLI는 사용자 소유의 기존 session을 대상으로 구조화된 조건과 동의 근거를 직접 기록합니다. 반환되는 JSON의 `work.work_id`를 후속 명령의 `<work-id>`로 사용하세요:

```sh
shacs-bot session create --session cli:direct --workspace /tmp/shacs-ws
shacs-bot work create "track release readiness" --workspace /tmp/shacs-ws --session cli:direct --consent explicit-request --evidence-ref cli:explicit-request --instructions "report verified changes" --budget "daily" --prior-decision "use official sources" --remaining-question "which release?" --linked-target channel:alerts
shacs-bot work read "<work-id>" --workspace /tmp/shacs-ws --session cli:direct
shacs-bot work revise "<work-id>" --workspace /tmp/shacs-ws --session cli:direct --expected-revision 1 --instructions "report verified release changes" --budget "daily" --prior-decision "use official sources" --remaining-question "which release?"
shacs-bot work revise "<work-id>" --workspace /tmp/shacs-ws --session cli:direct --expected-revision 2 --instructions "report verified release changes" --budget "daily" --prior-decision "use official sources" --remaining-question "which release?" --linked-target channel:new-alerts
shacs-bot work pause "<work-id>" --workspace /tmp/shacs-ws --session cli:direct
shacs-bot work resume "<work-id>" --workspace /tmp/shacs-ws --session cli:direct
shacs-bot work done "<work-id>" --workspace /tmp/shacs-ws --session cli:direct
```

CLI `revise`와 API `revise`는 조건 전체를 교체하므로 유지할 예산·결정·질문도 다시 보내야 합니다. 연결 대상은 생략하면 유지하며, CLI의 `--linked-target`(반복 가능) 또는 API `linked_targets` 배열을 지정하면 교체합니다. 전체 연결을 해제하려면 CLI `--clear-linked-targets` 또는 API 빈 배열을 사용하세요. 변경 전후 연결 대상은 revision 이력에도 기록됩니다. 이전 revision으로 수정하면 최신 조건·연결을 덮지 않고 거절합니다. 명시적으로 연결된 대상은 CLI에서 `--session channel:alerts --owner-session cli:direct`로 조회·수정할 수 있으며, 미연결 대상은 거절됩니다. 저장된 과거 goal은 자동으로 위임으로 승격하지 않습니다. `goal` 명령으로 별도 변경한 결과와 위임의 마지막 참조 전이가 어긋나면 안전을 위해 work 읽기·수정을 거절합니다.

`--allow-api-side-effects`를 지정해 loopback에 띄운 local API에서는 `POST /v1/sessions/cli%3Adirect/entrusted-work`로 생성하고 `GET /v1/sessions/cli%3Adirect/entrusted-work/<work-id>`로 읽습니다. 생성 본문은 `goal_text`, `consent` (`kind`, `evidence_ref`, `recorded_at`), `conditions` (`instructions`, 선택적인 `budget`, `prior_decisions`, `remaining_questions`), `observed_at`을 포함합니다. `POST .../<work-id>/revise`는 `expected_revision`, 전체 `conditions`, `changed_at`, 선택적인 `linked_targets`를 받고, `POST .../<work-id>/pause|resume|done|clear`는 `observed_at`을 받습니다. 연결 대상은 `/v1/sessions/<requester>/entrusted-work/<owner>/<work-id>` 경로를 사용합니다. 이 동의는 도구 권한·예약·구매·사용자를 대신한 외부 확정 행동의 승인이 아닙니다. 로컬 파일 관측과 명시적 정수 조건 비교, 아래에 명시한 Email 변경 알림을 지원합니다.

### 알림 없는 로컬 파일 예약 관측 (PRD 010)

활성 위임에 사용자 소유 workspace의 상대 파일 경로와 확인 주기(5~86,400초)를 설정할 수 있습니다. `observe`는 조건 revision을 하나 증가시키는 CAS 설정 명령이며 직접 읽기나 메시지 전송을 하지 않습니다. Unix의 일반 파일만 지원하고 source 상대 경로 component의 symlink, workspace 밖 경로, 실행 사용자가 소유하지 않은 workspace/파일, 64 KiB 초과 자료는 읽지 않습니다. 비교 설정이 없으면 SHA-256 근거 ref와 byte 수만 저장하며 원문을 저장하거나 provider에 보내지 않습니다. 이 digest는 의미 변화 판정이 아닙니다.

아래의 `DATA`는 **실제로 사용할 config가 있는 directory**, `WS`는 별도 workspace입니다. 기존 사용자 config/auth를 복사하지 말고 새 경로를 사용하세요. `onboard` 후 해당 config에서 기존 provider 설정 절차를 마쳐야 `serve`를 시작할 수 있지만, 관측 자체는 provider를 호출하지 않습니다. 예시의 `WORK`는 `work create` 출력의 실제 ID로 바꾸세요.

```sh
DATA=/tmp/shacs-observation-data
WS=/tmp/shacs-observation-workspace
shacs-bot --config "$DATA/config.json" onboard --workspace "$WS"
shacs-bot --config "$DATA/config.json" session create --workspace "$WS" --session cli:direct
printf 'controlled evidence A\n' > "$WS/source.txt"
shacs-bot work create "watch my local source" --workspace "$WS" --session cli:direct --consent explicit-request --evidence-ref cli:user --instructions "observe only"
WORK='<work-id-from-create>'
shacs-bot work observe "$WORK" --workspace "$WS" --session cli:direct --expected-revision 1 --source source.txt --cadence-seconds 15
shacs-bot work observations "$WORK" --workspace "$WS" --session cli:direct --data-dir "$DATA"
shacs-bot --config "$DATA/config.json" serve --workspace "$WS" --port 8900 --allow-api-side-effects
```

`serve` 또는 channel `runtime start`의 기존 production worker가 029 queue에 `entrusted.observation` work를 등록하고 due 시 실행합니다. 별도 cron job이나 새 scheduler를 만들지 않습니다. worker를 실행하지 않으면 `awaiting_enqueue`, 등록되면 `queued`와 실제 work ID·due 시각이 보입니다. 최초 관측 전 `current_observation: null`은 아직 모른다는 뜻이지 성공/무변화가 아닙니다. `leased`는 queue lease 사실이지 관측 성공이 아닙니다.

다른 터미널에서 동일한 `DATA`, `WS`, `WORK`를 사용하여 조회합니다. API의 session은 URL 인코딩하며 아래 POST는 기존 loopback mutation opt-in이 필요합니다. 조회는 GET입니다:

```sh
shacs-bot work observations "$WORK" --workspace "$WS" --session cli:direct --data-dir "$DATA"
curl --fail-with-body "http://127.0.0.1:8900/v1/sessions/cli%3Adirect/entrusted-work/$WORK/observations"
curl --fail-with-body -X POST "http://127.0.0.1:8900/v1/sessions/cli%3Adirect/entrusted-work/$WORK/observations" \
  -H 'content-type: application/json' \
  -d '{"expected_revision":2,"source":{"kind":"local_file","relative_path":"source.txt"},"cadence_seconds":30}'
shacs-bot work pause "$WORK" --workspace "$WS" --session cli:direct
```

성공한 `observed` 기록과 다음 확인 **의도**는 같은 durable terminal event에 저장됩니다. 다음 enqueue는 별도 029 event이므로 crash 사이에는 `awaiting_enqueue`로 복원될 수 있습니다. 이력은 최근 32개를 조회하며 현재 revision의 마지막 기록을 따로 표시합니다. 원래 029 event log가 근거이고 별도 관측 DB/queue는 없습니다. 프로세스를 Ctrl-C로 정상 종료한 뒤 같은 `serve` 명령으로 재시작하면 pending wake와 관측이 복원됩니다. 비정상 종료의 stale lease는 성공으로 처리하지 않으며 기존 `runtime inspect` / `runtime recover` 후 다시 시작하는 절차를 따릅니다.

enqueue 전, due 읽기 직전, commit 직전에 033 goal·동의·조건 revision·goal transition을 확인합니다. pause/done/clear, stale goal binding 또는 철회된 동의의 wake는 029 취소 사실로 끝나며 새 관측을 남기지 않습니다. 조건 수정과 resume 후에는 새 lineage만 사용하고 종료한 일의 이력은 계속 읽을 수 있습니다. 읽기 실패/누락/미지원은 `unavailable`, 현재 snapshot/hook/permission gate 거절은 `blocked`로 기록하고 다음 확인을 `stopped`로 표시합니다. 문제를 고친 뒤 현재 revision으로 `observe`를 다시 실행해야 재예약합니다. 현재 permission과 headless hook confirmation을 그대로 적용하며 위임만으로 승인이나 grant를 만들지 않습니다. 로컬 읽기에는 process 실행·외부 credential·provider disclosure가 없고, native-host containment를 보장하지 않습니다. 일반 cron/heartbeat와 goal continuation은 이 설정으로 위임 작업으로 바뀌지 않습니다. 비교 설정 없는 digest 관측은 알림·요약을 만들지 않습니다. 외부 확정 행동은 실행하지 않습니다.

### 사용자 조건에 따른 변화 판정 (PRD 020)

사용자가 **정수값이 상한 이하인지를 선택 기준으로 명시**하면 같은 조건 revision의 관측을 비교합니다. 일반 텍스트 diff, Cron 출처, 성공 exit code, 모델의 중요도 주장은 Notify 근거가 아닙니다. 자유 형식 `instructions`나 `budget`을 수치로 추론하지 않습니다. 가격·통화 변환·여행 추천·범용 JSON 비교는 지원하지 않으며, 값과 상한은 사용자가 같은 단위로 준비해야 합니다.

비교 source는 다음 세 필드만 있는 UTF-8 JSON입니다. `value`는 signed 64-bit 정수이며 소수·문자열·추가 필드·다른 schema는 비교할 수 없습니다. `as_of_ms`는 자료를 만든 사용자가 명시한 Unix millisecond 시각입니다. 파일 읽기와 digest는 확인하지만 이 시각이나 외부 세계의 사실을 별도로 검증하지 않습니다. 원문 대신 이 고정 크기 수치 요약, digest, byte 수, snapshot ref만 판정 근거로 보존합니다. 파일에는 credential을 넣지 마세요.

```json
{"schema":"integer_v1","value":120,"as_of_ms":1792400000000}
```

`as_of_ms`를 실제 자료 시각으로 바꾸고, 기존 `WORK`의 최신 revision을 확인한 뒤 설정하세요. `--at-most`와 `--max-age-seconds`는 함께 필요합니다. 최대 자료 나이는 5~86,400초이며 새 자료와 이전 기준 모두에 적용됩니다. 정기 요약 선호는 기본 false이고 `--regular-summary true`로만 켭니다. 비교 판정 자체는 메시지를 보내지 않으며 아래 Email 알림 경로가 그 사실을 소비합니다.

```sh
shacs-bot work observe "$WORK" --workspace "$WS" --session cli:direct \
  --expected-revision 2 --source source.json --cadence-seconds 15 \
  --at-most 100 --max-age-seconds 3600 --regular-summary false
shacs-bot work observations "$WORK" --workspace "$WS" --session cli:direct --data-dir "$DATA"
curl --fail-with-body "http://127.0.0.1:8900/v1/sessions/cli%3Adirect/entrusted-work/$WORK/observations"
curl --fail-with-body -X POST "http://127.0.0.1:8900/v1/sessions/cli%3Adirect/entrusted-work/$WORK/observations" \
  -H 'content-type: application/json' \
  -d '{"expected_revision":3,"source":{"kind":"local_file","relative_path":"source.json"},"cadence_seconds":15,"comparison":{"criterion":{"kind":"integer_at_most","value":100},"max_age_seconds":3600,"regular_summary":true}}'
```

`observe`/POST는 plan 전체 교체입니다. 비교 flag/API `comparison`을 생략하면 비교를 끄고 digest 관측만 유지합니다. 같은 명령으로 상한·자료 나이·요약 선호를 수정하며 기존 CAS와 linked-target/loopback mutation 경계를 그대로 사용합니다. 변경은 revision을 올리고 비교 기준을 재설정하므로 이전 revision의 B를 새 조건의 중요 변화로 재해석하지 않습니다. 일반 `work revise`로 자유 형식 조건을 바꾸어도 revision이 달라져 기준이 재설정됩니다. 수치 상한 변경은 `observe`의 `--at-most`로 별도 명시해야 합니다.

실제 worker의 각 due를 기다리며 `120 → 120 → 110 → 90`을 기록하고, 값을 변경할 때 `as_of_ms`도 갱신하면 다음 결과를 조회합니다:

| 값 | verdict | reason.code | 의미 |
|---|---|---|---|
| 첫 120 | `baseline` | `first_comparable_observation` | 최초 기준, 변화 아님 |
| 다시 120 | `suppress` | `equivalent_value` | 동일값, timestamp/공백 차이는 중요도 아님 |
| 110 | `suppress` | `same_condition_outcome` | 여전히 100 초과, 사용자 선택 결과 동일 |
| 90 | `notify_candidate` | `condition_outcome_changed` | 상한 조건 미충족에서 충족으로 변경 |
| 다시 90 | `suppress` | `equivalent_value` | 동일 결과를 새 중요 사건으로 만들지 않음 |

충족에서 미충족으로 바뀌는 경우도 Notify 후보입니다. 응답의 `current_decision`, 최근 32개 `decisions`, `comparison_baseline`에서 이전/새 수치 요약과 조건 충족 여부, 조건 revision/정책, source 상대 경로, 관측 시각, evidence/snapshot ref, reason, 한계를 확인합니다. `regular_summary_eligible`는 성공적으로 비교 가능한 결과에 대한 별도 opt-in 사실이며 `verdict`를 바꾸지 않습니다. false인 무변화는 반복 보고 후보가 아닙니다. 조회에 아직 `current_decision: null`이면 현재 조건의 판정이 없는 것이지 무변화가 아닙니다.

판정은 관측·다음 wake 의도와 같은 029 terminal fact에 저장됩니다. `decision_id`는 관측 work ID(위임·조건 revision·goal transition·due)에 결속된 안정적인 ID이며 중복 tick과 재시작으로 바뀌지 않습니다. 원래 event log에서 마지막 비교 가능 기준을 별도로 유지하므로 최근 32개 조회에서 빠져도 기준이 사라지지 않습니다. 성공적인 baseline/suppress/notify 후보만 기준을 갱신하며 unknown/blocked는 갱신하지 않습니다. 읽기 도중 조건 변경·중단·취소가 일어나면 새 관측/판정 terminal은 저장하지 않고 기존 029 cancellation fact를 `blocked` 결정으로 조회합니다(`basis.kind: cancellation_fact`, 원래 event ID 포함). 이것은 취소 이후의 관측이나 실행 성공이 아닙니다.

오래된/미래/역순 시각, 같은 시각의 서로 다른 값, 불완전 스키마는 `unknown`입니다. 읽기 실패는 `unknown/observation_unavailable`, gate 거절과 구버전 취소는 `blocked/observation_blocked`로 보입니다. 첫 자료가 불완전했다면 다음 fresh/comparable 값은 변화 통지가 아닌 첫 `baseline`이 됩니다. 이미 있던 기준이 너무 오래됐거나 이력이 모순되면 `baseline_stale`/`baseline_unavailable`이며 B를 중요 변화로 확정하지 않습니다. source 누락으로 확인이 중단됐거나 오래된 기준을 새로 시작해야 한다면 자료를 고치고 최신 revision으로 `observe`를 다시 설정하세요. 구형 digest-only terminal은 `legacy_observation_uncompared` 근거의 unknown으로 읽으며 의미 기준으로 승격하지 않습니다. pause/resume 후 이전 goal 전이의 판정은 현재 판정으로 표시되지 않고 이력에만 남습니다. 관측 이력·판정 로그를 삭제하면 전체 연속성 복구를 보장하지 않습니다. 별도 evaluator/queue/session lifecycle, 비교·알림 렌더링용 provider 호출, 배송/수신 ACK, 자동 예약·구매·발송은 추가하지 않습니다.

### Email 변경 알림과 전달 상태 (PRD 030 첫 구현 단위)

현재 자동 알림 대상은 **사용자가 선택한 owner session `email:<수신 주소>` 하나**입니다. 기존 Email SMTP 설정(`channels.email.enabled`, `consentGranted`, `smtp`)을 마친 뒤 같은 config로 `runtime start`를 실행하세요. linked target은 조회·조건 변경 권한이며 자동 발송 대상 목록이 아닙니다. 다른 channel/CLI/API owner의 알림 후보는 `unsupported_target`으로 남고 다른 사용자·채널로 자동 전환하지 않습니다.

```sh
shacs-bot --config "$DATA/config.json" session create --workspace "$WS" --session email:me@example.com
shacs-bot work create "watch my integer" --workspace "$WS" --session email:me@example.com \
  --consent explicit-request --evidence-ref cli:user --instructions "notify when the integer condition changes"
WORK='<new-work-id>'
shacs-bot work observe "$WORK" --workspace "$WS" --session email:me@example.com \
  --expected-revision 1 --source source.json --cadence-seconds 15 --at-most 100 --max-age-seconds 3600
shacs-bot --config "$DATA/config.json" runtime start --workspace "$WS"
# 다른 터미널에서 같은 경로로 조회
shacs-bot work observations "$WORK" --workspace "$WS" --session email:me@example.com --data-dir "$DATA"
```

`source.json`에는 위의 typed JSON과 실제 `as_of_ms`를 사용합니다. 기본 설정에서 baseline/같은 값/선택 무관 변화/unknown/blocked는 침묵하고, 조건 충족 여부가 바뀐 후보만 이전·새 수치, 명시 기준, 이유, 두 evidence digest, 시각, 검증 한계와 tracking ID를 가진 제한된 메시지로 렌더링합니다. `--regular-summary true`일 때만 비교 가능한 매 관측을 별도 `regular_summary` 유형으로 요약합니다. 기존 `suppress` 판정을 change alert로 바꾸지 않으며 중요한 변화가 발생한 한 사건에서 change와 summary를 중복 발송하지 않습니다.

`notifications`는 관측 성공 및 `current_decision`과 별개입니다. `intent.correlation_id`는 관측 사건·조건에 결속되고 durable inbound, session turn, outbound, Email worker의 delivery metadata로 이어집니다. `enqueue`는 `pending`, `failed`, `enqueued`, `suppressed`, `unsupported_target`이며 `consumption_started`와 `suppressed_stale`도 따로 표시합니다. `delivery`의 `pending`은 아직 전달 의도/처리 중이라는 뜻, `failed`는 관찰한 dispatch/SMTP 실패, `sent_hint`는 SMTP send 반환값일 뿐 원격 수신 ACK나 읽음 확인이 아닙니다. 실행 중단 또는 다른 process가 확정 송신 사실 없이 읽은 처리 중 상태는 `unknown`일 수 있고, 억제된 경우는 `not_requested`입니다.

알림 enqueue 직전과 durable inbound 소비 직전에 동일 owner의 033 goal·동의·조건 revision·goal transition을 잠금 안에서 다시 확인합니다. 그 전에 pause/done/clear 또는 조건 변경이 완료되면 오래된 알림은 outbound와 assistant history를 만들지 않습니다. 합성 알림은 provider, 진행 중인 일반 턴의 입력 주입, `ask_user`, permission approval, workflow resume를 소비하지 않습니다. 소비 시작을 영속 기록한 뒤에는 같은 사건을 자동 재발행하지 않으므로 crash 중 전달 누락/unknown이 남을 수 있습니다. 재시도는 기존 transport의 `sendMaxRetries` 범위이고 새로운 알림 의도를 만들지 않습니다. 이미 outbound로 넘긴 메시지의 취소·회수와 exactly-once 송신은 보장하지 않습니다.

`serve`의 worker도 관측·enqueue를 하지만 Email transport를 실행하지 않아 알림이 pending으로 남습니다. Email 전달에는 해당 `serve`를 정상 종료하고 같은 data/workspace의 `runtime start`를 사용합니다. 새 `entrusted.notification_fact` event를 모르는 이전 binary로의 저장 데이터 downgrade는 지원하지 않습니다. 이 알림 단위는 실제 로컬 SMTP 수신·550 거절·재시작까지 확인했으며 원격 mailbox, 모든 channel parity나 PRD030 전체 완료를 뜻하지 않습니다. 외부 행동 결정은 다음 별도 경계를 사용합니다.

### 외부 행동 제안의 결정과 도구 권한

현재 owner session의 활성 위임에 대해 어시스턴트가 예약·구매·발송을 제안할 때는 `ask_user`의 `external_action` 필드를 사용합니다. 예를 들어 사용자 메시지에서 해당 work ID와 검토할 초안을 지정하면 provider는 다음 형식으로 질문할 수 있습니다. 이 질문 자체는 예약·구매·발송 도구가 아니며 외부 실행을 하지 않습니다.

```json
{
  "question": "준비한 문의 초안을 발송할까요?",
  "external_action": {
    "work_id": "work-<current-id>",
    "category": "send",
    "summary": "준비한 문의 초안 한 건 발송",
    "reference": "draft:inquiry-1"
  }
}
```

`category`는 `reserve`, `purchase`, `send`만 받습니다. 요약은 240자 이내, work ID와 reference는 영문·숫자·`:`·`_`·`-`·`.`로 된 160 byte 이내 식별자여야 합니다. 원문 전체, URL, credential을 넣지 마세요. Runtime은 요약에 기존 redaction을 적용하고 provider의 임의 질문·선택지 대신 work ID·조건 revision·유형·요약·reference와 고유한 action ID를 표시합니다. 같은 provider 응답에 다른 도구가 함께 있으면 실행하지 않고 질문만 처리합니다.

표시된 제안에 **원래 session/channel/chat의 같은 발신자**가 `/action accept <action-id>`를 정확히 보내면 해당 제안의 provider 대화만 한 번 재개합니다. `/action decline <action-id>`는 보류를 지우고 provider나 도구를 실행하지 않습니다. 응답하지 않거나 `yes`, `approve`, `1`, 다른 ID, `/work accept ...`를 보내도 행동 동의가 되지 않습니다. `/stop`으로도 보류를 취소할 수 있습니다. 일반 `ask_user`의 자유 응답과 `/work`의 지속 위임 동의는 외부 행동 결정과 별개입니다.

수락 직전과 후속 tool permission 응답 직전에 저장된 work ID·동의 근거·현재 조건 revision·goal ID/전이를 session mutation lock 안에서 다시 확인합니다. 조건 수정, pause/resume/done/clear, goal binding 변경 또는 실제 프로세스 재시작 뒤에는 이전 제안이나 이미 수락한 후속 대기를 닫습니다. 새 제안을 받아 다시 결정해야 합니다. 따라서 별도 프로세스로 반복 실행하는 `ask` 명령 사이에서는 이전 제안을 수락할 수 없으며, 같은 실행 중인 `agent` REPL 또는 channel/API runtime의 같은 대화를 사용해야 합니다. 연결된 다른 대상의 조회 권한만으로 대신 결정할 수 없습니다.

수락은 tool permission grant/cache를 만들지 않으며 성공을 뜻하지도 않습니다. 실제 후속 도구는 기존 030 hook·confirmation·permission·credential·sandbox 경계를 그대로 거칩니다. 별도 permission 질문이 나오면 그 도구의 `approve`/`deny` 등 기존 선택으로 결정하세요. 이미 설정된 허용/거절 정책도 그대로 유효합니다. 이 기능은 provider가 선언한 typed 제안의 결정 경계이며 자연어 문장의 모든 외부 행동을 자동 분류하거나 새 예약·구매 서비스, 모든 channel UI parity, 이미 실행된 부작용의 취소를 제공하지 않습니다.

### 웹페이지 한 곳의 값 관찰 (Spec 042 PRD 000)

활성 위임 하나에 **공개 웹페이지 한 곳의 값 하나**를 맡길 수 있습니다. 어시스턴트가 페이지를 직접 읽어 볼 값 후보를 보여주고, 사용자가 확인한 위치와 조건으로 주기적으로 다시 읽어 판정합니다. 이 단계는 알림을 보내지 않으며 CLI/API로 판정과 상태를 조회합니다. 로컬 파일 관측(`observe`)을 설정한 위임에는 웹페이지를 설정할 수 없고, 그 반대도 마찬가지입니다.

```sh
shacs-bot --config "$DATA/config.json" session create --workspace "$WS" --session cli:direct
shacs-bot work create "watch the price" --workspace "$WS" --session cli:direct \
  --consent explicit-request --evidence-ref cli:user --instructions "tell me when it is 11000 or cheaper"
WORK='<work-id-from-create>'
URL='https://shop.example.com/item/42?ref=mail'
shacs-bot work web-preview "$WORK" --workspace "$WS" --session cli:direct --url "$URL" --label "판매가"
shacs-bot work web-watch "$WORK" --workspace "$WS" --session cli:direct --url "$URL" \
  --candidate '<token-from-preview>' --at-most 11000 --cadence-seconds 3600 --expected-revision 1
shacs-bot work web-status "$WORK" --workspace "$WS" --session cli:direct --data-dir "$DATA"
shacs-bot --config "$DATA/config.json" serve --workspace "$WS" --port 8900 --allow-api-side-effects
```

- **미리보기:** `web-preview`는 페이지를 한 번 읽고 `--label` 뒤 80자 안의 첫 숫자를 후보로 보여줍니다. `--phrase`만 주면 그 문구가 지금 있는 자리를 후보로 보여줍니다. 아직 없는 문구(예: `재입고`)를 기다리려면 `--phrase "재입고" --label "재고:" --until "배송:"`처럼 영역의 시작 문구와 끝 문구를 함께 줍니다. 그러면 두 문구 사이 영역(최대 120자)마다 영역 내용과 지금 문구가 있는지(`present`)를 후보로 보여줍니다. 소수점이 쉼표인 페이지는 `--decimal-comma`를 붙입니다. 후보마다 페이지의 표시(`shown_text`), 읽은 값, 앞뒤 고정 문구·몇 번째 일치인지, 숫자 형식(소수점·천 단위 문자·통화나 단위)이 나옵니다. 미리보기는 아무것도 저장하지 않습니다. 볼 값을 찾지 못하면 후보가 빈 목록입니다.
- **확인:** `web-watch`는 고른 후보의 `token`과 조건 하나(`--at-most`, `--at-least`, `--text-present`, `--text-absent`)로 페이지를 **다시 읽습니다.** 미리보기 때와 같은 값일 때만 그 결과를 첫 기준으로 저장하고, 값이 바뀌었으면 아무것도 저장하지 않고 다시 미리보기를 요구합니다. 확인은 위임의 revision을 하나 올리는 CAS 설정입니다. 주소·위치·조건·주기를 바꾸려면 새 미리보기와 확인을 거치며 기준도 새로 잡힙니다. 확인 주기는 600~86,400초입니다.
- **API:** `POST /v1/sessions/<session>/entrusted-work/<work>/web-watch/preview` (`{"url": ..., "label": ...}`, `"phrase"`, 또는 `"phrase"`·`"label"`·`"until"`), `POST .../web-watch` (`{"expected_revision", "url", "candidate", "condition": {"kind": "at_most", "value": "11000"}, "cadence_seconds"}`), `GET .../web-watch`. POST는 기존 loopback mutation opt-in이 필요합니다.

**읽는 규칙(`normalized_static_text_v1`):** 주석과 `script`·`style` 요소를 빼고, 문단·표 칸 같은 블록 태그는 공백 하나로, 나머지 태그는 지운 뒤(HTML처럼 왼쪽부터 읽으므로 따옴표로 감싼 속성값 안의 `>`, `<!--`, `</script>`는 태그를 끝내거나 주석·스크립트를 열지 않고, 속성값은 본문이 되지 않습니다) `;`로 끝나는 HTML 문자 참조(`&euro;`, `&#8361;` 등)를 풀고 Unicode NFC로 맞추며 연속 공백을 공백 하나로 줄입니다. 대소문자는 그대로입니다. 숫자는 ASCII 숫자로 된 부호 없는 수만 읽고, 확인한 소수점·천 단위 문자와 숫자에 바로 붙은 통화·단위(`₩12,900`, `12,900원`)가 같아야 합니다. `-5`, `+5`, `−3`, `.5`처럼 부호나 앞 소수점이 붙은 수는 후보로 보여주지 않습니다. 이때 그 뒤의 다른 숫자를 대신 보여주지도 않습니다. `₩10,000~12,000`, `₩10,000 ~ ₩12,000`, `10,000 원 ~ 12,000 원`, `10,000 ~ -12,000`, `$10 to $12`처럼 범위로 적힌 값이나 바로 다음 단어에 이어지는 숫자도 한 값으로 보여주지 않습니다. 앞뒤 고정 문구 사이 영역이 확인 때와 같은 개수만큼 있어야 하고, 그 영역에서 숫자 하나만 읽습니다. 문구 조건은 그 영역에 확인한 문구가 그대로 있는지만 봅니다. 뒤 고정 문구는 앞 공백 없이 다음 단어만 담습니다. 그래서 값이나 문구만 지워져도 영역은 그대로 찾고, 숫자는 `format_mismatch`, 문구는 `present=false`로 읽습니다. 문자 집합은 응답 형식(`text/html`, `application/xhtml+xml` 등)과 관계없이 HTTP 응답의 `charset`만 따릅니다. charset이 없거나 모르는 이름이면 UTF-8로 읽습니다. 응답은 1 MiB까지만 읽으며 잘린 사실을 기록합니다.

serve 또는 channel `runtime start`의 기존 worker가 029 queue에 `web_watch.observation` work를 등록해 due마다 읽습니다. 주기 확인은 provider·agent turn·일반 도구를 쓰지 않고 위 규칙으로만 읽습니다. 판정은 036과 같습니다. 같은 값과 조건 결과가 같은 변화는 `suppress`, 조건 충족 여부가 바뀐 경우만 `notify_candidate`입니다. 페이지 문구 diff나 페이지에 적힌 지시는 판정에 쓰지 않습니다. resume이나 일반 `work revise` 뒤 첫 성공 확인은 새 기준(`baseline`/`lineage_reset`)입니다.

| 결과 | 분류 | 의미 |
|---|---|---|
| 연결 실패·시간 초과(redirect를 포함한 한 번의 확인 전체 20초) | `network_error` | 응답을 받지 못함 |
| 실행 중 사설 주소로 가는 redirect | `guard_blocked` | 요청하지 않음 |
| 404 등 실패 응답, redirect 5회 초과, http/https 이외 주소로의 redirect | `http_error` | 응답 상태만 기록 |
| 429 | `rate_limited` | `Retry-After`를 지킴 |
| 앞뒤 고정 문구를 찾지 못함 | `static_content_unavailable` | 로그인·JavaScript 때문이라고 단정하지 않음 |
| 일치 개수가 달라짐, 영역에 숫자 여러 개(범위 표기 등), 값 뒤로 범위가 이어지거나 같은 단위의 다른 값이 붙음(취소선 옛 가격 등) | `ambiguous_value` | 다른 자리를 대신 고르지 않음 |
| 숫자가 없음, 통화·단위·구분 문자가 다름 | `format_mismatch` | 값을 추측하지 않음 |

실패는 모두 `unknown`이며 기준을 바꾸지 않고, 다음 확인을 멈추지 않습니다. 429와 503에는 `Retry-After`(초 또는 HTTP 날짜, 최대 7일)와 연속 횟수에 따른 대기(주기×2ⁿ, 최대 하루)를 주기보다 우선합니다. 한 번의 확인은 redirect 경로 하나만 따라가며 바로 다시 요청하지 않습니다. 첫 요청을 포함한 매 요청 직전에 주소를 다시 검사합니다. 성공한 확인 뒤 첫 실패에서 실패 구간이 시작되고 성공한 추출만 구간을 닫습니다. 어시스턴트가 꺼져 예정 확인이 주기 이상 밀리면 실패가 아니라 `runtime_gap`으로 기록하고 다시 켜지면 밀린 확인을 한 번 바로 실행합니다. 공백은 실패 구간을 닫거나 나누지 않습니다. pause/done/clear 뒤의 예약 확인은 관측 없이 취소되고 열린 구간은 알림 없이 닫힌 것으로 표시됩니다. `web-status`·`GET`은 계획, 첫 기준, 마지막 성공 확인 시각, 현재 판정, 열린 실패 구간(시작 시각·횟수·분류), 기록된 공백, 다음 확인, 최근 32개 기록을 보여줍니다.

**저장하는 것과 하지 않는 것:** 확인한 전체 URL은 위임 owner session 파일에 계획으로만 저장합니다. `session export`(JSON·JSONL)도 다른 출력처럼 이 계획 주소를 query와 fragment를 뺀 주소와 전체 URL의 SHA-256으로 바꿔 내보냅니다. 대화 메시지는 export에 원문 그대로 들어갑니다. 관측 기록, 판정, `web-status`·API 응답과 hook에 넘기는 값은 URL의 query(`?…`)와 fragment(`#…`)를 뺀 주소와 전체 URL의 SHA-256만 담습니다. 응답 본문과 주변 문구는 저장하지 않고, 정규화한 텍스트의 SHA-256, HTTP 상태, 최종 주소, 잘림 여부, 읽은 값만 남깁니다. 앞뒤 고정 문구는 확인한 추출 기준이라서 저장됩니다. 다만 `web-preview` 후보, `web-watch` 확인 응답, `web-status`·API 응답과 `session export`는 문구 조건의 문구, 앞뒤 고정 문구와 후보 표시(`shown_text`)를 비밀값을 가리고 그 안의 URL은 scheme·host·path만 남겨(사용자 정보·query·fragment 제거) 보여줍니다. session 파일의 추출 기준은 페이지와 맞춰 볼 수 있도록 원문 그대로 둡니다. 후보의 `token`도 확인할 때 쓰려고 원래 기준을 그대로 담고 있으므로 다른 곳에 남기지 마세요.

**지원하지 않는 것:** JavaScript 실행, 로그인·cookie·인증 header·credential이 필요한 페이지, URL의 사용자 이름·비밀번호, http/https 이외 scheme, 접근 차단이나 이용 제한의 우회, 여러 값·여러 페이지입니다. 내부·사설 주소는 `tools.ssrfWhitelist`와 관계없이 설정 단계와 실행 중 모두 거절합니다. 프록시 환경 변수(`ALL_PROXY`, `HTTPS_PROXY`, `HTTP_PROXY`)는 쓰지 않고 페이지에 직접 연결합니다. 그래서 프록시를 거쳐야만 밖으로 나갈 수 있는 환경에서는 `network_error`가 됩니다. 주소 확인과 실제 연결 사이에 DNS 응답이 바뀌는 경우까지 막지는 않습니다. 위임 확인은 이 읽기 하나의 근거일 뿐 `web_fetch` 같은 도구 권한이 아니며 예약·구매·발송 승인이 아닙니다. 읽기 전에는 031 실행 snapshot과 030 `tool:before` hook(`web_watch_fetch`, 인자는 query를 뺀 주소)을 거치며 hook이 거절하면 `blocked`로 기록하고 다음 주기에 다시 확인합니다.

종료 시험은 `--features spec042-test-profile`로 따로 빌드한 binary에서만 `SHACS_SPEC042_ALLOW_LOOPBACK=1`(127.0.0.1/32 허용)과 `SHACS_SPEC042_CLOCK_FILE`(시험용 시계)을 읽습니다. 일반 빌드와 Docker 빌드는 이 변수를 무시합니다.

### 웹페이지 관찰 알림 (Spec 042 PRD 010)

웹페이지 관찰을 확인한 위임에 Email 알림 대상 하나를 정하면, 아래 네 가지 경우에만 그 주소로 메일을 보냅니다. 알림이 없다면 기준 시간을 넘는 확인 공백 없이 확인했고, 조건 결과가 바뀌지 않았다는 뜻입니다.

```sh
shacs-bot work web-notify "$WORK" --workspace "$WS" --session cli:direct \
  --target email:me@example.com --failure-threshold-seconds 21600 --config "$DATA/config.json"
shacs-bot work web-notify "$WORK" --workspace "$WS" --session cli:direct --off
```

- **설정:** `--target email:<address>`는 위임마다 하나이며 owner session이나 036 `linked_targets`와 따로 저장됩니다. config의 Email 채널이 켜져 있고(`enabled: true`) `consentGranted`와 `smtp` 설정으로 보낼 수 있어야 하며, 그렇지 않거나 `email:`이 아닌 대상은 거절합니다. `--config`는 다른 명령처럼 `shacs-bot --config <path> work web-notify …`로 앞에 써도 되고, 앞뒤에 모두 쓰면 명령 뒤의 값을 씁니다. 실패 알림 기준 시간은 기본 `max(확인 주기×2, 6시간)`이고 `확인 주기×2`~7일에서 바꿀 수 있습니다. 바꿀 때마다 알림 설정 revision이 하나 오르며, 관측 revision과 비교 기준은 그대로입니다. `--off`는 알림을 끕니다. API는 `POST .../web-watch/notify`(`{"target": ..., "failure_threshold_seconds": ...}` 또는 `{"off": true}`)입니다.
- **변화 알림:** 조건 충족 여부가 바뀐 확인(`notify_candidate`)마다 한 번 보냅니다. 이전/새 값, 조건(문구 조건의 문구는 비밀값과 URL의 사용자 정보·query·fragment를 지워서)과 충족 여부, query·fragment를 뺀 주소와 SHA-256, 두 확인의 읽은 시각, 텍스트 digest와 HTTP 상태, 확인 한계, 추적 ID를 담습니다. 실패 중에는 값을 읽지 못하므로 변화 알림이 없습니다.
- **확인 실패 알림:** 실패 구간이 기준 시간 이상 이어지면 구간마다 한 번 보냅니다(분류, 시작 시각, 마지막 성공 시각, 다음 확인 시각). 구간 길이는 첫 실패부터 셉니다. 아직 실패 중이면 마지막 실패 확인까지, 회복했다면 회복을 확인한 성공까지이며, 어시스턴트가 꺼져 있던 시간은 뺍니다. 기준 시간 전에 회복한 구간은 아무것도 보내지 않고, 회복을 확인한 순간 기준 시간에 이르렀다면 실패 알림 뒤 회복 알림을 보냅니다. 사이트의 `Retry-After`나 429·503 대기 때문에 다음 확인이 기준 시간을 넘겨 잡히면, 그 사이에는 회복을 확인할 수 없으므로 그 실패에서 바로 알립니다. 알림 설정을 바꾸거나 새로 켜도 열린 구간은 이어지며, 다음 확인을 기다리지 않고 마지막 실패 확인까지의 길이를 새 기준 시간으로 바로 다시 판단합니다(이미 넘었으면 바로 알림). 그보다 먼저 성공한 확인이 구간을 닫았더라도, 그 확인이 기록될 때의 설정으로 기준을 넘은 구간이면 실패 알림을 먼저 보내고 이어서 회복을 알립니다. 이미 보낸 실패 알림은 다시 보내지 않습니다.
- **회복 알림:** 실패 알림을 보낸 구간이 성공으로 닫히면 한 번 보냅니다. 그 성공이 조건 결과도 바꿨다면 회복 사실을 담은 변화 알림 한 통만 보냅니다. 실패 알림이 보내지지 못한 채 queue에서 끝났다면(전달 시도 소진·취소) 회복 알림도 보내지 않고, 변화 알림에서는 회복 문구만 뺍니다. 실패 알림의 보내기가 시작됐다면 그 뒤 SMTP가 실패했더라도 회복은 알립니다.
- **공백 알림:** 어시스턴트가 꺼져 예정 확인이 기준 시간 이상 밀렸다면, 다시 켜진 뒤 밀린 확인을 바로 하고 공백 기간을 한 번 알립니다. 공백은 실패 구간 길이에 넣지 않으므로 같은 공백 때문에 실패 알림이 따라오지 않습니다. pause/done/clear 상태의 위임은 확인하지 않으므로 공백 알림도 없습니다.

알림은 확인을 기록한 그 시점의 알림 설정 revision으로만 만들어지고(열린 실패 구간만은 현재 설정으로 다시 판단), 사건(변화·실패 구간·회복·공백)마다 한 번만 durable queue에 들어갑니다. 재시작, 중복 wake, 재시도에도 같은 사건을 다시 넣지 않습니다. 같은 주소로 가는 알림은 queue에 들어간 순서대로 보내며, 앞선 알림이 다시 넘기기를 기다리는 동안 뒤의 알림도 기다립니다. enqueue 직전과 보내기 직전에 위임의 goal·동의·관측 revision·계획과 알림 설정 revision·대상을 다시 확인하고, 바뀌었으면 보내지 않습니다(`suppressed_stale`). queue에 들어갔지만 아직 보내기 전인 알림도 새 알림을 만들기 전에 같은 확인으로 먼저 억제하므로, 설정을 바꾼 뒤의 실패·회복 판단은 그 알림을 보낸 것으로 치지 않습니다. 보내기를 시작한 뒤 중단되면 다시 보내지 않고 전달 상태를 `unknown`으로 둡니다. 알림 문장은 저장된 사실로만 만들고 모델을 쓰지 않으며, 응답 본문·URL query·credential을 담지 않습니다. `web-status`·`GET`의 `notify`는 현재 설정을, `notifications`는 알림마다 종류·사건·설정 revision·enqueue 상태·enqueue 실패 횟수(`enqueue_failures`)·전달 상태(queue에 들어가기 전이거나 억제된 `not_requested`, `pending`, `sent_hint`, SMTP 실패나 보내기 전에 queue 작업이 끝난 `failed`, `unknown`)를 확인 기록과 따로 보여줍니다. 메일 전송 실패는 확인 성공을 바꾸지 않습니다. queue에 넣지 못한 알림은 enqueue 상태를 `failed`로 남기고 확인과 다른 알림은 그대로 이어가며, 1분 뒤부터 처음 기록된 순서대로 다시 시도해 모두 5번까지 시도합니다. 5번 모두 실패하면 더 시도하지 않고 enqueue `failed`로 남습니다.

메일은 Email 채널을 실행하는 channel runtime(`run`, `runtime start`)이 보냅니다. `serve`의 worker도 확인과 enqueue는 하지만 Email transport를 실행하지 않아 알림이 `pending`으로 남습니다. 이 알림을 보내려면 해당 `serve`를 정상 종료하고 같은 data/workspace로 `runtime start`를 실행합니다. 이때도 보내기 직전에 위임과 알림 설정을 다시 확인하므로 그사이 바뀌었으면 보내지 않습니다(`suppressed_stale`).

### 결정 마감 알림 (Spec 042 PRD 020)

웹페이지 관찰과 Email 알림 대상(`web-notify`)을 정한 위임에 결정 마감을 정하면, 알림 시점에 페이지 값이 바뀌지 않았어도 마감 알림을 한 번 보냅니다.

```sh
shacs-bot work web-deadline "$WORK" --workspace "$WS" --session cli:direct \
  --at 2026-11-01T18:00:00+09:00 --expected-deadline-revision 0
shacs-bot work web-deadline "$WORK" --workspace "$WS" --session cli:direct --clear --expected-deadline-revision 1
```

- **설정:** `--at`은 시간대 오프셋(`Z`, `+09:00` 등)이 붙은 64자 이하의 RFC 3339 시각만 받으며, 입력한 표기 그대로와 절대 시각을 함께 저장합니다. 알림 시점은 기본 마감 24시간 전이고 `--remind-before-seconds`(60초~30일)로 바꿀 수 있습니다. 마감 revision은 관측 revision과 따로이며 바꿀 때마다 하나 오릅니다. 바꿀 때는 마지막에 본 revision을 `--expected-deadline-revision`으로 주어야 합니다(처음은 0). 마감을 바꾸거나 지워도 관측 revision과 비교 기준은 그대로입니다. 확인된 웹페이지 계획과 Email 알림 대상이 있어야 하며, `--clear`는 대상이 없어도 됩니다. API는 `POST .../web-watch/deadline`(`{"expected_revision": 0, "at": "...", "remind_before_seconds": ..., "remind_now": false}` 또는 `{"expected_revision": 1, "clear": true}`)이고, `web-status`·`GET`의 `deadline`이 현재 설정입니다.
- **거절:** 이미 지난 마감과 오프셋이 없는 시각은 거절합니다. 마감은 미래지만 알림 시점이 이미 지났다면 `--remind-now`(API `remind_now`)를 명시했을 때만 받고, 이때는 설정 직후 바로 알립니다. 알림 시점이 아직 미래인데 `--remind-now`를 주면 거절합니다.
- **알림:** worker가 029 durable work로 알림 시점에 한 번 깨어나 위임 목표, 마감(UTC와 입력한 표기), 남은 시간, 조건, 마지막으로 읽은 값과 그 시각을 담아 PRD 010의 알림 대상과 경로로 보냅니다. 마감 revision 하나에 한 번이며 재시작이나 중복 wake에도 다시 보내지 않습니다. 어시스턴트가 꺼져 있어 알림 시점을 지나쳤다면 다시 켜진 뒤 마감 전이면 늦게라도 한 번 보내고, 마감이 지났으면 보내지 않습니다. 마감 전에 queue에 들어간 알림도 보내기 직전에 마감이 지났으면 보내지 않습니다(예: `serve`만 실행하다가 마감 뒤에 channel runtime을 시작한 경우). 목표는 비밀값(Bearer·Basic·Digest 등 scheme과 관계없는 `Authorization:`·`Authorization=` 값, 모든 종류의 private key 블록 포함)과 URL의 사용자 정보·query·fragment(`shop.example/item?…`처럼 scheme 없이 적은 주소 포함)를 지우고 한 줄 240자까지 보여줍니다. 문구 조건의 문구도 알림에서는 같은 방식으로 가립니다.
- **취소와 재개:** 마감이나 알림 시점을 바꾸거나 지우면, 또는 pause/done/clear나 동의 철회 뒤에는 이전 예약을 취소하고 알리지 않습니다. resume했을 때 알림 시점이 아직 미래면 다시 예약하고, 이미 지났으면 소급해 보내지 않습니다. 마감이 지난 뒤에도 goal이 active면 관측은 계속하고 마감 알림은 더 만들지 않습니다.
- **오래된 알림:** 마감 알림은 마감 revision과 알림 설정 revision에 함께 묶입니다. enqueue 직전과 보내기 직전에 위임의 goal·동의·lineage, 두 revision, 대상을 다시 확인하고 바뀌었으면 보내지 않습니다(`suppressed_stale`). 대상이 바뀌어 억제됐다면 마감 전인 동안 새 대상으로 한 번 다시 만듭니다.
- **비보장:** 원격 수신 확인과 exactly-once 전달은 없고, 메일은 channel runtime(`run`, `runtime start`)만 보냅니다. 반복 리마인더, 마감 자동 추정, 마감 뒤 자동 중단이나 자동 행동은 없습니다.

### 대화로 웹페이지 맡기기 (Spec 042 PRD 030)

채팅(WebSocket·Telegram 등 channel, `agent`)에서 "이 페이지 가격 좀 봐줘, 11000원 이하가 되면 user@example.com으로 알려줘"처럼 부탁하면, 어시스턴트가 무엇을 어디서 어떤 조건으로 보고 어떻게 알릴지 요약을 보여줍니다. **확인하기 전에는 아무것도 만들지 않습니다.** 확인하면 위에서 설명한 위임(036), 웹페이지 관찰과 첫 기준(PRD 000), Email 알림 설정(PRD 010), 결정 마감(PRD 020)을 한 번에 설정합니다.

- **요약:** 모델은 `propose_web_watch` 도구로 제안만 하고, 페이지는 런타임이 직접 한 번 읽어 볼 값의 지금 값과 위치(PRD 000 등록 후보의 첫 번째. 같은 앞뒤 문구가 여러 곳에 있으면 확인 뒤 읽을 곳이 그중 몇 번째인지)를 보여줍니다. 요약에는 요약 ID와 revision, 목표, 페이지 주소(query·fragment는 저장되지만 보이지 않음), 볼 값, 조건과 지금 충족 여부, 확인 주기(기본 1시간, 10분~1일), 알림 메일 주소와 실패 알림까지의 시간, 결정 마감과 알림 시점이 들어갑니다. "싸지면"처럼 막연한 바람은 지금 값보다 한 단위 낮게(`below_current`)·높게(`above_current`) 같은 지원 조건으로 바꿔 보여줍니다. 목표 문장도 주소의 query·fragment를 뺀 채 보이고, 보인 그대로 저장됩니다. 진단 출력(`--verbose` 로그, 도구 진행 이벤트)에는 제안의 숫자·참거짓·`null` 값, 한 단어로 쓴 조건 종류와 work ID, 숫자로 쓴 기준값만 나오고 주소·메일·목표·문구 같은 다른 글은 한 단어여도 정해졌다는 표시(`<set>`)로만 나오며(상태 질문 도구도 한 단어로 쓴 work ID만 보임), `--verbose`의 메시지·응답 미리보기에서 주소는 scheme·host·path로만 보입니다. provider가 붙인 도구 호출 ID가 영문자·숫자·`_`·`-`·`.`·`:`로 된 128자 이하 토큰이 아니면(주소를 담았을 수 있으므로) 진단 출력(진행 이벤트, 권한 자동 승인 진단, 실행 결과의 `tool_events`), plugin hook(hook이 막은 기록 포함)과 실행 기록(`runtime_execution`)에는 그 ID 대신 그 SHA-256 digest(`sha256:…`)가 나옵니다. plugin의 `tool:before` 검사(외부 명령과 JavaScript·TypeScript)도 제안과 상태 질문의 인자를 진행 이벤트와 같은 모양으로 받습니다(주소·메일·목표·문구는 정해졌다는 표시로만). 도구마다 실행 승인을 묻는 설정이면 제안 도구의 실행을 승인한 뒤에도 요약 확인이 따로 필요합니다(도구 실행 승인은 위임 확인이 아닙니다). 제안한 뒤 어시스턴트가 질문(`ask_user`나 권한 질문)을 하면 질문이 먼저 나오고, 답해서 그 turn이 끝날 때 요약이 나옵니다. 그 질문에 같은 대화방의 다른 사람이 답했으면 요약 대신 이유가 나오고 아무것도 기다리지 않으니, 맡길 사람이 다시 요청하고 질문에도 직접 답하면 됩니다. 요약 뒤에는 workflow 상태 같은 다른 메시지를 붙이지 않아 요약이 마지막 메시지로 남습니다. 제안 뒤의 모델 호출이 실패하거나 모델이 오류로 답해도 요약(또는 거절 이유)은 오류 문장 뒤에 나옵니다. 어시스턴트가 `message` 도구로 답을 따로 보낸 turn의 제안은 다른 도구를 쓴 turn이라 거절되고, 그 이유가 보낸 메시지 뒤에 옵니다.
- **안 되는 요청:** 모델이 같은 turn에서 다른 도구(페이지 읽기·검색·파일 등)를 쓴 뒤 낸 제안(읽은 내용이 조건이나 주기를 바꾸지 못하게. 부탁한 메시지부터, 권한 질문에 승인하거나 어시스턴트의 질문에 답해 이어진 부분까지 한 turn으로 봅니다), 사용자가 보내지 않은 turn(예약 작업, 하위 작업의 결과, 자동화)에서 낸 제안, 사용자가 직접 보낸 메시지에 쓰지 않은 페이지 주소나 메일 주소(페이지 내용이나 하위 작업의 결과가 주소를 바꿀 수 없게, 쓴 주소의 일부만 잘라 쓴 것도 안 됨), 내부 주소, 사용자 정보가 든 주소, 보낼 수 없는 Email 채널, 볼 값을 찾을 수 없는 페이지, 이미 다른 위임이 진행 중인 대화는 요약 단계에서 이유와 대안을 알려주고 아무것도 기다리지 않습니다.
- **확인 방법(두 가지뿐):** (1) 그 요약이 어시스턴트의 가장 최근 메시지일 때, 부탁한 사람이 다음 중 하나로만 답합니다: `응`, `네`, `예`, `맞아`, `맞아요`, `그렇게 해`, `그렇게 해줘`, `좋아`, `yes`, `ok`, `okay`(앞뒤 공백·끝의 `.`·`!`·`~`는 무시). (2) 버튼을 지원하는 채널(WebSocket·Telegram)의 확인 버튼, 또는 같은 내용인 `/watch confirm <요약 ID> <revision>`. `아니`, `아니요`, `아니오`, `취소`, `no`, `cancel` 또는 `/watch cancel <요약 ID>`는 요약을 닫습니다.
- **확인되지 않는 경우:** 다른 사람의 답이나 버튼, 이전 요약이나 이전 revision의 버튼은 아무것도 만들지 않고 요약을 다시 보여줍니다. 요약 뒤에 다른 응답(`/status` 같은 명령 응답, 어시스턴트의 다른 질문이나 권한 질문, `message` 도구로 보낸 답, 같은 대화로 온 변화·실패·마감 알림 메일)이 나갔다면 확인 답과 취소 답(`아니` 등)은 그 응답에 대한 답으로 처리되고 요약을 확인하거나 닫지 않습니다(버튼과 `/watch cancel`은 그대로 쓸 수 있고, 다른 질문이 답을 기다리는 중에도 그 질문을 그대로 둔 채 처리됩니다). 그 밖의 답(예: "음...", "응 근데 주기는 바꿔줘")은 평소처럼 대화하고 끝에 같은 요약을 다시 묻습니다. 바꾸고 싶은 점을 말하면 새 요약이 나옵니다. 새 요약이 나오거나, `/new`로 새로 시작하거나, 어시스턴트가 다시 시작되거나, 30분 동안 확인이 없으면 대기 중인 요약은 닫힙니다.
- **확인 순간:** 페이지를 다시 읽어 요약에 보인 값과 같을 때만 설정합니다. 값이 바뀌었으면 바뀐 값을 담은 다음 revision을 보여주고 다시 확인을 받습니다. 설정 도중 하나라도 실패하면(예: 그사이 Email 채널을 끔) 아무것도 남기지 않고 이유를 알려줍니다. 위임의 동의 근거는 `web-watch-summary:<요약 ID>:<revision>`입니다. 확인은 이 관찰만 맡기는 것이며 도구 권한이나 예약·구매·발송 승인이 아닙니다.
- **물어보기:** "마지막으로 언제 확인했어?", "지금 상태 어때?"처럼 물으면 모델이 `web_watch_status` 도구를 부르고, 런타임이 `web-status`와 같은 저장된 사실(마지막 성공 확인 시각과 그때 읽은 값, 마지막 판정과 그 이유, 진행 중인 실패, 다음 확인, 알림 대상, 마감)로 답합니다. 같은 말에서 바꾸기도 부탁하면 저장된 사실 뒤에 요약이 나옵니다. 그 turn이 어떻게 끝나든 이 답이 나갑니다: 어시스턴트가 `message` 도구로 다른 말을 보냈으면 그 뒤에, 되묻는 질문으로 끝났으면 그 질문 앞머리에(같은 메시지로), 도구 실행 승인을 거쳤으면 승인 뒤에, 그 뒤의 모델 호출이 실패했거나 모델이 오류로 답했으면 오류 문장 뒤에 보냅니다(`message` 도구로 먼저 보낸 말은 거둘 수 없습니다).
- **바꾸기:** "알림은 다른 주소로", "마감을 바꿔줘", "12000원 이하로 바꿔줘"처럼 말하면 바뀌는 점을 담은 새 요약이 나오고, 확인해야 적용됩니다. 이 요약에도 볼 값의 위치와 지금 값(페이지 표기 그대로)이 나옵니다. 메일 주소·실패 알림 시간·마감만 바꾸면 해당 revision만 오르고 비교 기준은 그대로입니다. 마감은 말한 항목만 바뀌므로 "마감 2시간 전에 알려줘"처럼 알림 시점만 바꿀 수 있습니다(마감이 없으면 마감 시각도 함께 말해야 합니다). 이때 지금 값은 한 번 읽어 보여주기만 하며, 페이지를 읽을 수 없으면 그 이유가 나오고 변경은 그대로 확인할 수 있습니다. 알림 메일 주소를 그대로 두는 변경도 그 주소로 보낼 수 있어야 하므로, 요약 때와 확인 때 모두 Email 채널이 보낼 수 있는지 확인하고 아니면 아무것도 바꾸지 않습니다. 확인 주기만 늘리면 정해 둔 실패 알림 시간은 그대로 두고, 새 주기의 2배보다 짧아지면 2배로 적용합니다(PRD 010과 같음). 페이지·볼 값·조건·확인 주기가 바뀌면 확인 순간 다시 읽은 값이 새 비교 기준이 됩니다. 요약을 만든 뒤 다른 곳(CLI·API 등)에서 관찰·알림·마감 중 하나라도 바뀌었으면, 요약이 바꾸는 항목이 아니어도 확인 때 거절하고 다시 물어보게 합니다. pause/resume/done/clear는 `/work pause|resume|done|clear <work-id>`로 합니다.
- **비보장:** 한 대화에서 하나의 위임과 한 페이지만 맡깁니다. 확인 문구 목록 밖의 표현은 확인으로 보지 않습니다. 앞선 turn에서 어시스턴트가 읽은 페이지 내용은 대화 기록에 남아 다음 제안의 조건이나 주기에 영향을 줄 수 있습니다. 주소와 메일은 직접 쓴 것만 쓰이므로, 조건과 주기는 요약에 보이는 대로인지 확인한 뒤 답하세요. 도구마다 실행 승인을 묻는 설정의 승인 질문은 다른 도구처럼 제안 도구의 인자를 그대로(쓴 주소 포함) 보여줍니다. `agent -m`처럼 메시지마다 프로그램을 새로 실행하면 다음 메시지 전에 요약이 닫히므로(재시작과 같음), 확인은 `agent` REPL이나 channel에서 합니다. 요약 상태는 session 파일에 저장되고, `session export`는 그 안의 후보 token을 빼고 주소의 query·fragment를 가려 내보냅니다. 대화 기록, 끝나지 않은 turn의 복구 기록, 대기 중인 승인 안의 제안·상태 질문 도구 호출(인자 이름 포함)과 결과에서도 주소의 query·fragment를 빼고 내보냅니다. 그 밖의 내용(사용자가 쓴 글, 요약과 상태 답에 보이는 알림 메일 주소 등)은 저장된 그대로입니다. 로컬 API를 streaming(SSE)으로 쓰면 모델의 글 뒤에 요약·확인 결과·상태 답과, turn을 끝내는 질문이 이어서 옵니다. 어시스턴트가 직접 쓰는 대화 문장(streaming 조각 포함)은 사용자가 쓴 주소를 그대로 되풀이할 수 있습니다.

## Spec 033 goal, improvement, replay

Goal lifecycle은 session metadata owner를 사용하며 CLI와 local API가 같은 typed projection을 반환합니다:

```sh
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- goal set "verify the workspace" --workspace /tmp/shacs-ws --session cli:direct
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- goal pause --workspace /tmp/shacs-ws --session cli:direct
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- goal resume --workspace /tmp/shacs-ws --session cli:direct
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- goal done --workspace /tmp/shacs-ws --session cli:direct
curl http://127.0.0.1:8900/v1/sessions/cli%3Adirect/goal-snapshot
```

Local artifact 개선 흐름은 명시적 root, proposal ID, target, candidate, immutable snapshot, expected digest를 요구합니다:

```sh
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- improve propose --root /tmp/shacs-improvement --proposal proposal-1 --target settings.json --candidate /tmp/candidate.json --snapshot /tmp/execution-snapshot.json --expected-digest sha256:<digest>
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- improve apply --root /tmp/shacs-improvement --proposal proposal-1
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- improve verify --root /tmp/shacs-improvement --proposal proposal-1
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- improve candidate --root /tmp/shacs-improvement --proposal proposal-1
```

Replay는 local recorded trajectory만 읽고 hook, confirmation, credential refresh, process, sandbox, delivery, config/apply를 실행하지 않습니다. 자세한 closure evidence와 non-guarantees는 [`specs/033-evaluation-automation-live-integration/CLOSURE.md`](specs/033-evaluation-automation-live-integration/CLOSURE.md)를 참고하세요.

이 문서는 사용자가 자신의 workspace에서 `shacs-bot`을 로컬로 실행하는 경우를 기준으로 합니다. SaaS control plane, fleet operator, 별도 관리자 조직을 전제로 하지 않습니다.

설계 계약과 invariant는 [docs/specs/README.md](specs/README.md)를 참고하세요. 이 문서는 현재 Rust CLI에서 실제 구현된 사용자-facing 표면을 설명합니다.

## 소스에서 빌드

저장소 루트에서 실행합니다:

```sh
cargo clean --manifest-path crates/Cargo.toml
cargo build --manifest-path crates/Cargo.toml -p shacs-cli --locked
```

아래 예시는 `shacs-bot` binary가 `PATH`에 있다고 가정합니다. 소스 checkout에서 바로 실행할 때는 필요하면 명령 앞에 `cargo run --manifest-path crates/Cargo.toml -p shacs-cli --`를 붙이세요:

```sh
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- status
```

## 설정

현재 Rust CLI는 하나의 JSON config 파일을 사용합니다. 기본 경로는 다음과 같습니다:

```text
$HOME/.shacs-bot/config.json
```

특정 config 파일을 사용하려면 `--config <path>` 또는 `-c <path>`를 지정합니다. Runtime 명령은 config를 저장하지 않는 일회성 workspace override로 `--workspace <path>` 또는 `-w <path>`도 받습니다.

각 config 파일의 parent directory가 해당 instance의 data directory입니다. 예를 들어 `/tmp/a/config.json`와 `/tmp/b/config.json`를 따로 쓰면 `auth.json`, `media/`, `cron/`, `logs/`, `channels/worker-metadata/`, `skills/`도 각각 `/tmp/a/`, `/tmp/b/` 아래로 분리됩니다. workspace는 config 값 또는 `--workspace` override로 별도 지정할 수 있지만, runtime metadata는 config별 data directory를 기준으로 유지됩니다.

**`--workspace`만으로 설정·인증은 격리되지 않습니다.** `onboard`는 선택된 config를 쓰므로 `--config`를 생략한 예시는 기본 사용자 설정을 생성·갱신합니다. 임시 검증에는 새로 확보한 사용자 소유 data directory의 `config.json`과 별도 workspace를 쓰고 CLI/TUI/API 시작·provider 인증 명령 모두에 같은 `--config`를 지정하세요. 같은 parent 아래 config 파일 이름만 바꾸면 `auth.json`과 runtime data를 공유합니다. 실제 config/auth를 임시 검증 경로에 복사하지 마세요. 아래 `/tmp` 경로는 예시일 뿐 기존 경로의 소유권이나 안전한 삭제 권한을 뜻하지 않습니다.

Config와 workspace template을 생성하거나 갱신합니다:

```sh
shacs-bot onboard --workspace /tmp/ws
shacs-bot --config /tmp/shacs-config.json onboard --workspace /tmp/ws
shacs-bot --config /tmp/shacs-config.json onboard --wizard --workspace /tmp/ws
```

`onboard`는 JSON config를 쓰고, runtime directory를 준비하며, `AGENTS.md`, `SOUL.md`, `USER.md`, `TOOLS.md`, `memory/MEMORY.md`, `memory/history.jsonl`, `skills/` 같은 workspace template 파일을 만듭니다. 또한 active built-in skill을 `builtin_skills/` 아래에 materialize하지만, reference-only deferred built-in skill은 복사하지 않습니다. 이미 존재하는 workspace 파일은 덮어쓰지 않습니다.

`onboard --wizard`는 raw secret 값을 묻거나 저장하지 않는 secret-ref-only line-driven wizard입니다. 현재 명령은 `provider <provider-id> env <ENV_VAR>`, `finish`, `cancel`, `restart`, `help`입니다. `provider` 명령은 ASCII uppercase letter로 시작하고 1..64자 범위이며 uppercase/digit/underscore만 포함하고, semantic word를 나누는 non-edge underscore가 하나 이상 있는 environment variable reference만 받습니다. 반복 underscore, edge underscore, lowercase, URL/userinfo, assignment, JWT/base64-like token, long alphanumeric token, raw secret-like word는 거부되며 입력값을 stdout/stderr/error/marker에 echo하지 않습니다. 기존 provider `apiKey`/`api_key` 또는 `apiKeyRef`/`api_key_ref`가 있으면 덮어쓰지 않고 실패합니다. `finish` 전 EOF는 partial로 끝나며 typed resume marker에 provider/env ref 진행 상태만 저장합니다. 완료 출력은 별도 aggregate readiness와 함께 실제 local Spec030 owner의 credential/disclosure/decision 관측 및 Spec031 config/profile의 `credential_declarations`, profile selection과 source flag를 표시합니다. 선언은 credential 존재·해석 성공·readiness·권한 부여의 증거가 아닙니다. 이 경로는 environment secret 해석, credential command 실행이나 OAuth refresh를 하지 않으며, 관측하지 못한 runtime credential 상태는 `unavailable`로 유지합니다. [Wizard 보정 증거](../.omo/evidence/spec035/closure/f1-wizard-owner-remediation.json)의 resolved/denied mapper fixture는 live credential·승인 QA가 아닙니다.

Config 문자열 값은 load 시 `${ENV_NAME}` 형태의 environment variable reference를 해석합니다. 예를 들어 provider key는 config에 `"apiKey": "${OPENROUTER_API_KEY}"`로 남겨두고 실행 환경에서 값을 제공할 수 있으며, migration write-back은 실제 secret 값을 config 파일에 저장하지 않습니다. 참조한 environment variable이 없으면 config load가 실패합니다.

스킬의 `requires.env` 확인이나 `exec`/subagent 실행에 필요한 환경 변수는 top-level `env`에 직접 둘 수 있습니다. `tools.exec.env`도 계속 지원하며 같은 key가 있으면 더 구체적인 `tools.exec.env` 값이 우선합니다. 이 값은 exec 실행 환경과 subagent exec 실행 환경에 주입되고, MCP 서버별 환경 변수는 기존처럼 `tools.mcpServers.<name>.env`에 따로 둡니다. MCP `tools.mcpServers.<name>.enabledTools`는 기본값이 빈 배열인 default-deny opt-in입니다. MCP tools/resources/prompts를 노출하려면 `*`, raw capability name, 또는 `mcp_<server>_<kind>_<name>` 형태의 wrapped capability name을 명시하세요. MCP 도구가 결과에 `isError: true`를 붙여 스스로 실패를 알리면, 그 결과는 성공 내용이 아니라 `(MCP tool call failed: <도구가 보낸 글>)` 형식의 실패로 전달됩니다. 빈 문자열은 `requires.env`를 만족하지 않습니다. Secret 값을 넣은 config 파일은 커밋하거나 공유하지 마세요.

`permissions.mode`는 provider tool 실행 경로가 소비하는 permission policy gate 설정입니다. Rust runtime은 `default`, `plan`, `accept_edits`, `auto`, `dont_ask`, `bypass_permissions` 값을 파싱해 safe fallback/diagnostics를 만들고, provider tool call과 deferred bridge call을 실행 직전 permission mode snapshot과 policy decision에 통과시킵니다. `allow`만 즉시 tool 실행으로 이어지고, `deny`는 실행하지 않은 permission outcome으로 provider-visible tool result에 남습니다. `ask`는 channel runtime에서 사용자에게 승인 질문을 보내며, 사용자는 여섯 가지 decision을 답할 수 있습니다: `1`/`approve`는 pending tool call 한 번만 실행하고, `2`/`deny` 또는 `cancel`은 한 번 취소하며, `3`/`approve_session`은 현재 session에서 session key, permission context, requested scope가 같은 후속 action을 재사용 승인하고, `4`/`approve_project`는 현재 project의 `permissions.json`에 matching action rule을 저장하고, `5`/`deny_session`은 현재 session에서 matching action을 재사용 거절하고, `6`/`deny_project`는 현재 project의 `permissions.json`에 matching deny rule을 저장합니다. `exec`의 단순 shell 명령은 OpenCode식 arity prefix를 사용하므로 `cargo test --workspace`를 승인하면 `cargo test *`가 재사용 패턴으로 저장되고 같은 prefix의 후속 명령도 같은 rule에 매칭될 수 있습니다. Shell 제어 문자나 따옴표를 포함한 복합 명령은 기존 action digest exact match를 유지합니다. 파일 계열 tool은 workspace 상대 exact path 또는 subtree matcher, `web_fetch`는 origin matcher, MCP tool은 wrapped tool name matcher, 그 외 action은 exact action matcher로 요약됩니다. 승인 질문의 `Reusable pattern` 항목에서 실제 저장될 패턴 또는 `exact action` 요약을 확인할 수 있습니다. `ask_user`는 여전히 별도 user interruption tool이며 formal approval decision으로 해석되지 않습니다. `auto`는 user-local opt-in이나 명시적 실행 source 없이 workspace config만으로 활성화되지 않고, `bypass_permissions`는 격리 precondition이 충족된 명시적 opt-in일 때만 normalized snapshot에서 유지됩니다. `auto`에서는 public target을 다루는 built-in `web_search`/`web_fetch`, workspace 일반 파일 read/edit, `pwd`, `cargo fmt --check`를 별도 질문 없이 실행합니다. `cargo check/test/clippy/build`는 confirmed non-privileged containment 안에서만 자동 승인하며, 이미지 생성, 외부 메시지 전송, secret 접근, protected target, runtime 설정과 파괴적·network·secret exec는 계속 승인 또는 거절 경계를 유지합니다.

현재 project lifetime으로 저장된 remembered permission rule은 config 파일의 parent data directory에 있는 `permissions.json`에 저장됩니다. Rule은 canonical workspace id bucket별로 분리되며, 현재 workspace에 저장된 rule은 CLI에서 별도로 확인하거나 제거할 수 있습니다:

```sh
shacs-bot permissions list --workspace /tmp/ws
shacs-bot permissions inspect <rule-id-prefix> --workspace /tmp/ws
shacs-bot permissions revoke <rule-id-prefix> --workspace /tmp/ws
```

`permissions list`와 `permissions inspect`는 remembered permission store를 읽기만 하며, store 파일이 없으면 새 파일을 만들지 않습니다. 출력은 rule id prefix, effect, matcher 요약, timestamp/count 같은 redacted projection만 보여주고 workspace 절대 경로나 raw action payload를 출력하지 않습니다. `permissions revoke`는 현재 canonical workspace bucket에서만 동작하며, rule id prefix가 없거나 여러 rule에 동시에 매칭되면 store를 변경하지 않고 실패합니다. Store가 malformed, oversized, symlink, non-regular file이면 read surface는 raw store content나 secret-like text를 출력하지 않고 실패하며, runtime은 remembered rule을 신뢰하지 않고 fail closed합니다. Remembered allow는 protected target, static deny, permission ceiling, containment precondition을 우회하지 않습니다. 이 기능은 완전한 sandboxing, 완전한 redaction, prompt/tool/repo content 기반 permission grant, 모든 process start에 대한 universal gate를 보장하지 않습니다.

같은 read-only projection은 local API `GET /v1/permissions`와 TUI remembered permissions view에서도 사용됩니다. API endpoint는 mutation을 받지 않으며, TUI도 rule id/effect/matcher/store health 요약만 렌더링합니다.

현재 config, provider field, workflow recipe discovery 상태를 확인합니다:

```sh
shacs-bot status
shacs-bot status --config /tmp/shacs-config.json
```

`status`는 plain text를 출력합니다. JSON envelope를 출력하지 않고, config migration을 disk에 다시 쓰지도 않습니다. Workflow recipe count는 skill metadata에서 발견한 read-only recipe 후보를 요약하며, recipe가 권한 grant나 실행 코드를 추가했다는 뜻이 아닙니다.

Secret이나 session message 본문을 읽지 않고 로컬 runtime/workspace 상태를 확인합니다:

```sh
shacs-bot runtime inspect
shacs-bot runtime inspect --workspace /tmp/ws
shacs-bot runtime diagnostics --bundle /tmp/shacs-diagnostics.zip --workspace /tmp/ws
```

`runtime inspect`는 선택된 config, workspace, data directory, provider/model, provider 설정 여부, binary version, data schema compatibility classification, stored-data migration plan 요약, ownership status, stop request marker, update marker, runtime capability 요약, durable diagnostics evidence 요약, channel restart hint projection, containment contained/backend/snapshot digest, session 개수와 최신 session metadata, workflow recipe discovery count를 보고합니다. Durable diagnostics evidence는 redacted trace/log 보조 자료이며 event truth, replay 입력, writable admission 기준이 아닙니다. Channel restart projection은 cursor ref, pending inbound/outbound safe ref count, delivery status count만 표시하며 raw content나 session truth를 출력하지 않습니다. `runtime diagnostics` bundle에는 containment summary/digest와 durable diagnostics evidence가 redacted diagnostics field로 포함됩니다. 같은 readiness projection은 local API의 `GET /v1/readiness`와 `GET /v1/diagnostics`에서도 확인할 수 있습니다. Native host에서 Docker/Compose 같은 인식 가능한 containment evidence가 없으면 containment는 unknown으로 보고되며, sandboxed라고 주장하지 않습니다. `bwrap`는 공식 image/package에 포함되어 자동 설정된 경우가 아니라면 optional hardening입니다. `auth.json` token 값이나 raw session message는 노출하지 않으며, 장기 실행 cron/heartbeat worker를 시작하거나 실행 중인 것처럼 표시하지 않습니다.

Readiness의 `plugin_app` 구성 요소는 항상 필수입니다. 앱 기능이 [Spec 043](specs/043-app-capability-retirement/SPEC.md)에서 퇴역한 뒤로 이 구성 요소는 plugin 상태만 반영하며, 이름은 readiness 형식을 지키려고 그대로 둡니다. 실제 plugin 디렉터리의 discovery가 성공하고 비어 있으면 `ready`입니다. Plugin 디렉터리(data directory의 `plugins/`와 workspace의 `.shacs-bot/plugins/`)가 둘 다 없거나 조회에 실패하거나 configured plugin의 manifest가 없으면 빈 성공이 아니라 `unavailable`입니다. Plugin gate 차단은 `blocked`, plugin runtime descriptor 진단은 `degraded`로 표시합니다. 남은 앱 자료(registry, journal)는 읽지 않으므로 이 결과와 전체 readiness에 영향을 주지 않습니다. Runtime owner가 active라는 사실만으로 plugin을 정상으로 간주하지 않으며, 조회가 plugin을 실행하거나 자격 증명을 만들어 주지는 않습니다.

Readiness의 필수 `runtime_controls`와 `resource_disclosure`는 실제 Spec030 owner 관측을 사용하며, JSON의 `trusted_runtime`에 같은 profile·adapter·sandbox·resource·disclosure 상세를 포함합니다. CLI는 config의 API 주소로 실행 중인 owner를 조회하고 API는 자신의 local provider를 직접 읽습니다. Owner가 없으면 `unavailable`, 실행 관측 전 sandbox는 `unknown`이며, optional sandbox disabled/native fallback은 `degraded`, execution denied는 `blocked`입니다. 별도의 필수 containment 조건은 그대로 유지됩니다. Disclosure가 `ready`여도 `rawContentPossible`과 trace destination은 숨기지 않으며 완전한 redaction을 뜻하지 않습니다.

Workspace context file과 inline `@` reference가 어떻게 해석되는지 dry-run으로 확인합니다:

```sh
shacs-bot context files list --workspace /tmp/ws
shacs-bot context files inspect --workspace /tmp/ws
shacs-bot context refs parse "read @src/lib.rs and @diff"
shacs-bot context refs resolve --workspace /tmp/ws --message "read @src/lib.rs"
shacs-bot context refs resolve --workspace /tmp/ws --network --message "read @url:https://example.com"
```

`context files list/inspect`는 `AGENTS.md`, `CLAUDE.md`, `.cursorrules`, `.shacs.md`, `.shacs-bot.md` 같은 workspace context file 후보의 path, ordering, included/skipped/truncated/denied status, digest, byte/token estimate만 보여주며 raw file content는 출력하지 않습니다. `context refs parse`는 source를 읽지 않고 message 안의 token span, kind, normalized target, parse diagnostic만 표시합니다. 지원되는 reference syntax는 `@path`, `@folder/`, `@diff`, `@staged`, `@git:<rev>`, `@git:<rev>:<path>`, `@url:https://...`, `@https://...`입니다. `context refs resolve`는 read-only resolver, permission/redaction safety gate, shared context budget handoff를 통과한 status를 보여줍니다. URL reference resolution은 `--network`를 명시한 resolve 경로에서만 fetch됩니다. 파일/URL body와 provider context block content는 diagnostics에 저장하지 않고 digest와 redacted summary만 사용합니다.

Context limits는 bounded file bytes, folder entry limit, URL byte limit, provider handoff의 shared context budget으로 적용됩니다. Protected target(`.env`, SSH/private key path 등)은 content를 읽기 전에 denied evidence로 처리되고, URL reference는 명시적으로 network가 enabled된 resolve 경로가 아니면 skipped diagnostic이 됩니다. External URL content는 `external_untrusted` trust label로 표시되며 prompt-injection 방어상 instruction이 아니라 data로 취급됩니다. Secret-like content는 diagnostics와 provider handoff 전에 redaction pass를 통과하고, replay는 live URL fetch나 mutable git state 재실행 대신 recorded digest/excerpt/evidence를 사용합니다.

## Plugins and hooks

User-local plugin manifests can be inspected through the plugin management surface. The supported local-manifest scope is opt-in discovery, redaction-safe projection, bounded hook command dispatch, command-backed plugin tools through the normal tool registry, production-only plugin MCP startup, plugin-provided read-only skill roots, explicit plugin command router/dispatcher execution, and one behavior-affecting hook boundary: `tool:before` may return block-only output that is consumed as a normalized tool error immediately before tool execution.

```sh
shacs-bot plugins list
shacs-bot plugins inspect <name>
shacs-bot plugins doctor
shacs-bot plugins enable <name>
shacs-bot plugins disable <name>
shacs-bot hooks list
shacs-bot hooks inspect <plugin-or-hook>
```

`plugins list`, `plugins inspect`, `plugins doctor`, `hooks list`, and `hooks inspect` read config and discovered manifests only. They do not run plugin commands, dispatch hook callbacks, register provider-visible plugin tools, start plugin MCP servers, or execute plugin processes. `plugins enable` and `plugins disable` mutate only `plugins.enabled`/`plugins.disabled` in the selected config file and report next-session/reload semantics; they do not mutate the currently running session prompt or toolset.

During agent turns, enabled plugin hook entrypoints may run through the runtime hook adapter with no shell, cleared environment, bounded stdio, timeout/kill, JSON stdin, and redacted diagnostics. Observer hook output is recorded only as redacted evidence. `tool:before` block output is the only supported behavior-affecting hook output: the first valid block in deterministic plugin/hook order skips the affected tool call and returns a normalized tool error. Hook output that asks to approve, allow, or grant permissions is rejected and never creates permission approval. Enabled plugin tools are command-backed tools registered through the existing tool runtime; enabled plugin MCP declarations are added only to the production MCP startup path; enabled plugin skills are read-only skill roots consumed by the skill registry/context builder. Enabled plugin commands route through a separate plugin command router and safe dispatcher, not through builtin `CommandId`, and builtin command conflicts are excluded rather than overridden.

Currently supported manifests are `plugin.json` and `plugin.toml` files under the config data directory's `plugins/<name>/` root or the workspace-local `.shacs-bot/plugins/<name>/` root. Workspace-local plugins still require an explicit trusted workspace gate before they can become enabled. Output shows secret reference names and presence metadata only, never raw secret values.

공식 로컬 lifecycle 명령으로 foreground channel runtime을 시작하거나 실행 중인 owner에게 종료/재시작을 요청합니다:

```sh
shacs-bot runtime start --workspace /tmp/ws
shacs-bot runtime stop --workspace /tmp/ws
shacs-bot runtime restart --workspace /tmp/ws
```

`runtime start`는 기존 `run`과 같은 channel runtime foreground 경로를 lifecycle admission과 strict v1 ownership lease 획득 이후 실행합니다. Lease record는 owner generation, process evidence, acquired/renewed/expires time, lifecycle state와 lock protocol을 담습니다. 장기 실행 owner는 공유 runtime volume의 process lock을 보유하고 heartbeat를 갱신합니다. 비정상 종료 뒤 lock-aware heartbeat가 만료되면 다음 `runtime start`/`run`/`serve`가 prior owner lifecycle evidence를 기록하고 새 generation으로 인계하지만, fresh heartbeat는 PID namespace가 달라도 인계하지 않습니다. Lock protocol이 없는 legacy stale marker는 명시적 `runtime recover`가 필요합니다. `runtime stop`과 `runtime restart`는 data directory 아래 `runtime/stop-request.json`을 기록할 뿐 active ownership marker를 직접 삭제하거나 숨은 daemon을 시작하지 않습니다. 두 명령은 현재 active owner generation에 durable request를 연결하므로 owner가 바뀌면 요청이 차단됩니다. `runtime restart`는 안전 종료 의도이며, Rust CLI가 process를 in-place reexec하거나 process manager처럼 새 runtime을 자동 시작하지 않습니다. 실행 중인 owner는 stop/restart request를 관찰하면 장기 실행 loop를 종료하고 정상 종료 시 자신이 소유한 `runtime/ownership-marker.json`만 정리합니다.

AgentLoop가 확정한 turn/workflow/command fact, durable work lifecycle, subagent child lifecycle/decision, runtime control request, owner lifecycle, supervision record는 data directory의 `runtime/durable-events/events.log`에 append-only record로 보존됩니다. `runtime/durable-checkpoints/`의 checkpoint는 이 event truth를 대체하지 않는 replay 최적화이며, runtime은 checkpoint 다음 event tail을 deterministic reducer로 적용합니다. External inbound turn work의 redacted payload artifact는 content-addressed `runtime/work-payloads/`에 저장되고, pending/leased/waiting-retry/cancellation-requested/terminal state와 attempt/next-wake/lease expiry는 같은 event sequence로 복원됩니다. Child record는 parent session/turn/spawn effect correlation과 state, attempt, timestamps, opaque result ref만 보존하며 raw result를 inline하지 않습니다. `runtime/durable-diagnostics/diagnostics.log`는 committed event sequence에 연결된 redacted diagnostics evidence만 저장하고, 손상·누락·retention 상태가 event replay truth나 writable start admission을 바꾸지 않습니다. `runtime/supervision-state.json`은 현재 runtime owner, API/WebSocket/external processor/channel component state, shutdown queue/work/child/event/checkpoint/component outcome을 v1 projection으로 남깁니다. 이 projection은 raw owner id, process handle, host absolute path, raw payload를 그대로 노출하지 않습니다. `runtime inspect`는 event recovery, durable work recovery, durable child recovery, durable diagnostics evidence, owner lease, supervision evidence를 분리해 표시합니다. Running 중 crash한 child는 `recovery_needed`이며 성공으로 추정되지 않고, 명시적 `runtime recover`는 cancellation-requested와 cancelled outcome을 순서대로 기록합니다. 손상된 payload, stale work lease, active/stale owner lease가 있으면 writable start를 차단합니다. Stale owner는 `runtime recover`가 prior owner evidence를 durable owner lifecycle event로 먼저 남긴 뒤 정리합니다. PID가 살아 있는데 heartbeat만 만료된 live-expired owner는 suspect 상태라서 `runtime recover`가 차단됩니다. 이때는 먼저 `runtime stop`을 요청하거나 해당 process를 직접 종료하고, 다시 `runtime inspect`와 `runtime recover`를 실행하세요. `owner_lost`와 `failed_shutdown`은 자동 성공으로 바뀌지 않으며, inspect/recover의 supervision reason/phase와 bounded shutdown outcome을 보고 정리해야 합니다. 현재 local safety bound는 inline payload 16 KiB, work payload 1 MiB, open work 1,024개, retry 5회, terminal work/child projection 각 512개, runtime request projection 32개이며, payload store와 event log가 각각 512 MiB에 도달하면 새 enqueue를 받지 않습니다. 한도를 넘는 새 external inbound는 runtime 전체를 중단하지 않고 거부되며, 저장 공간을 정리하거나 후속 compaction/migration을 수행하기 전에는 새 durable enqueue가 제한될 수 있습니다. 이 파일들은 session JSONL이나 diagnostics와 분리된 내부 runtime truth substrate이므로 직접 편집하면 안 됩니다. Durable work queue는 현재 policy/session truth, channel cursor/outbound delivery truth, child result truth가 아니며 exactly-once execution이나 delivery를 보장하지 않습니다. Dispatcher는 실행 채택이나 policy verdict를 만들지 않고 work를 process-local bus로 넘기며, 실제 실행 시점의 permission/policy 재확인은 AgentLoop/orchestrator 경계가 수행합니다. External inbound turn은 session마다 enqueue된 순서대로 하나씩 넘기고, 앞선 inbound가 재시도를 기다리는 동안 같은 session의 뒤 inbound도 기다립니다. `/stop` 같은 priority 명령은 이 순서와 따로 바로 처리됩니다.

Stored-data migration은 자동 실행되지 않습니다. 변환이 필요하거나 `runtime/migration-ledger.json`이 partial 상태이면 `runtime start`, `run`, 일반 writable runtime load가 inspect-only로 차단됩니다. 먼저 dry-run으로 plan을 보고, 사용자가 명시적으로 apply 또는 resume을 실행합니다:

```sh
shacs-bot runtime migrate --dry-run --workspace /tmp/ws
shacs-bot runtime migrate --apply --workspace /tmp/ws
shacs-bot runtime migrate --resume --workspace /tmp/ws
```

Migration runner는 session metadata, event, checkpoint, queue, scheduler, channel, child, trace, diagnostics artifact family를 분리해 inventory합니다. Real run은 첫 mutation 전에 ledger를 쓰고 family별 no-op/transformed/failed/blocked 결과를 남기며, bounded backup은 complete verification 뒤 정리합니다. Dry-run은 source data와 marker를 만들거나 바꾸지 않습니다. 현재 config/profile file은 029 runner가 변환하지 않고 readable/incompatible compatibility 결과만 admission에 결합합니다. CLI projection은 opaque refs와 digest만 표시하고 raw secret, payload, absolute host path를 migration detail로 출력하지 않습니다.

Config/profile/auth-source declaration migration은 Spec 031의 별도 JSON-compatible command입니다. Dry-run/apply/recover는 env placeholder와 auth locator를 secret 값으로 writeback하지 않으며 TOML로 전환하지 않습니다:

```sh
shacs-bot runtime config-migrate --dry-run --config /tmp/config.json
shacs-bot runtime config-migrate --apply --config /tmp/config.json
shacs-bot runtime config-migrate --recover --config /tmp/config.json
```

Persisted snapshot과 activation record의 inspect 출력도 JSON입니다. Snapshot/replay는 diagnostic-only이고 current config/profile/auth/resource truth나 permission grant가 아닙니다. Activation inspect는 record를 변경하거나 entrypoint를 실행하지 않습니다:

```sh
shacs-bot runtime snapshot inspect /tmp/execution-snapshot.json
shacs-bot runtime activation inspect activation:skill:formatter:v1 --store /tmp/activations.json --owner workspace:sha256:owner
```

소스 checkout/Cargo 기반 설치에서 새 binary를 빌드하거나 교체한 뒤 runtime upgrade evidence를 기록합니다. Stored-data transform은 `runtime update`가 자동 실행하지 않고 위의 `runtime migrate` 명령으로 분리되며, 실제 binary 교체나 `git pull`/`cargo install`은 사용자가 별도로 수행합니다:

```sh
shacs-bot --version
shacs-bot runtime update --target-version <current-shacs-bot-version>
shacs-bot runtime inspect
```

`runtime update`는 config 옆 data directory 아래의 `runtime/update-marker.json`에 current/target version, schema version, migration 필요 여부, 완료 phase를 atomic temp-file + rename 방식으로 기록합니다. 소스 checkout 기반 workflow에서는 사용자가 이미 새 binary를 실행 중이라는 증거로 `--target-version`이 현재 `shacs-bot --version`과 일치해야 합니다. 기존 `in_progress` 또는 `partial_migration` marker가 있으면 새 update와 일반 runtime mutation을 막고 먼저 inspect/recover를 요구합니다.

중단된 update marker나 완료된 local update marker를 정리합니다:

```sh
shacs-bot runtime recover
shacs-bot runtime recover --workspace /tmp/ws
```

`runtime recover`는 손상된 최신 checkpoint 대신 이전 checkpoint 또는 event-from-zero replay가 검증된 경우 새 checkpoint를 기록하고, active runtime owner가 없을 때 일반 만료 work lease를 pending으로 requeue합니다. 이미 cancellation request가 기록된 stale lease는 다시 실행하지 않고 `Cancelled` outcome으로 확정합니다. Stale owner marker는 삭제 전에 owner lifecycle evidence를 durable event로 남깁니다. Live-expired owner는 살아 있는 process가 멈췄다는 증거가 없으므로 recovery 대상이 아닙니다. Event corruption, unknown schema, reducer violation, incomplete tail, missing/corrupt work payload처럼 자동 복구하지 않는 상태는 evidence와 safe action을 표시하고 mutation을 차단합니다. Durable recovery가 healthy인 경우 marker와 stale lease가 없으면 no-op으로 보고하며, partial migration marker와 active ownership도 세션 truth나 실행 중인 owner를 추측으로 고치지 않기 위해 차단합니다. `runtime stop`/`runtime restart`의 process marker와 durable request event는 task cancellation request/outcome과 별도 evidence입니다. Restart request는 safe-stop intent일 뿐이며, 자동 reexec, runtime worker restart/backoff, fleet/admin process control을 뜻하지 않습니다.

Gateway preset과 Web UI server도 구현되어 있습니다:

```sh
shacs-bot gateway --workspace /tmp/ws --port 8900 --verbose
shacs-bot web --workspace /tmp/ws --gateway-port 8900 --websocket-host 127.0.0.1 --websocket-port 8765 --verbose
```

`gateway`는 config, workspace, resolved gateway URL을 출력하는 preset/inspection command입니다. `web`은 local Web UI server를 foreground로 시작하고, 같은 local API/chat adapter와 WebSocket path를 사용합니다. `--verbose`는 raw prompt나 full payload가 아니라 input/response/tool/usage preview만 stderr에 출력합니다. input·response preview의 주소는 scheme 없이 쓴 것까지 scheme·host·path만 보입니다(사용자 정보·query·fragment 없음).

Rust programmatic facade(`ShacsBot`/`Nanobot`)는 lifecycle hook과 별도로 observability hook을 제공합니다. Observability hook은 provider stream event와 tool start/finish progress payload를 in-process callback으로 그대로 전달하므로, tool arguments나 provider delta에 민감한 내용이 포함될 수 있습니다. Hook 구현자는 필요한 경우 직접 redaction 후 저장/로그 처리해야 하며, hook panic은 runtime을 중단하지 않고 redacted event kind만 stderr에 남깁니다.

일반 runtime turn은 skill 사용 알림과 subagent 시작 알림을 기본으로 보냅니다. Always-on skill이 실제 system prompt에 적재되는 새 session에서는 활성 skill 알림을 한 번 보내고, 선택 skill의 `SKILL.md`를 성공적으로 참고하면 해당 skill 사용 알림을 보냅니다. Background subagent가 성공적으로 시작되면 시작 메시지를 별도 runtime 알림으로 보냅니다. 단순히 사용 가능한 skill 목록에 나타나는 catalog 정보는 사용 알림으로 보내지 않고, 일반 도구/MCP 호출은 별도 사용 알림으로 보내지 않습니다.

해결된 workspace의 로컬 session 파일 목록을 봅니다:

```sh
shacs-bot session list
shacs-bot session list --workspace /tmp/ws
```

Raw message 본문을 출력하지 않고 session 하나를 확인합니다:

```sh
shacs-bot session inspect --session cli:direct
shacs-bot session inspect --session cli:direct --workspace /tmp/ws
```

`session list`는 key, timestamp, file path를 보여줍니다. `session inspect`는 key, path, timestamp, message count, `last_consolidated`, metadata key 이름, `pending_user_turn` 또는 `runtime_checkpoint` 같은 recovery marker 이름을 보여줍니다. Runtime execution metadata가 있으면 provider/tool/subagent별 pending·terminal outcome 수, duplicate/late/stale decision 수, 안전한 artifact reference 수도 함께 표시합니다. 저장된 prompt/assistant content, raw outcome detail, correlation ID, raw metadata value는 출력하지 않습니다.

빈 session 파일을 만들거나, 필터링된 conversation history를 출력하거나, 로컬 diagnostics를 확인합니다:

```sh
shacs-bot session create --session cli:work
shacs-bot session history --session cli:work --max-messages 10
shacs-bot session history --session cli:work --json
shacs-bot session diagnostics --session cli:work
```

`session history`는 runtime과 같은 filtered replay view를 사용합니다. Consolidated message는 건너뛰고, orphan tool result는 복구하며, 기본 text 출력에서는 긴 user/assistant message를 잘라 보여줍니다. Raw session 파일이 아니라 필터링된 구조화 history가 필요할 때 `--json`을 사용하세요.

`session diagnostics`도 같은 redacted runtime execution projection을 사용하며 diagnostics reference와 supervision projection을 보존합니다. Human-readable CLI는 artifact 개수만 요약하고, 구조화 session/local API projection의 artifact locator는 workspace 상대 `.nanobot/tool-results/...` 경로만 포함하며 absolute path나 parent traversal 경로는 제외합니다. Supervision detail은 opaque ref로 표시되며 raw owner id, raw component detail, process handle은 출력하지 않습니다.

Raw 로컬 session content는 명시적으로 필요할 때만 export하세요:

```sh
shacs-bot session export --session cli:work --format json --yes
shacs-bot session export --session cli:work --format jsonl --yes
```

`session export`는 raw prompt, assistant message, metadata value, tool payload를 포함할 수 있습니다. 따라서 `--yes`/`-y` 확인이 필요하며 민감한 로컬 데이터로 취급해야 합니다.

로컬 session 파일 하나를 비우거나 compact합니다:

```sh
shacs-bot session clear --session cli:work --yes
shacs-bot session compact --session cli:work --keep-messages 8 --yes
```

`session clear`는 session metadata를 유지하면서 모든 message를 제거하고 `last_consolidated`를 reset합니다. `session compact`는 최근 legal suffix만 남기도록 JSONL 파일을 다시 씁니다. Provider 기반 요약이 아니라 로컬 destructive trim입니다.

로컬 session 파일 하나를 삭제합니다:

```sh
shacs-bot session delete --session cli:direct --yes
```

Session 삭제는 workspace의 `sessions/*.jsonl` 파일 하나를 disk에서 제거하며 되돌릴 수 없습니다. CLI는 명시적 확인으로 `--yes`/`-y`를 요구합니다. Session이 없으면 `Deleted: no`로 보고하고, 없는 `sessions/` directory를 만들지 않습니다.

## 스킬

해결된 workspace 기준으로 활성 로컬 skill registry entry를 나열합니다:

```sh
shacs-bot skills list
shacs-bot skills list --workspace /tmp/ws
```

Skill prompt 본문 전체를 로드하지 않고 하나의 skill을 확인합니다:

```sh
shacs-bot skills show skill-creator
shacs-bot skills show clawhub --workspace /tmp/ws
```

Skill-backed workflow recipe 후보를 검사합니다:

```sh
shacs-bot skills recipes
shacs-bot skills recipes --all --workspace /tmp/ws
```

`skills list`는 `onboard`가 `builtin_skills/`를 materialize하기 전에도 embedded active built-in skill을 포함합니다. Deferred built-in skill은 reference-only source로 보관되며 `onboard`, `skills list`, `skills show`에는 나오지 않습니다. Workspace skill은 built-in skill을 shadow할 수 있습니다. Shadowed, conflicted, malformed 같은 비활성 diagnostic까지 보려면 `skills list --all`을 사용하세요. `skills show`는 source, status, body hash, requirements, install metadata, diagnostics를 출력하고, 해당 skill이 workflow recipe metadata를 갖고 있으면 recipe summary도 함께 보여줍니다. `skills recipes`는 active and ready recipe만 기본 표시하고, malformed/conflicted source까지 보려면 `--all`을 사용합니다. Recipe metadata는 `workflow.recipe.id`, `workflow.recipe.pattern`, `workflow.recipe.prompt_template_ref` 같은 skill frontmatter 또는 `metadata.shacs.workflow_recipe` JSON에서 읽는 read-only harness input이며, permission grant, hidden tool access, plugin code 실행 권한을 만들지 않습니다. ClawHub search/install/update는 remote marketplace 범위이므로 현재 numbered open spec으로 이관하지 않았으며 지원 명령으로 취급하지 않습니다.

## 앱

앱 기능(`.shacsapp` authoring과 lifecycle)은 [Spec 039](specs/039-app-capability-disposition/SPEC.md#판정-결과-2026-10-24)의 판정에 따라 [Spec 043](specs/043-app-capability-retirement/SPEC.md)에서 퇴역했습니다. `shacs-bot apps …`와 별칭 `app`은 하위 명령(`init`, `propose`, `apply`, `install`, `list`, `inspect`, `enable`, `disable`, `start`, `stop`, `restart`, `recover`, `uninstall`)이나 인자와 관계없이 같은 퇴역 안내를 출력하고 종료 코드 1로 끝납니다. 이 안내는 아무것도 실행·설치·중단·삭제하지 않으며 파일·폴더·lock을 만들지 않습니다. 이전처럼 `apps`를 부르던 개인 스크립트는 이 종료 코드로 실패합니다.

```sh
shacs-bot --config ~/.shacs-bot/config.json apps
```

안내는 `--config`(없으면 기본 config)가 있는 폴더, 즉 data dir 안의 정해진 위치만 읽어 남은 앱 자료를 보여줍니다. config 파일이 없거나 읽을 수 없어도 같은 폴더를 읽기만 합니다.

| 위치 (`<data dir>` 기준) | 들어 있던 것 |
|---|---|
| `apps/registry.json` | 설치·활성화한 앱 목록 |
| `apps/<app-id>.shacsapp/` | 설치된 bundle과 그 안에 직접 넣은 파일 |
| `apps/app-supervisor/<app-id>/` | 실행 기록(journal: `state.json`, `receipts/`, `lifecycle.lock`) |
| `authoring/apps/draft-<app-id>/` | 설치 전 초안 |
| `authoring/flow/` | propose·apply 기록 |
| `snapshots/app-lifecycle/` | start 시도마다 남은 실행 snapshot |
| `runtime/app-ledger/` | 017 app ledger(제품 명령이 쓰지 않으며 남아 있을 때만 표시) |

앱마다 registry 상태, 마지막 journal 상태, bundle 유무를 보여줍니다. 마지막 journal 상태가 `starting`·`running`·`stopping`·`recovery_needed`이거나 읽을 수 없는 앱은 process가 아직 실행 중일 수 있으며, shacs-bot은 그것을 확인하거나 멈추지 않았다고 표시합니다. `stopped`·`failed`도 마지막 기록일 뿐 지금 실행 중이 아니라는 보증은 아닙니다. 예전 버전에서 앱 owner를 Ctrl+C로 끝냈다면 앱 process가 별도 process group에서 계속 실행 중일 수 있습니다. 형식이 어긋난 파일은 내용을 보이지 않고 읽을 수 없다는 사실만 보이며, registry에 적힌 경로나 바로가기(symlink)는 따라가지 않습니다. bundle 안 파일 내용과 registry의 비밀·권한 요청도 보이지 않습니다. `snapshots/activation-records.json`은 031 activation 기록이라 앱 자료로 나열하지 않지만 그대로 둡니다.

남은 자료는 자동으로 옮기거나 지우지 않습니다. 필요 없으면 직접 지우세요. 지우기 전에 실행 중일 수 있다고 표시된 앱의 process가 남아 있는지 확인하고(예: `ps -ax | grep <manifest의 runtime command>`), 남아 있으면 직접 멈추세요. bundle 폴더 안에 직접 넣은 파일이 있다면 먼저 옮겨 두세요. readiness, Tasks, agent 실행 기록은 남은 앱 자료를 읽지 않으므로 지우든 두든 결과가 같습니다.

절차나 지식을 어시스턴트에 주려면 앱 대신 사용자 skill(`<data dir>/skills/<name>/SKILL.md`)을 두세요. 도구 노출은 plugin command tool이나 MCP 설정, 반복 확인은 [명시적으로 맡긴 작업](#명시적으로-맡긴-작업), 명령 실행은 셸이나 승인 경계가 있는 `exec` 도구를 쓰세요.

## 일회성 CLI 에이전트

로컬 `AgentLoop`에 메시지 하나를 보냅니다:

```sh
shacs-bot ask "hello" --workspace /tmp/ws
```

기존 nanobot 호환 direct 형식도 지원합니다:

```sh
shacs-bot agent -m "hello" --workspace /tmp/ws
shacs-bot agent --message "hello" --session work --workspace /tmp/ws
```

`ask`와 `agent -m/--message`는 같은 direct execution path를 사용합니다. Config를 로드하고, 설정된 provider/model을 resolve하고, `AgentLoop`를 만든 뒤 user turn 하나를 실행하고 assistant text를 stdout에 출력합니다.

Message 없이 `shacs-bot agent`를 실행하면 같은 config/session boundary를 사용하는 interactive REPL이 시작됩니다. 일반 입력은 기존 `AgentLoop` session turn 경계로 실행되고, active turn 중 들어온 일반 입력이나 non-priority slash command는 current session의 pending follow-up으로 직렬화됩니다. `/status`, `/stop`, `/restart`는 command router의 priority decision을 그대로 사용하므로 active turn 중에도 먼저 처리됩니다. EOF는 active turn이 없으면 종료하고, active turn이 있으면 `/stop`을 요청한 뒤 종료합니다. Ctrl-C는 active turn에 `/stop`을 요청하고, 반복 Ctrl-C는 deterministic exit 경로로 들어갑니다. 출력은 canonical projection vocabulary로 요약되며 raw session id, raw provider payload, raw tool payload를 출력하지 않습니다.

TUI는 별도 binary입니다. `--once`는 live projection을 한 번 읽어 plain text로 출력하고, interactive mode는 session selection, pending approval action, degraded readiness, stop/restart/recover action을 표시합니다. Fresh workspace에서는 session store와 표시할 session이 먼저 필요하므로, workspace template을 준비하고 session을 만든 뒤 실행합니다:

```sh
shacs-bot --config /tmp/shacs-data/config.json onboard --workspace /tmp/ws
shacs-bot --config /tmp/shacs-data/config.json session create --session cli:direct --workspace /tmp/ws
shacs-tui --config /tmp/shacs-data/config.json --workspace /tmp/ws --session cli:direct --once
shacs-tui --config /tmp/shacs-data/config.json --workspace /tmp/ws --session cli:direct
```

Approval action은 durable request가 기록되면 `Requested`로 남고, owner terminal event가 관찰되기 전에는 완료로 표시되지 않습니다. TUI와 REPL은 raw provider payload, raw tool payload, raw owner id를 표시하지 않습니다.

Formal permission approval의 pending 항목이 처리되어 제거되면 기존 session owner는 `permission_approval_receipts` v1 metadata에 최근 종료 이력 최대 32개를 보존합니다. `session inspect`, `GET /v1/sessions/{session_id}`의 `permission_approval_receipts`, TUI 새로고침은 같은 request ID/action digest/snapshot digest와 `consumed`, `denied`, `expired`, `rejected` 상태를 읽습니다. TUI는 최신 종료 이력을 표시하며 승인 키를 다시 활성화하지 않습니다. `consumed`는 상관관계가 검증된 승인 시도가 소진되었다는 기록이지 도구 성공, 현재 permission allow, remembered rule, exactly-once 실행 보장이 아닙니다. IPC work의 `Succeeded`는 요청 처리 결과일 뿐 이 permission receipt를 대신하지 않습니다. Raw tool 인자·secret은 receipt에 저장하지 않고, 오래된 이력은 최신 32개에서 밀려나며 알 수 없는 schema/잘못된 session binding은 조회에서 제외됩니다. Reload는 receipt를 pending이나 재사용 승인으로 복원하지 않습니다. 기존 process-local recent retry의 일회성·재시작 제한은 바뀌지 않습니다. 만료된 surface 요청의 기존 사전 거절 경로는 pending을 그대로 두며, 실제로 pending을 종료하는 owner 경로만 종료 receipt를 남깁니다.

복잡도가 높거나 병렬 검증, 큰 context 분할이 필요한 요청은 runtime이 deterministic read-only workflow admission을 통해 dynamic workflow로 실행할 수 있습니다. 명시적으로 제공된 typed write-capable workflow plan은 별도 승인과 isolated git worktree 정책을 통과해야 실행됩니다. Workflow는 typed harness plan, child/verifier execution, budget/checkpoint, verifier gate, and sanitized runtime metadata를 남깁니다. Write-capable workflow child는 승인된 isolated git worktree에서만 실행되고 parent checkout에는 자동 merge하지 않습니다. 결과에는 diff evidence와 parent-review merge handoff가 남으며, 사용자가 검토 후 별도로 적용해야 합니다. `/stop` 또는 runtime stop이 관찰되면 workflow parent cancellation token이 child/verifier execution까지 전달되고, cancelled workflow는 success로 표시되지 않습니다.

Workflow 상태는 raw prompt나 secret 없이 session inspect/diagnostics, local API session detail/diagnostics, channel runtime projection, and TUI projection consumer에서 같은 bounded vocabulary로 표시됩니다. CLI에서는 session 명령의 metadata/runtime workflow section에서 workflow id, state, progress count, verifier status, blocked reason, resume availability를 확인할 수 있습니다. API는 `/v1/sessions/{session}`와 `/v1/sessions/{session}/diagnostics`에 sanitized `runtime_workflow` field를 포함합니다.

Provider 호출 전에 built-in slash command는 로컬에서 처리됩니다:

- `/status`: 현재 loop/session에 active task가 있는지 보고합니다.
- `/new`: 현재 session을 비우고 새로 시작합니다.
- `/stop`: 등록된 active task에 cancellation을 요청합니다.
- `/restart`: 로컬 restart 요청을 acknowledge합니다. Rust CLI는 현재 process를 in-place로 교체하지 않고, runtime lifecycle의 `runtime restart`도 안전 종료 의도만 남깁니다. 새 process 시작은 다음 명시적 start나 외부 OS supervisor의 책임입니다.
- `/goal [status|pause|resume|clear|done|blocked <reason>|<text>]`: 현재 session의 persistent goal metadata를 설정하거나 상태를 바꿉니다. 새 목표를 설정하려면 기존 active goal을 먼저 `/goal clear`로 정리해야 합니다.
- `/permission`: `permissions.mode`를 `default`, `auto`, `bypass_permissions` 중 하나로 저장하는 대화형 wizard를 시작합니다. 현재 `auto`를 선택하면 `permissions.mode: "auto"`가 저장되고, 런타임은 이를 auto approval opt-in으로 해석해 정적 안전 규칙과 local capability allowlist를 통과한 낮은 위험 action을 먼저 자동 승인합니다. `permissions.autoApproval`은 protected target과 exec containment 같은 세부 옵션을 조정하는 블록입니다. 이 local fast path로 해결되지 않은 direct tool action과 resolved deferred bridge tool action 중 current user message로 scope 판단이 가능하고 classifier capability ceiling을 통과한 action은 같은 provider/model을 사용하는 auto-mode classifier 평가를 거쳐 high-confidence requested-scope allow일 때만 실행됩니다. `proc_exec`가 command summary unavailable 또는 containment unknown으로 `ask`가 된 경우는 classifier allow 대상이 아니며 approval prompt로 남습니다. Classifier 오류, 낮은 confidence, scope 불일치, parse failure, user scope 부재, classifier ceiling 밖 capability는 interactive session에서는 permission prompt로, non-interactive 경로에서는 deny로 접힙니다. `bypass_permissions`는 먼저 선택한 뒤 정확히 `confirm bypass_permissions`로 한 번 더 확인해야 저장되며, 저장된 값은 이후 turn의 permission snapshot에 반영됩니다. `cancel`은 진행 중인 wizard를 취소합니다.
- `/permission recent`: 최근 auto-mode classifier denial을 sanitized summary로 보여줍니다. `/permission recent retry <denial_id>`는 interactive channel에서 같은 denied action을 한 번 실행하기 위한 formal approval을 만들며, raw payload를 session metadata에 저장하지 않습니다. 같은 owner가 일회성 token을 소비하고 승인 상관관계를 검증한 뒤 executor에 전달한 경우, 기존 terminal receipt에 `recent_retry_<denial_id>`의 `consumed` 사실을 별도로 남깁니다. 원래 approval의 `denied` 이력은 유지되며, TUI는 최신 retry lineage를 표시합니다. 이 기록은 도구 성공이나 재사용 권한이 아니고 token을 저장·복원하지 않습니다. TUI의 recent retry 승인 버튼은 계속 unavailable이며 원래 session channel에서 응답해야 합니다.
- `/permission rules`: 현재 project remembered permission rule을 list합니다. `/permission inspect <rule-id-prefix>`와 `/permission revoke <rule-id-prefix>`는 CLI `permissions inspect`/`permissions revoke`와 같은 current workspace bucket projection/revoke semantics를 사용합니다.
- `/history [n]`: 최근 visible user/assistant message를 보여줍니다. 기본값은 10, 최대값은 50입니다.
- `/dream`: 설정된 Dream memory consolidation을 한 번 실행합니다.
- `/dream-log [sha]`: 최신 memory commit diff 또는 선택한 commit diff를 보여줍니다.
- `/dream-restore [sha]`: 복원 가능한 memory version 목록을 보여주거나 선택한 commit 이전 상태로 tracked memory file을 되돌립니다.
- `/help`: slash command 목록을 보여줍니다.

Command router는 priority, exact, prefix 경계를 분리합니다. `/status`, `/stop`, `/restart`는 priority command라서 같은 session의 active turn이 있어도 provider 호출 전에 처리됩니다. `/new`, `/permission`, `/help`, `/dream` 같은 exact command와 `/history 25`, `/dream-log <sha>`, `/dream-restore <sha>` 같은 prefix command는 일반 user turn과 같은 process-local session turn lock을 공유합니다. Prefix로 등록되지 않은 command는 정확히 일치할 때만 command로 처리됩니다. 예를 들어 `/status now`는 `/status` command가 아니라 일반 user message로 처리됩니다.

`ask` message가 `-`로 시작하면 option과 구분하기 위해 `--`를 사용하세요. 예: `shacs-bot ask -- "-starts-with-dash"`.

지원되는 direct-message option:

- `--config <path>` / `-c <path>`
- `--workspace <path>` / `-w <path>`
- `--message <text>` / `-m <text>`: `agent` direct-message alias에서 message를 전달합니다. `ask`는 positional message도 받습니다.
- `--session <id>` / `-s <id>`: 기본값은 `cli:direct`입니다. `:`가 없는 값은 `cli:<id>`로 저장됩니다.
- `--temperature <number>`
- `--max-tokens <positive integer>`
- `--no-side-effects`: 이 로컬 CLI turn에서 write/edit/exec/message/image tool과 plugin tool을 등록하지 않습니다. `ask`/`agent`/`run`은 기본으로 이 tool들을 등록하며, 등록된 tool의 실행 승인은 계속 permission mode가 결정합니다.
- `--allow-side-effects`: 기본 동작과 같으며 호환을 위해 받습니다. `run --allow-remote`는 side-effect tool을 기본으로 끄므로 이 flag를 명시해야 등록됩니다. `serve`의 `--allow-api-side-effects`와 channel `runtime start`는 기존처럼 opt-in입니다.
- `--markdown` / `--no-markdown`: nanobot CLI 호환을 위해 받지만, 현재 Rust binary는 plain stdout text를 출력합니다.

Message가 `-`로 시작하는 `agent` direct message는 `--message`/`-m`로 전달하세요. `agent hello` 같은 positional direct message는 REPL 경로와 구분하기 위해 거절됩니다.

## 요청별 수행 방식과 `/method`

`agent` REPL과 같은 `AgentLoop`를 사용하는 대화 표면에서 사용자 요청마다 수행 방식을 선택합니다. 한국어·영어의 명확한 요청 표현을 보수적으로 분류하며, 분류 자체는 provider·도구·credential command를 호출하지 않습니다. 모호하면 `general`입니다. 일반 자동 선택은 기존 답변 형식을 유지하고, 전문 방식이나 사용자 지정 방식에는 런타임이 선택 이유·가용성·예정 확인 항목을 footer로 덧붙입니다. Footer는 conversation history에 저장하지 않습니다.

| 별칭 | Canonical ID | 필요한 실제 등록 도구 | 예정 확인 항목 |
|---|---|---|---|
| `general` | `general` | 없음 | 없음 |
| `calculation` | `reproducible_calculation` | `exec` | 입력, 단위, 가정, 독립 확인 |
| `report` | `source_grounded_report` | `read_file` | 실제 읽은 출처, 주장–출처 연결, 미확인 주장 |
| `image` | `image_generation_review` | `image_generate` | 생성 artifact, 명시한 요구 조건, 사용자 수정 |
| (자동 선택만) | `composed` | 각 부분의 도구(하나 이상) | 각 부분의 확인 항목(부분 이름이 붙음) |

**방식 선택 자체는 전문 검증을 하지 않습니다.** 선택 직후 모든 확인 항목은 `not verified`이며, 각 방식의 판정은 아래 계산·보고서·이미지·복합 절의 실제 실행 기록으로만 바뀝니다. Tool 성공이나 사용자 승인만으로 계산·출처·이미지 품질의 검증 성공을 주장하지 않습니다. 이미지 도구가 등록되어도 credential과 provider 지원 여부는 실제 호출 시 확인하므로 `requires_runtime_check`입니다. 보고서는 로컬 `read_file`만 있어도 선택할 수 있고, `web_search`/`web_fetch`가 없으면 `web research unavailable`을 표시합니다. Skill의 요구사항 문자열이나 ready workflow recipe는 도구 가용성의 증거로 사용하지 않습니다.

```text
/method
/method status
/method list
/method use general
/method use calculation
/method use report
/method use image
/method auto
/method history
/method accept
/method revise
```

`use`는 표의 canonical ID도 받습니다. 명령 자체는 provider turn을 시작하지 않으며 non-priority builtin입니다. 실행 중인 provider를 끼어들어 바꾸는 명령이 아니라 기존 turn 직렬화 경계를 따릅니다.

- **활성 요청이 없을 때:** `use`는 다음 사용자 요청 **한 번만** 적용합니다. 다음 요청이 차단되어도 이 지정은 소비됩니다. `/method auto`로 아직 소비하지 않은 지정을 지울 수 있습니다.
- **질문·승인·workflow checkpoint 대기, 변경 후 이어가기 대기, 도구 부족으로 차단된 요청이 있을 때:** `use`는 같은 요청에 새 selection revision을 만듭니다. 같은 방식은 상태를 건드리지 않는 no-op입니다. `auto`는 원래 목표를 자동으로 다시 선택하는 새 revision입니다. 원문이 이미 conversation compaction으로 제거됐다면 기록된 원래 자동 분류를 사용합니다.
- **변경 후:** 다음 일반 메시지는 같은 요청 ID를 이어가며 재분류하지 않습니다. 목표 preview, 관측한 산출물, 답한 질문과 답변, 남은 질문, 외부 효과 기록을 유지합니다. 이전 질문에는 취소 tool result를 추가하고, 보류 승인·retry token·Spec036 행동 결정과 보류 workflow는 취소합니다. 이전 validation claim은 `superseded_not_inherited`이며 새 revision에는 승계하지 않습니다. 다만 새 방식에도 남는 계산·보고서·이미지 부분은 이미 기록된 실행·읽은 자료·저장된 이미지를 새 revision에서 다시 판정합니다(아래 "복합 요청의 결과 확인").
- **승인 정책:** 변경 전의 보류 승인은 새 행동에 재사용하지 않습니다. Session/project remembered permission rule 자체는 삭제하지 않으며, 새 행동은 현재 정상 permission gate를 통과합니다. 따라서 유지된 정책에 따라 승인되거나 다시 승인을 요구할 수 있습니다.
- **완료 후:** 다음 일반 메시지는 새 요청입니다. 차단된 요청도 방식 변경 없이 새 일반 메시지를 보내면 abandoned로 닫고 새 요청을 시작합니다. 차단된 목표를 이어가려면 먼저 `/method use general` 같은 변경 명령을 쓰세요.
- **`/stop`과 `/new`:** 방식 변경이 아니므로 위의 변경 처리(질문·승인·workflow 취소)를 하지 않습니다. `/stop`은 보류 중인 질문·승인·workflow가 있으면 요청을 그대로 두어 이어서 답하거나 승인할 수 있게 하고, 보류 항목 없이 이어가기 대기이거나 차단된 요청만 cancelled로 닫습니다. 실행 중인 turn에 보낸 `/stop`은 그 turn이 취소로 끝나면 요청을 cancelled로 닫습니다. `/new`는 새 session을 시작하면서 활성 요청을 cancelled로 닫습니다.

필요한 도구가 없으면 provider/workflow 실행 전에 차단합니다. 둘 이상의 전문 방식을 함께 요청하면 복합 방식(`composed`)으로 처리하며, 모든 부분을 쓸 수 없을 때만 차단합니다(아래 "복합 요청의 결과 확인"). 조용히 일반 방식으로 fallback하지 않습니다.

로컬 `ask`/`agent`/`run`은 side-effect tool을 기본으로 등록하므로 `tools.exec.enable`(기본값 `true`)이 켜져 있으면 계산 방식을 쓸 수 있습니다. `--no-side-effects`로 실행했거나 `run --allow-remote`를 `--allow-side-effects` 없이 실행했으면 `exec`가 등록되지 않아 계산 요청이 차단됩니다. 이미지에는 `tools.imageGeneration.enable`과 지원되는 image provider 설정도 필요합니다. 설정을 바꿔 재시작한 뒤 기존 차단 요청은 `/method auto`로 다시 평가할 수 있습니다. 단순히 같은 `use`를 반복하면 no-op입니다. 대신 `/method use general` 후 `continue`처럼 이어갈 수도 있습니다.

이미 실행한 파일 쓰기·외부 전송 등을 자동으로 되돌리거나 재실행하지 않습니다. `/method status`와 재시작 후 `/method history`에서도 관측한 외부 효과를 `not reverted`로 표시합니다. 기록에는 tool 이름·결과·canonical argument SHA-256·revision·turn·시각만 남기며 raw argument/result는 method metadata에 저장하지 않습니다. 단, 계산 방식 요청의 `exec` 실행은 재현과 확인을 위해 아래 계산 절의 범위로 명령과 출력 일부를 저장합니다. 이는 실제 원격 효과를 독립적으로 검증했다는 뜻은 아닙니다.

### 계산 방식의 결과 확인

계산 방식(`reproducible_calculation`)은 답을 실제로 실행한 코드와 별도 확인으로 판정합니다.

- **실행 기록:** 모델은 `exec`로 계산 코드(예: `python3 -c ...`)를 실행합니다. 각 실행은 결과 한 줄 `SHACS_CALC {json}`을 출력합니다.
  - 주 계산은 `role: "primary"`와 함께 `answer`, `unit`, `inputs`, `assumptions`를 출력합니다.
  - 확인 계산은 다른 코드·방법으로 `role: "check"`와 `answer`, `unit`, `method`를 출력합니다.
  - `answer`는 수여야 합니다(예: `6`, `6.0`, `-1.5e3`, `6,000`, `1/3`). `unit`은 공백과 대괄호가 없는 24자 이하의 한 낱말입니다(예: `m^2`, `km/h`, 빈 값). 그렇지 않은 결과 줄은 "result line problem"으로 기록하고 결과로 쓰지 않습니다. runtime이 확인된 값과 단위를 직접 화면에 적기 때문입니다.
  - 기록하는 글(입력, 가정, 방법 설명, 오류, 위치 등)의 줄바꿈과 제어 문자는 공백으로 바꿔 한 줄로 저장합니다. 실행 코드와 출력만 줄을 그대로 남깁니다.
- **판정은 runtime이 합니다.** 모델이 "검증됐다"고 말해도 판정에는 영향이 없습니다.
  - `verified`: 마지막 주 계산 뒤에 성공한 다른 코드의 확인 계산이 같은 값과 단위를 냈을 때만입니다.
  - `conflict`: 확인 계산의 값이나 단위가 다를 때입니다. 예를 들어 단위를 잘못 바꿔 600과 6이 나온 경우입니다.
  - `not verified`: 확인 계산이 없거나, 실패했거나, 주 계산과 같은 코드일 때입니다.
  - `failed`: 주 계산이 0이 아닌 종료 코드로 끝났거나 오류가 났을 때입니다.
  - `unavailable`: 실행이 permission·process gate에 막혔거나, Python 또는 필요한 package가 없을 때입니다.
  - `not recorded`: 결과 줄이 없을 때입니다.
  - `not run`: 실행이 아예 없을 때입니다.
  - 종료 코드 0만으로는 `verified`가 되지 않습니다.
  - 독립성은 확인 계산의 코드가 주 계산과 다르다는 것까지만 기계적으로 확인합니다. 실제로 다른 방법인지는 `/method status`의 코드와 `method` 설명으로 직접 확인해야 합니다.
- **재실행:** 오류를 고쳐 주 계산을 다시 실행하면 그 이전의 확인은 `superseded`로 표시되고 판정에 쓰이지 않습니다.
  - 마지막 주 계산 뒤의 실행이 하나라도 실패하거나 취소되면 이미 `verified`였던 판정도 거둬들입니다. 거부·Python 없음·package 없음이면 `unavailable`, 그 밖의 실패면 `not verified`가 됩니다. 실행 전에 취소된 실행(다른 호출의 승인 대기로 건너뛴 실행, 방식 변경으로 취소된 보류 실행)도 `not verified`이며 이유는 "a run after the latest primary result was cancelled"입니다. 그 뒤 확인 계산만 다시 성공해도 판정은 돌아오지 않습니다.
  - 마지막 주 계산 뒤에 결과 줄이 없거나 잘못된 실행이 있어도 판정을 거둬들여 `not verified`가 됩니다.
  - 주 계산과 확인 계산을 모두 다시 성공시켜야 다시 `verified`가 됩니다.
  - 다른 hook이 `exec`를 막은 경우와 사용자가 `exec` 승인을 거절한 경우도 거부된 실행으로 기록합니다. 실패하거나 거부된 실행의 출력에 결과 줄이 섞여 있어도 결과로 쓰지 않습니다.
  - 방식을 바꾸면 이전 revision의 계산 판정은 `superseded_not_inherited`가 됩니다. 새 방식에도 계산 부분이 남으면(예: 복합 요청에서 계산만 남김) 기록된 실행을 새 revision에서 다시 판정합니다. 방식 변경으로 취소된 보류 실행도 그 기록에 취소로 남습니다.
  - 각 실행은 요청 안의 순번(`#1`, `#2` …)으로 구분하므로 tool call ID가 겹쳐도 판정에 쓰인 실행을 정확히 가리킵니다.
- **결과 표시:** 답 다음에 판정과 핵심 근거를 보여 줍니다. 근거는 확인된 답 또는 확인되지 않은 주 계산 결과, 입력, 가정, 실행 상태입니다. `/method status`에서는 각 실행의 명령·출력·종료 상태·결과 줄을 볼 수 있습니다.
- **`/method accept`:** 현재 또는 마지막 요청의 결과를 사용자가 받아들였다는 사실을 따로 기록합니다. 판정은 바꾸지 않습니다. 사용자 아닌 turn에서는 거절합니다.
- **저장 범위:** 계산 실행 기록은 redaction 후 명령 2,048자, 출력 1,024자까지 저장합니다.
  - 활성 요청에는 최근 16개를 보관합니다.
  - 요청이 끝나면 판정에 쓰인 실행과 최근 4개만 남기고, 생략한 수를 표시합니다.
- **실행 환경:** 실제 실행은 기존 permission 승인과 Spec030 process gate를 그대로 거칩니다.
  - 공식 container처럼 containment가 확인된 환경에서는 승인 뒤 실행됩니다.
  - 이를 확인할 수 없는 native host에서는 승인 뒤에도 gate가 실행을 막을 수 있습니다. 그때 계산은 `unavailable`로 표시되며 결과를 합성하지 않습니다.

### 보고서 방식의 출처 확인

보고서 방식(`source_grounded_report`)은 결론이 이 요청에서 실제로 읽은 자료에 연결되는지 확인합니다. 결론이 참인지 보증하지는 않습니다.

- **읽은 자료 기록:** `read_file`과 `web_fetch`의 읽기 결과를 같은 요청에 기록합니다. 기록하는 항목은 위치, 읽은 시각, 내용 digest, 실패 사유입니다.
  - 작업 폴더 안의 파일은 읽을 때의 파일 digest도 남깁니다. 그래서 나중에 파일이 바뀌었는지 다시 확인할 수 있습니다.
  - 다시 확인할 때 읽을 때 쓴 경로가 더 이상 같은 실제 파일을 가리키지 않으면(예: symlink가 다른 파일을 가리키게 바뀜) 따라가 읽지 않고 "바뀐 파일"로 봅니다.
  - hook에 막히거나, 사용자가 거부했거나, 실행 전에 거절·취소된 읽기도 실패한 자료로 남깁니다. hook이 어떤 문구로 막았는지와 관계없이 실패로 기록합니다.
  - 준비 단계에서 거절되거나 실행 전에 취소된 읽기도 요청한 경로·URL로 기록합니다. 인자를 알 수 없을 때만 `<read call rejected/skipped before it ran>`으로 남깁니다.
  - `web_fetch`가 URL 검사 실패, 네트워크 오류, HTTP 오류처럼 `{"error": ...}` 결과를 돌려주면 읽기 실패입니다. 그 사유는 "Sources that could not be read"에 표시합니다.
- **인용 형식:** 모델은 결론 줄 끝에 `[source: <읽은 경로나 URL>]`을 붙입니다.
  - 인용은 읽을 때 쓴 경로·URL과 정확히 같아야 연결됩니다. 앞의 `./`, 따옴표, 끝의 `/`만 무시합니다.
  - 작업 폴더 파일은 같은 파일을 가리키는 작업 폴더 기준 상대 경로나 실제 절대 경로로 인용해도 됩니다. 파일 이름만 같은 다른 경로(예: `notes.md`를 읽고 `imaginary/notes.md`나 `docs/notes.md`로 인용)는 "never read"입니다.
  - 긴 경로·URL은 화면과 기록에서 256자로 줄이지만, 연결 여부는 줄이기 전 전체 값으로 비교합니다.
  - 여러 자료는 `[source: a; b]`로 씁니다. 구분자는 `;`뿐이며, 경로나 URL 안의 쉼표는 그대로 위치의 일부입니다.
  - 해결되지 않은 점은 `[unresolved: ...]`, 서로 다른 자료는 `[conflict: a; b]`로 표시합니다.
  - 답변 본문과 `write_file`로 쓴 보고서 파일 내용을 모두 검사합니다. `edit_file`로 고친 파일은 고친 뒤 디스크의 내용을 다시 읽어 검사합니다.
- **판정:**
  - `source-linked`: 모든 인용이 이 보고서를 위해 읽은 자료에 연결되고, 인용 표시 없는 문장 줄이 하나도 없을 때만입니다.
  - `citation problems`: 인용 중 하나라도 다음 중 하나일 때입니다. 읽지 않은 자료, 읽기 실패, 읽은 뒤 바뀐 파일, 이전 판에서 가져와 다시 확인하지 못한 웹 자료.
  - `partly cited`: 인용은 모두 연결됐지만 표시가 없는 문장 줄이 있을 때입니다. 이때 "claim-source linkage" 확인은 `not verified`로 남습니다.
  - 그 밖의 상태: `no citations`(인용 없음), `no sources read`(읽은 자료 없음), `unavailable`(보고서 글이 있고 모든 읽기가 실패), `not written`(보고서 글 없음). 글이 없으면 읽기 결과와 관계없이 먼저 `not written`입니다.
  - 표시만 있고 문장이 없는 줄(예: `[source: notes.md]`만 있는 줄)은 결론이 아닙니다. 빈 표시(`[source: ]`)만 있는 문장은 표시가 없는 줄로 셉니다.
  - 표시 없는 문장 줄은 "not source-checked"로 수와 처음 5개를 보여 주고, `/method status`에서 전체(최대 32개)를 보여 줍니다. 짧은 문장과 한국어 문장도 모두 셉니다. 세지 않는 줄은 제목(`#`), `:`로 끝나는 머리말, 구분선·표 구분 줄·코드 펜스 같은 서식 줄, 파일 저장 안내 `Saved: <파일>` 한 줄뿐입니다. 이런 줄은 출처 표시가 있어도 보고서 내용으로 세지 않습니다(그것만 있으면 `not written`). 다만 그 인용은 검사하므로 읽지 않은 자료를 가리키면 `citation problems`가 됩니다.
  - 검사 한도를 넘은 부분(출력당 결론 32개·판정당 64개를 넘는 결론, 결론당 32개를 넘는 인용, 출력당 65,536자를 넘는 본문, 판당 32개를 넘어 밀려난 출력)은 "Parts beyond the checking limits (not checked)"로 세며, 이 경우에도 `source-linked`가 아닙니다.
  - 검사 대상 답변은 모델이 정상으로 끝낸 최종 답변뿐입니다. 도구 오류로 멈춘 turn, 취소, 반복 한도 도달, 빈 응답, 질문·승인 대기에서 runtime이 만든 안내문은 보고서 본문으로 보지 않습니다.
  - 보고서를 파일로 저장할 때 답변도 검사 대상입니다. 답변을 "Saved: report.md"처럼 짧게 쓰지 않으면 그 문장도 표시 없는 줄로 셉니다.
  - 024 workflow로 실행된 보고서 요청에서는 child가 읽은 자료와 파일로 쓴 보고서를 검사합니다. workflow 진행 요약(완료 상태·child 요약)은 보고서 본문으로 보지 않으므로, 파일로 쓴 보고서가 없으면 `not written`으로 남습니다.
  - 자료끼리 내용이 맞지 않는지는 runtime이 판단하지 않습니다. 모델이 `[conflict: ...]`로 표시한 충돌만 보여 주고, 연결된 결론도 사실이라고 보증하지 않습니다.
- **결과 표시:** 답 다음에 판정, 읽은 자료, 연결되지 않은 결론과 그 이유, 충돌·미해결 수를 보여 줍니다. "Sources read"와 "Sources that could not be read"에는 이번 판정에 쓰는 자료(현재 selection revision에서 읽었거나 이전 판에서 가져온 자료)만 나옵니다. 방식 변경 전에 읽은 자료는 `/method status`에 `revision N; superseded_not_inherited`로 표시하고 판정에는 쓰지 않습니다. `/method status`는 자료별 기록과 결론별 인용 상태를 보여 줍니다. `/method status`, `/method history`, `/method accept`와 다음 turn의 모델 문맥은 판정을 보여 주거나 기록하기 전에 작업 폴더 파일이 읽은 뒤 바뀌었는지 다시 확인합니다.
- **`/method revise`:** 다음 메시지를 마지막으로 끝난 보고서 또는 이미지 요청의 새 판으로 만듭니다(이미지는 아래 절 참조). 보고서라면 이전 판에서 읽은 자료는 가져오지만 그대로 믿지 않습니다. 가져오는 자료는 이전 판의 마지막 판정에 쓰인 자료(그 판에서 읽었거나 이미 이월된 자료)뿐입니다.
  - 작업 폴더 파일은 바뀌지 않았을 때만 "이전 판에서 읽음, 그 뒤 변경 없음"으로 연결합니다.
  - 바뀐 파일과 웹 자료는 다시 읽어야 연결됩니다.
  - 새 판에서 새로 생기거나 바뀐 결론의 수와 처음 5개, 그리고 각 결론의 연결 여부를 표시합니다. 출처 표시 없는 문장 줄도 결론으로 비교하며 "no source marker"로 표시합니다.
  - 이전 판에는 있었지만 새 판에서 빠지거나 문구가 바뀐 결론(출처 표시 없는 줄 포함)도 "removed or reworded"로 수와 처음 5개를 표시합니다. 전체 목록은 `/method status`에서 봅니다. 새 판의 글이 아직 없으면 세지 않습니다.
  - 모델에게는 새 판 번호, 이전 요청 ID, 원래 요청을 함께 알려 줍니다.
  - 수정판 비교는 저장된 결론 문장(redaction 후 앞 256자)으로 합니다. 그 뒤만 바뀐 긴 결론은 바뀐 것으로 잡히지 않을 수 있습니다.
  - 진행 중인 요청이 있거나 사용자 아닌 turn이면 거절합니다.
- **저장 범위:**
  - 자료는 요청당 최근 32개를 보관합니다.
  - 결론은 출력당 32개, 판정당 64개까지 보관합니다.
  - 위치·결론 문장은 redaction 후 256자까지 저장합니다. redaction은 저장 한도에 1,024자를 더한 앞부분에만 적용합니다(긴 본문에서 느려지지 않게 하려는 것이며, 그보다 긴 secret은 가정하지 않습니다).
  - 보고서 본문 원문은 저장하지 않고 digest와 해석한 결론만 남깁니다. 검사 대상 본문은 출력당 앞 65,536자입니다.

### 이미지 방식의 결과 확인

이미지 방식(`image_generation_review`)은 `image_generate`로 실제 저장된 이미지 파일과, 요청에 적힌 조건 중 잴 수 있는 것을 따로 보여 줍니다. 그림의 내용이나 보기 좋은지는 자동으로 판단하지 않습니다.

- **필요한 설정:** `image_generate`가 등록돼야 합니다. side-effect 도구가 켜져 있고, `tools.imageGeneration.enable`과 이미지 생성을 지원하는 provider(`openai`, `openrouter`, `openai_codex`)가 설정돼야 합니다.
  - 등록되지 않았으면 provider를 부르기 전에 요청을 막고 다음 단계를 안내합니다.
  - 자격 증명과 provider 지원 여부는 호출할 때 확인합니다.
- **호출 기록:** 각 `image_generate` 호출의 옵션(size, format, count, quality, background), 프롬프트(redaction 후 256자), 결과를 같은 요청에 기록합니다.
  - 생성으로 인정하려면 도구가 보고한 파일이 media root 안에 실제로 있고 sha256이 일치해야 합니다. 그 파일의 머리 부분에서 형식(PNG·JPEG·WebP)과 픽셀 크기를 직접 잽니다.
  - 파일을 찾지 못하거나 digest가 다르면 `save not confirmed`입니다. 이미지가 생성됐다고 표시하지 않습니다. 한 호출이 8장을 넘게 보고하면 나머지를 확인할 수 없으므로 역시 `save not confirmed`입니다.
  - 판정은 가장 최근 호출이 정합니다. 같은 판에서 이미지가 저장된 뒤 다시 부른 호출이 실패·거부·저장 불확실이면, 앞의 이미지는 현재 결과가 아니며 "Earlier saved image in this request (not the current result)"로만 보여 줍니다. 다시 저장에 성공하면 그 이미지가 결과입니다.
  - 자격 증명이나 provider가 없으면 `unavailable`, provider가 지원하지 않는 요청은 `unsupported`, provider 오류와 저장 실패는 `failed`, 사용자 거부·취소·hook 차단은 `not generated`입니다.
- **요구 조건 확인:** 요청 문장에 적힌 크기(`1024x1024`), 형식(`png`, `jpeg`/`jpg`, `webp`), 장수(`2 images`, `2장`)를 저장된 파일과 비교합니다. 결과는 `met`, `not met`, `not checked`입니다.
  - 하나라도 `not met`이면 "stated requirements" 확인은 `failed`입니다.
  - 모두 맞아도 내용과 모양은 확인하지 않았으므로 `not verified`로 남습니다. 결과에는 항상 "content and look — not checked automatically; please review the image"를 붙입니다.
- **결과 표시:** 이미지 방식의 답은 모델 답변이 아니라 runtime이 확인한 내용입니다. 모델 답변은 이미지가 없는데 있다고 하거나, 확인하지 않은 내용(예: "파란 고양이")을 맞다고 할 수 있기 때문입니다. 성공·실패·호출 없음 모두 같습니다.
  - 저장이 확인되면 "Image saved and checked on disk (see below)."로 시작합니다. 요구 조건 중 `not met`이 있으면 "Image saved, but not every stated requirement is met (see below)."입니다. 이어서 내용과 모양은 직접 확인해 달라고 알립니다.
  - `image_generate`를 불렀지만 저장된 새 이미지가 없으면 "No new image was saved for this request (image: …)"입니다. 한 번도 부르지 않았으면 "No image was generated for this request (image: not generated)"이고, 다시 요청하거나 `/method use general`로 일반 답을 받는 방법을 안내합니다.
  - provider 오류나 반복 한도 등으로 turn이 정상 답 없이 끝났으면 "The turn ended early (…)"를 덧붙입니다.
  - 그 아래에 판정, 저장된 이미지의 참조, 형식·크기·바이트 수·digest 앞부분·provider/model, 요구 조건 확인 결과를 보여 줍니다.
  - 이미지 방식 요청에서 모델이 쓸 수 있는 도구는 `image_generate` 하나입니다. 질문(`ask_user`), 메시지 보내기(`message`), 하위 작업(`spawn`), 맡김 작업 제안 같은 다른 도구는 모델에게 보이지 않고, 불러도 실행되지 않습니다. 그런 호출의 인자는 대화 기록에 남기지 않고 진행 출력(tool 이벤트)으로도 내보내지 않습니다. 호출의 도구 이름도 모델이 쓴 글이므로(없는 도구 이름일 수도 있음), 대화 기록·복구용 checkpoint·runtime hook에는 `unoffered_tool`로 바꿔 남깁니다. 그래서 모델의 문장이 질문이나 메시지 모양으로 사용자에게 가지 않습니다.
  - 승인 요청 화면은 runtime이 만든 그대로 보여 줍니다(도구 이름·인자·위험도).
  - 모델 답변은 진단용으로 session metadata의 `withheld_reply`(redaction 후 512자)에만 남깁니다. `/method status`를 포함해 어떤 화면에도 보여 주지 않습니다.
  - 같은 이유로 이미지 방식 요청에서는 모델의 문장을 진행 출력으로 내보내지 않습니다. text·reasoning delta와 도구 호출 입력(delta·완성본) 모두 해당하며(WebSocket `sendProgress` 포함), 도구 호출 시작·종료와 미디어 이벤트만 보냅니다. provider가 스트리밍으로 알리는 도구 호출 시작은 모델이 쓸 수 있는 도구일 때만 보내고, 호출 ID는 digest(`call-…`)로 바꿉니다. runtime hook(`--verbose` 로그, observability hook, plugin hook)에도 모델 문장과 reasoning, 쓸 수 없는 도구 호출의 인자를 넘기지 않습니다. `image_generate`의 프롬프트도 tool 이벤트와 진단용 hook에는 길이·digest로만 넘기고, 도구 결과도 진행 이벤트·hook 문맥·대화 기록·checkpoint에는 길이·digest로만 남깁니다(아래 "복합 요청의 결과 확인"의 도구 인자·결과 규칙과 같음). 그래서 `run --verbose`의 "LLM response preview"는 이미지 요청에서 `<empty>`입니다. 대화 기록에도 이번 turn에 모델이 쓴 문장을 남기지 않습니다. 도구 호출과 함께 온 문장과 reasoning은 지우고 호출만 남기며, 마지막 답 자리에는 사용자에게 보인 runtime 답을 저장합니다. 실행 도중 저장하는 복구용 checkpoint에도 모델 문장·reasoning·thinking을 넣지 않으므로, 실행 중 프로세스가 멈춘 뒤 복구해도 기록에 되살아나지 않습니다. 그래서 승인을 기다리는 동안에도, 다음 turn의 모델 문맥에도 확인되지 않은 문구가 남지 않습니다. 이미지·복합 요청에서는 provider에 thinking(확장 추론)을 요청하지 않습니다. 설정한 reasoning effort를 이 요청들에는 보내지 않고, 앞선 대화의 thinking block도 보내지 않습니다. thinking block은 모델의 말인 데다, 승인 뒤 도구 호출을 이어갈 때 provider가 요구하는 모양 그대로(순서·redacted block 포함) 되돌려 줄 방법이 없기 때문입니다. 그래도 provider가 보낸 thinking block은 대화 기록(SDK 실행 결과의 `messages` 포함)에 남기지 않고 버립니다.
  - 이미지 방식 요청은 길거나 "review" 같은 말이 들어 있어도 자동 workflow로 보내지 않습니다. 메시지 metadata로 workflow를 요청해도 같습니다. workflow 하위 작업은 `image_generate`를 쓸 수 없고, workflow 답은 하위 작업이 쓴 문장을 그대로 담기 때문입니다. 대신 위와 같은 일반 실행과 확인을 거칩니다.
  - 같은 이유로, 이미지 요청의 사용자 turn에 이전부터 대기 중인 workflow가 있으면 이어서 실행하지 않고 취소로 기록합니다(durable 기록의 이유 `image_request`).
  - 참조는 data directory 또는 media root 기준 상대 경로이며 절대 경로를 보여 주지 않습니다.
  - `/method status`는 호출별 옵션, 프롬프트, 전체 digest, 저장 확인 여부, 오류를 보여 줍니다.
- **`/method revise`:** 마지막으로 끝난 보고서 또는 이미지 요청을 다음 메시지의 새 판으로 만듭니다. 이미지라면 원래 요청과 이전 판에 마지막으로 저장된 이미지 참조를 가져와 함께 표시합니다.
  - 명령 자체는 아무것도 만들지 않습니다. 다음 turn이 새 이미지를 요청하고, 저장이 확인된 이미지만 결과가 됩니다. 이전 요청에 저장된 이미지가 없으면 "has no saved image to link"로 알립니다.
  - 새 판은 수정된 요청으로 **새로 생성한** 이미지입니다. 이미지 편집·마스크·변형(edit, mask, variation)은 지원하지 않으므로, 이전 이미지를 고쳤다고 표시하지 않습니다.
  - 요구 조건은 원래 요청의 조건에 새 메시지의 조건을 덮어써서 다시 확인합니다. 이전 판의 확인 결과는 물려받지 않습니다.
- **저장 범위:**
  - 호출은 요청당 최근 16개, 호출당 이미지 8개, 이전 판 이미지 참조 8개를 보관합니다.
  - 마지막으로 저장이 확인된 이미지의 참조는 호출 기록이 한도로 밀려나도 따로 보관해 수정판 계보에 씁니다.
  - 이미지 파일은 media root에 남습니다. session metadata에는 호출별 옵션, redaction한 프롬프트(256자), 상태·오류, 호출 ID와 시각, 저장된 이미지의 참조·artifact ID·digest·잰 값·provider/model, 그리고 보류한 모델의 마지막 답변(512자, 화면에 보이지 않음)을 저장합니다. 이미지 바이트와 provider 원본 응답은 저장하지 않습니다.
  - 크기를 재려고 읽는 파일은 64 MiB 이하만 다룹니다.

### 복합 요청의 결과 확인

한 요청에 계산·보고서·이미지 중 둘 이상이 함께 들어 있으면 복합 방식(`composed`)으로 처리합니다. 예를 들어 "출처를 연결한 보고서를 쓰고 넓이도 계산해 줘", "자료로 보고서를 쓰고 그래프 이미지를 만들어 줘" 같은 요청입니다.

- **부분별 확인:** 각 부분(계산·보고서·이미지)은 앞 절들의 규칙대로 따로 기록하고 판정합니다. 한 부분의 성공이 다른 부분의 실패나 미검증을 통과시키지 않습니다.
  - 요약에는 먼저 "Parts (each checked on its own): calculation — verified; report — citation problems"처럼 부분별 상태를 한 줄로 보여 주고, 이어서 부분별 요약을 보여 줍니다.
  - 확인 항목 이름 앞에는 부분 이름이 붙습니다(예: `calculation: independent check`, `report: claim-source linkage`).
  - 방식 표시는 `composed [calculation + report]`처럼 부분을 함께 보여 줍니다.
- **답 표시:** 모델 답변은 runtime이 줄마다 확인할 수 있을 때만 보여 줍니다.
  - 보고서 부분이 있으면 모델이 쓴 보고서를 보여 주되, 확인되지 않은 줄 끝마다 runtime의 표시를 붙입니다. 출처 표시가 없는 줄은 `[runtime: no source marker, not checked]`, 연결되지 않은 인용이 있는 줄은 `[runtime: not backed: <인용> — <이유>]`입니다.
  - `[runtime: …]` 표시는 runtime만 붙입니다. 모델이 직접 쓴 `[runtime: …]`는 지우고, 그 줄도 확인 대상에서는 일반 문장으로 봅니다.
  - 확인되지 않은 부분(계산이 `verified`가 아님, 보고서가 `source-linked`가 아님, 이미지가 `generated`가 아님)이 하나라도 있으면 답 맨 앞에 "Not every part of this request is confirmed (…)"를 붙입니다.
  - 이미지 부분이 있는 복합 요청은 더 엄격합니다. 이 요청에서 읽은 파일의 문장을 그대로 옮긴 줄(인용)만 보여 줍니다. 그림의 내용은 아무도 확인하지 않으므로, 모델이 그림에 대해 쓴 문장이 확인된 것처럼 보이지 않게 하려는 것입니다. 특정 낱말로 그림 이야기를 가려내는 방식은 다른 말로 쓰면 빠져나가므로 쓰지 않습니다.
    - 인용인지는 그 줄이 `[source: …]`로 가리킨 파일을 다시 읽어 확인합니다. 읽을 때와 같은 내용(digest)이어야 하고, 줄의 문장이 그 파일의 **문장 전체나 줄 전체**와 글자 그대로 같아야 합니다(대소문자 포함). 공백의 수, 앞의 목록 기호(`-`, `1.`), 앞뒤 따옴표, 끝의 `,` `;` `:`, 문장 끝의 마침표만 달라도 됩니다. `?`나 `!`는 빠지면 안 됩니다.
    - 문장의 일부만 옮긴 줄은 인용이 아닙니다. 예를 들어 파일의 "The artwork is blue in the photo, not in the image."에서 "The artwork is blue"만 옮기거나, "Revenue grew 100%."에서 "Revenue grew 10"만 옮긴 줄은 감춥니다. 일부만 옮기면 뜻을 바꾸는 말(부정, 단서, 숫자의 나머지 자리, 부호)이 빠질 수 있기 때문입니다.
    - 문장의 끝은 `.` `?` `!` 뒤의 공백, 줄의 끝은 빈 줄·목록·제목·인용·표의 경계로 판단합니다. 한 문단을 여러 줄로 나눠 쓴 파일에서 줄 끝은 문장의 끝이 아닙니다.
    - "U.S." 같은 약어의 마침표는 뒤에 소문자로 시작하는 낱말이 오면 문장의 끝도 시작도 아닙니다. 그래서 "U.S. revenue fell."에서 "U.S."만 옮기거나 "revenue fell."만 옮긴 줄은 감춥니다. 다만 약어 뒤가 대문자로 시작하면 약어에서 문장이 끝난 것으로 보므로, 그 경우 약어 앞뒤로 나뉜 인용이 통과할 수 있습니다.
    - 8 MiB를 넘는 파일과 웹 자료는 인용으로 확인하지 않습니다.
    - 요청이 시작된 뒤 바뀐 파일(예: 모델이 이 요청에서 쓴 파일)은 인용으로 치지 않습니다. 모델이 직접 쓴 문장을 파일에 옮겨 놓고 인용하는 일을 막으려는 것입니다. 다만 수정 시각으로 판단하므로, 이전 요청에서 만든 파일이나 수정 시각을 되돌린 파일은 가려내지 못합니다.
    - 보여 주는 줄은 인용 문장과 그 자료의 번호만 남깁니다(예: "Revenue grew 10%. [source #1]"). 자료의 경로·주소는 모델이 고른 글이라 답에 적지 않으며, 번호와 이름의 짝은 `/method status`의 "Report sources"에서 봅니다. 같은 줄의 다른 표시(`[unresolved: …]` 등)에 모델이 쓴 말도 지웁니다.
    - 나머지 줄은 모두 "[runtime: line withheld — not a word-for-word quote of a file read in this request]"로 바꾸고(이어지는 줄은 하나로 묶음), 답 앞에 그 사실을 알립니다. 제목·안내 줄, 출처 표시 없는 줄, 연결됐지만 인용이 아닌 줄, 표시만 있는 줄(예: `[unresolved: …]`만 있는 줄), 저장 한도(256자)에 걸리는 긴 줄, 저장할 때 가려지는 비밀값이 든 줄이 여기에 들어갑니다. 글자가 없는 구분선(`---`)과 빈 줄만 그대로 둡니다.
    - 요약과 `/method status`에도 그런 줄의 문장은 옮기지 않고 수만 보이거나 "(withheld)"로 표시합니다.
    - 인용은 파일에 그렇게 적혀 있다는 뜻일 뿐, 그 내용이 맞다는 보증은 아닙니다(보고서 방식과 같음).
    - 요약에도 모델이 고른 이름과 도구 결과에서 나온 글을 적지 않습니다. 자료는 "Sources read: source #1 (names in /method status)", 읽기 실패는 "Sources that could not be read: source #2 (names and errors in /method status)", 이미지 호출의 오류(정책 hook의 거부 사유 등)는 "image_generate did not save an image (blocked before it ran); the error is in /method status", 계산 실행은 "Execution: primary #1 exit 0; check #2 exit 0"처럼 번호로 적습니다(호출 ID도 모델이 정할 수 있기 때문). 방식 표시(`/method history` 등)의 "Observed written file"도 경로 대신 길이·digest로 적습니다. 원문은 `/method status`의 상세에서 봅니다.
  - 보고서의 `[image]` 줄은 이미지가 들어갈 자리입니다. 그 줄에 모델이 쓴 문장은 버리고 runtime의 줄로 바꿉니다. 저장됐으면 "[Image: <참조> — saved and checked on disk; content and look not checked]", 아니면 "[Image: none — the image part is <상태>]"입니다. 그림의 내용은 아무도 확인하지 않으므로, 이 줄은 보고서 확인 대상에서도 빠집니다.
  - 보고서 부분이 없으면(예: 계산 + 이미지) 줄마다 확인할 방법이 없습니다. 이미지 방식처럼 모델 답변 대신 "Parts (each checked on its own): …"와 이미지 결과를 보여 주고, 계산 값은 요약의 "Verified answer"로 확인합니다. 원래 답변은 진단용 `withheld_reply`에만 남깁니다.
  - turn이 정상 답 없이 끝났으면 "The turn ended early (…), so its reply was not checked."를 붙이고, 출처를 단 줄도 모두 not checked로 표시합니다.
  - 도구 호출과 함께 온 문장과 reasoning은 대화 기록에서 지웁니다(이미지 방식과 같음). 마지막 답 자리에는 사용자에게 보인 답(runtime 표시가 붙은 보고서)을 저장합니다.
  - 복합 요청의 모델 문장은 진행 출력으로 내보내지 않고(text·reasoning delta와 도구 호출 입력), 판정이 끝난 답만 보냅니다. runtime hook(`--verbose` 로그, observability hook, plugin hook)과 복구용 checkpoint에도 모델 문장을 넘기지 않습니다(이미지 방식과 같음). runtime 표시 없이 모델 문장이 보이는 곳을 남기지 않으려는 것입니다.
  - 도구 호출의 인자도 모델이 쓴 말입니다(이미지 프롬프트, 파일 내용, 명령, 검색어, 경로, 호출 ID). 그래서 진행 이벤트(tool 이벤트)와 진단용 hook(`--verbose` 로그, observability hook, plugin의 `llm:after`)에는 인자 대신 그 길이·digest만 넘기고(예: `{"arguments":"<withheld: 52 chars, sha256:…>"}`), 호출 ID도 digest(`call-…`)로 바꿉니다. 같은 호출의 시작과 끝은 같은 digest라 서로 맞출 수 있습니다. provider가 응답이나 호출에 붙이는 고유 필드(호출의 `extra_content`·`provider_specific_fields`, 응답의 `error_type`·`error_code` 같은 오류 문자열)도 hook에 넘기지 않습니다. hook이 받는 응답에는 호출의 이름·digest와 종료 이유, 사용량, 상태 코드 같은 값만 있습니다. 종료 이유는 runtime이 아는 값(`stop`, `tool_calls`, `length`, `content_filter`, `error`)이 아니면 `other`로, 사용량은 알려진 token 수 항목만 넘깁니다. provider의 스트리밍 종료 이벤트도 같고, 이미지 생성 진행 이벤트는 provider가 정한 후보 ID 대신 그 digest로 보냅니다. 실행 기록(session metadata `runtime_execution`)도 도구 호출을 원래 호출 ID 대신 그 SHA-256 digest로 부르고, 모델이 한 질문은 digest로만 둡니다. 방식 변경으로 계산·일반 방식을 거쳐도 그 요청이 끝날 때까지 같고, 다시 이미지·복합 방식이 되면 남아 있던 기록도 이렇게 바꿉니다. 이미지 방식도 같습니다. 이 때문에 이런 요청에서는 skill 파일을 읽어도 skill 사용 알림이 나오지 않습니다.
  - 도구의 결과에도 모델의 말이 실릴 수 있습니다(명령이 출력한 글, 모델이 쓴 파일을 읽은 내용). 그래서 진행 이벤트, 진단용 hook의 문맥, 저장되는 대화 기록, 복구용 checkpoint에는 도구 결과 대신 길이·digest만 남깁니다(예: `<withheld: 1024 chars, sha256:…>`, 오류였으면 앞에 `Error: `). 이미지 방식도 같습니다.
  - 모델은 같은 요청 안에서는 도구 결과를 그대로 봅니다. 승인을 기다렸다가 이어 실행할 때 필요하기 때문입니다. 이를 위해 요청이 열려 있는 동안만 원래 결과를 session metadata(`task_method_held_results_v1`)에 두고 다음 실행 때 모델 문맥에 되돌립니다. 방식 변경으로 같은 요청이 계산·보고서 같은 다른 방식이 된 뒤에도 원래 결과를 모델에게 되돌립니다. 다만 그 실행의 runtime hook에는 이미지·복합 단계에서 저장된 호출(호출 ID, 인자, 결과)을 이미지·복합 요청에서처럼 길이·digest로만 넘기고, 새 방식에서 생긴 호출은 그 방식의 규칙대로 둡니다. 요청이 어떻게 끝나든(답, 실패, provider 오류, 승인 거부, `/stop`, `/new`, 새 요청 시작) 지웁니다. 화면에는 나오지 않지만 `session export --yes`(경고 후 원본 metadata를 내보내는 진단 기능)에는 그때 남아 있는 내용이 포함됩니다.
  - 그래서 요청이 끝난 뒤의 다음 요청에서 모델은 이전 도구 결과의 digest만 봅니다. 필요하면 다시 읽어야 합니다. 실행 중 프로세스가 멈춘 뒤 checkpoint로 복구한 경우에도 모델은 그 결과의 digest만 봅니다.
  - 승인 뒤 실행한 도구의 결과가 turn을 멈추는 오류이면(예: 작업 폴더 밖 경로) 답은 "The approved tool call stopped this turn; its output is withheld (Error: <withheld: …>)."입니다. 원래 결과는 보여 주지 않습니다.
  - provider API 오류의 글에도 모델의 말이 실릴 수 있습니다(예: 모델이 지어낸 도구 이름을 그대로 인용한 오류). 그래서 이미지·복합 요청이 provider API 오류로 끝나면 답은 "Sorry, I encountered an error calling the AI model: provider API error (HTTP 400); its message is withheld (<withheld: …>)."처럼 상태 코드와 오류 글의 길이·digest만 보여 줍니다. provider가 오류를 응답으로 알린 경우(HTTP 오류 응답, 스트림의 오류 frame, 재시도가 다 끝난 뒤의 오류)도 같습니다. 대화 기록, SDK 실행 결과, 실행 기록(`runtime_execution`)의 오류 상세도 같습니다. provider·model을 찾지 못함, 인증 필요 같은 설정 오류는 설정 값만 담으므로 그대로 보입니다.
  - 호출을 실행하는 도구, 권한 판정과 승인 화면, plugin의 `tool:before` 정책 hook, method 기록은 인자를 그대로 받습니다. 정책이 실제 인자를 보고 막을 수 있어야 하고, 사용자는 승인 전에 무엇을 실행하는지 봐야 하기 때문입니다. 대화 기록에도 provider가 대화를 이어가는 데 필요한 도구 호출(인자 포함)은 남습니다.
  - `/method status`는 확인 근거를 직접 살펴보는 곳이라 모델이 쓴 실행 기록을 그대로 보여 줍니다. 이미지 호출의 프롬프트, 계산 실행의 코드·출력·입력·가정이 여기에 해당합니다. 모두 "prompt", "code", "output"처럼 이름이 붙어 있으며, 그림에 대해 확인된 내용이 아닙니다. 이미지 부분이 있는 복합 요청의 답과 요약에는 계산의 입력·가정을 적지 않고 "Inputs and assumptions: recorded, shown in /method status"만 적습니다. 실행 출력에서 읽은 package 이름도 요약에는 적지 않고 "a required Python package is not installed (see /method status)"로 적습니다.
  - 복합 요청에서 모델은 질문(`ask_user`), 메시지 보내기(`message`), 하위 작업(`spawn`), 맡김 작업 제안 도구를 쓸 수 없습니다. 이 도구들은 확인을 거치지 않은 모델 문장을 답 밖으로 보내기 때문입니다. 다른 도구(자료 읽기, 계산 실행, 이미지 생성 등)는 그대로 쓸 수 있습니다.
  - 쓸 수 없는 도구(위 도구들이나 아예 없는 이름)를 부른 호출은 실행되지 않습니다. 그 호출의 진행 이벤트와 provider의 스트리밍 호출 시작 이벤트는 내보내지 않습니다. 도구 이름도 모델이 쓴 글이므로, runtime hook(정책 hook 포함)·대화 기록·복구용 checkpoint·SDK 실행 결과의 사용한 도구 목록(`tools_used`)에는 인자 없이 `unoffered_tool`이라는 이름으로만 남깁니다. 호출 ID는 대화 기록에 그대로 두어 호출과 결과의 짝을 유지합니다. 방식 변경으로 요청이 다시 이미지·복합 방식이 될 때 앞서 저장된 기록의 없는 도구 이름도 같이 바꿉니다.
  - 복합 요청은 길거나 "review" 같은 말이 있어도 자동·metadata workflow로 보내지 않습니다. 사용자 turn에 대기 중인 workflow가 있으면 이어 실행하지 않고 취소로 기록합니다(이유 `composed_request`). 이미지 방식과 같은 이유입니다.
- **보고서에서 계산 결과:** 보고서의 `[calculation]` 줄은 계산 결과가 들어갈 자리입니다(`[image]`와 같음). 그 줄에 모델이 쓴 문장은 버리고 runtime의 줄로 바꿉니다.
  - 계산이 `verified`이면 "[Calculation: 6 m^2 — verified by a separate check run]"처럼 확인된 값과 단위를 runtime이 직접 적습니다. 아니면 "[Calculation: none — the calculation part is <상태>]"(예: `in conflict`, `unavailable`), 계산 부분이 없으면 "[Calculation: none — this request has no calculation part]"입니다.
  - 모델이 그 줄에 다른 값이나 부정하는 문장을 써도("600 m^2", "6이 아니다") 사용자에게는 보이지 않습니다. 문장을 읽어 값이 맞는지 판단하는 방식은 표현이 다양해 빠져나갈 수 있으므로 쓰지 않습니다.
  - 이 줄은 보고서 확인 대상에서 빠집니다. 다만 파일로 쓴 보고서 안의 `[calculation]` 줄은 runtime이 바꿔 쓸 수 없으므로 확인하지 않은 인용으로 남습니다("not checked; only the runtime's [calculation] line in the answer shows the checked result"). 그래서 그 보고서는 `citation problems`가 됩니다.
- **일부 부분을 쓸 수 없을 때:** 필요한 도구가 없는 부분은 `unavailable`로 표시하고 나머지 부분만 진행합니다. 방식 표시 줄에 "unavailable parts: image"처럼 적습니다.
  - unavailable 부분의 확인 항목은 `not verified`이며, 근거에 "unavailable: <도구> is not registered"를 남깁니다.
  - 모든 부분을 쓸 수 없으면 provider를 부르기 전에 요청을 막습니다.
- **방식 변경과 부분 취소:** 진행 중 `/method use report`처럼 한 부분만 남기면, 보류 중인 승인·질문(예: 이미지 생성 승인)을 취소하고 같은 목표로 이어갑니다.
  - 방식 변경으로 요청이 이미지 방식이나 복합 방식이 되면(예: `/method use calculation`으로 계산만 남겼다가 `/method auto`로 다시 이미지를 포함), 그 요청에서 앞서 저장된 기록도 같은 규칙으로 정리합니다. 모델의 문장은 지우고, 도구 결과는 digest만 남깁니다. 원래 결과는 앞서 말한 보관 metadata로 옮겨 모델에게만 되돌리며, 같은 호출 ID의 결과가 여럿이어도 각각 제자리로 돌아갑니다.
  - 요청의 기록이 어디서 시작하는지는 요청의 첫 메시지(목표 문장)로 찾고, 없으면 요청이 시작된 시각으로 찾습니다. 둘 다 찾지 못하면 기록을 정리할 수 없으므로 이미지·복합 방식으로의 변경을 거절하고("Method not changed: …") 새 요청을 시작하라고 안내합니다.
  - 이미지 부분이 있는 요청(이미지 방식, 이미지를 포함한 복합 방식)에서는 방식 변경 응답, `/method status`, `/method history`의 "Remaining question"과 "User decision"에 모델이 했던 질문을 적지 않고 "(the model's question is withheld)"로 적습니다. 사용자의 답은 그대로 보입니다.
  - 보고서 부분이 남으면 이미 읽은 자료를 "read before the method change"로 가져와 다시 확인합니다. 작업 폴더 파일이 바뀌지 않았을 때만 연결됩니다.
  - 새 방식에도 남는 부분은 이미 만든 결과를 유지합니다. 계산 부분이 남으면 앞서 기록된 실행을, 이미지 부분이 남으면 앞서 저장된 이미지를 새 판에서 다시 판정합니다. `/method status`의 계산 실행에는 "kept across the method change and judged again"이 붙습니다.
  - 방식 변경으로 취소한 보류 승인의 호출은 남는 부분의 기록에 취소로 남깁니다. 계산 실행이었다면 마지막 주 계산 뒤의 취소된 실행이므로, 주 계산과 확인 계산을 다시 실행해야 `verified`가 됩니다. 이미지 생성이었다면 앞서 저장된 이미지는 더 이상 현재 결과가 아니고 "Earlier saved image in this request (not the current result)"로만 보입니다.
  - 빠지거나 새로 더해진 부분은 처음부터 다시 판정합니다. 보고서 부분이 한 번 빠지면 이월 자료도 버리므로, 나중에 다시 더해져도 그 전 자료는 되살아나지 않습니다. 보고서 부분이 남을 때 이월하는 자료는 방금 떠나는 판에서 쓰던 자료(그 판에서 읽었거나 이미 이월된 자료)뿐입니다. 이전 판의 확인 항목은 물려받지 않습니다(`superseded_not_inherited`).
  - 사용자가 한 부분의 도구 실행(예: 이미지 생성)을 거부하면 그 부분의 기록에 거부로 남습니다. 예를 들어 저장된 이미지 뒤의 새 생성을 거부하면 그 이미지는 더 이상 현재 결과가 아닙니다.
- **수용과 수정:** `/method accept`는 요청 전체에 한 번 기록하며 판정은 바꾸지 않습니다. 기록에는 부분별 상태 전체(예: "report — source-linked; image — unavailable")를 남깁니다. `/method revise`는 보고서 단독·이미지 단독 요청만 대상으로 합니다.

저장 범위는 session metadata `task_method_requests_v1`의 활성 요청 1개와 최근 종료 요청 32개입니다. Preview 문자열은 redaction 후 512자, provider projection은 8,192자로 제한합니다. 한 요청의 과거 revision은 최대 128개이며 한도에 도달하면 변경을 거절하고 기존 기록을 유지합니다. 관측 효과·산출물·결정·남은 질문은 각각 128개로 제한하며, 오래된 효과가 생략된 경우 그 수를 표시합니다. Conversation history의 보존·compaction 정책은 별개입니다. Synthetic/system/automation turn은 새 method request나 one-shot 소비를 만들지 않으며(사용자 channel로 들어오지만 `automation_route` 표시가 있는 자동화 알림·검증 메시지도 포함), method 가용성으로 차단하지 않습니다. 이런 turn에서 온 `/method use`와 `/method auto`는 상태를 바꾸지 않고 거절합니다. 사용자 요청이 시작한 workflow를 이런 turn이 재개해 끝내면 연결된 요청도 그 workflow 결과(완료·실패·취소)로 닫습니다. 이런 turn이 진행 중 요청의 보류 질문에 대한 응답으로 처리되어 실행을 이어가면 그 결과와 관측 효과도 같은 요청에 기록합니다. 다만 그 내용을 사용자 결정으로 기록하지는 않습니다.

## 작업 방법 후보와 `/learn`

대화에서 끝낸 작업과 사용자의 피드백을 근거로, 다음에도 쓸 만한 작업 방법의 **후보**를 만들고 조회합니다. 후보는 비활성입니다. 다음 요청에 자동으로 쓰이지 않고, 저장해도 도구 권한·skill·승인이 생기지 않습니다. 후보는 사용자가 검토 화면의 코드로 확인하거나, 서로 다른 두 번째 작업에서 근거 있는 단계가 모두 다시 실행됐을 때만 확인된 방법이 됩니다(아래 "방법의 확인·거절과 다른 작업으로 확인"). 확인된 방법은 그 뒤 맞는 요청에 작업 방식의 안내로 쓰이고, 답 끝에 어느 방법·버전을 썼는지 표시됩니다. 사용자는 방법을 고쳐 새 버전을 만들거나, 사용을 멈추거나, 이전에 확인한 버전으로 되돌릴 수 있습니다(아래 "확인한 방법의 사용·수정·되돌리기").

```text
/learn
/learn list
/learn show method-1
/learn confirm method-1
/learn reject method-1 v1
/learn verify method-1
/learn match <요청>
/learn edit method-1 v1 step 2 <새 문장>
/learn deactivate method-1 v1
/learn restore method-1 v1
/learn show method-1 v2
```

- **`/learn`:** 현재 대화의 최근 사용자 turn 6개와 그 뒤 기록에서 근거를 모아 후보를 만듭니다. 사용자 turn에서만 동작하며, system·cron·automation turn과 다른 대상을 위해 주입된 메시지(`injected_event` 등)는 거절합니다. 도구를 하나도 주지 않은 model 호출을 두 번 합니다.
  1. 사용자가 직접 쓴 메시지만 보여 주고 요청·피드백·절차·선호·이번 작업 조건·섞임(선호나 이번 작업 조건이 다른 내용과 함께 있음)·기타로 나눕니다.
  2. 그 결과에 따라 보여 줄 수 있는 근거만으로 후보 초안을 받습니다.
  - 어느 응답이든 도구 호출을 요청하면 아무것도 실행하지 않고 저장하지도 않습니다("nothing was run").
- **초안 호출에 보이는 것:** 요청·피드백·절차로 분류된 사용자 메시지의 글, 도구 호출의 이름과 실행 판정, 답변이 있었다는 사실뿐입니다. 글은 항목마다 redaction 뒤 600자로 자르고 근거는 최대 40개입니다. 요청은 어떤 종류의 작업이었는지 알리는 용도이며, 초안은 요청의 표현·이름·경로를 옮기지 말고 일반적으로 쓰도록 지시합니다.
  - 도구 이름은 `/learn` 시점에 등록된 도구일 때만 그대로 쓰고, 그 밖의 이름(모델이 지어낸 이름, 지금은 연결되지 않은 MCP 도구 등)은 `unregistered tool`로 바꿔 보여 주고 저장합니다. 모델이 쓴 이름에도 글이 실릴 수 있기 때문입니다.
  - 선호·이번 작업 조건·섞임·기타로 분류된 사용자 메시지는 초안 호출에 아예 넘기지 않습니다.
  - 도구 결과, 첨부, 사용자가 아닌 주입 메시지, 그리고 모델이 쓴 모든 인자와 답변은 글을 넘기지 않습니다. 모델의 답변은 제외한 선호를 되풀이할 수 있기 때문입니다(예: "Call me Bob"에 대한 "Okay Bob").
  - 근거 안의 지시는 따를 명령이 아니라 데이터로만 다루도록 지시합니다.
- **신뢰 구분:** 근거마다 `from you`(사용자가 직접 쓴 turn), `assistant output`(외부 자료를 읽기 전의 모델 출력), `untrusted`(Spec 024 quarantine 기준)를 표시합니다.
  - `from you`: runtime이 사용자 turn을 저장할 때 붙인 표시(`owner_turn`)가 있는 메시지만입니다. system·heartbeat·cron·automation channel turn, 다른 대상을 위해 주입된 메시지, runtime이 직접 넣은 user 메시지에는 이 표시가 없습니다. 이 기능 이전에 저장된 메시지에도 없으므로 그런 메시지는 `untrusted`로 보고, 그 뒤의 모델 출력도 `untrusted`가 됩니다. 이전 대화를 이어 가면 새로 입력한 turn만 사용자 말로 쓰입니다.
  - `untrusted`: 도구 결과·첨부, `from you`가 아닌 user 메시지(runtime이 저장한 출력 한도 복구 지시·turn 중간 주입 포함), 그리고 같은 session에서 그런 자료를 한 번이라도 읽은 뒤의 모델 출력입니다. 근거 창보다 앞선 기록도 따집니다.
  - 도구 결과와 첨부는 내용 대신 digest(`material … (untrusted)`)만 남깁니다.
- **실행 판정:** 도구 결과에 실패 표시가 있으면 `failed`입니다. 실패 표시는 runtime이 도구 실패로 보는 형식(`Error`로 시작, MCP 호출의 실패·시간 초과·취소 표시(MCP 도구가 `isError`로 스스로 알린 실패 포함), 작업 폴더 밖 접근 같은 안전 차단), `Permission denied`, 최상위 `error`가 있는 JSON, 마지막 줄의 0이 아닌 `Exit code: N`입니다. 도구가 이 밖의 자유로운 문장으로 실패를 알리면 알아보지 못합니다. 결과가 없거나 비었거나 승인 대기·건너뜀이거나 Spec 037에서 가려진 결과(`<withheld: …>`)이거나 마지막 줄에 종료 코드가 없는 `exec` 결과이거나, `read_file`이 같은 프로그램 실행 안에서 이미 읽은 바뀌지 않은 파일이라며 내용 대신 `[File unchanged since last read: …]`만 돌려준 결과이면 `did not finish`입니다. 그 밖의 결과만 `ran without a reported error`이며, 이는 그 단계가 좋은 결과를 만들었다는 뜻이 아닙니다.
  - 결과는 호출 ID로 짝지으며, 뒤에서 같은 ID로 다시 한 호출의 결과는 앞 호출의 결과로 보지 않습니다. 같은 호출 묶음 안에서 ID가 겹치거나 비어 있으면 `did not finish`입니다.
- **단계의 근거:** runtime이 단계가 인용한 근거로만 정합니다. 모델이 붙인 판정은 무시합니다.
  - `your own words`: 단계 문장이 인용한 피드백·절차 메시지에 그대로 들어 있습니다. 대소문자·띄어쓰기·끝 문장부호만 달라도 되며 6자 이상이어야 합니다. 그 단계의 근거로는 문장이 실제로 들어 있는 메시지만 남깁니다. 요청은 그 작업에만 해당하므로 인용으로 치지 않습니다.
  - `ran without a reported error`: 오류 없이 실행된 도구 호출을 인용했습니다. 단계 문장은 runtime이 쓴 "Run <도구>"(같은 도구는 한 번, 400자까지)이고, 모델의 설명은 확인하지 않은 글로 따로 붙입니다. 그 단계의 근거로는 그 호출만 남깁니다.
  - `unverified`: 그 밖의 모델 문장입니다. 실패하거나 끝나지 않은 실행만 인용했으면 이유를 적고, `untrusted` 근거를 인용했으면 `influenced by untrusted material`을 붙입니다.
  - 목적·적용 작업은 요청·피드백·절차 메시지를 인용해야 하며 모델의 문장으로 표시합니다. 전제는 인용한 피드백·절차 메시지에 그대로 들어 있을 때만 전제로 남고, 그렇지 않으면 "Premise not in your own words"로 미확인 사항에 옮깁니다.
  - 모든 후보에 "Effect not attributed"를 표시합니다. 결과가 좋았거나 받아들여졌다는 사실은 어느 단계 덕분인지 보여 주지 않습니다.
- **가려 둔 내용이 다시 들어오는 것 막기:** 초안의 어느 문장이든 초안 호출에 보여 주지 않은 글을 되풀이하거나 제외한 메시지를 인용하면 그 문장을 버리고 수를 적습니다. 목적이나 적용 작업이 그렇게 버려지면 후보를 저장하지 않습니다. 보여 준 글에도 있는 표현은 되풀이로 보지 않습니다.
  - 모든 가려 둔 글(제외한 사용자 메시지, `untrusted` 근거의 글, 근거로 남은 도구 호출의 결과): 16자 이상 글자 그대로 겹치면 되풀이입니다.
  - 선호·이번 작업 조건·섞임으로 분류된 메시지: 문장부호(`.` `!` `?` `,` `;` `:`)와 줄바꿈으로 나눈 구절이 낱말 단위로 그대로 들어 있으면, 한 낱말짜리 구절이라도 되풀이입니다.
  - 그 밖의 가려 둔 글(기타로 분류된 사용자 메시지, 사용자가 아닌 메시지, `untrusted` 답변·인자, 도구 결과): 두 낱말 이상인 구절이 그대로 들어 있으면 되풀이입니다.
  - 요청: 초안 호출이 보는 글이지만 그대로 옮기면 안 됩니다. 요청의 32자 이상, 요청의 구절 하나(한 낱말이나 기호뿐이라도: 요청 "Check the release date."나 한 낱말 요청 "zephyrcase", 띄어 쓰지 않은 "보고서요약해줘", 이모지 하나뿐인 요청을 그대로 쓴 목적), 또는 요청에 든 구체적인 표시(경로, `@`가 든 주소, 파일 이름처럼 가운데 점이 있는 낱말, 숫자, 대문자가 둘 이상인 4자 이상 코드)를 그대로 담은 문장은 버립니다. 같은 표현이 피드백·절차 메시지에도 같은 낱말로 있으면 버리지 않습니다. 더 긴 낱말의 일부로만 있으면(예: 피드백의 `1234` 속 `123`) 버립니다.
  - 글자 그대로의 되풀이만 찾으므로 바꿔 쓴 표현이나 16자보다 짧은 구절의 일부는 잡지 못합니다. 예를 들어 요청 구절 속의 소문자 이름이나 띄어 쓴 한국어 고유명사 한 낱말만 옮기면 걸러지지 않습니다. 다만 초안 호출은 요청 밖의 가려 둔 글을 볼 수 없습니다. 반대로 우연히 같은 표현을 쓴 문장도 버려질 수 있으며, 버린 수는 항상 표시합니다.
- **선호와 이번 작업 조건:** 선호·이번 작업 조건·섞임으로 분류된 메시지는 "Kept out of the method"에 근거 ID로만 남기고 그 글은 저장하지 않습니다. 분류는 모델이 메시지 단위로 하므로, 잘못 분류된 메시지는 걸러지지 않습니다. 기존 기억(`memory/`, `USER.md`)은 바꾸지 않습니다.
- **후보 없음:** 사용자 요청이 없거나, 요청·피드백·절차로 분류된 메시지가 없거나, 모델이 근거가 없다고 하거나, 사용자 말이나 오류 없이 실행된 도구 호출로 뒷받침된 단계가 하나도 없으면 "No method candidate was created … This is a normal result"를 보여 주고 아무것도 저장하지 않습니다. 오류가 아닙니다.
- **저장:** workspace의 `.shacs-methods/methods.json`(권한 `0600`)에 저장하며 `memory/`·`USER.md`·`skills/`와 분리됩니다. 같은 workspace의 다른 session에서도 `/learn list`로 보입니다. 목록에는 상태(와 그 상태를 만든 결정), 적용 작업, 단계 수가 보입니다.
  - `.shacs-methods/`는 보호 대상입니다. 모델의 파일 도구(`read_file`·`write_file`·`edit_file`·`notebook_edit`·`list_dir`·`glob`·`grep`)는 권한 모드와 관계없이 그 안을 읽거나 바꾸지 못합니다.
    - 경로에 `.shacs-methods`가 들어 있으면 권한 단계에서 거절됩니다(`auto`에서는 묻지만 승인해도 실행되지 않습니다).
    - 다른 이름으로 가리켜도 도구가 실제 경로를 확인해 거절합니다. 그 폴더로 가는 symlink와 저장 파일의 hard link(Unix 계열)가 여기에 해당합니다.
    - 폴더를 훑는 `list_dir`·`glob`·`grep`은 경로를 주지 않아도(`grep`의 기본값 `.` 포함) `.shacs-methods` 폴더를 건너뜁니다. `grep`은 저장 파일의 hard link도 읽지 않습니다. `list_dir`·`glob`에는 hard link의 이름만 보일 수 있습니다.
    - 셸 명령(`exec`), Python, 파일을 첨부해 보내는 `message` 같은 다른 도구는 이 경로를 따로 막지 않고 기존 승인 경계를 따릅니다. 승인하면 저장 파일을 읽거나 바꿀 수 있습니다.
  - 후보 ID, 버전, 작성 시각, 목적·적용 작업·전제와 인용, 단계, 미확인 사항, 제외 항목, 근거별 종류·신뢰 구분·출처·원래 메시지의 시각·digest를 남깁니다. 출처는 `session://<session key digest 12자리>/<당시 메시지 번호>`이고, session key 자체는 저장하지 않고 digest만 둡니다.
  - 저장하는 모든 글은 redaction을 거칩니다. 비밀값처럼 보이는 `키: 값`·`키=값`(예: `secrets: weekly`의 `weekly`)도 `[REDACTED]`로 바뀌므로, 그런 표현이 든 사용자 문장은 `your own words` 단계에도 바뀐 모양으로 남습니다.
  - 메시지 자체(요청·피드백·도구 결과·답변)는 저장하지 않고 digest와 출처만 둡니다. 저장되는 글은 모델이 쓴 목적·적용 작업·전제·단계 문장과 runtime의 표시이며, 위의 검사가 요청이나 가려 둔 글을 옮긴 문장을 버리지만 그 검사가 놓치는 짧은 표현은 남을 수 있습니다.
  - 보여 줄 때마다(`/learn`, `list`, `show`) 저장된 모든 후보를 원래 session의 지금 기록과 다시 대조합니다(session마다 한 번만 읽습니다). 같은 시각·같은 digest의 메시지가 아직 있으면 다음을 다시 확인하고, 하나라도 어긋나면 `/learn` 밖에서 고친 것으로 보고 "store is invalid (a method does not match the messages it cites)" 오류를 보여 주며 파일은 그대로 둡니다.
    - 저장된 신뢰 구분·실행 판정·결과 digest·도구 이름이 그 메시지에서 다시 계산한 값과 같은지. 사용자 메시지의 신뢰 구분은 정확히 같아야 하고, 모델·도구 기록은 저장된 쪽이 더 신뢰되지만 않으면 됩니다.
    - 전제와 `your own words` 단계 문장이 인용한 각 메시지(생성 때 보인 redaction·길이 제한 형태)에 그대로 들어 있는지.
    - 모델이 쓴 목적·적용 작업·전제·단계·설명·미확인 사항이 남아 있는 요청이나 제외한 사용자 메시지를 옮기지 않았고, 제외한 메시지를 인용하지 않았는지(생성 때와 같은 검사).
  - 메시지가 아직 있지만 저장했을 때와 다른 위치에 있으면(기록 정리로 앞부분이 잘린 경우 등) 내용은 다시 확인하되 그 근거에 "saved position not rechecked: the session changed since"를 붙입니다. 저장할 때 끝나지 않았거나 실패한 도구 호출에 그 뒤 결과가 붙었으면(승인이나 답이 늦게 온 경우) 오류로 보지 않고 "its result changed after saving, not rechecked"를 붙입니다. 이런 호출은 처음부터 어떤 단계도 뒷받침하지 않습니다.
  - 메시지가 더 이상 없으면 그 근거에 "no longer in that session"을 붙이고, 끝 줄에는 "some source messages changed or were cleared since"를 붙입니다. 그 근거에 기댄 단계는 "your own words when saved"·"ran without a reported error when saved"와 "not rechecked: its source is no longer in that session"으로, 목적·적용 작업·전제는 인용 뒤에 같은 표시를 붙여 보여 줍니다. `/learn list`에는 "some sources changed or cleared since, not rechecked"를 붙입니다. session이 지워졌으면 "(that session no longer exists)"입니다. "(this conversation)"은 후보를 만든 session에서 근거가 모두 저장한 위치에 남아 있을 때만 붙습니다. `/new`나 기록 정리 뒤에는 붙지 않습니다.
  - session 목록을 읽지 못하거나, 읽을 수 없거나 일반 파일이 아닌(링크·디렉터리) session 항목이 있는데 후보의 원래 session을 찾지 못하면 삭제된 것으로 보지 않고 "learned methods could not be checked against their sessions (…)" 오류를 보여 줍니다. 읽을 수 없는 파일 이름을 함께 보여 줍니다.
  - 이 대조가 잡는 것은 저장 파일에서 남아 있는 근거로 다시 계산·검사할 수 있는 부분을 고친 경우입니다. 근거 메시지가 사라진 뒤 고친 부분, 근거 기록 자체를 지운 편집(예: 제외한 선호의 기록과 "Kept out" 항목을 함께 지운 경우), 저장하지 않은 근거(인용하지 않은 답변·도구 결과)나 도구 결과·답변의 글을 옮겨 넣은 문장, 어떤 근거에도 없는 글로 바꾼 모델 문장(계속 "the model's wording"·"unverified"로 표시됨), 모델의 분류(`classified_as`)는 다시 확인하지 못합니다(분류를 바꾸면, 예를 들어 요청을 피드백으로 바꾸면 그 메시지에는 요청을 옮긴 문장 검사가 다시 적용되지 않습니다). 저장 파일과 session 파일은 모두 workspace의 일반 파일이므로 둘을 함께 맞춰 고치면 구별하지 못합니다.
  - 근거 창 밖으로 빠진 근거 수(앞선 turn과 40개 한도 초과분)를 표시합니다.
  - 최대 200개입니다. 저장하기 전과 읽을 때 모두 개수, 파일 크기(16 MiB), 버전·단계·전제·인용·목록 수, 글과 ref의 길이, 버전 번호와 근거 ID의 중복을 확인합니다. 저장 전에 한도를 넘으면 저장하지 않습니다. 그 한도를 넘거나, 파일이 손상됐거나 모르는 형식이거나 경로가 symlink이면 바꾸지 않고 오류를 보여 줍니다.
  - 읽을 때 표시와 근거가 맞는지도 확인합니다. 인용한 근거가 모두 있어야 하고, 원래 session과 근거의 digest는 `sha256:<소문자 16진수 64자리>`, 근거 ID는 `e<번호>`, 출처는 `session://<원래 session digest 12자리>/<번호>`(도구 호출은 뒤에 `#call-<12자리>`) 형식이어야 하며, 도구 이름·실행 판정은 도구 호출에만, 분류는 사용자 메시지에만 있어야 하고, 저장된 글(목적·단계·미확인 사항·도구 이름·시각 등)을 redaction해도 바뀌지 않아야 하며(손으로 넣은 비밀값은 거절), 도구 호출·답변은 `from you`일 수 없고 사용자 메시지는 `assistant output`일 수 없으며, 선호·이번 작업 조건·섞임으로 분류된 메시지마다 "Kept out" 항목이 있어야 합니다. `your own words` 단계와 전제는 `from you`인 피드백·절차 메시지를, 목적·적용 작업은 `from you`인 요청·피드백·절차 메시지를, `ran without a reported error` 단계는 오류 없이 실행된 도구 호출을 인용해야 하며, 그 단계 문장은 runtime이 쓰는 "Run <도구>"와 같아야 합니다. 파일을 고쳐 표시만 바꾸면(예: 사용자 말 단계를 실행 단계로) 저장소가 잘못됐다는 오류("malformed method record")를 보여 주고 파일은 그대로 둡니다.

### 방법의 확인·거절과 다른 작업으로 확인

```text
/learn confirm method-1
/learn confirm method-1 v1 3f2a9c1b
/learn reject method-1 v1
/learn verify method-1
/learn match Summarize the meeting notes in minutes.md
```

모든 명령은 사용자 turn에서만 동작합니다(위 `/learn`과 같은 기준).

- **상태:** `candidate`(후보: 기본으로 쓰지 않음), `confirmed`(확인됨: 맞는 작업에 기본 방법으로 제안할 수 있음), `rejected`(거절: 다시 제안하지 않음), `inactive`(확인됐지만 그 뒤 사용자의 피드백과 어긋났거나 사용자가 사용을 멈춤)입니다. 상태는 현재 버전에 대한 결정 기록으로 정해지고, 목록과 `show`에 그 상태를 만든 결정과 시각이 함께 보입니다. 어떤 상태도 도구 권한·skill·승인을 만들지 않습니다. 확인한 뒤에도 명령 실행·Python·plugin·외부 발송은 그때의 승인 경계를 그대로 거칩니다.
- **사용자 확인:** `/learn confirm <id>`는 현재 버전의 전체 내용(근거별 출처와 신뢰 구분, 지금까지의 결정 포함), 확인의 뜻("이 버전을 맞는 작업에 기본 방법으로 제안할 수 있다"), 권한이 생기지 않는다는 안내, 확인 코드를 보여 줍니다. 신뢰하지 않는 자료가 단계에 영향을 줬으면 그 점을 따로 알립니다.
  - 이미 확인된 방법이면 검토 화면 대신 그 사실과 사용을 멈추는 방법(`/learn deactivate`), 거두는 방법(`/learn reject`)을 알려 줍니다.
  - `/learn confirm <id> v<N> <코드>`로 그 화면에서 본 버전만 확인합니다. 코드는 방법이 바뀔 때마다(다른 작업의 확인이 기록될 때도) 달라지므로, 본 뒤에 바뀐 방법은 확인되지 않고 다시 검토하라고 안내합니다.
  - 작업 결과를 받아들이는 말("좋아요")이나 결과를 고른 사실은 확인으로 치지 않습니다.
- **거절:** `/learn reject <id> v<N>`. 거절한 방법은 다시 확인되거나 제안되지 않습니다. 확인한 방법을 거두려면 거절합니다.
- **다른 작업으로 확인(`/learn verify <id>`):** 지금 대화의 작업(최근 사용자 turn 6개와 그 뒤 기록)을 그 방법과 대조합니다. 도구를 주지 않은 model 호출을 두 번 합니다(사용자 메시지 분류와 대조). 대조 호출에는 방법의 목적·적용 작업·전제·근거 있는 단계와, 후보 초안 호출과 같은 범위의 근거(요청·피드백·절차의 글, 도구 이름과 실행 판정, 답변이 있었다는 사실)만 보냅니다.
  - 모델은 이 작업이 그 방법의 작업 종류인지, 맞지 않는 전제가 있는지, 어느 근거가 어느 단계를 다시 했는지, 어느 피드백이 어느 단계와 어긋나는지 답합니다. 작업 종류와 전제가 맞는지는 모델의 판단입니다.
  - 무엇을 세는지는 runtime이 정합니다. `your own words` 단계는 그 단계 문장이 그대로 든 이 작업의 피드백·절차 메시지로만, `ran without a reported error` 단계는 그 단계의 도구가 이 작업에서 모두 오류 없이 실행된 호출로만 다시 한 것으로 봅니다. 없는 근거 ID, 실패하거나 끝나지 않은 실행, 요청 메시지, 결과를 받아들이는 말은 세지 않습니다.
  - **새 작업:** 이 작업의 요청 글, 사용자가 쓴 메시지(글과 시각, 모델이 어떻게 분류했든), 도구 결과의 digest가 그 방법의 원래 작업이나 이미 기록된 다른 작업 어느 것과도 겹치지 않아야 합니다. 원래 작업의 것은 후보를 만들 때 근거 창 전체(인용하지 않은 메시지와 도구 결과 포함)에서 계산해 저장합니다. 신뢰하지 않는 자료가 단계에 영향을 준 방법은, 같은 출처(같은 인자로 부른 호출)를 다시 읽은 작업도 새 작업으로 보지 않습니다. 근거 있는 단계를 모두 다시 한 새 작업이 기록되면, 서로 다른 두 작업(원래 작업과 이 작업)에서 확인된 것으로 보고 `confirmed`가 됩니다. 그 전에 어긋나는 피드백이 기록됐으면 이렇게 확인되지 않습니다(사용자가 직접 확인할 수는 있습니다).
  - **다시 한 작업(재시도·재요청·같은 자료):** 새 작업이 아니면 확인 근거로 세지 않고, 이 작업이 다시 한 단계만 "rerun … kept as stability evidence only"로 남깁니다(다시 하지 않은 단계도 표시). 이미 기록된 같은 메시지·실행만 보여 주면 아무것도 저장하지 않습니다.
  - **어긋남:** 이 작업(원래 작업 포함)에서 사용자의 피드백·절차가 단계와 어긋난다고 판단되면 기록합니다. 모델이 이 작업은 그 방법의 작업 종류가 아니라거나 맞지 않는 전제가 있다고 답해도 기록합니다. `confirmed`였으면 `inactive`가 되어 사용자가 다시 확인할 때까지 제안되지 않고, 후보였으면 이후 다른 작업으로는 확인되지 않습니다. 같은 피드백은 한 번만 기록합니다.
  - 결정마다 버전, 시각, 그 작업의 session digest, 단계별 근거와 그 근거의 기록(digest·출처만)을 남깁니다. 볼 때마다 그 session의 지금 기록과 방법의 근거처럼 다시 대조하고, 확인을 만든 작업의 근거가 지워졌으면 상태 옆에 "(the task it was redone in is no longer in its session, not rechecked)"를 붙입니다.
- **맞는 방법 보기(`/learn match <요청>`):** 확인된 방법만 도구 없는 model 호출 한 번으로 그 요청에 맞는지(작업 종류와 전제)를 판단해 "Offered as the default"로 보여 줍니다. 요청이 결과의 모양을 정했다고 판단되면, 결과의 모양을 정하는 단계는 "left out"으로 표시합니다(아래 사용과 같은 기준). 후보·거절·사용 중지 방법은 model에 보내지 않고, 제안하지 않는 이유와 함께 나열합니다. 확인된 방법이 없으면 model을 부르지 않습니다. 아무것도 실행하지 않습니다.
- **결정 수:** 방법마다 결정은 64개, 버전은 64개까지 남습니다. 결정이 다 차면 확인·거절·다른 작업 기록·고치기·사용 멈추기·되돌리기를 하지 않고, 버전이 다 차면 고치기를 하지 않으며, 그렇다고 알려 줍니다.
- **저장 검사:** 읽을 때마다 결정의 형식, 다른 작업 기록마다 그 앞의 기록에 비춰 새 작업·다시 한 작업·어긋남의 조건에 맞는지, 저장된 상태가 결정으로 다시 계산한 상태와 같은지 확인하고, 어긋나면 "malformed method record"로 거절하며 파일은 그대로 둡니다.
- **한계:**
  - 같은 작업인지는 요청 글·사용자 메시지·도구 결과의 digest로만 봅니다. 표현을 바꿔 다시 요청하고 자료도 달라졌으면 새 작업으로 셀 수 있습니다. 반대로 근거 창에 앞 작업의 요청이 남아 있으면, 같은 대화의 다른 요청도 다시 한 작업으로 봅니다.
  - 같은 프로그램 실행 안에서 이미 읽은 바뀌지 않은 파일을 다시 읽으면 `read_file`이 내용 대신 바뀌지 않았다는 안내만 돌려주는데, 이런 읽기는 끝나지 않은 실행이라 단계를 다시 한 것으로 세지 않습니다.
  - 작업 종류·전제가 맞는지, 어긋나는 피드백이 있는지는 모델의 판단입니다. 모델이 어긋남을 놓치면 기록되지 않습니다.
  - 저장 파일은 인증된 기록이 아닙니다. 읽을 때 결정의 형식과 앞뒤 조건만 검사하므로, 저장 파일만 고쳐도(예: 셸 명령을 승인해 `confirmed` 결정과 상태를 넣으면) 사용자 확인으로 보이게 만들 수 있습니다. 확인 코드는 사용자가 확인 화면을 봤다는 표시일 뿐 비밀번호나 서명이 아니고, 사용자 확인은 session 기록과 대조하지 않습니다. 다른 작업의 확인도 근거가 그 session에 남아 있을 때만 다시 대조하므로, 근거가 없는 session을 가리키게 꾸민 기록은 "not rechecked" 표시로만 드러납니다. 위의 보호는 모델의 파일 도구가 저장 파일을 바꾸는 것을 막을 뿐입니다.

### 확인한 방법의 사용·수정·되돌리기

```text
/learn edit method-1 v1 purpose <새 목적>
/learn edit method-1 v1 applies <새 적용 작업>
/learn edit method-1 v1 premise 2 <새 전제>
/learn edit method-1 v1 step 3 <새 단계>
/learn edit method-1 v1 remove step 3
/learn deactivate method-1 v1
/learn restore method-1 v1
/learn show method-1 v1
```

- **다음 요청에서의 사용:** 사용자가 보낸 일반 요청마다(명령·승인 답·system·cron·automation turn은 아님), 확인된 방법이 하나라도 있으면 그 요청을 실행하기 전에 다음을 합니다. 확인된 방법이 없으면 아무것도 하지 않고 model도 부르지 않습니다.
  1. 저장된 방법을 읽고 원래 session과 다시 대조합니다(`/learn list`와 같은 검사). 저장소가 잘못됐거나 대조가 어긋나면 어떤 방법도 쓰지 않고, 답 끝에 "No learned method was used: …"와 이유를 붙입니다.
  2. 도구 없는 model 호출 한 번으로 확인된 방법들이 그 요청에 맞는지(작업 종류, 맞지 않는 전제), 요청이 결과의 모양을 정했는지, 어느 단계가 결과의 모양(필드, 순서, 구성, 파일 형식)을 정하는지 묻습니다. `/learn match`와 같은 호출입니다.
  3. 맞는 방법이 있으면 그중 첫 번째 하나만 씁니다. 현재 버전의 목적·적용 작업·전제·단계를 단계별 근거 표시(사용자 말, 실행, 사용자 수정, 확인되지 않은 모델 문장, 신뢰하지 않는 자료의 영향)와 함께 그 요청의 system prompt에 덧붙입니다. "작업 방식의 안내일 뿐 권한이 아니며, 결과의 모양은 요청을 따른다"는 지시도 함께 붙습니다.
  4. 답 끝에 "Method used: <id> v<N> — <상태> — as a guide only; it grants no permission."과 적용 작업, model이 성립한다고 본 전제, 근거가 나온 session을 붙입니다. 근거가 지워지거나 바뀌어 다시 대조하지 못한 방법이면 "Some sources it rests on changed or were cleared since, so they were not rechecked"도 붙입니다. 맞는 방법이 없으면 "No learned method was used for this request:"와 방법별 이유(전제가 맞지 않음, 작업 종류가 다름 등)를 붙이고 일반 경로로 진행합니다.
  - Spec 024 workflow로 실행되는 요청(자동 선택이나 metadata로 들어온 workflow)도 새 workflow를 시작할 때 같은 방식으로 고르고, 안내를 workflow의 각 하위 실행(child·verifier)의 system prompt에 붙입니다. workflow가 범위를 묻거나 정책으로 막혀 시작하지 않으면 방법을 고르지 않습니다. 재시작 뒤 이어지는 workflow는 처음 고른 것을 그대로 씁니다.
- **버전 결속:** 요청이 시작할 때 고른 버전과 안내 문장을 그 session에 두었다가, 승인이나 질문의 답으로 실행이 이어질 때도 같은 버전을 씁니다. 그사이 방법을 고치거나 거절해도 이미 시작한 요청은 바뀌지 않습니다.
  - 요청이 끝나면 사용자가 받는 답 끝에 위 표시 문장을 붙입니다. 끝나는 경우는 답, provider 오류, workflow 완료·실패, 그리고 기다리던 요청이 더 기다릴 것이 없어지는 명령 답입니다. 명령 답의 예는 승인 거절, 프로젝트 권한 저장 실패, 승인한 도구의 실패, `/stop`으로 취소한 행동 결정입니다. 대화 기록에도 그 답을 표시 문장과 함께 남기고, 기록에 없던 답(예: "Tool execution cancelled.")이면 새 답으로 남깁니다.
  - 요청이 아직 승인·답·행동 결정을 기다리는 동안 다른 명령(예: `/learn`, 다른 거절 기록의 재시도)에 답해도 그 요청은 끝나지 않습니다. `/new`는 대화와 함께 기다리던 요청의 선택도 버리며, 그 요청에는 표시하지 않습니다.
  - 쓴 방법이면 그 답에 방법의 ID·버전·revision·버전 내용의 sha256·당시 상태와 요청이 끝난 이유(`ended`, 예: `completed`, `permission_denied_by_user`, `provider_error`, `workflow_completed`)를 남깁니다. 방법을 나중에 바꿔도 이 기록은 그대로입니다.
  - 승인·질문·workflow를 기다리는 동안에는 표시하지 않고, 이어진 실행이 끝날 때 표시합니다.
- **결과의 모양:** 방법은 일하는 방식만 바꿉니다. model이 요청이 결과의 모양을 정했다고 판단하면, 결과의 모양을 정한다고 본 단계는 안내에서 빼고 답 끝에 "Left out: step N (your request says how the result should look)."를 붙입니다. 요청이 모양을 정하지 않았으면 방법의 모양을 기본으로 안내합니다. 어느 단계가 모양을 정하는지, 요청이 모양을 정했는지는 model의 판단입니다.
- **권한:** 방법 사용은 어떤 도구도 허락하지 않습니다. 안내를 받은 요청에서도 명령 실행·파일 쓰기·Python·plugin·외부 발송은 그때의 승인 경계를 그대로 거칩니다(예: 확인된 방법을 쓰는 요청의 `write_file`도 승인을 묻고, 승인 전에는 실행되지 않습니다).
- **고치기(`/learn edit <id> v<N> …`):** 현재 버전을 고친 새 버전을 만듭니다. `purpose`·`applies`는 바꾸고, `premise <k>`·`step <k>`는 k번째를 바꾸거나(마지막 번호+1이면 더하기), `remove premise|step <k>`는 뺍니다. 단계는 하나 이상 남아야 합니다.
  - 새 글은 redaction 뒤 400자까지이며, 사용자 수정("your edit in v<N>")으로 표시됩니다. 사용자 수정은 근거를 인용하지 않습니다. 근거 기록(digest·출처)은 고치기 전 버전의 것을 그대로 둡니다.
  - 새 버전은 현재 버전이 되고 후보로 시작합니다. 확인하기 전에는 쓰이지 않습니다. 응답에 새 버전의 검토 화면과 확인 코드가 바로 나옵니다.
  - `v<N>`이 현재 버전이 아니면(그사이 다른 수정이 있었으면) "is at v<M>, not v<N>"로 거절하고 아무것도 바꾸지 않습니다. 두 수정이 동시에 와도 저장소 lock 안에서 차례로 확인하므로 하나만 저장됩니다.
  - 다른 작업으로 확인(`/learn verify`)할 때, 사용자 수정 단계는 그 단계 문장이 그대로 든 그 작업의 피드백·절차 메시지가 있어야 다시 한 것으로 봅니다.
- **사용 멈추기(`/learn deactivate <id> v<N>`):** 확인된 현재 버전을 `inactive`로 바꿉니다. 다시 확인할 때까지 쓰이지 않습니다. 거절과 달리 다시 확인할 수 있습니다.
- **되돌리기(`/learn restore <id> v<N>`):** 확인된 적이 있고 거절되지 않은 이전 버전을 다시 현재 버전으로 만듭니다. 그 버전의 상태는 그 버전의 결정대로입니다(확인된 채였으면 `confirmed`, 사용을 멈췄거나 어긋남이 기록됐으면 `inactive`). 거절한 버전은 되돌릴 수 없습니다. 되돌리기는 앞으로의 요청이 쓸 버전만 바꿉니다. 이미 답한 요청은 쓴 버전 그대로 남고, 그 요청이 한 일(쓴 파일, 보낸 메시지)은 되돌리지 않습니다.
- **버전 보기(`/learn show <id> v<N>`):** 이전 버전도 내용과 그 버전의 상태를 보여 주고, 현재 버전이 아니면 "not the current version"을 붙입니다. 결정 목록에는 고치기("edited by you"), 사용 멈추기("put out of use by you"), 되돌리기("made current again by you")도 보입니다.
- **저장 검사:** 읽을 때마다 버전과 결정이 한 흐름인지 확인합니다. v1은 생성된 것이고, 이후 버전은 그때의 현재 버전을 고친 것이어야 하며 근거 기록이 같아야 합니다. 되돌리기는 그때 확인된 적이 있고 거절되지 않은 버전만 가리켜야 하고, 사용 멈추기는 그때 확인된 버전에만 있어야 합니다. 다른 결정은 그때의 현재 버전에 대한 것이어야 하고, 저장된 현재 버전과 상태는 이 흐름의 끝과 같아야 합니다. 어긋나면 "malformed method record"로 거절하며, 그동안 어떤 방법도 쓰지 않습니다.
- **한계:**
  - 맞는지, 전제가 성립하는지, 결과의 모양을 누가 정했는지는 model의 판단입니다. 안내를 따를지도 실행하는 model에 달려 있으며, runtime은 안내를 붙이고 그 사실을 표시할 뿐 단계가 실제로 지켜졌는지 확인하지 않습니다.
  - 한 요청에 방법 하나만 씁니다. 맞는 방법이 더 있으면 "Also fitting, not used"로만 알립니다.
  - 확인된 방법이 있으면 일반 요청마다 저장소를 읽어 대조하고 model 호출이 한 번 더 듭니다.
  - 실패 피드백은 기존 방법을 자동으로 고치지 않습니다. 새 후보(`/learn`)나 사용자 수정의 근거가 될 뿐이고, `/learn verify`가 어긋남을 기록하면 확인된 방법은 사용을 멈춥니다.
  - 저장 파일이 인증된 기록이 아니라는 위 한계는 버전과 결정에도 같습니다.

## Tasks 조회와 owner 변경 요청

Spec 035의 Tasks CLI, local API, TUI는 같은 `Spec035TasksProjection`을 소비합니다. Goal, child, workflow, automation, recovery 행은 기존 owner record의 locator·freshness·상태·goal budget을 보존하며 별도 task DB를 만들지 않습니다. Owner 자료 누락은 `unavailable`로 표현하며 빈 행을 성공으로 해석하면 안 됩니다. 손상된 durable replay에 child state가 없으면 child coverage는 `unavailable`, 정상 replay의 빈 집합은 `available(0)`으로 구분하도록 보정됐고 F2 재검토에서 확인됐습니다. Recovery 진단은 독립적으로 보존되며 child 수만으로 전체 건강 상태를 판단하지 마세요. Recovery는 data directory 범위의 관측도 포함하므로 session별 작업 목록으로만 해석하지 마세요.

앱 owner는 [Spec 043](specs/043-app-capability-retirement/SPEC.md)에서 퇴역했습니다. Tasks는 앱 행을 만들지 않고 남은 앱 자료도 읽지 않습니다. JSON 형식(schema version 1)을 지키려고 `coverage.app` 키는 남지만 값은 항상 `unavailable`이며, 이는 앱 owner가 퇴역했다는 뜻입니다. TUI의 Tasks 요약 줄에는 app 항목이 없습니다.

```sh
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- tasks --json --workspace /tmp/shacs-ws --data-dir /tmp/shacs-data --session cli:direct
curl --fail-with-body 'http://127.0.0.1:8900/v1/tasks?session_id=cli%3Adirect'
```

`tasks`에는 조회·변경 모두 `--json`이 필수이며 `--session` 기본값은 `cli:direct`입니다. `--workspace`와 `--data-dir`를 함께 지정하면 해당 경로를 직접 사용합니다. `--workspace`만 지정하면 같은 경로를 data directory로 사용하고, `--data-dir`만 지정하면 오류입니다. 둘 다 생략하면 설정에서 경로를 읽습니다. API는 실행 중인 adapter의 workspace/data directory를 사용하므로 CLI와 비교하려면 실제 owner 경로를 맞추세요.

`GET /v1/tasks`의 기본 session은 `api:default`입니다. CLI와 같은 session을 보려면 `session_id=cli%3Adirect`처럼 값 전체를 URL 인코딩하세요. 현재 parser는 `session_id` 하나만 받으며 빈 값, 추가 query 항목, 잘못된 percent encoding을 거부합니다. 이 조회에는 transport hello가 필요하지 않습니다.

변경 요청은 현재 행이 광고한 action과 locator에 한해 기존 owner handler로 전달됩니다. 지원 조합은 goal `pause`/`resume`, recovery `recover`입니다. Child/workflow/automation/app 변경이나 범용 retry/cancel은 Tasks action으로 지원하지 않으며, 이 owner의 요청은 transport 협상이나 owner 조회 전에 `unsupported tasks owner/action pair`로 거부됩니다(API는 400). 퇴역한 app owner의 `stop`/`recover` 요청도 같으며 남은 앱 자료를 바꾸지 않습니다. 아래 `OWNER_LOCATOR`에는 조회한 goal 행의 실제 locator를 넣으세요. 이 예시는 해당 goal을 일시 정지하므로 조회 명령이 아닙니다:

```sh
cargo run --manifest-path crates/Cargo.toml -p shacs-cli -- tasks --json --workspace /tmp/shacs-ws --data-dir /tmp/shacs-data --session cli:direct --owner goal --action pause --locator "$OWNER_LOCATOR" --transport-hello '{"client_id":"client:cli","schema_versions":[1],"mutation_capabilities":["task_pause"]}'
```

`--owner`, `--action`, `--locator`는 모두 필요합니다. 변경에는 `--transport-hello`도 필요하며 지원하지 않는 capability는 owner 접근 전에 `capability_unavailable`, schema 불일치는 `schema_mismatch`로 거부됩니다. 지원 협상 이후에도 현재 owner 행과 action 가용성을 다시 검사하므로 오래된 locator나 더 이상 사용 가능하지 않은 action은 실패합니다.

API에서 협상 결과만 확인하려면 다음 요청을 사용합니다. `task_pause`, `task_resume`, `task_stop`, `task_recover`는 지원하고 `task_retry`는 미지원으로 응답합니다:

```sh
curl --fail-with-body -H 'Content-Type: application/json' -d '{"client_id":"client:api","schema_versions":[1],"mutation_capabilities":["task_pause","task_retry"]}' http://127.0.0.1:8900/v1/transport/hello
```

실제 변경은 `POST /v1/tasks/actions`에 아래 JSON을 보냅니다. `locator` 예시를 현재 조회 결과로 교체하세요. `transport_hello`는 매 요청에 포함하며 앞선 `/v1/transport/hello` 호출이 인증 세션이나 권한을 만들지는 않습니다:

```json
{
  "owner": "goal",
  "action": "pause",
  "locator": "goal:replace-with-current-owner-locator",
  "session_id": "cli:direct",
  "transport_hello": {
    "client_id": "client:api",
    "schema_versions": [1],
    "mutation_capabilities": ["task_pause"]
  }
}
```

API 변경에는 `shacs-bot --config /tmp/shacs-data/config.json serve --workspace /tmp/shacs-ws --bind 127.0.0.1:8900 --allow-api-side-effects`로 활성화한 별도 loopback opt-in이 필요하며 없으면 403입니다. 이 option은 다른 API side effect도 허용하므로 조회만 할 때 켜지 마세요. Opt-in gate를 통과한 뒤 hello 누락·잘못된 요청·schema 불일치는 400, 지원되지 않는 mutation capability는 422입니다. `/v1/transport/hello` 자체는 mutation opt-in 없이 조회할 수 있습니다. **Capability는 호환성 정보이지 인증, permission approval, sandbox 증거가 아닙니다.** 현재 local API에는 인증이 없으며 owner 검증과 기존 권한 경계도 없어지지 않습니다.

### Reconnect와 전달 관측의 한계

API WebSocket은 stable client/session을 지정하면 Tasks snapshot을 먼저 보내는 계약입니다. 문서 예시 `/ws?client_id=client%3Aapi&session_id=cli%3Adirect`는 [최종 compiled QA](../.omo/evidence/spec035/closure/final-compiled-qa.json)에서 새 바이너리의 실제 연결·재연결로 snapshot generation 1·2, sequence 1, reconnect gap false·true를 확인했습니다. [과거 F3의 HTTP 400](../.omo/evidence/spec035/closure/f3-manual-qa.json)은 보정 전 역사 기록으로 보존하며 현재 성공 증거로 덮어쓰지 않습니다. SSE chat stream은 `x-shacs-spec035-client-id: client:api`로 이 경로를 선택하며 요청의 session을 동일하게 유지해야 합니다. 이후 delta는 같은 generation의 순서 검사에 따르며 snapshot 전 delta, 오래된 generation, 중복, gap을 별도 거절·관측합니다. 이 snapshot은 연결 초기화용 조회 결과이지 Spec031 execution snapshot이나 새로운 session truth가 아닙니다.

Reconnect tracker와 accounting은 process-local의 제한된 기록입니다. 재시작 후 지속성, 끊긴 구간의 재생, 무손실 전달, remote ACK/read receipt, exactly-once 실행·전달은 보장하지 않습니다. Accepted/emitted/coalesced/dropped 값은 독립적이며 누락 값은 0으로 합성하지 않습니다. Progress drop은 최종 전달 관측으로 상쇄되지 않고, `final_delivered`도 해당 owner 표면의 관측일 뿐 사용자 수신 증거가 아닙니다. SSE 최종 전달은 pending/unknown일 수 있습니다.

이 UI·진단·재연결 기능은 구현되어 있지만, 과거 workspace 검증이나 fixture 성공을 현재 소스의 전체 출시 승인으로 해석하지 않습니다. 최종 실행 증거 조립과 독립 감사는 완료되지 않았으며, 미완료였던 owner 스펙 문서는 현재 스펙 세트에서 제거했습니다. 역사적 실행 근거와 원본 source binding은 보존된 `.omo/evidence/spec035/` 기록의 범위에서만 해석합니다.

F2는 원래 네 결함인 reconnect identity 충돌·이전 generation의 final 관측 덮어쓰기·child 누락 처리·실패 owner transcript 수용과 stale 비동기 queue 역방향 사례의 보정을 제한된 재검토 범위에서 `confirmed`로 기록했습니다. 이는 wizard/runner 변경이나 전체 release 승인이 아닙니다. 과거 F3의 `FAIL_DOCUMENTED_WEBSOCKET_RECONNECT_URL`과 이전 `BLOCKED_BUILD_REQUIRED`는 역사적 시도이며, 최종 compiled QA는 encoded query와 CLI/API/TUI smoke, REPL·wizard 취소를 별도로 통과했습니다. [F1-R01/R01-A01 재검토](../.omo/evidence/spec035/closure/final-r01-recheck.json)는 실행 증거 수용 구현의 제한적 확인일 뿐 전체 F1 승인이 아닙니다. 전체 의미 증거와 최종 독립 승인 gate는 남아 있고 F4의 `finalApproval=false`도 덮어쓰지 않습니다.

## Codex provider 인증

Codex request/stream 지원은 provider id `openai_codex` 아래에 구현되어 있습니다. 인증은 `config.json` 옆의 OpenCode-style `auth.json` 파일을 사용합니다.

아래 `--config` 없는 인증 예시는 기본 사용자 저장소를 대상으로 합니다. 별도 instance에 인증하려면 예를 들어 `shacs-bot --config /tmp/shacs-data/config.json provider codex login --no-browser`처럼 지정하세요. Token import도 `shacs-bot --config /tmp/shacs-data/config.json provider codex import-token --token-env CODEX_TOKEN --no-select`처럼 같은 config를 지정합니다. `--no-select`는 provider/model 선택을 유지하는 옵션이지 auth write를 금지하는 옵션이 아닙니다. Provider 인증은 transport hello나 local API 접근 인증과 별개이며 문서·UI QA를 위해 실제 token을 import할 필요는 없습니다.

브라우저 OAuth login을 시작합니다:

```sh
shacs-bot provider codex login
```

브라우저를 자동으로 열 수 없는 terminal에서는 URL을 출력하고 localhost callback을 수동으로 완료합니다:

```sh
shacs-bot provider codex login --no-browser
```

Headless 환경에서는 device flow를 사용합니다:

```sh
shacs-bot provider codex login --headless
```

Login에 성공하면 `auth.json`에 `access`, `refresh`, `expires`, optional `accountId`를 저장하고, provider를 `openai_codex`, model을 `gpt-5.4`로 선택합니다. Runtime startup은 refresh token이 있으면 만료된 Codex access token을 갱신하고, 갱신된 session을 다시 `auth.json`에 씁니다.

`gpt-5.4`는 ChatGPT account Codex의 보수적인 기본값입니다. `gpt-5.5` 같은 더 새로운 Codex model slug는 계정 rollout 또는 entitlement가 필요할 수 있습니다. `openai/gpt-5.5` 같은 provider-qualified id는 ChatGPT Codex backend로 보내기 전에 정규화됩니다.

같은 Codex OAuth 인증으로 `image_generate`를 사용하려면 `config.json`에 다음 설정을 추가합니다:

```json
{
  "tools": {
    "imageGeneration": {
      "enable": true,
      "provider": "openai_codex",
      "model": "gpt-image-2",
      "defaultFormat": "png",
      "maxCount": 1
    }
  }
}
```

`openai_codex` image provider는 `auth.json`의 OAuth access token과 optional `accountId`를 사용해 Codex backend의 `/codex/images/generations`를 호출합니다. 이미지 인증은 `image_generate`를 실행할 때마다 공통 provider credential runtime에서 다시 해석되므로, access token 교체와 OAuth refresh는 runtime이나 tool registry를 다시 시작하지 않아도 다음 호출부터 반영됩니다. 로그아웃으로 등록된 인증을 제거하면 다음 호출은 이전 token을 재사용하지 않고 인증 필요 오류를 반환합니다. 생성 결과는 raw base64가 아니라 data directory 아래 local media artifact로 저장됩니다. 이 경로는 Codex account plan과 server-side image-generation entitlement에 따라 거절될 수 있고, 공개 OpenAI Images API의 안정성 계약을 따르지 않습니다. 공개 API가 필요한 경우 기존 `openai` image provider와 `OPENAI_API_KEY`를 사용하세요.

stdin에서 token을 import합니다:

```sh
printf '%s' "$CODEX_TOKEN" | shacs-bot provider codex import-token --token-stdin
```

환경 변수에서 token을 import합니다:

```sh
shacs-bot provider codex import-token --token-env CODEX_TOKEN --account-id acct_123
```

Codex `import-token`은 fallback으로 유지됩니다. Provider 선택/config metadata는 `config.json`에 쓰지만 bearer token은 config 옆 `auth.json`에만 저장합니다. Auth file은 provider-keyed OAuth entry를 사용하며 `type`, `access`, optional `refresh`, optional `expires`, optional `accountId` 같은 field를 가집니다. Unix에서는 secret-file permission으로 작성됩니다. Command output은 path와 status만 출력하며 token은 출력하지 않습니다. 기본적으로 import는 configured model을 `gpt-5.4`로 선택합니다. Auth만 저장하고 provider/model 선택을 바꾸지 않으려면 `--no-select`를 사용하세요.

Copilot provider bearer token도 같은 import-token 형식으로 가져올 수 있습니다:

```sh
printf '%s' "$COPILOT_TOKEN" | shacs-bot provider copilot import-token --token-stdin
shacs-bot provider copilot import-token --token-env COPILOT_TOKEN
```

Copilot import도 token은 config 옆 `auth.json`에 저장하고 command output에 raw token을 표시하지 않습니다.

Generic provider API key는 아래처럼 `import-key`로 가져옵니다:

```sh
shacs-bot provider import-key --provider openrouter --token-env OPENROUTER_API_KEY
```

이 경로도 secret은 `auth.json`에만 저장하고 `config.json`에는 provider 설정만 남깁니다.

## 로컬 OpenAI 호환 API

로컬 API server를 시작합니다:

```sh
shacs-bot serve --bind 127.0.0.1:8900 --workspace /tmp/ws --timeout 120
```

이전 API 중심 command shape와의 호환을 위해 `api serve`는 같은 command의 alias입니다:

```sh
shacs-bot api serve --bind 127.0.0.1:8900 --workspace /tmp/ws --timeout 120
```

기본 bind address는 JSON config의 API section에서 오며 기본값은 `127.0.0.1:8900`입니다. 한 번의 실행에서만 바꾸려면 `--bind <host:port>` 또는 `--host <ip> --port <port>`를 사용하세요.

로컬 API는 인증이 없으므로 non-loopback bind에는 명시적 opt-in이 필요합니다:

```sh
shacs-bot serve --bind 0.0.0.0:8900 --allow-remote --workspace /tmp/ws
```

API turn은 기본적으로 read/search/web tool을 사용합니다. Write/edit/exec/self-modifying tool은 다음 option이 필요합니다:

```sh
shacs-bot serve --allow-api-side-effects --workspace /tmp/ws
```

구현된 endpoint:

- `GET /health`
- `GET /v1/readiness`
- `GET /v1/models`
- `GET /v1/diagnostics`
- `GET /v1/tasks?session_id=cli%3Adirect`
- `POST /v1/tasks/actions`
- `POST /v1/transport/hello`
- `GET /v1/media/diagnostics` (호환용으로 남은 경로이며 항상 404 `media projection is unavailable`)
- `GET /v1/permissions`
- `GET /v1/workflows/recipes`
- `GET /v1/sessions`
- `GET /v1/sessions/{session}`
- `GET /v1/sessions/{session}/history`
- `GET /v1/sessions/{session}/diagnostics`
- `GET /v1/sessions/{session}/goal-snapshot`
- `POST /v1/sessions/{session}/goal/set`
- `POST /v1/sessions/{session}/goal/pause`
- `POST /v1/sessions/{session}/goal/resume`
- `POST /v1/sessions/{session}/goal/clear`
- `POST /v1/sessions/{session}/goal/done`
- `POST /v1/sessions/{session}/goal/blocked`
- `POST /v1/improvements/{proposal}/propose`
- `GET /v1/improvements/{proposal}/inspect`
- `POST /v1/improvements/{proposal}/apply`
- `POST /v1/improvements/{proposal}/verify`
- `GET /v1/improvements/{proposal}/candidate`
- `POST /v1/improvements/{proposal}/rollback`
- `POST /v1/chat/completions`
- `GET /ws` 또는 configured WebSocket path when the server is started with WebSocket support

`POST /v1/chat/completions`는 단일 user message, optional `session_id`, optional `temperature`, optional `max_tokens`, JSON text 또는 data-URL image content part, multipart upload, non-stream response, `stream=true` Server-Sent Events를 받습니다. Remote image URL은 거부합니다. Data URL과 uploaded file은 runtime media directory의 `attachments/api/` subtree 아래에 저장되며, 파일당 10 MiB 제한이 있습니다. 저장된 attachment는 provider/model이 native image input을 지원한다고 확인되는 경우 image block으로 라우팅되고, text/PDF/Office 계열은 가능한 경우 text note와 추출 텍스트로 라우팅됩니다. Provider나 model이 image input을 지원하지 않는 경우 image attachment는 raw 경로를 노출하지 않는 unsupported note로 전달됩니다. Audio attachment는 지원되는 analyzer가 runtime에 주입된 경우 bounded transcript 또는 summary text artifact로 라우팅되고, analyzer가 없거나 지원되지 않으면 내용을 들은 것처럼 처리하지 않고 unsupported 또는 extraction_failed note로 남습니다. Video attachment는 저장되지만 내용을 분석하지 않습니다. 길이와 형식에 관계없이 `video content analysis is not supported` reason의 unsupported note 하나로 모델에 전달되며, 영상을 조용히 빼거나 본 것처럼 다루지 않습니다. 기본 ffmpeg, built-in codec parser, native outbound video delivery는 제공하지 않습니다.

### Spec 034 generated media와 media diagnostics

Agent runtime의 approved `image_generate` path는 Codex Responses image event를 text와 분리하고 final bytes가 local media root에 commit된 뒤 relative artifact ref만 runtime result에 남깁니다. Provider-neutral edit/mask/variation 계약은 존재하지만 현재 capability는 provider별로 다르며 unsupported operation은 provider 호출 전에 명시적으로 거부될 수 있습니다. Partial image는 상태 evidence일 뿐 final artifact로 자동 승격되지 않습니다. 생성된 image artifact 목록은 `runtime inspect`에서 봅니다:

```sh
shacs-bot runtime inspect --workspace /tmp/ws
```

`runtime inspect`, `runtime diagnostics`, `GET /v1/media/diagnostics`, WebSocket `/ws`의 `{"type":"media_projection"}`, live TUI와 외부 채널은 영상 분석 기록(`<data dir>/media/projections/current.json`)을 읽지 않습니다([Spec 044](specs/044-video-analyzer-retirement/SPEC.md)). `runtime inspect`는 `Spec035 media projections: unavailable (canonical records unavailable)`을, `runtime diagnostics`의 진단 JSON은 빈 `spec035_media_projections`를, API는 404 `media projection is unavailable`을, WebSocket은 같은 not found 오류를, TUI는 session마다 `media: state=unavailable reason=unavailable freshness=unavailable`, `media lineage: unavailable`, `media disclosure: unavailable` 세 줄을 보여줍니다. 외부 채널 전송에는 media metadata가 붙지 않습니다. 이전 버전이나 코드로 주입한 analyzer가 남긴 기록 파일이 있어도, 손상돼 있어도 결과는 같습니다.

Generated artifact는 selected media root의 `artifacts/<artifact-id>/` 아래 `record.json`과 payload로 atomic commit되며 record는 relative path, MIME, byte length, SHA-256, provider/model, source ids, normalized options, retention과 disclosure status를 보존합니다. Remote provider output은 guarded local persistence, provider/domain/expiry만 담은 non-persisted reference, rejection 중 하나이며 arbitrary user URL intake가 아닙니다. Remote reference는 영구 접근이나 재다운로드를 보장하지 않습니다.

Media-root admission은 universal filesystem/process containment가 아니고, projection omission은 complete redaction이나 secret exfiltration prevention proof가 아닙니다. CDN/public gallery/editor, built-in ffmpeg, full codec coverage와 all-provider parity도 지원 범위가 아닙니다. Spec 034 종료 당시의 22/22 mapping, [review remediation PASS](../.omo/evidence/spec034/remediation/PASS.json), [Todo 14 QA baseline](../.omo/evidence/spec034/task-14-final-qa-candidate2/PASS.json)은 과거 구현 기록입니다. [Spec 044](specs/044-video-analyzer-retirement/SPEC.md) 이후 `spec034-release-runner`는 현재 요구사항 13개만 coverage로 보고하고, video analyzer·분석 기록 표면·media evidence replay에 근거했던 9개(`034-MH007`–`034-MH010`, `034-AC007`–`034-AC009`, `034-AC011`, `034-AC012`)는 퇴역으로 따로 기록합니다. 최신 source-bound release 상태는 [canonical final manifest](../.omo/evidence/spec034/task-15-closure/final-committed/manifest.json)가 현재 committed source와 최종 5개 리뷰를 결합해 판정하며, manifest가 없거나 검증에 실패하면 final seal이 아닙니다. 전체 non-guarantee와 판정 계약은 [Spec 034 closure record](specs/034-generated-media-and-rich-file-context-expansion/CLOSURE.md)를 참고하세요.

[Spec 040](specs/040-video-analyzer-capability-disposition/SPEC.md#판정-결과-2026-10-24)에서 video analyzer 기능을 제거하기로 판정했고 [Spec 044](specs/044-video-analyzer-retirement/SPEC.md)가 이를 구현했습니다. 영상 첨부는 지금처럼 저장되고 모델에는 영상마다 `video content analysis is not supported` unsupported note 하나가 전달됩니다. 영상 내용을 분석하거나 분석 기록을 쓰지 않습니다. 저장된 영상 첨부와 이전 버전이 남긴 `<data dir>/media/projections/`, `<workspace>/.shacs-video-analyzer-staging/`은 지우거나 고치지 않고 더는 읽지도 않으므로, 필요 없으면 직접 지워도 됩니다. 지울 때는 다음을 확인하세요.

- `<data dir>`는 config 파일이 있는 폴더입니다. 다른 data dir을 쓰는 실행이 있다면 그 폴더도 따로 확인합니다.
- `media/projections/`와 `.shacs-video-analyzer-staging/`은 이전 video analyzer만 쓰던 폴더라 통째로 지워도 됩니다.
- 영상 첨부가 있는 `<data dir>/media/attachments/<channel>/`에는 이미지·문서·오디오 첨부도 함께 있습니다. 영상 파일만 골라 지우고, 처리 중인 요청이 없을 때 지우세요.

영상 내용을 다뤄야 하면 화면을 캡처한 이미지를 이미지 입력을 지원하는 모델에 보내세요.

`spec034-release-runner`가 성공하면 runner가 방금 committed publication에서 반환한 lowercase `sha256:<64 hex>` digest 한 줄만 stdout에 쓰고 stderr는 비웁니다. CLI는 publication 경로를 다시 열어 별도 self-consistency audit으로 출력값을 교체하지 않습니다. 실패하면 stdout은 비어 있고 stderr는 `spec034 release runner failed: `로 시작하며 non-zero로 종료합니다.

Runner의 검증된 Rust tool closure cache는 Cargo target 밖의 사용자 cache에 영속됩니다. Darwin 기본값은 `~/Library/Caches/shacs-bot/spec034-release`입니다. 테스트나 일회성 격리 실행에서는 `SHACS_SPEC034_RELEASE_CACHE_ROOT`에 사용자 소유 절대 경로를 지정하세요. Cargo target 내부 경로, symlink leaf, repository 소유자와 다른 cache root는 거부됩니다. 이 cache에는 검증된 tool binary와 digest manifest만 들어가며 credential, environment secret, run-local source와 Cargo output은 저장되지 않습니다. Run-local copy는 계속 격리된 temporary root에 생성되고 실행 후 제거됩니다.

이 digest의 structural audit는 expected manifest/run ID/content digest 결합을 검사하지만 execution attestation 자체는 아닙니다. `success-fixture`는 runner mechanics만 검증하며 Spec034 closure가 아니고, dirty current-worktree 결과는 provenance일 뿐 final closure가 아닙니다. Darwin APFS vnode ledger와 suspended child의 static/live CDHash 검증에서 감지된 tool/runtime tamper는 fresh attestation과 publication을 fail closed합니다. Cleanup은 root와 nested pathname identity를 destructive unlink 직전에 다시 확인하고 감지된 교체나 event 불일치에서 publication을 차단합니다. 그러나 지속적으로 악의적인 same-UID process가 identity 확인과 unlink syscall 사이에 pathname을 다시 교체할 수 있으므로 unrelated replacement의 원자적 보존과 isolation root의 보장된 회수는 제공하지 않습니다. Public-API poll 사이의 double-fork/setsid 후 reparent 원자적 추적, zero-instruction prevention, universal sandbox/process containment도 보장하지 않습니다.

`GET /v1/readiness`는 Spec 035 readiness projection을 반환합니다. `GET /v1/diagnostics`는 redacted runtime diagnostics snapshot과 supervision projection을 반환합니다. `GET /v1/permissions`는 CLI `permissions list`와 같은 remembered permission projection을 반환하고 mutation method는 거부합니다. `GET /v1/workflows/recipes`는 CLI `skills recipes`와 같은 `024WorkflowRecipeProjection.v1` read-only projection을 반환합니다. `/v1/sessions` family는 configured workspace의 session list, detail, filtered message history, diagnostics를 raw session file이나 provider payload 없이 조회하는 read-only local-owner surface입니다. Filtered history는 user/assistant의 `role`/`content`만 반환하고 provider hidden reasoning, tool call arguments, tool result payload는 반환하지 않습니다. 그래도 user/assistant message content 자체는 포함될 수 있으므로 non-loopback/remote bind는 reverse proxy/auth 같은 별도 보호 없이는 권장하지 않으며, 로컬 API bind 범위와 로그 보관 정책을 그에 맞게 다루세요. `/ws`는 JSON `message` frame을 local `AgentLoop`로 전달하고 `delta`, `stream_end`, final `message`, attach/ready/error event를 반환하는 WebSocket bridge입니다. Server-Sent Events의 final delivery는 remote ACK/read receipt가 아니라 local stream state이며, current projection에서 pending 또는 unknown으로 남을 수 있습니다.

같은 session key의 API request는 CLI/channel runtime과 같은 process-local `SessionTurnLock`으로 직렬화됩니다. `--timeout`은 HTTP wait timeout을 제어합니다. Timeout response가 반환되어도 in-flight turn은 blocking `AgentLoop` 작업이 끝날 때까지 해당 session lock을 계속 소유합니다.

## 채널

로컬 channel registry와 configured channel plugin 설정을 확인합니다:

```sh
shacs-bot channels list
shacs-bot channels status --workspace /tmp/ws
```

`channels list`는 built-in channel descriptor, config-enabled 상태, capability, worker boundary 개수를 보여줍니다. `channels status`는 configured channel plugin과 `channels.sendMemoryHints`, send retry count 같은 runtime default, channel restart hint projection, redacted supervision projection을 요약합니다. Delivery projection status는 `pending`, `sent_hint`, `failed_hint`, `unknown`, `dedupe_candidate`이며 session truth, remote ACK/read receipt, replay 방지, exactly-once delivery를 의미하지 않습니다. `sendMaxRetries`는 `ChannelManager`가 channel adapter로 메시지를 넘기는 dispatch/enqueue와 실제 transport send의 총 시도 횟수이며, 값은 최소 1회, 최대 10회로 제한됩니다. 이 명령들은 read-only diagnostics입니다. Runnable channel worker를 시작하려면 `run`을 사용하세요.

선택된 channel runtime을 시작합니다:

```sh
shacs-bot run --workspace /tmp/ws
shacs-bot run --websocket-host 127.0.0.1 --websocket-port 8765 --workspace /tmp/ws
```

`run`은 `websocket` channel이 enabled이면 WebSocket channel server를 시작합니다. 들어오는 JSON text 또는 binary WebSocket frame은 channel contract를 통해 normalize되고, 로컬 `AgentLoop`에서 처리된 뒤 WebSocket channel adapter와 `ChannelManager` dispatch 정책을 거쳐 WebSocket server event로 반환됩니다. Provider text delta는 너무 잘게 나가지 않도록 coalesce되어 `delta` event로 전달되고, turn 종료 시 `stream_end` event가 뒤따릅니다. 최종 assistant answer는 backward compatibility를 위해 기존 `message` event로도 유지됩니다. WebSocket server는 adapter event를 bounded queue로 socket writer에 넘기므로 느린 client에는 backpressure가 적용되고, client disconnect 시 해당 connection의 event delivery를 중단합니다. WebSocket config는 `channels.websocket`의 `enabled`, `host`, `port`, `path`에서 읽습니다. Command-line `--websocket-host`와 `--websocket-port`는 한 번의 실행에서 host/port를 override합니다. Non-loopback WebSocket bind에는 `--allow-remote`가 필요합니다. `run`은 기본으로 side-effect tool을 등록하므로 allowlist에 등록한 channel sender의 turn에서도 write/edit/exec 등 tool이 permission 승인 절차를 거쳐 실행될 수 있습니다. 끄려면 `--no-side-effects`를 쓰세요. `--allow-remote` 실행은 `--allow-side-effects`를 명시해야 켜집니다. `run --verbose`와 `web --verbose`는 input/response/tool/usage preview만 stderr에 찍고 raw prompt나 full payload는 출력하지 않습니다. input·response preview의 주소는 scheme·host·path만 보입니다.

`onboard`는 built-in channel별 기본 config stub을 `channels.<name>`에 생성하고, 기존 channel config가 있으면 사용자 값과 secret/env placeholder를 덮어쓰지 않은 채 누락된 기본 key만 병합합니다. `run`은 plugin config에 충분한 인증 정보와 현재 구현된 transport에 필요한 설정이 있으면 선택된 외부 channel transport도 시작합니다. 인증 정보가 없으면 전체 runtime을 실패시키지 않고 `skipped-missing-credentials`로 보고하므로, WebSocket부터 켜고 외부 channel을 점진적으로 추가할 수 있습니다. Slack은 Socket Mode worker를 사용하므로 `appToken`/`app_token`과 `botToken`/`bot_token`/`token`이 모두 필요합니다. Discord는 Gateway worker를 사용하므로 `allowChannels: []`가 원본처럼 봇이 볼 수 있는 모든 채널을 의미합니다. `allowChannels: ["*"]`도 모든 채널 허용으로 처리합니다.

최소 외부 channel config key:

- `channels.telegram`: `enabled`, `botToken`/`bot_token`/`token`, optional `pollTimeoutSeconds`, `pollLimit`.
- `channels.discord`: `enabled`, `botToken`/`bot_token`/`token`, optional `allowFrom`, `allowChannels`, `groupPolicy`(`mention`/`open`), `streaming`(기본 true). `allowChannels`가 비어 있거나 `["*"]`이면 봇이 볼 수 있는 모든 채널을 허용합니다. Gateway worker는 현재 일반 메시지/DM 응답, permission approval prompt/reply, conservative Gateway resume metadata를 다룹니다. Inbound는 durable enqueue 뒤에만 resume cursor를 전진하며, restart replay는 중복 가능성을 남기는 at-least-once 성격이지 exactly-once delivery가 아닙니다.
- `channels.slack`: `enabled`, `appToken`/`app_token`, `botToken`/`bot_token`/`token`, optional `channelIds`/`allowedChannelIds`/`allowChannels` 또는 `defaultChannelId`.
- `channels.email`: `enabled`, `consentGranted: true`, inbound 허용 목록 `allowFrom`/`allowedSenders`가 필요합니다. `channels.email.smtp`: `host`/`smtpHost`, `port`, `from`/`fromAddress`, optional `username`/`smtpUsername`, `password`/`smtpPassword`, `security`, `timeoutSeconds`; `channels.email.imap`: `host`/`imapHost`, `port`, `username`/`imapUsername`, `password`/`imapPassword`, optional `mailbox`, `markSeen`(기본 true), `pollIntervalSeconds`, `timeoutSeconds`, `security`. 현재 IMAP polling은 TLS(`security: "tls"`)만 시작하며, inbound Email은 `Authentication-Results` header의 `spf=pass`/`dkim=pass`를 기본 확인합니다(`verifySpf`/`verifyDkim`로 비활성화 가능).
- `channels.whatsapp`: `enabled`, `bridgeUrl`(WebSocket URL; 기존 `http://`/`https://` 값은 같은 host/path의 `ws://`/`wss://`로 호환 변환), optional `bridgeToken`, `groupPolicy`, `allowlist.allowedSenders`.

외부 transport는 의도적으로 최소 adapter입니다. Telegram은 long polling, Discord는 Gateway worker(필요 시 configured channel REST polling mode), Slack은 Socket Mode inbound와 Web API outbound, Email은 SMTP/IMAP, WhatsApp은 로컬 bridge WebSocket과 통신합니다. 외부 transport inbound는 remote ACK/cursor 갱신 전에 durable work queue에 저장되고, outbound는 runtime `MessageBus`와 `ChannelManager` dispatch 경계를 통과합니다. Worker lifecycle은 channel adapter의 `start`/`stop`이 관리합니다. 외부 agent processor는 같은 session key의 turn을 동시에 실행하지 않고, durable queue에서 dispatch된 같은-session follow-up을 process-local coordinator에 보관한 뒤 현재 turn이 끝나면 이어 처리합니다. Coordinator 자체는 process-local이지만 아직 dispatch되지 않은 durable work와 retry/cancellation request는 restart 후 복원됩니다. 서로 다른 session은 transport processor 안에서 병렬 turn으로 처리될 수 있습니다. Telegram/Discord/Slack은 provider text delta를 채널에 보내지 않고 최종 assistant answer만 새 message로 전송하며 기존 message를 edit/update하지 않습니다. Platform message 길이 제한에 가까워지면 문단/줄/공백 경계에서 후속 message로 나눠 전송합니다. Background subagent가 시작되면 같은 channel/thread에 시작 알림을 보내고, Discord는 agent turn이 실행되는 동안 그리고 그 turn이 시작한 background subagent가 같은 session에서 계속 실행되는 동안 같은 channel/thread에 typing indicator를 best-effort로 반복 전송합니다. Slack Web API outbound에는 bot typing indicator 전송 API가 없어 fake “typing...” 메시지는 보내지 않습니다. Email/WhatsApp은 final-only transport로 유지됩니다. Telegram topic, Slack thread, Discord thread, Email subject/reply context는 outbound reply metadata로 이어집니다. 모두 로컬 `AgentLoop`와 같은 personal-use `shacs-bot run` process 안에서 실행됩니다. Telegram offset, Discord REST last message id, Discord Gateway resume state, Email IMAP seen UID + UIDVALIDITY hint, 실제 remote send를 시도한 outbound delivery의 pending/sent/failed status는 runtime metadata JSON에 best-effort로 저장됩니다. Provider stream marker는 external final-only transport에서 실제 send가 아니므로 delivery hint로 기록하지 않습니다. 이 metadata는 duplicate/replay를 줄이기 위한 cursor/dedupe/diagnostic hint이며 durable queue truth, transaction, remote ACK/read receipt, replay 방지, exactly-once delivery 보장이 아닙니다. ACK 전후 crash timing에서는 conservative replay 때문에 중복 전달이 발생할 수 있습니다.

## Spec 030 trusted runtime과 release evidence

현재 trusted runtime owner facts는 CLI, TUI와 local API에서 같은 `Spec030RuntimeProjection`을 사용한다. 이 projection은 현재 OS 사용자 권한, lifecycle-only 경계, adapter별 process control, credential status, sandbox 적용 범위, resource provenance와 session/log/trace의 raw-content 가능성을 표시하며 permission grant, universal process gate, kernel isolation 또는 complete redaction을 의미하지 않는다.

```sh
cargo run --manifest-path crates/Cargo.toml --locked -p shacs-cli -- runtime trusted-runtime --format human --workspace .
cargo run --manifest-path crates/Cargo.toml --locked -p shacs-cli -- runtime trusted-runtime --format json --workspace .
cargo run --manifest-path crates/Cargo.toml --locked -p shacs-tui -- --workspace . --once
curl 'http://127.0.0.1:8901/v1/trusted-runtime?schema_version=1'
```

Enabled plugin은 `entrypoints.trustedHooks.tool:before`가 가리키는 단일 `.js`, `.mjs`, `.ts` 파일을 process-local trusted hook으로 로드할 수 있다. 파일은 동기 `toolBefore(context)` 함수를 정의하고 `{ allow: true }` 또는 `{ block: true, reason: "..." }`를 반환한다. Import/module graph와 Node API는 제공하지 않으며, user-data plugin은 explicit enable, workspace-local plugin은 explicit enable과 workspace trust가 모두 필요하다.

Spec 030 closure runner는 PRD 000-006의 explicit integration target, active production owner의 CLI/TUI/API semantic parity, cleanup, manual surface record와 Linux bwrap evidence를 fail-closed로 검사한다. Linux current-worktree lane은 runner가 직접 생성한 `spec030.bwrap_record.v4` provenance와 `spec030.surface_owner.v1` lifecycle evidence를 source snapshot, command receipt, executable digest와 결속한다. 외부에서 작성한 bwrap record는 closure evidence로 인정하지 않는다. Success fixture는 runner 자체만 검증하고 semantic closure를 뜻하지 않는다.

```sh
cargo run --manifest-path crates/Cargo.toml --locked -p shacs-projection --bin spec030-release-runner -- \
  --run-id spec030-current \
  --evidence-root /private/tmp/spec030-current \
  --repo-root . \
  --mode current-worktree \
  --manual-record /path/to/spec030-manual-record.json
```

## Spec 031 release evidence

Spec 031 release runner는 `shacs-projection` package의 `spec031-release-runner` binary로 제공됩니다. Current worktree closure run은 실제 Cargo command 결과와 exact external adapter fact를 검사하고 generated `manifest.json`, `coverage-matrix.json`, `results.json`, `failure-triage.json`, `reproducibility-observations.json`, `summary.md`를 evidence root에 씁니다:

```sh
cargo run --manifest-path crates/Cargo.toml --locked -p shacs-projection --bin spec031-release-runner -- --run-id spec031-current --evidence-root /tmp/spec031-current --repo-root "$(git rev-parse --show-toplevel)" --mode current-worktree
```

현재 Spec031 coverage는 65행이며 parent Must Have 13개, Acceptance 14개, Closure Evidence 12개와 PRD000-005의 요구사항을 포함합니다. 외부 owner는 Specs 029/030/033/034의 4개이며 전체 status 대신 소비하는 정확한 owner fact와 통과 증거를 검사합니다. 034의 owner fact는 [Spec 044](specs/044-video-analyzer-retirement/SPEC.md) 이후 생성 media 테스트(`crates/shacs-core/tests/spec034_persistence_operations.rs`의 `final_edit_and_variation_candidates_publish_one_complete_record_each`)입니다. 앱 기능이 [Spec 043](specs/043-app-capability-retirement/SPEC.md)에서 퇴역해 새 실행은 `spec031.release_runner.v5`로 기록되며 032 owner와 `spec031-owner-spec032` 명령이 없습니다. 이전 v4·v3·v2 결과는 032 owner를 포함한 기록된 구성 그대로 판독하고, 지워진 앱 테스트는 다시 실행하지 않고 [앱 보관 사본](archive/spec043-app-environment/README.md)의 sha256으로만 존재를 확인합니다. 지워진 영상 분석 테스트를 034 fact로 썼던 v3·v2 결과도 같은 방식으로 [영상 보관 사본](archive/spec034-video-analyzer/README.md)으로 확인합니다. 제거된 035 owner의 과거 80행은 현행 coverage에 추가하지 않습니다. [035 비활성 보관 자료](archive/spec035/README.md)의 누락·변조는 별도로 차단하며, 행의 존재만으로 실행 증거 통과를 주장하지 않습니다. Missing/unknown fact나 실패 command는 blocked이고 PASS를 합성하지 않습니다. Dirty worktree는 `observations/dirty-worktree.json`과 `reproducibility-observations.json`에 별도 typed observation으로 남으며 failure triage나 semantic verdict에 포함되지 않습니다.

삭제 전 035 classification v2의 45행/6 owner `BLOCKED` 입력과 당시 146행 runner 결과는 역사적 증거이며, 현재 Spec031 runner의 입력이나 완료 판정이 아닙니다. [과거 current-worktree](../.omo/worktree-archives/spec035-20260928/evidence/spec035/closure/final-compiled-runner-current/summary.md)와 [success-fixture](../.omo/worktree-archives/spec035-20260928/evidence/spec035/closure/final-compiled-runner-fixture/summary.md)의 결과를 새 65행 실행으로 재결속하지 않습니다. 과거 fixture 성공은 runner mechanics만 검증했으며 현재 Spec031 closure나 제거된 035의 최종 승인이 아닙니다.

[Spec 041](specs/041-release-evidence-tooling-disposition/SPEC.md#판정-결과-2026-10-25)은 이 runner를 **범위 축소**로 판정했고 [Spec 045](specs/045-release-runner-scope-reduction/SPEC.md)가 구현합니다. 위 명령, v5 결과와 v4·v3 판독은 그대로 남고, 새로 만들어지지 않는 v2 결과의 판독은 파일별 sha256 기록과 고정 commit의 validator로 옮긴 뒤 지웁니다. v2 판독은 아직 지금과 같습니다. 판독과 결과에 관해 알아 둘 점은 다음과 같습니다.

- 통과로 기록된 `cargo test` 명령은 test 개수가 있어야 하고 transcript에서 다시 센 값과 같아야 하며, 그렇지 않은 v5·v4·v3 결과는 거절합니다. [Spec 045 PRD 000](specs/045-release-runner-scope-reduction/prds/000-passed-cargo-test-count-check.md) 전의 validator는 `cargo test --workspace` 기록의 개수가 비어 있으면 transcript를 다시 세지 않아 손으로 고친 결과를 PASS로 받아들였습니다. 실패하거나 시간 초과로 끝나 개수가 없는 기록은 전처럼 명령 실패로 보고합니다.
- 결과는 commit을 기록하지 않아 실행 뒤 바뀐 요구사항 문구나 owner status를 알아채지 못합니다. 명령은 repo root 경로에 묶이고 035 보관 자료의 변조와 owner 테스트의 삭제는 거절합니다.
- 추적하지 않는 `.omo/evidence/spec034/task-12-integration.json`이 없는 checkout에서는 이 파일을 읽는 034 테스트(`mismatch_uses_isolated_tree`)가 실패해 current-worktree 결과가 `BLOCKED`가 됩니다. workspace 명령에 `--no-fail-fast`가 없어 첫 실패 package 뒤의 결과는 transcript에 남지 않습니다.

Runner 자체의 passing fixture는 아래처럼 실행할 수 있습니다. 이 fixture는 artifact writer와 validator의 success path를 확인할 뿐, 현재 checkout의 semantic Spec031 closure를 뜻하지 않습니다:

```sh
cargo run --manifest-path crates/Cargo.toml --locked -p shacs-projection --bin spec031-release-runner -- --run-id spec031-success-fixture --evidence-root /tmp/spec031-success-fixture --repo-root "$(git rev-parse --show-toplevel)" --mode success-fixture
```

## Docker Compose

저장소에는 개인 사용 서비스로 로컬 HTTP API와 channel runtime을 실행하기 위한 multi-stage Dockerfile과 `docker-compose.yml`이 포함되어 있습니다. Docker/Compose는 현재 zero-setup containment의 primary path입니다. 사용자가 host에 gVisor, Firecracker, Kata, bubblewrap 같은 별도 sandbox runtime을 직접 설치하는 것을 기본 경로로 요구하지 않습니다. Docker 동작은 upstream nanobot deployment와 같이 먼저 host config directory를 초기화하고, host의 `~/.shacs-bot`을 non-root container user의 `/home/shacs/.shacs-bot`에 mount하는 방식을 따릅니다:

```sh
export SHACS_UID=$(id -u)
export SHACS_GID=$(id -g)
mkdir -p ~/.shacs-bot
docker compose run --rm shacs-cli onboard   # first-time setup
vim ~/.shacs-bot/config.json                # add API keys or provider config
docker compose up -d shacs-gateway          # start channel runtime
```

`shacs-gateway`는 container 안에서 `shacs-bot run --websocket-host 0.0.0.0 --allow-remote --verbose --workspace /home/shacs/.shacs-bot/workspace --allow-side-effects`를 실행하고 host loopback의 WebSocket port `8765`에만 publish합니다. 기본 `docker compose up`은 이 단일 runtime owner만 시작합니다. `shacs-api`는 같은 data directory의 두 번째 owner가 되지 않도록 opt-in profile이며, gateway를 중지한 뒤 `docker compose up -d shacs-api`처럼 서비스 이름을 명시해 시작합니다. 이 Compose path는 Docker socket mount, `privileged: true`, host network를 기본값으로 쓰지 않습니다. Preview-only runtime logs는 `docker compose logs -f shacs-gateway`로 확인할 수 있습니다. Provider 설정이 없거나 명시적으로 활성화한 선택 도구의 provider 인증이 빠져 있으면 runtime은 시작하지 않으므로, 먼저 `config.json` 또는 `auth.json` workflow로 provider를 설정하세요.

Spec023의 공식 Compose smoke gate는 opt-in으로 실행합니다. 이 명령은 임시 Compose service와 data directory로 `runtime inspect`의 official-container runtime evidence를 확인하고, 별도로 기본 `docker-compose.yml`이 Docker socket, `privileged: true`, host network를 쓰지 않는지도 검사합니다:

```sh
./docs/scripts/spec023-compose-smoke.sh
```

로컬 OpenAI 호환 API를 시작하려면 같은 config/workspace를 사용해 별도 API service를 띄웁니다:

```sh
docker compose up -d shacs-api
curl http://127.0.0.1:8900/health
```

CLI command를 container에서 한 번 실행하려면 다음처럼 `shacs-cli` service를 사용합니다:

```sh
docker compose run --rm shacs-cli ask "Hello!"
docker compose run --rm shacs-cli status
```

Channel runtime preview logs를 확인하거나 서비스를 내리려면 다음 명령을 사용합니다:

```sh
docker compose up -d shacs-gateway
docker compose logs -f shacs-gateway
docker compose down
```

Provider secret은 로컬 config/environment workflow로 제공하세요. Image 안에 secret을 bake하지 마세요. 기본 container UID/GID는 nanobot과 같은 `1000:1000`이고, 위 예시처럼 `SHACS_UID`/`SHACS_GID`를 지정하면 host user 소유권에 맞춰 실행합니다. Docker containment는 permission mode나 side-effect gate를 없애는 근거가 아니며, unsafe privileged evidence가 보이면 permissive permission mode는 safe fallback으로 내려갑니다. Permission denied가 계속 나면 host에서 `sudo chown -R 1000:1000 ~/.shacs-bot`로 ownership을 맞추거나 Podman의 `--userns=keep-id` 같은 실행 user 전략을 사용하세요.

## 아직 남은 명령 범위

`plugins`와 `hooks`는 위의 plugin/hook 섹션에 설명된 관리 명령으로 구현되어 있습니다. Inspect/doctor 계열은 실행하지 않고, agent turn에서의 live hook 소비는 `tool:before` block-only 경계로 제한됩니다. Plugin command는 standalone dispatcher 경계에서만 실행되며 running session store를 직접 mutate하지 않습니다. TUI/REPL/wizard, projection/readiness, Tasks, capability/reconnect/accounting 및 release evidence 검증은 구현되어 있지만 최종 독립 감사와 전체 출시 승인은 완료되지 않았습니다. Local owner lease와 gateway supervision의 current scoped boundary는 `029-durable-runtime-recovery-and-data-migration`에서 완료됐으며, 자동 process reexec나 worker restart/backoff를 의미하지 않습니다. 구현된 Codex login 외 provider OAuth는 현재 지원 provider/auth 범위 밖의 비목표이며, ClawHub search/install/update도 remote marketplace 비목표로 닫았습니다. 위에서 구현된 것으로 명시하지 않은 command는 사용할 수 있는 기능으로 취급하지 마세요.
