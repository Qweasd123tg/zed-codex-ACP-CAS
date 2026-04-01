//! Обработчики server-инициированных JSON-RPC-запросов, требующих прямого ACP-ответа.

use super::{Error, ServerRequest, ThreadInner, protocol_contract};
use crate::thread::features::approvals;

// Преобразуем server request в конкретные ACP-промпты и подтверждения.
pub(super) async fn handle_server_request(
    inner: &mut ThreadInner,
    request: codex_app_server_protocol::JSONRPCRequest,
) -> Result<(), Error> {
    let request_id = request.id.clone();
    let request_method = request.method.clone();

    let server_request = match ServerRequest::try_from(request) {
        Ok(server_request) => server_request,
        Err(err) => {
            let mut app = inner.app.lock().await;
            protocol_contract::reject_unparseable_server_request(
                &mut app,
                request_id,
                &request_method,
                &err,
            )
            .await?;
            return Ok(());
        }
    };

    match server_request {
        ServerRequest::CommandExecutionRequestApproval { request_id, params } => {
            approvals::command::handle_command_approval(inner, request_id, params).await
        }
        ServerRequest::FileChangeRequestApproval { request_id, params } => {
            approvals::file_change::handle_file_change_approval(inner, request_id, params).await
        }
        ServerRequest::ToolRequestUserInput { request_id, params } => {
            approvals::user_input::handle_tool_request_user_input(inner, request_id, params).await
        }
        ServerRequest::McpServerElicitationRequest { request_id, .. } => {
            let mut app = inner.app.lock().await;
            protocol_contract::reject_unsupported_server_request(
                &mut app,
                request_id,
                "mcpServer/elicitation/request",
            )
            .await
        }
        ServerRequest::PermissionsRequestApproval { request_id, params } => {
            approvals::permissions::handle_permissions_request_approval(inner, request_id, params)
                .await
        }
        ServerRequest::DynamicToolCall { request_id, .. } => {
            let mut app = inner.app.lock().await;
            protocol_contract::reject_unsupported_server_request(
                &mut app,
                request_id,
                "item/tool/call",
            )
            .await
        }
        ServerRequest::ChatgptAuthTokensRefresh { request_id, .. } => {
            let mut app = inner.app.lock().await;
            protocol_contract::reject_unsupported_server_request(
                &mut app,
                request_id,
                "account/chatgptAuthTokens/refresh",
            )
            .await
        }
        ServerRequest::ApplyPatchApproval { request_id, .. } => {
            let mut app = inner.app.lock().await;
            protocol_contract::reject_unsupported_server_request(
                &mut app,
                request_id,
                "applyPatchApproval",
            )
            .await
        }
        ServerRequest::ExecCommandApproval { request_id, .. } => {
            let mut app = inner.app.lock().await;
            protocol_contract::reject_unsupported_server_request(
                &mut app,
                request_id,
                "execCommandApproval",
            )
            .await
        }
    }
}
