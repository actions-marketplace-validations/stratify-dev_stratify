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
fn rust_module_cycle_is_detected() {
    let stdout = scan("tests/sample-rustcycle");
    let cycles = stdout.matches("\"rule\": \"cycle\"").count();
    assert_eq!(cycles, 1, "stdout: {stdout}");
    assert!(stdout.contains("src/a.rs -> src/b.rs"), "stdout: {stdout}");
}

#[test]
fn rust_layer_boundary_is_enforced() {
    let stdout = scan("tests/sample-rustboundary");
    let hits = stdout.matches("\"rule\": \"boundary\"").count();
    assert_eq!(hits, 1, "stdout: {stdout}");
    assert!(
        stdout.contains("layer `domain` must not import `api`"),
        "stdout: {stdout}"
    );
    // `use super::*` in the inline test module must not add a cycle.
    assert_eq!(
        stdout.matches("\"rule\": \"cycle\"").count(),
        0,
        "stdout: {stdout}"
    );
}
