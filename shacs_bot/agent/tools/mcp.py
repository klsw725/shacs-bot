"""MCP 클라이언트: MCP 서버에 연결하고, 해당 서버의 도구들을 shacs-bot 네이티브 도구로 감싸서(wrap) 제공합니다."""
import asyncio
from contextlib import AsyncExitStack
from typing import Any

import httpx
from loguru import logger
from mcp import ClientSession, StdioServerParameters, stdio_client
from mcp.client.streamable_http import streamable_http_client
from mcp.types import Tool as MCPTool

from shacs_bot.agent.tools.base import Tool
from shacs_bot.agent.tools.registry import ToolRegistry
from shacs_bot.config.schema import MCPServerConfig


class MCPToolWrapper(Tool):
    """MCP 서버의 단일 도구를 shacs-bot Tool로 감싸는 래퍼입니다."""

    def __init__(
            self,
            session: ClientSession,
            server_name: str,
            tool_def: MCPTool,
            tool_timeout: int = 30
    ):
        self._session = session
        self._original_name = tool_def.name
        self._name = f"mcp_{server_name}_{tool_def.name}"
        self._description = tool_def.description or tool_def.name
        self._parameters = tool_def.inputSchema or {"type": "object", "properties": {}}
        self._tool_timeout = tool_timeout

    @property
    def name(self) -> str:
        return self._name

    @property
    def description(self) -> str:
        return self._description

    @property
    def parameters(self) -> dict[str, Any]:
        return self._parameters

    async def execute(self, **kwargs: Any) -> str:
        from mcp import types
        try:
            result: types.CallToolResult = await asyncio.wait_for(
                fut=self._session.call_tool(name=self._original_name, arguments=kwargs),
                timeout=self._tool_timeout,
            )
        except asyncio.TimeoutError:
            logger.warning(f"MCP 도구 '{self._name}'가 {self._tool_timeout}초 후 타임아웃되었습니다.")
            return f"(MCP 도구 호출이 {self._tool_timeout}초 후 타임아웃되었습니다.)"

        parts = []

        for block in result.content:
            if isinstance(block, types.TextContent):
                parts.append(block.text)
            else:
                parts.append(str(block))

        return "\n".join(parts) or "(출력 없음)"

async def connect_mcp_servers(
        mcp_servers: dict[str, MCPServerConfig],
        registry: ToolRegistry,
        stack: AsyncExitStack
) -> None:
    """구성된 MCP 서버에 연결하고 해당 서버의 도구들을 등록합니다."""
    for name, cfg in mcp_servers.items():
        try:
            if cfg.command:
                params: StdioServerParameters = StdioServerParameters(
                    command=cfg.command,
                    args=cfg.args,
                    env=cfg.env or None
                )
                read, write = await stack.enter_async_context(stdio_client(params))
            elif cfg.url:
                # MCP HTTP 전송이 httpx의 기본 5초 타임아웃을 상속받아
                # 상위 레벨의 tool 타임아웃보다 먼저 종료되지 않도록,
                # 항상 명시적인 httpx 클라이언트를 제공한다.
                http_client = await stack.enter_async_context(
                    httpx.AsyncClient(
                        headers=cfg.headers or None,
                        follow_redirects=True,
                        timeout=None,
                    )
                )
                read, write, _ = await stack.enter_async_context(
                    streamable_http_client(url=cfg.url, http_client=http_client)
                )
            else:
                logger.warning("MCP 서버 '{}'에 대한 유효한 연결 정보가 없습니다 (command 또는 url 필요).", name)
                continue

            session: ClientSession = await stack.enter_async_context(ClientSession(read, write))
            await session.initialize()

            from mcp import types
            tools: types.ListToolsResult = await session.list_tools()
            for tool_def in tools.tools:
                wrapper = MCPToolWrapper(session, name, tool_def, tool_timeout=cfg.tool_timeout)
                registry.register(wrapper)
                logger.debug("MCP 서버 '{}'의 도구 '{}'이(가) 등록되었습니다.", name, wrapper.name)

            logger.info("MCP 서버 '{}'에 성공적으로 연결되고 도구가 등록되었습니다 ({} 도구).", name, len(tools.tools))
        except Exception as e:
            logger.error("MCP 서버 '{}'에 연결하는 동안 오류 발생: {}", name, str(e))