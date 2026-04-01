# README Lifecycle Hooks 사용 예시 추가

## 사용자 프롬프트

```text
README에만 작성해줘 그러니 원래ㅜ적을 것 보다는 길게 적어
```

## 변경 내용

- `README.md`에 `Lifecycle Hooks` 섹션 추가
- 목차에 `Lifecycle Hooks` 항목 추가
- README 안에서 다음 내용을 바로 이해할 수 있도록 정리
  - hooks 목적과 기본 원칙
  - `hooks.enabled`, `hooks.redactPayloads`, `hooks.outboundMutationEnabled`
  - built-in example hook 자동 등록 동작
  - 관측 이벤트 목록
  - 로그 예시
  - outbound mutation이 기본적으로 opt-in이라는 점

## 메모

- 사용자 요청에 맞춰 README에만 사용 설명을 추가했고, spec/별도 가이드 문서는 수정하지 않았다.
