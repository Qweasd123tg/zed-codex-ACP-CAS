//! Рендер и обновление command/mcp/web/image tool-call событий.
//! Модуль оставлен фасадом; конкретные сценарии вынесены по типам tool-call.

#[path = "command.rs"]
pub(in crate::thread) mod command;
#[path = "mcp.rs"]
pub(in crate::thread) mod mcp;
#[path = "web_image.rs"]
pub(in crate::thread) mod web_image;
