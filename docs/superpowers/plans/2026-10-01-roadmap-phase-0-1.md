# Roadmap Phase 0 and Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give Rust the two missing analyses. Ship Phase 1: findings evidence, rule metadata, comment suppression, and a baseline gate. Build Phase 0's measurement tools and record their first data. This plan does not close Phase 0. Phase 0 closes when every rule-language pair is gate-ready in the labelled corpus and `docs/benchmarks.md` holds a measured run on a repo above 500k lines.

**Architecture:**
- **Rust imports.** The Rust adapter turns `use` paths into `Dependency` symbols. A repo-wide resolver in `stratify-analysis` rewrites them to file paths, the same way `go_imports` works for Go. The six existing analyses then see Rust import edges with no analysis change.
- **Phase 1 types.** Phase 1 adds optional evidence fields to `Finding` and a static rule registry in `stratify-core`.
- **Suppression.** Each adapter records suppression markers it finds in tree-sitter comment nodes into the IR. A language-blind filter in `stratify-analysis` applies them.
- **Baseline.** A baseline filter lives in `stratify-cli`.
- **Measurement.** Corpus data lives in `corpus/`. The measurement scripts live in `scripts/` and rely on `stratify check --stats`.

**Tech Stack:** Rust 2021 workspace, tree-sitter 0.24 and the six 0.23 grammars, serde, clap 4, toml 0.8, Python 3 standard library for the scripts, bash for the Action wrapper.

**Spec:** `docs/roadmap-full-stack-static-analysis.md` (Phase 0 and Phase 1 sections).

## Global Constraints

- One binary. No servers. No accounts.
- Analyses read only the IR and never branch on language.
- `Report::SCHEMA_VERSION` stays `1`. New `Finding` fields are optional, default to empty, and are omitted from JSON when empty. Reason: `stratify-judge` passes any report with `schema_version > 1` through unjudged (`~/dev/stratify-jev/crates/stratify-judge-cli/src/main.rs:127`) and already tolerates unknown fields through `#[serde(flatten)] extra`.
- A finding with no evidence serializes byte-for-byte as it does in v0.6.0.
- Every change passes `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test --locked` (the CI gate in `.github/workflows/ci.yml`).
- `cargo` lives at `~/.cargo/bin/cargo`. Run `export PATH="$HOME/.cargo/bin:$PATH"` before any cargo command.
- Each user-visible change adds a line under `## [Unreleased]` in `CHANGELOG.md` (Task 1 creates the heading above `## [0.6.0]`).
- Commit messages follow the repo style: a `feat(scope): ...`, `fix(scope): ...`, or `docs: ...` subject line. The plan prescribes no trailer or co-author line.
- Work on branch `roadmap-phase-0-1`. Never commit to `main`.
  - Note: The branch was merged to `main` locally on 2026-10-02 at the owner's direction. Follow-up work happens on `roadmap-qa-fixes`.
- Status language:
  - No document, commit, or changelog entry calls Phase 0 complete.
  - Phase 0 closes only when two conditions hold. First, `scripts/precision.py` reports all 36 rule-language pairs `gate-ready`. Second, `docs/benchmarks.md` holds a measured large-repo row.
  - Tasks 7 and 8 deliver tools plus initial data. Neither one is a Phase 0 sign-off.

## Review Focus

1. `use super::*;` inside an inline `#[cfg(test)] mod tests { ... }` refers to the same file. Expected: no import edge. Test in Task 1.
2. `use std::...`, `use serde::...`, and any crate outside the scan. Expected: no edge, no finding, no panic. Test in Task 2.
3. A `stratify-ignore:` marker inside a string literal or Python docstring on the line above a finding. Expected: no suppression, the finding stays. Tests in Task 5 (each adapter, and `e2e_suppress`).
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
- Consumes: Task 3 registry (context only).
- Produces in `stratify_core`:
  - `pub struct Related { pub span: Span, pub role: String, pub message: String }`
  - `Finding` gains three fields, each `#[serde(default, skip_serializing_if = "Vec::is_empty")]`:
    - `pub related: Vec<Related>`: supporting locations, no order implied.
    - `pub path: Vec<Related>`: ordered steps of a flow. Cycles use it now. Taint paths use it in Phase 5.
    - `pub reasons: Vec<String>`: ordered reason chain, first reason first.
  - `Finding::new(rule: impl Into<String>, severity: Severity, message: impl Into<String>, span: Span, confidence: Confidence) -> Finding`
  - `Finding::with_related(self, span: Span, role: impl Into<String>, message: impl Into<String>) -> Finding`
  - `Finding::with_path_step(self, span: Span, role: impl Into<String>, message: impl Into<String>) -> Finding`
  - `Finding::with_reason(self, reason: impl Into<String>) -> Finding`
  - `Related` re-exported from `stratify_core`.

