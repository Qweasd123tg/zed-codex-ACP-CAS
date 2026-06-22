//! User-facing review slash-команды и ACP picker-flows поверх `review/start`.

use std::process::Stdio;
use std::time::{SystemTime, UNIX_EPOCH};

use agent_client_protocol::{
    Error,
    schema::v1::{
        PermissionOption, PermissionOptionKind, RequestPermissionOutcome,
        SelectedPermissionOutcome, StopReason, ToolCallId, ToolCallStatus, ToolCallUpdate,
        ToolCallUpdateFields, ToolKind,
    },
};
use codex_app_server_protocol::ReviewTarget;
use serde_json::json;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tracing::warn;

use crate::thread::ThreadInner;

const REVIEW_CANCEL_OPTION_ID: &str = "review-cancel";
const REVIEW_PRESET_UNCOMMITTED_OPTION_ID: &str = "review-preset-uncommitted";
const REVIEW_PRESET_BRANCH_OPTION_ID: &str = "review-preset-branch";
const REVIEW_PRESET_COMMIT_OPTION_ID: &str = "review-preset-commit";
const REVIEW_PRESET_CUSTOM_OPTION_ID: &str = "review-preset-custom";
const REVIEW_BRANCH_PICKER_LIMIT: usize = 100;
const REVIEW_COMMIT_PICKER_LIMIT: usize = 100;

pub(in crate::thread) enum ReviewDispatch {
    Start(ReviewTarget),
    Stop(StopReason),
}

#[derive(Clone)]
struct ReviewCommitEntry {
    sha: String,
    subject: String,
}

pub(in crate::thread) async fn handle_review_command(
    inner: &mut ThreadInner,
    instructions: Option<String>,
) -> Result<ReviewDispatch, Error> {
    if let Some(instructions) = instructions
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        return Ok(ReviewDispatch::Start(ReviewTarget::Custom { instructions }));
    }

    let Some(preset) = show_review_preset_picker(inner).await? else {
        return Ok(ReviewDispatch::Stop(StopReason::EndTurn));
    };

    match preset.as_str() {
        REVIEW_PRESET_UNCOMMITTED_OPTION_ID => {
            Ok(ReviewDispatch::Start(ReviewTarget::UncommittedChanges))
        }
        REVIEW_PRESET_BRANCH_OPTION_ID => handle_review_branch_command(inner, None).await,
        REVIEW_PRESET_COMMIT_OPTION_ID => handle_review_commit_command(inner, None).await,
        REVIEW_PRESET_CUSTOM_OPTION_ID => {
            inner
                .client
                .send_agent_text(
                    "Custom review instructions require text input in the command itself.\nUse `/review <your instructions>`.",
                )
                .await;
            Ok(ReviewDispatch::Stop(StopReason::EndTurn))
        }
        _ => {
            warn!(preset, "review preset picker returned unknown option id");
            inner
                .client
                .send_agent_text(
                    "Could not resolve the selected review preset. Run `/review` again.",
                )
                .await;
            Ok(ReviewDispatch::Stop(StopReason::EndTurn))
        }
    }
}

pub(in crate::thread) async fn handle_review_branch_command(
    inner: &mut ThreadInner,
    branch: Option<String>,
) -> Result<ReviewDispatch, Error> {
    if let Some(branch) = branch
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        return Ok(ReviewDispatch::Start(ReviewTarget::BaseBranch { branch }));
    }

    let branches = match local_git_branches(&inner.workspace_cwd).await {
        Ok(branches) => branches,
        Err(error) => {
            inner
                .client
                .send_agent_text(format!(
                    "Could not list local git branches for this workspace.\nUse `/review <instructions>` for a custom review if needed.\n\nError: {error}"
                ))
                .await;
            return Ok(ReviewDispatch::Stop(StopReason::EndTurn));
        }
    };
    if branches.is_empty() {
        inner
            .client
            .send_agent_text(
                "No local git branches found for this workspace.\nUse `/review <instructions>` for a custom review if needed.",
            )
            .await;
        return Ok(ReviewDispatch::Stop(StopReason::EndTurn));
    }

    let current_branch = current_branch_name(&inner.workspace_cwd)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| "(detached HEAD)".to_string());
    let Some(branch) = show_review_branch_picker(inner, &current_branch, branches).await? else {
        return Ok(ReviewDispatch::Stop(StopReason::EndTurn));
    };

    Ok(ReviewDispatch::Start(ReviewTarget::BaseBranch { branch }))
}

