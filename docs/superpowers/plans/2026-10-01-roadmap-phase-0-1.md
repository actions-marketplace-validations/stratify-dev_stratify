# Roadmap Phase 0 and Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give Rust the two missing analyses, measure precision and speed, and give findings evidence, rule metadata, suppression, and a baseline gate.

**Architecture:** Rust `use` paths become `Dependency` symbols in the Rust adapter. A repo-wide resolver in `stratify-analysis` rewrites them to file paths, the same way `go_imports` works for Go. The six existing analyses then see Rust import edges with no analysis change. Phase 1 adds optional fields to `Finding`, a static rule registry in `stratify-core`, and two post-analysis filters in `stratify-cli` (suppression comments, baseline). Measurement lives in `corpus/` and `scripts/`.

**Tech Stack:** Rust 2021 workspace, tree-sitter 0.24 and tree-sitter-rust 0.23, serde, clap 4, toml 0.8, Python 3 standard library for the scripts.

**Spec:** `docs/roadmap-full-stack-static-analysis.md` (Phase 0 and Phase 1 sections).

## Global Constraints

- One binary. No servers. No accounts.
- Analyses read only the IR and never branch on language.
- `Report::SCHEMA_VERSION` stays `1`. New `Finding` fields are optional, default to empty, and are omitted from JSON when empty. Reason: `stratify-judge` passes any report with `schema_version > 1` through unjudged (`~/dev/stratify-jev/crates/stratify-judge-cli/src/main.rs:127`) and already tolerates unknown fields through `#[serde(flatten)] extra`.
- A finding with no evidence serializes byte-for-byte as it does in v0.6.0.
- Every change passes `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test --locked` (the CI gate in `.github/workflows/ci.yml`).
- `cargo` lives at `~/.cargo/bin/cargo`. Run `export PATH="$HOME/.cargo/bin:$PATH"` before any cargo command.
- Each user-visible change adds a line under `## [Unreleased]` in `CHANGELOG.md` (Task 1 creates the heading above `## [0.6.0]`).
- Commit messages follow the repo style (`feat(scope): ...`, `fix(scope): ...`, `docs: ...`) and end with the line `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Work on branch `roadmap-phase-0-1`. Never commit to `main`.

## Review Focus

1. `use super::*;` inside an inline `#[cfg(test)] mod tests { ... }` refers to the same file. Expected: no import edge. Test in Task 1.
2. `use std::...`, `use serde::...`, and any crate outside the scan. Expected: no edge, no finding, no panic. Test in Task 2.
3. A suppression comment naming a different rule. Expected: the finding stays. Test in Task 5.
4. A baselined finding moves down ten lines after an edit above it. Expected: still baselined, gate passes. Test in Task 6.
5. A `--baseline` path missing or holding invalid JSON. Expected: error on stderr, exit code 2, never a silent pass. Test in Task 6.

---

### Task 1: Rust adapter emits `use` imports

**Files:**
- Modify: `crates/stratify-lang-rust/src/extract.rs`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: nothing new.
- Produces: for each `use` declaration, one `SymbolKind::Dependency` symbol per flattened path (`name` and `fqn` both equal the path, e.g. `crate::a::b`), plus a `RefKind::Imports` edge from the File symbol, `Confidence::Certain`. The Rust File symbol's `fqn` becomes the file path itself (was the parent directory). Task 2 relies on both.

- [ ] **Step 1: Write the failing tests**

Append to the `tests` module in `crates/stratify-lang-rust/src/extract.rs`. Replace the existing `file_fqn_is_parent_dir` test with `file_fqn_is_file_path`.

```rust
    fn import_names(g: &IrGraph) -> Vec<String> {
        let mut v: Vec<String> = g
            .references()
            .iter()
            .filter(|r| matches!(r.kind, RefKind::Imports))
            .map(|r| g.symbol(r.to).unwrap().name.clone())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn file_fqn_is_file_path() {
        let g = extract("src/svc/a.rs", "fn a() {}\n");
        let f = g
            .symbols()
            .iter()
            .find(|s| s.kind == SymbolKind::File)
            .unwrap();
        assert_eq!(f.fqn, "src/svc/a.rs");
    }

    #[test]
    fn expands_use_trees() {
        assert_eq!(expand_use_tree("crate::a::b"), vec!["crate::a::b"]);
        assert_eq!(
            expand_use_tree("crate::a::{b, c::{d, e}, self}"),
            vec!["crate::a::b", "crate::a::c::d", "crate::a::c::e", "crate::a"]
        );
        assert_eq!(expand_use_tree("super::x as y"), vec!["super::x"]);
        assert_eq!(expand_use_tree("crate::m::*"), vec!["crate::m"]);
        assert_eq!(expand_use_tree("::std::fmt"), vec!["std::fmt"]);
        assert_eq!(
            expand_use_tree("a::{\n    b,\n    c as d,\n}"),
            vec!["a::b", "a::c"]
        );
    }

    #[test]
    fn emits_import_per_use_path() {
        let src = "use crate::a::{b, c};\npub use super::d;\nfn f() {}\n";
        let g = extract("src/x.rs", src);
        assert_eq!(
            import_names(&g),
            vec!["crate::a::b", "crate::a::c", "super::d"]
        );
    }

    #[test]
    fn use_inside_inline_test_module_stays_in_file() {
        // `super` from `mod tests` is this file: no import edge.
        let src = "fn f() {}\n#[cfg(test)]\nmod tests {\n    use super::*;\n    use self::inner::x;\n}\n";
        let g = extract("src/x.rs", src);
        assert!(import_names(&g).is_empty(), "{:?}", import_names(&g));
    }

    #[test]
    fn super_super_inside_inline_module_keeps_one_level() {
        let src = "mod inner {\n    use super::super::y;\n}\n";
        let g = extract("src/a/x.rs", src);
        assert_eq!(import_names(&g), vec!["super::y"]);
    }

    #[test]
    fn crate_path_inside_inline_module_is_unchanged() {
        let src = "mod inner {\n    use crate::z::w;\n}\n";
        let g = extract("src/x.rs", src);
        assert_eq!(import_names(&g), vec!["crate::z::w"]);
    }

    #[test]
    fn duplicate_use_paths_emit_once() {
        let src = "use crate::a::b;\nmod m { use crate::a::b; }\n";
        let g = extract("src/x.rs", src);
        assert_eq!(import_names(&g), vec!["crate::a::b"]);
    }
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p stratify-lang-rust`
Expected: compile error, `cannot find function expand_use_tree`.

- [ ] **Step 3: Implement**

In `extract.rs`:

1. Delete `package_dir` and set the File symbol's `fqn: file.to_string()`. Update the comment above it to `// File symbol: fqn is the file path. A Rust file is one module.`
2. Replace the `// NOTE: No import edges...` comment with a call `extract_imports(&mut g, &lang, root, src, file, file_id);` placed after `walk::tokenize`.
3. Add these functions above `extract`:

