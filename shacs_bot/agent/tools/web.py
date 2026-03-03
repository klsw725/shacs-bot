"""웹 도구들: 웹 검색 그리고 웹 크롤링."""
import html
import json
import os
import re
from typing import Any
from urllib.parse import urlparse

import httpx
from httpx import Response
from loguru import logger
from readability import Document

from shacs_bot.agent.tools.base import Tool


# Shared constants
USER_AGENT = "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_7_2) AppleWebKit/537.36"
MAX_REDIRECTS = 5  # Limit redirects to prevent DoS attacks

def _strip_tags(text: str) -> str:
    """HTML 태그 제거 및 엔티티 디코딩."""
    text = re.sub(r'<script[\s\S]*?</script>', '', text, flags=re.I)
    text = re.sub(r'<style[\s\S]*?</style>', '', text, flags=re.I)
    text = re.sub(r'<[^>]+>', '', text)
    return html.unescape(text).strip()

def _normalize(text: str) -> str:
    """공백 정규화."""
    text = re.sub(r'[ \t]+', ' ', text)
    return re.sub(r'\n{3,}', '\n\n', text).strip()

def _validate_url(url: str) -> tuple[bool, str]:
    """URL 검증: http(s) 및 유효한 도메인만 허용."""
    try:
        p = urlparse(url)
        if p.scheme not in ('http', 'https'):
            return False, f"Only http/https allowed, got '{p.scheme or 'none'}'"
        if not p.netloc:
            return False, "Missing domain"
        return True, ""
    except Exception as e:
        return False, str(e)

def _to_markdown(html: str) -> str:
    """HTML을 마크다운으로 변환합니다."""
    # 태크 stripping 전에 links, headings, lists 변환
    text = re.sub(pattern=r'<a\s+[^>]*href=["\']([^"\']+)["\'][^>]*>([\s\S]*?)</a>',
                  repl=lambda m: f'[{_strip_tags(m[2])}]({m[1]})',
                  string=html,
                  flags=re.I)
    text = re.sub(pattern=r'<h([1-6])[^>]*>([\s\S]*?)</h\1>',
                  repl=lambda m: f'\n{"#" * int(m[1])} {_strip_tags(m[2])}\n',
                  string=text,
                  flags=re.I)
    text = re.sub(pattern=r'<li[^>]*>([\s\S]*?)</li>',
                  repl=lambda m: f'\n- {_strip_tags(m[1])}',
                  string=text,
                  flags=re.I)
    text = re.sub(pattern=r'</(p|div|section|article)>',
                  repl='\n\n',
                  string=text,
                  flags=re.I)
    text = re.sub(pattern=r'<(br|hr)\s*/?>',
                  repl='\n',
                  string=text,
                  flags=re.I)
    return _normalize(_strip_tags(text))


class WebSearchTool(Tool):
    """웹 검색 도구. Brave Search API를 사용하여 웹을 검색합니다."""

    name = "web_search"
    description = "웹을 검색합니다. 제목, URL, 스니펫을 반환합니다."
    parameters = {
        "type": "object",
        "properties": {
            "query": {"type": "string", "description": "검색 쿼리"},
            "count": {"type": "integer", "description": "결과 수 (1-10)", "minimum": 1, "maximum": 10}
        },
        "required": ["query"]
    }

    def __init__(self, api_key: str | None = None, max_results: int = 5, proxy: str | None = None):
        self._init_api_key: str = api_key
        self._max_results: int = max_results
        self._proxy = proxy

    @property
    def api_key(self) -> str:
        """환경 변수나 설정 변경 사항이 반영되도록, 호출 시점에 API 키를 결정(조회)한다."""
        return self._init_api_key or os.environ.get("BRAVE_API_KEY", "")

    async def execute(self, query: str, count: int | None = None, **kwargs: Any) -> str:
        if not self.api_key:
            return """ 
                에러: BRAVE_API_KEY가 설정되어 있지 않습니다.,
                ~/.shacs-bot/config.json 파일의 tools.web.search.apiKey에 설정하거나
                (또는 BRAVE_API_KEY 환경 변수를 export한 후) 게이트웨이를 다시 시작하세요.
            """

        try:
            n: int = min(max(count or self._max_results, 1), 10)
            logger.debug("WebSearch: {}", "proxy enabled" if self._proxy else "direct connection")
            async with httpx.AsyncClient(proxy=self._proxy) as client:
                r: Response = await client.get(
                    "https://api.search.brave.com/res/v1/web/search",
                    params={"q": query, "count": n},
                    headers={"Accept": "application/json", "X-Subscription-Token": self.api_key},
                    timeout=10.0,
                )
                r.raise_for_status()

            result: list[dict[str, Any]] = r.json().get("web", {}).get("results", [])
            if not result:
                return f"'{query}'에 대한 검색 결과가 없습니다."

            lines: list[str] = [f"검색 결과: '{query}' (최대 {n}개)"]
            for i, item in enumerate(result[:n], 1):
                lines.append(f"{i}. {item.get("title", "")}\n   {item.get("url", "")}")
                if desc := item.get("description"):
                    lines.append(f"   {desc}")
            return "\n".join(lines)
        except httpx.ProxyError as e:
            logger.error("WebSearch 프록시 에러: {}", e)
            return f"프록시 error: {e}"
        except Exception as e:
            logger.error("WebSearch 에러: {}", e)
            return f"에러: {e}"


