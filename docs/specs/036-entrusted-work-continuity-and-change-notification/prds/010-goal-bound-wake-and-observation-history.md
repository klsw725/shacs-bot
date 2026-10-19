# PRD 010. 위임된 일에 결속된 확인과 관측 기록

Status: Complete (Scoped)

Depends on: [PRD 000](000-entrusted-work-consent-and-lifecycle.md) 완료. 뒤 번호 PRD에는 의존하지 않는다.

## Goal

사용자가 맡긴 활성 일에 대해서만 기존 scheduler/durable work로 확인을 예약·실행하고, 마지막 관측·확인 시각·다음 확인을 재시작 후 조회할 수 있게 한다. **변화 판정이나 선제 메시지 전송 없이 관측 이력 조회만으로 단독 완료할 수 있어야 한다.**

## Scope

1. PRD 000의 위임 ID, 현재 조건 버전·goal owner 사실을 확인한다. 이 작업의 확인 대상과 주기는 사용자 입력으로 등록·조회하고 029의 wake/durable work에 결속한다. 기존 일반 cron/heartbeat 실행을 사용자 위임으로 소급 변환하지 않는다.
2. enqueue 전과 실행 직전에 활성·동의·조건 버전을 다시 확인한다. pause/done/clear, 철회, stale 조건의 work는 기존 029 cancellation/terminal fact를 사용해 실행·새 관측 없이 처리한다.
3. 기존 실행 경계로 지원되는 사용자 소유 관찰 대상을 읽고, 작업 ID/조건 버전/관측 시각/제한된 근거 ref/결과 상태를 저장한다. 비교 가능한 마지막 관측과 다음 확인 시각을 조회할 수 있게 한다. 이전 상태가 없거나 실패한 확인은 `unknown`/`blocked`로 구별한다.
4. Snapshot/hook, 필요 시 confirmation, adapter별 process/credential/sandbox/disclosure는 030/031 owner 사실을 소비한다. 관측 저장은 tool permission grant나 외부 확정 행동 승인을 만들지 않는다.

## Non Scope

- 관측 A와 B의 의미 차이 판정, Notify/Suppress 결정, 사용자에게 메시지 전송 또는 정기 요약 발송.
- 별도 scheduler·범용 queue·session truth, 모든 외부 사이트 감지, 지원되지 않는 app/channel/local API background producer의 성공 합성.
- 재시작 뒤 열린 work를 성공으로 가정하거나 원격 exactly-once 실행을 보장하는 것.

## Parent Requirement Mapping

- [036 SPEC](../SPEC.md)의 소유 범위 2 중 마지막 관측·다음 확인·근거의 재시작 복원, 범위 3의 goal-bound wake·두 시점 gate를 소유한다.
- 부모 불변식의 종료 후 늦은 wake 차단과 조건 버전 경합을 실행 경계에서 소유한다.
- 부모 수용 조건 2 중 관측·다음 확인의 복원 및 pause/done/clear/stale work 억제 부분을 소유한다. 사용자 위임 자체는 PRD 000에서 소비한다.

## Required Contract

1. 현재 producer의 `goal_id: None`, `active_goal: true`, `confirmation: NotRequired` 값을 위임 사실이나 실행 허가로 신뢰하지 않는다. 기존 자동화도 유지하되 이 PRD의 작업별 producer는 검증된 위임/goal/조건 lineage를 넣는다.
2. 읽기 직전의 조건 버전과 기록 직전의 현재 버전을 비교해 오래된 관측이 새 조건의 마지막 관측을 덮지 않게 한다. 상태 전이가 중간에 일어나면 해당 attempt의 취소/미반영 이유를 남긴다.
3. 관측값이 없거나 검사에 실패하면 무변화/성공으로 기록하지 않는다. 출처·관측 시각·안전한 ref는 조회 가능하지만 raw secret과 무제한 원본 payload를 기록하지 않는다.
4. Job 실행·관측 저장·다음 wake는 분리된 사실이다. 종료 작업은 이전 이력을 조회할 수 있으나 다시 주기적 확인을 시작하지 않는다.

## Acceptance Criteria

