//! Helpers for tracking turn completion and deduplicating final events by turn id.

use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TurnCompletionDisposition {
    Accepted,
    Duplicate,
    UnexpectedTurnId,
}

// Track completed turn ids so duplicate terminal notifications can be ignored.
pub(super) fn register_turn_completion(
    completed_turn_ids: &mut HashSet<String>,
    expected_turn_id: &str,
    completed_turn_id: &str,
) -> TurnCompletionDisposition {
    if completed_turn_id != expected_turn_id {
        return TurnCompletionDisposition::UnexpectedTurnId;
    }

    if !completed_turn_ids.insert(completed_turn_id.to_string()) {
        return TurnCompletionDisposition::Duplicate;
    }

    TurnCompletionDisposition::Accepted
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{TurnCompletionDisposition, register_turn_completion};

    #[test]
    fn register_turn_completion_accepts_first_completion() {
        let mut completed_turn_ids = HashSet::new();
        let result = register_turn_completion(&mut completed_turn_ids, "turn_1", "turn_1");
        assert_eq!(result, TurnCompletionDisposition::Accepted);
        assert!(completed_turn_ids.contains("turn_1"));
    }

    #[test]
    fn register_turn_completion_detects_duplicate() {
        let mut completed_turn_ids = HashSet::from(["turn_1".to_string()]);
        let result = register_turn_completion(&mut completed_turn_ids, "turn_1", "turn_1");
        assert_eq!(result, TurnCompletionDisposition::Duplicate);
    }

    #[test]
    fn register_turn_completion_rejects_unexpected_turn_id() {
        let mut completed_turn_ids = HashSet::new();
        let result = register_turn_completion(&mut completed_turn_ids, "turn_1", "turn_2");
        assert_eq!(result, TurnCompletionDisposition::UnexpectedTurnId);
        assert!(completed_turn_ids.is_empty());
    }
}