SARIF contract (the roadmap's Phase 1 item 2 states the same contract):

| Finding field | SARIF 2.1.0 location in `runs[0].results[i]` | Emitted when |
|---|---|---|
| `related` | `relatedLocations[j]` = `{ "id": j, "physicalLocation": {...}, "message": { "text": role message } }`, `j` in `related` order | `related` is non-empty |
| `path` | `codeFlows` = exactly one code flow holding exactly one thread flow. `threadFlows[0].locations[k]` = `{ "location": { "physicalLocation": {...}, "message": { "text": step message } }, "executionOrder": k + 1 }`, in `path` order. The array order is the flow order, and `executionOrder` repeats it so viewers that sort by it agree | `path` is non-empty |
| `reasons` | `properties.reasons` = the full ordered array. When a code flow exists, `codeFlows[0].message.text` = the reasons joined with `"; "` | `reasons` is non-empty |

A reason is prose with no location, so it never becomes a thread-flow location. SARIF requires `threadFlows` and `locations` to be non-empty arrays. Emitting `codeFlows` only for a non-empty `path` keeps every document valid.

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
    fn evidence_round_trips_and_keeps_order() {
        let f = Finding::new("cycle", Severity::Warning, "m", sp("a.rs", 1), Confidence::Certain)
            .with_path_step(sp("a.rs", 1), "cycle_member", "imports b.rs")
            .with_path_step(sp("b.rs", 3), "cycle_member", "imports a.rs")
            .with_related(sp("c.rs", 9), "clone", "same block")
            .with_reason("first")
            .with_reason("second");
        let json = serde_json::to_string(&f).unwrap();
        let back: Finding = serde_json::from_str(&json).unwrap();
        assert_eq!(back, f);
        assert_eq!(back.path[1].span.file, "b.rs");
        assert_eq!(back.reasons, vec!["first", "second"]);
    }

    #[test]
    fn v1_report_without_evidence_still_parses() {
        let json = r#"{"schema_version":1,"findings":[{"rule":"dead_code","severity":"warning","message":"m","span":{"file":"a","start_byte":0,"end_byte":1,"start_line":1},"confidence":"certain"}]}"#;
        let r: Report = serde_json::from_str(json).unwrap();
        assert!(r.findings[0].related.is_empty());
        assert!(r.findings[0].path.is_empty());
        assert!(r.findings[0].reasons.is_empty());
    }
```

In `crates/stratify-analysis/src/cycles.rs` tests:

```rust
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
```

In `crates/stratify-analysis/src/duplication.rs` tests, add a test using that module's existing helpers (read them first). Build a graph with the same token run in three files and assert:

```rust
        assert_eq!(findings.len(), 1);
        let sites: Vec<&str> = findings[0].related.iter().map(|r| r.span.file.as_str()).collect();
        assert_eq!(sites.len(), 2);
        assert!(findings[0].related.iter().all(|r| r.role == "clone"));
        assert!(findings[0].path.is_empty());
```

In `crates/stratify-analysis/src/boundaries.rs` tests, extend the existing violation test (read it first) with:

```rust
        assert_eq!(findings[0].related.len(), 1);
        assert_eq!(findings[0].related[0].role, "imported_file");
```

and assert `related[0].span.file` equals that test's imported file.

In `crates/stratify-analysis/src/deadcode.rs` tests:

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
    fn span_at(file: &str, line: usize) -> Span {
        Span { file: file.into(), start_byte: 0, end_byte: 1, start_line: line }
    }

    #[test]
    fn related_spans_become_related_locations() {
        let f = finding("duplication", Severity::Warning, "a.rb", 1)
            .with_related(span_at("b.rb", 4), "clone", "same block")
            .with_related(span_at("c.rb", 7), "clone", "same block");
        let v: serde_json::Value = serde_json::from_str(&render(&Report::new(vec![f]))).unwrap();
        let rel = v["runs"][0]["results"][0]["relatedLocations"].as_array().unwrap();
        assert_eq!(rel.len(), 2);
        assert_eq!(rel[1]["id"], 1);
        assert_eq!(rel[1]["physicalLocation"]["artifactLocation"]["uri"], "c.rb");
        assert_eq!(rel[1]["physicalLocation"]["region"]["startLine"], 7);
        assert_eq!(rel[1]["message"]["text"], "same block");
        assert!(v["runs"][0]["results"][0].get("codeFlows").is_none());
    }

    #[test]
    fn path_becomes_one_ordered_thread_flow() {
        let f = finding("cycle", Severity::Warning, "a.rb", 1)
            .with_path_step(span_at("a.rb", 1), "cycle_member", "imports b.rb")
            .with_path_step(span_at("b.rb", 2), "cycle_member", "imports c.rb")
            .with_path_step(span_at("c.rb", 3), "cycle_member", "imports a.rb")
            .with_reason("each file imports the next")
            .with_reason("the last imports the first");
        let v: serde_json::Value = serde_json::from_str(&render(&Report::new(vec![f]))).unwrap();
        let result = &v["runs"][0]["results"][0];
        let flows = result["codeFlows"].as_array().unwrap();
        assert_eq!(flows.len(), 1);
        assert_eq!(flows[0]["message"]["text"], "each file imports the next; the last imports the first");
        let threads = flows[0]["threadFlows"].as_array().unwrap();
        assert_eq!(threads.len(), 1);
        let locs = threads[0]["locations"].as_array().unwrap();
        let uris: Vec<&str> = locs
            .iter()
            .map(|l| l["location"]["physicalLocation"]["artifactLocation"]["uri"].as_str().unwrap())
            .collect();
        assert_eq!(uris, vec!["a.rb", "b.rb", "c.rb"]);
        let orders: Vec<u64> = locs.iter().map(|l| l["executionOrder"].as_u64().unwrap()).collect();
        assert_eq!(orders, vec![1, 2, 3]);
        assert_eq!(locs[2]["location"]["message"]["text"], "imports a.rb");
        assert_eq!(result["properties"]["reasons"][1], "the last imports the first");
    }

    #[test]
    fn reasons_without_path_go_to_properties_only() {
        let f = finding("dead_code", Severity::Warning, "a.rb", 1).with_reason("no caller");
        let v: serde_json::Value = serde_json::from_str(&render(&Report::new(vec![f]))).unwrap();
        let result = &v["runs"][0]["results"][0];
        assert_eq!(result["properties"]["reasons"][0], "no caller");
        assert!(result.get("codeFlows").is_none());
    }

    #[test]
    fn result_without_evidence_has_no_evidence_keys() {
        let v: serde_json::Value = serde_json::from_str(&render(&Report::new(vec![finding("cycle", Severity::Warning, "a.rb", 1)]))).unwrap();
        let r = v["runs"][0]["results"][0].as_object().unwrap();
        assert!(!r.contains_key("relatedLocations"));
        assert!(!r.contains_key("codeFlows"));
        assert!(!r.contains_key("properties"));
    }
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test --workspace`
Expected: compile errors (`Finding::new`, `related`, `path`, `reasons` not found).

- [ ] **Step 3: Implement the core type**

In `finding.rs`:

```rust
/// A secondary location backing a finding: another clone site, a step in a
/// cycle, the file a forbidden import points at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Related {
    pub span: Span,
    /// Machine-readable role, e.g. "clone", "cycle_member", "imported_file".
    pub role: String,
    pub message: String,
}

/// One reported problem. `rule` identifies the analysis, e.g. "dead_code".
/// `related`, `path`, and `reasons` are optional evidence, omitted from JSON
/// when empty, so a finding without evidence serializes exactly as in v0.6.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub rule: String,
    pub severity: Severity,
    pub message: String,
    pub span: Span,
    pub confidence: Confidence,
    /// Supporting locations. No order implied.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<Related>,
    /// Ordered steps of a flow: the files of a cycle, later the hops of a
    /// taint path. SARIF renders it as one thread flow.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<Related>,
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
            path: Vec::new(),
            reasons: Vec::new(),
        }
    }

    pub fn with_related(
        mut self,
        span: Span,
        role: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        self.related.push(Related { span, role: role.into(), message: message.into() });
        self
    }

    pub fn with_path_step(
        mut self,
        span: Span,
        role: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        self.path.push(Related { span, role: role.into(), message: message.into() });
        self
    }

    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reasons.push(reason.into());
        self
    }
}
```

Update `lib.rs`: `pub use finding::{Finding, Related, Report, Severity};`. Leave `SCHEMA_VERSION` at 1, with this doc line above it: `/// Additive optional fields do not bump this. stratify-judge skips judgment for any version it does not know.`

- [ ] **Step 4: Convert every literal and populate evidence**

Replace each production `Finding { ... }` literal with `Finding::new(...)`. In test code, either use `Finding::new` or add `related: Vec::new(), path: Vec::new(), reasons: Vec::new()`.

Evidence per analysis:

- `cycles.rs`: for the cycle `[n0, ..., nk]` (fqns) with display files `files`, add one path step per member, in cycle order:
  - span = `spans.get(fqn)`, falling back to the same zero span the primary uses;
  - role `"cycle_member"`;
  - message `format!("imports {}", files[(i + 1) % files.len()])`.
  - Then add reason `"each file in the cycle imports the next, and the last imports the first"`.
- `duplication.rs`: for each `o` in `valid_others` sorted ascending, add a related entry:
  - span = `Span { file: tokens[o].file.clone(), start_byte: tokens[o].start_byte, end_byte: tokens[o + k - 1].end_byte, start_line: tokens[o].start_line }`;
  - role `"clone"`;
  - message `format!("same {k}-token block")`.
  - Then add reason `format!("{k} normalized tokens match exactly; identifiers, numbers, and strings are compared by kind")`.
- `boundaries.rs`: add one related entry:
  - span = `span_of.get(tgt)`, falling back to a zero span at `tgt`;
  - role `"imported_file"`;
  - message `format!("{tgt} is in layer `{tgt_layer}`")`.
  - Then add reason `format!("stratify.toml forbids layer `{src_layer}` from importing layer `{tgt_layer}`")`.
- `deadcode.rs`:
  - Certain branch: reason `"no call from any entrypoint reaches this function"`.
  - Likely branch: when `reached_any.contains(&s.id)`, push `"reached only through a name-matched call, which cannot prove this exact function is the target"`. When `s.visibility == Visibility::Public && mode == DeadCodeMode::Library`, push `"public in library mode: code outside this scan may call it"`. When both hold, push both.
- `complexity.rs`: reason `format!("{cx} independent paths exceed the threshold of {threshold}")`.
- `hotspot.rs`: read the function first. Add one reason that names the complexity and churn values the message already uses.

- [ ] **Step 5: SARIF mapping**

In `sarif.rs`, add a shared helper and the evidence types:

```rust
fn physical(span: &Span) -> PhysicalLocation {
    PhysicalLocation {
        artifact_location: ArtifactLocation { uri: span.file.clone() },
        region: Region { start_line: span.start_line.max(1) },
    }
}

#[derive(Serialize)]
struct RelatedLocation {
    id: usize,
    #[serde(rename = "physicalLocation")]
    physical_location: PhysicalLocation,
    message: Text,
}

#[derive(Serialize)]
struct CodeFlow {
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<Text>,
    #[serde(rename = "threadFlows")]
    thread_flows: Vec<ThreadFlow>,
}

#[derive(Serialize)]
struct ThreadFlow {
    locations: Vec<ThreadFlowLocation>,
}

#[derive(Serialize)]
struct ThreadFlowLocation {
    location: FlowLocation,
    #[serde(rename = "executionOrder")]
    execution_order: usize,
}

#[derive(Serialize)]
struct FlowLocation {
    #[serde(rename = "physicalLocation")]
    physical_location: PhysicalLocation,
    message: Text,
}

#[derive(Serialize)]
struct ResultProperties {
    reasons: Vec<String>,
}
```

Extend `SarifResult` with:

```rust
    #[serde(rename = "relatedLocations", skip_serializing_if = "Vec::is_empty")]
    related_locations: Vec<RelatedLocation>,
    #[serde(rename = "codeFlows", skip_serializing_if = "Vec::is_empty")]
    code_flows: Vec<CodeFlow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    properties: Option<ResultProperties>,
```

Fill them per the SARIF contract table above:
- `related_locations`: from `f.related`, with `id` = the index.
- `code_flows`: empty when `f.path` is empty. Otherwise one `CodeFlow`:
  - `message` = `Some(Text { text: f.reasons.join("; ") })` when `reasons` is non-empty, else `None`;
  - one `ThreadFlow` whose `locations` follow `f.path` order, with `execution_order` = index + 1.
- `properties`: `Some(ResultProperties { reasons: f.reasons.clone() })` only when `reasons` is non-empty.

Use `physical()` for the primary location too, so all four location shapes share one helper.

`CHANGELOG.md` under `## [Unreleased]` `### Added`:

```markdown
- Findings carry evidence: `related` locations (clone sites, the file a
  forbidden import points at), an ordered `path` (the files of a cycle), and
  a `reasons` chain. All three are optional and omitted when empty, so the
  JSON schema stays at version 1. SARIF maps `related` to
  `relatedLocations`, `path` to one ordered `codeFlows` thread flow, and
  `reasons` to `properties.reasons` (and to the code flow's message).
```

- [ ] **Step 6: Run the tests and confirm they pass**

Run: `cargo test --workspace`
Expected: all pass.

Check judge compatibility. Run `cargo run -q -p stratify-cli -- check crates/stratify-cli/tests/sample-java --format json`. Expect:
- `"schema_version": 1`;
- no `related`, `path` or `reasons` keys on any finding the analysis gave no evidence to.

- [ ] **Step 7: Lint and commit**

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings
git add -A crates CHANGELOG.md
git commit -m "feat(core): attach related locations, flow paths, and reasons to findings"
```

---

### Task 5: Comment-only suppression markers

**Files:**
- Modify: `crates/stratify-core/src/ir.rs`, `crates/stratify-core/src/graph.rs`, `crates/stratify-core/src/lib.rs`
- Modify: `crates/stratify-lang/src/walk.rs`
- Modify: the six adapters' `crates/stratify-lang-{java,ruby,ts,py,go,rust}/src/extract.rs`
- Create: `crates/stratify-analysis/src/suppress.rs`
- Modify: `crates/stratify-analysis/src/lib.rs`, `crates/stratify-cli/src/run.rs`
- Create: `crates/stratify-cli/tests/sample-suppress/app.rb`, `crates/stratify-cli/tests/e2e_suppress.rs`
- Modify: `README.md`, `CHANGELOG.md`

**Interfaces:**
- Consumes: `Finding` with `rule` and `span.start_line` (1-based), and `Finding::new` (Task 4).
- Produces:
  - `stratify_core::ir::Suppression { pub file: String, pub start_line: usize, pub end_line: usize, pub rules: Vec<String> }` (serde derives, re-exported from `stratify_core`).
  - `IrGraph::add_suppression(&mut self, s: Suppression)` and `IrGraph::suppressions(&self) -> &[Suppression]`. `IrGraph::merge` appends the other graph's suppressions.
  - `stratify_lang::walk::parse_suppression_marker(comment: &str) -> Vec<String>`
  - `stratify_lang::walk::collect_suppressions(graph: &mut IrGraph, root: Node, src: &str, file: &str, comment_kinds: &[&str])`
  - `stratify_analysis::suppress::apply(graph: &IrGraph, findings: Vec<Finding>) -> (Vec<Finding>, usize)`. The `usize` is the count removed.

**How comments are found.** Every adapter already holds a tree-sitter parse tree. A string literal is never a comment node, so a marker counts only when it sits inside a node whose kind is in the language's comment-kind list. The adapter records each marker comment in the IR. The filter in `stratify-analysis` reads only the IR and never branches on language.

| Language | Comment kinds (tree-sitter 0.23 grammars) | Forms covered |
|---|---|---|
| Java | `line_comment`, `block_comment` | `//`, `/* */`, `/** */` |
| Ruby | `comment` | `#`, `=begin ... =end` |
| TypeScript | `comment` | `//`, `/* */` |
| Python | `comment` | `#` (docstrings are strings and never count) |
| Go | `comment` | `//`, `/* */` |
| Rust | `line_comment`, `block_comment` | `//`, `///`, `/* */` |

The per-adapter tests below fail if a kind name is wrong for the grammar. If one fails that way, print the tree's node kinds (`root.to_sexp()`) and fix the kind list. Never fix the test.

**Marker syntax.**
- Write `stratify-ignore: <rule>[, <rule>...]` inside a comment.
- Each comma-separated item contributes its first word, so `stratify-ignore: dead_code, complexity -- called by the router` names `dead_code` and `complexity`.
- A word counts only when it is rule-shaped: lowercase ASCII letters, digits and `_`, starting with a letter.
- An empty list or `*` names nothing. A rule id is required.

**Line rule.** A comment covers lines `start_line..=end_line`. A grammar whose comment node ends at column 0 of the next row (a line comment that swallows its newline) gets `end_line` = that row, not the next one. A finding at line `L` is suppressed by a comment in the same file that names its rule when either holds:
- `start_line <= L <= end_line`: a trailing comment on the same line;
- `end_line == L - 1`: the comment ends on the line directly above.

- [ ] **Step 1: Write the failing IR and walker tests**

In `crates/stratify-core/src/graph.rs` tests:

```rust
    #[test]
    fn suppressions_survive_merge() {
        let mut a = IrGraph::new();
        a.add_suppression(Suppression { file: "a.rs".into(), start_line: 1, end_line: 1, rules: vec!["dead_code".into()] });
        let mut b = IrGraph::new();
        b.add_suppression(Suppression { file: "b.rs".into(), start_line: 4, end_line: 6, rules: vec!["cycle".into()] });
        a.merge(b);
        let files: Vec<&str> = a.suppressions().iter().map(|s| s.file.as_str()).collect();
        assert_eq!(files, vec!["a.rs", "b.rs"]);
    }
```

In `crates/stratify-lang/src/walk.rs` tests (the module already parses Go in tests):

```rust
    #[test]
    fn parses_marker_rule_lists() {
        assert_eq!(parse_suppression_marker("// stratify-ignore: dead_code"), vec!["dead_code"]);
        assert_eq!(
            parse_suppression_marker("# stratify-ignore: dead_code, complexity -- router calls it"),
            vec!["dead_code", "complexity"]
        );
        assert_eq!(parse_suppression_marker("/* stratify-ignore: cycle */"), vec!["cycle"]);
        assert_eq!(parse_suppression_marker("/*stratify-ignore: cycle*/"), vec!["cycle"]);
        assert!(parse_suppression_marker("// stratify-ignore:").is_empty());
        assert!(parse_suppression_marker("// stratify-ignore: *").is_empty());
        assert!(parse_suppression_marker("// nothing here").is_empty());
    }

    #[test]
    fn collects_markers_only_from_comment_nodes_in_source_order() {
        let src = "package a\n// stratify-ignore: dead_code\nfunc a() {}\nvar s = \"stratify-ignore: complexity\"\n/* stratify-ignore: duplication\n */\nfunc c() {}\n";
        let tree = go_parse(src);
        let mut g = IrGraph::new();
        collect_suppressions(&mut g, tree.root_node(), src, "a.go", &["comment"]);
        let got: Vec<(usize, usize, Vec<String>)> = g
            .suppressions()
            .iter()
            .map(|s| (s.start_line, s.end_line, s.rules.clone()))
            .collect();
        assert_eq!(
            got,
            vec![(2, 2, vec!["dead_code".to_string()]), (5, 6, vec!["duplication".to_string()])]
        );
    }
```

`go_parse` stands for however the existing walk tests build a Go tree. Read the test module and reuse its helper, or add a two-line one.

- [ ] **Step 2: Run them and confirm they fail**

Run: `cargo test -p stratify-core suppressions && cargo test -p stratify-lang`
Expected: compile errors (`Suppression`, `parse_suppression_marker` not found).

- [ ] **Step 3: Implement the IR type and the walker**

In `ir.rs`:

```rust
/// A `stratify-ignore: <rules>` marker found inside a comment. Lines are
/// 1-based and inclusive. Adapters record these; analyses never look for
/// comment syntax themselves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Suppression {
    pub file: String,
    pub start_line: usize,
    pub end_line: usize,
    pub rules: Vec<String>,
}
```

In `graph.rs`, add a `suppressions: Vec<Suppression>` field, plus `add_suppression`, `suppressions`, and `self.suppressions.extend(other.suppressions)` in `merge`. Re-export `Suppression` from `lib.rs`.

In `walk.rs`:

```rust
const SUPPRESSION_MARKER: &str = "stratify-ignore:";

/// Rule ids named by a `stratify-ignore: a, b` marker in one comment's text.
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
pub fn collect_suppressions(
    graph: &mut IrGraph,
    root: Node,
    src: &str,
    file: &str,
    comment_kinds: &[&str],
) {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if comment_kinds.contains(&node.kind()) {
            let rules = parse_suppression_marker(node_text(node, src));
            if !rules.is_empty() {
                let start = node.start_position();
                let end = node.end_position();
                // A comment that swallows its newline ends at column 0 of the
                // next row. It still belongs to the row before.
                let end_row = if end.column == 0 && end.row > start.row {
                    end.row - 1
                } else {
                    end.row
                };
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
```

Run: `cargo test -p stratify-core && cargo test -p stratify-lang`
Expected: pass.

- [ ] **Step 4: Wire each adapter, with a test per language**

In each adapter's `extract`, right after the existing `walk::tokenize(...)` call, add one line with that language's kinds from the table, for example Rust:

```rust
    walk::collect_suppressions(&mut g, root, src, file, &["line_comment", "block_comment"]);
```

Add one test per adapter in its `extract.rs` test module. Each test calls the adapter's own `extract` function the same way the file's existing tests do, and uses this assertion helper inline:

```rust
        let got: Vec<(usize, usize, Vec<String>)> = g
            .suppressions()
            .iter()
            .map(|s| (s.start_line, s.end_line, s.rules.clone()))
            .collect();
```

| Adapter | Test name | Source (`src`) | Expected `got` |
|---|---|---|---|
| rust | `suppression_markers_come_only_from_comments` | `"// stratify-ignore: dead_code\nfn a() {}\nfn b() { let s = \"stratify-ignore: complexity\"; }\n/* stratify-ignore: duplication */\nfn c() {}\n"` | `[(1, 1, ["dead_code"]), (4, 4, ["duplication"])]` |
| java | same | `"class A {\n  // stratify-ignore: dead_code\n  void a() {}\n  String s = \"stratify-ignore: complexity\";\n  /* stratify-ignore: duplication */\n  void c() {}\n}\n"` | `[(2, 2, ["dead_code"]), (5, 5, ["duplication"])]` |
| ts | same | `"// stratify-ignore: dead_code\nfunction a() {}\nconst s = \"stratify-ignore: complexity\";\n/* stratify-ignore: duplication */\nfunction c() {}\n"` | `[(1, 1, ["dead_code"]), (4, 4, ["duplication"])]` |
| go | same | `"package a\n// stratify-ignore: dead_code\nfunc a() {}\nvar s = \"stratify-ignore: complexity\"\n/* stratify-ignore: duplication */\nfunc c() {}\n"` | `[(2, 2, ["dead_code"]), (5, 5, ["duplication"])]` |
| py | same | `"# stratify-ignore: dead_code\ndef a():\n    \"\"\"stratify-ignore: complexity\"\"\"\n    s = \"stratify-ignore: duplication\"\n"` | `[(1, 1, ["dead_code"])]` |
| ruby | same | `"# stratify-ignore: dead_code\ndef a\n  s = \"stratify-ignore: complexity\"\nend\n=begin\nstratify-ignore: duplication\n=end\ndef c\nend\n"` | `[(1, 1, ["dead_code"]), (5, 7, ["duplication"])]` |

Write each expected value as `vec![(1, 1, vec!["dead_code".to_string()]), ...]`. Each test covers the string-literal case (the marker inside a string must not appear in `got`), a line comment, and a block comment where the language has one.

Run: `cargo test --workspace`
Expected: pass.

- [ ] **Step 5: The filter, with failing tests first**

Create `crates/stratify-analysis/src/suppress.rs` with its tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use stratify_core::ir::{Span, Suppression};
    use stratify_core::{Confidence, Finding, IrGraph, Severity};

    fn f(rule: &str, file: &str, line: usize) -> Finding {
        Finding::new(rule, Severity::Warning, "m", Span { file: file.into(), start_byte: 0, end_byte: 1, start_line: line }, Confidence::Certain)
    }

    fn graph() -> IrGraph {
        let mut g = IrGraph::new();
        // line comment on line 2, naming dead_code
        g.add_suppression(Suppression { file: "a.rs".into(), start_line: 2, end_line: 2, rules: vec!["dead_code".into()] });
        // trailing comment on line 10, naming two rules
        g.add_suppression(Suppression { file: "a.rs".into(), start_line: 10, end_line: 10, rules: vec!["complexity".into(), "dead_code".into()] });
        // block comment on lines 20-22
        g.add_suppression(Suppression { file: "a.rs".into(), start_line: 20, end_line: 22, rules: vec!["cycle".into()] });
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
        let (kept, _) = apply(&graph(), vec![f("dead_code", "a.rs", 10), f("complexity", "a.rs", 10)]);
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
```

Run: `cargo test -p stratify-analysis suppress`
Expected: compile error, `apply` not found.

Implement above the tests:

```rust
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
```

Add `pub mod suppress;` to `crates/stratify-analysis/src/lib.rs`.

In `run.rs` `analyze_repo_with_stats`, after the boundary findings and before `scan_stats`:

```rust
    let (findings, _suppressed) = stratify_analysis::suppress::apply(&graph, findings);
```

- [ ] **Step 6: E2E fixture and test**

`crates/stratify-cli/tests/sample-suppress/app.rb`:

```ruby
def main
  puts "hi"
end

# stratify-ignore: dead_code
def kept_for_reflection
  1
end

NOTE = "stratify-ignore: dead_code"
def really_unused
  2
end

main
```

`crates/stratify-cli/tests/e2e_suppress.rs`:

```rust
use std::path::Path;

#[test]
fn only_comment_markers_suppress() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/sample-suppress");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_stratify"))
        .arg("check")
        .arg(&dir)
        .arg("--format")
        .arg("json")
        .output()
        .expect("run stratify binary");
    let stdout = String::from_utf8(output.stdout).unwrap();
    // A comment marker on the line above hides the finding.
    assert!(!stdout.contains("kept_for_reflection"), "stdout: {stdout}");
    // The same text inside a string literal on the line above does not.
    assert!(stdout.contains("really_unused"), "stdout: {stdout}");
}
```

If `main` itself is reported (Ruby entrypoint handling), adjust only the fixture, never the assertions.

- [ ] **Step 7: Docs**

Add this section to `README.md` after "Output formats":

````markdown
## Suppress a finding

Put `stratify-ignore: <rule>` in a comment on the finding's line or the line above. Name one rule or a comma-separated list. A rule id is required, so a suppression never hides more than you meant. Text after the first word of each item is ignored, so you can give a reason.

```ruby
# stratify-ignore: dead_code -- called by the framework
def after_commit_hook
end
```

The marker counts only inside a comment: `//`, `/* */`, and `///` in Java, TypeScript, Go, and Rust, and `#` in Ruby and Python, plus Ruby's `=begin`/`=end`. The same text inside a string literal or a Python docstring does nothing. A block comment counts when it ends on the line above the finding.

