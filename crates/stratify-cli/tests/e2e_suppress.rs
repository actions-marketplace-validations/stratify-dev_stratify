use std::path::Path;

#[test]
fn only_comment_markers_suppress() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/sample-suppress");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_stratify"))
        .arg("check")
        .arg(&dir)
        .arg("--format")
        .arg("json")
        .output()
        .expect("run stratify binary");
    let stdout = String::from_utf8(output.stdout).unwrap();
    // A comment marker on the line above hides the finding.
    assert!(!stdout.contains("kept_for_reflection"), "stdout: {stdout}");
    // The same text inside a string literal on the line above does not.
    assert!(stdout.contains("really_unused"), "stdout: {stdout}");
}