pub(in crate::thread) async fn handle_review_commit_command(
    inner: &mut ThreadInner,
    sha: Option<String>,
) -> Result<ReviewDispatch, Error> {
    if let Some(sha) = sha
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        return Ok(ReviewDispatch::Start(ReviewTarget::Commit {
            sha,
            title: None,
        }));
    }

    let commits = match recent_commits(&inner.workspace_cwd, REVIEW_COMMIT_PICKER_LIMIT).await {
        Ok(commits) => commits,
        Err(error) => {
            inner
                .client
                .send_agent_text(format!(
                    "Could not list recent commits for this workspace.\nUse `/review <instructions>` for a custom review if needed.\n\nError: {error}"
                ))
                .await;
            return Ok(ReviewDispatch::Stop(StopReason::EndTurn));
        }
    };
    if commits.is_empty() {
        inner
            .client
            .send_agent_text(
                "No recent commits found for this workspace.\nUse `/review <instructions>` for a custom review if needed.",
            )
            .await;
        return Ok(ReviewDispatch::Stop(StopReason::EndTurn));
    }

    let Some(commit) = show_review_commit_picker(inner, commits).await? else {
        return Ok(ReviewDispatch::Stop(StopReason::EndTurn));
    };

    Ok(ReviewDispatch::Start(ReviewTarget::Commit {
        sha: commit.sha,
        title: Some(commit.subject),
    }))
}

async fn show_review_preset_picker(inner: &mut ThreadInner) -> Result<Option<String>, Error> {
    let outcome = inner
        .client
        .request_permission(
            ToolCallUpdate::new(
                ToolCallId::new(next_review_tool_call_id("review-preset")),
                ToolCallUpdateFields::new()
                    .title("Select a review preset")
                    .kind(ToolKind::Think)
                    .status(ToolCallStatus::Pending)
                    .content(vec![
                        "Pick what to review. For custom instructions, use `/review <your instructions>`.".into(),
                    ])
                    .raw_input(json!({
                        "presets": [
                            {
                                "id": REVIEW_PRESET_BRANCH_OPTION_ID,
                                "label": "Review against a base branch",
                                "description": "PR-style review against another local branch",
                            },
                            {
                                "id": REVIEW_PRESET_UNCOMMITTED_OPTION_ID,
                                "label": "Review uncommitted changes",
                                "description": "Review staged, unstaged, and untracked changes",
                            },
                            {
                                "id": REVIEW_PRESET_COMMIT_OPTION_ID,
                                "label": "Review a commit",
                                "description": "Review one recent commit",
                            },
                            {
                                "id": REVIEW_PRESET_CUSTOM_OPTION_ID,
                                "label": "Custom review instructions",
                                "description": "Run `/review <instructions>` to use this mode",
                            }
                        ]
                    })),
            ),
            vec![
                PermissionOption::new(
                    REVIEW_PRESET_BRANCH_OPTION_ID,
                    "Review against a base branch (PR style)",
                    PermissionOptionKind::AllowOnce,
                ),
                PermissionOption::new(
                    REVIEW_PRESET_UNCOMMITTED_OPTION_ID,
                    "Review uncommitted changes",
                    PermissionOptionKind::AllowOnce,
                ),
                PermissionOption::new(
                    REVIEW_PRESET_COMMIT_OPTION_ID,
                    "Review a commit",
                    PermissionOptionKind::AllowOnce,
                ),
                PermissionOption::new(
                    REVIEW_PRESET_CUSTOM_OPTION_ID,
                    "Custom review instructions (`/review <text>`)",
                    PermissionOptionKind::AllowOnce,
                ),
                PermissionOption::new(
                    REVIEW_CANCEL_OPTION_ID,
                    "Cancel",
                    PermissionOptionKind::RejectOnce,
                ),
            ],
        )
        .await?;

    selected_option_id(outcome)
}

