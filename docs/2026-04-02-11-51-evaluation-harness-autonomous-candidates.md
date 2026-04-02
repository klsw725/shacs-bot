# 작업 기록: evaluation harness autonomous candidate 비교 확장

## 사용자 프롬프트

> [SYSTEM DIRECTIVE: TODO CONTINUATION]

## 브랜치

- `feature/evaluation-harness-m1`

## 변경 사항

- `shacs_bot/evals/state.py` 수정
  - `autonomous_candidates` 필드 추가
- `shacs_bot/evals/autoloop.py` 수정
  - `run_auto_eval()`가 state의 `autonomous_candidates`를 읽어 trigger/scheduled 경로에서도 candidate 비교를 수행하도록 확장
  - `_run_candidates()` helper 추가
  - autonomous candidate 결과를 `candidateBest`, `recommendedProviderName`, `recommendedModel`에 반영
- `shacs_bot/cli/commands.py` 수정
  - `create_eval_runtime()`를 공용 helper로 승격
  - `eval policy --candidate provider:model` 지원 추가
  - `eval status`에 `Autonomous Candidates` 표시 추가
  - 수동 `eval auto-run --candidate ...`도 state의 autonomous candidate 설정을 갱신하도록 정리

## 검증

- `lsp_diagnostics`
  - `shacs_bot/evals/autoloop.py` clean
  - `shacs_bot/evals/state.py` clean
- `uv run python -c ...` 스모크 테스트
  - `eval policy --candidate`가 state에 저장되는지 확인
  - autonomous `run_auto_eval()`가 state의 candidate 집합을 읽어 best candidate를 선택하는지 확인
  - `candidateBest == 'openrouter:model-b'` 형태로 state recommendation이 갱신되는지 확인

## 비고

- 이번 단계로 수동 candidate 비교뿐 아니라 turn-trigger / scheduled self-eval도 같은 후보 집합을 비교할 수 있게 됐다.
- candidate runtime 생성은 여전히 현재 config를 기반으로 하며, explicit provider/model override만 바꿔서 최소 침습적으로 동작한다.
