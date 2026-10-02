use std::collections::HashMap;
use stratify_core::ir::{Reference, Span, SymbolId};
use stratify_core::{Confidence, IrGraph, RefKind, SymbolKind};

/// Resolve cross-file calls: for each recorded unresolved call, add a `Calls`
/// edge to every repo-wide Function whose name matches the callee, when that
/// function lives in a DIFFERENT file than the caller. Edges are `Likely`
/// (bare-name matching is a heuristic, so this only ever downgrades a "dead"
/// verdict, never falsely clears one). Existing identical edges are not
/// duplicated.
pub fn cross_file_calls(graph: &mut IrGraph) {
    // Repo-wide function name -> (symbol id, file).
    let mut by_name: HashMap<String, Vec<(SymbolId, String)>> = HashMap::new();
    for s in graph.symbols() {
        if matches!(s.kind, SymbolKind::Function) {
            by_name
                .entry(s.name.clone())
                .or_default()
                .push((s.id, s.span.file.clone()));
        }
    }

    // Existing Calls edges, to avoid duplicates.
    let mut existing: std::collections::HashSet<(SymbolId, SymbolId)> = graph
        .references()
        .iter()
        .filter(|r| matches!(r.kind, RefKind::Calls))
        .map(|r| (r.from, r.to))
        .collect();

    // Caller id -> caller file, for the cross-file check.
    let caller_file: HashMap<SymbolId, String> = graph
        .symbols()
        .iter()
        .map(|s| (s.id, s.span.file.clone()))
        .collect();

    let mut to_add: Vec<Reference> = Vec::new();
    for (from, name) in graph.unresolved_calls() {
        let Some(candidates) = by_name.get(name) else {
            continue; // not a repo function (stdlib/builtin/external) — skip
        };
        let from_file = caller_file.get(from);
        for (to, to_file) in candidates {
            // cross-file only: intra-file calls were already resolved by adapters
            if from_file.map(|f| f == to_file).unwrap_or(false) {
                continue;
            }
            if to == from {
                continue;
            }
            if existing.insert((*from, *to)) {
                to_add.push(Reference {
                    from: *from,
                    to: *to,
                    kind: RefKind::Calls,
                    span: Span {
                        file: "<resolved>".into(),
                        start_byte: 0,
                        end_byte: 0,
                        start_line: 0,
                    },
                    confidence: Confidence::Likely,
                });
            }
        }
    }
    for r in to_add {
        graph.add_reference(r);
    }
}

/// Promote unambiguous intra-file Calls edges (Likely -> Certain): when a call
/// targets the unique function of that name in the same file, it is a real,
/// unambiguous use, so the target is genuinely used (not "possibly unused").
/// Never touches cross-file edges, preserving the never-false-clear guarantee.
pub fn promote_intra_file_calls(graph: &mut IrGraph) {
    // (file, function name) -> count of Function symbols
    let mut counts: HashMap<(String, String), usize> = HashMap::new();
    for s in graph.symbols() {
        if matches!(s.kind, SymbolKind::Function) {
            *counts
                .entry((s.span.file.clone(), s.name.clone()))
                .or_insert(0) += 1;
        }
    }
    let mut promote: Vec<usize> = Vec::new();
    for (i, r) in graph.references().iter().enumerate() {
        if !matches!(r.kind, RefKind::Calls) || r.confidence != Confidence::Likely {
            continue;
        }
        let (Some(from), Some(to)) = (graph.symbol(r.from), graph.symbol(r.to)) else {
            continue;
        };
        if to.kind != SymbolKind::Function {
            continue;
        }
        if from.span.file != to.span.file {
            continue; // intra-file only
        }
        if counts
            .get(&(to.span.file.clone(), to.name.clone()))
            .copied()
            == Some(1)
        {
            promote.push(i);
        }
    }
    for i in promote {
        graph.set_reference_confidence(i, Confidence::Certain);
    }
}

