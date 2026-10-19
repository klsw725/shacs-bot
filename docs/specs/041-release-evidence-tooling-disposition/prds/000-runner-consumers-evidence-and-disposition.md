# PRD 000. Release runner 소비자·증거·대체 경로 판정

Status: Planned (Decision only)

## Goal

`spec031-release-runner`의 CLI·공개 API·source-bound artifact 계약을 누가 쓰며 어떤 검증을 제공하는지 확인하고 **유지 / 범위 축소 / 제거** 중 근거 있는 결론을 기록한다. 이 PRD 하나로 [041 SPEC](../SPEC.md)의 판정을 독립 완료하며 실제 코드 제거는 수행하지 않는다.

## Scope

1. CLI `--help`, success fixture, 실패 입력, 격리 evidence root의 current-worktree 결과와 stdout/stderr/exit 의미를 각각 관찰한다. 공개 Rust 함수, 테스트, README/USAGE, CI 및 사용자 소유 스크립트 소비를 확인하고 미확인 외부 사용을 0으로 단정하지 않는다.
2. 현행 v3 coverage/command/external owner/source identity와 보존 v2 artifact 판독을 분리한다. 정확한 requirement→명령→transcript→result 연결, cleanup/triage/dirty 관측 및 archive hash/변조 거부를 현재 동작으로 기록한다.
3. 같은 누락·명령 실패·stale source·위조 PASS·변조 입력으로 기존 runner와 제안된 대체 gate를 각각 검사해 결과/재현성 동등성·실행 비용을 비교한다. 대체가 없으면 미충족이라고 남긴다.
4. 선택한 결론의 CLI/API·artifact 보존, 033/034 독립 runner 비영향, 문서/검증 변경 범위와 후속 구현 순서를 결정문에 기록한다.

## Non Scope

- Spec031 snapshot/replay/config migration이나 다른 release runner 삭제, 과거 evidence를 현행 PASS로 재결속.
- 검증을 통과시키려고 실패 테스트를 지우거나 기존 누락/변조 거절을 완화하는 코드 변경.
- 판정과 동시에 바이너리/API를 제거하거나 기존 사용자 evidence를 덮어쓰는 일.

## Parent Requirement Mapping

- [041 SPEC](../SPEC.md)의 조사할 계약 1–4, 유지/축소/제거 판정 규칙, 변경 전 불변식과 수용 조건 1–3을 이 PRD 하나가 소유한다.

## Acceptance Criteria

1. 바이너리/공개 API/테스트·문서·CI/로컬 스크립트와 artifact 소비의 조사 범위를 적고, 격리된 root에서 help·fixture·실패·current-worktree의 차이를 실제로 확인한다.
2. 실행 시간/비용과 필수 source·owner·coverage·artifact/cleanup 검증표를 만들고, 축소·제거 선택지라면 동일한 실패 입력을 넣어 대체 gate가 fail-closed로 동등한지 확인한다. `cargo test` green만으로 동등하다고 판정하지 않는다.
3. 현행 v3/보존 v2, 035 역사 자료와 033/034 별도 runner의 영향 및 검증 가능한 판독 경로를 적는다. 유지/축소/제거 가운데 하나의 이유·보존 계약·구현/회귀 범위를 결정문에 남긴다. 근거가 부족하면 미결정으로 남긴다.

## Closure Evidence

- CLI/API 호출자 목록, 독립 실행 기록과 측정된 비용·필수 gate 대응표.
- 기존/대체 경로의 누락·실패·변조 테스트 결과, 과거 artifact 호환 영향표와 검토 가능한 결정문.

이 PRD는 **release 도구의 판정**만으로 완료한다. 다른 판정 스펙이나 선택한 구현의 실행을 기다리지 않는다.
