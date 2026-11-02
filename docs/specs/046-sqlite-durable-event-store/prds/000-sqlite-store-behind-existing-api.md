# PRD 000. SQLite 저장소와 공개 입구 유지

Status: Complete (Scoped)

Depends on: 없음. 뒤 번호 PRD에 의존하지 않는다.

## Goal

`DurableEventStore`의 저장소를 `events.log`에서 SQLite 파일로 바꾼다. 공개 입구와 029 계약의 뜻은 그대로 두어 소비자 코드를 바꾸지 않는다. 옛 `events.log`는 옮기지 않고, 남아 있으면 writable start를 막는다.

## Scope

1. **schema:** `runtime/durable-events/events.sqlite3`에 `events` 테이블을 둔다.
   - 열: sequence(INTEGER PRIMARY KEY), event_id(UNIQUE), session_id, turn_id, kind, work_id(inline payload 최상위의 `work_id` 문자열, 없으면 NULL), schema_version, recorded_at, 정규화한 record JSON, record checksum.
   - 색인: (session_id, work_id, sequence), (kind, sequence).
   - `BEFORE UPDATE`·`BEFORE DELETE` trigger로 행의 수정과 삭제를 거절한다.
   - 저장 형식 판은 `PRAGMA user_version = 1`이다.
2. **열기 순서와 저장소 판:** open은 두 단계로 연다.
   - 먼저 `events.log`·`events.sqlite3`·`events.sqlite3-wal`·`events.sqlite3-shm`을 `symlink_metadata`로 확인한다. 옛 `events.log` 항목이 있으면 다른 확인보다 먼저 범위 8대로 막는다. DB 파일이 없는데 `-wal`이나 `-shm`이 있으면 새 DB를 만들지 않고 `Corruption`으로 막는다. 넷 다 없을 때만 빈 DB를 schema와 함께 만든다.
   - 기존 DB는 쓰기 연결을 열지 않고 read-only 연결로 `user_version`과 schema를 먼저 확인한다. 판 확인을 통과하기 전에는 DDL·DML·checkpoint·파일을 바꾸는 PRAGMA를 실행하지 않는다. DB의 판·schema·행은 바뀌지 않으며, SQLite가 WAL DB를 읽으려고 부속 파일을 만들거나 그 안의 읽기 표시를 고치는 것은 허용한다.
   - `user_version`이 1이 아니면 행을 읽지 않고 compatibility를 `UnsupportedFrameVersion { found: user_version }`으로 보고하며 read-only다(지금 frame 판이 맡던 자리). `user_version`이 1인데 이 저장소의 테이블·trigger·색인이 정의와 다르면 `Corruption`이다.
   - 판 확인을 통과했을 때만 범위 7의 설정을 적용한 쓰기 연결을 연다.
3. **append 순서와 캐시 공개:** append는 아래 순서를 한 쓰기 transaction(`BEGIN IMMEDIATE`) 안에서 지킨다.
   1. 저장소 정체를 확인한다. 다르면 전체를 다시 검증한다.
   2. work 계열 기록이면 work 상태를 만든다(이 PRD에서는 캐시가 유효하면 캐시, 아니면 처음부터 재생).
   3. 입력 검증, redaction, record 크기 제한, work 정규화와 전이 검증을 지금과 같은 순서와 의미로 한다.
   4. 마지막 sequence 다음 값을 부여해 행을 넣고 commit한다.

   commit이 성공한 뒤에만 process 캐시(저장소 정체, work 상태)를 새 값으로 바꾼다. commit 전 실패는 `DefinitelyNotCommitted`이며 캐시를 바꾸지 않는다. commit 중 실패는 `CommitUnknown`이며 저장소 정체와 work 상태 캐시를 모두 버려 다음 접근에서 다시 만든다.