async fn show_review_branch_picker(
    inner: &mut ThreadInner,
    current_branch: &str,
    branches: Vec<String>,
) -> Result<Option<String>, Error> {
    let raw_input = review_branch_picker_raw_input(current_branch, &branches);
    let count = branches.len();
    let choices = branches
        .into_iter()
        .map(|branch| (format!("{current_branch} -> {branch}"), branch))
        .collect::<Vec<_>>();

    show_review_choice_picker(
        inner,
        "review-base-branch",
        format!("Select a base branch ({count} match(es))"),
        "Search in the picker list. Open View Raw Input for the full branch list.",
        raw_input,
        choices,
        "review branch picker returned unknown option id",
        "Could not resolve the selected branch. Run `/review` again.",
    )
    .await
}

async fn show_review_commit_picker(
    inner: &mut ThreadInner,
    commits: Vec<ReviewCommitEntry>,
) -> Result<Option<ReviewCommitEntry>, Error> {
    let raw_input = review_commit_picker_raw_input(&commits);
    let count = commits.len();
    let choices = commits
        .into_iter()
        .map(|commit| {
            let short_sha: String = commit.sha.chars().take(7).collect();
            (format!("{short_sha} · {}", commit.subject), commit)
        })
        .collect::<Vec<_>>();

    show_review_choice_picker(
        inner,
        "review-select-commit",
        format!("Select a commit to review ({count} match(es))"),
        "Search in the picker list. Open View Raw Input for full SHAs and subjects.",
        raw_input,
        choices,
        "review commit picker returned unknown option id",
        "Could not resolve the selected commit. Run `/review` again.",
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn show_review_choice_picker<T>(
    inner: &mut ThreadInner,
    id_prefix: &str,
    title: String,
    content: &'static str,
    raw_input: serde_json::Value,
    choices: Vec<(String, T)>,
    unknown_log: &'static str,
    unknown_message: &'static str,
) -> Result<Option<T>, Error> {
    let mut options = Vec::new();
    for (index, (label, _)) in choices.iter().enumerate() {
        let option_id = format!("{id_prefix}-{}", index + 1);
        options.push(PermissionOption::new(
            option_id.clone(),
            label.clone(),
            PermissionOptionKind::AllowOnce,
        ));
    }
    options.push(PermissionOption::new(
        REVIEW_CANCEL_OPTION_ID,
        "Cancel",
        PermissionOptionKind::RejectOnce,
    ));

    let outcome = inner
        .client
        .request_permission(
            ToolCallUpdate::new(
                ToolCallId::new(next_review_tool_call_id(id_prefix)),
                ToolCallUpdateFields::new()
                    .title(title)
                    .kind(ToolKind::Think)
                    .status(ToolCallStatus::Pending)
                    .content(vec![content.into()])
                    .raw_input(raw_input),
            ),
            options,
        )
        .await?;

    let Some(selected_option_id) = selected_option_id(outcome)? else {
        return Ok(None);
    };
    let selected_index = selected_option_id
        .strip_prefix(&format!("{id_prefix}-"))
        .and_then(|suffix| suffix.parse::<usize>().ok())
        .and_then(|index| index.checked_sub(1));
    let Some((_, value)) = selected_index.and_then(|index| choices.into_iter().nth(index)) else {
        warn!(selected_option_id, "{}", unknown_log);
        inner.client.send_agent_text(unknown_message).await;
        return Ok(None);
    };
    Ok(Some(value))
}

fn selected_option_id(outcome: RequestPermissionOutcome) -> Result<Option<String>, Error> {
    match outcome {
        RequestPermissionOutcome::Cancelled => Ok(None),
        RequestPermissionOutcome::Selected(SelectedPermissionOutcome { option_id, .. }) => {
            if option_id.0.as_ref() == REVIEW_CANCEL_OPTION_ID {
                Ok(None)
            } else {
                Ok(Some(option_id.0.to_string()))
            }
        }
        other => {
            Err(Error::internal_error()
                .data(format!("unsupported review picker outcome: {other:?}")))
        }
    }
}

async fn local_git_branches(cwd: &std::path::Path) -> Result<Vec<String>, Error> {
    let stdout = run_git(
        cwd,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    )
    .await?;
    Ok(stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(REVIEW_BRANCH_PICKER_LIMIT)
        .map(ToString::to_string)
        .collect())
}

async fn current_branch_name(cwd: &std::path::Path) -> Result<Option<String>, Error> {
    let stdout = run_git(cwd, &["branch", "--show-current"]).await?;
    let branch = stdout.trim();
    if branch.is_empty() {
        Ok(None)
    } else {
        Ok(Some(branch.to_string()))
    }
}

async fn recent_commits(
    cwd: &std::path::Path,
    limit: usize,
) -> Result<Vec<ReviewCommitEntry>, Error> {
    let limit = limit.max(1).to_string();
    let stdout = run_git(
        cwd,
        &[
            "log",
            "--pretty=format:%H%x09%s",
            "-n",
            &limit,
            "--no-decorate",
        ],
    )
    .await?;

    Ok(stdout
        .lines()
        .filter_map(|line| {
            let (sha, subject) = line.split_once('\t')?;
            let sha = sha.trim();
            let subject = subject.trim();
            if sha.is_empty() || subject.is_empty() {
                return None;
            }
            Some(ReviewCommitEntry {
                sha: sha.to_string(),
                subject: subject.to_string(),
            })
        })
        .collect())
}

async fn run_git(cwd: &std::path::Path, args: &[&str]) -> Result<String, Error> {
    let mut child = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| Error::internal_error().data(format!("failed to start git: {error}")))?;

    let mut stdout = String::new();
    if let Some(mut pipe) = child.stdout.take() {
        pipe.read_to_string(&mut stdout).await.map_err(|error| {
            Error::internal_error().data(format!("failed to read git stdout: {error}"))
        })?;
    }

    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        pipe.read_to_string(&mut stderr).await.map_err(|error| {
            Error::internal_error().data(format!("failed to read git stderr: {error}"))
        })?;
    }

    let status = child.wait().await.map_err(|error| {
        Error::internal_error().data(format!("failed to wait for git: {error}"))
    })?;

    if status.success() {
        return Ok(stdout);
    }

    let stderr = stderr.trim();
    if stderr.is_empty() {
        Err(Error::internal_error().data(format!(
            "git {} failed with status {status}",
            args.join(" ")
        )))
    } else {
        Err(Error::internal_error().data(stderr.to_string()))
    }
}