Rule ids: `dead_code`, `duplication`, `complexity`, `hotspot`, `cycle`, `boundary`. A finding's `rule` field in JSON output shows its id.
````

`CHANGELOG.md` under `## [Unreleased]` `### Added`:

```markdown
- Comment suppression: a `stratify-ignore: <rule>` comment on a finding's
  line or the line above hides that rule's finding there. Only real
  comments count; the same text in a string literal is ignored.
```

- [ ] **Step 8: Run all tests, lint, commit**

```bash
cargo test --workspace
cargo fmt --all && cargo clippy --all-targets -- -D warnings
git add crates README.md CHANGELOG.md
git commit -m "feat: suppress a finding with a stratify-ignore comment"
```

---

### Task 6: Baseline file and new-findings gate

**Files:**
- Create: `crates/stratify-cli/src/baseline.rs`
- Modify: `crates/stratify-cli/src/main.rs`, `crates/stratify-cli/Cargo.toml`
- Create: `crates/stratify-cli/tests/e2e_baseline.rs`
- Create: `scripts/action-run.sh`, `scripts/test_action_run.sh`
- Modify: `action.yml`, `.github/workflows/ci.yml`, `README.md`, `CHANGELOG.md`

**Interfaces:**
- Consumes: `stratify_core::{Finding, Report}`, `Finding::new` (Task 4).
- Produces in `crates/stratify-cli/src/baseline.rs`:
  - `pub fn fingerprint(f: &Finding) -> String`
  - `pub fn write(report: &Report) -> String` (JSON text)
  - `pub fn filter_new(report: Report, baseline_json: &str) -> Result<(Report, usize), String>`, which returns only the findings absent from the baseline, plus the count it hid.
