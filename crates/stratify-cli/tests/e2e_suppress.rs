use std::path::Path;

fn scan(fixture: &str) -> String {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(fixture);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_stratify"))
        .arg("check")
        .arg(&dir)
        .arg("--format")
        .arg("json")
        .output()
        .expect("run stratify binary");
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn only_comment_markers_suppress() {
    let stdout = scan("tests/sample-suppress");
    // A comment marker on the line above hides the finding.
    assert!(!stdout.contains("kept_for_reflection"), "stdout: {stdout}");
    // The same text inside a string literal on the line above does not.
    assert!(stdout.contains("really_unused"), "stdout: {stdout}");
}

#[test]
fn marker_above_a_decorator_suppresses_the_function() {
    let stdout = scan("tests/sample-suppress");
    assert!(
        !stdout.contains("decorated_unused_handler"),
        "stdout: {stdout}"
    );
}
