# 작업 기록: Evaluation Harness M1 저장소 추가

## 사용자 프롬프트

> 브랜치 새로 파고 진행해

## 브랜치

- `feature/evaluation-harness-m1`

## 변경 사항

- `shacs_bot/evals/models.py` 신규 추가
  - 평가 케이스, variant, trace, result, summary, manifest용 Pydantic 모델 정의
  - JSON 저장 시 camelCase alias를 쓰도록 설정
- `shacs_bot/evals/storage.py` 신규 추가
  - run 디렉터리 생성
  - `manifest.json`, `summary.json`, variant별 `*.result.json`, `*.trace.json` 저장
  - tmp 파일 후 replace 방식의 atomic write 적용
- `shacs_bot/evals/__init__.py` 신규 추가
  - eval 패키지 export 정리

## 검증

- `lsp_diagnostics`:
  - `shacs_bot/evals/models.py` clean
  - `shacs_bot/evals/storage.py` clean
  - `shacs_bot/evals/__init__.py` clean
- `uv run python -c ...` smoke test:
  - `EvaluationCase` camelCase alias 확인
  - manifest/result/trace/summary 파일 생성 확인
  - variant 디렉터리 구조 확인

## 비고

- 이번 단계는 Evaluation Harness PRD의 Step 1 범위만 반영했다.
- runner, observer hook, CLI는 아직 추가하지 않았다.