fn review_branch_picker_raw_input(current_branch: &str, branches: &[String]) -> serde_json::Value {
    json!({
        "current_branch": current_branch,
        "count": branches.len(),
        "branches": branches,
    })
}

fn review_commit_picker_raw_input(commits: &[ReviewCommitEntry]) -> serde_json::Value {
    json!({
        "count": commits.len(),
        "commits": commits.iter().enumerate().map(|(index, commit)| {
            let short_sha: String = commit.sha.chars().take(7).collect();
            json!({
                "index": index + 1,
                "sha": commit.sha,
                "short_sha": short_sha,
                "subject": commit.subject,
                "preview": format!("{short_sha} {}", commit.subject),
            })
        }).collect::<Vec<_>>()
    })
}

pub(in crate::thread) fn review_user_hint(target: &ReviewTarget) -> String {
    match target {
        ReviewTarget::UncommittedChanges => {
            "Starting inline review for uncommitted changes.".to_string()
        }
        ReviewTarget::BaseBranch { branch } => {
            format!("Starting inline review against branch `{branch}`.")
        }
        ReviewTarget::Commit { sha, .. } => {
            format!("Starting inline review for commit `{sha}`.")
        }
        ReviewTarget::Custom { instructions } => {
            format!("Starting inline custom review: `{instructions}`")
        }
    }
}

fn next_review_tool_call_id(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{prefix}-{nanos}")
}

#[cfg(test)]
mod tests {
    use super::{
        ReviewCommitEntry, review_branch_picker_raw_input, review_commit_picker_raw_input,
    };

    #[test]
    fn branch_picker_raw_input_keeps_full_branch_names() {
        let raw = review_branch_picker_raw_input(
            "main",
            &["feature/login".to_string(), "bugfix/long/name".to_string()],
        );

        assert_eq!(raw["current_branch"], "main");
        assert_eq!(raw["branches"][0], "feature/login");
        assert_eq!(raw["branches"][1], "bugfix/long/name");
    }

    #[test]
    fn commit_picker_raw_input_keeps_full_sha_and_subject() {
        let raw = review_commit_picker_raw_input(&[ReviewCommitEntry {
            sha: "1234567890abcdef".to_string(),
            subject: "Refactor review picker".to_string(),
        }]);

        assert_eq!(raw["commits"][0]["sha"], "1234567890abcdef");
        assert_eq!(raw["commits"][0]["short_sha"], "1234567");
        assert_eq!(raw["commits"][0]["subject"], "Refactor review picker");
    }
}
