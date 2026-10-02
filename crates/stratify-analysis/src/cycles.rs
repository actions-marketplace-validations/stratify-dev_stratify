use crate::imports::ImportGraph;
use std::collections::{BTreeSet, HashMap};
use stratify_core::ir::Span;
use stratify_core::{Confidence, Finding, IrGraph, Severity};

/// Detect circular dependencies in the cross-file import graph. An `Imports`
/// edge (File -> Dependency) resolves to a file-to-file edge when the
/// Dependency's name (import key) equals some File/Class/Module symbol's fqn
/// (export key). Cycles are found by DFS back-edge detection.
pub fn analyze(graph: &IrGraph) -> Vec<Finding> {
    let adj = crate::imports::fqn_import_graph(graph);
    let spans = crate::imports::fqn_spans(graph);

    // DFS back-edge detection. Colors: 0 = white, 1 = gray (on stack), 2 = black.
    let mut color: HashMap<String, u8> = HashMap::new();
    let mut path: Vec<String> = Vec::new();
    let mut reported: BTreeSet<Vec<String>> = BTreeSet::new();
    let nodes: Vec<String> = adj.keys().cloned().collect();
    for start in &nodes {
        if color.get(start).copied().unwrap_or(0) == 0 {
            dfs(start, &adj, &mut color, &mut path, &mut reported);
        }
    }

    // Emit one finding per distinct cycle (canonicalized to its lexicographically
    // smallest rotation so A->B->A and B->A->B are the same cycle).
    let mut findings = Vec::new();
    for cycle in reported {
        let files: Vec<String> = cycle
            .iter()
            .map(|fqn| {
                spans
                    .get(fqn)
                    .map(|s| s.file.clone())
                    .unwrap_or_else(|| fqn.clone())
            })
            .collect();
        let fallback = Span {
            file: files[0].clone(),
            start_byte: 0,
            end_byte: 0,
            start_line: 1,
        };
        let span = spans.get(&cycle[0]).cloned().unwrap_or(fallback.clone());
        // One unconfirmed (re-export) edge makes the whole cycle unconfirmed.
        let unconfirmed = (0..cycle.len()).any(|i| {
            let next = &cycle[(i + 1) % cycle.len()];
            adj.get(&cycle[i]).and_then(|m| m.get(next)) == Some(&Confidence::Likely)
        });
        let (severity, confidence) = if unconfirmed {
            (Severity::Info, Confidence::Likely)
        } else {
            (Severity::Warning, Confidence::Certain)
        };
        let mut finding = Finding::new(
            "cycle",
            severity,
            format!("circular dependency: {}", files.join(" -> ")),
            span,
            confidence,
        );
        for (i, fqn) in cycle.iter().enumerate() {
            let step_span = spans.get(fqn).cloned().unwrap_or(fallback.clone());
            finding = finding.with_path_step(
                step_span,
                "cycle_member",
                format!("imports {}", files[(i + 1) % files.len()]),
            );
        }
        finding = finding
            .with_reason("each file in the cycle imports the next, and the last imports the first");
        if unconfirmed {
            finding = finding.with_reason(
                "one or more imports in this cycle land on a re-exporting module, not the file that defines the item",
            );
        }
        findings.push(finding);
    }
    findings
}

fn dfs(
    node: &str,
    adj: &ImportGraph,
    color: &mut HashMap<String, u8>,
    path: &mut Vec<String>,
    reported: &mut BTreeSet<Vec<String>>,
) {
    color.insert(node.to_string(), 1);
    path.push(node.to_string());
    if let Some(neighbors) = adj.get(node) {
        for next in neighbors.keys() {
            match color.get(next).copied().unwrap_or(0) {
                0 => dfs(next, adj, color, path, reported),
                1 => {
                    // Back edge: cycle is path[pos..] where path[pos] == next.
                    if let Some(pos) = path.iter().position(|n| n == next) {
                        let cycle = canonical_cycle(&path[pos..]);
                        reported.insert(cycle);
                    }
                }
                _ => {}
            }
        }
    }
    path.pop();
    color.insert(node.to_string(), 2);
}

