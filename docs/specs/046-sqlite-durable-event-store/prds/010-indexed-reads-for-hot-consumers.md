# PRD 010. 필요한 행만 읽기

Status: Complete (Scoped)

Depends on: [PRD 000](000-sqlite-store-behind-existing-api.md) 완료 사실. 뒤 번호 PRD에 의존하지 않는다.

## Goal

자주 읽는 소비자와 work 상태를 만드는 경로가 필요한 행만 읽게 해서, 기록이 쌓여도 쓰기·읽기 비용이 전체 행 수에 비례하지 않게 한다.

## Scope

1. **거른 읽기:** `DurableEventStore`에 session id, kind 목록, work id 접두어로 거른 행을 sequence 순으로 넘기는 읽기를 더한다. 조건은 각각 생략할 수 있다. 넘기는 행마다 checksum, 색인 열과 record의 일치, compatibility를 검증한다. 요약은 decode한 행 수를 담는다.
2. **소비자 전환:** 아래 소비자가 `visit_from_sequence(0)` 대신 거른 읽기를 쓴다. 각 소비자의 판정 코드는 바꾸지 않는다.
   - 웹 감시 ledger(`WebWatchLedger::read`): owner session, work id 접두어 `web-watch:{work_id}:`
   - 웹 감시 통지 저장소, 위임 작업 관측 저장소, 위임 작업 통지 저장소, 결정 마감 dispatch: 지금 visitor가 거르는 session·kind·work id 조건
3. **work 상태 따라잡기:** work 상태를 만드는 모든 production 경로가 하나의 계약을 쓴다. process 캐시는 work 상태와 그 상태가 반영한 마지막 sequence를 PRD 000의 저장소 정체와 함께 두고, 정체가 같으면 그 뒤의 행만 읽어 reducer를 적용한다. 정체가 다르거나 캐시가 없거나 검증에 실패하면 처음부터 만든다. 적용 대상은 `DurableWorkEnqueuer::enqueue`·`enqueue_json`, `DurableEventStore::append`의 work 정규화, dispatch의 `replay_work_state`와 그 호출처, child recorder의 work 상태 재생이다. 구현 전에 다시 검색해 빠진 production 경로가 없는지 확인한다.
   - 기록으로 이어지는 경로(enqueue, append 정규화, child recorder의 기록)는 PRD 000이 정한 append 순서를 지킨다. 따라잡기와 그 상태로 하는 판단(정규화, 전이 검증, enqueue의 한도·중복 확인)을 행을 넣는 쓰기 transaction 안에서 하고, 이를 위해 transaction 안에서 따라잡은 work 상태를 받아 판단한 뒤 행을 넣는 append 입구를 둔다.
   - 읽기만 하는 경로(dispatch의 상태 조회)는 read transaction 안에서 따라잡는다.
   - 캐시는 commit이 성공한 뒤에만 새 상태로 바뀌고, `CommitUnknown`이면 버린다.
4. **진단 값:** 저장소는 process 안에서 decode한 행 수를 읽기 전용 진단 값으로 센다. 측정과 테스트는 이 값으로 판정한다.
5. **측정:** 대상 소비자가 읽어야 할 행(웹 감시 하나의 기록 50행)과 따라잡을 새 행(10행)은 고정하고, 관계없는 session·kind의 행만 1만·10만·30만 개로 늘린 합성 DB를 만든다. durable work enqueue 한 번, 웹 감시 ledger 읽기 한 번, dispatch 한 번의 work 상태 갱신마다 decode한 행 수와 시간(따뜻한 상태 10회 중앙값)을 잰다. 첫 open 전체 검증 시간은 따로 잰다. 같은 행 구성의 `events.log`로 046 착수 직전 commit의 시간을 재고, 합성 파일 크기가 512 MiB 아래인지 확인해 기록한다. 측정은 임시 폴더에서만 하고 사용자의 data directory를 쓰지 않는다.

## Non Scope

- 저장소 형식과 공개 입구([PRD 000](000-sqlite-store-behind-existing-api.md)), recovery admission의 checkpoint 위조 검사, 오래된 기록 정리, 소비자의 판정 규칙 변경.

## Parent Requirement Mapping

- [046 SPEC](../SPEC.md)의 소유 범위 7과 수용 조건 6, 이 PRD 변경에 대한 수용 조건 7을 소유한다. PRD 000이 소유한 수용 조건 5(같은 work의 동시 전이)는 따라잡기를 켠 상태에서 다시 확인한다.

## Required Contract