- CLI:
  - `stratify check --write-baseline <FILE>` writes the baseline and exits 0.
  - `stratify check --baseline <FILE>` reports and gates only new findings.
  - A missing or invalid baseline file prints `stratify: baseline <path>: <reason>` to stderr and exits with code 2.
- Action: inputs reach the shell only through environment variables. `scripts/action-run.sh` builds a quoted argument array.

Fingerprint: `rule`, `span.file`, and `message`, joined by `\u{1f}`. In the message, every run of ASCII digits becomes `#`. Line numbers are not part of the fingerprint, so a finding that moves stays baselined. Matching counts multiplicity: if the baseline holds a fingerprint twice and the scan finds it three times, one finding is new.

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

In `main.rs`, add these to `Command::Check`. Also add `baseline` and `write_baseline` to the `Command::Check { .. }` destructuring in `main`:

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

Telemetry keeps receiving the filtered report. That is intended, so dashboards track what the gate sees.

- [ ] **Step 5: E2E test**

`crates/stratify-cli/tests/e2e_baseline.rs`:

```rust
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
}
```

If the Ruby fixture's dead-code shape does not produce Warning-level findings for `old_dead` and `new_dead`, read `crates/stratify-cli/tests/sample-ruby` and copy its working shape. Keep the assertions.

