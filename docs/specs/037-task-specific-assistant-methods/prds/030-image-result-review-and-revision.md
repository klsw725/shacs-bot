# PRD 030. 생성 이미지 확인과 사용자 수정

Status: Planned

Depends on: [PRD 000](000-method-selection-and-user-override.md) 완료. 뒤 번호 PRD에는 의존하지 않는다.

## Goal

한 이미지 요청의 실제 산출물을 사용자에게 보여주고 요구 조건 확인·수정 요청을 같은 작업으로 이어간다. **Production artifact 저장 경계를 통과한 이미지 생성 성공과 미설정/미지원 실패를 모두 검증해야 종료한다.** 보고서나 계산 방식은 필요하지 않다.

## Scope

1. 선택된 이미지 수행 방식이 [034](../../034-generated-media-and-rich-file-context-expansion/SPEC.md)의 provider capability·credential status·artifact store를 확인하고 실제 생성 결과 또는 unsupported/unavailable/failed를 반환한다.
2. 사용자가 요구한 크기/형식/내용 등 확인 가능한 조건과 생성 artifact의 출처·상태를 함께 보여준다. 확인 가능한 검사는 증거로 남기되 미관측 미감·의미 품질을 자동 PASS라고 주장하지 않는다.
3. 사용자가 결과를 보고 변경을 요청하면 기존 목표·원본 artifact와 새 요청을 결속해 수정 결과를 구분한다. Provider가 지원하지 않는 edit/mask/variation은 다른 동작으로 성공을 합성하지 않는다.

## Non Scope

- Image editor UI, provider 간 edit/streaming 완전 동등성, 보고서·계산·복합 요청.
- 새로운 media root, raw provider payload 영구 저장, 외부 CDN/gallery, 전면 sandbox·privacy 보장.

## Parent Requirement Mapping

- [037 SPEC](../SPEC.md)의 소유 범위 3 중 이미지 결과 검토/수정, 범위 4의 이미지 artifact·상세 조회를 소유한다.
- 부모 수용 조건 3의 실제 artifact·사용자 수정·미지원 상태와 조건 5의 이미지 경로를 소유한다.

## Acceptance Criteria

1. 구성된 provider 또는 production 경계에 주입한 결정론적 호환 provider의 제어 입력에서 실제 도구 호출과 artifact commit을 거쳐 이미지 산출물을 만들고, 안전한 참조와 요구 조건 확인 상태를 사용자가 볼 수 있다. 테스트용 빈 응답이나 mock 화면만으로 대체하지 않으며 검사가 불가능한 조건은 미확인으로 남는다.
2. 사용자 수정 요청은 원래 목표와 이전 artifact를 유지하면서 새 artifact/실패 사실을 남긴다. 미지원 edit 계열은 정확한 unsupported로 표시된다.
3. 같은 production 호출·artifact 경계에서 provider 미설정, credential unavailable, 생성 실패, artifact commit 불확실 상태도 검사하고 성공 이미지를 합성하지 않는다. 실제 사용자 표면 또는 호환 provider가 주입된 실행 표면에서 성공과 거절을 각각 확인한다. 외부 계정의 image entitlement는 테스트의 선행 조건이 아니다.

## Closure Evidence

- 생성·검토·수정 요청과 실제 artifact lineage의 사용자-visible 기록.
- 미설정/미지원/실패·미확인 품질의 거절 테스트와 034 owner fact 소비 근거.

PRD 030은 **한 이미지 결과의 검토·수정 흐름**으로 닫힌다. 뒤 번호 PRD의 복합 결과나 별도 editor가 필요하지 않다.
