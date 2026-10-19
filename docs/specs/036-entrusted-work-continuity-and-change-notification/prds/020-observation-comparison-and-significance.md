# PRD 020. 관측 비교와 의미 있는 변화 판정

Status: Complete (Scoped)

Depends on: [PRD 000](000-entrusted-work-consent-and-lifecycle.md), [PRD 010](010-goal-bound-wake-and-observation-history.md) 완료. 뒤 번호 PRD에는 의존하지 않는다.

## Goal

같은 위임 작업의 관측값을 현재 조건과 이전 기준에 대조해 **무변화·무관한 변화·사용자 결정에 중요한 변화·판정 불가**를 구분하고, 근거를 사용자에게 조회 가능하게 한다. **실제 메시지 발송이 없어도 이 판단과 조회만으로 단독 완료할 수 있어야 한다.**

## Scope

1. PRD 010의 마지막 확인과 이번 결과가 같은 작업/조건 버전 아래에서 비교 가능한지 확인한다. 첫 성공 관측은 기준을 만들되 이를 변화 알림으로 취급하지 않는다. 조건을 수정했다면 기준 재설정 또는 제한된 비교 이유를 명시한다.
2. 동등 결과와 사용자 선택에 영향이 없는 차이를 Suppress, 사용자 조건·선택지·결정 시점에 영향을 주는 차이를 Notify 후보로 판정한다. 관측 A→A→무관한 차이→의미 있는 B를 한 작업에서 재현한다. 출처가 Cron이라는 이유만으로 Notify 후보가 되지 않는다.
3. 후보에는 이전/새 값의 안전한 요약, 현재 조건, 중요도 판단 근거, 관측 시간과 source ref, 비교 한계를 남긴다. 수집 실패·불충분/오래된 자료는 `unknown`/`blocked`로 표시하고 무변화나 성공으로 합성하지 않는다.
4. 명시적 정기 요약 선호와 변화 알림은 구별한다. 사용자가 정기 요약을 선택하지 않았으면 무변화 결과를 반복 보고 대상으로 삼지 않는다. 이 PRD는 현재 판정과 선호를 조회 표면에 제공하지만 메시지를 보내지는 않는다.

## Non Scope

- durable Notify inbound 생성, channel outbound, 원격 사용자 수신 판정, 예약·구매·발송의 외부 동작.
- 새로운 범용 evaluator, Python 계산 하네스, 가격·여행 도메인별 비교 알고리즘. 지원 가능한 관측 입력과 사용자 조건만 평가한다.
- 모델이 “중요해 보인다”고 말한 문구만으로 확인되지 않은 source를 검증된 변화로 바꾸는 일.

## Parent Requirement Mapping

- [036 SPEC](../SPEC.md)의 소유 범위 4에서 기준 관측·비교·중요도·unknown/blocked 판정과 정기 요약 opt-in 사실을 소유한다.
- 부모 불변식의 Cron source-only Notify 금지, 동일 결과 재통지 금지의 **결정 근거**와 제한된 근거 노출을 소유한다. 실제 route/delivery는 이 PRD의 범위가 아니다.
- 부모 수용 조건 3 중 A→A→무관한 차이→B에 대한 판정·근거를 소유한다. 실제 B 통지 enqueue는 이 PRD의 완료 조건이 아니다.

## Required Contract

1. 중요도 판정은 사용자가 설정한 조건이나 선택 결과의 차이에 결속한다. 단순 text 차이, 성공 exit code, source 종류, 이전 Notify 여부만으로 중요하다고 선언하지 않는다.
2. 같은 관측 사건과 조건 버전은 안정적인 decision identity를 갖고 반복 처리해도 반대 verdict를 만들지 않는다. 조건 변경 직후의 이전 버전 결과는 현재 비교 기준을 갱신하지 않는다.
3. Advisory evaluator는 비교 근거를 제공할 수 있으나 033 session/automation truth를 직접 바꾸거나 송신 권한을 갖지 않는다. 원본 전체, credential, 불필요한 payload를 사용자 요약에 넣지 않는다.

## Acceptance Criteria

