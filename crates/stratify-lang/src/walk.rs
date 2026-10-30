//! Shared tree-walking helpers for the language adapters.
//!
//! Every `stratify-lang-*` adapter repeats the same scaffolding: a `span`
//! builder, a `text` accessor, a leaf-walk that normalizes tokens for
//! duplication detection, a cyclomatic decision counter, and an enclosing
//! ancestor walk. This module captures that common structure once. The
//! per-language specifics (which node kinds count) are passed in as parameters.

use stratify_core::ir::{Span, Suppression};
use stratify_core::{IrGraph, Token};
use tree_sitter::Node;

/// The source text a node spans. Empty string if the bytes are not valid UTF-8.
pub fn node_text<'a>(node: Node, src: &'a str) -> &'a str {
    node.utf8_text(src.as_bytes()).unwrap_or("")
}

/// A [`Span`] for `node` within `file`. `start_line` is 1-based.
pub fn span(node: Node, file: &str) -> Span {
    Span {
        file: file.to_string(),
        start_byte: node.start_byte(),
        end_byte: node.end_byte(),
        start_line: node.start_position().row + 1,
    }
}

/// Nearest ancestor (or self) whose kind is in `kinds`.
pub fn enclosing<'a>(node: Node<'a>, kinds: &[&str]) -> Option<Node<'a>> {
    let mut cur = Some(node);
    while let Some(n) = cur {
        if kinds.contains(&n.kind()) {
            return Some(n);
        }
        cur = n.parent();
    }
    None
}

/// How a language maps leaf tokens to normalized forms for duplication.
pub struct NormalizeRules<'a> {
    /// Node kinds normalized to "ID".
    pub identifier_kinds: &'a [&'a str],
    /// Node kinds normalized to "NUM".
    pub number_kinds: &'a [&'a str],
    /// Node kinds normalized to "STR".
    pub string_kinds: &'a [&'a str],
    /// Node kinds treated as ONE atomic leaf and not descended into
    /// (e.g. Rust `string_literal`, which is not itself a leaf in the grammar:
    /// it decomposes into quote delimiters plus a `string_content` child).
    /// These also normalize to "STR".
    pub atomic_string_kinds: &'a [&'a str],
}

impl NormalizeRules<'_> {
    /// Normalize one leaf `node` to its token class. Identifier/number/string
    /// kinds collapse to ID/NUM/STR; everything else keeps its literal `text`
    /// (keywords, operators, punctuation).
    fn normalize(&self, kind: &str, text: &str) -> String {
        if self.identifier_kinds.contains(&kind) {
            "ID".to_string()
        } else if self.number_kinds.contains(&kind) {
            "NUM".to_string()
        } else if self.string_kinds.contains(&kind) || self.atomic_string_kinds.contains(&kind) {
            "STR".to_string()
        } else {
            text.to_string()
        }
    }
}

/// Walk `root`'s leaves in document order and emit normalized tokens into the
/// graph for `file`. Identifier/number/string kinds collapse to ID/NUM/STR;
/// `atomic_string_kinds` are emitted as a single "STR" without descending;
/// everything else emits its literal text (keywords, operators, punctuation).
///
/// Both named and unnamed leaves are emitted (operators and punctuation are
/// unnamed). Zero-width and whitespace-only leaves are skipped. Tokens are
/// emitted per-file so duplication spans never straddle files.
pub fn tokenize(graph: &mut IrGraph, root: Node, src: &str, file: &str, rules: &NormalizeRules) {
    let mut leaves: Vec<Node> = Vec::new();
    collect_leaves(root, rules, &mut leaves);
    for leaf in leaves {
        if leaf.start_byte() >= leaf.end_byte() {
            continue;
        }
        let t = node_text(leaf, src);
        if t.trim().is_empty() {
            continue;
        }
        let norm = rules.normalize(leaf.kind(), t);
        graph.add_token(Token {
            file: file.to_string(),
            start_byte: leaf.start_byte(),
            end_byte: leaf.end_byte(),
            start_line: leaf.start_position().row + 1,
            norm,
        });
    }
}

