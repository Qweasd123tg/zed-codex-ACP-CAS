//! Логика подтверждений для command/file-change/user-input запросов от app-server.
//! Файл оставлен фасадом; детали вынесены по сценариям подтверждения.

#[path = "command.rs"]
pub(in crate::thread) mod command;
#[path = "file_change.rs"]
pub(in crate::thread) mod file_change;
#[path = "permissions.rs"]
pub(in crate::thread) mod permissions;
#[path = "user_input.rs"]
pub(in crate::thread) mod user_input;
