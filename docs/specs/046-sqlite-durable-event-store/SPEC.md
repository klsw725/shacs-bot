# 046. durable event store의 SQLite 전환 구현 명세

Status: Complete (Scoped)

Consumes: 029 durable event store, checkpoint와 writable admission, durable work, stored-data migration(PRD 000·001·002·006), 036·042의 durable event 소비자(위임 작업 관측·통지 저장소, 웹 감시 ledger·통지 저장소, 결정 마감 dispatch), 031 release runner의 029 owner fact, 034 release runner의 vendor·격리 빌드 경로

## 목적과 완료 경계

[029](../029-durable-runtime-recovery-and-data-migration/SPEC.md)의 durable event store는 `${data_dir}/runtime/durable-events/events.log`에 checksum이 붙은 frame을 한 줄씩 덧붙인다. 이 형식에는 기록이 쌓일수록 비용이 커지는 곳이 세 군데 있다.

- append, open, `scan`, `visit_from_sequence`마다 process 캐시를 다시 쓸지 판단하는 stamp를 만들려고 파일 전체의 SHA-256을 계산한다(`file_stamp`).
- 웹 감시 ledger·통지 저장소, 위임 작업 관측·통지 저장소, 결정 마감 dispatch는 session·kind·work id 몇 개에 해당하는 기록을 찾으려고 `visit_from_sequence(0)`으로 파일 전체를 읽는다. durable work를 enqueue할 때와 dispatch·child recorder가 work 상태를 만들 때도 처음부터 읽는다.
- 오래된 기록을 지울 방법이 없어 파일이 512 MiB에 닿으면 새 enqueue를 받지 않는다.