/// Collect leaves in document order. An `atomic_string_kinds` node is treated
/// as a single leaf and not descended into; otherwise a node with no children
/// is a leaf, and any other node recurses over its children.
fn collect_leaves<'a>(node: Node<'a>, rules: &NormalizeRules, out: &mut Vec<Node<'a>>) {
    if rules.atomic_string_kinds.contains(&node.kind()) {
        out.push(node);
        return;
    }
    if node.child_count() == 0 {
        out.push(node);
        return;
    }
    let mut c = node.walk();
    for child in node.children(&mut c) {
        collect_leaves(child, rules, out);
    }
}

/// How a language counts cyclomatic decision points.
pub struct ComplexityRules<'a> {
    /// Named node kinds that each add one decision
    /// (if/while/for/case arms/catch/ternary/etc.).
    pub decision_kinds: &'a [&'a str],
    /// Unnamed operator leaf texts that each add one
    /// (e.g. "&&", "||", "and", "or", "?", "??").
    pub operator_texts: &'a [&'a str],
}

/// Cyclomatic complexity of `body` = 1 + decision points found in its subtree.
///
/// For a NAMED node whose kind is in `decision_kinds`, +1. For an UNNAMED leaf
/// whose text is in `operator_texts`, +1. Splitting on `is_named()` avoids
/// double-counting keyword tokens that share a kind string with statement nodes.
pub fn cyclomatic(body: Node, src: &str, rules: &ComplexityRules) -> u32 {
    let mut count = 0u32;
    let mut stack = vec![body];
    while let Some(n) = stack.pop() {
        if n.is_named() {
            if rules.decision_kinds.contains(&n.kind()) {
                count += 1;
            }
        } else if n.child_count() == 0 && rules.operator_texts.contains(&node_text(n, src)) {
            count += 1;
        }
        let mut c = n.walk();
        for child in n.children(&mut c) {
            stack.push(child);
        }
    }
    1 + count
}

const SUPPRESSION_MARKER: &str = "stratify-ignore:";

/// Rule ids named by a suppression marker in one comment's text. The marker is
/// `stratify-ignore` and a colon, then a comma-separated rule list.
/// Each comma-separated item contributes its first word when the word is a
/// rule-shaped id. No marker, an empty list, or `*` gives an empty Vec.
pub fn parse_suppression_marker(comment: &str) -> Vec<String> {
    let Some(pos) = comment.find(SUPPRESSION_MARKER) else {
        return Vec::new();
    };
    comment[pos + SUPPRESSION_MARKER.len()..]
        .split(',')
        .filter_map(|item| item.split_whitespace().next())
        .map(|word| word.trim_end_matches("*/"))
        .filter(|word| is_rule_id(word))
        .map(str::to_string)
        .collect()
}

fn is_rule_id(word: &str) -> bool {
    let mut chars = word.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Record a Suppression for each comment node (matched by grammar kind)
/// whose text carries a marker, in source order. String literals are never
/// comment nodes, so a marker inside a string is ignored.
///
/// A marker above an attribute or decorator must still reach the item below,
/// whose own span starts after that prelude. So the recorded range extends
/// through the contiguous siblings that follow the comment:
/// - `prelude_kinds`: sibling kinds that belong to the item (Rust
///   `attribute_item`). The range ends at the last such sibling.
/// - `wrapper_kinds`: (wrapper kind, field name of the wrapped definition)
///   pairs (Python `decorated_definition`, `definition`). The range ends on the
///   line before the wrapped definition starts.
pub fn collect_suppressions(
    graph: &mut IrGraph,
    root: Node,
    src: &str,
    file: &str,
    comment_kinds: &[&str],
    prelude_kinds: &[&str],
    wrapper_kinds: &[(&str, &str)],
) {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if comment_kinds.contains(&node.kind()) {
            let rules = parse_suppression_marker(node_text(node, src));
            if !rules.is_empty() {
                let start = node.start_position();
                let end = node.end_position();
                let mut end_row = normalized_end_row(start, end);
                let mut sibling = node.next_named_sibling();
                while let Some(sib) = sibling {
                    // Only a prelude that touches the marker belongs to it.
                    if sib.start_position().row > end_row + 1 {
                        break;
                    }
                    if prelude_kinds.contains(&sib.kind()) {
                        end_row = end_row
                            .max(normalized_end_row(sib.start_position(), sib.end_position()));
                    } else if let Some((_, field)) =
                        wrapper_kinds.iter().find(|(kind, _)| *kind == sib.kind())
                    {
                        if let Some(def) = sib.child_by_field_name(field) {
                            end_row = end_row.max(def.start_position().row.saturating_sub(1));
                        }
                        break;
                    } else {
                        break;
                    }
                    sibling = sib.next_named_sibling();
                }
                graph.add_suppression(Suppression {
                    file: file.to_string(),
                    start_line: start.row + 1,
                    end_line: end_row + 1,
                    rules,
                });
            }
            continue;
        }
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();
        for child in children.into_iter().rev() {
            stack.push(child);
        }
    }
}