- [ ] **Step 6: Action inputs through environment variables**

Inputs never appear inside a `run:` script body. GitHub expands `${{ }}` before bash runs, so an interpolated input becomes shell code. Environment variables arrive as data.

`scripts/action-run.sh`:

```bash
#!/usr/bin/env bash
# Run the Stratify gate from the GitHub Action's inputs. action.yml passes
# each input as an environment variable, never as interpolated shell text,
# and this script quotes every value into an argument array.
set -euo pipefail

args=(check "${STRATIFY_PATH:-.}" --fail-on "${STRATIFY_FAIL_ON:-warning}" --format "${STRATIFY_FORMAT:-human}")
if [[ -n "${STRATIFY_BASELINE:-}" ]]; then
  args+=(--baseline "$STRATIFY_BASELINE")
fi
exec stratify "${args[@]}"
```

`scripts/test_action_run.sh`:

```bash
#!/usr/bin/env bash
# Check that action-run.sh passes Action inputs as literal arguments.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/bin"
cat >"$tmp/bin/stratify" <<'STUB'
#!/usr/bin/env bash
for a in "$@"; do printf '<%s>\n' "$a"; done
STUB
chmod +x "$tmp/bin/stratify"

out="$(PATH="$tmp/bin:$PATH" STRATIFY_PATH="dir with space" STRATIFY_FAIL_ON=warning \
  STRATIFY_FORMAT=human STRATIFY_BASELINE="base \$(touch $tmp/pwned).json" \
  bash "$here/action-run.sh")"
expected="<check>
<dir with space>
<--fail-on>
<warning>
<--format>
<human>
<--baseline>
<base \$(touch $tmp/pwned).json>"
if [[ "$out" != "$expected" ]]; then
  echo "unexpected arguments:"
  echo "$out"
  exit 1
fi
if [[ -e "$tmp/pwned" ]]; then
  echo "the baseline input was executed as shell code"
  exit 1
fi

out="$(PATH="$tmp/bin:$PATH" STRATIFY_PATH=. STRATIFY_FAIL_ON=never STRATIFY_FORMAT=json \
  STRATIFY_BASELINE= bash "$here/action-run.sh")"
if [[ "$out" == *"--baseline"* ]]; then
  echo "an empty baseline input must add no --baseline flag"
  exit 1
fi
echo "ok"
```

Run: `bash scripts/test_action_run.sh`
Expected: `ok`.

`action.yml`:
1. Add the input:

```yaml
  baseline:
    description: "Optional path to a baseline file written by `stratify check --write-baseline`. When set, only findings absent from it are reported and gated. Relative to the job's working directory."
    default: ''
```

2. Replace the `Run quality gate` step with:

```yaml
    - name: Run quality gate
      shell: bash
      env:
        STRATIFY_PATH: ${{ inputs.path }}
        STRATIFY_FAIL_ON: ${{ inputs.fail-on }}
        STRATIFY_FORMAT: ${{ inputs.format }}
        STRATIFY_BASELINE: ${{ inputs.baseline }}
      run: bash "$GITHUB_ACTION_PATH/scripts/action-run.sh"
```

`.github/workflows/ci.yml`: add a step after `Tests`:

```yaml
      - name: Script tests
        run: bash scripts/test_action_run.sh
```

- [ ] **Step 7: Docs**

`README.md`:
- Add a row to the Action inputs table: `| \`baseline\` | \`''\` | Baseline file. Only findings absent from it are reported and gated. |`
- Under the table, add: `Paths in \`path\` and \`baseline\` may contain spaces. Both are relative to the job's working directory. The Action passes inputs as environment variables, never as shell text.`
- Add a section after "CI quality gate":

````markdown
## Adopt on an existing codebase (baseline)

Record today's findings once, commit the file, and gate only on new ones:

```sh
stratify check . --write-baseline stratify-baseline.json
stratify check . --baseline stratify-baseline.json --fail-on warning
```

A baselined finding stays baselined when code above it moves. A missing or unreadable baseline file exits with code 2, so a broken path never passes the gate.
````

`CHANGELOG.md` under `## [Unreleased]`:

```markdown
### Added
- `--write-baseline <file>` and `--baseline <file>`: record current findings,
  then report and gate only new ones. The GitHub Action takes a `baseline`
  input.

### Fixed
- The GitHub Action passes its inputs as environment variables instead of
  interpolating them into the shell script, so a path with spaces or shell
  syntax is passed through literally.
```

(Merge these into the existing `### Added` heading under `[Unreleased]`, and create `### Fixed` if it does not exist.)

- [ ] **Step 8: Run all tests, lint, commit**

```bash
cargo test --workspace
cargo fmt --all && cargo clippy --all-targets -- -D warnings
bash scripts/test_action_run.sh
git add crates/stratify-cli action.yml scripts/action-run.sh scripts/test_action_run.sh \
  .github/workflows/ci.yml README.md CHANGELOG.md Cargo.lock
git commit -m "feat(cli): gate on new findings with a baseline file"
```

---

### Task 7: Labelled corpus and precision script (initial measurement)

**Files:**
- Create: `corpus/README.md`, `corpus/repos.json`, `corpus/labels/stratify.json`
- Create: `scripts/precision.py`, `scripts/test_precision.py`
- Modify: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: `stratify check <dir> --format json` output.
- Produces: `python3 scripts/precision.py --stratify <bin> [--work <dir>] [--json]`. It prints one row per rule and language pair (6 rules × 6 languages = 36 rows). Exit codes:
  - 0: the corpus is valid;
  - 1: a clone, checkout, or scan failed (the command and its stderr are printed);
  - 2: label validation failed (every error is printed).

**Scope.** This task builds the measurement tool and records its first data: Rust `dead_code` on this repo. That data cannot satisfy the roadmap's per-rule, per-language precision gate. Every other pair reports `no-data`, `incomplete`, or `insufficient`. Phase 0 stays open (see Task 9).

**Label file format**, one per corpus repo, at `corpus/labels/<name>.json`:

```json
{
  "repo": "stratify",
  "complete_rules": ["dead_code"],
  "labels": [
    {"rule": "dead_code", "file": "crates/stratify-cli/src/lsp.rs", "line": 188, "match": "run_stdio", "label": "fp", "note": "called from main.rs:124"}
  ]
}
```

- `label` is `tp` (a maintainer would act on it) or `fp` (the finding is wrong).
- `complete_rules` lists the rules labelled exhaustively in this repo: every finding of those rules has a label. Labels for other rules are allowed but partial.

**Matching.** A label matches a finding when all four hold:
- `rule` is equal;
- `file` is equal;
- `line` equals `span.start_line`;
- `match` is a substring of `message`.

**Validation.** The script reports every error and exits 2 when any of these occur:
- a label lacks one of `rule`, `file`, `line`, `match`, `label`, `note`;
- a `rule` is not a known rule id, `line` is not an integer, or `label` is not `tp`/`fp`;
- `complete_rules` is missing or names an unknown rule;
- a label matches no finding (stale);
- a label matches more than one finding (ambiguous);
- a finding matches more than one label (duplicate);
- a finding of a `complete_rules` rule matches no label (incomplete labelling).

**Reporting.** Per rule and language pair, the script reports:
- `findings`, `tp`, `fp`, `unlabelled`;
- `repos`: the number of corpus repos contributing labelled findings;
- `coverage` = (tp + fp) / findings;
- `precision` = tp / (tp + fp);
- `status`.

`precision` counts labelled findings only. `coverage` shows how much of the pair it covers. Unlabelled findings never count as tp or fp, and a pair with any unlabelled finding cannot be gate-ready.

Statuses:

| Status | Condition |
|---|---|
| `no-data` | zero findings in the corpus |
| `incomplete` | one or more unlabelled findings |
| `insufficient` | fully labelled, but fewer than `MIN_REPOS = 2` repos or fewer than `MIN_LABELLED = 30` labelled findings |
| `gate-ready` | fully labelled, with 2 or more repos and 30 or more labelled findings |