```rust
/// Flatten the text of a `use` tree into one path per imported item.
/// `a::{b, c::{d, e}, self}` gives `a::b`, `a::c::d`, `a::c::e`, `a`.
/// Aliases (`x as y`) keep the path. Globs (`m::*`) keep the module path.
/// A leading `::` is dropped.
pub(crate) fn expand_use_tree(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    expand_into("", text, &mut out);
    out
}

fn join_path(prefix: &str, rest: &str) -> String {
    match (prefix.is_empty(), rest.is_empty()) {
        (true, _) => rest.to_string(),
        (_, true) => prefix.to_string(),
        _ => format!("{prefix}::{rest}"),
    }
}

/// Split `inner` on commas at brace depth zero.
fn split_top_level(inner: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (i, ch) in inner.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(&inner[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&inner[start..]);
    parts
}

fn expand_into(prefix: &str, tree: &str, out: &mut Vec<String>) {
    let tree = tree.trim();
    let tree = tree.strip_prefix("::").unwrap_or(tree).trim();
    if tree.is_empty() {
        return;
    }
    if let (Some(open), Some(close)) = (tree.find('{'), tree.rfind('}')) {
        let head: String = tree[..open].chars().filter(|c| !c.is_whitespace()).collect();
        let base = join_path(prefix, head.trim_end_matches("::"));
        for item in split_top_level(&tree[open + 1..close]) {
            if item.trim() == "self" {
                if !base.is_empty() {
                    out.push(base.clone());
                }
            } else {
                expand_into(&base, item, out);
            }
        }
        return;
    }
    // Drop an `as alias` suffix, then any whitespace inside the path.
    let tokens: Vec<&str> = tree.split_whitespace().collect();
    let path_tokens: Vec<&str> = tokens.iter().take_while(|t| **t != "as").copied().collect();
    let path: String = path_tokens.concat();
    let path = path.trim_end_matches('*').trim_end_matches("::");
    let full = join_path(prefix, path);
    if !full.is_empty() {
        out.push(full);
    }
}

/// Count `mod name { ... }` blocks enclosing `node`. A `mod name;` declaration
/// has no body and never encloses anything, so every ancestor `mod_item` is inline.
fn inline_mod_depth(node: Node) -> usize {
    let mut depth = 0;
    let mut cur = node.parent();
    while let Some(n) = cur {
        if n.kind() == "mod_item" {
            depth += 1;
        }
        cur = n.parent();
    }
    depth
}

/// Rebase a `use` path written inside `depth` inline modules onto the file's
/// own module. Returns None when the path points inside this same file
/// (`self::...`, or no more `super`s than inline levels).
fn rebase_for_inline_mods(path: &str, depth: usize) -> Option<String> {
    if depth == 0 {
        return Some(path.to_string());
    }
    let segs: Vec<&str> = path.split("::").collect();
    if segs[0] == "self" {
        return None;
    }
    let supers = segs.iter().take_while(|s| **s == "super").count();
    if supers == 0 {
        return Some(path.to_string());
    }
    if supers <= depth {
        return None;
    }
    Some(segs[depth..].join("::"))
}

/// Emit one Dependency + Imports edge per distinct `use` path in this file.
/// Raw paths only: the repo-wide `resolve::rust_imports` pass maps them to files.
fn extract_imports(
    g: &mut IrGraph,
    lang: &tree_sitter::Language,
    root: Node,
    src: &str,
    file: &str,
    file_id: SymbolId,
) {
    let query = Query::new(lang, "(use_declaration argument: (_) @arg) @decl")
        .expect("rust use query");
    let arg_idx = query.capture_index_for_name("arg").unwrap();
    let decl_idx = query.capture_index_for_name("decl").unwrap();

    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut found: Vec<(String, Node)> = Vec::new();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, root, src.as_bytes());
    while let Some(m) = matches.next() {
        let mut arg = None;
        let mut decl = None;
        for cap in m.captures {
            if cap.index == arg_idx {
                arg = Some(cap.node);
            } else if cap.index == decl_idx {
                decl = Some(cap.node);
            }
        }
        let (Some(arg), Some(decl)) = (arg, decl) else {
            continue;
        };
        let depth = inline_mod_depth(decl);
        for path in expand_use_tree(walk::node_text(arg, src)) {
            if let Some(p) = rebase_for_inline_mods(&path, depth) {
                if seen.insert(p.clone()) {
                    found.push((p, decl));
                }
            }
        }
    }
    for (path, decl) in found {
        let dep_id = g.add_symbol(Symbol {
            id: SymbolId(0),
            kind: SymbolKind::Dependency,
            name: path.clone(),
            fqn: path,
            span: walk::span(decl, file),
            visibility: Visibility::Unknown,
            confidence: Confidence::Certain,
        });
        g.add_reference(Reference {
            from: file_id,
            to: dep_id,
            kind: RefKind::Imports,
            span: walk::span(decl, file),
            confidence: Confidence::Certain,
        });
    }
}
```

4. In `CHANGELOG.md`, insert above `## [0.6.0] - 2026-09-01`:

```markdown
## [Unreleased]

### Changed
- The Rust adapter records `use` declarations as imports, and a Rust file's
  module key is now its own path instead of its parent directory.
```

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p stratify-lang-rust && cargo test --workspace`
Expected: all pass. If the workspace run fails in another crate because Rust File fqns changed, report it. Do not change analysis code in this task.

- [ ] **Step 5: Lint and commit**

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings
git add crates/stratify-lang-rust/src/extract.rs CHANGELOG.md
git commit -m "feat(rust): record use declarations as import edges"
```

---

### Task 2: Resolve Rust imports to files (cycles and boundaries for Rust)

**Files:**
- Modify: `crates/stratify-analysis/src/resolve.rs`
- Modify: `crates/stratify-cli/src/run.rs`
- Create: `crates/stratify-cli/tests/sample-rustcycle/Cargo.toml`, `src/lib.rs`, `src/a.rs`, `src/b.rs`
- Create: `crates/stratify-cli/tests/sample-rustboundary/Cargo.toml`, `stratify.toml`, `src/lib.rs`, `src/domain/mod.rs`, `src/api/mod.rs`
- Create: `crates/stratify-cli/tests/e2e_rustcycle.rs`
- Modify: `README.md`, `CHANGELOG.md`

**Interfaces:**
- Consumes: Task 1's Dependency symbols named with raw `use` paths, and Rust File fqn equal to the file path.
- Produces: `pub struct RustCrate { pub name: String, pub src_dir: String }` and `pub fn rust_imports(graph: &mut IrGraph, crates: &[RustCrate])` in `stratify_analysis::resolve`. After this pass a resolved Dependency's `name` equals the target `.rs` file path. Unresolved ones keep the raw path and form no edge.

- [ ] **Step 1: Write the failing unit tests**

Append to the `tests` module in `crates/stratify-analysis/src/resolve.rs`:

```rust
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
            &[krate("my_core", "crates/core/src"), krate("cli", "crates/cli/src")],
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
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p stratify-analysis rust_`
Expected: compile error, `cannot find struct RustCrate`.

- [ ] **Step 3: Implement the resolver**

Add to `crates/stratify-analysis/src/resolve.rs` after `go_imports`:

```rust
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
        if let Some(target) = resolve_rust_path(&from.span.file, &to.name, &files, &roots, crates)
        {
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
```

- [ ] **Step 4: Run the unit tests and confirm they pass**

Run: `cargo test -p stratify-analysis`
Expected: all pass.

- [ ] **Step 5: Write the fixtures and the failing e2e test**

`crates/stratify-cli/tests/sample-rustcycle/Cargo.toml`:
```toml
[package]
name = "rust-cycle-demo"
version = "0.1.0"
edition = "2021"
```
`src/lib.rs`:
```rust
pub mod a;
pub mod b;
```
`src/a.rs`:
```rust
use crate::b::from_b;

pub fn from_a() -> i32 {
    from_b() + 1
}
```
`src/b.rs`:
```rust
use crate::a::from_a;

pub fn from_b() -> i32 {
    if false { from_a() } else { 0 }
}
```

`crates/stratify-cli/tests/sample-rustboundary/Cargo.toml`:
```toml
[package]
name = "rust-boundary-demo"
version = "0.1.0"
edition = "2021"
```
`stratify.toml`:
```toml
[layers]
domain = ["src/domain/**"]
api = ["src/api/**"]

[[forbid]]
from = "domain"
to = "api"
```
`src/lib.rs`:
```rust
pub mod api;
pub mod domain;
```
`src/domain/mod.rs`:
```rust
use crate::api::handler;

pub fn rule() -> i32 {
    handler()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn works() {
        assert_eq!(rule(), 1);
    }
}
```
`src/api/mod.rs`:
```rust
pub fn handler() -> i32 {
    1
}
```

`crates/stratify-cli/tests/e2e_rustcycle.rs`:
```rust
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
    assert_eq!(stdout.matches("\"rule\": \"cycle\"").count(), 0, "stdout: {stdout}");
}
```

Run: `cargo test -p stratify-cli --test e2e_rustcycle`
Expected: FAIL, zero cycle and zero boundary findings.

- [ ] **Step 6: Wire the resolver into `run.rs`**

In `analyze_repo_with_stats`:

1. Before the walk loop add `let mut rust_crates: Vec<stratify_analysis::resolve::RustCrate> = Vec::new();`.
2. Inside the loop, right after the `is_file` check and before the extension lookup, add:

```rust
        if path.file_name().and_then(|n| n.to_str()) == Some("Cargo.toml") {
            let rel = path.strip_prefix(root).unwrap_or(path).to_string_lossy().to_string();
            if !ignore_globs.is_match(&rel) {
                if let Some(c) = rust_crate_of(root, path) {
                    rust_crates.push(c);
                }
            }
            continue;
        }
```

3. After `stratify_analysis::resolve::go_imports(&mut graph);` add `stratify_analysis::resolve::rust_imports(&mut graph, &rust_crates);`.
4. Add the helper below `analyze_repo`:

```rust
/// Read a Cargo manifest's `[package] name` and pair it with the package's
/// `src` directory (relative to the scan root). A manifest with no
/// `[package]` (a bare `[workspace]` root) yields None. A custom
/// `[lib] name` or `path` is not read.
fn rust_crate_of(root: &Path, manifest: &Path) -> Option<stratify_analysis::resolve::RustCrate> {
    let text = std::fs::read_to_string(manifest).ok()?;
    let doc: toml::Value = text.parse().ok()?;
    let name = doc.get("package")?.get("name")?.as_str()?.replace('-', "_");
    let src = manifest.parent()?.join("src");
    let src_dir = src
        .strip_prefix(root)
        .unwrap_or(&src)
        .to_string_lossy()
        .to_string();
    Some(stratify_analysis::resolve::RustCrate { name, src_dir })
}
```

5. Add a unit test in the `tests` module of `run.rs`:

```rust
    #[test]
    fn rust_crate_of_reads_package_name_and_src_dir() {
        let dir = temp_dir("rust-crate-of");
        std::fs::create_dir_all(dir.join("crates/my-core")).unwrap();
        std::fs::write(
            dir.join("crates/my-core/Cargo.toml"),
            "[package]\nname = \"my-core\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(dir.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
        let c = rust_crate_of(&dir, &dir.join("crates/my-core/Cargo.toml")).unwrap();
        assert_eq!(c.name, "my_core");
        assert_eq!(c.src_dir, "crates/my-core/src");
        assert!(rust_crate_of(&dir, &dir.join("Cargo.toml")).is_none());
    }
```

- [ ] **Step 7: Run all tests and confirm they pass**

Run: `cargo test --workspace`
Expected: all pass, including `e2e_rustcycle`.

Then run the self-scan and record the new cycle and boundary counts in your report:
`cargo run -q -p stratify-cli -- check . --format json | python3 -c "import json,sys,collections; d=json.load(sys.stdin); print(collections.Counter(f['rule'] for f in d['findings']))"`

- [ ] **Step 8: Update docs and commit**

`README.md`:
- Line 126: `They all run in a single pass. Java, Ruby, TypeScript, Python, Go, and Rust get all six.`
- Section "Six languages, one engine": replace the Rust sentences with `All six languages get all six analyses. Rust import resolution follows \`crate::\`, \`self::\`, \`super::\`, and paths that start with the name of a crate in the scan. Paths into external crates form no edge.`
- Section "Status": `Six analyses across six languages, exposed through every surface: ...` (drop the Rust parenthetical).

`CHANGELOG.md` under `## [Unreleased]`, add:
```markdown
### Added
- Dependency cycles and layer boundaries for Rust. Every language now gets
  all six analyses. Imports resolve through `crate::`, `self::`, `super::`,
  and crate names read from each `Cargo.toml` in the scan.
```

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings
git add crates/stratify-analysis/src/resolve.rs crates/stratify-cli/src/run.rs \
  crates/stratify-cli/tests/sample-rustcycle crates/stratify-cli/tests/sample-rustboundary \
  crates/stratify-cli/tests/e2e_rustcycle.rs README.md CHANGELOG.md
git commit -m "feat(rust): resolve use paths to files for cycles and boundaries"
```

---

### Task 3: Rule registry

**Files:**
- Create: `crates/stratify-core/src/rule.rs`
- Modify: `crates/stratify-core/src/lib.rs`
- Modify: `crates/stratify-report/src/sarif.rs`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: nothing.
- Produces: `stratify_core::rule::{RuleMeta, RULES, rule}` with `pub fn rule(id: &str) -> Option<&'static RuleMeta>`. Tasks 4 and later read `RuleMeta.remediation` and `RuleMeta.description`.

- [ ] **Step 1: Write the failing tests**

Create `crates/stratify-core/src/rule.rs` with only the test module first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_emitted_rule_has_metadata() {
        for id in ["dead_code", "duplication", "complexity", "hotspot", "cycle", "boundary"] {
            let r = rule(id).unwrap_or_else(|| panic!("missing rule {id}"));
            assert_eq!(r.id, id);
            assert!(!r.description.is_empty());
            assert!(!r.remediation.is_empty());
            assert_eq!(r.languages.len(), 6, "{id} covers all six languages");
        }
    }

    #[test]
    fn rule_ids_are_unique() {
        let mut ids: Vec<&str> = RULES.iter().map(|r| r.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), RULES.len());
    }

    #[test]
    fn unknown_rule_is_none() {
        assert!(rule("nope").is_none());
    }
}
```

Add `pub mod rule;` to `crates/stratify-core/src/lib.rs`.

Add to the `tests` module of `crates/stratify-report/src/sarif.rs`:

```rust
    #[test]
    fn driver_rules_carry_registry_metadata() {
        let report = Report::new(vec![finding("cycle", Severity::Warning, "a.rb", 1)]);
        let v: serde_json::Value = serde_json::from_str(&render(&report)).unwrap();
        let r = &v["runs"][0]["tool"]["driver"]["rules"][0];
        assert_eq!(r["id"], "cycle");
        assert_eq!(r["name"], "Dependency cycle");
        assert!(r["help"]["text"].as_str().unwrap().contains("Break the cycle"));
        assert_eq!(r["properties"]["category"], "architecture");
        assert_eq!(r["defaultConfiguration"]["level"], "warning");
    }

    #[test]
    fn unknown_rule_still_renders() {
        let report = Report::new(vec![finding("custom", Severity::Info, "a.rb", 1)]);
        let v: serde_json::Value = serde_json::from_str(&render(&report)).unwrap();
        let r = &v["runs"][0]["tool"]["driver"]["rules"][0];
        assert_eq!(r["id"], "custom");
        assert_eq!(r["shortDescription"]["text"], "Stratify finding.");
    }
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p stratify-core rule && cargo test -p stratify-report`
Expected: compile errors (`rule` and `RULES` not found, `help` missing).

- [ ] **Step 3: Implement**

Above the test module in `rule.rs`:

```rust
use crate::finding::Severity;

/// Static description of one rule. The registry is the single source for
/// rule names, explanations, and fix guidance across every output surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleMeta {
    pub id: &'static str,
    pub name: &'static str,
    /// "maintainability" or "architecture" today. "correctness" and
    /// "security" arrive with flow analysis.
    pub category: &'static str,
    /// Severity of a full-confidence finding. Lower-confidence findings of
    /// the same rule may report lower.
    pub default_severity: Severity,
    pub languages: &'static [&'static str],
    pub tags: &'static [&'static str],
    pub description: &'static str,
    pub remediation: &'static str,
}

pub const ALL_LANGUAGES: &[&str] = &["java", "ruby", "typescript", "python", "go", "rust"];

pub static RULES: &[RuleMeta] = &[
    RuleMeta {
        id: "dead_code",
        name: "Unused function",
        category: "maintainability",
        default_severity: Severity::Warning,
        languages: ALL_LANGUAGES,
        tags: &["dead-code"],
        description: "Unused code: functions never reached from an entrypoint.",
        remediation: "Delete the function, or call it from code that runs. If a framework, reflection, or an external caller reaches it, suppress the finding with a comment naming the reason.",
    },
    RuleMeta {
        id: "duplication",
        name: "Duplicated code",
        category: "maintainability",
        default_severity: Severity::Warning,
        languages: ALL_LANGUAGES,
        tags: &["duplication"],
        description: "Duplicated code blocks.",
        remediation: "Extract the shared block into one function and call it from each site.",
    },
    RuleMeta {
        id: "complexity",
        name: "High cyclomatic complexity",
        category: "maintainability",
        default_severity: Severity::Warning,
        languages: ALL_LANGUAGES,
        tags: &["complexity"],
        description: "Functions with high cyclomatic complexity.",
        remediation: "Split the function. Move each branch body into a named helper, or replace condition chains with a lookup table.",
    },
    RuleMeta {
        id: "hotspot",
        name: "Churn hotspot",
        category: "maintainability",
        default_severity: Severity::Info,
        languages: ALL_LANGUAGES,
        tags: &["complexity", "churn"],
        description: "Complex code that also changes frequently.",
        remediation: "Prioritize this function for tests and refactoring. Frequent edits to complex code are where regressions cluster.",
    },
    RuleMeta {
        id: "cycle",
        name: "Dependency cycle",
        category: "architecture",
        default_severity: Severity::Warning,
        languages: ALL_LANGUAGES,
        tags: &["architecture", "imports"],
        description: "Circular dependencies between files.",
        remediation: "Break the cycle: move the shared code into a new module both sides import, or invert one import through an interface.",
    },
    RuleMeta {
        id: "boundary",
        name: "Layer boundary violation",
        category: "architecture",
        default_severity: Severity::Warning,
        languages: ALL_LANGUAGES,
        tags: &["architecture", "layers"],
        description: "Imports that violate configured layer boundaries.",
        remediation: "Move the import's target into a layer the source may depend on, or route the call through an allowed layer.",
    },
];

/// Look up a rule by id.
pub fn rule(id: &str) -> Option<&'static RuleMeta> {
    RULES.iter().find(|r| r.id == id)
}
```

In `sarif.rs`:
1. Delete `rule_description`.
2. Extend `RuleMeta` (the SARIF struct, rename it `SarifRule` to avoid confusion with the core type) with:

```rust
#[derive(Serialize)]
struct SarifRule {
    id: String,
    name: String,
    #[serde(rename = "shortDescription")]
    short_description: Text,
    #[serde(rename = "fullDescription", skip_serializing_if = "Option::is_none")]
    full_description: Option<Text>,
    #[serde(skip_serializing_if = "Option::is_none")]
    help: Option<Text>,
    #[serde(rename = "defaultConfiguration", skip_serializing_if = "Option::is_none")]
    default_configuration: Option<DefaultConfiguration>,
    #[serde(skip_serializing_if = "Option::is_none")]
    properties: Option<RuleProperties>,
}

#[derive(Serialize)]
struct DefaultConfiguration {
    level: &'static str,
}