1. 거른 읽기의 결과는 같은 조건의 visitor로 전체를 읽었을 때와 같은 행을 같은 순서로 낸다.
2. 따라잡은 work 상태는 처음부터 만든 상태와 같다.
3. 손상이나 미지원 행을 만나면 지금처럼 오류나 compatibility로 드러난다. 거른 읽기가 읽은 행의 손상을 건너뛰어 성공으로 만들지 않는다.
4. 기록으로 이어지는 판단은 그 기록을 넣는 쓰기 transaction 안에서 따라잡은 상태로만 한다.

## Acceptance Criteria

1. 각 소비자에 대해, 다른 session·kind·work id의 행을 섞은 DB에서 거른 읽기와 전체 읽기가 같은 결과를 낸다.
2. 다른 process가 work 행을 더한 뒤 각 경로가 따라잡은 work 상태가 처음부터 만든 상태와 같다. 캐시 정체가 바뀐 DB에서는 처음부터 다시 만든다.
3. PRD 000의 같은 work 동시 전이 경합 테스트가 따라잡기를 켠 상태에서 통과한다. 두 process가 같은 work id나 같은 dedupe 값으로 동시에 enqueue해도 결과(받아들임·거절·중복 처리)가 둘을 차례로 enqueue했을 때와 같다.
4. 측정에서 세 작업이 decode한 행 수는 관계없는 행이 1만·10만·30만 개일 때 모두 같다. 시간, 첫 open 시간, 046 착수 직전 commit의 시간은 기록으로 남긴다.
5. fmt, workspace clippy `-D warnings`, 변경한 package 테스트, 031 runner의 029 owner 명령이 통과한다.

## Closure Evidence

1. 거른 읽기·따라잡기·진단 값 구현과 동등성 테스트.
2. 측정 방법과 결과표(로컬 evidence).
3. Cargo 검증 기록.

PRD 010은 PRD 000의 닫힌 결과만 소비한다.

## 구현 메모 및 검증 근거 (2026-11-01)

실행 기록은 로컬 `.omo/evidence/spec046/prd010/`(추적하지 않음)에 있고 `implementation.md`와 `measurement.md`에 정리했다.

- 거른 읽기: `DurableEventFilter`(session id, kind 목록, work id 접두어. 각각 생략할 수 있다)와 `DurableEventStore::visit_matching`.
  - filter 하나가 색인으로 답하는 `SELECT sequence` 하나이고, 여럿이면 `UNION`으로 합쳐 행마다 한 번씩 sequence 순으로 넘긴다. 접두어는 `work_id >= 접두어 AND work_id < 상한`으로 고른다. 상한은 마지막 code point를 다음 값으로 바꾼 문자열이다(UTF-8의 byte 순서가 code point 순서와 같다).
  - 고른 행만 읽고, 읽은 행은 모두 checksum, 색인 열과 record의 일치, compatibility를 검증한 뒤 조건을 한 번 더 확인한다. 요약에 decode한 행 수(`decoded`)를 더했다.
- 소비자 전환: 범위 2의 다섯 소비자가 각자의 filter로 읽는다. 판정 코드(visitor)는 그대로이며, 읽기를 filter를 받는 `*_through`로 나눠 테스트가 전체 읽기와 비교한다.
  - 웹 감시 ledger와 위임 작업 관측 ledger는 owner session과 work id 접두어, 결정 마감 dispatch는 owner session과 `work.terminal`로 거른다. 모두 visitor가 보는 조건 그대로다.
  - 두 통지 저장소는 fact kind와, work 행 중 correlation id(`web-watch-notification:`·`entrusted-notification:`)나 관측 wake의 work id(`observation:`)로 시작하는 것만 읽는다. 그런 id는 `WebNotifyIntent::correlation`, `EntrustedNotificationIntent::from_record`, `ObservationWake::durable_work_id`가 만든다.
- work 상태 따라잡기: `DurableEventStore::work_state()`.
  - process 캐시가 work 상태와 그 상태가 반영한 마지막 sequence를 PRD 000의 저장소 정체와 함께 둔다. 그 검증을 다시 쓸 수 있을 때만 뒤의 행을 읽고, 아니면 처음부터 만든다. 읽는 행은 work reducer가 다루는 kind 9개뿐이다.
  - dispatch의 `replay_work_state`와 그 호출처, child recorder의 work 상태 재생이 이것을 쓴다.
- 기록으로 이어지는 판단: 결정 append(`append_deciding`)는 쓰기 transaction(`BEGIN IMMEDIATE`) 안에서 따라잡은 work 상태를 판단에 넘긴다. 판단이 넣을 행을 정하면 같은 transaction에서 정규화, 전이 검증, insert, commit을 한다.
  - enqueue는 로그 크기 한도, 같은 work 확인(identity 충돌과 기존 기록 반환), 열린 work 한도, payload 쓰기를 그 안에서 한다.
  - child recorder는 lease·finish·cancel의 판단을 그 안에서 한다.
  - 기존 `append`도 같은 경로를 지나므로 dispatch의 기록도 그 안에서 전이 검증을 받는다.
