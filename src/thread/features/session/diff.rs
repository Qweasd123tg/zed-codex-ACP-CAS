//! Slash-команда `/diff`: показывает unified-diff последнего turn или накопленную
//! историю turn-diffs для текущей сессии. Использует уже собранную `turn_diff_history`
//! (см. `thread/turn/diff.rs`) и ACP `Diff` content, как и обычная turn-diff карточка.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use agent_client_protocol::{
    Diff, Error, StopReason, ToolCall, ToolCallContent, ToolCallId, ToolCallLocation,
    ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
};

use crate::thread::turn_diff::{
    TurnUnifiedDiffFile, parse_turn_diff_files, resolve_turn_diff_absolute_path,
};
use crate::thread::{DIFF_COMMAND_TOOL_CALL_PREFIX, DiffScope, ThreadInner, TurnDiffRecord};

// Агрегат по файлу, который мы показываем пользователю.
struct AggregatedFile {
    display_path: String,
    absolute_path: PathBuf,
    // Для ACP Diff: старое содержимое берём из самой ранней версии в диапазоне,
    // новое — из последней. Так пользователь видит "итоговый diff диапазона".
    old_text: String,
    new_text: String,
    is_delete: bool,
    // Для текстовой сводки.
    added_lines: usize,
    removed_lines: usize,
    // Сколько turns затронули этот файл.
    touching_turns: usize,
}

pub(in crate::thread) async fn handle_diff_command(
    inner: &mut ThreadInner,
    scope: DiffScope,
    paths: Vec<String>,
) -> Result<StopReason, Error> {
    let records = select_records(&inner.turn_diff_history, scope);
    if records.is_empty() {
        let message = match scope {
            DiffScope::LastTurn => {
                "No diff to show yet. `/diff` becomes available after a turn finishes with file changes."
            }
            DiffScope::Session => {
                "No diffs recorded in this session yet. Finish a turn with file changes first."
            }
            DiffScope::LastN(_) => "Not enough turn diffs recorded yet for this range.",
        };
        inner.client.send_agent_text(message).await;
        return Ok(StopReason::EndTurn);
    }

    let filter = normalize_path_filters(&paths);
    let aggregated = aggregate_files(&inner.workspace_cwd, records.iter().copied(), &filter);
    if aggregated.is_empty() {
        let message = if filter.is_empty() {
            "Recorded turn diffs contained no net file changes (all changes cancel out)."
                .to_string()
        } else {
            format!(
                "No recorded file changes match the provided path filter: {}",
                filter.join(", ")
            )
        };
        inner.client.send_agent_text(message).await;
        return Ok(StopReason::EndTurn);
    }

    let tool_call_key = next_diff_tool_call_key(inner);
    let tool_call_id = ToolCallId::new(tool_call_key.clone());
    let tool_call_title = diff_card_title(scope, records.len());
    let summary = build_summary_text(scope, records.len(), &aggregated);
    inner.client.send_agent_text(summary).await;

    let locations: Vec<ToolCallLocation> = aggregated
        .iter()
        .map(|file| ToolCallLocation::new(file.absolute_path.clone()))
        .collect();

    let content: Vec<ToolCallContent> = aggregated
        .into_iter()
        .map(|file| {
            let old_text = if file.old_text.is_empty() && !file.is_delete {
                None
            } else {
                Some(file.old_text)
            };
            ToolCallContent::Diff(
                Diff::new(file.absolute_path.clone(), file.new_text).old_text(old_text),
            )
        })
        .collect();

    inner.started_tool_calls.insert(tool_call_key.clone());
    inner
        .client
        .send_tool_call(
            ToolCall::new(tool_call_id.clone(), tool_call_title.clone())
                .kind(ToolKind::Edit)
                .status(ToolCallStatus::InProgress)
                .locations(locations.clone())
                .content(content.clone()),
        )
        .await;
    inner
        .client
        .send_tool_call_update(ToolCallUpdate::new(
            tool_call_id,
            ToolCallUpdateFields::new()
                .kind(ToolKind::Edit)
                .title(tool_call_title)
                .status(ToolCallStatus::Completed)
                .locations(locations)
                .content(content),
        ))
        .await;
    inner.started_tool_calls.remove(&tool_call_key);

    Ok(StopReason::EndTurn)
}