/// Rotate a cycle so it starts at its lexicographically smallest node, so the
/// same cycle discovered from different entry points dedupes.
fn canonical_cycle(nodes: &[String]) -> Vec<String> {
    let min_pos = nodes
        .iter()
        .enumerate()
        .min_by_key(|(_, n)| (*n).clone())
        .map(|(i, _)| i)
        .unwrap_or(0);
    let mut out: Vec<String> = nodes[min_pos..].to_vec();
    out.extend_from_slice(&nodes[..min_pos]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{add_import, add_import_at, file_sym};

    #[test]
    fn detects_two_file_cycle() {
        let mut g = IrGraph::new();
        let a = file_sym(&mut g, "a.rb"); // exports key "a.rb"
        let b = file_sym(&mut g, "b.rb"); // exports key "b.rb"
        add_import(&mut g, a, "b.rb"); // a imports b
        add_import(&mut g, b, "a.rb"); // b imports a
        let findings = analyze(&g);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, "cycle");
        assert!(findings[0].message.contains("a.rb"));
        assert!(findings[0].message.contains("b.rb"));
    }

    #[test]
    fn cycle_records_members_as_an_ordered_path() {
        let mut g = IrGraph::new();
        let a = file_sym(&mut g, "a.rb");
        let b = file_sym(&mut g, "b.rb");
        let c = file_sym(&mut g, "c.rb");
        add_import(&mut g, a, "b.rb");
        add_import(&mut g, b, "c.rb");
        add_import(&mut g, c, "a.rb");
        let f = &analyze(&g)[0];
        let files: Vec<&str> = f.path.iter().map(|r| r.span.file.as_str()).collect();
        assert_eq!(files, vec!["a.rb", "b.rb", "c.rb"]);
        assert!(f.path.iter().all(|r| r.role == "cycle_member"));
        assert_eq!(f.path[2].message, "imports a.rb");
        assert!(f.related.is_empty());
        assert!(!f.reasons.is_empty());
    }

    #[test]
    fn no_cycle_for_dag() {
        let mut g = IrGraph::new();
        let a = file_sym(&mut g, "a.rb");
        let _b = file_sym(&mut g, "b.rb");
        add_import(&mut g, a, "b.rb"); // a -> b only
        assert!(analyze(&g).is_empty());
    }

    #[test]
    fn unresolved_import_is_ignored() {
        let mut g = IrGraph::new();
        let a = file_sym(&mut g, "a.rb");
        add_import(&mut g, a, "nonexistent.rb"); // no matching export
        assert!(analyze(&g).is_empty());
    }

    #[test]
    fn detects_three_file_cycle_once() {
        let mut g = IrGraph::new();
        let a = file_sym(&mut g, "a.rb");
        let b = file_sym(&mut g, "b.rb");
        let c = file_sym(&mut g, "c.rb");
        add_import(&mut g, a, "b.rb");
        add_import(&mut g, b, "c.rb");
        add_import(&mut g, c, "a.rb");
        let findings = analyze(&g);
        assert_eq!(findings.len(), 1, "one cycle, not one per entry point");
        assert!(findings[0].message.contains("a.rb -> b.rb -> c.rb"));
    }

    #[test]
    fn cycle_with_a_likely_edge_reports_info_likely() {
        let mut g = IrGraph::new();
        let a = file_sym(&mut g, "a.rs");
        let b = file_sym(&mut g, "b.rs");
        add_import_at(&mut g, a, "b.rs", "a.rs", 1, Confidence::Certain);
        add_import_at(&mut g, b, "a.rs", "b.rs", 1, Confidence::Likely);
        let findings = analyze(&g);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Info);
        assert_eq!(findings[0].confidence, Confidence::Likely);
        assert!(findings[0].message.contains("a.rs -> b.rs"));
        assert!(findings[0]
            .reasons
            .iter()
            .any(|r| r.contains("re-exporting module")));
    }

    #[test]
    fn cycle_with_only_certain_edges_stays_warning() {
        let mut g = IrGraph::new();
        let a = file_sym(&mut g, "a.rs");
        let b = file_sym(&mut g, "b.rs");
        add_import_at(&mut g, a, "b.rs", "a.rs", 1, Confidence::Certain);
        add_import_at(&mut g, b, "a.rs", "b.rs", 1, Confidence::Certain);
        let findings = analyze(&g);
        assert_eq!(findings[0].severity, Severity::Warning);
        assert_eq!(findings[0].confidence, Confidence::Certain);
        assert!(findings[0]
            .reasons
            .iter()
            .all(|r| !r.contains("re-exporting module")));
    }
}