- 캐시: PRD 000대로 commit이 성공한 뒤에만 바뀌고, `CommitUnknown`이면 버린다.
  - 읽기, 따라잡기, 판단 중의 읽기, reducer 가운데 하나라도 실패하면 검증과 work 상태 캐시를 모두 버려, 다음 작업이 처음부터 검증한다.
  - work 계열이 아닌 기록을 넣을 때도 reducer를 적용해 work 상태 캐시를 새 마지막 행으로 옮긴다. PRD 000 코드는 runtime stop·restart 요청을 적용하지 않고 캐시의 위치만 옮겼다(그 캐시를 읽는 곳이 없어 드러나지 않던 문제다).
- 진단 값: `decoded_event_count()`는 process 안에서 이 저장소의 모든 handle이 decode한 행 수다.
- 다시 검색한 production 경로: work 상태를 만들거나 저장소 전체를 읽는 곳을 다시 찾았다. 위 경로 밖에 남은 전체 읽기는 029 recovery admission(비범위), child *상태* 재생(`DurableChildRecorder::replay_state`, work 상태가 아님), agent loop의 workflow fact 읽기(범위 2의 소비자가 아님)다.
- 테스트:
  - `crates/shacs-session/tests/durable_event_store.rs`: 거른 읽기와 전체 visit의 동등성(접두어 경계와 합집합 12가지, decode 행 수까지), 손상·미지원 행, 다른 process가 더한 work의 따라잡기, 바뀐 DB의 재구성, 다른 kind append 뒤의 캐시, 다른 process의 취소를 따라잡은 정규화, 관계없는 행 수와 무관한 decode 행 수, 판단 중 손상 발견 뒤의 캐시 폐기.
  - `tests/durable_work.rs`: 두 process가 같은 work id, 같은 dedupe 값, 다른 identity의 같은 work id를 동시에 enqueue하는 경합.
  - `tests/durable_child.rs`: 다른 process가 바꾼 work를 child recorder가 판단하는 경우.
  - `src/durable_event.rs`: `WORK_STATE_KINDS`가 default registry의 kind 가운데 reducer가 다루는 것과 정확히 같은지.
  - `crates/shacs-core/src/runtime/durable_filtered_reads_tests.rs`: 다섯 소비자의 동등성.
  - `durable_work.rs` 안의 enqueue 테스트 6개는 append closure 대신 같은 단계의 hook을 받도록 바꿨고, 판정은 그대로다.
- 변이 확인: 다른 kind append에서 reducer 빼기, 따라잡지 않고 캐시 쓰기, 접두어 상한을 접두어 자신으로 바꾸기, 검증 재사용 여부를 보지 않기, 소비자마다 filter 하나 틀리게 하기, 판단 실패 때 캐시 남기기를 각각 넣어 해당 테스트가 실패함을 확인하고 되돌렸다.

## 종료 확인 (2026-11-01)

- 범위: 거른 읽기, 소비자 전환, work 상태 따라잡기와 결정 append, 진단 값, 측정. 저장소 형식과 공개 입구([PRD 000](000-sqlite-store-behind-existing-api.md)), recovery admission의 검사, 오래된 기록 정리는 포함하지 않는다.
- 수용 조건:
  1. 다섯 소비자 각각, 다른 session·kind·work id(비슷한 접두어 포함)를 섞은 저장소에서 거른 읽기와 전체 읽기가 같은 결과를 내고, 거른 읽기가 더 적은 행을 decode한다.
  2. 다른 process가 work 행을 더한 뒤 `work_state()`, append 정규화, enqueue, child recorder가 따라잡은 상태가 처음부터 만든 상태와 같다. 마지막 sequence가 같은 다른 DB로 바꾸면 처음부터 다시 만든다.
  3. PRD 000의 같은 work 동시 전이 경합 테스트가 따라잡기를 켠 상태에서 통과한다. 두 process가 같은 work id, 같은 dedupe 값, 다른 identity의 같은 work id로 동시에 enqueue한 결과가 차례로 넣은 결과와 같다.
  4. 측정: 아래 표와 같다. 세 작업이 decode한 행 수는 관계없는 행 수와 무관하다.
  5. Google 옵션을 붙인 `cargo fmt --all -- --check`, workspace clippy `-D warnings`, 변경한 package 테스트(`shacs-session` 148, `shacs-core` 1,681, `shacs-cli` 466, `shacs-api` 118 통과, 실패 0. 테스트가 띄운 자식 process 실행은 별도), 031 runner의 029 owner 명령 2개가 통과했다. 마지막 소스로 Dockerfile의 builder 단계(`rust:1.88-bookworm`, `cargo build -p shacs-cli --release --locked`)와 Rust 1.88.0의 `cargo check --locked --tests -p shacs-session -p shacs-core`도 통과했다. 이 check가 낸 경고는 모두 이번에 바꾸지 않은 줄에서 나온다. 034 runner 코드(`generated_media_release`)의 Linux 쪽 경고들과, `tests/durable_dispatch.rs`에 원래 있던 `unlock()` 호출이 Rust 1.88에서 내는 이름 충돌 경고(`unstable_name_collisions`) 하나다.
