use super::*;

#[test]
fn apply_unified_diff_to_text_reconstructs_content() {
    let old_text = "one\ntwo\nthree\n";
    let unified_diff = "\
--- a/file.txt
+++ b/file.txt
@@ -1,3 +1,4 @@
 one
-two
+TWO
 three
+four
";
    let new_text = apply_unified_diff_to_text(old_text, unified_diff)
        .expect("diff should be applicable to old content");
    assert_eq!(new_text, "one\nTWO\nthree\nfour\n");
}

#[test]
fn unified_diff_to_old_new_ignores_move_suffix() {
    let diff = "\
--- a/src/old.txt
+++ b/src/new.txt
@@ -1 +1 @@
-before
+after

Moved to: src/new.txt
";
    let (old_text, new_text) =
        unified_diff_to_old_new(diff).expect("should extract old/new hunk text");
    assert_eq!(old_text, "before\n");
    assert_eq!(new_text, "after\n");
}

#[test]
fn unified_diff_to_old_new_keeps_hunk_lines_starting_with_header_prefixes() {
    let diff = "\
--- a/src/example.txt
+++ b/src/example.txt
@@ -1 +1 @@
---- starts-with-triple-dash
++++ starts-with-triple-plus
";
    let (old_text, new_text) =
        unified_diff_to_old_new(diff).expect("should keep hunk body lines intact");
    assert_eq!(old_text, "--- starts-with-triple-dash\n");
    assert_eq!(new_text, "+++ starts-with-triple-plus\n");
}

#[test]
fn parse_turn_unified_diff_files_handles_add_update_delete() {
    let diff = "\
diff --git a/src/update.txt b/src/update.txt
--- a/src/update.txt
+++ b/src/update.txt
@@ -1 +1 @@
-old
+new
diff --git a/src/add.txt b/src/add.txt
new file mode 100644
--- /dev/null
+++ b/src/add.txt
@@ -0,0 +1 @@
+added
diff --git a/src/delete.txt b/src/delete.txt
deleted file mode 100644
--- a/src/delete.txt
+++ /dev/null
@@ -1 +0,0 @@
-removed
";

    let files = parse_turn_unified_diff_files(diff);
    assert_eq!(files.len(), 3);

    assert_eq!(files[0].path, PathBuf::from("src/update.txt"));
    assert_eq!(files[0].old_text, "old\n");
    assert_eq!(files[0].new_text, "new\n");
    assert!(!files[0].is_delete);
    assert_eq!(files[0].line, Some(0));

    assert_eq!(files[1].path, PathBuf::from("src/add.txt"));
    assert_eq!(files[1].old_text, "");
    assert_eq!(files[1].new_text, "added\n");
    assert!(!files[1].is_delete);
    assert_eq!(files[1].line, Some(0));

    assert_eq!(files[2].path, PathBuf::from("src/delete.txt"));
    assert_eq!(files[2].old_text, "removed\n");
    assert_eq!(files[2].new_text, "");
    assert!(files[2].is_delete);
    assert_eq!(files[2].line, Some(0));
}

#[test]
fn parse_turn_unified_diff_files_normalizes_quoted_paths() {
    let diff = "\
diff --git \"a/src/space file.txt\" \"b/src/space file.txt\"
--- \"a/src/space file.txt\"
+++ \"b/src/space file.txt\"
@@ -1 +1 @@
-before
+after
";

    let files = parse_turn_unified_diff_files(diff);
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, PathBuf::from("src/space file.txt"));
    assert_eq!(files[0].old_text, "before\n");
    assert_eq!(files[0].new_text, "after\n");
    assert!(!files[0].is_delete);
    assert_eq!(files[0].line, Some(0));
}

#[test]
fn parse_turn_unified_diff_files_preserves_missing_final_newline() {
    let diff = "\
diff --git a/src/no-newline.txt b/src/no-newline.txt
--- a/src/no-newline.txt
+++ b/src/no-newline.txt
@@ -1 +1 @@
-before
\\ No newline at end of file
+after
\\ No newline at end of file
";

    let files = parse_turn_unified_diff_files(diff);
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, PathBuf::from("src/no-newline.txt"));
    assert_eq!(files[0].old_text, "before");
    assert_eq!(files[0].new_text, "after");
}

