# Thread Feature Map (Curated)

Paste this file into https://markmap.js.org/repl

## Legend
- `Depends on` means outgoing edge (`A --> B`).
- `Used by` means incoming edge.

## AppServer - src/app_server.rs
- Depends on
  - AppServerPolicy - src/app_server/request_policy.rs
  - AppServerReader - src/app_server/reader.rs
  - NotificationDispatch - src/thread/notification/dispatch.rs
- Used by
  - TurnExecution - src/thread/turn/execution.rs

## AppServerPolicy - src/app_server/request_policy.rs
- Depends on: none
- Used by
  - AppServer - src/app_server.rs

## AppServerReader - src/app_server/reader.rs
- Depends on: none
- Used by
  - AppServer - src/app_server.rs

## ApprovalsCommand - src/thread/features/approvals/command.rs
- Depends on: none
- Used by
  - ServerRequests - src/thread/core/server_requests.rs

## ApprovalsFile - src/thread/features/approvals/file_change.rs
- Depends on: none
- Used by
  - ServerRequests - src/thread/core/server_requests.rs

## ApprovalsPermissions - src/thread/features/approvals/permissions.rs
- Depends on: none
- Used by
  - ServerRequests - src/thread/core/server_requests.rs

## ApprovalsUserInput - src/thread/features/approvals/user_input.rs
- Depends on: none
- Used by
  - ServerRequests - src/thread/core/server_requests.rs

## CodexAgent - src/codex_agent.rs
- Depends on
  - SessionLifecycle - src/thread/session/lifecycle.rs
  - SessionView - src/thread/session/view.rs
- Used by: none

## CollabContent - CollabContent
- Depends on: none
- Used by
  - CollabRender - src/thread/features/collab/render.rs

## CollabRender - src/thread/features/collab/render.rs
- Depends on
  - CollabContent - CollabContent
  - CollabStatus - CollabStatus
- Used by
  - ItemHandlers - src/thread/core/item_handlers.rs
  - Replay - src/thread/core/replay.rs

## CollabReplay - src/thread/features/collab/render.rs
- Depends on: none
- Used by
  - Replay - src/thread/core/replay.rs

## CollabStatus - CollabStatus
- Depends on: none
- Used by
  - CollabRender - src/thread/features/collab/render.rs

## CommandReplay - src/thread/features/tool_events/command.rs
- Depends on: none
- Used by
  - Replay - src/thread/core/replay.rs

## FileChanges - src/thread/features/file/changes.rs
- Depends on: none
- Used by
  - FileEvents - src/thread/features/file/events.rs

## FileEvents - src/thread/features/file/events.rs
- Depends on
  - FileChanges - src/thread/features/file/changes.rs
  - StatusMapping - src/thread/features/status_mapping.rs
- Used by
  - ItemHandlers - src/thread/core/item_handlers.rs

## FileReplay - src/thread/features/file/events.rs
- Depends on: none
- Used by
  - Replay - src/thread/core/replay.rs

## ItemHandlers - src/thread/core/item_handlers.rs
- Depends on
  - CollabRender - src/thread/features/collab/render.rs
  - FileEvents - src/thread/features/file/events.rs
  - PlanEvents - src/thread/features/plan/events.rs
  - SessionEvents - src/thread/features/session/events.rs
  - ToolEventsCommand - src/thread/features/tool_events/command.rs
  - ToolEventsMcp - src/thread/features/tool_events/mcp.rs
  - ToolEventsWebImage - src/thread/features/tool_events/web_image.rs
- Used by
  - NotificationFeature - src/thread/features/notification/mod.rs

## McpReplay - src/thread/features/tool_events/mcp.rs
- Depends on: none
- Used by
  - Replay - src/thread/core/replay.rs

## NotificationDeltas - src/thread/features/notification/events/deltas.rs
- Depends on: none
- Used by
  - NotificationFeature - src/thread/features/notification/mod.rs