1. 격리된 workspace에서 활성 위임의 제어 가능한 자료를 한 번 확인하고 현재 관측과 다음 확인 시각을 CLI/API 조회로 본다. 같은 상태를 재시작 뒤 조회할 수 있다. 알림 경로가 없어도 성공·실패 관측은 보인다.
2. 미동의/paused/done/cleared 작업, 만료된 조건 버전, 동시 조건 변경을 각각 enqueue 전과 due 실행 전후에 재현한다. 어느 경우에도 구버전 결과가 현재 관측을 덮거나 사용자 메시지를 보내지 않는다.
3. source 누락·읽기 실패·지원되지 않는 실행 경계를 주면 명시적인 unavailable/blocked 관측과 보수적인 next-wake 상태를 남긴다. 기존 일반 cron/heartbeat나 033 goal continuation의 동작이 이 새 경로 때문에 변경되지 않았음을 검증한다.

## Closure Evidence

1. 실제 예약 시각 전후, 재시작/복구 전후의 work ID·goal/조건 버전·관측 조회 출력.
2. 동의 없는 입력, pause/done/clear, stale wake, 조건 수정 경합 및 중복 due의 fail-closed 테스트와 terminal reason.
3. 029 durable wake/recovery와 030/031 현재 실행 gate를 소비한 근거 및 기존 cron/goal 경로 회귀 결과.

PRD 010은 **알림 없는 예약 확인과 관측 조회**로 닫힌다. 이후 변화 판정·통지 구현을 종료 조건으로 끌어오지 않는다.

## 구현과 검증 기록 (Oracle PASS)

PRD 010은 독립 Oracle gate에서 **PASS**를 받았다. 이 판정은 관측·재시작 복원·stale 차단 범위만 닫으며 PRD 020의 의미 비교와 PRD 030의 통지는 포함하지 않는다.