#[derive(Serialize)]
struct RuleProperties {
    category: &'static str,
    tags: Vec<&'static str>,
}
```

3. Build each driver rule from `stratify_core::rule::rule(id)`:

```rust
fn sarif_rule(id: &str) -> SarifRule {
    match stratify_core::rule::rule(id) {
        Some(m) => SarifRule {
            id: id.to_string(),
            name: m.name.to_string(),
            short_description: Text { text: m.description.to_string() },
            full_description: Some(Text { text: m.description.to_string() }),
            help: Some(Text { text: m.remediation.to_string() }),
            default_configuration: Some(DefaultConfiguration {
                level: level_of(m.default_severity),
            }),
            properties: Some(RuleProperties {
                category: m.category,
                tags: m.tags.to_vec(),
            }),
        },
        None => SarifRule {
            id: id.to_string(),
            name: id.to_string(),
            short_description: Text { text: "Stratify finding.".to_string() },
            full_description: None,
            help: None,
            default_configuration: None,
            properties: None,
        },
    }
}
```

and in `build`, replace the `.map(|id| RuleMeta { ... })` with `.map(|id| sarif_rule(id))`.

`CHANGELOG.md` under `## [Unreleased]` `### Added`:
```markdown
- A rule registry: each rule carries a name, category, default severity,
  tags, and fix guidance. SARIF output now includes `help` text,
  `defaultConfiguration`, and tags for every rule.
```

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test --workspace`
Expected: all pass.

- [ ] **Step 5: Lint and commit**

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings
git add crates/stratify-core crates/stratify-report/src/sarif.rs CHANGELOG.md
git commit -m "feat(core): add a rule registry and surface it in SARIF"
```

---

### Task 4: Finding evidence

**Files:**
- Modify: `crates/stratify-core/src/finding.rs`, `crates/stratify-core/src/lib.rs`
- Modify: `crates/stratify-analysis/src/{cycles,duplication,boundaries,deadcode,complexity,hotspot}.rs`
- Modify: `crates/stratify-report/src/sarif.rs`, `crates/stratify-report/src/lib.rs`
- Modify: every other `Finding { ... }` literal (`crates/stratify-cli/src/run.rs`, `crates/stratify-telemetry/src/lib.rs`). Find them with `grep -rn 'Finding {' crates | grep -v 'struct Finding'`.
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: Task 3 registry (not required, only for context).
- Produces in `stratify_core`:
  - `pub struct Related { pub span: Span, pub role: String, pub message: String }`
  - `Finding` gains `pub related: Vec<Related>` and `pub reasons: Vec<String>`, both `#[serde(default, skip_serializing_if = "Vec::is_empty")]`.
  - `Finding::new(rule: impl Into<String>, severity: Severity, message: impl Into<String>, span: Span, confidence: Confidence) -> Finding`
  - `Finding::with_related(self, span: Span, role: impl Into<String>, message: impl Into<String>) -> Finding`
  - `Finding::with_reason(self, reason: impl Into<String>) -> Finding`
  - `Related` re-exported from `stratify_core`.

- [ ] **Step 1: Write the failing tests**

In `crates/stratify-core/src/finding.rs` tests:

```rust
    fn sp(file: &str, line: usize) -> Span {
        Span { file: file.into(), start_byte: 0, end_byte: 1, start_line: line }
    }

    #[test]
    fn finding_without_evidence_omits_new_keys() {
        let f = Finding::new("dead_code", Severity::Warning, "m", sp("a.rs", 1), Confidence::Certain);
        let v = serde_json::to_value(&f).unwrap();
        let mut keys: Vec<&String> = v.as_object().unwrap().keys().collect();
        keys.sort();
        assert_eq!(keys, vec!["confidence", "message", "rule", "severity", "span"]);
    }

    #[test]
    fn evidence_round_trips() {
        let f = Finding::new("cycle", Severity::Warning, "m", sp("a.rs", 1), Confidence::Certain)
            .with_related(sp("b.rs", 3), "cycle_member", "imports a.rs")
            .with_reason("each file imports the next");
        let json = serde_json::to_string(&f).unwrap();
        let back: Finding = serde_json::from_str(&json).unwrap();
        assert_eq!(back, f);
        assert_eq!(back.related[0].role, "cycle_member");
    }

    #[test]
    fn v1_report_without_evidence_still_parses() {
        let json = r#"{"schema_version":1,"findings":[{"rule":"dead_code","severity":"warning","message":"m","span":{"file":"a","start_byte":0,"end_byte":1,"start_line":1},"confidence":"certain"}]}"#;
        let r: Report = serde_json::from_str(json).unwrap();
        assert!(r.findings[0].related.is_empty());
        assert!(r.findings[0].reasons.is_empty());
    }
```

In `crates/stratify-analysis/src/cycles.rs` tests:

```rust
    #[test]
    fn cycle_lists_every_member_as_related() {
        let mut g = IrGraph::new();
        let a = file_sym(&mut g, "a.rb");
        let b = file_sym(&mut g, "b.rb");
        let c = file_sym(&mut g, "c.rb");
        add_import(&mut g, a, "b.rb");
        add_import(&mut g, b, "c.rb");
        add_import(&mut g, c, "a.rb");
        let f = &analyze(&g)[0];
        let files: Vec<&str> = f.related.iter().map(|r| r.span.file.as_str()).collect();
        assert_eq!(files, vec!["a.rb", "b.rb", "c.rb"]);
        assert!(f.related.iter().all(|r| r.role == "cycle_member"));
        assert_eq!(f.related[2].message, "imports a.rb");
        assert!(!f.reasons.is_empty());
    }
```

In `crates/stratify-analysis/src/duplication.rs` tests, add a test reusing the existing test helpers in that module (read them first). It builds a graph with the same token run in three files and asserts:

```rust
        // one finding, anchored in the first file, listing both other copies
        assert_eq!(findings.len(), 1);
        let sites: Vec<&str> = findings[0].related.iter().map(|r| r.span.file.as_str()).collect();
        assert_eq!(sites.len(), 2);
        assert!(findings[0].related.iter().all(|r| r.role == "clone"));
```

In `crates/stratify-analysis/src/boundaries.rs` tests, extend the existing violation test (read it first) with:

```rust
        assert_eq!(findings[0].related.len(), 1);
        assert_eq!(findings[0].related[0].role, "imported_file");
```

and assert `related[0].span.file` equals the imported file of that test.

In `crates/stratify-analysis/src/deadcode.rs` tests, add:

```rust
    #[test]
    fn certain_dead_code_explains_why() {
        let mut g = IrGraph::new();
        func(&mut g, "orphan");
        let f = &analyze(&g, DeadCodeMode::Library)[0];
        assert_eq!(f.reasons, vec!["no call from any entrypoint reaches this function"]);
    }

    #[test]
    fn public_library_symbol_explains_why() {
        let mut g = IrGraph::new();
        sym_with_visibility(&mut g, "api", Visibility::Public);
        let f = &analyze(&g, DeadCodeMode::Library)[0];
        assert!(f.reasons.iter().any(|r| r.contains("public in library mode")), "{:?}", f.reasons);
    }
```

In `crates/stratify-report/src/sarif.rs` tests:

```rust
    #[test]
    fn related_spans_become_related_locations() {
        let f = finding("cycle", Severity::Warning, "a.rb", 1)
            .with_related(Span { file: "b.rb".into(), start_byte: 0, end_byte: 1, start_line: 4 }, "cycle_member", "imports a.rb")
            .with_reason("each file imports the next");
        let v: serde_json::Value = serde_json::from_str(&render(&Report::new(vec![f]))).unwrap();
        let rel = &v["runs"][0]["results"][0]["relatedLocations"][0];
        assert_eq!(rel["id"], 0);
        assert_eq!(rel["physicalLocation"]["artifactLocation"]["uri"], "b.rb");
        assert_eq!(rel["physicalLocation"]["region"]["startLine"], 4);
        assert_eq!(rel["message"]["text"], "imports a.rb");
        assert_eq!(v["runs"][0]["results"][0]["properties"]["reasons"][0], "each file imports the next");
    }

    #[test]
    fn result_without_evidence_has_no_related_keys() {
        let v: serde_json::Value = serde_json::from_str(&render(&Report::new(vec![finding("cycle", Severity::Warning, "a.rb", 1)]))).unwrap();
        let r = v["runs"][0]["results"][0].as_object().unwrap();
        assert!(!r.contains_key("relatedLocations"));
        assert!(!r.contains_key("properties"));
    }
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test --workspace`
Expected: compile errors (`Finding::new`, `related`, `reasons` not found).

- [ ] **Step 3: Implement the core type**

In `finding.rs`:

```rust
/// A secondary location backing a finding: another clone site, another file
/// in a cycle, the file a forbidden import points at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Related {
    pub span: Span,
    /// Machine-readable role, e.g. "clone", "cycle_member", "imported_file".
    pub role: String,
    pub message: String,
}

/// One reported problem. `rule` identifies the analysis, e.g. "dead_code".
/// `related` and `reasons` are optional evidence. Both are omitted from JSON
/// when empty, so a finding without evidence serializes exactly as in v0.6.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub rule: String,
    pub severity: Severity,
    pub message: String,
    pub span: Span,
    pub confidence: Confidence,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<Related>,
    /// Ordered reason chain: why the analysis flagged this.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<String>,
}

impl Finding {
    pub fn new(
        rule: impl Into<String>,
        severity: Severity,
        message: impl Into<String>,
        span: Span,
        confidence: Confidence,
    ) -> Self {
        Finding {
            rule: rule.into(),
            severity,
            message: message.into(),
            span,
            confidence,
            related: Vec::new(),
            reasons: Vec::new(),
        }
    }

    pub fn with_related(
        mut self,
        span: Span,
        role: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        self.related.push(Related {
            span,
            role: role.into(),
            message: message.into(),
        });
        self
    }

    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reasons.push(reason.into());
        self
    }
}
```

Update `lib.rs`: `pub use finding::{Finding, Related, Report, Severity};`. Leave `SCHEMA_VERSION` at 1 and add a doc line above it: `/// Additive optional fields do not bump this. stratify-judge skips judgment for any version it does not know.`

- [ ] **Step 4: Convert every literal and populate evidence**

Replace each production `Finding { ... }` literal with `Finding::new(...)`. In test code, either use `Finding::new` or add `related: Vec::new(), reasons: Vec::new()`.

Evidence per analysis:

- `cycles.rs`: for the cycle `[n0, n1, ..., nk]` (fqns) with display files `files`, attach one related per member, in cycle order: span = `spans.get(fqn)` (fall back to the same zero span used for the primary), role `"cycle_member"`, message `format!("imports {}", files[(i + 1) % files.len()])`. Add reason `"each file in the cycle imports the next, and the last imports the first"`.
- `duplication.rs`: for each `o` in `valid_others` sorted ascending, related span = `Span { file: tokens[o].file.clone(), start_byte: tokens[o].start_byte, end_byte: tokens[o + k - 1].end_byte, start_line: tokens[o].start_line }`, role `"clone"`, message `format!("same {k}-token block")`. Add reason `format!("{} normalized tokens match exactly; identifiers, numbers, and strings are compared by kind", k)`.
- `boundaries.rs`: related = `span_of.get(tgt)` (fall back to a zero span at `tgt`), role `"imported_file"`, message `format!("{tgt} is in layer `{tgt_layer}`")`. Reason `format!("stratify.toml forbids layer `{src_layer}` from importing layer `{tgt_layer}`")`.
- `deadcode.rs`:
  - Certain branch: reason `"no call from any entrypoint reaches this function"`.
  - Likely branch: if `reached_any.contains(&s.id)` push `"reached only through a name-matched call, which cannot prove this exact function is the target"`. If `s.visibility == Visibility::Public && mode == DeadCodeMode::Library` push `"public in library mode: code outside this scan may call it"`. Both reasons are pushed when both hold.
- `complexity.rs`: reason `format!("{cx} independent paths exceed the threshold of {threshold}")`.
- `hotspot.rs`: read the function first, then add one reason naming the complexity and churn values the message already uses.

- [ ] **Step 5: SARIF related locations**

In `sarif.rs`, extend `SarifResult`:

```rust
    #[serde(rename = "relatedLocations", skip_serializing_if = "Vec::is_empty")]
    related_locations: Vec<RelatedLocation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    properties: Option<ResultProperties>,
```

with

```rust
#[derive(Serialize)]
struct RelatedLocation {
    id: usize,
    #[serde(rename = "physicalLocation")]
    physical_location: PhysicalLocation,
    message: Text,
}

#[derive(Serialize)]
struct ResultProperties {
    reasons: Vec<String>,
}
```

Fill `related_locations` from `f.related` with `id` = index, and `properties` = `Some(ResultProperties { reasons: f.reasons.clone() })` only when `f.reasons` is non-empty.

`CHANGELOG.md` under `## [Unreleased]` `### Added`:
```markdown
- Findings carry evidence: `related` locations (every file in a cycle, every
  clone site, the file a forbidden import points at) and a `reasons` chain.
  Both fields are optional and omitted when empty, so the JSON schema stays
  at version 1. SARIF maps them to `relatedLocations` and
  `properties.reasons`.
```

- [ ] **Step 6: Run the tests and confirm they pass**

Run: `cargo test --workspace`
Expected: all pass.

Check judge compatibility: `cargo run -q -p stratify-cli -- check crates/stratify-cli/tests/sample-java --format json` must still show `"schema_version": 1`, and its findings must have no `related` or `reasons` keys when the analysis attached none.

- [ ] **Step 7: Lint and commit**

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings
git add -A crates CHANGELOG.md
git commit -m "feat(core): attach related locations and reasons to findings"
```

---

### Task 5: Line-level suppression comments

**Files:**
- Create: `crates/stratify-analysis/src/suppress.rs`
- Modify: `crates/stratify-analysis/src/lib.rs`
- Modify: `crates/stratify-cli/src/run.rs`
- Create: `crates/stratify-cli/tests/sample-suppress/app.rb`
- Create: `crates/stratify-cli/tests/e2e_suppress.rs`
- Modify: `README.md`, `CHANGELOG.md`

**Interfaces:**
- Consumes: `Finding` with `rule` and `span.start_line` (1-based).
- Produces: `pub fn is_suppressed(source: &str, line: usize, rule: &str) -> bool` and `pub fn apply(findings: Vec<Finding>, read: impl FnMut(&str) -> Option<String>) -> (Vec<Finding>, usize)` in `stratify_analysis::suppress`. The `usize` is the count removed.

Syntax: a comment containing `stratify-ignore: <rule>[, <rule>...]` on the finding's start line or on the line directly above. The marker works inside any comment syntax (`//`, `#`, `/* */`, `--`), because the check reads the raw line text. A rule id is required. `stratify-ignore:` with no rule, or `stratify-ignore: *`, suppresses nothing.

- [ ] **Step 1: Write the failing tests**

Create `crates/stratify-analysis/src/suppress.rs` with the test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use stratify_core::ir::Span;
    use stratify_core::{Confidence, Finding, Severity};

    const SRC: &str = "fn a() {}\n// stratify-ignore: dead_code\nfn b() {}\nfn c() {} # stratify-ignore: complexity, dead_code\n// stratify-ignore:\nfn d() {}\n// stratify-ignore: *\nfn e() {}\n";

    #[test]
    fn marker_on_line_above_suppresses() {
        assert!(is_suppressed(SRC, 3, "dead_code"));
    }

    #[test]
    fn marker_on_same_line_with_rule_list_suppresses() {
        assert!(is_suppressed(SRC, 4, "dead_code"));
        assert!(is_suppressed(SRC, 4, "complexity"));
    }

    #[test]
    fn marker_for_other_rule_does_not_suppress() {
        assert!(!is_suppressed(SRC, 3, "duplication"));
    }

    #[test]
    fn marker_without_rule_or_wildcard_does_not_suppress() {
        assert!(!is_suppressed(SRC, 6, "dead_code"));
        assert!(!is_suppressed(SRC, 8, "dead_code"));
    }

    #[test]
    fn unmarked_line_and_out_of_range_lines_are_not_suppressed() {
        assert!(!is_suppressed(SRC, 1, "dead_code"));
        assert!(!is_suppressed(SRC, 0, "dead_code"));
        assert!(!is_suppressed(SRC, 999, "dead_code"));
    }

    #[test]
    fn rule_prefix_is_not_a_match() {
        assert!(!is_suppressed("x // stratify-ignore: dead\n", 1, "dead_code"));
    }

    fn f(rule: &str, file: &str, line: usize) -> Finding {
        Finding::new(rule, Severity::Warning, "m", Span { file: file.into(), start_byte: 0, end_byte: 1, start_line: line }, Confidence::Certain)
    }

    #[test]
    fn apply_drops_suppressed_and_counts_them() {
        let findings = vec![f("dead_code", "a.rs", 3), f("dead_code", "a.rs", 1), f("dead_code", "missing.rs", 3)];
        let (kept, removed) = apply(findings, |file| (file == "a.rs").then(|| SRC.to_string()));
        assert_eq!(removed, 1);
        assert_eq!(kept.len(), 2);
        assert!(kept.iter().any(|k| k.span.file == "missing.rs"), "unreadable file keeps its finding");
    }
}
```

Add `pub mod suppress;` to `crates/stratify-analysis/src/lib.rs`.

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p stratify-analysis suppress`
Expected: compile error, `is_suppressed` not found.

- [ ] **Step 3: Implement**

Above the tests in `suppress.rs`:

