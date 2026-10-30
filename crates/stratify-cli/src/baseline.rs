use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use stratify_core::{Finding, Report};

const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct BaselineFile {
    version: u32,
    fingerprints: Vec<String>,
}

/// A line-independent identity for a finding: rule, file, and message with
/// every digit run replaced by `#`.
pub fn fingerprint(f: &Finding) -> String {
    let mut msg = String::with_capacity(f.message.len());
    let mut in_digits = false;
    for ch in f.message.chars() {
        if ch.is_ascii_digit() {
            if !in_digits {
                msg.push('#');
            }
            in_digits = true;
        } else {
            msg.push(ch);
            in_digits = false;
        }
    }
    format!("{}\u{1f}{}\u{1f}{}", f.rule, f.span.file, msg)
}

/// Serialize every finding's fingerprint, sorted, as a baseline file.
pub fn write(report: &Report) -> String {
    let mut fingerprints: Vec<String> = report.findings.iter().map(fingerprint).collect();
    fingerprints.sort();
    let file = BaselineFile {
        version: VERSION,
        fingerprints,
    };
    let mut text = serde_json::to_string_pretty(&file).expect("baseline serializes");
    text.push('\n');
    text
}

/// Keep only findings the baseline does not cover. Returns the filtered
/// report and how many findings the baseline hid.
pub fn filter_new(report: Report, baseline_json: &str) -> Result<(Report, usize), String> {
    let file: BaselineFile =
        serde_json::from_str(baseline_json).map_err(|e| format!("invalid JSON: {e}"))?;
    if file.version != VERSION {
        return Err(format!("unsupported baseline version {}", file.version));
    }
    let mut budget: HashMap<String, usize> = HashMap::new();
    for fp in file.fingerprints {
        *budget.entry(fp).or_insert(0) += 1;
    }
    let mut hidden = 0;
    let mut kept = Vec::new();
    for f in report.findings {
        match budget.get_mut(&fingerprint(&f)) {
            Some(n) if *n > 0 => {
                *n -= 1;
                hidden += 1;
            }
            _ => kept.push(f),
        }
    }
    Ok((Report::new(kept), hidden))
}

#[cfg(test)]
mod tests {
    use super::*;
    use stratify_core::ir::Span;
    use stratify_core::{Confidence, Finding, Report, Severity};

    fn f(rule: &str, file: &str, line: usize, msg: &str) -> Finding {
        Finding::new(
            rule,
            Severity::Warning,
            msg,
            Span {
                file: file.into(),
                start_byte: 0,
                end_byte: 1,
                start_line: line,
            },
            Confidence::Certain,
        )
    }

    #[test]
    fn fingerprint_ignores_line_and_digits() {
        let a = f(
            "duplication",
            "a.rs",
            3,
            "duplicated code block (>= 100 tokens) also at b.rs:10",
        );
        let b = f(
            "duplication",
            "a.rs",
            13,
            "duplicated code block (>= 100 tokens) also at b.rs:20",
        );
        assert_eq!(fingerprint(&a), fingerprint(&b));
    }

    #[test]
    fn fingerprint_separates_rule_and_file() {
        let a = f("dead_code", "a.rs", 1, "unused function `x`");
        assert_ne!(
            fingerprint(&a),
            fingerprint(&f("dead_code", "b.rs", 1, "unused function `x`"))
        );
        assert_ne!(
            fingerprint(&a),
            fingerprint(&f("complexity", "a.rs", 1, "unused function `x`"))
        );
    }

    #[test]
    fn moved_finding_stays_baselined() {
        let base = write(&Report::new(vec![f(
            "dead_code",
            "a.rs",
            5,
            "unused function `x`",
        )]));
        let now = Report::new(vec![f("dead_code", "a.rs", 15, "unused function `x`")]);
        let (out, hidden) = filter_new(now, &base).unwrap();
        assert!(out.findings.is_empty());
        assert_eq!(hidden, 1);
    }

    #[test]
    fn extra_copy_of_baselined_fingerprint_is_new() {
        let one = f("dead_code", "a.rs", 1, "unused function `x`");
        let base = write(&Report::new(vec![one.clone()]));
        let (out, hidden) = filter_new(Report::new(vec![one.clone(), one]), &base).unwrap();
        assert_eq!(out.findings.len(), 1);
        assert_eq!(hidden, 1);
    }

    #[test]
    fn new_finding_is_reported() {
        let base = write(&Report::new(vec![]));
        let (out, hidden) = filter_new(
            Report::new(vec![f("cycle", "a.rs", 1, "circular dependency: a -> b")]),
            &base,
        )
        .unwrap();
        assert_eq!(out.findings.len(), 1);
        assert_eq!(hidden, 0);
    }

    #[test]
    fn invalid_baseline_is_an_error() {
        assert!(filter_new(Report::new(vec![]), "not json").is_err());
        assert!(filter_new(Report::new(vec![]), r#"{"version": 2, "fingerprints": []}"#).is_err());
    }
}