class WebFetchTool(Tool):
    """Readability를 사용하여 URL에서 콘텐츠를 가져오고 추출하는 도구."""

    name = "web_fetch"
    description = "URL을 가져오고 읽을수 있는 내용을 추출합니다."
    parameters = {
        "type": "object",
        "properties": {
            "url": {
                "type": "string",
                "description": "가져올 웹 페이지의 URL"
            },
            "extractMode": {
                "type": "string",
                "enum": ["markdown", "text"],
                "default": "markdown",
            },
            "maxChars": {
                "type": "integer",
                "minimum": 100,
            }
        },
        "required": ["url"]
    }

    def __init__(self, max_chars: int = 50000, proxy: str | None = None):
        self._max_chars = max_chars
        self._proxy: str = proxy

    async def execute(self, url: str, extractMode: str = "markdown", maxChars: int | None = None, **kwargs: Any) -> str:
        # 가져오기 전에 URL 검증
        is_valid, error_msg = _validate_url(url)
        if not is_valid:
            return json.dumps({
                "error": f"URL 검증 실패: {error_msg}",
                "url": url
            })

        try:
            logger.debug("WebFetch: {}", "proxy enabled" if self._proxy else "direct connection")
            async with httpx.AsyncClient(
                follow_redirects=True,
                max_redirects=MAX_REDIRECTS,
                timeout=30.0,
                proxy=self._proxy
            ) as client:
                r: Response = await client.get(url, headers={"User-Agent": USER_AGENT})
                r.raise_for_status()

            ctype: str = r.headers.get("content-type", "")

            # JSON
            if "application/json" in ctype:
                text, extractor = json.dumps(r.json(), indent=2), "json"
            # HTML
            elif "text/html" in ctype or r.text[:256].lower().startswith(("<!doctype", "<html")):
                doc: Document = Document(r.text)
                content: str = _to_markdown(doc.summary()) if extractMode == "markdown" else _strip_tags(doc.summary())
                text: str = f"# {doc.title()}\n\n{content}" if doc.title() else content
                extractor: str = "readability"
            else:
                text, extractor = r.text, "raw"

            max_chars: int = maxChars or self._max_chars
            truncated: bool = len(text) > max_chars
            if truncated:
                text = text[:max_chars]

            return json.dumps({
                "url": url,
                "finalUrl": str(r.url),
                "status": r.status_code,
                "extractor": extractor,
                "truncated": truncated,
                "length": len(text),
                "text": text
            })

        except httpx.ProxyError as e:
            logger.error("WebFetch 프록시 에러 for {}: {}", url, e)
            return json.dumps({
                "error": f"Proxy error: {e}",
                "url": url
            }, ensure_ascii=False)
        except Exception as e:
            logger.error("WebFetch 에러 for {}: {}", url, e)
            return json.dumps({
                "error": str(e),
                "url": url
            }, ensure_ascii=False)