/// Resolve Go imports: rewrite each Dependency reached by an `Imports` edge
/// from a `.go` file so its name becomes the longest in-repo Go package dir
/// that is a suffix of the raw import path. External imports (no matching
/// package dir) are left unchanged (they then match no fqn and form no edge).
pub fn go_imports(graph: &mut IrGraph) {
    // Known Go package dirs = fqns of File symbols whose file ends in ".go".
    let mut pkgs: Vec<String> = graph
        .symbols()
        .iter()
        .filter(|s| matches!(s.kind, SymbolKind::File) && s.span.file.ends_with(".go"))
        .map(|s| s.fqn.clone())
        .collect();
    pkgs.sort();
    pkgs.dedup();
    if pkgs.is_empty() {
        return;
    }

    // Collect (dependency id, new name) for Go import edges.
    let mut renames: Vec<(stratify_core::ir::SymbolId, String)> = Vec::new();
    for r in graph.references() {
        if !matches!(r.kind, RefKind::Imports) {
            continue;
        }
        let (Some(from), Some(to)) = (graph.symbol(r.from), graph.symbol(r.to)) else {
            continue;
        };
        if !from.span.file.ends_with(".go") {
            continue;
        }
        let path = to.name.as_str();
        // longest package dir that equals the path or is a trailing path segment of it
        let best = pkgs
            .iter()
            .filter(|d| path == d.as_str() || path.ends_with(&format!("/{d}")))
            .max_by_key(|d| d.len());
        if let Some(dir) = best {
            if dir.as_str() != path {
                renames.push((to.id, dir.clone()));
            }
        }
    }
    for (id, name) in renames {
        graph.set_symbol_name(id, name);
    }
}

/// One Cargo package: its crate name as `use` paths spell it (hyphens become
/// underscores) and its `src` directory relative to the scan root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustCrate {
    pub name: String,
    pub src_dir: String,
}

