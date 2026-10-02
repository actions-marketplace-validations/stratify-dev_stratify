use std::path::Path;

#[test]
fn stats_flag_prints_one_json_line_on_stderr() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/sample-ruby");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_stratify"))
        .args([
            "check",
            dir.to_str().unwrap(),
            "--format",
            "json",
            "--stats",
        ])
        .output()
        .expect("run stratify binary");
    assert!(output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    let line = stderr
        .lines()
        .find_map(|l| l.strip_prefix("stratify-stats "))
        .unwrap_or_else(|| panic!("no stats line: {stderr}"));
    let v: serde_json::Value = serde_json::from_str(line).unwrap();
    let expected_lines: u64 = ["app.rb", "unused.rb"]
        .iter()
        .map(|f| {
            std::fs::read_to_string(dir.join(f))
                .unwrap()
                .lines()
                .count() as u64
        })
        .sum();
    assert_eq!(v["files_scanned"], 2);
    assert_eq!(v["lines_scanned"], expected_lines);
    // stdout stays a clean JSON report
    let _: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
}
