use std::collections::HashMap;
use stratify_core::ir::Suppression;
use stratify_core::{Finding, IrGraph};

/// True if `s` names `f`'s rule and sits on `f`'s line or ends on the line above.
fn covers(s: &Suppression, f: &Finding) -> bool {
    let line = f.span.start_line;
    s.rules.iter().any(|r| r == &f.rule)
        && ((s.start_line <= line && line <= s.end_line) || s.end_line + 1 == line)
}

/// Remove findings covered by a comment suppression recorded in the IR.
/// Returns the kept findings and how many were removed.
pub fn apply(graph: &IrGraph, findings: Vec<Finding>) -> (Vec<Finding>, usize) {
    let mut by_file: HashMap<&str, Vec<&Suppression>> = HashMap::new();
    for s in graph.suppressions() {
        by_file.entry(s.file.as_str()).or_default().push(s);
    }
    let mut kept = Vec::with_capacity(findings.len());
    let mut removed = 0;
    for f in findings {
        let hit = by_file
            .get(f.span.file.as_str())
            .is_some_and(|list| list.iter().any(|s| covers(s, &f)));
        if hit {
            removed += 1;
        } else {
            kept.push(f);
        }
    }
    (kept, removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use stratify_core::ir::{Span, Suppression};
    use stratify_core::{Confidence, Finding, IrGraph, Severity};

    fn f(rule: &str, file: &str, line: usize) -> Finding {
        Finding::new(
            rule,
            Severity::Warning,
            "m",
            Span {
                file: file.into(),
                start_byte: 0,
                end_byte: 1,
                start_line: line,
            },
            Confidence::Certain,
        )
    }

    fn sup(start: usize, end: usize, rules: &[&str]) -> Suppression {
        Suppression {
            file: "a.rs".into(),
            start_line: start,
            end_line: end,
            rules: rules.iter().map(|r| r.to_string()).collect(),
        }
    }

    fn graph() -> IrGraph {
        let mut g = IrGraph::new();
        // line comment on line 2, naming dead_code
        g.add_suppression(sup(2, 2, &["dead_code"]));
        // trailing comment on line 10, naming two rules
        g.add_suppression(sup(10, 10, &["complexity", "dead_code"]));
        // block comment on lines 20-22
        g.add_suppression(sup(20, 22, &["cycle"]));
        g
    }

    #[test]
    fn preceding_line_comment_suppresses() {
        let (kept, removed) = apply(&graph(), vec![f("dead_code", "a.rs", 3)]);
        assert!(kept.is_empty());
        assert_eq!(removed, 1);
    }

    #[test]
    fn same_line_comment_suppresses_each_named_rule() {
        let (kept, _) = apply(
            &graph(),
            vec![f("dead_code", "a.rs", 10), f("complexity", "a.rs", 10)],
        );
        assert!(kept.is_empty());
    }

    #[test]
    fn block_comment_ending_above_suppresses() {
        let (kept, _) = apply(&graph(), vec![f("cycle", "a.rs", 23)]);
        assert!(kept.is_empty());
    }

    #[test]
    fn other_rule_other_file_and_distant_lines_stay() {
        let findings = vec![
            f("duplication", "a.rs", 3), // rule not named
            f("dead_code", "b.rs", 3),   // other file
            f("dead_code", "a.rs", 4),   // two lines below the comment
            f("dead_code", "a.rs", 1),   // above the comment
        ];
        let (kept, removed) = apply(&graph(), findings);
        assert_eq!(kept.len(), 4);
        assert_eq!(removed, 0);
    }
}
