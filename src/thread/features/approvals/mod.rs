//! Approval logic for command, file-change, and user-input requests from app-server.
//! This file stays as a facade; scenario-specific details live in submodules.

#[path = "command.rs"]
pub(in crate::thread) mod command;
#[path = "file_change.rs"]
pub(in crate::thread) mod file_change;
#[path = "user_input.rs"]
pub(in crate::thread) mod user_input;
