# PRD 000. 비디오 분석 실제 사용과 호환성 판정

Status: Planned (Decision only)

## Goal

비디오 analyzer의 **기본/명시 주입 여부, 저장 첨부의 실제 분석 결과, 이후 소비자와 비용**을 한 조사 흐름에서 확인해 유지·범위 축소·제거 중 근거 있는 결론을 기록한다. 이 PRD 하나가 [040 SPEC](../SPEC.md)의 독립 종료 단위이며 코드나 저장된 영상을 제거하지 않는다.

## Scope

1. 기본 runtime 구성과 `ContextBuilder::with_video_analyzer` 호출자를 나눠 확인한다. 실제 analyzer가 없는 경우 `analyzer: None`과 injected test success를 다른 증거로 남긴다.
2. 사용자 소유 격리 자료에서 저장된 비디오 첨부 → 제한된 context evidence → provider input/inspect/API/TUI/channel 중 실제 지원 표면을 추적한다. 정상, 미설정, 비지원, 크기/시간 초과, 취소·실패를 함께 기록한다.
3. 관측된 호출·실패·비용·민감정보 범위와 projection/lineage의 저장·읽기 소비자를 확인한다. 제거/축소하더라도 027 attachment, 이미지와 오디오 경로, 과거 media record의 읽기 의미가 어떻게 남는지 비교한다.
4. 유지/축소/제거의 사용자-visible 지원 범위·상태 vocabulary·대체 동작·호환 테스트를 비교하고 선택한 결론을 문서화한다. 생성자/config/호출자/문서/보존 record 조사에도 production 주입 경로가 없으면 그 범위의 부재를 증거로 삼되, 저장소 밖 소비는 `미확인` 위험으로 남긴다. 공개 주입 API와 과거 record의 보존/deprecation 계약 없이 축소·제거를 확정하지 않는다.

## Non Scope

- 새 비디오 analyzer/provider 구현, CDN/editor, 이미지 생성·audio 분석의 삭제 또는 이전.
- 완전한 영상 이해, 비밀정보 제거, 모든 입력 URL 수용, universal sandbox를 증명했다고 주장하는 일.

## Parent Requirement Mapping

- [040 SPEC](../SPEC.md)의 조사 계약 1–4, 세 판정 규칙, 변경 전 불변식 및 수용 조건 1–3을 한 PRD에서 모두 소유한다. 조사와 결론을 따로 나눠 뒤 PRD에 완료를 맡기지 않는다.

## Acceptance Criteria

1. 소스와 실행 구성을 대조해 기본/주입 analyzer availability와 실제 production 호출자·미확인 부분을 표시한다. 구성이 있다면 제어 비디오로 attachment→context→표면을 관찰한다. 없으면 unsupported의 실제 의미와 시험 주입 결과의 한계를 각각 남긴다.
2. 크기/시간 초과·비지원 codec·취소/timeout·result publication 실패를 안전한 status와 구분한다. API/TUI/channel 등의 소비가 없는 표면을 성공으로 만들지 않고 저장된 기록 및 audio/image 경계 영향을 확인한다.
3. 현재 사용자 가치·실패/저장·측정된 비용 또는 미측정 한계와 유지/축소/제거별 대체·호환 범위를 비교한 뒤 하나의 근거 있는 결론과 후속 작업·회귀 기준을 남긴다. 조사 범위 내 production 경로 부재도 유효한 결과이며 외부 소비자는 미확인 위험으로 표시한다. 조사 범위 또는 과거 record 처리 자체의 근거가 부족하면 미결정으로 두고 완료하지 않는다.

## Closure Evidence

- 격리된 stored video, 기본/주입 구성, 실제 표면과 안전한 projection/failure 기록.
- 데이터·호출자·비용/민감정보 범위와 선택지 비교표, 미확인 사항, 검토 가능한 결정문.

이 PRD는 **비디오 analyzer를 유지할지에 관한 판정**으로만 닫힌다. 다른 판정 스펙이나 실제 제거 구현을 필요로 하지 않는다.
