# DynamicToolCall Backup

This feature was removed from the main runtime path because it did not have enough value for the current Zed ACP integration.

What it covered:
- typed `item/tool/call` request handling
- ACP popup and fallback response path
- live and replay rendering for `ThreadItem::DynamicToolCall`
- text and image content-item mapping

Why it was removed:
- no strong Zed-side client-native surface to justify the maintenance cost
- partial support was becoming dead weight
- the adapter has higher-value daily-use work to prioritize

What would justify bringing it back:
- a concrete Zed-native picker or editor-context workflow
- structured tool results that are more useful than text fallback
- a real user-facing flow that cannot be covered by approvals, MCP, or session controls
