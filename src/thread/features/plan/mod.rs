//! Фасад plan-логики.
//! Публичный API сохранён, а реализация разделена на fallback-state-machine и parse/mode helpers.

#[path = "events.rs"]
pub(in crate::thread) mod events;
#[path = "fallback.rs"]
mod fallback;
#[path = "parse.rs"]
mod parse;

#[cfg(test)]
pub(in crate::thread) use fallback::fallback_plan_should_advance;
pub(in crate::thread) use fallback::{
    clear_visible_plan_state, fallback_plan_can_enter_summarizing, fallback_plan_entries_for_steps,
    has_visible_plan_state, initialize_fallback_plan_for_turn, maybe_advance_fallback_plan,
    should_clear_visible_plan_for_mode_change,
};
pub(in crate::thread) use parse::{
    collaboration_mode_for_turn, collaboration_mode_label, limit_plan_entries,
    parse_collaboration_mode, plan_entries_all_pending, plan_from_plan_item_text, plan_from_text,
    promote_first_pending_step, turn_plan_step_to_entry,
};
