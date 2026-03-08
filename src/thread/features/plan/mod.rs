//! Facade for plan-related logic.
//! The public API stays stable while implementation is split into fallback-state-machine and parse/mode helpers.

#[path = "events.rs"]
pub(in crate::thread) mod events;
#[path = "fallback.rs"]
mod fallback;
#[path = "parse.rs"]
mod parse;

#[cfg(test)]
pub(in crate::thread) use fallback::fallback_plan_should_advance;
pub(in crate::thread) use fallback::{
    fallback_plan_can_enter_summarizing, fallback_plan_entries_for_steps,
    initialize_fallback_plan_for_turn, maybe_advance_fallback_plan,
};
pub(in crate::thread) use parse::{
    collaboration_mode_for_turn, collaboration_mode_label, limit_plan_entries,
    parse_collaboration_mode, plan_entries_all_pending, plan_from_plan_item_text, plan_from_text,
    promote_first_pending_step, turn_plan_step_to_entry,
};