The roadmap's precision targets apply only to `gate-ready` pairs. After the table, the script prints `gate-ready: N of 36 rule-language pairs. Precision targets apply only to gate-ready pairs.`

- [ ] **Step 1: Write the failing test**

`scripts/test_precision.py`:

```python
import unittest

from precision import classify, label_matches, language_of, summarize, validate_labels


def finding(rule, file, line, message):
    return {"rule": rule, "message": message, "span": {"file": file, "start_line": line}}


def label(rule, file, line, match, value="fp"):
    return {"rule": rule, "file": file, "line": line, "match": match, "label": value, "note": "n"}


class LanguageAndMatching(unittest.TestCase):
    def test_language_of(self):
        self.assertEqual(language_of("a/b.rs"), "rust")
        self.assertEqual(language_of("x.tsx"), "typescript")
        self.assertEqual(language_of("README.md"), "other")

    def test_match_requires_rule_file_line_and_substring(self):
        lab = label("dead_code", "a.rs", 3, "foo")
        self.assertTrue(label_matches(lab, finding("dead_code", "a.rs", 3, "unused function `foo`")))
        self.assertFalse(label_matches(lab, finding("dead_code", "a.rs", 4, "unused function `foo`")))
        self.assertFalse(label_matches(lab, finding("cycle", "a.rs", 3, "foo")))
        self.assertFalse(label_matches(lab, finding("dead_code", "a.rs", 3, "unused function `bar`")))


class Validation(unittest.TestCase):
    def test_schema_errors(self):
        doc = {"complete_rules": ["nope"], "labels": [
            {"rule": "dead_code", "file": "a.rs", "line": "3", "match": "x", "label": "maybe", "note": ""},
            {"rule": "dead_code"},
        ]}
        errors = validate_labels(doc)
        self.assertTrue(any("unknown rule in complete_rules" in e for e in errors), errors)
        self.assertTrue(any("line must be an integer" in e for e in errors), errors)
        self.assertTrue(any("label must be tp or fp" in e for e in errors), errors)
        self.assertTrue(any("missing" in e for e in errors), errors)

    def test_stale_ambiguous_duplicate_and_incomplete(self):
        doc = {"complete_rules": ["dead_code"], "labels": [
            label("dead_code", "a.rs", 1, "x"),
            label("dead_code", "a.rs", 1, "x"),        # duplicate of the first
            label("dead_code", "a.rs", 9, "gone"),     # stale
            label("complexity", "b.rs", 2, ""),        # ambiguous: matches two findings
        ]}
        findings = [
            finding("dead_code", "a.rs", 1, "x"),
            finding("dead_code", "a.rs", 5, "unlabelled"),  # incomplete: rule is complete
            finding("complexity", "b.rs", 2, "one"),
            finding("complexity", "b.rs", 2, "two"),
        ]
        _, errors = classify(findings, doc, "r")
        text = "\n".join(errors)
        self.assertIn("matches 2 labels", text)
        self.assertIn("stale", text)
        self.assertIn("ambiguous", text)
        self.assertIn("complete_rules", text)

    def test_partial_rules_allow_unlabelled(self):
        doc = {"complete_rules": [], "labels": [label("cycle", "a.rs", 1, "x", "tp")]}
        outcomes, errors = classify([finding("cycle", "a.rs", 1, "x"), finding("cycle", "a.rs", 2, "y")], doc, "r")
        self.assertEqual(errors, [])
        self.assertEqual(sorted(o for _, _, o in outcomes), ["tp", "unlabelled"])


class Summary(unittest.TestCase):
    def row(self, rows, rule, lang):
        return next(r for r in rows if r["rule"] == rule and r["language"] == lang)

    def test_every_pair_is_reported(self):
        self.assertEqual(len(summarize({})), 36)
        self.assertEqual(self.row(summarize({}), "cycle", "go")["status"], "no-data")

    def test_statuses(self):
        rows = summarize({
            "one": [("dead_code", "rust", "fp")] * 15 + [("cycle", "rust", "tp"), ("cycle", "rust", "unlabelled")],
            "two": [("dead_code", "rust", "tp")] * 15 + [("complexity", "go", "tp")] * 40,
        })
        dead = self.row(rows, "dead_code", "rust")
        self.assertEqual((dead["status"], dead["repos"], dead["precision"], dead["coverage"]), ("gate-ready", 2, 0.5, 1.0))
        cyc = self.row(rows, "cycle", "rust")
        self.assertEqual((cyc["status"], cyc["coverage"]), ("incomplete", 0.5))
        self.assertEqual(self.row(rows, "complexity", "go")["status"], "insufficient")


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
the pinned commit, scan it, and match findings to corpus/labels/<name>.json.
Precision counts labelled findings only; coverage shows how much of a rule and
language pair is labelled. Precision targets apply only to gate-ready pairs.

Exit codes: 0 valid corpus, 1 clone/checkout/scan failure, 2 label errors.
"""
import argparse
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EXTENSIONS = {
    "java": "java", "rb": "ruby", "ts": "typescript", "tsx": "typescript",
    "mts": "typescript", "cts": "typescript", "py": "python", "pyi": "python",
    "go": "go", "rs": "rust",
}
LANGUAGES = ["java", "ruby", "typescript", "python", "go", "rust"]
RULES = ["dead_code", "duplication", "complexity", "hotspot", "cycle", "boundary"]
LABEL_FIELDS = ("rule", "file", "line", "match", "label", "note")
MIN_REPOS = 2
MIN_LABELLED = 30


class CorpusError(Exception):
    """A clone, checkout, or scan failed."""


def language_of(path):
    ext = path.rsplit(".", 1)[-1] if "." in path else ""
    return EXTENSIONS.get(ext, "other")


def label_matches(label, finding):
    span = finding["span"]
    return (label["rule"] == finding["rule"] and label["file"] == span["file"]
            and label["line"] == span["start_line"] and label["match"] in finding["message"])


def validate_labels(doc):
    """Schema errors in one labels document, as strings."""
    errors = []
    complete = doc.get("complete_rules")
    if not isinstance(complete, list):
        errors.append("complete_rules must be a list")
    else:
        errors += [f"unknown rule in complete_rules: {r}" for r in complete if r not in RULES]
    for i, lab in enumerate(doc.get("labels", [])):
        missing = [k for k in LABEL_FIELDS if k not in lab]
        if missing:
            errors.append(f"label {i}: missing {', '.join(missing)}")
            continue
        if lab["rule"] not in RULES:
            errors.append(f"label {i}: unknown rule {lab['rule']}")
        if not isinstance(lab["line"], int):
            errors.append(f"label {i}: line must be an integer")
        if lab["label"] not in ("tp", "fp"):
            errors.append(f"label {i}: label must be tp or fp")
    return errors


def classify(findings, doc, repo):
    """Match findings to labels. Returns ([(rule, language, outcome)], errors)
    where outcome is tp, fp, or unlabelled."""
    labels = doc["labels"]
    hits = [0] * len(labels)
    outcomes, errors = [], []
    for f in findings:
        where = f"{f['span']['file']}:{f['span']['start_line']} {f['rule']}"
        matched = [i for i, lab in enumerate(labels) if label_matches(lab, f)]
        for i in matched:
            hits[i] += 1
        if len(matched) > 1:
            errors.append(f"{repo}: finding {where} matches {len(matched)} labels (duplicate)")
        if matched:
            outcome = labels[matched[0]]["label"]
        else:
            outcome = "unlabelled"
            if f["rule"] in doc["complete_rules"]:
                errors.append(f"{repo}: finding {where} has no label, but {f['rule']} is in complete_rules")
        outcomes.append((f["rule"], language_of(f["span"]["file"]), outcome))
    for i, n in enumerate(hits):
        where = f"label {i} ({labels[i]['file']}:{labels[i]['line']})"
        if n == 0:
            errors.append(f"{repo}: {where} matches no finding (stale)")
        elif n > 1:
            errors.append(f"{repo}: {where} matches {n} findings (ambiguous)")
    return outcomes, errors


def summarize(per_repo):
    """per_repo: {repo: [(rule, language, outcome)]}. One row per rule x language."""
    rows = []
    for rule in RULES:
        for lang in LANGUAGES:
            counts = {"tp": 0, "fp": 0, "unlabelled": 0}
            repos = set()
            for repo, outcomes in per_repo.items():
                for r, l, outcome in outcomes:
                    if r == rule and l == lang:
                        counts[outcome] += 1
                        if outcome != "unlabelled":
                            repos.add(repo)
            labelled = counts["tp"] + counts["fp"]
            total = labelled + counts["unlabelled"]
            if total == 0:
                status = "no-data"
            elif counts["unlabelled"]:
                status = "incomplete"
            elif len(repos) < MIN_REPOS or labelled < MIN_LABELLED:
                status = "insufficient"
            else:
                status = "gate-ready"
            rows.append({
                "rule": rule, "language": lang, "findings": total, **counts,
                "repos": len(repos),
                "coverage": round(labelled / total, 3) if total else None,
                "precision": round(counts["tp"] / labelled, 3) if labelled else None,
                "status": status,
            })
    return rows


def run(cmd):
    proc = subprocess.run(cmd, capture_output=True, text=True)
    if proc.returncode != 0:
        raise CorpusError(f"{' '.join(cmd)} exited {proc.returncode}: {proc.stderr.strip()}")
    return proc.stdout


def checkout(repo, work):
    dest = work / repo["name"]
    if not dest.exists():
        run(["git", "clone", "--quiet", repo["url"], str(dest)])
    run(["git", "-C", str(dest), "checkout", "--quiet", repo["commit"]])
    return dest


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--stratify", required=True)
    parser.add_argument("--work", default=str(ROOT / "target" / "corpus"))
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()

    work = Path(args.work)
    work.mkdir(parents=True, exist_ok=True)
    repos = json.loads((ROOT / "corpus" / "repos.json").read_text())
    per_repo, errors = {}, []
    try:
        for repo in repos:
            doc = json.loads((ROOT / "corpus" / "labels" / f"{repo['name']}.json").read_text())
            schema = validate_labels(doc)
            if schema:
                errors += [f"{repo['name']}: {e}" for e in schema]
                continue
            out = run([args.stratify, "check", str(checkout(repo, work)), "--format", "json"])
            outcomes, errs = classify(json.loads(out)["findings"], doc, repo["name"])
            per_repo[repo["name"]] = outcomes
            errors += errs
    except CorpusError as e:
        print(f"precision: {e}", file=sys.stderr)
        sys.exit(1)
    if errors:
        for e in errors:
            print(f"precision: {e}", file=sys.stderr)
        sys.exit(2)

    rows = summarize(per_repo)
    ready = sum(r["status"] == "gate-ready" for r in rows)
    if args.json:
        json.dump({"rows": rows, "gate_ready": ready, "pairs": len(rows)}, sys.stdout, indent=2)
        print()
        return
    print(f"{'rule':<12} {'language':<11} {'tp':>4} {'fp':>4} {'unlab':>6} {'repos':>5} {'coverage':>8} {'precision':>9}  status")
    for r in rows:
        cov = "n/a" if r["coverage"] is None else f"{r['coverage']:.0%}"
        prec = "n/a" if r["precision"] is None else f"{r['precision']:.0%}"
        print(f"{r['rule']:<12} {r['language']:<11} {r['tp']:>4} {r['fp']:>4} {r['unlabelled']:>6} "
              f"{r['repos']:>5} {cov:>8} {prec:>9}  {r['status']}")
    print(f"\ngate-ready: {ready} of {len(rows)} rule-language pairs. "
          "Precision targets apply only to gate-ready pairs.")


if __name__ == "__main__":
    main()
```