4. **검증과 캐시 정체:** 각 process의 첫 open은 모든 행을 sequence 순으로 읽어 checksum, 색인 열과 record의 일치, sequence 연속성(1부터 틈·중복 없음), compatibility를 검증한다. 검증이 끝나면 process 캐시에 저장소 정체(DB 파일 device·inode, `PRAGMA schema_version`, `user_version`, 마지막 sequence와 그 행의 checksum)를 남긴다. 같은 process의 다음 open과 append는 정체가 같고 마지막 sequence가 같거나 늘었을 때만 늘어난 행을 검증하고, 하나라도 다르면 전체를 다시 검증한다. 파일 전체 SHA-256 stamp(`file_stamp`)는 지운다.
5. **scan과 visit:** 
   - `scan(max_records)`는 지금처럼 모든 행을 검증하고 앞의 `max_records`개만 돌려주며, `truncated`·`last_sequence`·compatibility는 저장소 전체 기준이다.
   - `visit_from_sequence(after)`는 `after` 다음 행부터 읽어 넘긴다. 그 앞 행은 캐시 정체가 보장하는 검증 결과를 쓰고, 정체가 다르면 먼저 전체를 검증한다. `last_sequence`는 넘긴 행이 없어도 저장소의 마지막 sequence다.
   - 두 요약의 `incomplete_tail`은 항상 false이고 `bytes_scanned`는 읽은 record JSON의 byte 수다.
   - `Corruption { offset }`의 offset은 문제가 된 행의 sequence이고, 저장소 전체의 문제(schema 불일치 등)는 0이다.
6. **공개 입구의 다른 값:** `path()`는 DB 파일을 가리킨다. `root()`와 오류 종류는 그대로다.
7. **SQLite 설정:** 쓰기 연결에 WAL, `synchronous=FULL`, macOS의 `fullfsync`와 `checkpoint_fullfsync`를 적용한다. 파일을 바꾸는 설정(WAL 전환)은 판 확인을 통과한 쓰기 연결에서만 한다. 모든 연결에 `busy_timeout` 5000 ms를 적용한다. 새로 만들거나 판 확인을 통과한 쓰기 저장소의 DB와 `-wal`·`-shm` 파일은 0600이다(지원하지 않는 판의 저장소는 권한을 고치지 않는다). 열기 전에 symlink와 일반 파일이 아닌 항목을 거절한다.
8. **옛 파일:** open은 `symlink_metadata`로 `events.log` 항목을 확인한다. 종류와 관계없이 있으면 별도 오류(예: `LegacyLogPresent`)를 내고 DB를 만들지 않는다. 029 recovery admission은 이를 blocked 문제로 보고하고 정리 방법(옛 파일을 지우거나 다른 곳으로 옮기기)을 안내한다. 그 항목은 읽지도 바꾸지도 않는다.
9. **연결된 표면:**
   - `evaluate_durable_recovery`의 저장소 존재 확인, incomplete tail 분기, recovery hint를 새 저장소 기준으로 바꾼다. `DiscardIncompleteTail`은 SQLite에서 생기지 않는다. checkpoint 위조 검사는 지금처럼 처음부터의 재생 결과와 대조한다.
   - `runtime migrate`의 event·queue·scheduler·child 계열은 inventory 경로를 DB로 바꾼다. precondition digest는 한 read transaction 안에서 `user_version`, 이 저장소가 소유한 schema 정의(`sqlite_master`의 SQL), 모든 행의 모든 열을 sequence 순으로 넣어 만든 SHA-256이다.
   - durable work의 512 MiB 한도는 DB, `-wal`, `-shm` 파일 크기의 합으로 재고, enqueue 직렬화 잠금을 쥔 채로 잰다.
   - `spec035_tasks_source`의 저장소 존재 확인과 `runtime inspect`·diagnostics의 경로 표기를 바꾼다.