1. 사용자 소유 제어 자료를 A, 다시 A, 선택에 영향 없는 차이, 의미 있는 B 순서로 관찰한다. 첫 A는 기준, 둘째 A와 무관한 차이는 Suppress, B는 근거 있는 Notify 후보로 조회되며 어떤 경우에도 이 PRD의 평가만으로 outbound 메시지가 나가지 않는다.
2. 사용자의 예산/조건 수정 후 오래된 관측을 넣거나 이전 증거를 지운다. stale·비교 불능·실패는 blocked/unknown으로 보이고 새 조건에서 B의 중요도를 허위 확정하지 않는다.
3. 같은 사건 재평가의 안정성, 입력/근거의 제한과 source provenance, 정기 요약 선호 미설정/설정의 차이를 CLI/API 또는 현재 작업 조회 표면에서 확인한다.

## Closure Evidence

1. A→A→무관 변화→B 및 조건 수정/누락 관측의 decision record와 조회 결과; 어떤 판단이 왜 나왔는지 추적할 수 있는 source ref.
2. 동일 사건 반복·조건 버전 경합·source-only Notify 시도의 거절 테스트.
3. 판정 성공과 사용자 통지 부재를 별도로 확인한 실제 사용자 조회 결과, 제한된 근거/비보장 audit.

PRD 020은 **변화 여부를 판단하고 설명하는 기능**으로 닫힌다. 뒤 번호 PRD의 통지·전달 완료나 전문 계산 하네스를 기다리지 않는다.

## 구현과 독립 종료 기록 (Oracle PASS)

Oracle 재검토에서 PRD 020의 독립 수용 조건을 **PASS**로 판정했다. 아래 기록은 의미 비교와 근거 조회만 닫으며 실제 통지·전달은 PRD 030에 남긴다.