#[test]
fn parse_turn_unified_diff_files_uses_rename_target_path() {
    let diff = "\
diff --git a/src/old_name.txt b/src/new_name.txt
similarity index 50%
rename from src/old_name.txt
rename to src/new_name.txt
--- a/src/old_name.txt
+++ b/src/new_name.txt
@@ -1 +1 @@
-before
+after
";

    let files = parse_turn_unified_diff_files(diff);
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, PathBuf::from("src/new_name.txt"));
    assert_eq!(files[0].old_text, "before\n");
    assert_eq!(files[0].new_text, "after\n");
    assert!(!files[0].is_delete);
}

#[test]
fn parse_turn_unified_diff_files_ignores_sections_without_hunks() {
    let diff = "\
diff --git a/src/example.txt b/src/example.txt
--- a/src/example.txt
+++ b/src/example.txt
";

    let files = parse_turn_unified_diff_files(diff);
    assert!(files.is_empty());
}

#[test]
fn replay_diff_for_update_uses_old_and_new_text() {
    let change = codex_app_server_protocol::FileUpdateChange {
        path: "README.md".to_string(),
        kind: PatchChangeKind::Update { move_path: None },
        diff: "\
--- a/README.md
+++ b/README.md
@@ -1 +1 @@
-hello
+world
"
        .to_string(),
    };

    let diff = file_change_to_replay_diff(Path::new("/tmp/workspace"), change);
    assert_eq!(diff.path, PathBuf::from("/tmp/workspace/README.md"));
    assert_eq!(diff.old_text.as_deref(), Some("hello\n"));
    assert_eq!(diff.new_text, "world\n");
}

#[test]
fn replay_diff_for_add_uses_unified_hunk_when_available() {
    let change = codex_app_server_protocol::FileUpdateChange {
        path: "notes.md".to_string(),
        kind: PatchChangeKind::Add,
        diff: "\
--- /dev/null
+++ b/notes.md
@@ -0,0 +1,2 @@
+line one
+line two
"
        .to_string(),
    };

    let diff = file_change_to_replay_diff(Path::new("/tmp/workspace"), change);
    assert_eq!(diff.path, PathBuf::from("/tmp/workspace/notes.md"));
    assert_eq!(diff.old_text.as_deref(), None);
    assert_eq!(diff.new_text, "line one\nline two\n");
}

#[test]
fn replay_diff_for_delete_uses_unified_hunk_when_available() {
    let change = codex_app_server_protocol::FileUpdateChange {
        path: "notes.md".to_string(),
        kind: PatchChangeKind::Delete,
        diff: "\
--- a/notes.md
+++ /dev/null
@@ -1,2 +0,0 @@
-line one
-line two
"
        .to_string(),
    };

    let diff = file_change_to_replay_diff(Path::new("/tmp/workspace"), change);
    assert_eq!(diff.path, PathBuf::from("/tmp/workspace/notes.md"));
    assert_eq!(diff.old_text.as_deref(), Some("line one\nline two\n"));
    assert_eq!(diff.new_text, "");
}

#[test]
fn file_change_tool_location_uses_move_target_and_hunk_line() {
    let change = codex_app_server_protocol::FileUpdateChange {
        path: "src/old.rs".to_string(),
        kind: PatchChangeKind::Update {
            move_path: Some(PathBuf::from("src/new.rs")),
        },
        diff: "\
--- a/src/old.rs
+++ b/src/new.rs
@@ -3,2 +8,3 @@
-old
+new
 keep
"
        .to_string(),
    };

    let location = file_change_tool_location(Path::new("/tmp/workspace"), &change);
    assert_eq!(location.path, PathBuf::from("/tmp/workspace/src/new.rs"));
    assert_eq!(location.line, Some(7));
}

#[test]
fn file_change_tool_location_defaults_to_first_line_for_non_unified_add() {
    let change = codex_app_server_protocol::FileUpdateChange {
        path: "notes.txt".to_string(),
        kind: PatchChangeKind::Add,
        diff: "hello\nworld\n".to_string(),
    };

    let location = file_change_tool_location(Path::new("/tmp/workspace"), &change);
    assert_eq!(location.path, PathBuf::from("/tmp/workspace/notes.txt"));
    assert_eq!(location.line, Some(0));
}