10. **의존성:** `shacs-session`에 `rusqlite = { version = "=0.40.2", features = ["bundled"] }`를 더한다. Rust 1.88로 빌드되지 않으면 이 PRD를 멈추고 사용자에게 묻는다. Rust 판 변경은 AGENTS.md가 정한 세 곳(`rust-version`·`Cargo.lock`·`Dockerfile`)을 함께 바꾸는 별도 결정이다.
11. **테스트 이전:** `events.log` byte를 직접 읽거나 고치던 테스트를 같은 상황의 DB 조작으로 다시 쓴다. 알려진 위치는 아래와 같고, 구현 전에 다시 검색해 확인한다. 031 runner가 고정한 두 테스트는 이름과 판정을 유지한다.
    - `crates/shacs-session`: `src/durable_event.rs`·`src/durable_work.rs`·`src/durable_migration.rs`의 내부 테스트, `tests/durable_event_store.rs`, `tests/durable_replay.rs`, `tests/durable_work_compat.rs`(legacy frame을 DB 행으로 넣어 같은 판정 확인)
    - `crates/shacs-core`: `tests/durable_child.rs`, `tests/durable_dispatch.rs`, `tests/spec033_automation_dispatch.rs`, `tests/it/spec035_tasks_source.rs`, `tests/it/spec042_web_watch/deadline.rs`
    - `crates/shacs-cli`: `src/lib.rs`의 내부 테스트, `tests/it/spec035_tasks_fail_closed.rs`
    - `crates/shacs-api`: `tests/it/spec035_tasks_fail_closed.rs`
12. **문서:** README·`docs/USAGE.md`의 `events.log` 설명과 Docker 규칙, 029 SPEC 본문의 대체 안내(Status 줄은 그대로), 스펙 색인의 046 행.

## Non Scope

- 소비자의 거른 읽기와 work 상태 따라잡기([PRD 010](010-indexed-reads-for-hot-consumers.md)).
- checkpoint·trace log·work payload 저장 형식, 오래된 기록 정리, 옛 `events.log`의 변환.

## Parent Requirement Mapping

- [046 SPEC](../SPEC.md)의 소유 범위 1–6·8·9와 수용 조건 1–5, 이 PRD 변경에 대한 수용 조건 7을 소유한다.

## Required Contract

1. 소비자가 보는 record, sequence, compatibility, 오류 종류는 029와 같은 뜻이다. 새로 생기는 오류는 옛 파일 발견 하나다.
2. commit된 행만 보인다. 반쯤 쓴 행은 보이지 않는다.
3. 손상을 성공으로 합성하지 않는다. checksum 불일치, 색인 열과 record의 불일치, 연속성 위반은 `Corruption`이고 writable start를 막는다.
4. 캐시는 저장소 정체가 같고 행이 뒤에만 붙었을 때만 이전 검증 결과를 다시 쓰며, commit이 성공한 뒤에만 새 값으로 바뀐다.
5. 지원하지 않는 저장소 판의 DB는 열어도 판·schema·행이 바뀌지 않고, 쓰기 연결이 열리지 않으며, WAL로 바뀌지 않는다.

## Acceptance Criteria

1. 046 SPEC 수용 조건 1–5를 테스트로 확인한다. 동시 append 테스트는 독립 process를 써서 sequence에 틈과 중복이 없음을 본다.
2. 손상 상황은 저장소 입구를 거치지 않고 만든다(DB 파일 byte를 직접 고치거나, 저장소의 trigger를 끈 SQLite 연결로 고친다): 행의 record나 checksum을 바꾼 DB, 색인 열만 바꾼 DB, 행 하나를 뺀 DB, record의 schema_version·kind를 바꾸고 checksum을 맞춘 DB, `user_version`을 2로 바꾼 DB.
   - 새 process의 첫 open은 각각을 `Corruption`이나 read-only로 판정한다.
   - 캐시를 가진 process는 마지막 sequence가 같은 다른 DB로 바꾼 경우와 schema를 바꾼 경우를 다음 open에서 알아채고 전체를 다시 검증한다. 이미 검증한 행을 직접 바꾼 경우는 그 행을 다시 읽는 `scan`·`visit_from_sequence`에서 `Corruption`이 된다(SPEC의 남은 위험대로 다음 open만으로는 알아채지 못한다).
   - SQL `UPDATE`·`DELETE`는 trigger가 거절한다.