/// Last row a node occupies. A node that swallows its newline ends at column 0
/// of the next row. It still belongs to the row before.
fn normalized_end_row(start: tree_sitter::Point, end: tree_sitter::Point) -> usize {
    if end.column == 0 && end.row > start.row {
        end.row - 1
    } else {
        end.row
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::Parser;

    fn parse_go(src: &str) -> tree_sitter::Tree {
        let mut p = Parser::new();
        p.set_language(&tree_sitter_go::LANGUAGE.into())
            .expect("load go grammar");
        p.parse(src, None).expect("parse go")
    }

    #[test]
    fn parses_marker_rule_lists() {
        assert_eq!(
            parse_suppression_marker("// stratify-ignore: dead_code"),
            vec!["dead_code"]
        );
        assert_eq!(
            parse_suppression_marker("# stratify-ignore: dead_code, complexity -- router calls it"),
            vec!["dead_code", "complexity"]
        );
        assert_eq!(
            parse_suppression_marker("/* stratify-ignore: cycle */"),
            vec!["cycle"]
        );
        assert_eq!(
            parse_suppression_marker("/*stratify-ignore: cycle*/"),
            vec!["cycle"]
        );
        assert!(parse_suppression_marker("// stratify-ignore:").is_empty());
        assert!(parse_suppression_marker("// stratify-ignore: *").is_empty());
        assert!(parse_suppression_marker("// nothing here").is_empty());
    }

    #[test]
    fn collects_markers_only_from_comment_nodes_in_source_order() {
        let src = "package a\n// stratify-ignore: dead_code\nfunc a() {}\nvar s = \"stratify-ignore: complexity\"\n/* stratify-ignore: duplication\n */\nfunc c() {}\n";
        let tree = parse_go(src);
        let mut g = IrGraph::new();
        collect_suppressions(
            &mut g,
            tree.root_node(),
            src,
            "a.go",
            &["comment"],
            &[],
            &[],
        );
        let got: Vec<(usize, usize, Vec<String>)> = g
            .suppressions()
            .iter()
            .map(|s| (s.start_line, s.end_line, s.rules.clone()))
            .collect();
        assert_eq!(
            got,
            vec![
                (2, 2, vec!["dead_code".to_string()]),
                (5, 6, vec!["duplication".to_string()])
            ]
        );
    }

    fn go_normalize_rules() -> NormalizeRules<'static> {
        NormalizeRules {
            identifier_kinds: &[
                "identifier",
                "field_identifier",
                "type_identifier",
                "package_identifier",
            ],
            number_kinds: &["int_literal", "float_literal", "imaginary_literal"],
            string_kinds: &["rune_literal"],
            // In tree-sitter-go 0.23 string literals decompose into quote
            // delimiters plus a content child, so treat them as atomic.
            atomic_string_kinds: &["interpreted_string_literal", "raw_string_literal"],
        }
    }

    fn go_complexity_rules() -> ComplexityRules<'static> {
        ComplexityRules {
            decision_kinds: &[
                "if_statement",
                "for_statement",
                "expression_case",
                "type_case",
                "communication_case",
            ],
            operator_texts: &["&&", "||"],
        }
    }

    /// First descendant (or self) of `node` with the given kind, depth-first.
    fn find_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
        let mut stack = vec![node];
        while let Some(n) = stack.pop() {
            if n.kind() == kind {
                return Some(n);
            }
            let mut c = n.walk();
            for child in n.children(&mut c) {
                stack.push(child);
            }
        }
        None
    }

    #[test]
    fn node_text_returns_source_slice() {
        let src = "package main\n";
        let tree = parse_go(src);
        let pkg = find_kind(tree.root_node(), "package_identifier").unwrap();
        assert_eq!(node_text(pkg, src), "main");
    }

    #[test]
    fn span_has_one_based_line_and_byte_range() {
        let src = "package main\nvar x = 5\n";
        let tree = parse_go(src);
        let id = find_kind(tree.root_node(), "identifier").unwrap();
        let s = span(id, "a.go");
        assert_eq!(s.file, "a.go");
        assert_eq!(s.start_line, 2); // `x` is on the second line
        assert_eq!(&src[s.start_byte..s.end_byte], "x");
    }

    #[test]
    fn tokenize_normalizes_id_num_str_and_keeps_keywords() {
        let src = "package main\nvar x = 5\nvar s = \"hi\"\n";
        let tree = parse_go(src);
        let mut g = IrGraph::new();
        let rules = go_normalize_rules();
        tokenize(&mut g, tree.root_node(), src, "a.go", &rules);
        let norms: Vec<&str> = g.tokens().iter().map(|t| t.norm.as_str()).collect();
        assert!(norms.contains(&"ID"), "expected ID: {norms:?}");
        assert!(norms.contains(&"NUM"), "expected NUM: {norms:?}");
        assert!(norms.contains(&"STR"), "expected STR: {norms:?}");
        // Keywords and operators stay literal.
        assert!(norms.contains(&"package"));
        assert!(norms.contains(&"var"));
        assert!(norms.contains(&"="));
        // The string contents never leak as a separate token.
        assert!(!norms.contains(&"hi"));
    }

    #[test]
    fn tokenize_emits_in_document_order() {
        let src = "package main\n";
        let tree = parse_go(src);
        let mut g = IrGraph::new();
        let rules = go_normalize_rules();
        tokenize(&mut g, tree.root_node(), src, "a.go", &rules);
        let bytes: Vec<usize> = g.tokens().iter().map(|t| t.start_byte).collect();
        let mut sorted = bytes.clone();
        sorted.sort_unstable();
        assert_eq!(bytes, sorted, "tokens must be in document order");
    }

    #[test]
    fn cyclomatic_straight_line_is_one() {
        let src = "package main\nfunc f() {\n  return\n}\n";
        let tree = parse_go(src);
        let body = find_kind(tree.root_node(), "function_declaration").unwrap();
        assert_eq!(cyclomatic(body, src, &go_complexity_rules()), 1);
    }

    #[test]
    fn cyclomatic_counts_if_for_and_logical_and() {
        // base 1 + if + && + for = 4
        let src = "package main\nfunc m(x int) {\n  if x > 0 && x < 9 {\n  }\n  for {\n  }\n}\n";
        let tree = parse_go(src);
        let body = find_kind(tree.root_node(), "function_declaration").unwrap();
        assert_eq!(cyclomatic(body, src, &go_complexity_rules()), 4);
    }

    #[test]
    fn enclosing_finds_function_declaration() {
        let src = "package main\nfunc m() {\n  x := 1\n}\n";
        let tree = parse_go(src);
        // Start from a deep node (the `1` literal) and walk up.
        let lit = find_kind(tree.root_node(), "int_literal").unwrap();
        let func = enclosing(lit, &["function_declaration"]).expect("enclosing fn");
        assert_eq!(func.kind(), "function_declaration");
        // No enclosing struct_type -> None.
        assert!(enclosing(lit, &["struct_type"]).is_none());
    }
}