// Выбираем срез истории для заданного scope. Возвращаем ссылки, чтобы не клонировать unified_diff.
fn select_records(history: &[TurnDiffRecord], scope: DiffScope) -> Vec<&TurnDiffRecord> {
    if history.is_empty() {
        return Vec::new();
    }
    match scope {
        DiffScope::LastTurn => history
            .last()
            .map(|record| vec![record])
            .unwrap_or_default(),
        DiffScope::Session => history.iter().collect(),
        DiffScope::LastN(count) => {
            let count = count as usize;
            let start = history.len().saturating_sub(count);
            history[start..].iter().collect()
        }
    }
}

fn normalize_path_filters(paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .map(|value| value.trim().trim_matches('"').to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

fn matches_path_filter(display_path: &str, filter: &[String]) -> bool {
    if filter.is_empty() {
        return true;
    }
    filter.iter().any(|pattern| {
        display_path == pattern
            || display_path.ends_with(pattern)
            || Path::new(display_path)
                .file_name()
                .map(|name| name.to_string_lossy() == *pattern)
                .unwrap_or(false)
            || display_path.contains(pattern)
    })
}

// Сворачиваем последовательные turn-diffs по файлам. Старое берём из первой версии,
// новое — из последней. Если файл в последнем turn удалён — помечаем is_delete.
fn aggregate_files<'a, I>(
    workspace_cwd: &Path,
    records: I,
    filter: &[String],
) -> Vec<AggregatedFile>
where
    I: IntoIterator<Item = &'a TurnDiffRecord>,
{
    // BTreeMap даёт стабильный порядок (по display_path).
    let mut aggregated: BTreeMap<String, AggregatedFile> = BTreeMap::new();

    for record in records {
        let parsed = parse_turn_diff_files(&record.unified_diff);
        let mut touched_this_record: HashSet<String> = HashSet::new();
        for file in parsed {
            let display_path = file.path.to_string_lossy().into_owned();
            if !matches_path_filter(&display_path, filter) {
                continue;
            }
            let (added, removed) = count_diff_lines(&file);
            touched_this_record.insert(display_path.clone());
            let absolute_path = resolve_turn_diff_absolute_path(workspace_cwd, &file.path);
            match aggregated.get_mut(&display_path) {
                Some(existing) => {
                    // Старый текст оставляем от первой записи; новый — перетираем.
                    existing.new_text = file.new_text;
                    existing.is_delete = file.is_delete;
                    existing.added_lines += added;
                    existing.removed_lines += removed;
                }
                None => {
                    aggregated.insert(
                        display_path.clone(),
                        AggregatedFile {
                            display_path,
                            absolute_path,
                            old_text: file.old_text,
                            new_text: file.new_text,
                            is_delete: file.is_delete,
                            added_lines: added,
                            removed_lines: removed,
                            touching_turns: 0,
                        },
                    );
                }
            }
        }
        for path in touched_this_record {
            if let Some(entry) = aggregated.get_mut(&path) {
                entry.touching_turns += 1;
            }
        }
    }

    // Отфильтровываем файлы, которые в итоге ничего не меняют (old==new, не delete).
    aggregated
        .into_values()
        .filter(|file| file.is_delete || file.old_text != file.new_text)
        .collect()
}

fn count_diff_lines(file: &TurnUnifiedDiffFile) -> (usize, usize) {
    let old_lines = if file.old_text.is_empty() {
        0
    } else {
        file.old_text.lines().count()
    };
    let new_lines = if file.new_text.is_empty() {
        0
    } else {
        file.new_text.lines().count()
    };
    // Точная статистика "+/-" требует повторного парсинга hunks; берём грубую оценку
    // по числу строк. Для краткой сводки этого достаточно.
    if file.is_delete {
        (0, old_lines)
    } else if file.old_text.is_empty() {
        (new_lines, 0)
    } else {
        // Используем максимум одной стороны как оценку «изменённых строк».
        // Это не git --stat, но читаемый ориентир.
        let changed = new_lines.max(old_lines);
        let added = new_lines.saturating_sub(old_lines.min(new_lines));
        let removed = old_lines.saturating_sub(new_lines.min(old_lines));
        // Если ни add, ни remove (одинаковая длина, модификация в теле) — показываем "≈ changed".
        if added == 0 && removed == 0 {
            (changed, changed)
        } else {
            (added.max(0), removed.max(0))
        }
    }
}

fn build_summary_text(scope: DiffScope, record_count: usize, files: &[AggregatedFile]) -> String {
    let scope_label = match scope {
        DiffScope::LastTurn => "last turn".to_string(),
        DiffScope::Session => format!("session ({record_count} turn{})", plural(record_count)),
        DiffScope::LastN(count) => {
            format!(
                "last {count} turn{} (showing {record_count})",
                plural(count as usize)
            )
        }
    };

    let total_added: usize = files.iter().map(|file| file.added_lines).sum();
    let total_removed: usize = files.iter().map(|file| file.removed_lines).sum();

    let mut out = String::new();
    out.push_str(&format!(
        "Diff for {scope_label}: {} file{}, +{total_added} / -{total_removed}\n",
        files.len(),
        plural(files.len())
    ));

    for file in files {
        let marker = if file.is_delete { "[deleted] " } else { "" };
        let turns_note = if file.touching_turns > 1 {
            format!(" ({} turns)", file.touching_turns)
        } else {
            String::new()
        };
        out.push_str(&format!(
            "- {marker}`{}` +{} / -{}{turns_note}\n",
            file.display_path, file.added_lines, file.removed_lines
        ));
    }

    out
}

fn diff_card_title(scope: DiffScope, record_count: usize) -> String {
    match scope {
        DiffScope::LastTurn => "Last turn diff".to_string(),
        DiffScope::Session => format!("Session diff ({record_count} turn{})", plural(record_count)),
        DiffScope::LastN(count) => format!("Last {count} turn diff{}", plural(count as usize)),
    }
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

// Тоол-коллы для /diff уникальны per invocation: используем длину истории + размер scope
// как суффикс, чтобы повторные /diff подряд давали разные tool-card id.
fn next_diff_tool_call_key(inner: &ThreadInner) -> String {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    format!(
        "{DIFF_COMMAND_TOOL_CALL_PREFIX}{}-{}",
        inner.turn_diff_history.len(),
        nonce
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(turn: &str, unified: &str) -> TurnDiffRecord {
        TurnDiffRecord {
            turn_id: turn.to_string(),
            recorded_at: 0,
            unified_diff: unified.to_string(),
        }
    }

    #[test]
    fn select_records_last_turn_returns_only_last() {
        let history = vec![record("t1", ""), record("t2", ""), record("t3", "")];
        let selected = select_records(&history, DiffScope::LastTurn);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].turn_id, "t3");
    }

    #[test]
    fn select_records_session_returns_everything() {
        let history = vec![record("t1", ""), record("t2", "")];
        let selected = select_records(&history, DiffScope::Session);
        assert_eq!(selected.len(), 2);
    }

    #[test]
    fn select_records_last_n_clamps_to_history() {
        let history = vec![record("t1", ""), record("t2", "")];
        let selected = select_records(&history, DiffScope::LastN(5));
        assert_eq!(selected.len(), 2);
    }

    #[test]
    fn matches_path_filter_supports_basename_and_contains() {
        let filter = vec!["lib.rs".to_string()];
        assert!(matches_path_filter("src/lib.rs", &filter));
        assert!(!matches_path_filter("src/main.rs", &filter));

        let filter = vec!["features/".to_string()];
        assert!(matches_path_filter("src/features/diff.rs", &filter));
    }

    #[test]
    fn normalize_path_filters_drops_empty_and_quotes() {
        let input = vec![
            "   ".to_string(),
            "\"src/lib.rs\"".to_string(),
            "other".to_string(),
        ];
        let normalized = normalize_path_filters(&input);
        assert_eq!(
            normalized,
            vec!["src/lib.rs".to_string(), "other".to_string()]
        );
    }
}
