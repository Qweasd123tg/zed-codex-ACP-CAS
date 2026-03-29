//! Логика slash-команд, связанных с выбором и загрузкой thread (`/threads`, `/resume`).
//! Корневой модуль оставлен фасадом; сценарии разнесены по подпотокам.

#[path = "apply.rs"]
pub(in crate::thread) mod apply;
#[path = "common.rs"]
pub(in crate::thread) mod common;
#[path = "listing.rs"]
pub(in crate::thread) mod listing;
#[path = "selector.rs"]
pub(in crate::thread) mod selector;
