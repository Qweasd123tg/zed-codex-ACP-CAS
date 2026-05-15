use std::io;

use serde::Serialize;

use super::SelectorPreferences;

pub(super) fn parse_selector_preferences_jsonc(contents: &str) -> io::Result<SelectorPreferences> {
    serde_json::from_str(&strip_json_trailing_commas(&strip_json_comments(contents)?))
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}

pub(super) fn selector_preferences_jsonc(preferences: &SelectorPreferences) -> io::Result<Vec<u8>> {
    let mut sections = Vec::new();
    if let Some(display) = &preferences.display {
        sections.push(jsonc_section(
            "Display styles for compact lower-panel selectors.",
            "display",
            display,
        )?);
    }
    if let Some(defaults) = &preferences.defaults {
        sections.push(jsonc_section(
            "Defaults applied when a new ACP session starts. null keeps the app-server default.",
            "defaults",
            defaults,
        )?);
    }
    if let Some(model_selector) = &preferences.model_selector {
        sections.push(jsonc_section(
            "Model selector controls. Comment out list rows to hide them; row order controls menu order.",
            "model_selector",
            model_selector,
        )?);
    }
    if let Some(layout) = &preferences.layout {
        sections.push(jsonc_section(
            "Lower selector order, titles, visibility, and group order.",
            "layout",
            layout,
        )?);
    }
    if let Some(slash_commands) = &preferences.slash_commands {
        sections.push(jsonc_section(
            "Slash commands. Comment out list rows to hide/block them; row order controls Zed command order.",
            "slash_commands",
            slash_commands,
        )?);
    }

    let mut output = String::from("{\n");
    output.push_str(&sections.join(",\n\n"));
    output.push_str("\n}\n");
    Ok(output.into_bytes())
}

fn jsonc_section<T: Serialize>(comment: &str, key: &str, value: &T) -> io::Result<String> {
    let json = serde_json::to_string_pretty(value)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    let mut lines = json.lines();
    let mut section = format!("  // {comment}\n  \"{key}\": ");
    if let Some(first) = lines.next() {
        section.push_str(first);
    }
    for line in lines {
        section.push('\n');
        section.push_str("  ");
        section.push_str(line);
    }
    Ok(section)
}

fn strip_json_comments(input: &str) -> io::Result<String> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum State {
        Normal,
        String,
        LineComment,
        BlockComment,
    }

    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut state = State::Normal;
    let mut escaped = false;

    while let Some(ch) = chars.next() {
        match state {
            State::Normal => {
                if ch == '"' {
                    output.push(ch);
                    state = State::String;
                } else if ch == '/' && chars.peek() == Some(&'/') {
                    chars.next();
                    output.push(' ');
                    output.push(' ');
                    state = State::LineComment;
                } else if ch == '/' && chars.peek() == Some(&'*') {
                    chars.next();
                    output.push(' ');
                    output.push(' ');
                    state = State::BlockComment;
                } else {
                    output.push(ch);
                }
            }
            State::String => {
                output.push(ch);
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    state = State::Normal;
                }
            }
            State::LineComment => {
                if ch == '\n' {
                    output.push('\n');
                    state = State::Normal;
                } else {
                    output.push(' ');
                }
            }
            State::BlockComment => {
                if ch == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    output.push(' ');
                    output.push(' ');
                    state = State::Normal;
                } else if ch == '\n' {
                    output.push('\n');
                } else {
                    output.push(' ');
                }
            }
        }
    }

    if state == State::BlockComment {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unterminated block comment in selector preferences",
        ));
    }

    Ok(output)
}

fn strip_json_trailing_commas(input: &str) -> String {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum State {
        Normal,
        String,
    }

    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut state = State::Normal;
    let mut escaped = false;

    while let Some(ch) = chars.next() {
        match state {
            State::Normal => {
                if ch == '"' {
                    output.push(ch);
                    state = State::String;
                } else if ch == ',' {
                    let mut lookahead = chars.clone();
                    while matches!(lookahead.peek(), Some(next) if next.is_whitespace()) {
                        lookahead.next();
                    }
                    if matches!(lookahead.peek(), Some(']' | '}')) {
                        output.push(' ');
                    } else {
                        output.push(ch);
                    }
                } else {
                    output.push(ch);
                }
            }
            State::String => {
                output.push(ch);
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    state = State::Normal;
                }
            }
        }
    }

    output
}