Run: `cd scripts && python3 -m unittest test_precision -v`
Expected: every test passes.

- [ ] **Step 3: Label the self-scan dead-code findings**

1. Check out commit `a809171` in a temp worktree: `git worktree add /tmp/stratify-a809171 a809171`.
2. Build the current branch's binary: `cargo build --release -p stratify-cli`.
3. Scan the worktree with `--format json`.
4. For each `dead_code` finding, find its caller with `grep -rn "<name>(" /tmp/stratify-a809171/crates --include=*.rs`, and record a label.

Expected result: 15 findings, all `fp`. Each label's `note` names the call site (file and line), or reads `test-only helper under #[cfg(test)]`. If a finding has no caller, label it `tp` and say so in your report.

Write `corpus/labels/stratify.json` with `"repo": "stratify"`, `"complete_rules": ["dead_code"]`, and the labels. Do not label other rules in this task. Afterwards, remove the worktree with `git worktree remove /tmp/stratify-a809171`.

`corpus/repos.json`:

```json
[
  {"name": "stratify", "url": "https://github.com/stratify-dev/stratify", "commit": "a809171"}
]
```

- [ ] **Step 4: Write `corpus/README.md`**

```markdown
# Labelled corpus

Real repositories at pinned commits, with Stratify findings marked true
(`tp`) or false (`fp`). `scripts/precision.py` scans each repo and prints
one row per rule and language.

    cargo build --release -p stratify-cli
    python3 scripts/precision.py --stratify target/release/stratify

## Files

- `repos.json`: name, clone URL, pinned commit.
- `labels/<name>.json`: `repo`, `complete_rules` (rules labelled
  exhaustively in this repo), and `labels`, one per finding: `rule`, `file`,
  `line`, `match` (a substring of the message), `label`, `note`.

## Matching and validation

A label matches a finding when rule, file, and line are equal and `match`
is a substring of the message. The script exits 2 and lists every problem
when a label is malformed, matches no finding (stale), matches several
findings (ambiguous), when a finding matches several labels, or when a
finding of a `complete_rules` rule has no label.

## Rules for labelling

- `tp` means a maintainer would act on the finding.
- `fp` means the finding is wrong. Say why in `note`, with the call site or
  config that proves it.
- A label is tied to a pinned commit. Moving the commit means relabelling.

## When a precision number counts

Precision counts labelled findings only. A rule and language pair is
`gate-ready` when every finding of that rule in that language is labelled,
the labels come from 2 or more repos, and 30 or more findings are labelled.
The roadmap's precision targets apply only to gate-ready pairs. Every other
pair reports `no-data`, `incomplete`, or `insufficient`, and its precision
number is a sample, not a measurement.

## Coverage

| Repo | Language | Rules labelled exhaustively |
|------|----------|-----------------------------|
| stratify @ a809171 | rust | dead_code |

Gate-ready pairs: 0 of 36. Phase 0 needs all 36.
```

- [ ] **Step 5: CI, run end to end, commit**

In `.github/workflows/ci.yml`, extend the `Script tests` step from Task 6 to:

```yaml
      - name: Script tests
        run: |
          bash scripts/test_action_run.sh
          python3 -m unittest discover -s scripts -p 'test_*.py'
```

Run: `python3 scripts/precision.py --stratify target/release/stratify`
Expected:
- a `dead_code rust` row with `fp` 15 (or the Step 3 count), coverage `100%`, and status `insufficient` (one repo, under 30 labels);
- every other pair `no-data` or `incomplete`;
- the footer `gate-ready: 0 of 36`.

Paste the table into your report.

```bash
git add corpus scripts/precision.py scripts/test_precision.py .github/workflows/ci.yml
git commit -m "feat(corpus): add a labelled corpus and a precision script"
```

---

### Task 8: Scan-time and memory benchmark

**Files:**
- Modify: `crates/stratify-cli/src/run.rs`, `crates/stratify-cli/src/main.rs`
- Create: `crates/stratify-cli/tests/e2e_stats.rs`
- Create: `scripts/bench.py`, `scripts/test_bench.py`
- Create: `docs/benchmarks.md`

**Interfaces:**
- Consumes: `run::ScanStats` and `run::scan_stats`.
- Produces:
  - `ScanStats.lines_scanned: u64`: lines of every file the adapters parsed, after `[ignore]` globs and the walker's own skips. These are the files Stratify scans.
  - `run::scan_stats(graph: &IrGraph, duration_ms: u64, lines_scanned: u64) -> ScanStats`
  - `stratify check --stats` prints one line to stderr after the report: `stratify-stats {"files_scanned":..,"lines_scanned":..,"functions":..,"languages":[..],"duration_ms":..}`.
  - `python3 scripts/bench.py --stratify <bin> <repo>` prints one JSON object: `files`, `lines`, `functions`, `wall_s`, `peak_rss_mb`. It exits non-zero on any failure (see Step 4).

The counts come from Stratify itself, so the benchmark measures exactly the files the scan reads. The script uses no `grep -z`, `xargs` or `/usr/bin/time`, so it runs the same on macOS and Linux.

- [ ] **Step 1: Write the failing Rust tests**

In `run.rs` `stats_tests`, update the two existing `scan_stats` calls to pass a third argument (`0`). Add:

```rust
    #[test]
    fn scan_counts_lines_of_parsed_files_only() {
        let dir = std::env::temp_dir().join("stratify-stats-lines");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.rb"), "def a\nend\n\na\n").unwrap(); // 4 lines
        std::fs::write(dir.join("notes.txt"), "not\nsource\n").unwrap(); // not parsed
        let (_, stats) = analyze_repo_with_stats(&dir).unwrap();
        assert_eq!(stats.files_scanned, 1);
        assert_eq!(stats.lines_scanned, 4);
    }
```