```rust
use std::collections::HashMap;
use stratify_core::Finding;

const MARKER: &str = "stratify-ignore:";

/// True if `line` (1-based) or the line above carries a
/// `stratify-ignore: <rules>` marker naming `rule`.
pub fn is_suppressed(source: &str, line: usize, rule: &str) -> bool {
    if line == 0 {
        return false;
    }
    let lines: Vec<&str> = source.lines().collect();
    let mut candidates = Vec::new();
    if let Some(l) = lines.get(line - 1) {
        candidates.push(*l);
    }
    if line >= 2 {
        if let Some(l) = lines.get(line - 2) {
            candidates.push(*l);
        }
    }
    candidates.iter().any(|l| names_rule(l, rule))
}

fn names_rule(line: &str, rule: &str) -> bool {
    let Some(pos) = line.find(MARKER) else {
        return false;
    };
    line[pos + MARKER.len()..]
        .split(|c: char| c == ',' || c.is_whitespace())
        .map(|t| t.trim_end_matches("*/"))
        .any(|t| t == rule)
}

/// Remove suppressed findings. `read` returns a file's source by the
/// finding's relative path, or None when unreadable (the finding stays).
/// Each file is read once.
pub fn apply(
    findings: Vec<Finding>,
    mut read: impl FnMut(&str) -> Option<String>,
) -> (Vec<Finding>, usize) {
    let mut cache: HashMap<String, Option<String>> = HashMap::new();
    let mut kept = Vec::with_capacity(findings.len());
    let mut removed = 0;
    for f in findings {
        let src = cache
            .entry(f.span.file.clone())
            .or_insert_with(|| read(&f.span.file));
        match src {
            Some(s) if is_suppressed(s, f.span.start_line, &f.rule) => removed += 1,
            _ => kept.push(f),
        }
    }
    (kept, removed)
}
```

In `run.rs` `analyze_repo_with_stats`, after the boundary findings are added and before `scan_stats`:

```rust
    let (findings, _suppressed) = stratify_analysis::suppress::apply(findings, |rel| {
        std::fs::read_to_string(root.join(rel)).ok()
    });
```

(`findings` must be declared `let mut findings` already. Rebinding is fine.)

- [ ] **Step 4: E2E fixture and test**

`crates/stratify-cli/tests/sample-suppress/app.rb`:
```ruby
def main
  puts "hi"
end

# stratify-ignore: dead_code
def kept_for_reflection
  1
end

def really_unused
  2
end

main
```

`crates/stratify-cli/tests/e2e_suppress.rs`:
```rust
use std::path::Path;

#[test]
fn suppression_comment_hides_only_the_named_finding() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/sample-suppress");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_stratify"))
        .arg("check")
        .arg(&dir)
        .arg("--format")
        .arg("json")
        .output()
        .expect("run stratify binary");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(!stdout.contains("kept_for_reflection"), "stdout: {stdout}");
    assert!(stdout.contains("really_unused"), "stdout: {stdout}");
}
```

Run `cargo test -p stratify-cli --test e2e_suppress` before Step 3 wiring to see it fail, then after to see it pass. If `main` itself is reported (Ruby entrypoint handling), adjust only the fixture, never the assertion targets.

- [ ] **Step 5: Docs**

`README.md`, add a section `## Suppress a finding` after "Output formats":

````markdown
## Suppress a finding

Put `stratify-ignore: <rule>` in a comment on the finding's line or the line above. Name one rule or a comma-separated list. A rule id is required, so a suppression never hides more than you meant.

```ruby
# stratify-ignore: dead_code
def called_by_the_framework
end
```

Rule ids: `dead_code`, `duplication`, `complexity`, `hotspot`, `cycle`, `boundary`. A finding's `rule` field in JSON output shows its id.
````

`CHANGELOG.md` under `## [Unreleased]` `### Added`:
```markdown
- Line-level suppression: a `stratify-ignore: <rule>` comment on a
  finding's line or the line above hides that rule's finding there.
```

- [ ] **Step 6: Run all tests, lint, commit**

```bash
cargo test --workspace
cargo fmt --all && cargo clippy --all-targets -- -D warnings
git add crates/stratify-analysis/src/suppress.rs crates/stratify-analysis/src/lib.rs \
  crates/stratify-cli/src/run.rs crates/stratify-cli/tests/sample-suppress \
  crates/stratify-cli/tests/e2e_suppress.rs README.md CHANGELOG.md
git commit -m "feat(cli): suppress a finding with a stratify-ignore comment"
```

---

### Task 6: Baseline file and new-findings gate

**Files:**
- Create: `crates/stratify-cli/src/baseline.rs`
- Modify: `crates/stratify-cli/src/main.rs`
- Create: `crates/stratify-cli/tests/e2e_baseline.rs`
- Modify: `action.yml`, `README.md`, `CHANGELOG.md`

**Interfaces:**
- Consumes: `stratify_core::{Finding, Report}`.
- Produces in `crates/stratify-cli/src/baseline.rs`:
  - `pub fn fingerprint(f: &Finding) -> String`
  - `pub fn write(report: &Report) -> String` (JSON text)
  - `pub fn filter_new(report: Report, baseline_json: &str) -> Result<(Report, usize), String>` returning only findings absent from the baseline and the hidden count.
- CLI: `stratify check --write-baseline <FILE>` writes the baseline and exits 0. `stratify check --baseline <FILE>` reports and gates only new findings. A missing or invalid baseline file prints `stratify: baseline <path>: <reason>` to stderr and exits with code 2.

Fingerprint: `rule`, `span.file`, and `message` with every run of ASCII digits replaced by `#`, joined by `\u{1f}`. Line numbers are not part of it, so a finding that moves stays baselined. Matching counts multiplicity: if the baseline holds a fingerprint twice and the scan finds it three times, one finding is new.

Baseline file format:

```json
{
  "version": 1,
  "fingerprints": ["dead_code\u001fsrc/a.rs\u001funused function `f`"]
}
```

- [ ] **Step 1: Write the failing unit tests**