- 측정: 웹 감시 하나의 기록 50행과 매회 다른 process가 더한 새 work 행 10개를 고정하고, 관계없는 session·kind의 행만 늘렸다. release build, 10회 중앙값이다. 046 직전 값은 `92c1e72`의 `events.log` 저장소를 같은 행 구성으로 잰 것이다. 모두 임시 폴더에서 쟀고 data directory는 쓰지 않았다.

  | 관계없는 행 | decode 행 수 (work 상태 / ledger 읽기 / enqueue) | 새 저장소 중앙값 | 046 직전 중앙값 | 첫 open (새 / 직전) | DB 크기 |
  |---:|---|---|---|---|---:|
  | 1만 | 20 / 60 / 20 | 0.33 / 0.59 / 4.8 ms | 51 / 104 / 112 ms | 39 ms / 52 ms | 8.8 MB |
  | 10만 | 20 / 60 / 20 | 0.34 / 0.59 / 4.7 ms | 504 / 1,008 / 1,018 ms | 377 ms / 498 ms | 72 MB |
  | 30만 | 20 / 60 / 20 | 0.37 / 0.60 / 4.7 ms | 1,518 / 3,007 / 3,062 ms | 1,135 ms / 1,508 ms | 214 MB |

  DB 크기는 DB와 `-wal`·`-shm`의 합이며 512 MiB 아래다. 웹 감시 ledger 읽기는 crate 밖에서 부를 수 없어, 같은 저장소 사용(새 handle, 같은 filter, reducer, terminal payload 읽기)을 측정 harness에 옮겨 쟀다.
- 검증한 소스: `92c1e72` 위에 PRD 000과 PRD 010의 변경을 모두 담은 커밋 전 마지막 작업 트리. 위 검증, Rust 1.88 빌드, 측정을 모두 이 소스로 했다.
- Oracle 독립 검증: **PASS**(2차).
  - 1차는 BLOCKED였다. 결정 append의 판단 중 읽기가 손상을 찾아도 캐시가 남아, 다음 기록이 그 손상을 다시 읽지 않고 성공할 수 있었다. 판단 중 읽기·reducer 실패를 기록해 캐시를 버리게 고치고 회귀 테스트를 더했다.
  - 같은 검증 실행에서 clippy가 큰 enum 변형 두 개와 불필요한 closure 하나를 지적해, `#[allow]` 없이 고쳤다.
- 남은 제한:
  - 두 통지 저장소의 work 행 filter는 correlation id와 관측 wake의 work id를 만드는 규칙에 기댄다. 그 규칙을 따르지 않는 행을 직접 넣은 DB에서는 전체 읽기와 결과가 다를 수 있다. SQLite를 거치지 않고 색인 열만 바꾼 행이 거른 읽기에서 빠질 수 있다는 SPEC의 남은 위험도 그대로다.
  - `enqueue_json`의 payload 쓰기(fsync 포함)가 SQLite 쓰기 잠금을 쥔 채로 돈다. 그동안 다른 기록은 `busy_timeout` 5000 ms 안에서 기다린다.
  - 같은 work를 다시 enqueue하면 work 상태가 가리키는 enqueue 기록을 돌려준다. terminal 항목 퇴출(512개) 뒤 같은 work id를 다시 넣은 경우에만 예전(전체 scan의 첫 일치)과 다르다.
  - 첫 open과 recovery admission은 지금처럼 모든 행을 읽는다(30만 행에서 약 1.1 s). process 안에 저장소 handle이 하나도 남지 않으면 캐시도 사라져 다음 open이 다시 모두 검증한다.
  - `DurableEventFilter`의 kind 목록이 SQLite bind 수 한도(32,766)를 넘으면 오류가 난다. 지금 production 목록은 1–3개다.
