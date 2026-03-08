//! Minimal unified-diff parser used to reconstruct before/after text snapshots for previews.

#[derive(Clone, Debug)]
struct UnifiedDiffLine {
    kind: char,
    text: String,
}

#[derive(Clone, Debug)]
struct UnifiedDiffHunk {
    old_start: usize,
    new_start: usize,
    lines: Vec<UnifiedDiffLine>,
}

// Parse `@@` ranges first because the full hunk-application logic depends on them.
fn parse_unified_range(input: &str) -> Option<(usize, usize)> {
    let (start, len) = match input.split_once(',') {
        Some((start, len)) => (start, len),
        None => (input, "1"),
    };
    let start = start.parse::<usize>().ok()?;
    let len = len.parse::<usize>().ok()?;
    Some((start, len))
}

fn parse_unified_hunk_header(line: &str) -> Option<(usize, usize)> {
    let line = line.strip_prefix("@@ -")?;
    let (old_range, rest) = line.split_once(" +")?;
    let (new_range, _) = rest.split_once(" @@")?;
    let (old_start, _old_len) = parse_unified_range(old_range)?;
    let (new_start, _new_len) = parse_unified_range(new_range)?;
    Some((old_start, new_start))
}

fn parse_unified_diff_hunks(unified_diff: &str) -> Option<Vec<UnifiedDiffHunk>> {
    // Parse only the subset we need: hunk headers plus line bodies with
    // (' ', '+', '-') prefixes. File headers and move suffixes are ignored.
    let mut hunks = Vec::new();
    let mut current_hunk: Option<UnifiedDiffHunk> = None;

    for raw_line in unified_diff.split_inclusive('\n') {
        let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);

        if line.starts_with("@@") {
            if let Some(hunk) = current_hunk.take() {
                hunks.push(hunk);
            }
            let (old_start, new_start) = parse_unified_hunk_header(line)?;
            current_hunk = Some(UnifiedDiffHunk {
                old_start,
                new_start,
                lines: Vec::new(),
            });
            continue;
        }

        let Some(hunk) = current_hunk.as_mut() else {
            continue;
        };

        let Some(kind) = line.chars().next() else {
            continue;
        };

        if !matches!(kind, ' ' | '+' | '-') {
            continue;
        }

        let mut text = line[1..].to_string();
        if raw_line.ends_with('\n') {
            text.push('\n');
        }
        hunk.lines.push(UnifiedDiffLine { kind, text });
    }

    if let Some(hunk) = current_hunk {
        hunks.push(hunk);
    }

    if hunks.is_empty() { None } else { Some(hunks) }
}

pub(super) fn unified_diff_to_old_new(unified_diff: &str) -> Option<(String, String)> {
    let hunks = parse_unified_diff_hunks(unified_diff)?;
    let capacity = unified_diff.len() / 2;
    let mut old_text = String::with_capacity(capacity);
    let mut new_text = String::with_capacity(capacity);
    for hunk in hunks {
        for line in hunk.lines {
            match line.kind {
                ' ' => {
                    old_text.push_str(&line.text);
                    new_text.push_str(&line.text);
                }
                '-' => old_text.push_str(&line.text),
                '+' => new_text.push_str(&line.text),
                _ => {}
            }
        }
    }

    if old_text.is_empty() && new_text.is_empty() {
        None
    } else {
        Some((old_text, new_text))
    }
}

pub(super) fn apply_unified_diff_to_text(old_text: &str, unified_diff: &str) -> Option<String> {
    // Best-effort reconstruction of post-edit text from unified diff.
    // Validate context/deleted lines against `old_text` to avoid incorrect previews.
    let hunks = parse_unified_diff_hunks(unified_diff)?;
    let old_lines = if old_text.is_empty() {
        Vec::new()
    } else {
        old_text.split_inclusive('\n').collect::<Vec<_>>()
    };
    let mut old_index = 0usize;
    let mut new_text = String::with_capacity(old_text.len().saturating_add(unified_diff.len() / 8));

    for hunk in hunks {
        let target_index = hunk.old_start.saturating_sub(1);
        if target_index > old_lines.len() || target_index < old_index {
            return None;
        }

        for line in &old_lines[old_index..target_index] {
            new_text.push_str(line);
        }
        old_index = target_index;

        for line in hunk.lines {
            match line.kind {
                ' ' => {
                    let current_line = old_lines.get(old_index)?;
                    if *current_line != line.text {
                        return None;
                    }
                    new_text.push_str(current_line);
                    old_index += 1;
                }
                '-' => {
                    let current_line = old_lines.get(old_index)?;
                    if *current_line != line.text {
                        return None;
                    }
                    old_index += 1;
                }
                '+' => {
                    new_text.push_str(&line.text);
                }
                _ => return None,
            }
        }
    }

    for line in &old_lines[old_index..] {
        new_text.push_str(line);
    }
    Some(new_text)
}

fn usize_to_u32_saturating(value: usize) -> u32 {
    value.min(u32::MAX as usize) as u32
}

pub(super) fn first_hunk_line(unified_diff: &str, use_new_start: bool) -> Option<u32> {
    let first_hunk = parse_unified_diff_hunks(unified_diff)?.into_iter().next()?;
    let start = if use_new_start {
        first_hunk.new_start
    } else {
        first_hunk.old_start
    };
    Some(usize_to_u32_saturating(start.saturating_sub(1)))
}
