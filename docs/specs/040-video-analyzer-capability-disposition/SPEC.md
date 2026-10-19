# 040. 비디오 분석 기능의 유지·축소·제거 판정 명세

Status: Planned (Decision only)

Consumes: 027 stored attachment intake, 030 trusted execution, 031 snapshot provenance, 034 video analyzer/domain evidence

## 목적과 종료 조건

이 스펙은 **저장된 비디오 첨부에서 bounded context와 진단 사실을 만드는 선택적 analyzer**가 개인 어시스턴트의 실제 사용자 경험에 필요한지 평가한다. [034](../034-generated-media-and-rich-file-context-expansion/SPEC.md)의 구현·완료 범위를 다시 열거나 이 스펙만으로 analyzer를 제거하지 않는다. 이미지 생성, 일반 첨부 저장, 오디오 분석과 별개의 판단이다.

완료 산출물은 현재 주입 가능한 analyzer의 사용자 진입점·실제 배포 구성·입력/결과/비용·projection 소비를 확인하고 **유지 / 범위 축소 / 제거** 가운데 근거 있는 하나를 선택한 결정문이다. 생성자·설정·production 호출자·사용자 표면·보존 record를 정해진 범위에서 조사해도 주입 경로가 없다면 **조사 범위 내 production 경로 부재**를 유효한 관측으로 기록한다. 외부 사용 미확인은 위험으로 남기되 이것만으로 판정을 영구 차단하지 않는다. 조사 범위 자체나 이전 record 처리를 확인하지 못했을 때는 `미결정`이며, 다른 신규 기능이나 판정 스펙 없이 독립 판정 가능해야 한다.

## 조사할 현재 계약

1. **사용자 입력과 활성 조건:** local/channel/API에서 저장한 비디오 첨부가 `ContextBuilder`로 전달되는 경로와 기본 analyzer 미설정 상태를 구분한다 (`crates/shacs-core/src/runtime/context/video.rs:20`). 테스트에서 `with_video_analyzer`로 주입 가능하다는 것과 기본 배포에서 실제 analyzer가 제공된다는 것은 다르다. 구성·생성자·사용자 설정을 확인하고, 외부 URL은 임의 비디오 intake라고 가정하지 않는다.
2. **분석 결과와 제한:** `video_route`의 크기·재생 시간 제한, staging, cancellation/deadline, owner/snapshot refs, video에서 추출된 audio의 재사용 경계를 추적한다 (`crates/shacs-core/src/runtime/file_context/spec034_context/video_route.rs`, `crates/shacs-core/src/runtime/video_analyzer_runtime/invocation.rs`). 비지원 codec, analyzer missing, timeout, truncated와 extraction failure가 성공으로 합쳐지지 않는지 확인한다.
3. **영속 기록과 소비자:** provider context의 bounded metadata/transcript/scene/keyframe, canonical media projection, inspect/API/TUI/channel 소비자가 각각 무엇을 보여주는지 확인한다 (`docs/USAGE.md`의 media 섹션). Projection 미설정 시 unavailable/404, 관찰 상태와 remote ACK의 구분을 유지한다. 기존 data dir에서 관련 record가 남는지와 삭제·축소 시 그 읽기 의미가 어떻게 달라지는지 기록한다.
4. **사용 가치와 비용:** 사용자 소유 격리 자료에서 실제 stored video의 결과를 기본 경로와 analyzer 주입 경로로 비교한다. 분석 시간·실패·저장량·민감정보 노출과 이후 어시스턴트 답에 미친 영향을 관찰한다. 시험용 fake analyzer 성공을 실제 provider/video 이해의 가치로 승격하지 않는다.

## 판정 규칙

| 결론 | 필요한 증거와 후속 계약 |
|---|---|
| 유지 | 지원 가능한 실제 주입/사용 경로 또는 구체적 수요가 확인되고, 제한된 결과가 사용자 답에 유용하며 현재 실패 상태·출처·data disclosure를 설명할 수 있다. 유지할 기본/선택 구성과 회귀 경계를 적는다. |
| 범위 축소 | 일부 비디오 입력 또는 bounded evidence만 가치가 확인될 때 남길 입력·분석·projection을 한정하고, 제거할 상태/경로에 대한 사용자-visible `unsupported`/`unavailable` 동작, 공개 주입 API의 deprecation 및 이전 record 읽기를 정한다. |
| 제거 | 현재 production 주입·사용자 요청·기존 projection 소비·저장 자료를 조사하고, 첨부가 이후 어떻게 취급되는지와 이미지/오디오 경로를 보존하는 대체 결과, 공개 주입 API와 과거 record의 보존/deprecation 계약을 명시한다. 단순히 기본 analyzer가 `None`이라는 이유만으로 제거하지 않는다. |

## 변경 전 불변식

- 실제 사용자 config/auth/data dir이나 임의 외부 비디오 URL을 검사 대상으로 사용하지 않는다. 재현 가능한 소유 자료와 분리된 설정으로 기본/주입 두 경로를 평가한다.
- Bounded 추출을 완전한 영상 이해, 개인정보 제거 또는 universal sandbox로 표현하지 않는다. Analyzer는 [030](../030-trusted-agent-runtime-and-operational-controls/SPEC.md)의 신뢰·실행 경계를 소비한다.
- 027의 첨부 intake와 034의 이미지 생성·media artifact를 비디오 분석 제거 결론과 함께 자동 삭제하지 않는다. 036의 선제 관찰이나 037의 이미지 하네스 완료를 이 판정의 선행 조건으로 삼지 않는다.
- 오래된 media projection의 성공 여부를 새 source 검증 없이 현재 지원 가능 상태로 승격하지 않는다.

## 판정 수용 조건

1. 실제 기본 runtime에 analyzer가 설정되는지 생성자/설정 경로를 확인하고, 구성된 실행이 있으면 stored attachment → context evidence → 사용자 표면을 실행한다. 없다면 그 부재와 시험 주입 경로의 한계를 각각 기록한다.
2. 정상·누락·비지원·크기/시간 초과·중단 입력에 대해 provider context와 inspect/API/TUI/channel 중 실제 지원 표면의 상태·근거·disclosure를 비교한다. 본인 소유 자료만 사용하고 독립 사용자 수신을 증명하지 않은 delivery hint를 성공으로 처리하지 않는다.
3. 기존 record·호출자·테스트·문서와 이미지/오디오 경계의 영향표, 확인한 production 사용 또는 범위를 밝힌 경로 부재, 측정 가능한 비용과 미측정 한계, 세 선택지의 보존/대체 계약을 기록한다. 외부 사용 여부를 `없음`으로 만들지 않고 위험으로 표기한 뒤 결론 하나를 선택할 수 있다. 조사 범위나 과거 record 처리의 근거가 모자라면 미결정 이유를 남기고 완료하지 않는다.

이 스펙의 완료는 사용 가치에 대한 **독립 판정**이다. 선택한 축소·제거의 구현이나 기존 034 산출물의 소급 변경은 별도 검증 전까지 일어나지 않는다.

## 실행 PRD

| PRD | 유일하게 소유하는 결과 | 선행 PRD |
|---|---|---|
| [000 비디오 사용·호환성 판정](prds/000-video-runtime-value-and-compatibility-decision.md) | 조사 계약 1–4, 판정 수용 조건 1–3의 기본/주입 실행·영향 근거와 유지/축소/제거 결정문 | 없음 |

PRD 000은 기본 미설정만으로 제거를 정하지 않고 실제 표면과 보존 자료를 조사해 한 결론으로 닫는다. 040의 `Complete (Decision)`은 analyzer 변경 완료를 뜻하지 않는다.