## NotificationDispatch - src/thread/notification/dispatch.rs
- Depends on
  - NotificationFeature - src/thread/features/notification/mod.rs (edge: JSONRPCNotification)
  - ServerRequests - src/thread/core/server_requests.rs (edge: JSONRPCRequest)
- Used by
  - AppServer - src/app_server.rs

## NotificationFeature - src/thread/features/notification/mod.rs
- Depends on
  - ItemHandlers - src/thread/core/item_handlers.rs
  - NotificationDeltas - src/thread/features/notification/events/deltas.rs
  - NotificationTurn - src/thread/features/notification/events/turn.rs
  - NotificationUsage - src/thread/features/notification/events/usage.rs
  - NotificationWarnings - src/thread/features/notification/events/warnings.rs
  - TerminalUpdates - src/thread/core/terminal_updates.rs
  - TurnDiff - src/thread/turn/diff.rs
- Used by
  - NotificationDispatch - src/thread/notification/dispatch.rs (edge: JSONRPCNotification)

## NotificationTurn - src/thread/features/notification/events/turn.rs
- Depends on: none
- Used by
  - NotificationFeature - src/thread/features/notification/mod.rs

## NotificationUsage - src/thread/features/notification/events/usage.rs
- Depends on: none
- Used by
  - NotificationFeature - src/thread/features/notification/mod.rs

## NotificationWarnings - src/thread/features/notification/events/warnings.rs
- Depends on: none
- Used by
  - NotificationFeature - src/thread/features/notification/mod.rs

## PlanEvents - src/thread/features/plan/events.rs
- Depends on: none
- Used by
  - ItemHandlers - src/thread/core/item_handlers.rs

## PlanFallback - src/thread/features/plan/fallback.rs
- Depends on: none
- Used by
  - TurnExecution - src/thread/turn/execution.rs

## PlanParse - src/thread/features/plan/parse.rs
- Depends on: none
- Used by
  - TurnExecution - src/thread/turn/execution.rs

## PromptCommands - src/thread/prompt/commands.rs
- Depends on: none
- Used by
  - PromptFlow - src/thread/prompt/flow.rs

## PromptFlow - src/thread/prompt/flow.rs
- Depends on
  - PromptCommands - src/thread/prompt/commands.rs
  - SessionControls - src/thread/features/session/controls.rs
  - SessionModes - src/thread/features/session/modes.rs
  - TurnExecution - src/thread/turn/execution.rs
- Used by
  - UserClient - User/Zed

## ReconnectGuard - src/thread/turn/execution.rs
- Depends on: none
- Used by
  - TurnExecution - src/thread/turn/execution.rs

## Replay - src/thread/core/replay.rs
- Depends on
  - CollabRender - src/thread/features/collab/render.rs
  - CollabReplay - src/thread/features/collab/render.rs
  - CommandReplay - src/thread/features/tool_events/command.rs
  - FileReplay - src/thread/features/file/events.rs
  - McpReplay - src/thread/features/tool_events/mcp.rs
  - SessionReplay - src/thread/features/session/events.rs
  - WebImageReplay - src/thread/features/tool_events/web_image.rs
- Used by
  - SessionView - src/thread/session/view.rs

## ResumeCommon - src/thread/features/resume/common.rs
- Depends on: none
- Used by
  - SessionLifecycle - src/thread/session/lifecycle.rs

## ServerRequests - src/thread/core/server_requests.rs
- Depends on
  - ApprovalsCommand - src/thread/features/approvals/command.rs
  - ApprovalsFile - src/thread/features/approvals/file_change.rs
  - ApprovalsPermissions - src/thread/features/approvals/permissions.rs
  - ApprovalsUserInput - src/thread/features/approvals/user_input.rs
- Used by
  - NotificationDispatch - src/thread/notification/dispatch.rs (edge: JSONRPCRequest)