3. `user_version`을 2로 바꾼 DB를 열고 `runtime inspect`를 실행해도 DB의 `user_version`·schema·행이 그대로이고, 쓰기 연결이 열리지 않으며, journal mode가 바뀌지 않는다. 쓰기 연결이 열리지 않았음은 연결을 여는 곳에 둔 시험용 계수로 확인한다. 부속 파일의 byte는 비교하지 않는다.
4. 옛 `events.log`가 일반 파일, 깨진 symlink, 폴더인 각 경우와, `events.sqlite3` 없이 `-wal`이나 `-shm`만 있는 경우에 `runtime inspect`가 이를 보고하고 `runtime start`가 writable start를 거절한다. 그 항목의 byte·종류·수정 시각이 바뀌지 않고, 폴더의 항목 이름 목록이 전후로 같다. 옛 `events.log`와 부속 파일이 함께 있으면 옛 파일 문제를 먼저 보고한다.
5. 두 독립 process가 같은 work에 이어지는 전이(예: lease 뒤 terminal)를 동시에 기록하는 경합 테스트에서, 전이를 잃지 않고 잘못된 전이는 거절되며, 최종 work 상태가 기록된 행을 처음부터 재생한 상태와 같다.
6. migration precondition digest는 색인 열, schema, `user_version`, 행 삭제, 행 추가가 바뀌면 달라지고, 그대로면 같다.
7. WAL에 쌓인 크기만으로 한도를 넘긴 상태에서 enqueue가 거절된다.
8. 031 runner의 029 owner 명령 두 개가 통과한다.
9. fmt, workspace clippy `-D warnings`, 변경한 package 테스트가 통과한다.

## Closure Evidence

1. 저장소 구현, 다시 쓴 테스트, 옛 파일 거절의 inspect·start 관측.
2. Rust 1.88 `--locked` 빌드, Docker 이미지 빌드, `Cargo.lock` 변경 내용, Cargo 검증 기록, 031 runner 029 owner 명령 결과.
3. 034 runner의 vendor·격리 빌드 경로가 새 의존성으로 동작하는지 확인한 방법과 결과(실행은 사용자 승인 후).
4. 고친 사용자 문서와 029 대체 안내.

PRD 000은 소비자의 거른 읽기 없이 닫힌다. 뒤 번호 PRD의 완료를 기다리지 않는다.

## 구현 메모 및 검증 근거 (2026-11-01)

실행 기록은 로컬 `.omo/evidence/spec046/prd000/`(추적하지 않음)에 있고 `implementation.md`에 정리했다.

- 저장소: `crates/shacs-session/src/durable_event.rs`가 `runtime/durable-events/events.sqlite3`를 쓴다. 공개 입구와 029 계약의 뜻은 그대로이고, 새 오류는 `LegacyLogPresent` 하나다.
  - 새 저장소는 같은 폴더의 숨은 임시 이름에서 schema와 `user_version = 1`까지 만든 뒤 hard link로 제자리에 놓는다. 경로에서 찾은 빈 DB는 이 저장소가 만든 것이 아니므로 바꾸지 않고 `UnsupportedFrameVersion { found: 0 }`로 읽는다.
  - 쓰기 연결은 그 연결에서 판과 schema를 확인한 뒤에만 WAL 등 파일을 바꾸는 설정을 한다. 지원하지 않는 판을 본 뒤에는 쓰기 연결을 보관하지 않는다.
  - process마다 저장소 하나에 연결 하나를 둔다. 모든 연결은 close 때 checkpoint하지 않고, 마지막 handle이 사라질 때만 같은 파일·판·schema이고 옛 log가 없으면 기다리지 않는 `wal_checkpoint(TRUNCATE)`를 한다.
- 캐시: 검증은 캐시를 바꾸지 않고 결과를 돌려준다. 읽기는 끝난 뒤, append는 commit 뒤에만 반영한다. 이전 검증을 다시 쓰는 규칙은 `reusable_verification` 하나다(같은 파일·판·schema·registry이고 이전 마지막 행이 그대로일 때).
- 연결된 표면: recovery admission의 옛 log 문제와 정리 hint, `runtime migrate`의 inventory 경로와 논리 digest, DB와 `-wal`·`-shm`을 합한 512 MiB 한도, `spec035_tasks_source`의 저장소 존재 확인.
- 테스트: 범위 11의 위치를 다시 검색해 DB 조작으로 다시 썼다. 031 runner가 고정한 두 테스트는 이름과 판정이 그대로다.
- 의존성: `rusqlite = "=0.40.2"`(bundled, SQLite 3.53.2), `Cargo.lock`에 package 7개 추가. Rust 1.88로 빌드된다.
- 문서: README·`docs/USAGE.md`, 029 SPEC의 대체 안내(Status 줄은 그대로), 스펙 색인의 046 행.

