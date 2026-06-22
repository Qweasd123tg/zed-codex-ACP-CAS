use std::io;

use super::DisplayMapsConfig;

pub(super) fn parse_display_maps_jsonc(contents: &str) -> io::Result<DisplayMapsConfig> {
    serde_json::from_str(&strip_json_trailing_commas(&strip_json_comments(contents)?))
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}

pub(super) fn display_maps_jsonc(config: &DisplayMapsConfig) -> io::Result<Vec<u8>> {
    let json = serde_json::to_string_pretty(config)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    let mut output = String::from(
        "{\n  // Account limit display maps. Values receive percentages in the 0..100 range.\n",
    );
    let inner = json
        .trim_start_matches('{')
        .trim_end_matches('}')
        .trim_matches('\n');
    for line in inner.lines() {
        output.push_str(line);
        output.push('\n');
    }
    output.push_str("}\n");
    Ok(output.into_bytes())
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
            "unterminated block comment in display maps",
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