- CLI `work observe <id> --workspace <ws> --session <owner> --expected-revision <n> --source source.txt --cadence-seconds 15`로 활성 위임에 설정하고, `work observations <id> --workspace <ws> --session <owner> --data-dir <actual-config-directory>`로 읽는다. GET/POST `/v1/sessions/<encoded-owner>/entrusted-work/<id>/observations`도 같은 owner를 사용한다. 전체 준비·재시작 명령은 [사용 가이드](../../../USAGE.md#알림-없는-로컬-파일-예약-관측-prd-010)에 있다.
- 지원 범위는 Unix workspace-relative, symlink 없는 동일 소유자 일반 파일, 최대 64 KiB, 5~86,400초 cadence다. 원문 대신 digest/byte count를 저장한다. 중요도 비교·Notify/Suppress 판정·provider 호출·message enqueue는 없다.
- 029 `DurableWorkDispatcher`의 기존 payload/enqueue/admission/lease/cancel/recovery/terminal을 사용한다. `serve`의 기존 surface/automation worker와 channel runtime의 기존 dispatch tick에 연결했으며 새 queue나 독립 lifecycle은 없다. 설정은 PRD000의 session에 저장하고 관측과 다음 확인 의도는 한 `work.terminal` fact로 원자 저장한다. 이후 실제 enqueue는 별도 사실이다.
- enqueue/read/commit 경계의 session mutation lock 안에서 033 goal, 동의 근거, 조건 revision, goal transition을 재검사한다. 읽기 중 조건 변경은 새 관측 없이 취소된다. 029 `WorkTerminalCommit.cancel_if_requested` opt-in은 terminal append의 기존 event lock 안에서 cancel 경합을 해결하여 실패/blocked 관측도 취소 이후 저장하지 않는다. 기존 terminal caller의 기본 동작은 유지된다.
- 실제 실행은 031 current execution snapshot, 030 trusted/plugin `tool:before`, 기존 noninteractive `RuntimeToolExecutor`의 read permission/ceiling을 소비한다. 로컬 읽기에 process/remote credential/provider disclosure는 필요하지 않으며 미지원 외부 adapter를 성공으로 만들지 않는다.
- 재시작 조회는 029 ledger의 최근 32개 관측을 읽는다. `current_observation: null`, `awaiting_enqueue`, `queued`, `leased`, `stopped`를 구별하며 열린 lease를 성공으로 합성하지 않는다. 실패 후에는 사용자가 현재 revision으로 다시 설정해야 한다.

### 실행 증거 (2026-10-19)

격리 config/workspace에서 빌드한 `shacs-bot`을 tmux로 실행하고 `serve --port 18936 --allow-api-side-effects`의 실제 API/worker를 확인했다. Provider 설정은 `custom`, 주소는 도달 불가능한 `http://127.0.0.1:1/v1`이며 관측 중 provider 호출은 필요하지 않았다.

```text
work_id = work-d29ef8afd20e
goal_id = goal-cli-owner-188ebbd6c07a
configuration revision = 2
before due: current_observation=null, next_wake.status=queued
first due_at_ms=1792397269064, observed_at_ms=1792397269137
result.status=observed, byte_count=28
evidence_ref=sha256:12c06983f12d727bb9056f99a101befa78d091bf3d86d066873371f3d4862c8b
next_wake_at_ms=1792397284137
```

Ctrl-C 종료 후 별도 CLI process에서 같은 이력과 pending work `...:1792397329378`을 읽었다. 같은 config로 API를 재시작하자 이 wake를 `1792397338948`에 관측했고 이전 이력도 유지됐다. API pause 뒤 이력 5개와 마지막 digest는 유지되고 `next_wake={status:stopped,reason:goal_inactive}`였다. paused 설정 요청은 HTTP 409, 오래된 revision 설정은 HTTP 409였다. revision 3의 `missing.txt`는 `unavailable/source_missing`, `next_wake_at_ms:null`, stopped로 남았고 revision 4의 `sessions` directory는 `unavailable/unsupported_source`였다. GET session history는 `history: []`였으며 일반 agent turn이나 알림을 만들지 않았다.

검증 명령:

```sh
cargo fmt --manifest-path crates/Cargo.toml --all -- --check
cargo clippy --manifest-path crates/Cargo.toml --locked --workspace --all-targets -- -D warnings
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core --test spec036_observation --test spec036_entrusted_work --test spec036_entrusted_work_compat --test entrusted_work_conversation --test spec033_automation_dispatch --test spec033_automation_gates --test spec033_automation_rejection --test spec033_automation_results --test spec033_automation_service --test spec033_goal_accounting --test durable_dispatch -p shacs-session --test durable_work --test durable_work_compat --test durable_work_observation_commit -p shacs-cli --test spec036_observation_cli --test spec036_entrusted_work_cli -p shacs-api --test spec036_observation_api --test spec036_entrusted_work_api
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-cron -p shacs-heartbeat
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-cli --lib observation_wake_is_not_a_channel_outbound_request
cargo clean --manifest-path crates/Cargo.toml
cargo build --manifest-path crates/Cargo.toml --locked -p shacs-cli
```

새 core 회귀 19개는 due/restart, 중복 tick, 누락/읽기 실패/미지원/크기/symlink, enqueue/due/commit의 비활성·철회·revision 경합, 별도 thread의 동시 조건 변경, 029 취소와 stale lease recovery를 검증한다. Session 회귀는 성공/실패/blocked 모든 terminal의 취소 후 fact 폐기를 확인한다. CLI/API 회귀는 CAS·opt-in·연결 대상·재시작 조회를 확인하며 기존 033 automation/goal, 029 dispatcher/work, cron 10개와 heartbeat 8개도 통과했다. 실제 QA에서 새 관측 wake가 채널 pending-outbound hint로 잘못 보이는 공용 projection 분류를 발견해 이 work kind만 제외하고 회귀를 추가했다. 전체 workspace test/release seal이나 Oracle PASS를 주장하지 않는다.

최종 소스에서 targeted test 120개, cron/heartbeat 18개, outbound projection 회귀 1개, workspace fmt/clippy, clean CLI build가 통과했다. 최종 재빌드한 바이너리의 `work-46de49ae7ff1`도 실제 worker에서 digest 관측과 다음 queue 등록을 확인했다. clear 직후와 한 cadence 이상 지난 뒤 모두 이력 8개, 마지막 `observed_at_ms=1792398341439`, `stopped/goal_inactive`, session `history: []`가 유지됐다. 최종 runtime inspect는 pending/leased/retry/cancel-requested가 모두 0이었다. 이전 done 작업의 이력 8개도 재시작 뒤 유지됐다. Rust 변경 파일의 LSP error는 없으며 Markdown/TOML LSP는 구성되지 않았다. 기존 `imap-proto 0.10.2` future-incompatibility 안내는 남아 있다.

Oracle 독립 검토는 실제 029 due wake, enqueue/read/commit의 033·consent·revision gate, 취소 후 terminal fact 폐기, bounded local source와 030/031 실행 gate, CLI/API·재시작 조회를 대조해 **PASS**로 판정했다. Digest/byte count는 이 PRD의 byte 단위 동일성 비교 기준이며 의미 차이 판정은 PRD 020에서 처리한다. 비차단 위험으로 `run` 모드의 surface worker와 external-channel tick이 같은 due item을 동시에 lease할 때 한쪽 오류가 runtime 종료로 이어지는지에 대한 worker-level 회귀는 아직 없다.
