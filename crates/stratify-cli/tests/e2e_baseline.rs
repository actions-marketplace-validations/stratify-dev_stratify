use std::process::Command;

fn stratify() -> Command {
    Command::new(env!("CARGO_BIN_EXE_stratify"))
}

fn temp_repo(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("stratify-e2e-baseline-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("app.rb"),
        "def main\nend\n\ndef old_dead\nend\n\nmain\n",
    )
    .unwrap();
    dir
}

#[test]
fn baseline_hides_old_findings_and_gates_new_ones() {
    let repo = temp_repo("gate");
    let base = repo.join("baseline.json");

    let w = stratify()
        .arg("check")
        .arg(&repo)
        .arg("--write-baseline")
        .arg(&base)
        .output()
        .unwrap();
    assert!(w.status.success(), "{w:?}");

    // Same code, shifted down two lines: still baselined, gate passes.
    std::fs::write(
        repo.join("app.rb"),
        "# a\n# b\ndef main\nend\n\ndef old_dead\nend\n\nmain\n",
    )
    .unwrap();
    let ok = stratify()
        .arg("check")
        .arg(&repo)
        .arg("--baseline")
        .arg(&base)
        .arg("--fail-on")
        .arg("warning")
        .output()
        .unwrap();
    assert!(
        ok.status.success(),
        "{}",
        String::from_utf8_lossy(&ok.stdout)
    );

    // A new dead function fails the gate and is the only finding shown.
    std::fs::write(
        repo.join("app.rb"),
        "def main\nend\n\ndef old_dead\nend\n\ndef new_dead\nend\n\nmain\n",
    )
    .unwrap();
    let bad = stratify()
        .arg("check")
        .arg(&repo)
        .arg("--baseline")
        .arg(&base)
        .arg("--fail-on")
        .arg("warning")
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&bad.stdout);
    assert!(!bad.status.success());
    assert!(out.contains("new_dead"), "{out}");
    assert!(!out.contains("old_dead"), "{out}");
}

#[test]
fn missing_or_invalid_baseline_exits_2() {
    let repo = temp_repo("bad");
    let missing = stratify()
        .arg("check")
        .arg(&repo)
        .arg("--baseline")
        .arg(repo.join("nope.json"))
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("baseline"));

    std::fs::write(repo.join("bad.json"), "{").unwrap();
    let invalid = stratify()
        .arg("check")
        .arg(&repo)
        .arg("--baseline")
        .arg(repo.join("bad.json"))
        .output()
        .unwrap();
    assert_eq!(invalid.status.code(), Some(2));
}