이 스펙은 저장소를 SQLite 파일 하나로 바꾸고 `DurableEventStore`의 공개 입구를 유지해 소비자 코드를 그대로 둔다. 자주 읽는 소비자와 work 상태를 만드는 경로에는 필요한 행만 읽는 읽기를 더한다. record 형식, sequence·compatibility·쓰기 실패 분류의 뜻, checkpoint와 writable admission 규칙은 029를 따른다. 손상을 알아채는 시점은 소유 범위 5와 [남은 위험과 한계](#남은-위험과-한계)에 적은 만큼만 바뀐다.

기존 `events.log`는 옮기지 않는다. 지금까지의 기록은 시험 운영 자료이며 사용자가 지우기로 했다. 옛 파일이 남아 있으면 runtime은 이를 알리고 writable start를 막는다.

완료되면 사용자는 다음을 경험한다.

- 기록이 쌓여도 기록 쓰기(durable work enqueue 포함), 웹 감시·위임 작업의 지난 기록 읽기, dispatch의 work 상태 갱신이 전체 기록 수에 비례해 느려지지 않는다.
- 손상, 호환되지 않는 기록, 쓰기 실패의 판정과 writable start 차단은 지금과 같은 뜻으로 동작한다.
- 옛 `events.log`가 있으면 `runtime inspect`가 이를 알리고, 사용자가 정리하기 전까지 runtime이 쓰기를 시작하지 않는다.

process가 저장소를 처음 열 때의 전체 검증과 재시작 때의 recovery admission(029의 checkpoint 위조 검사 포함)은 지금처럼 전체 기록을 읽으므로 기록 수에 비례한다.

이 스펙은 다른 스펙이나 신규 기능의 구현을 기다리지 않는다.

## 소유 범위

1. **저장소:** `runtime/durable-events/events.sqlite3` 하나. 기록 하나가 행 하나이고 sequence가 기본 키다. 행은 029 record의 모든 필드와 record checksum을 담고, session id·kind·work id(inline payload 최상위의 `work_id`)로 찾는 색인을 둔다. 저장 형식 판은 `user_version = 1`이다. trigger로 행의 수정과 삭제를 막아 append만 허용한다.
2. **공개 입구 유지:** `DurableEventStore`의 `open`·`open_with_registry`·`append`·`scan`·`visit_from_sequence`·`compatibility`·`is_writable`·`root`·`path`가 같은 뜻으로 동작한다. 저장소 때문에 뜻이 바뀌는 부분(`path`가 가리키는 파일, incomplete tail, 요약 값, 오류 위치, 저장소 판)은 PRD 000이 정한다.
3. **쓰기 실패 분류:** commit 전에 실패하면 `DefinitelyNotCommitted`, commit 중에 실패하면 `CommitUnknown`이다.
4. **SQLite 설정과 의존성:** 저장소 판을 쓰기 연결 없이 read-only 연결로 먼저 확인하고(DB의 판·schema·행을 바꾸지 않는다), 지원되는 판일 때만 WAL, `synchronous=FULL`, macOS의 `fullfsync`·`checkpoint_fullfsync`를 적용한 쓰기 연결을 연다. 모든 연결에 `busy_timeout` 5000 ms를 적용한다. 의존성은 `rusqlite = "=0.40.2"`의 `bundled`(SQLite 3.53.2, 3.51.3의 WAL-reset 수정 포함)이며 Rust 1.88에서 빌드되어야 한다.
5. **변경 감지와 검증:** 각 process의 첫 open은 모든 행을 검증한다. 같은 process의 다음 open과 append는 캐시가 기록한 저장소 정체(파일 device·inode, `schema_version`, `user_version`, 마지막 sequence와 그 행의 checksum)가 그대로이고 새 행이 그 뒤에만 붙었을 때만 새 행을 검증하며, 그렇지 않으면 전체를 다시 검증한다. 모든 읽기는 행마다 checksum과, 색인 열이 record에서 다시 뽑은 값과 같은지를 확인한다.
6. **옛 파일과 남은 부속 파일 처리:** `runtime/durable-events/`에 `events.log`라는 항목(일반 파일, symlink, 그 밖의 종류 모두)이 있으면 그 항목을 읽거나 바꾸지 않고, recovery admission 문제로 보고해 writable start를 막는다. DB 파일 없이 SQLite 부속 파일(`-wal`·`-shm`)만 있어도 막는다. 두 경우 모두 새 DB를 만들지 않는다.
7. **필요한 행만 읽기:** session id·kind 목록·work id 접두어로 거른 읽기와, 이미 본 sequence 다음 행만 읽는 범위 읽기를 더한다. 웹 감시 ledger·통지 저장소, 위임 작업 관측·통지 저장소, 결정 마감 dispatch는 거른 읽기를 쓴다. work 상태를 만드는 모든 production 경로(durable work enqueue, append 정규화, dispatch의 `replay_work_state`, child recorder)는 하나의 캐시 계약으로 새 행만 따라잡는다.
8. **연결된 표면:** 029 recovery admission의 저장소 존재 확인과 incomplete tail 분기, `runtime migrate`의 event 계열 inventory와 precondition digest(한 read transaction 안의 논리 digest), durable work의 512 MiB 한도(DB와 SQLite 부속 파일을 합친 크기), `runtime inspect`·diagnostics의 저장소 표기를 새 저장소 기준으로 바꾼다.
9. **문서:** README·`docs/USAGE.md`의 `events.log` 설명, Docker에서 host와 container가 같은 DB를 함께 열지 않는다는 규칙, 029 SPEC 본문의 대체 안내(Status 줄 제외), 스펙 색인.

## 기존 owner와의 경계

| 기존 owner | 046이 소비하는 것 | 046이 소유하지 않는 것 |
|---|---|---|
| [029](../029-durable-runtime-recovery-and-data-migration/SPEC.md) | record schema, sequence 규칙, schema registry와 compatibility, 쓰기 실패 분류의 뜻, checkpoint와 그 위조 검사, writable admission, durable work reducer, local safety bound | checkpoint·trace log·work payload 저장 형식, migration runner의 다른 계열, 029 Status 줄 |
| [036](../036-entrusted-work-continuity-and-change-notification/SPEC.md)·[042](../042-conversational-web-watch-and-trustworthy-silence/SPEC.md) | durable event를 읽는 저장소·ledger·dispatch | 관측·판정·알림 규칙 |
| [031](../031-configuration-runtime-layout-and-execution-snapshots/SPEC.md) release runner | 029 owner fact(029 Status 줄, `durable_dispatch_restores_due_inbound_and_requeues_stale_lease`, `subagent_runtime_restart_discards_late_success_after_durable_cancellation`) | runner catalog와 결과 형식 |
| [034](../034-generated-media-and-rich-file-context-expansion/SPEC.md) release runner | `Cargo.lock` 전체를 `cargo vendor --locked`로 옮겨 격리 빌드하는 경로 | runner의 tool closure 규칙 |

## 반드시 지킬 것

- record의 필드와 뜻, sequence 단조 증가, compatibility 판정, record 크기 제한과 local safety bound를 바꾸지 않는다.
- 029의 checkpoint 위조 검사(checkpoint를 처음부터의 기록 재생 결과와 대조)를 약하게 만들지 않는다.
- 029 Status 줄과, 031 runner가 이름으로 고정한 두 테스트의 이름과 판정을 유지한다.
- 옛 `events.log`를 읽거나 지우거나 고치지 않는다.
- checkpoint, trace log, work payload 저장 형식은 바꾸지 않는다.
- 검증을 통과시키려고 테스트를 지우거나 약하게 바꾸지 않는다. `events.log` byte를 직접 읽거나 고치던 테스트는 같은 상황(손상 행, sequence 틈, 미지원 schema·kind·저장소 판, 크기 한도, legacy record)을 DB에 만들어 같은 판정을 확인하도록 다시 쓴다. `#[ignore]`나 `#[allow(...)]`로 남기지 않는다.
- 새 C/C++ 빌드 도구나 빌드 중 download를 요구하지 않는다(`bundled`는 이미 쓰는 `cc`로 빌드된다).

## 수용 조건

1. 기존 durable event·replay·work·child·dispatch·웹 감시·위임 작업 테스트가 같은 판정으로 통과한다.
2. 쓰기 실패 주입(commit 전, commit 중)이 `DefinitelyNotCommitted`와 `CommitUnknown`으로 나뉜다. 독립 process들이 동시에 append해도 sequence에 틈과 중복이 없다.
3. 손상 행(checksum 불일치, 색인 열과 record 불일치), sequence 틈, 미지원 schema·kind, 모르는 `user_version`이 새 process의 첫 open에서 `Corruption`이나 read-only로 드러나고 writable start를 막는다. 모르는 `user_version`의 DB는 열어도 판·schema·행이 바뀌지 않고 WAL로 바뀌지 않는다(SQLite가 읽기 위해 부속 파일을 다루는 것은 예외). SQL로 행을 수정·삭제하면 거절된다. 캐시를 가진 process는 마지막 sequence가 같은 다른 DB로 바뀐 것과 schema 변경을 다음 open에서 알아채고 전체를 다시 검증하며, 이미 검증한 행의 직접 변경은 그 행을 다시 읽을 때 판정한다.
4. 옛 `events.log` 항목이나 DB 없는 부속 파일이 있으면 `runtime inspect`가 이를 보고하고 writable start가 막히며, 그 항목은 바뀌지 않고 새 DB도 생기지 않는다.
5. 두 process가 같은 work에 이어지는 전이를 동시에 기록해도 전이를 잃거나 잘못된 전이를 받아들이지 않는다.
6. 필요한 행만 읽기의 효과를 decode한 행 수로 판정한다. 대상 소비자가 읽어야 할 행 수와 따라잡을 새 행 수를 고정하고 관계없는 행만 1만·10만·30만 개로 늘린 합성 DB에서, durable work enqueue 한 번, 웹 감시 ledger 읽기 한 번, dispatch 한 번의 work 상태 갱신이 decode한 행 수는 관계없는 행 수와 무관하다. 같은 작업의 시간(10회 중앙값), 첫 open 전체 검증 시간, 046 착수 직전 commit의 `events.log` 시간을 함께 기록한다.
7. fmt, workspace clippy `-D warnings`, 변경한 package 테스트, 031 runner의 029 owner 명령이 통과한다. Rust 1.88의 `--locked` 빌드와 034 runner의 vendor·격리 빌드 경로가 새 의존성으로 동작함을 기록한다. 전체 workspace 테스트와 034 runner 실행은 사용자 승인을 받아 실행한다.

위 조건은 046의 단독 종료 기준이다.

## 남은 위험과 한계

- SQLite WAL은 같은 컴퓨터의 process끼리만 안전하다. Docker 공유 폴더로 host와 container가 같은 DB를 함께 열면 보장하지 않는다.
- DB 파일만 바꾸거나 옮기고 옛 `-wal`을 남기면 SQLite가 그 WAL의 내용을 새 DB 위에 보인다. 사용자 문서에 DB와 부속 파일을 함께 다루라고 적는다.
- SQLite를 거치지 않고 DB 파일 byte를 직접 바꾸면, 이미 검증한 process는 그 행을 읽거나 다음 process가 처음 열 때 알아챈다. 지금처럼 다음 open마다 파일 전체 지문으로 알아채지는 않는다. 같은 방식으로 색인 열만 바뀐 행은 그 사이 거른 읽기에서 빠질 수 있다.
- 손상 범위가 줄 하나에서 DB 전체로 커질 수 있다. 행 checksum은 행 내용의 손상을 잡지만 SQLite 내부 구조가 깨지면 열기 자체가 실패할 수 있다. 이때는 writable start를 막고 사용자가 정리한다.
- 각 process의 첫 open과 재시작 때의 recovery admission은 기록 수에 비례한다. CLI 명령처럼 짧게 도는 process는 실행할 때마다 이 비용을 낸다.
- 오래된 기록 정리(compaction)는 다루지 않는다. 지우기 쉬운 구조만 만든다.
- 옛 `events.log`를 옮기는 길이 없다. 다른 설치본이 있다면 그 기록은 새 저장소에서 보이지 않는다.

## 실행 PRD와 독립 종료

각 PRD는 앞 번호의 닫힌 결과만 소비하며, 구현 전 상태는 모두 `Planned`다.

| PRD | 유일하게 소유하는 결과 | 앞선 PRD 소비 | 자체 완료 표면 |
|---|---|---|---|
| [000 SQLite 저장소와 공개 입구 유지](prds/000-sqlite-store-behind-existing-api.md) | 소유 범위 1–6·8·9, append 순서와 캐시 공개, 테스트 이전 | 없음 | 수용 조건 1–5·7 |
| [010 필요한 행만 읽기](prds/010-indexed-reads-for-hot-consumers.md) | 소유 범위 7과 측정 | 000 | 수용 조건 6·7 |

범위 1–6·8·9와 수용 조건 1–5는 000, 범위 7과 수용 조건 6은 010이 소유한다. 010의 따라잡기는 000이 정한 append 순서 안에서 하므로 수용 조건 5를 다시 확인한다(소비이지 공동 소유가 아니다). 수용 조건 7은 각 PRD가 자기 변경에 대해 확인한다. 010은 000의 저장소 위에서만 구현할 수 있으므로 번호 순서대로 구현한다.

## 종료 증거 (2026-11-01)

PRD [000](prds/000-sqlite-store-behind-existing-api.md)과 [010](prds/010-indexed-reads-for-hot-consumers.md)이 각각 독립 Oracle **PASS**를 받았다(000은 구현 4차, 010은 2차). 마지막 소스의 Rust 1.88 빌드를 더하고 기록의 표현을 바로잡은 뒤, 수용 조건 1–7과 이 종료 기록도 스펙 차원에서 **PASS**로 판정됐다. 이는 현재 self-hosted 지원 범위의 scoped 완료이며 전체 release seal이 아니다. 실행 기록은 로컬 `.omo/evidence/spec046/`(추적하지 않음)에 있다.

- 조건 1: 기존 durable event·replay·work·child·dispatch·웹 감시·위임 작업 테스트가 같은 판정으로 통과한다. `events.log` byte를 다루던 테스트는 같은 상황을 DB에 만들어 다시 썼다. 사용자 승인을 받아 000 종료 때 돌린 전체 workspace 테스트는 suite 188개에서 3,283 passed / 0 failed / 7 ignored였다(000).
- 조건 2: 쓰기 실패 주입이 `DefinitelyNotCommitted`와 `CommitUnknown`으로 나뉘고, 독립 process들의 동시 append에서 sequence에 틈과 중복이 없다(000).
- 조건 3: 손상 행, sequence 틈, 미지원 schema·kind, 모르는 `user_version`이 새 process의 첫 open에서 `Corruption`이나 read-only로 드러나고 writable start를 막는다. 모르는 `user_version`의 DB는 열어도 판·schema·행과 journal mode가 바뀌지 않고 쓰기 연결도 열리지 않는다. SQL 수정·삭제는 trigger가 거절한다. 캐시를 가진 process는 바뀐 DB와 schema 변경을 다음 open에서 알아채고, 이미 검증한 행의 직접 변경은 그 행을 다시 읽을 때 판정한다(000). 기록 전 판단 중의 읽기가 손상을 찾으면 그 process는 검증을 버려 다음 작업이 다시 판정한다(010).
- 조건 4: 옛 `events.log`(일반 파일, 깨진 symlink, 폴더)나 DB 없는 부속 파일이 있으면 `runtime inspect`가 보고하고 writable start가 막히며, 그 항목은 바뀌지 않고 새 DB도 생기지 않는다(000).
- 조건 5: 두 독립 process가 같은 work의 이어지는 전이를 동시에 기록해도 전이를 잃거나 잘못된 전이를 받아들이지 않는다(000). 010의 따라잡기를 켠 상태에서도 통과하고, 같은 work id·dedupe 값·다른 identity로 동시에 enqueue한 결과도 차례로 넣은 결과와 같다(010).
- 조건 6: 관계없는 행 1만·10만·30만 개에서 durable work enqueue, 웹 감시 ledger 읽기, dispatch의 work 상태 갱신이 decode한 행 수는 각각 20·60·20으로 같다. 30만 행에서 10회 중앙값은 4.7 ms·0.60 ms·0.37 ms이고 046 직전 commit의 `events.log`는 3.1 s·3.0 s·1.5 s다. 첫 open 전체 검증은 1.1 s(046 직전 1.5 s), DB 크기는 214 MB다(010).
- 조건 7: fmt, workspace clippy `-D warnings`, 변경한 package 테스트, 031 runner의 029 owner 명령이 두 PRD에서 통과했다. Rust 1.88(`rust:1.88-bookworm`)의 `--locked` release 빌드가 000 종료 때와 마지막 소스에서 모두 되고, 마지막 소스의 테스트도 Rust 1.88로 compile된다(000, 010). 034 runner의 vendor·격리 빌드 경로는 사용자 승인을 받아 짧은 경로의 사본에서 runner 테스트 6개가 모두 통과했다. 이 checkout에서 직접 돌리면 runner가 작업 트리를 옮기는 첫 단계에서 추적하지 않는 로컬 symlink를 `InvalidConfig`로 거절하며, 별도 점검으로 `docs/refs/`의 크기 초과와 로컬 fixture가 없다는 것도 확인했다(000).

[남은 위험과 한계](#남은-위험과-한계)는 그대로이고, 각 PRD의 종료 확인에 적은 제한도 남는다.