## 종료 확인 (2026-11-01)

- 범위: 위 저장소 교체와 연결된 표면. 거른 읽기와 work 상태 따라잡기([PRD 010](010-indexed-reads-for-hot-consumers.md))는 포함하지 않는다.
- 최종 검증: Google 옵션을 붙인 `cargo fmt --all -- --check`, workspace clippy `-D warnings`, 변경한 package 테스트(`shacs-session` 전부, `shacs-api` 118, `shacs-core` 1,676(테스트가 띄운 자식 process 실행 6번 별도), `shacs-cli` 466 통과, 실패 0), 031 runner의 029 owner 명령 2개, Docker `rust:1.88-bookworm` builder 단계의 `cargo build -p shacs-cli --release --locked`가 통과했다.
- 전체 workspace 테스트: 사용자 승인을 받아 `--no-fail-fast`로 실행했다(`closure/workspace-tests.log`). suite 188개에서 3,283 passed / 0 failed / 7 ignored이고, 테스트가 띄운 자식 process 실행 10번도 모두 통과했다. 045 종료 때 baseline 실패였던 Spec034 fixture 테스트는 `92c1e72`에서 고쳐져 통과한다.
- 034 runner의 vendor·격리 빌드 경로: 사용자 승인을 받아 `cargo test -p shacs-core --test spec034_release_runner`를 실행했다.
  - 이 checkout에서는 6개 중 5개가 `InvalidConfig`로 실패했다(`closure/spec034-runner-checkout.log`). runner가 작업 트리를 통째로 옮기는 첫 단계는 symlink를 `InvalidConfig`로 거절하는데, 이 checkout에는 추적하지 않는 로컬 symlink 다섯 개(`.codegraph`, `docs/specs/.codegraph`, `docs/refs/` 아래 셋)가 있다. 별도 점검(`closure/spec034-checkout-conditions.txt`)에서 그 단계의 한도를 넘는 `docs/refs/`(항목 45,568개 > 32,768개, 687 MB > 512 MiB)와, runner가 읽는 로컬 fixture `.omo/evidence/spec034/task-12-integration.json`이 없다는 것(`92c1e72`의 commit 설명대로)도 확인했다. 셋 다 이 변경과 관계없는 이 machine의 상태다.
  - 같은 소스를 짧은 경로의 사본에서 실행했다(`closure/spec034-runner-copy.sh`). `git clone` 뒤 작업 트리의 diff와 추적하지 않는 파일을 옮기고 두 diff가 같음을 확인했으며, fixture는 035 보관 폴더의 사본을 넣었다. 6 passed / 0 failed, exit 0이다(554 s, `closure/spec034-runner-copy.log`). 이 시험은 `Cargo.lock` 전체를 `cargo vendor --locked --versioned-dirs`로 옮겨 최소 환경에서 spec034 test target 두 개를 빌드·실행하므로, bundled SQLite의 C 빌드가 격리 경로에서도 된다.
  - 처음 만든 사본은 임시 폴더의 긴 경로 때문에 runner의 linker socket 경로가 `SUN_LEN`을 넘어 실패했다(`closure/spec034-runner-long-path-copy.log`).
- 검증한 소스: `92c1e72` 위의 커밋 전 작업 트리. PRD 010 작업은 이 검증이 끝난 뒤 시작했다.
- Oracle 독립 검증: **PASS**(구현 4차). 1–3차와 3차 이어서의 BLOCKED 지적과 조치는 `implementation.md`에 있다.
- 남은 제한:
  - 이미 검증한 행을 trigger를 끈 연결로 바꾸면 다음 open은 알아채지 못하고, 그 행을 다시 읽는 읽기에서만 `Corruption`이 된다(SPEC의 남은 위험).
  - DB를 만들다 멈춘 process는 숨은 임시 파일을 남길 수 있다.
