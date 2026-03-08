//! Slash-command logic for selecting and loading threads (`/threads`, `/resume`).
//! The root module remains a facade while concrete flows live in submodules.

#[path = "apply.rs"]
pub(in crate::thread) mod apply;
#[path = "listing.rs"]
pub(in crate::thread) mod listing;
#[path = "selector.rs"]
pub(in crate::thread) mod selector;
