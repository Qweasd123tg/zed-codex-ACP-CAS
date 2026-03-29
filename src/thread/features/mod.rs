//! Доменные feature-срезы thread-слоя.
//! Здесь собираем логику по функциональным областям, чтобы уменьшать связность.

pub(super) mod approvals;
pub(super) mod collab;
pub(super) mod dynamic_tool_call;
pub(super) mod file;
pub(super) mod notification;
pub(super) mod plan;
pub(super) mod resume;
pub(super) mod session;
pub(super) mod status_mapping;
pub(super) mod tool_call_ui;
pub(super) mod tool_events;