/// Resolve Rust imports: rewrite each Dependency reached by an `Imports` edge
/// from a `.rs` file so its name becomes the `.rs` file of the module the
/// path names. `crate::`, `self::`, `super::`, and paths starting with a
/// scanned crate's name resolve. Anything else (std, external crates) keeps
/// its raw path and forms no edge. The longest path prefix that names an
/// existing module file wins, so `crate::a::Item` lands on `src/a.rs`.
pub fn rust_imports(graph: &mut IrGraph, crates: &[RustCrate]) {
    let files: std::collections::HashSet<String> = graph
        .symbols()
        .iter()
        .filter(|s| matches!(s.kind, SymbolKind::File) && s.span.file.ends_with(".rs"))
        .map(|s| s.span.file.clone())
        .collect();
    if files.is_empty() {
        return;
    }
    // Crate source roots: Cargo packages, plus any directory holding a
    // `lib.rs` or `main.rs` (covers scans with no Cargo.toml).
    let mut roots: Vec<String> = crates.iter().map(|c| c.src_dir.clone()).collect();
    for f in &files {
        let name = file_name(f);
        if name == "lib.rs" || name == "main.rs" {
            roots.push(parent_dir(f).unwrap_or_default());
        }
    }
    roots.sort();
    roots.dedup();

    let mut renames: Vec<(SymbolId, String)> = Vec::new();
    for r in graph.references() {
        if !matches!(r.kind, RefKind::Imports) {
            continue;
        }
        let (Some(from), Some(to)) = (graph.symbol(r.from), graph.symbol(r.to)) else {
            continue;
        };
        if !from.span.file.ends_with(".rs") {
            continue;
        }
        if let Some(target) = resolve_rust_path(&from.span.file, &to.name, &files, &roots, crates) {
            if target != to.name {
                renames.push((to.id, target));
            }
        }
    }
    for (id, name) in renames {
        graph.set_symbol_name(id, name);
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Parent directory of a `/`-separated path. `"a.rs"` gives `Some("")`,
/// `""` gives `None`.
fn parent_dir(path: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    Some(match path.rfind('/') {
        Some(i) => path[..i].to_string(),
        None => String::new(),
    })
}

fn join_dir(base: &str, rel: &str) -> String {
    if base.is_empty() {
        rel.to_string()
    } else {
        format!("{base}/{rel}")
    }
}

/// Directory holding this file's child modules: `src/a.rs` -> `src/a`,
/// `src/a/mod.rs` or `src/lib.rs` -> its own directory.
fn children_dir(file: &str) -> String {
    match file_name(file) {
        "lib.rs" | "main.rs" | "mod.rs" => parent_dir(file).unwrap_or_default(),
        _ => file.trim_end_matches(".rs").to_string(),
    }
}

/// The crate root directory containing `file`: the longest root that prefixes it.
fn crate_root_of(file: &str, roots: &[String]) -> Option<String> {
    roots
        .iter()
        .filter(|r| r.is_empty() || file.starts_with(&format!("{r}/")))
        .max_by_key(|r| r.len())
        .cloned()
}

/// The file defining the module whose children live in `dir`.
fn module_file(
    dir: &str,
    files: &std::collections::HashSet<String>,
    roots: &[String],
) -> Option<String> {
    let mut candidates = Vec::new();
    if roots.iter().any(|r| r == dir) {
        candidates.push(join_dir(dir, "lib.rs"));
        candidates.push(join_dir(dir, "main.rs"));
    }
    if !dir.is_empty() {
        candidates.push(format!("{dir}.rs"));
        candidates.push(join_dir(dir, "mod.rs"));
    }
    candidates.into_iter().find(|c| files.contains(c))
}

/// Longest prefix of `segs` naming a module file under `base`. With
/// `fallback_to_base`, an empty match lands on `base`'s own module file.
fn longest_module(
    base: &str,
    segs: &[&str],
    files: &std::collections::HashSet<String>,
    roots: &[String],
    fallback_to_base: bool,
) -> Option<String> {
    for k in (1..=segs.len()).rev() {
        let stem = join_dir(base, &segs[..k].join("/"));
        for cand in [format!("{stem}.rs"), format!("{stem}/mod.rs")] {
            if files.contains(&cand) {
                return Some(cand);
            }
        }
    }
    if fallback_to_base {
        module_file(base, files, roots)
    } else {
        None
    }
}

fn resolve_rust_path(
    file: &str,
    path: &str,
    files: &std::collections::HashSet<String>,
    roots: &[String],
    crates: &[RustCrate],
) -> Option<String> {
    let segs: Vec<&str> = path.split("::").filter(|s| !s.is_empty()).collect();
    let first = *segs.first()?;
    match first {
        "crate" => {
            let base = crate_root_of(file, roots)?;
            longest_module(&base, &segs[1..], files, roots, true)
        }
        "self" => longest_module(&children_dir(file), &segs[1..], files, roots, true),
        "super" => {
            let n = segs.iter().take_while(|s| **s == "super").count();
            let mut dir = children_dir(file);
            for _ in 0..n {
                dir = parent_dir(&dir)?;
            }
            longest_module(&dir, &segs[n..], files, roots, true)
        }
        name => {
            if let Some(c) = crates.iter().find(|c| c.name == name) {
                longest_module(&c.src_dir, &segs[1..], files, roots, true)
            } else {
                // 2018-edition path to a child module of this file.
                longest_module(&children_dir(file), &segs, files, roots, false)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stratify_core::ir::{Symbol, Visibility};

    fn func(g: &mut IrGraph, name: &str, file: &str) -> SymbolId {
        g.add_symbol(Symbol {
            id: SymbolId(0),
            kind: SymbolKind::Function,
            name: name.into(),
            fqn: name.into(),
            span: Span {
                file: file.into(),
                start_byte: 0,
                end_byte: 1,
                start_line: 1,
            },
            visibility: Visibility::Unknown,
            confidence: Confidence::Certain,
        })
    }

    #[test]
    fn resolves_cross_file_call() {
        let mut g = IrGraph::new();
        let caller = func(&mut g, "caller", "a.rb");
        let target = func(&mut g, "target", "b.rb");
        g.add_unresolved_call(caller, "target".into());
        cross_file_calls(&mut g);
        assert!(g
            .references()
            .iter()
            .any(|r| matches!(r.kind, RefKind::Calls)
                && r.from == caller
                && r.to == target
                && r.confidence == Confidence::Likely));
    }

    #[test]
    fn ignores_unknown_callee() {
        let mut g = IrGraph::new();
        let caller = func(&mut g, "caller", "a.rb");
        g.add_unresolved_call(caller, "println".into()); // no repo function named this
        cross_file_calls(&mut g);
        assert!(g
            .references()
            .iter()
            .all(|r| !matches!(r.kind, RefKind::Calls)));
    }

    #[test]
    fn does_not_resolve_same_file() {
        // An unresolved call whose only match is in the caller's own file is not
        // re-added here (intra-file resolution is the adapter's job).
        let mut g = IrGraph::new();
        let caller = func(&mut g, "caller", "a.rb");
        let _same = func(&mut g, "target", "a.rb");
        g.add_unresolved_call(caller, "target".into());
        cross_file_calls(&mut g);
        assert!(g
            .references()
            .iter()
            .all(|r| !matches!(r.kind, RefKind::Calls)));
    }

    #[test]
    fn dedupes_against_existing_edge() {
        let mut g = IrGraph::new();
        let caller = func(&mut g, "caller", "a.rb");
        let target = func(&mut g, "target", "b.rb");
        g.add_reference(Reference {
            from: caller,
            to: target,
            kind: RefKind::Calls,
            span: Span {
                file: "a.rb".into(),
                start_byte: 0,
                end_byte: 1,
                start_line: 1,
            },
            confidence: Confidence::Likely,
        });
        g.add_unresolved_call(caller, "target".into());
        cross_file_calls(&mut g);
        let count = g
            .references()
            .iter()
            .filter(|r| matches!(r.kind, RefKind::Calls) && r.from == caller && r.to == target)
            .count();
        assert_eq!(count, 1, "should not duplicate the existing edge");
    }

    fn likely_call(g: &mut IrGraph, from: SymbolId, to: SymbolId, file: &str) {
        g.add_reference(Reference {
            from,
            to,
            kind: RefKind::Calls,
            span: Span {
                file: file.into(),
                start_byte: 0,
                end_byte: 1,
                start_line: 1,
            },
            confidence: Confidence::Likely,
        });
    }

    #[test]
    fn promotes_unique_intra_file_call() {
        let mut g = IrGraph::new();
        let main = func(&mut g, "main", "a.rb");
        let helper = func(&mut g, "helper", "a.rb");
        likely_call(&mut g, main, helper, "a.rb");
        promote_intra_file_calls(&mut g);
        assert_eq!(g.references()[0].confidence, Confidence::Certain);
    }

    #[test]
    fn keeps_ambiguous_same_name_intra_file_likely() {
        let mut g = IrGraph::new();
        let main = func(&mut g, "main", "a.rb");
        let helper1 = func(&mut g, "helper", "a.rb");
        let _helper2 = func(&mut g, "helper", "a.rb"); // two functions share the name
        likely_call(&mut g, main, helper1, "a.rb");
        promote_intra_file_calls(&mut g);
        assert_eq!(g.references()[0].confidence, Confidence::Likely);
    }

    #[test]
    fn does_not_promote_cross_file_call() {
        let mut g = IrGraph::new();
        let caller = func(&mut g, "caller", "a.rb");
        let target = func(&mut g, "target", "b.rb"); // different file, unique name
        likely_call(&mut g, caller, target, "<resolved>");
        promote_intra_file_calls(&mut g);
        assert_eq!(g.references()[0].confidence, Confidence::Likely);
    }

    #[test]
    fn go_imports_resolves_by_suffix() {
        use crate::test_support::{add_ref, add_sym};
        let mut g = IrGraph::new();
        // package "b" exists (b/b.go), file in package "a" imports example.com/m/b
        let a_file = add_sym(&mut g, SymbolKind::File, "a/a.go", "a", "a/a.go");
        add_sym(&mut g, SymbolKind::File, "b/b.go", "b", "b/b.go");
        let dep = add_sym(
            &mut g,
            SymbolKind::Dependency,
            "example.com/m/b",
            "example.com/m/b",
            "x",
        );
        add_ref(&mut g, a_file, dep, RefKind::Imports, "a/a.go");
        go_imports(&mut g);
        assert_eq!(g.symbol(dep).unwrap().name, "b");
    }

    #[test]
    fn go_imports_leaves_external_unchanged() {
        use crate::test_support::{add_ref, add_sym};
        let mut g = IrGraph::new();
        let a_file = add_sym(&mut g, SymbolKind::File, "a/a.go", "a", "a/a.go");
        let dep = add_sym(&mut g, SymbolKind::Dependency, "fmt", "fmt", "x");
        add_ref(&mut g, a_file, dep, RefKind::Imports, "a/a.go");
        go_imports(&mut g);
        assert_eq!(g.symbol(dep).unwrap().name, "fmt"); // no matching package dir -> unchanged
    }

    fn rust_file(g: &mut IrGraph, path: &str) -> SymbolId {
        crate::test_support::add_sym(g, SymbolKind::File, path, path, path)
    }

    fn rust_import(g: &mut IrGraph, from: SymbolId, from_file: &str, raw: &str) -> SymbolId {
        let dep = crate::test_support::add_sym(g, SymbolKind::Dependency, raw, raw, from_file);
        crate::test_support::add_ref(g, from, dep, RefKind::Imports, from_file);
        dep
    }

    fn krate(name: &str, src_dir: &str) -> RustCrate {
        RustCrate {
            name: name.into(),
            src_dir: src_dir.into(),
        }
    }

    #[test]
    fn rust_crate_path_resolves_to_module_file() {
        let mut g = IrGraph::new();
        rust_file(&mut g, "src/lib.rs");
        let a = rust_file(&mut g, "src/a.rs");
        rust_file(&mut g, "src/b.rs");
        let dep = rust_import(&mut g, a, "src/a.rs", "crate::b::Thing");
        rust_imports(&mut g, &[krate("demo", "src")]);
        assert_eq!(g.symbol(dep).unwrap().name, "src/b.rs");
    }

    #[test]
    fn rust_mod_rs_and_nested_paths_resolve() {
        let mut g = IrGraph::new();
        rust_file(&mut g, "src/lib.rs");
        let d = rust_file(&mut g, "src/domain/mod.rs");
        rust_file(&mut g, "src/api/mod.rs");
        rust_file(&mut g, "src/api/routes.rs");
        let d1 = rust_import(&mut g, d, "src/domain/mod.rs", "crate::api::handler");
        let d2 = rust_import(&mut g, d, "src/domain/mod.rs", "crate::api::routes::list");
        rust_imports(&mut g, &[krate("demo", "src")]);
        assert_eq!(g.symbol(d1).unwrap().name, "src/api/mod.rs");
        assert_eq!(g.symbol(d2).unwrap().name, "src/api/routes.rs");
    }

    #[test]
    fn rust_item_defined_in_crate_root_resolves_to_lib_rs() {
        let mut g = IrGraph::new();
        rust_file(&mut g, "src/lib.rs");
        let a = rust_file(&mut g, "src/a.rs");
        let dep = rust_import(&mut g, a, "src/a.rs", "crate::Config");
        rust_imports(&mut g, &[krate("demo", "src")]);
        assert_eq!(g.symbol(dep).unwrap().name, "src/lib.rs");
    }

    #[test]
    fn rust_super_and_self_paths_resolve() {
        let mut g = IrGraph::new();
        rust_file(&mut g, "src/lib.rs");
        rust_file(&mut g, "src/a.rs");
        let b = rust_file(&mut g, "src/a/b.rs");
        rust_file(&mut g, "src/a/b/c.rs");
        rust_file(&mut g, "src/a/sibling.rs");
        let up = rust_import(&mut g, b, "src/a/b.rs", "super::sibling::f");
        let up_self = rust_import(&mut g, b, "src/a/b.rs", "super::Item");
        let down = rust_import(&mut g, b, "src/a/b.rs", "self::c::g");
        rust_imports(&mut g, &[krate("demo", "src")]);
        assert_eq!(g.symbol(up).unwrap().name, "src/a/sibling.rs");
        assert_eq!(g.symbol(up_self).unwrap().name, "src/a.rs");
        assert_eq!(g.symbol(down).unwrap().name, "src/a/b/c.rs");
    }

    #[test]
    fn rust_cross_crate_path_resolves_through_crate_map() {
        let mut g = IrGraph::new();
        rust_file(&mut g, "crates/core/src/lib.rs");
        rust_file(&mut g, "crates/core/src/ir.rs");
        let cli = rust_file(&mut g, "crates/cli/src/main.rs");
        let dep = rust_import(&mut g, cli, "crates/cli/src/main.rs", "my_core::ir::Span");
        rust_imports(
            &mut g,
            &[
                krate("my_core", "crates/core/src"),
                krate("cli", "crates/cli/src"),
            ],
        );
        assert_eq!(g.symbol(dep).unwrap().name, "crates/core/src/ir.rs");
    }

    #[test]
    fn rust_external_crates_stay_unresolved() {
        let mut g = IrGraph::new();
        rust_file(&mut g, "src/lib.rs");
        let a = rust_file(&mut g, "src/a.rs");
        let std_dep = rust_import(&mut g, a, "src/a.rs", "std::collections::HashMap");
        let serde_dep = rust_import(&mut g, a, "src/a.rs", "serde::Serialize");
        rust_imports(&mut g, &[krate("demo", "src")]);
        assert_eq!(g.symbol(std_dep).unwrap().name, "std::collections::HashMap");
        assert_eq!(g.symbol(serde_dep).unwrap().name, "serde::Serialize");
    }

    #[test]
    fn rust_crate_root_without_cargo_map_uses_lib_rs_dir() {
        let mut g = IrGraph::new();
        rust_file(&mut g, "lib.rs");
        let a = rust_file(&mut g, "a.rs");
        rust_file(&mut g, "b.rs");
        let dep = rust_import(&mut g, a, "a.rs", "crate::b::f");
        rust_imports(&mut g, &[]);
        assert_eq!(g.symbol(dep).unwrap().name, "b.rs");
    }

    #[test]
    fn rust_imports_ignore_non_rust_files() {
        let mut g = IrGraph::new();
        let a = crate::test_support::add_sym(&mut g, SymbolKind::File, "a/a.go", "a", "a/a.go");
        let dep = rust_import(&mut g, a, "a/a.go", "crate::b");
        rust_imports(&mut g, &[]);
        assert_eq!(g.symbol(dep).unwrap().name, "crate::b");
    }
}
