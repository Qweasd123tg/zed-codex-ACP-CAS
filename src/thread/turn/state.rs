//! Хелперы учёта завершения turn для дедупликации финальных событий по turn id.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TurnCompletionDisposition {
    Accepted,
    Duplicate,
    UnexpectedTurnId,
}

// Дедупликация терминальных уведомлений текущего turn. Нам достаточно помнить только
// последний принятый turn_id: другие id отсекаются проверкой expected == completed,
// а двойной приём одного id — сравнением с last_completed.
pub(super) fn register_turn_completion(
    last_completed_turn_id: &mut Option<String>,
    expected_turn_id: &str,
    completed_turn_id: &str,
) -> TurnCompletionDisposition {
    if completed_turn_id != expected_turn_id {
        return TurnCompletionDisposition::UnexpectedTurnId;
    }

    if last_completed_turn_id.as_deref() == Some(completed_turn_id) {
        return TurnCompletionDisposition::Duplicate;
    }

    *last_completed_turn_id = Some(completed_turn_id.to_string());
    TurnCompletionDisposition::Accepted
}

#[cfg(test)]
mod tests {
    use super::{TurnCompletionDisposition, register_turn_completion};

    #[test]
    fn register_turn_completion_accepts_first_completion() {
        let mut last_completed: Option<String> = None;
        let result = register_turn_completion(&mut last_completed, "turn_1", "turn_1");
        assert_eq!(result, TurnCompletionDisposition::Accepted);
        assert_eq!(last_completed.as_deref(), Some("turn_1"));
    }

    #[test]
    fn register_turn_completion_detects_duplicate() {
        let mut last_completed = Some("turn_1".to_string());
        let result = register_turn_completion(&mut last_completed, "turn_1", "turn_1");
        assert_eq!(result, TurnCompletionDisposition::Duplicate);
    }

    #[test]
    fn register_turn_completion_rejects_unexpected_turn_id() {
        let mut last_completed: Option<String> = None;
        let result = register_turn_completion(&mut last_completed, "turn_1", "turn_2");
        assert_eq!(result, TurnCompletionDisposition::UnexpectedTurnId);
        assert!(last_completed.is_none());
    }
}