Create `crates/stratify-cli/src/baseline.rs` with tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use stratify_core::ir::Span;
    use stratify_core::{Confidence, Finding, Report, Severity};

    fn f(rule: &str, file: &str, line: usize, msg: &str) -> Finding {
        Finding::new(rule, Severity::Warning, msg, Span { file: file.into(), start_byte: 0, end_byte: 1, start_line: line }, Confidence::Certain)
    }

    #[test]
    fn fingerprint_ignores_line_and_digits() {
        let a = f("duplication", "a.rs", 3, "duplicated code block (>= 100 tokens) also at b.rs:10");
        let b = f("duplication", "a.rs", 13, "duplicated code block (>= 100 tokens) also at b.rs:20");
        assert_eq!(fingerprint(&a), fingerprint(&b));
    }

    #[test]
    fn fingerprint_separates_rule_and_file() {
        let a = f("dead_code", "a.rs", 1, "unused function `x`");
        assert_ne!(fingerprint(&a), fingerprint(&f("dead_code", "b.rs", 1, "unused function `x`")));
        assert_ne!(fingerprint(&a), fingerprint(&f("complexity", "a.rs", 1, "unused function `x`")));
    }

    #[test]
    fn moved_finding_stays_baselined() {
        let base = write(&Report::new(vec![f("dead_code", "a.rs", 5, "unused function `x`")]));
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
        let (out, hidden) = filter_new(Report::new(vec![f("cycle", "a.rs", 1, "circular dependency: a -> b")]), &base).unwrap();
        assert_eq!(out.findings.len(), 1);
        assert_eq!(hidden, 0);
    }

    #[test]
    fn invalid_baseline_is_an_error() {
        assert!(filter_new(Report::new(vec![]), "not json").is_err());
        assert!(filter_new(Report::new(vec![]), r#"{"version": 2, "fingerprints": []}"#).is_err());
    }
}
```

Add `mod baseline;` to `main.rs`.

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p stratify-cli baseline`
Expected: compile error, `fingerprint` not found.

- [ ] **Step 3: Implement `baseline.rs`**

```rust
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
    let file = BaselineFile { version: VERSION, fingerprints };
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
```

Add `serde = { workspace = true }` to `[dependencies]` in `crates/stratify-cli/Cargo.toml`.

- [ ] **Step 4: Wire the CLI flags**

In `main.rs`, add to `Command::Check` (and add `baseline` and `write_baseline` to the `Command::Check { .. }` destructuring in `main`):

```rust
        /// Report and gate only findings absent from this baseline file.
        #[arg(long, conflicts_with = "write_baseline")]
        baseline: Option<PathBuf>,
        /// Write every current finding to this baseline file and exit 0.
        #[arg(long)]
        write_baseline: Option<PathBuf>,
```

After `analyze_repo_with_stats` succeeds and before rendering:

```rust
            if let Some(out) = &write_baseline {
                if let Err(e) = std::fs::write(out, baseline::write(&report)) {
                    eprintln!("stratify: baseline {}: {e}", out.display());
                    return ExitCode::from(2);
                }
                eprintln!(
                    "stratify: wrote {} finding(s) to {}",
                    report.findings.len(),
                    out.display()
                );
                return ExitCode::SUCCESS;
            }
            let report = match &baseline {
                None => report,
                Some(p) => {
                    let text = match std::fs::read_to_string(p) {
                        Ok(t) => t,
                        Err(e) => {
                            eprintln!("stratify: baseline {}: {e}", p.display());
                            return ExitCode::from(2);
                        }
                    };
                    match baseline::filter_new(report, &text) {
                        Ok((r, hidden)) => {
                            eprintln!("stratify: {hidden} baselined finding(s) hidden");
                            r
                        }
                        Err(e) => {
                            eprintln!("stratify: baseline {}: {e}", p.display());
                            return ExitCode::from(2);
                        }
                    }
                }
            };
```

Telemetry keeps receiving the filtered report. That is intended: dashboards track what the gate sees.

- [ ] **Step 5: E2E test**

`crates/stratify-cli/tests/e2e_baseline.rs`:

```rust
use std::path::Path;
use std::process::Command;

fn stratify() -> Command {
    Command::new(env!("CARGO_BIN_EXE_stratify"))
}

fn temp_repo(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("stratify-e2e-baseline-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("app.rb"), "def main\nend\n\ndef old_dead\nend\n\nmain\n").unwrap();
    dir
}

#[test]
fn baseline_hides_old_findings_and_gates_new_ones() {
    let repo = temp_repo("gate");
    let base = repo.join("baseline.json");

    let w = stratify().arg("check").arg(&repo).arg("--write-baseline").arg(&base).output().unwrap();
    assert!(w.status.success(), "{w:?}");

    // Same code, shifted down two lines: still baselined, gate passes.
    std::fs::write(repo.join("app.rb"), "# a\n# b\ndef main\nend\n\ndef old_dead\nend\n\nmain\n").unwrap();
    let ok = stratify().arg("check").arg(&repo).arg("--baseline").arg(&base).arg("--fail-on").arg("warning").output().unwrap();
    assert!(ok.status.success(), "{}", String::from_utf8_lossy(&ok.stdout));

    // A new dead function fails the gate and is the only finding shown.
    std::fs::write(repo.join("app.rb"), "def main\nend\n\ndef old_dead\nend\n\ndef new_dead\nend\n\nmain\n").unwrap();
    let bad = stratify().arg("check").arg(&repo).arg("--baseline").arg(&base).arg("--fail-on").arg("warning").output().unwrap();
    let out = String::from_utf8_lossy(&bad.stdout);
    assert!(!bad.status.success());
    assert!(out.contains("new_dead"), "{out}");
    assert!(!out.contains("old_dead"), "{out}");
}

#[test]
fn missing_or_invalid_baseline_exits_2() {
    let repo = temp_repo("bad");
    let missing = stratify().arg("check").arg(&repo).arg("--baseline").arg(repo.join("nope.json")).output().unwrap();
    assert_eq!(missing.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("baseline"));

    std::fs::write(repo.join("bad.json"), "{").unwrap();
    let invalid = stratify().arg("check").arg(&repo).arg("--baseline").arg(repo.join("bad.json")).output().unwrap();
    assert_eq!(invalid.status.code(), Some(2));
    let _ = Path::new(&repo);
}
```

If the Ruby fixture's dead-code shape does not produce Warning-level findings for `old_dead` and `new_dead`, read `crates/stratify-cli/tests/sample-ruby` and copy its working shape. Keep the assertions.

- [ ] **Step 6: Action input and docs**

`action.yml`, add an input:

```yaml
  baseline:
    description: "Optional path to a baseline file written by `stratify check --write-baseline`. When set, only findings absent from it are reported and gated."
    default: ''
```

and change the run step to:

```yaml
      run: |
        args=(check "${{ inputs.path }}" --fail-on "${{ inputs.fail-on }}" --format "${{ inputs.format }}")
        if [ -n "${{ inputs.baseline }}" ]; then
          args+=(--baseline "${{ inputs.baseline }}")
        fi
        stratify "${args[@]}"
```

`README.md`: add a row to the Action inputs table (`| \`baseline\` | \`''\` | Baseline file. Only findings absent from it are reported and gated. |`) and a section after "CI quality gate":

````markdown
## Adopt on an existing codebase (baseline)

Record today's findings once, commit the file, and gate only on new ones:

```sh
stratify check . --write-baseline stratify-baseline.json
stratify check . --baseline stratify-baseline.json --fail-on warning
```

A baselined finding stays baselined when code above it moves. A missing or unreadable baseline file exits with code 2, so a broken path never passes the gate.
````

`CHANGELOG.md` under `## [Unreleased]` `### Added`:
```markdown
- `--write-baseline <file>` and `--baseline <file>`: record current findings,
  then report and gate only new ones. The GitHub Action takes a `baseline`
  input.
```

- [ ] **Step 7: Run all tests, lint, commit**

```bash
cargo test --workspace
cargo fmt --all && cargo clippy --all-targets -- -D warnings
git add crates/stratify-cli action.yml README.md CHANGELOG.md Cargo.lock
git commit -m "feat(cli): gate on new findings with a baseline file"
```

---

### Task 7: Labelled corpus and precision script

**Files:**
- Create: `corpus/README.md`, `corpus/repos.json`, `corpus/labels/stratify.json`
- Create: `scripts/precision.py`, `scripts/test_precision.py`

**Interfaces:**
- Consumes: `stratify check <dir> --format json` output.
- Produces: `python3 scripts/precision.py --stratify <bin> [--work <dir>] [--json]` printing precision per rule and language.

Label format, one file per corpus repo, `corpus/labels/<name>.json`:

```json
[
  {"rule": "dead_code", "file": "crates/stratify-cli/src/lsp.rs", "line": 188, "match": "run_stdio", "label": "fp", "note": "called from main.rs:124"}
]
```

`label` is `tp` (real problem) or `fp` (false positive). A finding matches a label when `rule` and `file` are equal, `line` equals `span.start_line`, and `match` is a substring of `message`.

`corpus/repos.json`:

```json
[
  {"name": "stratify", "url": "https://github.com/stratify-dev/stratify", "commit": "a809171"}
]
```

- [ ] **Step 1: Write the failing test**

`scripts/test_precision.py`:

```python
import unittest

from precision import language_of, match_label, tally


def finding(rule, file, line, message):
    return {"rule": rule, "message": message, "span": {"file": file, "start_line": line}}


class PrecisionTest(unittest.TestCase):
    def test_language_of(self):
        self.assertEqual(language_of("a/b.rs"), "rust")
        self.assertEqual(language_of("x.tsx"), "typescript")
        self.assertEqual(language_of("README.md"), "other")

    def test_match_label_requires_rule_file_line_and_substring(self):
        labels = [{"rule": "dead_code", "file": "a.rs", "line": 3, "match": "foo", "label": "fp"}]
        self.assertEqual(match_label(finding("dead_code", "a.rs", 3, "unused function `foo`"), labels)["label"], "fp")
        self.assertIsNone(match_label(finding("dead_code", "a.rs", 4, "unused function `foo`"), labels))
        self.assertIsNone(match_label(finding("cycle", "a.rs", 3, "foo"), labels))
        self.assertIsNone(match_label(finding("dead_code", "a.rs", 3, "unused function `bar`"), labels))

    def test_tally_counts_and_precision(self):
        labels = [
            {"rule": "dead_code", "file": "a.rs", "line": 1, "match": "x", "label": "tp"},
            {"rule": "dead_code", "file": "a.rs", "line": 2, "match": "y", "label": "fp"},
        ]
        findings = [
            finding("dead_code", "a.rs", 1, "x"),
            finding("dead_code", "a.rs", 2, "y"),
            finding("dead_code", "a.rs", 9, "z"),
        ]
        rows = tally(findings, labels)
        self.assertEqual(rows[("dead_code", "rust")], {"tp": 1, "fp": 1, "unlabelled": 1})


if __name__ == "__main__":
    unittest.main()
```

Run: `cd scripts && python3 -m unittest test_precision -v`
Expected: `ModuleNotFoundError: No module named 'precision'`.

- [ ] **Step 2: Implement `scripts/precision.py`**

```python
#!/usr/bin/env python3
"""Measure Stratify precision per rule and language against the labelled corpus.

Usage: python3 scripts/precision.py --stratify target/release/stratify [--work DIR] [--json]

For each repo in corpus/repos.json: clone it into the work dir (once), check out
the pinned commit, scan it, and compare findings to corpus/labels/<name>.json.
Precision = tp / (tp + fp). Unlabelled findings are counted but not scored.
"""
import argparse
import json
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EXTENSIONS = {
    "java": "java", "rb": "ruby", "ts": "typescript", "tsx": "typescript",
    "mts": "typescript", "cts": "typescript", "py": "python", "pyi": "python",
    "go": "go", "rs": "rust",
}


def language_of(path):
    ext = path.rsplit(".", 1)[-1] if "." in path else ""
    return EXTENSIONS.get(ext, "other")


def match_label(finding, labels):
    span = finding["span"]
    for label in labels:
        if (label["rule"] == finding["rule"] and label["file"] == span["file"]
                and label["line"] == span["start_line"] and label["match"] in finding["message"]):
            return label
    return None


def tally(findings, labels):
    rows = defaultdict(lambda: {"tp": 0, "fp": 0, "unlabelled": 0})
    for f in findings:
        key = (f["rule"], language_of(f["span"]["file"]))
        label = match_label(f, labels)
        rows[key][label["label"] if label else "unlabelled"] += 1
    return dict(rows)


def checkout(repo, work):
    dest = work / repo["name"]
    if not dest.exists():
        subprocess.run(["git", "clone", "--quiet", repo["url"], str(dest)], check=True)
    subprocess.run(["git", "-C", str(dest), "checkout", "--quiet", repo["commit"]], check=True)
    return dest


def scan(stratify, path):
    out = subprocess.run([stratify, "check", str(path), "--format", "json"],
                         check=True, capture_output=True, text=True).stdout
    return json.loads(out)["findings"]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--stratify", required=True)
    parser.add_argument("--work", default=str(ROOT / "target" / "corpus"))
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()

    work = Path(args.work)
    work.mkdir(parents=True, exist_ok=True)
    repos = json.loads((ROOT / "corpus" / "repos.json").read_text())
    totals = defaultdict(lambda: {"tp": 0, "fp": 0, "unlabelled": 0})
    for repo in repos:
        labels = json.loads((ROOT / "corpus" / "labels" / f"{repo['name']}.json").read_text())
        for key, row in tally(scan(args.stratify, checkout(repo, work)), labels).items():
            for k, v in row.items():
                totals[key][k] += v

    result = []
    for (rule, lang), row in sorted(totals.items()):
        scored = row["tp"] + row["fp"]
        precision = round(row["tp"] / scored, 3) if scored else None
        result.append({"rule": rule, "language": lang, **row, "precision": precision})

    if args.json:
        json.dump(result, sys.stdout, indent=2)
        print()
        return
    print(f"{'rule':<12} {'language':<11} {'tp':>4} {'fp':>4} {'unlab':>6} {'precision':>9}")
    for r in result:
        p = "n/a" if r["precision"] is None else f"{r['precision']:.0%}"
        print(f"{r['rule']:<12} {r['language']:<11} {r['tp']:>4} {r['fp']:>4} {r['unlabelled']:>6} {p:>9}")


if __name__ == "__main__":
    main()
```

Run: `cd scripts && python3 -m unittest test_precision -v`
Expected: 3 tests pass.

- [ ] **Step 3: Label the self-scan dead-code findings**

Check out commit `a809171` in a temp worktree (`git worktree add /tmp/stratify-a809171 a809171`), build the current branch's binary (`cargo build --release -p stratify-cli`), and scan the worktree with `--format json`. For each `dead_code` finding, find its caller with `grep -rn "<name>(" <worktree>/crates --include=*.rs` and record a label. Expected result: 15 findings, all `fp`, each with a `note` naming the call site (file and line) or `test-only helper under #[cfg(test)]`. Write them to `corpus/labels/stratify.json`. Do not label other rules in this task. If a finding has no caller, label it `tp` and say so in your report. Remove the worktree afterwards (`git worktree remove /tmp/stratify-a809171`).

- [ ] **Step 4: Write `corpus/README.md`**

```markdown
# Labelled corpus

Real repositories at pinned commits, with Stratify findings marked true (`tp`)
or false (`fp`). `scripts/precision.py` scans each repo and prints precision
per rule and language.

    cargo build --release -p stratify-cli
    python3 scripts/precision.py --stratify target/release/stratify

## Files

- `repos.json`: name, clone URL, pinned commit.
- `labels/<name>.json`: one entry per labelled finding: `rule`, `file`,
  `line`, `match` (a substring of the message), `label`, `note`.

## Rules for labelling

- `tp` means a maintainer would act on the finding.
- `fp` means the finding is wrong. Say why in `note`, with the call site or
  config that proves it.
- Unlabelled findings count toward `unlab` and do not affect precision.
- A label is tied to a pinned commit. Moving the commit means relabelling.

## Coverage

| Repo | Language | Rules labelled |
|------|----------|----------------|
| stratify @ a809171 | rust | dead_code |

Each language needs one or more labelled repos before the roadmap's 80 percent
precision gate can apply to it.
```

- [ ] **Step 5: Run the script end to end and commit**

Run: `python3 scripts/precision.py --stratify target/release/stratify`
Expected: a `dead_code rust` row with `fp` 15 (or the count from Step 3) and precision `0%`. Paste the table in your report.

```bash
git add corpus scripts/precision.py scripts/test_precision.py
git commit -m "feat(corpus): add a labelled corpus and a precision script"
```

---

### Task 8: Scan-time and memory benchmark

**Files:**
- Create: `scripts/bench.sh`
- Create: `docs/benchmarks.md`

**Interfaces:**
- Consumes: a built `stratify` binary.
- Produces: `scripts/bench.sh <stratify-bin> <repo-path>` printing wall seconds, peak RSS in MB, file count, and line count for the six languages.

- [ ] **Step 1: Write the script**

```bash
#!/usr/bin/env bash
# Measure one full `stratify check` on a repo: wall time, peak memory, size.
# Usage: scripts/bench.sh <stratify-bin> <repo-path>
set -euo pipefail

bin="$1"
repo="$2"
exts='\.(java|rb|ts|tsx|mts|cts|py|pyi|go|rs)$'

files=$(git -C "$repo" ls-files | grep -E "$exts" | wc -l | tr -d ' ')
lines=$(git -C "$repo" ls-files -z | grep -zE "$exts" | (cd "$repo" && xargs -0 cat) | wc -l | tr -d ' ')

tmp=$(mktemp)
if [[ "$(uname)" == "Darwin" ]]; then
  /usr/bin/time -l "$bin" check "$repo" --format json >/dev/null 2>"$tmp" || true
  rss_bytes=$(awk '/maximum resident set size/ {print $1}' "$tmp")
  wall=$(awk '/ real / {print $1}' "$tmp")
  rss_mb=$((rss_bytes / 1024 / 1024))
else
  /usr/bin/time -v "$bin" check "$repo" --format json >/dev/null 2>"$tmp" || true
  rss_kb=$(awk -F: '/Maximum resident set size/ {gsub(/ /, "", $2); print $2}' "$tmp")
  wall=$(awk '/Elapsed \(wall clock\)/ {print $NF}' "$tmp")
  rss_mb=$((rss_kb / 1024))
fi
rm -f "$tmp"

echo "files=$files lines=$lines wall_s=$wall peak_rss_mb=$rss_mb"
```

`chmod +x scripts/bench.sh`. Test it on this repo: `scripts/bench.sh target/release/stratify .`. Expected: one line with all four values filled.

- [ ] **Step 2: Measure one large repo**

```bash
git clone --depth 1 --branch v1.31.0 https://github.com/kubernetes/kubernetes /tmp/k8s
scripts/bench.sh target/release/stratify /tmp/k8s
```

If the clone fails (no network), stop and report it. Do not substitute a smaller repo without saying so.

- [ ] **Step 3: Record and commit**

`docs/benchmarks.md`:

```markdown
# Benchmarks

Full `stratify check --format json` runs, measured with `scripts/bench.sh`.
The roadmap's scan-time budget comes from these numbers.

| Date | Stratify | Repo | Files | Lines | Wall (s) | Peak RSS (MB) | Machine |
|------|----------|------|-------|-------|----------|---------------|---------|
| <date> | <git rev-parse --short HEAD> | kubernetes v1.31.0 | <files> | <lines> | <wall> | <rss> | <uname -sm, CPU model> |
| <date> | <rev> | stratify (self) | ... | ... | ... | ... | ... |
```

Fill every cell from the measured output. Then:

```bash
git add scripts/bench.sh docs/benchmarks.md
git commit -m "docs: record a scan-time and memory baseline"
```

---

### Task 9: Update the roadmap status

**Files:**
- Modify: `docs/roadmap-full-stack-static-analysis.md`

- [ ] **Step 1: Edit the roadmap**

1. In "Where Stratify stands today", change the Languages row to `All six languages get six of six`, the Findings row to note `related` and `reasons`, and the Gate row to `--fail-on severity threshold, --baseline new-findings gate, stratify-ignore line suppression`.
2. In Phase 1, replace work item 3 (`Report schema version 2. Additive only.`) with `Optional evidence fields on a version 1 schema. stratify-judge skips judgment on any unknown schema version, so additive fields do not bump it.` Update the exit criterion `stratify-judge reads a version 2 report` to `stratify-judge judges a report with evidence fields with no change in verdicts`.
3. Under Phase 0 and Phase 1 add a `Status:` line listing what shipped (with the benchmark and precision numbers from `docs/benchmarks.md` and the Task 7 table) and what remains: labelled repos for the other five languages.

- [ ] **Step 2: Commit**

```bash
git add docs/roadmap-full-stack-static-analysis.md
git commit -m "docs: mark roadmap phase 0 and 1 progress"
```
