//! Утилиты трекинга turn diff: парсинг/применение unified diff и отправка сводок изменений в ACP.

use std::path::{Path, PathBuf};

use crate::thread::{
    DEV_NULL, Diff, SessionClient, TURN_DIFF_HISTORY_LIMIT, TURN_DIFF_TOOL_CALL_PREFIX,
    ThreadInner, ToolCall, ToolCallContent, ToolCallId, ToolCallLocation, ToolCallStatus,
    ToolCallUpdate, ToolCallUpdateFields, ToolKind, TurnDiffRecord, TurnDiffUpdatedNotification,
    first_hunk_line, unified_diff_to_old_new,
};

#[derive(Clone, Debug)]
pub(in crate::thread) struct TurnUnifiedDiffFile {
    pub(in crate::thread) path: PathBuf,
    pub(in crate::thread) old_text: String,
    pub(in crate::thread) new_text: String,
    pub(in crate::thread) is_delete: bool,
    pub(in crate::thread) line: Option<u32>,
}

#[derive(Clone, Debug)]
struct ResolvedTurnDiffFile {
    path: PathBuf,
    old_text: String,
    new_text: String,
    line: Option<u32>,
}

pub(super) struct PreparedTurnDiffRender {
    pub(super) send_as_new: bool,
    pub(super) tool_call_id: ToolCallId,
    pub(super) locations: Vec<ToolCallLocation>,
    pub(super) content: Vec<ToolCallContent>,
}

pub(super) struct FinalizedTurnDiffSnapshot {
    pub(super) client: SessionClient,
    pub(super) render: Option<PreparedTurnDiffRender>,
}

// Обрабатываем обновления turn-diff в одном месте, чтобы логика patch-preview была консистентной.
pub(super) async fn handle_turn_diff_updated(
    inner: &mut ThreadInner,
    payload: TurnDiffUpdatedNotification,
    expected_turn_id: &str,
) {
    if payload.turn_id != expected_turn_id {
        return;
    }

    inner.latest_turn_diff = Some(payload.diff);
}

pub(super) fn prepare_finalized_turn_diff_snapshot(
    inner: &mut ThreadInner,
    turn_id: &str,
) -> Option<FinalizedTurnDiffSnapshot> {
    let diff = inner.latest_turn_diff.take()?;

    let parsed_files = parse_turn_unified_diff_files(&diff);
    if parsed_files.is_empty() {
        return None;
    }

    // Сохраняем копию в истории ещё до resolution путей: /diff работает с сырым
    // unified-diff, а не с перевязанными file paths. Дедуплицируем по turn_id,
    // чтобы повторные finalize одного turn (например, после reconnect) не раздували список.
    record_turn_diff(inner, turn_id, diff.clone());
    let repo_root = find_repo_root(&inner.workspace_cwd);
    let mut resolved_files = Vec::with_capacity(parsed_files.len());
    for file in parsed_files {
        let path = resolve_turn_diff_path(&inner.workspace_cwd, repo_root.as_deref(), &file.path);
        resolved_files.push(ResolvedTurnDiffFile {
            path,
            old_text: file.old_text,
            new_text: file.new_text,
            line: file.line,
        });
    }
    if resolved_files.is_empty() {
        return None;
    }

    let render = prepare_turn_diff_tool_call(inner, turn_id, resolved_files, false);
    Some(FinalizedTurnDiffSnapshot {
        client: inner.client.clone(),
        render,
    })
}

// Turn diff остаётся transcript-only: disk changes подхватывает watcher клиента,
// а адаптер не отправляет non-atomic full-buffer writeback.
pub(super) async fn emit_finalized_turn_diff_snapshot(snapshot: FinalizedTurnDiffSnapshot) {
    if let Some(render) = snapshot.render {
        if render.send_as_new {
            snapshot
                .client
                .send_tool_call(
                    ToolCall::new(render.tool_call_id, "Turn diff")
                        .kind(ToolKind::Edit)
                        .status(ToolCallStatus::Completed)
                        .locations(render.locations)
                        .content(render.content),
                )
                .await;
        } else {
            snapshot
                .client
                .send_tool_call_update(ToolCallUpdate::new(
                    render.tool_call_id,
                    ToolCallUpdateFields::new()
                        .status(ToolCallStatus::Completed)
                        .locations(render.locations)
                        .content(render.content),
                ))
                .await;
        }
    }
}

pub(super) async fn finalize_turn_diff(inner: &mut ThreadInner, turn_id: &str) {
    let Some(snapshot) = prepare_finalized_turn_diff_snapshot(inner, turn_id) else {
        return;
    };
    emit_finalized_turn_diff_snapshot(snapshot).await;
}

fn prepare_turn_diff_tool_call(
    inner: &mut ThreadInner,
    turn_id: &str,
    resolved_files: Vec<ResolvedTurnDiffFile>,
    in_progress: bool,
) -> Option<PreparedTurnDiffRender> {
    let tool_call_key = format!("{TURN_DIFF_TOOL_CALL_PREFIX}{turn_id}");
    let tool_call_id = ToolCallId::new(tool_call_key.clone());
    let send_as_new = inner.started_tool_calls.insert(tool_call_key.clone());

    let mut content = Vec::new();
    let mut locations = Vec::new();
    for file in resolved_files {
        if inner.file_change_paths_this_turn.contains(&file.path) {
            continue;
        }

        let old_text = if file.old_text.is_empty() {
            None
        } else {
            Some(file.old_text)
        };
        let path = file.path.clone();
        content.push(ToolCallContent::Diff(
            Diff::new(path.clone(), file.new_text).old_text(old_text),
        ));
        let location = ToolCallLocation::new(path);
        locations.push(if let Some(line) = file.line {
            location.line(line)
        } else {
            location
        });
    }
    if content.is_empty() {
        if !in_progress {
            inner.started_tool_calls.remove(&tool_call_key);
        }
        return None;
    }

    if !in_progress {
        inner.started_tool_calls.remove(&tool_call_key);
    }
    Some(PreparedTurnDiffRender {
        send_as_new,
        tool_call_id,
        locations,
        content,
    })
}

