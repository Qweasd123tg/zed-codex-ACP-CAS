//! Handlers for server-initiated JSON-RPC requests that require a direct ACP response.

use super::{Error, ServerRequest, ThreadInner, protocol_contract};
use crate::thread::features::approvals;

// Map server requests to concrete ACP prompts and approval flows.
pub(super) async fn handle_server_request(
    inner: &mut ThreadInner,
    request: codex_app_server_protocol::JSONRPCRequest,
) -> Result<(), Error> {
    let request_id = request.id.clone();
    let request_method = request.method.clone();
    let server_request = match ServerRequest::try_from(request) {
        Ok(server_request) => server_request,
        Err(err) => {
            protocol_contract::reject_unparseable_server_request(
                &mut inner.app,
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
        ServerRequest::DynamicToolCall { request_id, .. } => {
            protocol_contract::reject_unsupported_server_request(
                &mut inner.app,
                request_id,
                "item/tool/call",
            )
            .await
        }
        ServerRequest::ChatgptAuthTokensRefresh { request_id, .. } => {
            protocol_contract::reject_unsupported_server_request(
                &mut inner.app,
                request_id,
                "account/chatgptAuthTokens/refresh",
            )
            .await
        }
        ServerRequest::ApplyPatchApproval { request_id, .. } => {
            protocol_contract::reject_unsupported_server_request(
                &mut inner.app,
                request_id,
                "applyPatchApproval",
            )
            .await
        }
        ServerRequest::ExecCommandApproval { request_id, .. } => {
            protocol_contract::reject_unsupported_server_request(
                &mut inner.app,
                request_id,
                "execCommandApproval",
            )
            .await
        }
    }
}
