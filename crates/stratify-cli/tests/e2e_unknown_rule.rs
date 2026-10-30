#[test]
fn unknown_rule_id_warns_and_suppresses_nothing() {
    let dir = std::env::temp_dir().join(format!("stratify-unknown-rule-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("a.rb"),
        "# stratify-ignore: deadcode\ndef unused_helper\nend\n",
    )
    .unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_stratify"))
        .arg("check")
        .arg(&dir)
        .arg("--format")
        .arg("json")
        .output()
        .expect("run stratify binary");
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        stderr.contains("unknown rule id `deadcode` in stratify-ignore marker"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("a.rb:1:"), "stderr: {stderr}");
    assert!(stdout.contains("unused_helper"), "stdout: {stdout}");
}
