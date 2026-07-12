# PRD 002. Codex Responses and provider expansion

## 목표

이 문서는 OpenAI Images API baseline 이후, Codex Responses `image_generation` tool과 다른 image generation provider를 같은 capability 계약에 붙이기 위한 실행 기준이다. 목표는 1차 구현을 흔들지 않고 provider 확장, image edit, streaming partial image 같은 고급 기능을 안전하게 추가할 수 있게 하는 것이다.

## SPEC 입력

- 주관 spec: `docs/specs/019-image-generation-and-generated-media/SPEC.md`
- 선행 PRD:
  - `docs/specs/019-image-generation-and-generated-media/prds/000-provider-capability-and-openai-baseline.md`
  - `docs/specs/019-image-generation-and-generated-media/prds/001-image-generate-tool-and-media-artifact.md`
- 교차 의존:
  - `docs/specs/003-provider-runtime/SPEC.md`
  - `docs/specs/004-tool-runtime/SPEC.md`
  - `docs/specs/014-observability-diagnostics-and-inspection/SPEC.md`
  - `docs/specs/016-verification-matrix-and-release-gates/SPEC.md`

## Dependency Cut

- PRD 000은 provider-neutral trait과 OpenAI baseline을 제공한다.
- PRD 001은 tool과 generated media artifact 저장 경계를 제공한다.
- Codex provider의 현재 `/codex/responses` integration은 text, reasoning, function call 중심이다. image generation event parsing은 이 PRD에서 별도 확장으로 다룬다.
- 016은 provider expansion이 release evidence와 regression tests를 가져야 함을 제공한다.

## 범위

- Codex Responses `image_generation` tool support 분석과 구현 기준
- Responses API image generation output event parsing
- provider capability descriptor 확장
- OpenAI-compatible Images API provider 추가 가능성
- image edit, input image, mask, streaming partial image의 future contract
- provider-specific option을 stable tool schema에 반영하지 않는 기준

## 범위 제외

- 모든 provider 구현 완료
- Codex CLI `$imagegen` UX 복제
- third-party marketplace provider install
- remote image hosting, CDN, shared gallery
- provider billing dashboard
- UI image editor

## 구현 요구사항

- Codex image generation은 기존 chat stream parser에 몰래 섞지 말고, image generation capability path에서 명시적으로 처리해야 한다.
- Responses API `image_generation` tool을 사용할 경우 request에는 input text, optional input image, image_generation tool descriptor가 포함되어야 한다.
- stream event에서 generated image payload, partial image, final image reference를 구분해야 한다.
- partial image는 1차적으로 diagnostics progress 또는 ignored preview로 취급하고, final artifact가 없으면 success로 닫지 않는다.
- provider별 지원 action은 `generate`, `edit`, `auto`처럼 capability descriptor에 표현해야 한다.
- stable `image_generate` tool schema는 provider별 raw option으로 계속 커지면 안 된다. 필요한 경우 advanced options는 allow-list와 diagnostics marker를 가져야 한다.
- URL output provider는 만료 URL, remote URL, downloadable artifact를 구분해야 한다. long-lived result에는 local persisted artifact가 필요하다.
- provider expansion은 fixture와 golden response parser tests 없이 registry flag만 추가하면 안 된다.

## 데이터/상태 모델

- `ImageGenerationAction`: generate, edit, auto.
- `ImageGenerationProviderDescriptor`: provider id, backend, supported actions, output modes, streaming support, edit support.
- `ResponsesImageGenerationEvent`: started, partial, final, failed.
- `RemoteImageOutput`: url, expires at, download required, mime type hint.
- `ProviderImageOptionPolicy`: allowed options, default values, unsupported option behavior.

## 정상 시퀀스

1. provider descriptor가 Codex 또는 다른 provider의 image generation support를 표시한다.
2. resolver가 provider-specific image generation client를 선택한다.
3. Codex client가 Responses request with `image_generation` tool을 만든다.
4. stream parser가 image generation final output을 수집한다.
5. capability result가 PRD 001의 media artifact writer로 전달된다.
6. diagnostics가 provider route, action, output mode, artifact ref를 기록한다.

## 실패 시퀀스

1. provider가 image generation event 대신 text만 반환하면 image generation success로 처리하지 않는다.
2. partial image만 받고 final image가 없으면 incomplete provider output으로 실패한다.
3. provider URL output이 만료됐거나 다운로드가 실패하면 artifact write failure로 닫는다.
4. unsupported action 또는 option은 provider 호출 전에 rejected option error로 반환한다.
5. stream parser가 알 수 없는 image event를 만나면 raw payload를 session에 넣지 않고 diagnostics summary만 남긴다.

## 검증 관점

- Codex Responses image generation request builder가 expected tool descriptor를 포함하는지 확인한다.
- image generation stream fixture에서 final image만 artifact result로 승격되는지 확인한다.
- partial-only stream이 success가 아닌 incomplete failure로 닫히는지 확인한다.
- URL output provider의 download/persist 정책이 만료 URL을 long-lived result로 표시하지 않는지 확인한다.
- unsupported action과 unsupported provider가 서로 다른 error로 보이는지 확인한다.

## 완료 기준

- Codex 또는 두 번째 provider가 `ImageGenerationClient` 계약으로 붙는다.
- stream 또는 URL output provider의 결과가 local generated media artifact로 저장된다.
- provider expansion regression fixture가 release gate에 연결된다.
- `image_generate` tool schema는 provider-specific raw API에 종속되지 않고 유지된다.

## 현재 구현 상태

2026-07-06 기준 OpenRouter image generation provider slice는 구현됐다. `OpenRouterImageGenerationClient`는 OpenRouter chat completions image contract를 사용하고, parser는 data URL image output을 decoded image result로 변환하며 remote URL output은 persisted artifact로 과장하지 않고 malformed provider output으로 거절한다. Resolver는 configured OpenRouter model and default image model mapping을 제공한다.

아직 열린 범위는 Codex Responses `image_generation` event parsing, image edit/input/mask/variation, streaming partial image handling, remote URL output provider download/persist policy, provider expansion release fixture completion이다.