## SessionControls - src/thread/features/session/controls.rs
- Depends on: none
- Used by
  - PromptFlow - src/thread/prompt/flow.rs

## SessionEvents - src/thread/features/session/events.rs
- Depends on: none
- Used by
  - ItemHandlers - src/thread/core/item_handlers.rs

## SessionLifecycle - src/thread/session/lifecycle.rs
- Depends on
  - ResumeCommon - src/thread/features/resume/common.rs
- Used by
  - CodexAgent - src/codex_agent.rs

## SessionModes - src/thread/features/session/modes.rs
- Depends on: none
- Used by
  - PromptFlow - src/thread/prompt/flow.rs

## SessionReplay - src/thread/features/session/events.rs
- Depends on: none
- Used by
  - Replay - src/thread/core/replay.rs

## SessionView - src/thread/session/view.rs
- Depends on
  - Replay - src/thread/core/replay.rs
- Used by
  - CodexAgent - src/codex_agent.rs

## StatusMapping - src/thread/features/status_mapping.rs
- Depends on: none
- Used by
  - FileEvents - src/thread/features/file/events.rs
  - ToolEventsCommand - src/thread/features/tool_events/command.rs
  - ToolEventsMcp - src/thread/features/tool_events/mcp.rs

## TerminalUpdates - src/thread/core/terminal_updates.rs
- Depends on: none
- Used by
  - NotificationFeature - src/thread/features/notification/mod.rs

## ToolEventsCommand - src/thread/features/tool_events/command.rs
- Depends on
  - StatusMapping - src/thread/features/status_mapping.rs
  - ToolUiKind - src/thread/features/tool_call_ui/kind.rs
  - ToolUiLocation - src/thread/features/tool_call_ui/location.rs
  - ToolUiRaw - src/thread/features/tool_call_ui/raw.rs
  - ToolUiTitle - src/thread/features/tool_call_ui/title.rs
- Used by
  - ItemHandlers - src/thread/core/item_handlers.rs

## ToolEventsMcp - src/thread/features/tool_events/mcp.rs
- Depends on
  - StatusMapping - src/thread/features/status_mapping.rs
- Used by
  - ItemHandlers - src/thread/core/item_handlers.rs

## ToolEventsWebImage - src/thread/features/tool_events/web_image.rs
- Depends on: none
- Used by
  - ItemHandlers - src/thread/core/item_handlers.rs

## ToolUiKind - src/thread/features/tool_call_ui/kind.rs
- Depends on: none
- Used by
  - ToolEventsCommand - src/thread/features/tool_events/command.rs

## ToolUiLocation - src/thread/features/tool_call_ui/location.rs
- Depends on: none
- Used by
  - ToolEventsCommand - src/thread/features/tool_events/command.rs

## ToolUiRaw - src/thread/features/tool_call_ui/raw.rs
- Depends on: none
- Used by
  - ToolEventsCommand - src/thread/features/tool_events/command.rs

## ToolUiTitle - src/thread/features/tool_call_ui/title.rs
- Depends on: none
- Used by
  - ToolEventsCommand - src/thread/features/tool_events/command.rs

## TurnDiff - src/thread/turn/diff.rs
- Depends on: none
- Used by
  - NotificationFeature - src/thread/features/notification/mod.rs

## TurnExecution - src/thread/turn/execution.rs
- Depends on
  - AppServer - src/app_server.rs
  - PlanFallback - src/thread/features/plan/fallback.rs
  - PlanParse - src/thread/features/plan/parse.rs
  - ReconnectGuard - src/thread/turn/execution.rs
- Used by
  - PromptFlow - src/thread/prompt/flow.rs

## UserClient - User/Zed
- Depends on
  - PromptFlow - src/thread/prompt/flow.rs
- Used by: none

## WebImageReplay - src/thread/features/tool_events/web_image.rs
- Depends on: none
- Used by
  - Replay - src/thread/core/replay.rs