- 지원 비교는 사용자가 선택한 `integer_at_most` 조건 하나다. `{"schema":"integer_v1","value":120,"as_of_ms":1792400000000}` 형태의 고정 필드 JSON을 기존 PRD010 bounded local reader로 읽는다. `value`는 i64, `as_of_ms`는 사용자 자료의 Unix millisecond 시각이며 추가 필드·소수·텍스트·다른 schema는 의미 요약으로 채택하지 않는다. 자유 형식 예산/지시를 추론하거나 모델의 중요도 주장을 소비하지 않는다.
- `work observe`의 `--at-most`, `--max-age-seconds`, `--regular-summary true|false`와 기존 POST observations의 `comparison`으로 명시 설정한다. 기존 session mutation lock과 `expected_revision` CAS로 plan 전체를 교체하고 revision을 증가시킨다. 설정 생략은 비교 없음, 요약 선호 기본값은 false다. CLI/API syntax와 전체 교체 의미는 [사용 가이드](../../../USAGE.md#사용자-조건에-따른-변화-판정-prd-020)에 있다.
- 첫 비교 가능 값은 Baseline, 같은 값이나 상한 충족 여부가 같은 수치 차이는 Suppress, 충족 여부가 바뀔 때만 Notify 후보다. 양방향 변화를 지원한다. 정기 요약 적격 여부는 독립 boolean이며 opt-in이 Suppress를 Notify로 바꾸지 않는다.
- 신규 decision과 안전한 수치 요약은 PRD010 관측의 029 terminal fact에 함께 저장한다. enqueue/read/commit gate와 cancellation-sensitive terminal append는 유지한다. 조건·goal lineage가 바뀌면 새 기준을 만들고 구버전 결과는 새 기준을 갱신하지 않는다. 취소는 새 관측 없이 기존 cancellation fact와 원래 wake payload로 blocked를 투영하며 `basis.kind=cancellation_fact`와 event ID로 구별한다.
- stable decision ID는 관측 work ID의 SHA-256에 결속한다. 같은 사건을 다시 소비해 새 반대 판정을 쓰는 API는 없다. 동일 tick 재처리는 기존 029 terminal/dedupe로 차단된다. 최근 32개 조회와 별도로 마지막 유효 기준 하나를 전체 event replay에서 유지하므로 조회 history eviction이 기준 유실을 만들지 않는다.
- 구형 digest-only terminal은 `legacy_observation_uncompared` 근거의 unknown이며 의미 기준이 아니다. malformed/missing summary와 stale/future/out-of-order/conflicting timestamp는 unknown이다. 아직 유효 기준이 없으면 첫 fresh/comparable 관측만 Baseline이 되고 중요한 변화로 승격되지 않는다. 기존 기준이 노후됐거나 이력이 서로 모순되면 unknown으로 남는다. 읽기 실패는 unknown, 실행 gate/구버전 취소는 blocked다. 실패 판정은 기준을 갱신하지 않으며 source 누락으로 주기가 중단됐거나 기존 기준이 노후된 경우 사용자가 최신 revision으로 다시 설정한다.
- 새 comparison module은 typed model, 순수 비교, cancellation fact 투영으로 나뉜다. 별도 queue/evaluator/session lifecycle, provider 호출, durable Notify inbound, channel outbound, 외부 확정 행동은 추가하지 않았다. PRD010의 symlink/동일 소유자/64 KiB/permission/hook/snapshot 제한을 유지한다.

## 실행 증거 (2026-10-19)

clean build한 CLI를 tmux에서 사용하고 `serve --port 18937 --allow-api-side-effects`의 현재 production worker를 실제로 구동했다. 격리 config/workspace는 `/var/folders/zr/3g_bnyrs53xb7y9sb1nh35sw0000gn/T/opencode/spec036-prd020.QQdNx7/{data,workspace}`이며 실제 사용자 config/auth를 복사하지 않았다. `custom` provider는 도달 불가능한 `http://127.0.0.1:1/v1`, 외부 channel은 disabled다. 관측에 provider 호출이 필요하지 않았다. CLI `--help`, 비교 조건 없는 요약 flag의 오류, CLI 설정/조회, API 설정/조회/409를 직접 확인했다.

작업 `work-302de0133ea5`, goal `goal-cli-owner-a34f826f0c9d`, 조건 revision 2, 상한 100, 최대 자료 나이 3600초, cadence 15초, summary opt-in false:

| 관측 | 관측 시각 ms | verdict / reason | decision ID의 SHA-256 부분 |
|---|---|---|---|
| 첫 120 | 1792400758038 | baseline / first_comparable_observation | `51c30411e5318a3a3ac84f108da9e2a38f8198c9d7b67cc07f0dd69ee1dfad85` |
| 다시 120 | 1792400773110 | suppress / equivalent_value | `6228c92a9e605f87448463649953cc00df316e4f679d31439e6de5641b9d7a7c` |
| 110 | 1792400803300 | suppress / same_condition_outcome | `b1ce7a620f27e7e8f221c3c7875dec9f5441627ac91c080ea312e99f7b51c75b` |
| 90 | 1792400833464 | notify_candidate / condition_outcome_changed | `f88d3fed7fb252a4a89940819d9ac1db4b63bf5998c1b0fcd7238005781ab6fa` |

90의 decision은 이전 110/false, 새 90/true, policy `integer_at_most:100`, 조건 revision, source `source.json`, 각각의 관측 시각·snapshot ref·digest와 비교 한계를 반환했다. 이전 digest는 `sha256:140b5df2fe6c432e6be68c351629ea645f14b063c1a05dc56d3ce137d0be32c5`, 새 digest는 `sha256:1d71dcd247dd2e55d70c5c9ea5aa7a9e16fa71aafbc32727409f47eecd5e9464`였다. 그 뒤 같은 90의 확인은 Suppress였다. Ctrl-C 후 별도 CLI process와 재시작 API 모두 동일한 Notify 후보 ID 하나를 반환했고 새 중요 사건으로 재포장하지 않았다.

API로 상한 80, cadence 5초, summary opt-in true를 설정하자 revision 3이 됐다. 남은 revision 2 wake는 `event-00000000000000000036`의 `blocked/observation_blocked/stale_lineage`로 보였고 새 관측은 없었다. revision 3의 90은 `baseline/lineage_reset`, 다음 90은 `suppress/equivalent_value`였으며 둘 다 `regular_summary_eligible:true`였다. 이전 revision 2로 상한 999를 쓰려는 요청은 HTTP 409였고 상한 80을 유지했다.

같은 revision 3에서 추가 필드 `credential`을 넣은 자료는 `unknown/summary_unavailable`, `as_of_ms:0`의 70은 `unknown/source_stale`였다. API 응답에 입력의 시험용 payload marker는 없었고 두 경우 모두 기준은 90을 유지했다. 실제 source 파일을 삭제하자 다음 판정은 아래처럼 남았다:

```json
{
  "condition_revision": 3,
  "decision_id": "comparison:v1:3da883e9907b25594457698d787796328af2d564906d93931c209663a1548336",
  "verdict": "unknown",
  "reason": {"code":"observation_unavailable","reason":"source_missing"},
  "regular_summary_eligible": false,
  "next_wake": {"status":"stopped","reason":"source_missing"}
}
```

다시 종료하고 CLI 및 재시작 API로 읽었을 때 동일한 실패 ID, 기준 90, 결정 25개, Notify 후보 1개, 중복 decision ID 0개가 유지됐다. `GET /v1/sessions/cli%3Aowner/history`는 전체 과정 후에도 `history:[]`였다. 실제 `runtime inspect`는 pending/leased/retry_waiting/cancel_requested 모두 0, 여섯 channel의 pending inbound/outbound 모두 0, deliveries none을 반환했다. 이는 PRD020의 결정 조회 증거이지 메시지 송신·원격 수신 성공 증거가 아니다.

## 검증과 잔여 범위

```sh
cargo fmt --manifest-path crates/Cargo.toml --all -- --check
cargo clippy --manifest-path crates/Cargo.toml --locked --workspace --all-targets -- -D warnings
cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core --test spec036_observation --test spec036_entrusted_work --test spec036_entrusted_work_compat --test entrusted_work_conversation --test spec033_automation_dispatch --test spec033_automation_gates --test spec033_automation_rejection --test spec033_automation_results --test spec033_automation_service --test spec033_goal_accounting --test durable_dispatch -p shacs-session --test durable_work --test durable_work_compat --test durable_work_observation_commit -p shacs-cli --test spec036_observation_cli --test spec036_entrusted_work_cli -p shacs-api --test spec036_observation_api --test spec036_entrusted_work_api
cargo clean --manifest-path crates/Cargo.toml
cargo build --manifest-path crates/Cargo.toml --locked -p shacs-cli
```

최초 구현에서 targeted test 135개, workspace fmt/clippy, clean CLI build가 통과했다. 관측 회귀는 당시 31개였으며 실제 bounded reader, A/A/무관/B, 요약 선호, 중복 tick/restart, old digest terminal, criteria CAS/read 중 변경, timestamp 역순/충돌, missing/stale evidence, 32개 history eviction을 확인했다. CLI/API 비교 회귀도 포함한다. 변경 Rust 파일의 LSP error는 없고 Markdown LSP는 구성되지 않았다. 전체 workspace test 또는 release seal은 수행/주장하지 않으며 기존 `imap-proto 0.10.2` future-incompatibility 안내는 남아 있다.

Oracle 최초 검토는 초기 불완전 관측 뒤 유효 기준을 영구히 만들지 못하는 문제와 pause/resume 뒤 이전 goal transition의 판정이 현재값으로 보이는 문제로 **BLOCKED**였다. 수정 후 첫 유효값을 기준으로 만드는 회귀와 현재 transition/plan lineage에서만 `current_observation`·`current_decision`·`comparison_baseline`을 노출하는 회귀를 추가했다. 최신 관측 회귀 33개와 CLI/API 5개, fmt·workspace clippy·clean CLI build가 통과했다. 실제 API에서 pause 후 세 current 필드는 null, resume 뒤 첫 결정은 `baseline/lineage_reset`, 과거 통지 후보는 이력에만 남았다. **같은 Oracle이 두 결함 해소와 잔여 blocker 부재를 확인해 PASS로 재판정했다.**

수치 조건 이외의 의미 비교, 외부 사실·source timestamp 진위 확인, 단위 변환, 임의 원문/secret의 완전한 redaction, 로그 삭제 후 전체 연속성 복구는 지원하지 않는다. Source timestamp는 명시적으로 사용자 제공 사실이다. PRD010에서 남긴 `run` mode surface worker와 external-channel tick의 동시 lease 오류에 대한 worker-level 회귀 공백은 이 작업에서도 해결했다고 주장하지 않는다. Native containment unknown과 기존 runtime readiness의 unrelated unavailable은 이 비교 판정의 성공으로 덮지 않는다. 다음 단계는 부모의 독립 Oracle 검토뿐이며 PASS 전 PRD030은 잠겨 있다.
