# PRD 000. 앱 흐름 사용·호환성 조사와 판정

Status: Planned (Decision only)

## Goal

개인용 앱의 authoring과 설치 후 lifecycle을 실제 사용자 경로·보존 자료·유지 비용으로 함께 평가해 **유지 / 범위 축소 / 제거** 중 하나의 근거 있는 결론을 기록한다. 이 PRD 하나로 [039 SPEC](../SPEC.md)의 판정을 닫으며, 결론을 구현하거나 사용자 자료를 삭제하지 않는다.

## Scope

1. 격리된 config/data dir에서 `apps init/propose/apply/install/list/inspect/enable/start/stop/restart/recover/disable/uninstall`을 단계별로 실행해 draft, bundle, registry, activation/supervisor receipt 변화를 분리해 기록한다.
2. 실제 CLI/API·readiness·Tasks·plugin/extension consumer와 테스트/문서, 사용자가 보유한 설치 앱/로컬 스크립트의 확인 범위·미확인 범위를 조사한다. 사용자 소유 실제 config/auth/data는 변경하지 않는다.
3. Authoring과 lifecycle 각각의 사용자 가치·설정/실패/복구 비용, skill/tool/workflow 대체 결과를 비교한다. 기존 데이터와 명령을 읽고 다룰 호환 계약을 유지/축소/제거 각 경우에 적는다.
4. 단일 결론을 근거·위험·보존 자료·대체 동작·후속 구현/데이터 이전 범위·필요 회귀 테스트와 함께 결정문에 기록한다. 불충분한 근거는 `미결정`으로 남긴다.

## Non Scope

- 앱 CLI/registry/supervisor 코드 제거, 실 사용자 자료 이전·삭제, 021/032 기존 완료 판정 변경.
- 실제 앱 호출이 검색되지 않았다는 이유만으로 미사용이라고 단정하거나 계획상 simplify를 retire 확정으로 격상하는 일.

## Parent Requirement Mapping

- [039 SPEC](../SPEC.md)의 조사할 현재 계약 1–4, 판정 규칙 전체, 변경 전 불변식 전체와 판정 수용 조건 1–3을 이 PRD 하나가 소유한다. 별도 뒤 번호 PRD를 필요로 하지 않는다.

## Acceptance Criteria

1. 격리 실행에서 초안 idempotency/conflict, apply와 install/start의 분리, disabled/missing credential 차단, stop/recover와 저장 receipt를 실제 표면으로 재현한다. 초기/종료 자료를 임의로 지우지 않는다.
2. Draft/bundle/registry/journal·readiness/Tasks 소비자의 경로·읽기·호환성 목록과 authoring/lifecycle별 가치·비용 비교를 제공한다. 저장소 밖 사용량을 알 수 없으면 `미확인`이라고 적는다.
3. 유지·축소·제거 중 선택한 하나의 결론에 대해 기존 사용자 동작과 보존 자료, 필요 호환/대체·회귀 테스트·후속 작업의 실행 경계를 명시한다. 근거가 부족하면 완료하지 않는다.

## Closure Evidence

- 격리된 CLI/API·readiness/Tasks 상태의 명령별 transcript 및 자료/receipt 변화표.
- 사용자·호출자 조사 범위, 대체 경로/비용 비교, 미확인 항목과 검토 가능한 판정 기록.

이 PRD는 **판정문 자체**가 완료물이다. 기능 변경이나 다른 판정 스펙의 완료에 의존하지 않는다.