pub(super) fn parse_turn_unified_diff_files(unified_diff: &str) -> Vec<TurnUnifiedDiffFile> {
    fn finalize_section(
        section: &mut String,
        old_path: &mut Option<String>,
        new_path: &mut Option<String>,
        output: &mut Vec<TurnUnifiedDiffFile>,
    ) {
        if section.trim().is_empty() {
            section.clear();
            *old_path = None;
            *new_path = None;
            return;
        }

        let old = old_path.take();
        let new = new_path.take();
        let new_is_dev_null = new.as_deref().is_some_and(|path| path.trim() == DEV_NULL);
        let chosen_path = if new_is_dev_null { old } else { new.or(old) };
        let Some(path) = chosen_path else {
            section.clear();
            return;
        };

        let normalized = normalize_unified_diff_path(&path);
        if normalized.is_empty() {
            section.clear();
            return;
        }
        if !section.contains("@@") {
            section.clear();
            return;
        }

        let Some((old_text, new_text)) = unified_diff_to_old_new(section) else {
            section.clear();
            return;
        };
        if old_text == new_text {
            section.clear();
            return;
        }

        output.push(TurnUnifiedDiffFile {
            path: PathBuf::from(normalized),
            old_text,
            new_text,
            is_delete: new_is_dev_null,
            line: if new_is_dev_null {
                first_hunk_line(section, false)
            } else {
                first_hunk_line(section, true).or_else(|| first_hunk_line(section, false))
            },
        });
        section.clear();
    }

    let mut files = Vec::new();
    let mut section = String::with_capacity(unified_diff.len().min(8192));
    let mut old_path: Option<String> = None;
    let mut new_path: Option<String> = None;
    let mut saw_file_header = false;

    for raw_line in unified_diff.split_inclusive('\n') {
        let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);

        if line.starts_with("diff --git ") {
            finalize_section(&mut section, &mut old_path, &mut new_path, &mut files);
            saw_file_header = true;
        }

        if let Some(path) = line.strip_prefix("--- ") {
            old_path = Some(path.trim().to_string());
        } else if let Some(path) = line.strip_prefix("+++ ") {
            new_path = Some(path.trim().to_string());
        }

        if saw_file_header
            || !section.is_empty()
            || line.starts_with("--- ")
            || line.starts_with("+++ ")
        {
            section.push_str(raw_line);
        }
    }

    finalize_section(&mut section, &mut old_path, &mut new_path, &mut files);
    files
}

fn normalize_unified_diff_path(path: &str) -> String {
    let trimmed = path.trim().trim_matches('"');
    if trimmed == DEV_NULL {
        return String::new();
    }
    trimmed
        .strip_prefix("a/")
        .or_else(|| trimmed.strip_prefix("b/"))
        .unwrap_or(trimmed)
        .to_string()
}

fn resolve_turn_diff_path(workspace_cwd: &Path, repo_root: Option<&Path>, path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }

    let direct = workspace_cwd.join(path);
    if direct.exists() {
        return direct;
    }

    if let Some(repo_root) = repo_root {
        let candidate = repo_root.join(path);
        if candidate.exists() {
            return candidate;
        }
    }

    direct
}

fn find_repo_root(workspace_cwd: &Path) -> Option<PathBuf> {
    workspace_cwd
        .ancestors()
        .find(|ancestor| ancestor.join(".git").exists())
        .map(Path::to_path_buf)
}

fn record_turn_diff(inner: &mut ThreadInner, turn_id: &str, unified_diff: String) {
    let recorded_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    // Если тот же turn уже был финализирован — обновляем запись, а не плодим дубли.
    if let Some(existing) = inner
        .turn_diff_history
        .iter_mut()
        .find(|record| record.turn_id == turn_id)
    {
        existing.unified_diff = unified_diff;
        existing.recorded_at = recorded_at;
        return;
    }

    inner.turn_diff_history.push(TurnDiffRecord {
        turn_id: turn_id.to_string(),
        recorded_at,
        unified_diff,
    });
    // Ограничиваем память за долгую сессию: первые записи (самые старые) вытесняются.
    let overflow = inner
        .turn_diff_history
        .len()
        .saturating_sub(TURN_DIFF_HISTORY_LIMIT);
    if overflow > 0 {
        inner.turn_diff_history.drain(0..overflow);
    }
}

// Позволяет /diff переиспользовать тот же парсер, что и finalize_turn_diff.
pub(in crate::thread) fn parse_turn_diff_files(unified_diff: &str) -> Vec<TurnUnifiedDiffFile> {
    parse_turn_unified_diff_files(unified_diff)
}

// Совмещает разрешение путей (workspace_cwd + .git root) с тем, что использует turn-diff.
pub(in crate::thread) fn resolve_turn_diff_absolute_path(
    workspace_cwd: &Path,
    path: &Path,
) -> PathBuf {
    let repo_root = find_repo_root(workspace_cwd);
    resolve_turn_diff_path(workspace_cwd, repo_root.as_deref(), path)
}
