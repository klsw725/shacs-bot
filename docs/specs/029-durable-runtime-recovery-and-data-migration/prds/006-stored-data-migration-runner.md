# PRD 006: stored-data migration runner

## 목표

Session metadata, event, checkpoint, queue, channel, child, trace, diagnostics artifact schema를 안전하게 변환하는 local migration runner를 만들고 partial migration에서 writable runtime을 차단한다. Config/profile transform은 Spec 035가 소유하며, 이 runner는 현재 config loader의 compatibility state만 통합 admission에서 소비한다. 이후 035 migration result는 같은 boundary를 확장하지만 029 closure의 선행 조건은 아니다.

Status: Complete (Scoped). `shacs-session/src/durable_migration.rs`가 현재 v1 durable family inventory와 명시적 v0 fixture transform runner를 제공하고, `shacs-cli`의 `runtime migrate`/admission이 이를 소비한다. Future schema migration은 새 versioned transform을 추가해야 하며 config/profile transform은 계속 Spec 035 소유다.

## 범위

1. Schema inventory와 compatibility plan
2. Dry-run migration plan
3. Start, per-family result, partial, complete marker
4. Idempotent/resumable transform boundary
5. Rollback 가능 범위와 inspect-only fallback
6. Runtime start/update/recover admission 통합
7. Current config compatibility와 runtime path-helper admission 결과 소비

## 비범위

- SaaS migration service
- cross-host data replication
- 모든 migration의 automatic rollback 보장
- partial 상태에서 normal writable runtime 허용
- config/profile schema transform, profile resolution, runtime directory layout 또는 physical owner marker 경로 정의

## SPEC 입력

1. 필수 선행 PRD: `005-durable-trace-log-and-diagnostics-correlation.md`
2. Lifecycle baseline: `../../015-packaging-process-lifecycle-and-upgrades/SPEC.md`
3. PRD 000-005가 정의한 모든 schema family를 소비한다.
4. Config/profile migration과 formal runtime layout owner는 `../../035-configuration-runtime-layout-and-execution-snapshots/SPEC.md`다. 이 ownership reference는 035 구현 완료를 선행 조건으로 만들지 않는다.

## Dependency Cut

1. Migration은 mutation 전에 compatibility와 plan을 기록한다.
2. Family별 transform은 독립 결과를 남기지만 overall completion 전 writable start를 허용하지 않는다.
3. Rollback 불가능한 transform은 명시적으로 inspect-only/manual recovery를 선택한다.
4. Migration runner는 application policy나 session content 의미를 임의 변경하지 않는다.
5. 029 runner는 config/profile file을 transform하지 않는다. 현재 config loader가 산출하는 readable/incompatible 결과를 overall admission에 포함하고, 035는 나중에 같은 input을 current/legacy/future/partial migration result로 확장한다.
6. Diagnostics artifact는 trace record와 별도 schema family로 inventory, plan, transform result를 가져야 한다.

## 구현 요구사항

1. Plan은 source/target version, affected family, action, precondition, rollback capability를 가진다.
2. Dry-run은 source data를 mutate하지 않는다.
3. Start marker는 첫 mutation 전에 durable하게 기록된다.
4. Family result는 skipped/no-op/transformed/failed/blocked를 구분한다.
5. Partial interruption 후 재실행은 completed family를 검증하고 안전하게 resume한다.
6. Unknown newer schema와 missing migration path는 inspect-only로 차단한다.
7. Backup/checkpoint strategy와 cleanup 시점을 명시한다.
8. Migration diagnostics는 secret/raw payload를 노출하지 않는다.
9. Schema inventory는 session metadata, event, checkpoint, queue, scheduler, channel, child, trace, diagnostics artifact family를 각각 구분한다.
10. Config/profile family는 external compatibility result로 표시하며 029 transform 목록에 넣지 않는다. 029 tests는 current loader adapter를 사용하고 035 구현을 요구하지 않는다.

## 정상 시퀀스

1. Runtime update/start가 schema inventory를 읽는다.
2. Migration planner가 dry plan을 만든다.
3. User-visible plan과 compatibility를 확인한다.
4. Start marker를 기록하고 family 순서대로 transform한다.
5. 모든 결과를 검증한 뒤 completion marker를 기록한다.
6. Complete admission 이후에만 writable runtime을 연다.

## 실패 시퀀스

1. Transform 중 crash는 partial marker와 completed family evidence를 남긴다.
2. Resume 불가능하거나 rollback 불가능하면 inspect-only로 남긴다.
3. Checksum/source precondition mismatch는 mutation 전에 차단한다.
4. Completion marker가 없으면 normal start를 허용하지 않는다.

## 검증 관점

1. No-op, single-family transform, multi-family transform을 검증한다.
2. Dry-run non-mutation을 확인한다.
3. 각 family 전/중/후 interruption fixture를 둔다.
4. Resume, rollback, inspect-only blocked path를 검증한다.
5. Newer unknown schema와 missing path를 검증한다.
6. Migration 후 replay/queue/channel/child/trace/diagnostics artifact compatibility를 확인한다.
7. Current config compatibility adapter의 readable/incompatible 결과가 029 admission에서 각각 allow/block으로 결합되는지 확인한다. 035가 추가할 partial/future-unsupported 상태는 035 acceptance에서 같은 boundary로 검증한다.

## 완료 기준

- Dry plan, start, family result, partial, complete marker가 구현된다.
- Partial 상태에서 writable runtime이 열리지 않는다.
- No-op/transform/interruption/resume/blocked matrix가 테스트된다.

## Wave 7 구현 증거

1. Core runner: `crates/shacs-session/src/durable_migration.rs`가 session metadata, event, checkpoint, queue, scheduler, channel, child, trace, diagnostics artifact family를 deterministic order로 inventory/plan/result 처리한다. Plan entry는 source/target version, family, action, precondition digest, rollback capability를 가진다.
2. Marker/ledger: `runtime/migration-ledger.json`은 real run 시작을 첫 mutation 전에 기록하고, family별 `skipped`/`no_op`/`transformed`/`failed`/`blocked` 결과와 partial/complete phase를 저장한다. Dry-run은 ledger나 marker를 만들지 않는다.
3. Resume/backup: transformed family는 bounded backup을 run별 backup root에 만들고, complete verification 뒤 cleanup한다. Partial ledger가 있으면 `runtime migrate --resume`만 이어서 실행한다. Unknown newer schema, missing v0 path, precondition 불일치류는 writable runtime을 막는 inspect-only 상태로 남긴다.
4. CLI/admission: `runtime migrate --dry-run`, `runtime migrate --apply`, `runtime migrate --resume`가 명시적 사용자 surface다. `runtime start`/`run`/runtime config load/update/recover admission은 migration-required/partial/blocked plan을 만나면 자동 변환하지 않고 writable mutation을 차단한다.
5. Redaction/projection: CLI output은 plan/result digest와 opaque refs를 출력하고 v0 fixture의 raw secret/payload/path를 출력하지 않는다. Config/profile은 `DurableConfigCompatibility` adapter로 readable/incompatible만 결합하며 transform 목록에 들어가지 않는다.
6. Tests: `durable_migration::*` tests cover no-op, dry-run byte nonmutation, single/multi/all-family transforms, before/during/after interruption for every family, resume, unknown newer/missing path/config incompatible blockers, and post-migration durable replay/trace/artifact compatibility. `shacs-cli` tests cover parser/help bad input, dry-run/apply/admission block, and redacted inspect projection.
