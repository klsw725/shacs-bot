"""Base class for agent tools."""
from abc import ABC, abstractmethod
from typing import Any


class Tool(ABC):
    """
    에이전트 도구에 대한 추상 기본 클래스입니다.

    도구는 에이전트가 환경과 상호 작용하는 데 사용할 수 있는 기능입니다.
    예: 파일 읽기, 명령 실행 등
    """

    _TYPE_MAP = {
        "string": str,
        "integer" : int,
        "number": (int, float),
        "boolean": bool,
        "array": list,
        "object": dict,
    }

    @property
    @abstractmethod
    def name(self) -> str:
        """함수 호출에 사용되는 도구 이름입니다."""
        pass

    @property
    @abstractmethod
    def description(self) -> str:
        """도구가 수행하는 작업에 대한 설명입니다."""
        pass

    @property
    @abstractmethod
    def parameters(self) -> dict[str, Any]:
        """도구 매개변수에 대한 JSON 스키마입니다."""
        pass

    @abstractmethod
    async def execute(self, **kwargs: Any) -> str:
        """
        주어진 매개변수로 도구를 실행합니다.

        인수:
            **kwargs: 도구별 매개변수입니다.

        반환값:
            도구 실행의 문자열 결과입니다.
        """
        pass

    def validate_params(self, param: dict[str, Any]) -> list[str]:
        """JSON 스키마에 대한 도구 매개변수를 검증합니다. 오류 목록을 반환합니다(유효한 경우 비어 있음)."""
        schema: dict[str, Any] = self.parameters or {}
        if schema.get("type", "object") != "object":
            raise ValueError(f"스키마는 객체 유형이어야 합니다. {schema.get('type')!r}이(가) 제공되었습니다.")

        return self._validate(param, {**schema, "type": "object"}, "")

    def _validate(self, val: Any, schema: dict[str, Any], path: str) -> list[str]:
        t, label = schema.get("type"), path or "parameter"
        if t in self._TYPE_MAP and not isinstance(val, self._TYPE_MAP[t]):
            return [f"{label} should be {t}"]

        errors = []
        if "enum" in schema and val not in schema["enum"]:
            errors.append(f"{label}은 {t} 중 하나여야 합니다.")

        if t in ("integer", "number"):
            if "minimum" in schema and val < schema["minimum"]:
                errors.append(f"{label}은(는) {schema['minimum']} 이상이어야 합니다.")

            if "maximum" in schema and val > schema["maximum"]:
                errors.append(f"{label}은(는) {schema['maximum']} 이하이어야 합니다.")

        elif t == "string":
            if "minLength" in schema and len(val) < schema["minLength"]:
                errors.append(f"{label}의 길이는 최소 {schema['minLength']}이어야 합니다.")

            if "maxLength" in schema and len(val) > schema["maxLength"]:
                errors.append(f"{label}의 길이는 최대 {schema['maxLength']}이어야 합니다.")

        elif t == "object":
            for key in schema.get("required", []):
                if key not in val:
                    errors.append(f"{path + '.' + key if path else key}가 없습니다.")

            props: Any | dict = schema.get("properties", {})
            for key, value in val.items():
                if key in props:
                    errors.extend(self._validate(value, props[key], path + "." + key if path else key))

        elif t == "array" and "items" in schema:
            for i, item in enumerate(val):
                errors.extend(
                    self._validate(item, schema["items"], f"{path}[{i}]" if path else f"[{i}]")
                )

        return errors

    def to_schema(self) -> dict[str, Any]:
        """OpenAI function 스키마 형식으로 도구를 직렬화합니다."""
        return {
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": self.parameters,
            }
        }

