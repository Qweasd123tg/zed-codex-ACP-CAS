//! Domain UI helpers for shell and MCP cards: titles, kinds, and raw input.
//! This module stays as a facade; heuristics and formatting live in submodules.

#[path = "kind.rs"]
pub(in crate::thread) mod kind;
#[path = "raw.rs"]
pub(in crate::thread) mod raw;
#[path = "title.rs"]
pub(in crate::thread) mod title;