`crates/stratify-cli/tests/e2e_stats.rs`:

```rust
use std::path::Path;

#[test]
fn stats_flag_prints_one_json_line_on_stderr() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/sample-ruby");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_stratify"))
        .args(["check", dir.to_str().unwrap(), "--format", "json", "--stats"])
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
        .map(|f| std::fs::read_to_string(dir.join(f)).unwrap().lines().count() as u64)
        .sum();
    assert_eq!(v["files_scanned"], 2);
    assert_eq!(v["lines_scanned"], expected_lines);
    // stdout stays a clean JSON report
    let _: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
}
```

Run: `cargo test -p stratify-cli scan_counts_lines && cargo test -p stratify-cli --test e2e_stats`
Expected: compile error (`lines_scanned` not found).

- [ ] **Step 2: Implement**

In `run.rs`:
1. Add `pub lines_scanned: u64` to `ScanStats`.
2. Add a `lines_scanned: u64` parameter to `scan_stats` and store it.
3. In `analyze_repo_with_stats`, declare `let mut lines_scanned: u64 = 0;` before the walk loop. Add `lines_scanned += source.lines().count() as u64;` right before `adapter.parse_file`, after the ignore check.
4. Pass it to `scan_stats`.

In `main.rs`:
1. Add to `Command::Check`, and to its destructuring in `main`:

```rust
        /// Print scan statistics as one JSON line on stderr, prefixed `stratify-stats `.
        #[arg(long = "stats")]
        print_stats: bool,
```

2. Right after `analyze_repo_with_stats` succeeds, before any baseline handling:

```rust
            if print_stats {
                eprintln!(
                    "stratify-stats {}",
                    serde_json::json!({
                        "files_scanned": stats.files_scanned,
                        "lines_scanned": stats.lines_scanned,
                        "functions": stats.functions,
                        "languages": stats.languages,
                        "duration_ms": stats.duration_ms,
                    })
                );
            }
```

Run: `cargo test --workspace`
Expected: pass.

- [ ] **Step 3: Write the failing script test**

`scripts/test_bench.py`:

```python
import unittest

from bench import parse_stats, peak_rss_mb


class BenchTest(unittest.TestCase):
    def test_parse_stats_finds_the_prefixed_line(self):
        stderr = "warning: something\nstratify-stats {\"files_scanned\": 2, \"lines_scanned\": 9}\n"
        self.assertEqual(parse_stats(stderr)["lines_scanned"], 9)

    def test_parse_stats_without_line_raises(self):
        with self.assertRaises(ValueError):
            parse_stats("stratify: path not found\n")

    def test_peak_rss_units(self):
        self.assertEqual(peak_rss_mb(512 * 1024 * 1024, "darwin"), 512.0)  # bytes on macOS
        self.assertEqual(peak_rss_mb(512 * 1024, "linux"), 512.0)          # KiB on Linux


if __name__ == "__main__":
    unittest.main()
```

Run: `cd scripts && python3 -m unittest test_bench -v`
Expected: `ModuleNotFoundError: No module named 'bench'`.

- [ ] **Step 4: Implement `scripts/bench.py`**

```python
#!/usr/bin/env python3
"""Measure one full `stratify check` run: wall time, peak memory, and the
files and lines Stratify itself reports scanning.

Usage: python3 scripts/bench.py --stratify BIN REPO

Prints one JSON object. Exits non-zero and prints the scan's stderr when the
scan exits non-zero or prints no stats line. Runs the same on macOS and Linux.
"""
import argparse
import json
import resource
import subprocess
import sys
import time

STATS_PREFIX = "stratify-stats "


def parse_stats(stderr):
    for line in reversed(stderr.splitlines()):
        if line.startswith(STATS_PREFIX):
            return json.loads(line[len(STATS_PREFIX):])
    raise ValueError("no stratify-stats line on stderr")


def peak_rss_mb(ru_maxrss, platform):
    """ru_maxrss is in bytes on macOS and in KiB on Linux."""
    divisor = 1024 * 1024 if platform == "darwin" else 1024
    return round(ru_maxrss / divisor, 1)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--stratify", required=True)
    parser.add_argument("repo")
    args = parser.parse_args()

    cmd = [args.stratify, "check", args.repo, "--format", "json", "--stats"]
    start = time.monotonic()
    proc = subprocess.run(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
    wall = time.monotonic() - start
    if proc.returncode != 0:
        sys.stderr.write(proc.stderr)
        sys.exit(f"bench: {' '.join(cmd)} exited {proc.returncode}")
    try:
        stats = parse_stats(proc.stderr)
    except ValueError as e:
        sys.stderr.write(proc.stderr)
        sys.exit(f"bench: {e}")
    rss = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
    print(json.dumps({
        "files": stats["files_scanned"],
        "lines": stats["lines_scanned"],
        "functions": stats["functions"],
        "wall_s": round(wall, 2),
        "peak_rss_mb": peak_rss_mb(rss, sys.platform),
    }))


if __name__ == "__main__":
    main()
```

Run: `cd scripts && python3 -m unittest test_bench -v`
Expected: pass.

Run on this repo: `cargo build --release -p stratify-cli && python3 scripts/bench.py --stratify target/release/stratify .`
Expected: one JSON line with every value filled.

Check failure reporting: `python3 scripts/bench.py --stratify target/release/stratify /no/such/path; echo "exit $?"`
Expected: stratify's `path not found` message and a non-zero exit.

- [ ] **Step 5: Measure one large repo**

```bash
git clone --depth 1 --branch v1.31.0 https://github.com/kubernetes/kubernetes /tmp/k8s
python3 scripts/bench.py --stratify target/release/stratify /tmp/k8s
```

If the clone or the scan fails, record the failure and its message in your report. Do not substitute a smaller repo. A missing large-repo row keeps Phase 0 open.

- [ ] **Step 6: Record and commit**

`docs/benchmarks.md`:

```markdown
# Benchmarks

Full `stratify check --format json` runs measured with `scripts/bench.py`.
Files and lines are what Stratify reports scanning (`--stats`), after
`.gitignore`, hidden-file skips, and `[ignore]` globs. The roadmap's
scan-time budget comes from the large-repo row.

| Date | Stratify | Repo | Files | Lines | Wall (s) | Peak RSS (MB) | Machine |
|------|----------|------|-------|-------|----------|---------------|---------|
| <date> | <git rev-parse --short HEAD> | stratify (self) | ... | ... | ... | ... | <uname -sm, CPU model> |
| <date> | <rev> | kubernetes v1.31.0 | ... | ... | ... | ... | ... |
```

Fill every cell from measured output. If Step 5 failed, write the kubernetes row as `not measured: <reason>` in the Files cell and leave the rest empty.

In `CHANGELOG.md` under `## [Unreleased]` `### Added`:

```markdown
- `--stats` prints scan statistics (files, lines, functions, languages,
  duration) as one `stratify-stats` JSON line on stderr.
```

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test --workspace
git add crates/stratify-cli scripts/bench.py scripts/test_bench.py docs/benchmarks.md CHANGELOG.md
git commit -m "feat(cli): add --stats and a cross-platform scan benchmark"
```

---

### Task 9: Update the roadmap status

**Files:**
- Modify: `docs/roadmap-full-stack-static-analysis.md`

The roadmap's Phase 0 exit criteria, Phase 1 evidence contract, and schema decision are already in place. This task records status only.

- [ ] **Step 1: Edit the roadmap**

1. In "Where Stratify stands today", update three rows:
   - Languages: `All six languages get six of six`.
   - Findings: add `related`, `path`, and `reasons`.
   - Gate: `--fail-on severity threshold, --baseline new-findings gate, stratify-ignore comment suppression`.
2. Under Phase 0, add a `Status: in progress.` line. Follow it with a list of what shipped and what remains:
   - shipped: Rust cycles and boundaries; the precision tool; the first labels (quote the Task 7 table's dead_code rust row and the `gate-ready: N of 36` footer); the benchmark tool and the self-scan row from `docs/benchmarks.md`;
   - remaining: labelled repos until all 36 rule-language pairs are gate-ready; the large-repo benchmark row, if `docs/benchmarks.md` marks it `not measured`; the scan-time budget, which is set from that row.
   - Do not call Phase 0 complete.
3. Under Phase 1, add a `Status:` line listing the shipped items:
   - rule registry;
   - evidence fields and the SARIF mapping;
   - comment suppression;
   - baseline gate.
   - Phase 1's judge-compatibility exit test needs a run of `stratify-judge` against a report with evidence fields. Mark it `not yet run` unless someone has run it.

- [ ] **Step 2: Commit**

```bash
git add docs/roadmap-full-stack-static-analysis.md
git commit -m "docs: record roadmap phase 0 and phase 1 status"
```
