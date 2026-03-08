//! Protocol guardrails for unsupported methods and strict request/response validation.

use agent_client_protocol::Error;
use codex_app_server_protocol::RequestId;
use tracing::warn;

use crate::app_server::AppServerProcess;

pub(super) const JSONRPC_METHOD_NOT_FOUND: i64 = -32601;

// Centralize unsupported-method messages for consistent diagnostics.
pub(super) fn unsupported_method_message(method: &str) -> String {
    format!("Unsupported app-server request `{method}`")
}

pub(super) async fn reject_unparseable_server_request(
    app: &mut AppServerProcess,
    request_id: RequestId,
    request_method: &str,
    parse_error: &impl std::fmt::Display,
) -> Result<(), Error> {
    warn!(
        request_method,
        error = %parse_error,
        "Failed to decode app-server request"
    );
    app.send_server_request_error(
        request_id,
        JSONRPC_METHOD_NOT_FOUND,
        format!("Unsupported app-server request method `{request_method}`"),
        None,
    )
    .await
}

pub(super) async fn reject_unsupported_server_request(
    app: &mut AppServerProcess,
    request_id: RequestId,
    method: &str,
) -> Result<(), Error> {
    warn!(method, "Rejecting unsupported app-server request");
    app.send_server_request_error(
        request_id,
        JSONRPC_METHOD_NOT_FOUND,
        unsupported_method_message(method),
        None,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::unsupported_method_message;

    #[test]
    fn unsupported_method_message_is_stable() {
        assert_eq!(
            unsupported_method_message("item/tool/call"),
            "Unsupported app-server request `item/tool/call`"
        );
    }
}
