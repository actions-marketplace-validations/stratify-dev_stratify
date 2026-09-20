# stratify-jev Plan 1: Walking Skeleton Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A working `stratify-jev` binary that reads a Stratify JSON report on stdin, asks Jev whether each `dead_code` finding is reachable by something the call graph cannot see, and re-emits the report with confidence and severity adjusted.

**Architecture:** Three crates in a new Cargo workspace. `jev-client` is a typed async HTTP client over TypeSafe's `POST /v1/systemone`. `stratify-jev-judge` owns the report model, repo context, verdict application, the answer cache, and the `dead_code` judge. `stratify-jev-cli` wires stdin, arguments, concurrency, and output. Findings are never deleted; a dismissed finding drops below the display threshold and keeps its full judgment in JSON.

**Tech Stack:** Rust 2021, tokio, reqwest (rustls), serde, serde_json, clap 4, globset, ignore, sha2, wiremock (dev), insta (dev).

**Spec:** `docs/superpowers/specs/2026-09-20-stratify-jev-design.md` (in the `stratify` repo). Read it before Task 1.

## Global Constraints

- **New repo.** All paths below are relative to a fresh repo at `~/dev/stratify-jev`. Nothing in `~/dev/stratify` is modified by this plan.
- **Rust edition 2021.** Workspace version starts at `0.1.0`.
- **No dependency on `stratify-core`.** The JSON report is the contract. Report types are redefined locally and round-trip unknown fields so a newer engine's output is never silently dropped.
- **API key source is `TYPESAFE_API_KEY` only.** Never a config field, never a CLI flag.
- **Endpoint:** `POST https://api.typesafe.ai/v1/systemone`. Model default `jev-latest`.
- **Every failure is a pass-through.** Missing key, network error, malformed answer: the report goes out unchanged and the exit code still follows `--fail-on`.
- **Confidence ordering is `Unknown < Likely < Certain`.** Severity ordering is `Info < Warning < Error`.
- **No em dashes in user-facing strings or docs.** Use commas or periods.
- **Every commit ends with:**
  ```
  Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
  ```
- **TDD throughout.** Test first, watch it fail, minimal implementation, watch it pass, commit.

## File Structure

```
~/dev/stratify-jev/
  Cargo.toml                                  workspace manifest
  rust-toolchain.toml                         pinned stable
  .gitignore
  README.md
  crates/
    jev-client/
      Cargo.toml
      src/lib.rs                              re-exports
      src/question.rs                         Question, NoulCriteria, request types
      src/answer.rs                           Answer, Usage, response types
      src/client.rs                           transport, auth, retry
    stratify-jev-judge/
      Cargo.toml
      src/lib.rs                              re-exports
      src/model.rs                            Report, Finding, Severity, Confidence, Span
      src/context.rs                          RepoContext, FileEntry, source reads, ignore globs
      src/verdict.rs                          Verdict, Judgment, apply()
      src/cache.rs                            cache key, read, write
      src/config.rs                           [jev] table, thresholds
      src/judges/mod.rs                       Judge trait, registry
      src/judges/dead_code.rs                 state builder, questions, policy
      src/driver.rs                           cache split, batching, concurrency, scatter
    stratify-jev-cli/
      Cargo.toml
      src/main.rs                             clap, orchestration, exit code
      src/output/human.rs                     human renderer plus summary line
      src/output/json.rs                      JSON renderer
  tests/fixtures/
    sample-repo/                              tiny Rust repo with known dead code
    report-dead-code.json                     captured engine output for that repo
```

---

### Task 1: Workspace scaffold and the report model

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `.gitignore`
- Create: `crates/stratify-jev-judge/Cargo.toml`
- Create: `crates/stratify-jev-judge/src/lib.rs`
- Create: `crates/stratify-jev-judge/src/model.rs`
- Test: inline `#[cfg(test)] mod tests` in `model.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `Severity`, `Confidence`, `Span`, `Finding`, `Report`. `Severity` and `Confidence` derive `PartialOrd`/`Ord` with `Info < Warning < Error` and `Unknown < Likely < Certain`. `Finding` carries `extra: serde_json::Map<String, Value>` via `#[serde(flatten)]`.

- [ ] **Step 1: Create the repo and workspace manifest**

```bash
mkdir -p ~/dev/stratify-jev/crates/stratify-jev-judge/src
cd ~/dev/stratify-jev
git init
printf 'target\n' > .gitignore
printf '[toolchain]\nchannel = "1.75"\n' > rust-toolchain.toml
```

`Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = ["crates/stratify-jev-judge"]

[workspace.package]
edition = "2021"
version = "0.1.0"
license = "MIT OR Apache-2.0"
repository = "https://github.com/stratify-dev/stratify-jev"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = { version = "1", features = ["preserve_order"] }
```

`crates/stratify-jev-judge/Cargo.toml`:

```toml
[package]
name = "stratify-jev-judge"
edition.workspace = true
version.workspace = true
license.workspace = true

[dependencies]
serde.workspace = true
serde_json.workspace = true
```

`crates/stratify-jev-judge/src/lib.rs`:

```rust
pub mod model;
```

- [ ] **Step 2: Write the failing test**

Append to `crates/stratify-jev-judge/src/model.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "schema_version": 1,
      "findings": [{
        "rule": "dead_code",
        "severity": "info",
        "message": "possibly unused function `helper`",
        "span": {"file":"src/a.rs","start_byte":0,"end_byte":10,"start_line":3},
        "confidence": "likely",
        "future_field": 42
      }]
    }"#;

    #[test]
    fn parses_an_engine_report() {
        let r: Report = serde_json::from_str(SAMPLE).unwrap();
        assert_eq!(r.schema_version, 1);
        assert_eq!(r.findings[0].rule, "dead_code");
        assert_eq!(r.findings[0].severity, Severity::Info);
        assert_eq!(r.findings[0].confidence, Confidence::Likely);
        assert_eq!(r.findings[0].span.start_line, 3);
    }

    #[test]
    fn round_trip_preserves_unknown_fields() {
        let r: Report = serde_json::from_str(SAMPLE).unwrap();
        let back = serde_json::to_value(&r).unwrap();
        assert_eq!(back["findings"][0]["future_field"], 42);
    }

    #[test]
    fn severity_and_confidence_order_low_to_high() {
        assert!(Severity::Info < Severity::Warning);
        assert!(Severity::Warning < Severity::Error);
        assert!(Confidence::Unknown < Confidence::Likely);
        assert!(Confidence::Likely < Confidence::Certain);
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p stratify-jev-judge`
Expected: FAIL, `cannot find type Report in this scope`.

- [ ] **Step 4: Write the model**

Prepend to `crates/stratify-jev-judge/src/model.rs`:

```rust
use serde::{Deserialize, Serialize};

/// Ordering matters: Info < Warning < Error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl Severity {
    /// One step down. Info is the floor and stays Info.
    pub fn step_down(self) -> Severity {
        match self {
            Severity::Error => Severity::Warning,
            Severity::Warning => Severity::Info,
            Severity::Info => Severity::Info,
        }
    }
}

/// Ordering matters: Unknown < Likely < Certain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    Unknown,
    Likely,
    Certain,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub file: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: usize,
}

/// One engine finding. `extra` round-trips any field a newer engine adds,
/// including the `judgment` object this tool writes back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub rule: String,
    pub severity: Severity,
    pub message: String,
    pub span: Span,
    pub confidence: Confidence,
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub schema_version: u32,
    pub findings: Vec<Finding>,
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Report {
    /// Schema this tool understands. A higher value is passed through untouched.
    pub const KNOWN_SCHEMA_VERSION: u32 = 1;
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p stratify-jev-judge`
Expected: PASS, 3 tests.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
feat: workspace scaffold and the engine report model

Report types are redefined locally rather than depending on
stratify-core: the versioned JSON is the contract between the two
tools. Unknown fields round-trip so a newer engine's output is never
silently dropped.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: RepoContext, source reads, and ignore globs

**Files:**
- Create: `crates/stratify-jev-judge/src/context.rs`
- Create: `tests/fixtures/sample-repo/stratify.toml`
- Create: `tests/fixtures/sample-repo/src/lib.rs`
- Create: `tests/fixtures/sample-repo/vendor/skipme.rs`
- Modify: `crates/stratify-jev-judge/src/lib.rs`
- Modify: `crates/stratify-jev-judge/Cargo.toml`
- Test: inline `#[cfg(test)] mod tests` in `context.rs`

**Interfaces:**
- Consumes: `model::Span` from Task 1.
- Produces: `RepoContext::new(root: PathBuf) -> io::Result<RepoContext>`, `RepoContext::function_source(&self, span: &Span) -> Option<String>` (span expanded to whole lines), `RepoContext::file_text(&self, rel: &str) -> Option<String>`, `RepoContext::files(&self) -> &[FileEntry]`, `RepoContext::root(&self) -> &Path`, `FileEntry { pub rel: String, pub language: Option<&'static str> }`, and `language_of(path: &str) -> Option<&'static str>`.

- [ ] **Step 1: Add dependencies**

In `crates/stratify-jev-judge/Cargo.toml` under `[dependencies]`:

```toml
globset = "0.4"
ignore = "0.4"
toml = "0.8"
```

And in the workspace `Cargo.toml` under `[workspace.dependencies]` add nothing; these are crate-local.

- [ ] **Step 2: Create the fixture repo**

`tests/fixtures/sample-repo/stratify.toml`:

```toml
[ignore]
paths = ["vendor/**"]
```

`tests/fixtures/sample-repo/src/lib.rs`:

```rust
pub fn used() -> u32 {
    helper() + 1
}

fn helper() -> u32 {
    41
}

#[allow(dead_code)]
fn orphan() -> u32 {
    0
}
```

`tests/fixtures/sample-repo/vendor/skipme.rs`:

```rust
fn vendored() {}
```

- [ ] **Step 3: Write the failing test**

Append to `crates/stratify-jev-judge/src/context.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Span;
    use std::path::PathBuf;

    fn fixture() -> RepoContext {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/sample-repo");
        RepoContext::new(root).unwrap()
    }

    #[test]
    fn walk_honors_stratify_toml_ignore_paths() {
        let ctx = fixture();
        let rels: Vec<&str> = ctx.files().iter().map(|f| f.rel.as_str()).collect();
        assert!(rels.contains(&"src/lib.rs"));
        assert!(
            !rels.iter().any(|r| r.starts_with("vendor/")),
            "vendor/** is ignored in stratify.toml, got {rels:?}"
        );
    }

    #[test]
    fn function_source_expands_a_span_to_whole_lines() {
        let ctx = fixture();
        let text = ctx.file_text("src/lib.rs").unwrap();
        // Byte range covering only "fn helper" inside the helper declaration.
        let start = text.find("fn helper").unwrap();
        let span = Span {
            file: "src/lib.rs".into(),
            start_byte: start,
            end_byte: start + 9,
            start_line: 5,
        };
        let src = ctx.function_source(&span).unwrap();
        assert!(src.starts_with("fn helper() -> u32 {"), "got: {src:?}");
        assert!(src.ends_with('\n') || src.ends_with('}'), "got: {src:?}");
    }

    #[test]
    fn function_source_is_none_for_a_missing_file() {
        let ctx = fixture();
        let span = Span {
            file: "src/nope.rs".into(),
            start_byte: 0,
            end_byte: 1,
            start_line: 1,
        };
        assert!(ctx.function_source(&span).is_none());
    }

    #[test]
    fn language_is_derived_from_the_extension() {
        assert_eq!(language_of("a/b.rs"), Some("rust"));
        assert_eq!(language_of("a/b.tsx"), Some("typescript"));
        assert_eq!(language_of("a/b.txt"), None);
    }
}
```

- [ ] **Step 4: Run the tests to verify they fail**

Run: `cargo test -p stratify-jev-judge context`
Expected: FAIL, `cannot find type RepoContext in this scope`.

- [ ] **Step 5: Write RepoContext**

Prepend to `crates/stratify-jev-judge/src/context.rs`:

```rust
use crate::model::Span;
use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;
use serde::Deserialize;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

/// One file in scope, matching what the engine would have parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub rel: String,
    pub language: Option<&'static str>,
}

/// Map a path to the engine's language name, by extension.
pub fn language_of(path: &str) -> Option<&'static str> {
    match path.rsplit('.').next()? {
        "java" => Some("java"),
        "rb" => Some("ruby"),
        "ts" | "tsx" | "mts" | "cts" => Some("typescript"),
        "py" | "pyi" => Some("python"),
        "go" => Some("go"),
        "rs" => Some("rust"),
        _ => None,
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
struct IgnoreSection {
    #[serde(default)]
    paths: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct IgnoreToml {
    #[serde(default)]
    ignore: IgnoreSection,
}

/// Everything a judge needs about the repo under analysis: the file
/// inventory and on-demand source reads, both scoped by the same
/// `stratify.toml` `[ignore] paths` globs the engine honors.
pub struct RepoContext {
    root: PathBuf,
    files: Vec<FileEntry>,
    cache: RefCell<HashMap<String, Option<String>>>,
}

impl RepoContext {
    pub fn new(root: PathBuf) -> io::Result<Self> {
        let globs = load_ignore_globs(&root);
        let mut files = Vec::new();
        for entry in WalkBuilder::new(&root).build() {
            let Ok(entry) = entry else { continue };
            if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                continue;
            }
            let rel = entry
                .path()
                .strip_prefix(&root)
                .unwrap_or(entry.path())
                .to_string_lossy()
                .replace('\\', "/");
            if globs.is_match(&rel) {
                continue;
            }
            let language = language_of(&rel);
            if language.is_none() {
                continue;
            }
            files.push(FileEntry { rel, language });
        }
        files.sort_by(|a, b| a.rel.cmp(&b.rel));
        Ok(RepoContext {
            root,
            files,
            cache: RefCell::new(HashMap::new()),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn files(&self) -> &[FileEntry] {
        &self.files
    }

    /// Whole-file text, read once and memoized. None when unreadable.
    pub fn file_text(&self, rel: &str) -> Option<String> {
        if let Some(hit) = self.cache.borrow().get(rel) {
            return hit.clone();
        }
        let read = std::fs::read_to_string(self.root.join(rel)).ok();
        self.cache.borrow_mut().insert(rel.to_string(), read.clone());
        read
    }

    /// The span's byte range widened to whole lines, which is what a model
    /// needs to read a function. Byte offsets that do not land on a char
    /// boundary are clamped outward to the nearest one.
    pub fn function_source(&self, span: &Span) -> Option<String> {
        let text = self.file_text(&span.file)?;
        if text.is_empty() {
            return None;
        }
        let start = span.start_byte.min(text.len());
        let end = span.end_byte.clamp(start, text.len());
        let line_start = text[..start].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let line_end = text[end..]
            .find('\n')
            .map(|i| end + i)
            .unwrap_or_else(|| text.len());
        let mut lo = line_start;
        while lo < text.len() && !text.is_char_boundary(lo) {
            lo += 1;
        }
        let mut hi = line_end;
        while hi < text.len() && !text.is_char_boundary(hi) {
            hi += 1;
        }
        text.get(lo..hi).map(|s| s.to_string())
    }
}

/// Read `[ignore] paths` from `stratify.toml` at the root. Absent or
/// unparseable means no globs, matching the engine's own fallback.
fn load_ignore_globs(root: &Path) -> GlobSet {
    let Ok(text) = std::fs::read_to_string(root.join("stratify.toml")) else {
        return GlobSet::empty();
    };
    let cfg: IgnoreToml = toml::from_str(&text).unwrap_or_default();
    let mut b = GlobSetBuilder::new();
    for p in &cfg.ignore.paths {
        if let Ok(g) = Glob::new(p) {
            b.add(g);
        }
    }
    b.build().unwrap_or_else(|_| GlobSet::empty())
}
```

Add to `crates/stratify-jev-judge/src/lib.rs`:

```rust
pub mod context;
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p stratify-jev-judge`
Expected: PASS, 7 tests.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
feat: RepoContext with source reads and stratify.toml ignore globs

Both tools must agree on what is in scope, so the walk reads the same
[ignore] paths table the engine reads and falls back the same way when
the file is absent or unparseable.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: jev-client request and answer types

**Files:**
- Create: `crates/jev-client/Cargo.toml`
- Create: `crates/jev-client/src/lib.rs`
- Create: `crates/jev-client/src/question.rs`
- Create: `crates/jev-client/src/answer.rs`
- Modify: `Cargo.toml` (workspace members)
- Test: inline `#[cfg(test)] mod tests` in both new modules

**Interfaces:**
- Consumes: nothing.
- Produces: `Question` (enum with `Noul`, `Choice`, `Score` variants), `NoulCriteria`, `SystemOneRequest { state, model, questions }`, `SystemOneResponse { model, answers, usage }`, `Answer` with accessors `as_noul() -> Option<f64>`, `as_choice() -> Option<(&str, f64)>`, `as_score() -> Option<(f64, f64)>`, and `Usage { input_tokens, output_tokens }`.

- [ ] **Step 1: Create the crate**

```bash
mkdir -p ~/dev/stratify-jev/crates/jev-client/src
```

`crates/jev-client/Cargo.toml`:

```toml
[package]
name = "jev-client"
edition.workspace = true
version.workspace = true
license.workspace = true

[dependencies]
serde.workspace = true
serde_json.workspace = true
```

Workspace `Cargo.toml` members become:

```toml
members = ["crates/jev-client", "crates/stratify-jev-judge"]
```

`crates/jev-client/src/lib.rs`:

```rust
pub mod answer;
pub mod question;

pub use answer::{Answer, SystemOneResponse, Usage};
pub use question::{NoulCriteria, Question, SystemOneRequest};
```

- [ ] **Step 2: Write the failing test for questions**

Append to `crates/jev-client/src/question.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn noul_serializes_with_its_type_tag_and_criteria() {
        let q = Question::noul(
            "Would a framework invoke `finding_0.function`?",
            Some(NoulCriteria {
                yes: "A framework, container, or route table invokes it.".into(),
                no: "Only explicit calls in source would reach it.".into(),
            }),
        );
        let v = serde_json::to_value(&q).unwrap();
        assert_eq!(v["type"], "noul");
        assert_eq!(v["criteria"]["true"], "A framework, container, or route table invokes it.");
    }

    #[test]
    fn noul_without_criteria_omits_the_key() {
        let q = Question::noul("Is it test-only?", None);
        let v = serde_json::to_value(&q).unwrap();
        assert!(v.get("criteria").is_none(), "got {v}");
    }

    #[test]
    fn choice_keeps_its_option_keys() {
        let q = Question::choice(
            "Why does nothing call it?",
            [("genuinely_unused", "No caller exists anywhere."),
             ("cannot_tell", "The evidence does not settle it.")],
        );
        let v = serde_json::to_value(&q).unwrap();
        assert_eq!(v["type"], "choice");
        assert_eq!(v["criteria"]["cannot_tell"], "The evidence does not settle it.");
    }

    #[test]
    fn score_criteria_stay_ordered_low_to_high() {
        let q = Question::score("How valuable is extraction?", ["Level 0", "Level 1", "Level 2"]);
        let v = serde_json::to_value(&q).unwrap();
        assert_eq!(v["criteria"][0], "Level 0");
        assert_eq!(v["criteria"][2], "Level 2");
    }

    #[test]
    fn request_carries_state_model_and_named_questions() {
        let req = SystemOneRequest {
            state: json!({ "finding_0": { "name": "helper" } }),
            model: "jev-latest".into(),
            questions: [("framework_invoked".to_string(), Question::noul("q", None))]
                .into_iter()
                .collect(),
        };
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["model"], "jev-latest");
        assert_eq!(v["state"]["finding_0"]["name"], "helper");
        assert_eq!(v["questions"]["framework_invoked"]["type"], "noul");
    }
}
```

- [ ] **Step 3: Run to verify it fails**

Run: `cargo test -p jev-client`
Expected: FAIL, `cannot find type Question in this scope`.

- [ ] **Step 4: Write the question types**

Prepend to `crates/jev-client/src/question.rs`:

```rust
use serde::Serialize;
use std::collections::BTreeMap;

/// Optional yes/no clarification on a Noul. The API names these keys
/// "true" and "false".
#[derive(Debug, Clone, Serialize)]
pub struct NoulCriteria {
    #[serde(rename = "true")]
    pub yes: String,
    #[serde(rename = "false")]
    pub no: String,
}

/// One typed question. Instructions are a JSON value so a caller can send
/// a plain string or a structured object, as the API allows.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Question {
    Noul {
        instructions: serde_json::Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        criteria: Option<NoulCriteria>,
    },
    Choice {
        instructions: serde_json::Value,
        /// At most 255 options. Always include a no-match option.
        criteria: BTreeMap<String, String>,
    },
    Score {
        instructions: serde_json::Value,
        /// 2 to 10 levels, ordered low to high, each describing a situation.
        criteria: Vec<String>,
    },
}

impl Question {
    pub fn noul(instructions: impl Into<String>, criteria: Option<NoulCriteria>) -> Question {
        Question::Noul {
            instructions: serde_json::Value::String(instructions.into()),
            criteria,
        }
    }

    pub fn choice<K, V>(
        instructions: impl Into<String>,
        options: impl IntoIterator<Item = (K, V)>,
    ) -> Question
    where
        K: Into<String>,
        V: Into<String>,
    {
        Question::Choice {
            instructions: serde_json::Value::String(instructions.into()),
            criteria: options
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        }
    }

    pub fn score<S: Into<String>>(
        instructions: impl Into<String>,
        levels: impl IntoIterator<Item = S>,
    ) -> Question {
        Question::Score {
            instructions: serde_json::Value::String(instructions.into()),
            criteria: levels.into_iter().map(Into::into).collect(),
        }
    }
}

/// One POST body for /v1/systemone.
#[derive(Debug, Clone, Serialize)]
pub struct SystemOneRequest {
    pub state: serde_json::Value,
    pub model: String,
    pub questions: BTreeMap<String, Question>,
}
```

- [ ] **Step 5: Write the failing test for answers**

Append to `crates/jev-client/src/answer.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const BODY: &str = r#"{
      "model": "jev-1.13.0",
      "answers": {
        "framework_invoked": { "noul": 0.91 },
        "explanation": {
          "choice": "framework_invoked",
          "probabilities": { "framework_invoked": 0.88, "cannot_tell": 0.12 },
          "confidence": 0.88
        },
        "cognitive_load": {
          "score": 2.4,
          "legend": { "0": "flat", "3": "tangled" },
          "probabilities": { "0": 0.1, "3": 0.5 },
          "confidence": 0.62
        }
      },
      "usage": { "input_tokens": 412, "output_tokens": 0 }
    }"#;

    #[test]
    fn parses_all_three_answer_shapes() {
        let r: SystemOneResponse = serde_json::from_str(BODY).unwrap();
        assert_eq!(r.model, "jev-1.13.0");
        assert_eq!(r.usage.input_tokens, 412);
        assert_eq!(r.answers["framework_invoked"].as_noul(), Some(0.91));
        assert_eq!(
            r.answers["explanation"].as_choice(),
            Some(("framework_invoked", 0.88))
        );
        assert_eq!(r.answers["cognitive_load"].as_score(), Some((2.4, 0.62)));
    }

    #[test]
    fn accessors_return_none_for_the_wrong_shape() {
        let r: SystemOneResponse = serde_json::from_str(BODY).unwrap();
        assert_eq!(r.answers["framework_invoked"].as_choice(), None);
        assert_eq!(r.answers["explanation"].as_noul(), None);
        assert_eq!(r.answers["cognitive_load"].as_noul(), None);
    }
}
```

- [ ] **Step 6: Run to verify it fails**

Run: `cargo test -p jev-client answer`
Expected: FAIL, `cannot find type SystemOneResponse in this scope`.

- [ ] **Step 7: Write the answer types**

Prepend to `crates/jev-client/src/answer.rs`:

```rust
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One answer. Variants are tried in order, so the most constrained shape
/// (Noul, which has only `noul`) comes first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Answer {
    Noul {
        noul: f64,
    },
    Choice {
        choice: String,
        probabilities: BTreeMap<String, f64>,
        confidence: f64,
    },
    Score {
        score: f64,
        legend: BTreeMap<String, String>,
        probabilities: BTreeMap<String, f64>,
        confidence: f64,
    },
}

impl Answer {
    /// Probability of yes, 0 to 1. A Noul carries no separate confidence:
    /// 0.5 means yes and no are equally likely, not "medium intensity".
    pub fn as_noul(&self) -> Option<f64> {
        match self {
            Answer::Noul { noul } => Some(*noul),
            _ => None,
        }
    }

    /// The selected option and the distribution's confidence.
    pub fn as_choice(&self) -> Option<(&str, f64)> {
        match self {
            Answer::Choice {
                choice, confidence, ..
            } => Some((choice.as_str(), *confidence)),
            _ => None,
        }
    }

    /// The score (which may fall between levels) and its confidence.
    pub fn as_score(&self) -> Option<(f64, f64)> {
        match self {
            Answer::Score {
                score, confidence, ..
            } => Some((*score, *confidence)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemOneResponse {
    pub model: String,
    pub answers: BTreeMap<String, Answer>,
    pub usage: Usage,
}
```

- [ ] **Step 8: Run all tests**

Run: `cargo test`
Expected: PASS, 14 tests across both crates.

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
feat(jev-client): typed request and answer shapes for /v1/systemone

Answer is untagged with Noul first, since it is the most constrained
shape and a Choice or Score body carries no `noul` key. Accessors return
None on a shape mismatch rather than panicking, so a judge that asks the
wrong question degrades to Keep instead of crashing the run.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: jev-client transport, auth, and retry

**Files:**
- Create: `crates/jev-client/src/client.rs`
- Modify: `crates/jev-client/src/lib.rs`
- Modify: `crates/jev-client/Cargo.toml`
- Test: inline `#[cfg(test)] mod tests` in `client.rs`

**Interfaces:**
- Consumes: `SystemOneRequest`, `SystemOneResponse` from Task 3.
- Produces: `Client::from_env() -> Option<Client>` (None when `TYPESAFE_API_KEY` is unset or empty), `Client::new(base_url, api_key) -> Client`, `Client::with_retry(RetryPolicy) -> Client`, `async Client::ask(&self, &SystemOneRequest) -> Result<SystemOneResponse, ClientError>`, `ClientError { Auth, Validation(String), Transport(String), Overloaded }`, `RetryPolicy { max_attempts: u32, base_delay: Duration }`.

- [ ] **Step 1: Add dependencies**

`crates/jev-client/Cargo.toml`:

```toml
[dependencies]
serde.workspace = true
serde_json.workspace = true
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
tokio = { version = "1", features = ["time"] }
thiserror = "2"

[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
wiremock = "0.6"
```

- [ ] **Step 2: Write the failing tests**

Append to `crates/jev-client/src/client.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::Question;
    use serde_json::json;
    use std::time::Duration;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn req() -> SystemOneRequest {
        SystemOneRequest {
            state: json!({ "finding_0": { "name": "helper" } }),
            model: "jev-latest".into(),
            questions: [("framework_invoked".to_string(), Question::noul("q", None))]
                .into_iter()
                .collect(),
        }
    }

    fn ok_body() -> serde_json::Value {
        json!({
            "model": "jev-1.13.0",
            "answers": { "framework_invoked": { "noul": 0.9 } },
            "usage": { "input_tokens": 100, "output_tokens": 0 }
        })
    }

    fn fast(server: &MockServer) -> Client {
        Client::new(server.uri(), "test-key".into()).with_retry(RetryPolicy {
            max_attempts: 4,
            base_delay: Duration::from_millis(1),
        })
    }

    #[tokio::test]
    async fn sends_bearer_auth_to_the_systemone_path() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/systemone"))
            .and(header("authorization", "Bearer test-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_body()))
            .mount(&server)
            .await;

        let got = fast(&server).ask(&req()).await.unwrap();
        assert_eq!(got.answers["framework_invoked"].as_noul(), Some(0.9));
    }

    #[tokio::test]
    async fn retries_a_429_then_succeeds() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(429))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_body()))
            .mount(&server)
            .await;

        let got = fast(&server).ask(&req()).await.unwrap();
        assert_eq!(got.model, "jev-1.13.0");
    }

    #[tokio::test]
    async fn gives_up_after_max_attempts_on_persistent_overload() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(529))
            .mount(&server)
            .await;

        let err = fast(&server).ask(&req()).await.unwrap_err();
        assert!(matches!(err, ClientError::Overloaded), "got {err:?}");
    }

    #[tokio::test]
    async fn a_401_is_a_hard_error_and_is_not_retried() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(401))
            .expect(1)
            .mount(&server)
            .await;

        let err = fast(&server).ask(&req()).await.unwrap_err();
        assert!(matches!(err, ClientError::Auth), "got {err:?}");
    }

    #[tokio::test]
    async fn a_422_carries_the_body_for_diagnosis() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(422).set_body_string("score needs 2-10 levels"))
            .mount(&server)
            .await;

        let err = fast(&server).ask(&req()).await.unwrap_err();
        match err {
            ClientError::Validation(body) => assert!(body.contains("2-10 levels")),
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn from_env_is_none_without_a_key() {
        std::env::remove_var("TYPESAFE_API_KEY");
        assert!(Client::from_env().is_none());
    }
}
```

- [ ] **Step 3: Run to verify it fails**

Run: `cargo test -p jev-client client`
Expected: FAIL, `cannot find type Client in this scope`.

- [ ] **Step 4: Write the client**

Prepend to `crates/jev-client/src/client.rs`:

```rust
use crate::{SystemOneRequest, SystemOneResponse};
use std::time::Duration;

pub const DEFAULT_BASE_URL: &str = "https://api.typesafe.ai";
pub const ENV_API_KEY: &str = "TYPESAFE_API_KEY";

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("invalid or missing API key")]
    Auth,
    #[error("request rejected: {0}")]
    Validation(String),
    #[error("service overloaded or rate limited after every retry")]
    Overloaded,
    #[error("transport failure: {0}")]
    Transport(String),
}

#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub base_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        RetryPolicy {
            max_attempts: 4,
            base_delay: Duration::from_millis(500),
        }
    }
}

pub struct Client {
    http: reqwest::Client,
    base: String,
    key: String,
    retry: RetryPolicy,
}

impl Client {
    pub fn new(base: String, key: String) -> Client {
        Client {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .expect("reqwest client builds with default TLS"),
            base,
            key,
            retry: RetryPolicy::default(),
        }
    }

    /// None when TYPESAFE_API_KEY is unset or empty. The caller treats that
    /// as "run without judgment", never as an error.
    pub fn from_env() -> Option<Client> {
        let key = std::env::var(ENV_API_KEY).ok()?;
        if key.trim().is_empty() {
            return None;
        }
        Some(Client::new(DEFAULT_BASE_URL.to_string(), key))
    }

    pub fn with_retry(mut self, retry: RetryPolicy) -> Client {
        self.retry = retry;
        self
    }

    /// One evaluation. 429 and 529 back off exponentially; 401 and 422 fail
    /// immediately, since retrying either one cannot help.
    pub async fn ask(&self, req: &SystemOneRequest) -> Result<SystemOneResponse, ClientError> {
        let url = format!("{}/v1/systemone", self.base.trim_end_matches('/'));
        let mut attempt = 0;
        loop {
            attempt += 1;
            let sent = self
                .http
                .post(&url)
                .bearer_auth(&self.key)
                .json(req)
                .send()
                .await;

            let resp = match sent {
                Ok(r) => r,
                Err(e) => {
                    if attempt >= self.retry.max_attempts {
                        return Err(ClientError::Transport(e.to_string()));
                    }
                    self.backoff(attempt).await;
                    continue;
                }
            };

            let status = resp.status().as_u16();
            match status {
                200 => {
                    return resp
                        .json::<SystemOneResponse>()
                        .await
                        .map_err(|e| ClientError::Transport(e.to_string()))
                }
                401 | 403 => return Err(ClientError::Auth),
                422 => {
                    let body = resp.text().await.unwrap_or_default();
                    return Err(ClientError::Validation(body));
                }
                429 | 529 | 500..=599 => {
                    if attempt >= self.retry.max_attempts {
                        return Err(ClientError::Overloaded);
                    }
                    self.backoff(attempt).await;
                }
                other => {
                    let body = resp.text().await.unwrap_or_default();
                    return Err(ClientError::Transport(format!("HTTP {other}: {body}")));
                }
            }
        }
    }

    async fn backoff(&self, attempt: u32) {
        let factor = 1u32 << (attempt.saturating_sub(1)).min(6);
        tokio::time::sleep(self.retry.base_delay * factor).await;
    }
}
```

Add to `crates/jev-client/src/lib.rs`:

```rust
pub mod client;

pub use client::{Client, ClientError, RetryPolicy, DEFAULT_BASE_URL, ENV_API_KEY};
```

- [ ] **Step 5: Run to verify it passes**

Run: `cargo test -p jev-client`
Expected: PASS, 13 tests.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
feat(jev-client): transport with bearer auth and bounded retry

429 and 529 back off exponentially, matching the API docs. 401 and 422
fail immediately since a retry cannot change either outcome. from_env
returns None rather than erroring on a missing key: running without
judgment is a supported mode, not a failure.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 5: Verdict model and application to findings

**Files:**
- Create: `crates/stratify-jev-judge/src/verdict.rs`
- Modify: `crates/stratify-jev-judge/src/lib.rs`
- Test: inline `#[cfg(test)] mod tests` in `verdict.rs`

**Interfaces:**
- Consumes: `model::{Finding, Severity, Confidence}` from Task 1.
- Produces: `Verdict { Dismiss, Weaken, Keep, Strengthen }`, `Judgment { judge: String, verdict: Verdict, reason: String, answers: serde_json::Map<String, Value>, model: String }`, and `apply(finding: &mut Finding, judgment: &Judgment, full_strength: Severity)`.

- [ ] **Step 1: Write the failing test**

Append to `crates/stratify-jev-judge/src/verdict.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Confidence, Finding, Severity, Span};

    fn finding(sev: Severity, conf: Confidence) -> Finding {
        Finding {
            rule: "dead_code".into(),
            severity: sev,
            message: "possibly unused function `helper`".into(),
            span: Span {
                file: "src/a.rs".into(),
                start_byte: 0,
                end_byte: 1,
                start_line: 1,
            },
            confidence: conf,
            extra: serde_json::Map::new(),
        }
    }

    fn judgment(v: Verdict) -> Judgment {
        Judgment {
            judge: "dead_code".into(),
            verdict: v,
            reason: "reached by a framework".into(),
            answers: serde_json::Map::new(),
            model: "jev-1.13.0".into(),
        }
    }

    #[test]
    fn dismiss_drops_to_unknown_and_info() {
        let mut f = finding(Severity::Warning, Confidence::Certain);
        apply(&mut f, &judgment(Verdict::Dismiss), Severity::Warning);
        assert_eq!(f.confidence, Confidence::Unknown);
        assert_eq!(f.severity, Severity::Info);
    }

    #[test]
    fn weaken_steps_severity_down_once() {
        let mut f = finding(Severity::Error, Confidence::Certain);
        apply(&mut f, &judgment(Verdict::Weaken), Severity::Warning);
        assert_eq!(f.confidence, Confidence::Likely);
        assert_eq!(f.severity, Severity::Warning);
    }

    #[test]
    fn weaken_leaves_info_at_info_because_info_is_the_floor() {
        let mut f = finding(Severity::Info, Confidence::Certain);
        apply(&mut f, &judgment(Verdict::Weaken), Severity::Warning);
        assert_eq!(f.severity, Severity::Info);
        assert_eq!(f.confidence, Confidence::Likely);
    }

    #[test]
    fn keep_changes_nothing_but_still_records_the_judgment() {
        let mut f = finding(Severity::Info, Confidence::Likely);
        apply(&mut f, &judgment(Verdict::Keep), Severity::Warning);
        assert_eq!(f.severity, Severity::Info);
        assert_eq!(f.confidence, Confidence::Likely);
        assert_eq!(f.extra["judgment"]["verdict"], "keep");
    }

    #[test]
    fn strengthen_restores_the_rules_full_strength() {
        let mut f = finding(Severity::Info, Confidence::Likely);
        apply(&mut f, &judgment(Verdict::Strengthen), Severity::Warning);
        assert_eq!(f.confidence, Confidence::Certain);
        assert_eq!(f.severity, Severity::Warning);
    }

    #[test]
    fn the_original_severity_and_confidence_are_always_recorded() {
        let mut f = finding(Severity::Warning, Confidence::Certain);
        apply(&mut f, &judgment(Verdict::Dismiss), Severity::Warning);
        assert_eq!(f.extra["judgment"]["original"]["severity"], "warning");
        assert_eq!(f.extra["judgment"]["original"]["confidence"], "certain");
        assert_eq!(f.extra["judgment"]["reason"], "reached by a framework");
        assert_eq!(f.extra["judgment"]["model"], "jev-1.13.0");
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p stratify-jev-judge verdict`
Expected: FAIL, `cannot find type Verdict in this scope`.

- [ ] **Step 3: Write the verdict module**

Prepend to `crates/stratify-jev-judge/src/verdict.rs`:

```rust
use crate::model::{Confidence, Finding, Severity};
use serde::{Deserialize, Serialize};

/// What the model concluded about one finding. Deliberately small: four
/// states are auditable by a reviewer and scoreable against a label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Dismiss,
    Weaken,
    Keep,
    Strengthen,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Judgment {
    pub judge: String,
    pub verdict: Verdict,
    pub reason: String,
    /// Raw probabilities, keyed by canonical question name.
    pub answers: serde_json::Map<String, serde_json::Value>,
    pub model: String,
}

/// Move a finding along the confidence ladder and record why. Nothing is
/// ever removed: a dismissed finding stays in the report and falls below
/// the display threshold instead.
pub fn apply(finding: &mut Finding, judgment: &Judgment, full_strength: Severity) {
    let original = serde_json::json!({
        "severity": finding.severity,
        "confidence": finding.confidence,
    });

    match judgment.verdict {
        Verdict::Dismiss => {
            finding.confidence = Confidence::Unknown;
            finding.severity = Severity::Info;
        }
        Verdict::Weaken => {
            finding.confidence = Confidence::Likely;
            finding.severity = finding.severity.step_down();
        }
        Verdict::Keep => {}
        Verdict::Strengthen => {
            finding.confidence = Confidence::Certain;
            finding.severity = full_strength;
        }
    }

    finding.extra.insert(
        "judgment".to_string(),
        serde_json::json!({
            "judge": judgment.judge,
            "verdict": judgment.verdict,
            "original": original,
            "answers": judgment.answers,
            "reason": judgment.reason,
            "model": judgment.model,
        }),
    );
}
```

Add to `crates/stratify-jev-judge/src/lib.rs`:

```rust
pub mod verdict;
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p stratify-jev-judge`
Expected: PASS, 13 tests.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
feat: four-state verdict model applied to findings

Info is the severity floor, so Weaken on an Info finding moves only
confidence. That is what the display threshold reads, so a weakened Info
and a dismissed Info remain distinguishable.

Every application records the original severity and confidence, so no
change the model makes is untraceable.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 6: The answer cache

**Files:**
- Create: `crates/stratify-jev-judge/src/cache.rs`
- Modify: `crates/stratify-jev-judge/src/lib.rs`
- Modify: `crates/stratify-jev-judge/Cargo.toml`
- Test: inline `#[cfg(test)] mod tests` in `cache.rs`

**Interfaces:**
- Consumes: `jev_client::{Answer, Question, Usage}`.
- Produces: `cache_key(judge: &str, version: u32, model: &str, state: &Value, questions: &BTreeMap<String, Question>) -> String` (64-char lowercase hex), `Cache::new(dir: PathBuf, enabled: bool) -> Cache`, `Cache::get(&self, key: &str) -> Option<Entry>` (`Entry` carries `model` and `answers`), `Cache::put(&self, key: &str, judge: &str, version: u32, model: &str, answers: &BTreeMap<String, Answer>, usage: Usage) -> io::Result<()>`.

**Note for the implementer:** the key is per finding, not per request. A request batches ten findings, but each finding gets its own key and its own cache entry holding only its own answers, stored under canonical question names with the per-batch prefix stripped. Editing one function must not invalidate the other nine.

- [ ] **Step 1: Add dependencies**

`crates/stratify-jev-judge/Cargo.toml` gains:

```toml
jev-client = { path = "../jev-client" }
sha2 = "0.10"

[dev-dependencies]
tempfile = "3"
```

Workspace members become:

```toml
members = ["crates/jev-client", "crates/stratify-jev-judge"]
```

- [ ] **Step 2: Write the failing test**

Append to `crates/stratify-jev-judge/src/cache.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use jev_client::{Answer, Question, Usage};
    use serde_json::json;

    fn questions() -> BTreeMap<String, Question> {
        [("framework_invoked".to_string(), Question::noul("q", None))]
            .into_iter()
            .collect()
    }

    fn answers() -> BTreeMap<String, Answer> {
        [("framework_invoked".to_string(), Answer::Noul { noul: 0.91 })]
            .into_iter()
            .collect()
    }

    #[test]
    fn key_is_stable_for_identical_inputs() {
        let s = json!({ "name": "helper" });
        let a = cache_key("dead_code", 1, "jev-latest", &s, &questions());
        let b = cache_key("dead_code", 1, "jev-latest", &s, &questions());
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
    }

    #[test]
    fn key_changes_when_any_input_changes() {
        let s = json!({ "name": "helper" });
        let base = cache_key("dead_code", 1, "jev-latest", &s, &questions());
        assert_ne!(base, cache_key("dead_code", 2, "jev-latest", &s, &questions()));
        assert_ne!(base, cache_key("dead_code", 1, "jev-1.13.0", &s, &questions()));
        assert_ne!(
            base,
            cache_key("dead_code", 1, "jev-latest", &json!({ "name": "other" }), &questions())
        );
        let mut q2 = questions();
        q2.insert("test_only".into(), Question::noul("q2", None));
        assert_ne!(base, cache_key("dead_code", 1, "jev-latest", &s, &q2));
    }

    #[test]
    fn round_trips_answers_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let c = Cache::new(dir.path().to_path_buf(), true);
        let key = "a".repeat(64);
        assert!(c.get(&key).is_none());

        c.put(&key, "dead_code", 1, "jev-1.13.0", &answers(), Usage { input_tokens: 10, output_tokens: 0 })
            .unwrap();

        let got = c.get(&key).unwrap();
        assert_eq!(got.model, "jev-1.13.0");
        assert_eq!(got.answers["framework_invoked"].as_noul(), Some(0.91));
    }

    #[test]
    fn a_disabled_cache_never_reads_or_writes() {
        let dir = tempfile::tempdir().unwrap();
        let c = Cache::new(dir.path().to_path_buf(), false);
        let key = "b".repeat(64);
        c.put(&key, "dead_code", 1, "jev-1.13.0", &answers(), Usage { input_tokens: 10, output_tokens: 0 })
            .unwrap();
        assert!(c.get(&key).is_none());
        assert!(!dir.path().join("bb").exists());
    }

    #[test]
    fn a_corrupt_entry_reads_as_a_miss_rather_than_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let c = Cache::new(dir.path().to_path_buf(), true);
        let key = "c".repeat(64);
        std::fs::create_dir_all(dir.path().join("cc")).unwrap();
        std::fs::write(dir.path().join("cc").join(format!("{key}.json")), "{ not json").unwrap();
        assert!(c.get(&key).is_none());
    }
}
```

- [ ] **Step 3: Run to verify it fails**

Run: `cargo test -p stratify-jev-judge cache`
Expected: FAIL, `cannot find function cache_key in this scope`.

- [ ] **Step 4: Write the cache**

Prepend to `crates/stratify-jev-judge/src/cache.rs`:

```rust
use jev_client::{Answer, Question, Usage};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;

/// One cached finding's answers. Raw answers only, never verdicts, so a
/// threshold change re-scores from disk with no network call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub key: String,
    pub judge: String,
    pub judge_version: u32,
    pub model: String,
    pub answers: BTreeMap<String, Answer>,
    pub usage: Usage,
}

/// SHA-256 over every input that could change an answer. Per finding, not
/// per request: editing one function must not invalidate its batch mates.
pub fn cache_key(
    judge: &str,
    version: u32,
    model: &str,
    state: &serde_json::Value,
    questions: &BTreeMap<String, Question>,
) -> String {
    let mut h = Sha256::new();
    h.update(judge.as_bytes());
    h.update([0]);
    h.update(version.to_le_bytes());
    h.update([0]);
    h.update(model.as_bytes());
    h.update([0]);
    h.update(serde_json::to_vec(state).unwrap_or_default());
    h.update([0]);
    h.update(serde_json::to_vec(questions).unwrap_or_default());
    format!("{:x}", h.finalize())
}

pub struct Cache {
    dir: PathBuf,
    enabled: bool,
}

impl Cache {
    pub fn new(dir: PathBuf, enabled: bool) -> Cache {
        Cache { dir, enabled }
    }

    fn path_for(&self, key: &str) -> PathBuf {
        let shard = key.get(..2).unwrap_or("00");
        self.dir.join(shard).join(format!("{key}.json"))
    }

    /// A miss for any reason, including a corrupt or unreadable entry.
    /// A bad cache file costs one request, never a crash. Returns the whole
    /// entry so a cache-hit judgment records the model that actually
    /// produced the answers, not the config default.
    pub fn get(&self, key: &str) -> Option<Entry> {
        if !self.enabled {
            return None;
        }
        let text = std::fs::read_to_string(self.path_for(key)).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn put(
        &self,
        key: &str,
        judge: &str,
        version: u32,
        model: &str,
        answers: &BTreeMap<String, Answer>,
        usage: Usage,
    ) -> io::Result<()> {
        if !self.enabled {
            return Ok(());
        }
        let entry = Entry {
            key: key.to_string(),
            judge: judge.to_string(),
            judge_version: version,
            model: model.to_string(),
            answers: answers.clone(),
            usage,
        };
        let path = self.path_for(key);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let body = serde_json::to_string_pretty(&entry)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, body)
    }
}
```

Add to `crates/stratify-jev-judge/src/lib.rs`:

```rust
pub mod cache;
```

- [ ] **Step 5: Run to verify it passes**

Run: `cargo test -p stratify-jev-judge`
Expected: PASS, 18 tests.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
feat: per-finding answer cache keyed on state and question set

Keys cover judge id, judge version, model, state, and questions, so any
change that could change an answer produces a miss and nothing else does.
Entries hold raw answers rather than verdicts, so retuning a threshold
re-scores from disk.

A corrupt entry reads as a miss. A bad cache file costs one request, not
a crashed run.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 7: Config and thresholds

**Files:**
- Create: `crates/stratify-jev-judge/src/config.rs`
- Modify: `crates/stratify-jev-judge/src/lib.rs`
- Test: inline `#[cfg(test)] mod tests` in `config.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `JevConfig { model: String, concurrency: usize, batch_findings: usize, batch_files: usize, thresholds: Thresholds }`, `Thresholds { dead_code: DeadCodeThresholds }`, `DeadCodeThresholds { dismiss_at: f64, api_at: f64, low_at: f64, explanation_at: f64 }`, and `JevConfig::load(root: &Path) -> JevConfig`.

Defaults (from the spec): `model = "jev-latest"`, `concurrency = 8`, `batch_findings = 10`, `batch_files = 25`, `dismiss_at = 0.75`, `api_at = 0.75`, `low_at = 0.25`, `explanation_at = 0.70`.

- [ ] **Step 1: Write the failing test**

Append to `crates/stratify-jev-judge/src/config.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_spec_when_no_config_exists() {
        let dir = tempfile::tempdir().unwrap();
        let c = JevConfig::load(dir.path());
        assert_eq!(c.model, "jev-latest");
        assert_eq!(c.concurrency, 8);
        assert_eq!(c.batch_findings, 10);
        assert_eq!(c.thresholds.dead_code.dismiss_at, 0.75);
        assert_eq!(c.thresholds.dead_code.explanation_at, 0.70);
    }

    #[test]
    fn reads_the_jev_table_out_of_stratify_toml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("stratify.toml"),
            r#"
[ignore]
paths = ["vendor/**"]

[jev]
concurrency = 3

[jev.thresholds.dead_code]
dismiss_at = 0.9
"#,
        )
        .unwrap();
        let c = JevConfig::load(dir.path());
        assert_eq!(c.concurrency, 3);
        assert_eq!(c.thresholds.dead_code.dismiss_at, 0.9);
        // Unset keys keep their defaults.
        assert_eq!(c.thresholds.dead_code.api_at, 0.75);
        assert_eq!(c.model, "jev-latest");
    }

    #[test]
    fn stratify_jev_toml_overrides_stratify_toml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("stratify.toml"), "[jev]\nconcurrency = 3\n").unwrap();
        std::fs::write(dir.path().join("stratify-jev.toml"), "[jev]\nconcurrency = 16\n").unwrap();
        assert_eq!(JevConfig::load(dir.path()).concurrency, 16);
    }

    #[test]
    fn an_unparseable_config_falls_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("stratify.toml"), "[jev\nbroken").unwrap();
        assert_eq!(JevConfig::load(dir.path()).concurrency, 8);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p stratify-jev-judge config`
Expected: FAIL, `cannot find type JevConfig in this scope`.

- [ ] **Step 3: Write the config module**

Prepend to `crates/stratify-jev-judge/src/config.rs`:

```rust
use serde::Deserialize;
use std::path::Path;

fn d_model() -> String { "jev-latest".to_string() }
fn d_concurrency() -> usize { 8 }
fn d_batch_findings() -> usize { 10 }
fn d_batch_files() -> usize { 25 }
fn d_dismiss_at() -> f64 { 0.75 }
fn d_api_at() -> f64 { 0.75 }
fn d_low_at() -> f64 { 0.25 }
fn d_explanation_at() -> f64 { 0.70 }

#[derive(Debug, Clone, Deserialize)]
pub struct DeadCodeThresholds {
    /// framework_invoked or test_only above this gives Dismiss.
    #[serde(default = "d_dismiss_at")]
    pub dismiss_at: f64,
    /// external_api above this gives Weaken.
    #[serde(default = "d_api_at")]
    pub api_at: f64,
    /// All three Nouls below this is a precondition for Strengthen.
    #[serde(default = "d_low_at")]
    pub low_at: f64,
    /// Choice confidence floor for genuinely_unused to give Strengthen.
    #[serde(default = "d_explanation_at")]
    pub explanation_at: f64,
}

impl Default for DeadCodeThresholds {
    fn default() -> Self {
        DeadCodeThresholds {
            dismiss_at: d_dismiss_at(),
            api_at: d_api_at(),
            low_at: d_low_at(),
            explanation_at: d_explanation_at(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Thresholds {
    #[serde(default)]
    pub dead_code: DeadCodeThresholds,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JevConfig {
    #[serde(default = "d_model")]
    pub model: String,
    #[serde(default = "d_concurrency")]
    pub concurrency: usize,
    #[serde(default = "d_batch_findings")]
    pub batch_findings: usize,
    #[serde(default = "d_batch_files")]
    pub batch_files: usize,
    #[serde(default)]
    pub thresholds: Thresholds,
}

impl Default for JevConfig {
    fn default() -> Self {
        JevConfig {
            model: d_model(),
            concurrency: d_concurrency(),
            batch_findings: d_batch_findings(),
            batch_files: d_batch_files(),
            thresholds: Thresholds::default(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
struct Wrapper {
    #[serde(default)]
    jev: Option<JevConfig>,
}

impl JevConfig {
    /// `[jev]` from `stratify.toml`, overridden wholesale by
    /// `stratify-jev.toml` when that file exists. Unparseable or absent
    /// falls back to defaults, matching how the engine reads its own tables.
    pub fn load(root: &Path) -> JevConfig {
        for name in ["stratify-jev.toml", "stratify.toml"] {
            let Ok(text) = std::fs::read_to_string(root.join(name)) else {
                continue;
            };
            let w: Wrapper = match toml::from_str(&text) {
                Ok(w) => w,
                Err(_) => continue,
            };
            if let Some(cfg) = w.jev {
                return cfg;
            }
        }
        JevConfig::default()
    }
}
```

Add to `crates/stratify-jev-judge/src/lib.rs`:

```rust
pub mod config;
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p stratify-jev-judge`
Expected: PASS, 22 tests.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
feat: [jev] config read from stratify.toml, overridable per tool

The engine deserializes only its own tables through #[serde(default)]
wrappers and serde ignores unknown fields, so a [jev] table added to an
existing stratify.toml changes nothing for the engine. One config file
by default, two when someone wants them separate.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 8: The dead_code judge

**Files:**
- Create: `crates/stratify-jev-judge/src/judges/mod.rs`
- Create: `crates/stratify-jev-judge/src/judges/dead_code.rs`
- Modify: `crates/stratify-jev-judge/src/lib.rs`
- Test: inline `#[cfg(test)] mod tests` in `dead_code.rs`

**Interfaces:**
- Consumes: `model::Finding`, `context::RepoContext`, `config::DeadCodeThresholds`, `verdict::{Verdict, Judgment}`, `jev_client::{Answer, Question}`.
- Produces: `trait Judge` with `rule()`, `version()`, `full_strength()`, `state_for(&Finding, &RepoContext) -> Value`, `questions() -> BTreeMap<String, Question>`, `judge(&BTreeMap<String, Answer>, &Thresholds) -> Judgment`; and `DeadCodeJudge` implementing it. Also `fn function_name(message: &str) -> Option<&str>`.

**Note for the implementer:** questions use canonical names (`framework_invoked`, `test_only`, `external_api`, `explanation`). The driver in Task 9 prefixes them per batch slot and strips the prefix before caching.

- [ ] **Step 1: Write the failing test**

Append to `crates/stratify-jev-judge/src/judges/dead_code.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DeadCodeThresholds;
    use crate::context::RepoContext;
    use crate::model::{Confidence, Finding, Severity, Span};
    use jev_client::Answer;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn ctx() -> RepoContext {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/sample-repo");
        RepoContext::new(root).unwrap()
    }

    fn finding() -> Finding {
        let text = std::fs::read_to_string(ctx().root().join("src/lib.rs")).unwrap();
        let start = text.find("fn helper").unwrap();
        Finding {
            rule: "dead_code".into(),
            severity: Severity::Info,
            message: "possibly unused function `helper`".into(),
            span: Span {
                file: "src/lib.rs".into(),
                start_byte: start,
                end_byte: start + 9,
                start_line: 5,
            },
            confidence: Confidence::Likely,
            extra: serde_json::Map::new(),
        }
    }

    fn noul(v: f64) -> Answer {
        Answer::Noul { noul: v }
    }

    fn choice(pick: &str, conf: f64) -> Answer {
        Answer::Choice {
            choice: pick.into(),
            probabilities: BTreeMap::new(),
            confidence: conf,
        }
    }

    fn answers(fw: f64, test: f64, api: f64, pick: &str, conf: f64) -> BTreeMap<String, Answer> {
        [
            ("framework_invoked".to_string(), noul(fw)),
            ("test_only".to_string(), noul(test)),
            ("external_api".to_string(), noul(api)),
            ("explanation".to_string(), choice(pick, conf)),
        ]
        .into_iter()
        .collect()
    }

    #[test]
    fn extracts_the_function_name_from_either_message_form() {
        assert_eq!(function_name("unused function `neverCalled`"), Some("neverCalled"));
        assert_eq!(function_name("possibly unused function `helper`"), Some("helper"));
        assert_eq!(function_name("no backticks here"), None);
    }

    #[test]
    fn state_carries_the_function_source_and_its_identity() {
        let s = DeadCodeJudge.state_for(&finding(), &ctx());
        assert_eq!(s["function"]["name"], "helper");
        assert_eq!(s["function"]["language"], "rust");
        assert_eq!(s["function"]["file"], "src/lib.rs");
        assert!(
            s["function"]["source"].as_str().unwrap().contains("fn helper"),
            "got {}", s["function"]["source"]
        );
    }

    #[test]
    fn attributes_collect_markers_and_never_comments() {
        let ctx = ctx();
        // src/lib.rs has `#[allow(dead_code)]` above `orphan`, and the
        // fixture's comment lines must not be mistaken for attributes.
        let text = ctx.file_text("src/lib.rs").unwrap();
        let line_of_orphan = text
            .lines()
            .position(|l| l.contains("fn orphan"))
            .unwrap()
            + 1;
        let attrs = attributes_above(&ctx, "src/lib.rs", line_of_orphan);
        assert_eq!(attrs, vec!["#[allow(dead_code)]".to_string()]);
        assert!(attrs.iter().all(|a| !a.starts_with("//")), "got {attrs:?}");
    }

    #[test]
    fn asks_four_questions_with_canonical_names() {
        let q = DeadCodeJudge.questions();
        let mut names: Vec<&str> = q.keys().map(|s| s.as_str()).collect();
        names.sort();
        assert_eq!(names, ["explanation", "external_api", "framework_invoked", "test_only"]);
    }

    #[test]
    fn a_confident_framework_hit_dismisses() {
        let j = DeadCodeJudge.judge(&answers(0.91, 0.02, 0.1, "framework_invoked", 0.9), &DeadCodeThresholds::default());
        assert_eq!(j.verdict, Verdict::Dismiss);
        assert!(j.reason.contains("framework"), "got {}", j.reason);
    }

    #[test]
    fn a_confident_test_helper_dismisses() {
        let j = DeadCodeJudge.judge(&answers(0.05, 0.88, 0.1, "test_support", 0.9), &DeadCodeThresholds::default());
        assert_eq!(j.verdict, Verdict::Dismiss);
        assert!(j.reason.contains("test"), "got {}", j.reason);
    }

    #[test]
    fn a_public_api_symbol_weakens_rather_than_dismissing() {
        let j = DeadCodeJudge.judge(&answers(0.05, 0.05, 0.9, "public_api", 0.9), &DeadCodeThresholds::default());
        assert_eq!(j.verdict, Verdict::Weaken);
    }

    #[test]
    fn all_signals_low_plus_a_confident_explanation_strengthens() {
        let j = DeadCodeJudge.judge(&answers(0.04, 0.03, 0.05, "genuinely_unused", 0.85), &DeadCodeThresholds::default());
        assert_eq!(j.verdict, Verdict::Strengthen);
    }

    #[test]
    fn low_signals_with_an_unsure_explanation_keeps() {
        let j = DeadCodeJudge.judge(&answers(0.04, 0.03, 0.05, "genuinely_unused", 0.4), &DeadCodeThresholds::default());
        assert_eq!(j.verdict, Verdict::Keep);
    }

    #[test]
    fn a_middling_answer_keeps() {
        let j = DeadCodeJudge.judge(&answers(0.5, 0.4, 0.3, "cannot_tell", 0.4), &DeadCodeThresholds::default());
        assert_eq!(j.verdict, Verdict::Keep);
    }

    #[test]
    fn missing_answers_keep_rather_than_guessing() {
        let j = DeadCodeJudge.judge(&BTreeMap::new(), &DeadCodeThresholds::default());
        assert_eq!(j.verdict, Verdict::Keep);
    }

    #[test]
    fn every_raw_probability_is_recorded_on_the_judgment() {
        let j = DeadCodeJudge.judge(&answers(0.91, 0.02, 0.1, "framework_invoked", 0.9), &DeadCodeThresholds::default());
        assert_eq!(j.answers["framework_invoked"], 0.91);
        assert_eq!(j.answers["explanation"]["choice"], "framework_invoked");
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p stratify-jev-judge dead_code`
Expected: FAIL, `cannot find type DeadCodeJudge in this scope`.

- [ ] **Step 3: Write the Judge trait**

`crates/stratify-jev-judge/src/judges/mod.rs`:

```rust
pub mod dead_code;

use crate::config::Thresholds;
use crate::context::RepoContext;
use crate::model::{Finding, Severity};
use crate::verdict::Judgment;
use jev_client::{Answer, Question};
use std::collections::BTreeMap;

/// Adjusts findings the engine already produced.
pub trait Judge: Send + Sync {
    /// Stable id matching the engine's rule name.
    fn rule(&self) -> &'static str;

    /// Bumped whenever state shape or question text changes. Part of the
    /// cache key, so a bump invalidates every entry for this judge.
    fn version(&self) -> u32;

    /// Severity restored by Verdict::Strengthen.
    fn full_strength(&self) -> Severity;

    /// Nested JSON state for one finding.
    fn state_for(&self, finding: &Finding, ctx: &RepoContext) -> serde_json::Value;

    /// Questions under canonical names. The driver prefixes them per batch slot.
    fn questions(&self) -> BTreeMap<String, Question>;

    /// Map raw answers to a verdict and a reason.
    fn judge(&self, answers: &BTreeMap<String, Answer>, cfg: &Thresholds) -> Judgment;
}

/// Every judge registered in this build.
pub fn registry() -> Vec<Box<dyn Judge>> {
    vec![Box::new(dead_code::DeadCodeJudge)]
}
```

Note: `Judge::judge` above takes `&Thresholds`; `DeadCodeJudge` reads `cfg.dead_code` from it. The unit tests call the inherent method with `&DeadCodeThresholds` directly, so implement both: an inherent `judge(&self, answers, &DeadCodeThresholds)` and the trait method delegating to it.

- [ ] **Step 4: Write the dead_code judge**

Prepend to `crates/stratify-jev-judge/src/judges/dead_code.rs`:

```rust
use crate::config::{DeadCodeThresholds, Thresholds};
use crate::context::{language_of, RepoContext};
use crate::judges::Judge;
use crate::model::{Finding, Severity};
use crate::verdict::{Judgment, Verdict};
use jev_client::{Answer, NoulCriteria, Question};
use std::collections::BTreeMap;

pub struct DeadCodeJudge;

/// The engine writes "unused function `name`" or "possibly unused
/// function `name`". Pull the backticked name out of either.
pub fn function_name(message: &str) -> Option<&str> {
    let start = message.find('`')? + 1;
    let rest = &message[start..];
    let end = rest.find('`')?;
    Some(&rest[..end])
}

/// Attributes, annotations, and decorators directly above the function.
/// These are the strongest framework signal in most languages, so only
/// real markers are collected: a comment or a blank line is walked past,
/// never captured. Collecting `#` comments would poison the signal in
/// Python and Ruby, where `#` starts a comment rather than an attribute.
fn attributes_above(ctx: &RepoContext, file: &str, start_line: usize) -> Vec<String> {
    let Some(text) = ctx.file_text(file) else {
        return Vec::new();
    };
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut i = start_line.saturating_sub(1);
    while i > 0 {
        let line = lines.get(i - 1).map(|s| s.trim()).unwrap_or("");
        let is_marker = line.starts_with("#[") || line.starts_with('@');
        let is_skippable = line.is_empty() || line.starts_with("//") || line.starts_with('#');
        if is_marker {
            out.push(line.to_string());
        } else if !is_skippable {
            break;
        }
        i -= 1;
    }
    out.reverse();
    out
}

/// Import-looking lines from the head of the file, capped so state stays small.
fn imports_of(ctx: &RepoContext, file: &str) -> Vec<String> {
    let Some(text) = ctx.file_text(file) else {
        return Vec::new();
    };
    text.lines()
        .take(200)
        .map(str::trim)
        .filter(|l| {
            l.starts_with("use ")
                || l.starts_with("import ")
                || l.starts_with("from ")
                || l.starts_with("require")
                || l.starts_with("package ")
        })
        .take(40)
        .map(str::to_string)
        .collect()
}

/// Repo-level signals that change what "unreachable" means.
fn project_markers(ctx: &RepoContext) -> Vec<String> {
    let mut out = Vec::new();
    let root = ctx.root();
    if let Ok(t) = std::fs::read_to_string(root.join("Cargo.toml")) {
        if t.contains("publish = false") {
            out.push("Cargo.toml declares publish = false".into());
        }
    }
    if let Ok(t) = std::fs::read_to_string(root.join("package.json")) {
        if t.contains("\"private\"") && t.contains("true") {
            out.push("package.json declares private: true".into());
        }
    }
    if root.join("config/routes.rb").exists() || root.join("app/controllers").is_dir() {
        out.push("Rails application layout detected".into());
    }
    if root.join("pom.xml").exists() || root.join("build.gradle").exists() {
        out.push("Maven or Gradle project detected".into());
    }
    out
}

impl DeadCodeJudge {
    pub fn judge(
        &self,
        answers: &BTreeMap<String, Answer>,
        cfg: &DeadCodeThresholds,
    ) -> Judgment {
        let fw = answers.get("framework_invoked").and_then(Answer::as_noul);
        let test = answers.get("test_only").and_then(Answer::as_noul);
        let api = answers.get("external_api").and_then(Answer::as_noul);
        let explanation = answers.get("explanation").and_then(Answer::as_choice);

        let mut raw = serde_json::Map::new();
        if let Some(v) = fw {
            raw.insert("framework_invoked".into(), serde_json::json!(v));
        }
        if let Some(v) = test {
            raw.insert("test_only".into(), serde_json::json!(v));
        }
        if let Some(v) = api {
            raw.insert("external_api".into(), serde_json::json!(v));
        }
        if let Some((pick, conf)) = explanation {
            raw.insert(
                "explanation".into(),
                serde_json::json!({ "choice": pick, "confidence": conf }),
            );
        }

        let (verdict, reason) = decide(fw, test, api, explanation, cfg);

        Judgment {
            judge: "dead_code".into(),
            verdict,
            reason,
            answers: raw,
            model: String::new(), // filled by the driver from the response
        }
    }
}

/// Policy lives here alone, separate from the answers, so retuning a
/// threshold never needs a new request.
fn decide(
    fw: Option<f64>,
    test: Option<f64>,
    api: Option<f64>,
    explanation: Option<(&str, f64)>,
    cfg: &DeadCodeThresholds,
) -> (Verdict, String) {
    // A missing answer never produces an action. Absence is not evidence.
    let (Some(fw), Some(test), Some(api)) = (fw, test, api) else {
        return (Verdict::Keep, "no usable answer, left as the engine found it".into());
    };

    if fw >= cfg.dismiss_at {
        return (Verdict::Dismiss, format!("reached by a framework ({fw:.2})"));
    }
    if test >= cfg.dismiss_at {
        return (Verdict::Dismiss, format!("test-only helper ({test:.2})"));
    }
    if api >= cfg.api_at {
        return (
            Verdict::Weaken,
            format!("public API surface for outside consumers ({api:.2})"),
        );
    }
    if fw < cfg.low_at && test < cfg.low_at && api < cfg.low_at {
        if let Some(("genuinely_unused", conf)) = explanation {
            if conf >= cfg.explanation_at {
                return (
                    Verdict::Strengthen,
                    format!("no caller and no hidden entry point ({conf:.2})"),
                );
            }
        }
    }
    (Verdict::Keep, "no clear signal either way".into())
}

impl Judge for DeadCodeJudge {
    fn rule(&self) -> &'static str {
        "dead_code"
    }

    fn version(&self) -> u32 {
        1
    }

    fn full_strength(&self) -> Severity {
        Severity::Warning
    }

    fn state_for(&self, finding: &Finding, ctx: &RepoContext) -> serde_json::Value {
        let file = finding.span.file.as_str();
        serde_json::json!({
            "function": {
                "name": function_name(&finding.message).unwrap_or_default(),
                "file": file,
                "line": finding.span.start_line,
                "language": language_of(file),
                "source": ctx.function_source(&finding.span).unwrap_or_default(),
                "attributes": attributes_above(ctx, file, finding.span.start_line),
            },
            "file_imports": imports_of(ctx, file),
            "project_markers": project_markers(ctx),
            "engine_message": finding.message,
        })
    }

    fn questions(&self) -> BTreeMap<String, Question> {
        [
            (
                "framework_invoked".to_string(),
                Question::noul(
                    "Would a framework, dependency-injection container, reflection call, \
                     route table, serializer, or plugin registry invoke the function in \
                     `function` without any explicit call to it appearing in source?",
                    Some(NoulCriteria {
                        yes: "Something outside ordinary call syntax reaches this function: \
                              an annotation or attribute registers it, a container wires it, \
                              a route or event table names it, or a serializer calls it by \
                              convention.".into(),
                        no: "Only an ordinary call written in source would reach this \
                             function.".into(),
                    }),
                ),
            ),
            (
                "test_only".to_string(),
                Question::noul(
                    "Does the function in `function` exist only to support tests, such as a \
                     fixture builder, a test helper, or a assertion utility?",
                    Some(NoulCriteria {
                        yes: "Its purpose is setting up or supporting tests, and production \
                              code has no reason to call it.".into(),
                        no: "It carries production behavior, whatever else also calls it.".into(),
                    }),
                ),
            ),
            (
                "external_api".to_string(),
                Question::noul(
                    "Is the function in `function` part of a public API surface intended for \
                     consumers outside this repository, given `project_markers`?",
                    Some(NoulCriteria {
                        yes: "It is exported for other codebases to call, and no caller inside \
                              this repository is expected.".into(),
                        no: "It is internal to this repository, whether or not the language \
                             marks it public.".into(),
                    }),
                ),
            ),
            (
                "explanation".to_string(),
                Question::choice(
                    "Nothing in this repository calls the function in `function`. What best \
                     explains why?",
                    [
                        ("framework_invoked", "A framework, container, or registry calls it without an explicit call site."),
                        ("test_support", "It exists to support tests."),
                        ("public_api", "It is exported for consumers outside this repository."),
                        ("entrypoint", "It is a program entry point, such as a main function or a CLI command handler."),
                        ("genuinely_unused", "Nothing calls it and nothing is expected to. The code is dead."),
                        ("cannot_tell", "The available evidence does not settle the question."),
                    ],
                ),
            ),
        ]
        .into_iter()
        .collect()
    }

    fn judge(&self, answers: &BTreeMap<String, Answer>, cfg: &Thresholds) -> Judgment {
        DeadCodeJudge::judge(self, answers, &cfg.dead_code)
    }
}
```

Add to `crates/stratify-jev-judge/src/lib.rs`:

```rust
pub mod judges;
```

- [ ] **Step 5: Run to verify it passes**

Run: `cargo test -p stratify-jev-judge`
Expected: PASS, 34 tests.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
feat: the dead_code judge, three Nouls and a Choice

Policy is a separate function from the answers, so retuning a threshold
never needs a new request. A missing answer produces Keep: absence of
evidence is not evidence, and the engine's own verdict stands.

Criteria spell out both sides of every Noul, since a bare question leaves
the model guessing at what counts as yes.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 9: The driver, batching and concurrency

**Files:**
- Create: `crates/stratify-jev-judge/src/driver.rs`
- Modify: `crates/stratify-jev-judge/src/lib.rs`
- Modify: `crates/stratify-jev-judge/Cargo.toml`
- Test: inline `#[cfg(test)] mod tests` in `driver.rs`

**Interfaces:**
- Consumes: everything from Tasks 1 through 8.
- Produces: `Driver::new(client: Option<Client>, cache: Cache, cfg: JevConfig) -> Driver`, `async Driver::run(&self, report: &mut Report, ctx: &RepoContext) -> RunStats`, and `RunStats { judged: usize, from_cache: usize, requested: usize, failed: usize, input_tokens: u64, by_verdict: BTreeMap<String, usize> }` (keyed by the verdict's lowercase name, so it serializes without a custom map-key impl). Also `plan_batches(items: usize, max_per_batch: usize, est_tokens: &[usize], token_ceiling: usize) -> Vec<Vec<usize>>`.

**Note for the implementer:** questions are namespaced per batch slot as `s{i}__{canonical}` and state per slot as `finding_{i}`. Answers are un-namespaced before caching so a cache entry is independent of the slot a finding happened to occupy.

- [ ] **Step 1: Add dependencies**

`crates/stratify-jev-judge/Cargo.toml` gains:

```toml
tokio = { version = "1", features = ["rt", "sync", "macros"] }
futures = "0.3"

[dev-dependencies]
tempfile = "3"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
wiremock = "0.6"
```

- [ ] **Step 2: Write the failing test**

Append to `crates/stratify-jev-judge/src/driver.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Confidence, Severity, Span};
    use jev_client::RetryPolicy;
    use serde_json::json;
    use std::path::PathBuf;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn ctx() -> RepoContext {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/sample-repo");
        RepoContext::new(root).unwrap()
    }

    fn report(n: usize) -> Report {
        Report {
            schema_version: 1,
            findings: (0..n)
                .map(|i| Finding {
                    rule: "dead_code".into(),
                    severity: Severity::Info,
                    message: format!("possibly unused function `fn{i}`"),
                    span: Span {
                        file: "src/lib.rs".into(),
                        start_byte: 0,
                        end_byte: 5,
                        start_line: 1,
                    },
                    confidence: Confidence::Likely,
                    extra: serde_json::Map::new(),
                })
                .collect(),
            extra: serde_json::Map::new(),
        }
    }

    #[test]
    fn batches_respect_the_count_cap() {
        let est = vec![100; 25];
        let b = plan_batches(25, 10, &est, 24_000);
        assert_eq!(b.len(), 3);
        assert_eq!(b[0].len(), 10);
        assert_eq!(b[2].len(), 5);
    }

    #[test]
    fn a_batch_splits_before_it_exceeds_the_token_ceiling() {
        let est = vec![10_000; 6];
        let b = plan_batches(6, 10, &est, 24_000);
        assert!(b.iter().all(|batch| batch.len() <= 2), "got {b:?}");
        assert_eq!(b.iter().map(Vec::len).sum::<usize>(), 6);
    }

    #[test]
    fn a_single_oversized_item_still_gets_its_own_batch() {
        let est = vec![90_000];
        let b = plan_batches(1, 10, &est, 24_000);
        assert_eq!(b, vec![vec![0]]);
    }

    #[tokio::test]
    async fn without_a_client_the_report_passes_through_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let d = Driver::new(None, Cache::new(dir.path().into(), true), JevConfig::default());
        let mut r = report(3);
        let before = r.clone();
        let stats = d.run(&mut r, &ctx()).await;
        assert_eq!(r, before);
        assert_eq!(stats.judged, 0);
        assert_eq!(stats.requested, 0);
    }

    #[tokio::test]
    async fn judges_every_finding_and_applies_the_verdict() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "model": "jev-1.13.0",
                "answers": {
                    "s0__framework_invoked": { "noul": 0.95 },
                    "s0__test_only": { "noul": 0.01 },
                    "s0__external_api": { "noul": 0.02 },
                    "s0__explanation": {
                        "choice": "framework_invoked",
                        "probabilities": { "framework_invoked": 0.9 },
                        "confidence": 0.9
                    }
                },
                "usage": { "input_tokens": 300, "output_tokens": 0 }
            })))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let client = Client::new(server.uri(), "k".into());
        let d = Driver::new(Some(client), Cache::new(dir.path().into(), true), JevConfig::default());

        let mut r = report(1);
        let stats = d.run(&mut r, &ctx()).await;

        assert_eq!(stats.judged, 1);
        assert_eq!(stats.requested, 1);
        assert_eq!(r.findings[0].confidence, Confidence::Unknown);
        assert_eq!(r.findings[0].extra["judgment"]["verdict"], "dismiss");
        assert_eq!(r.findings[0].extra["judgment"]["model"], "jev-1.13.0");
    }

    #[tokio::test]
    async fn a_second_run_is_served_entirely_from_cache() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "model": "jev-1.13.0",
                "answers": {
                    "s0__framework_invoked": { "noul": 0.95 },
                    "s0__test_only": { "noul": 0.01 },
                    "s0__external_api": { "noul": 0.02 },
                    "s0__explanation": {
                        "choice": "framework_invoked",
                        "probabilities": {},
                        "confidence": 0.9
                    }
                },
                "usage": { "input_tokens": 300, "output_tokens": 0 }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let cfg = JevConfig::default();
        let make = || {
            Driver::new(
                Some(Client::new(server.uri(), "k".into())),
                Cache::new(dir.path().into(), true),
                cfg.clone(),
            )
        };

        let mut first = report(1);
        make().run(&mut first, &ctx()).await;

        let mut second = report(1);
        let stats = make().run(&mut second, &ctx()).await;

        assert_eq!(stats.requested, 0);
        assert_eq!(stats.from_cache, 1);
        assert_eq!(second.findings[0].confidence, Confidence::Unknown);
    }

    #[tokio::test]
    async fn a_failed_request_leaves_its_findings_untouched() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let mut cfg = JevConfig::default();
        cfg.concurrency = 2;
        let d = Driver::new(
            Some(Client::new(server.uri(), "k".into()).with_retry(RetryPolicy {
                max_attempts: 2,
                base_delay: std::time::Duration::from_millis(1),
            })),
            Cache::new(dir.path().into(), true),
            cfg,
        );

        let mut r = report(2);
        let before = r.clone();
        let stats = d.run(&mut r, &ctx()).await;

        assert_eq!(stats.failed, 1);
        assert_eq!(stats.judged, 0);
        assert_eq!(r, before);
    }
}
```

`JevConfig` must derive `Clone` for the test above. It already does.

- [ ] **Step 3: Run to verify it fails**

Run: `cargo test -p stratify-jev-judge driver`
Expected: FAIL, `cannot find function plan_batches in this scope`.

- [ ] **Step 4: Write the driver**

Prepend to `crates/stratify-jev-judge/src/driver.rs`:

```rust
use crate::cache::{cache_key, Cache};
use crate::config::JevConfig;
use crate::context::RepoContext;
use crate::judges::{registry, Judge};
use crate::model::Report;
use crate::verdict::{apply, Judgment, Verdict};
use jev_client::{Answer, Client, Question, SystemOneRequest};
use std::collections::BTreeMap;
use std::sync::Arc;

pub const TOKEN_CEILING: usize = 24_000;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunStats {
    pub judged: usize,
    pub from_cache: usize,
    pub requested: usize,
    pub failed: usize,
    pub input_tokens: u64,
    pub by_verdict: BTreeMap<String, usize>,
}

/// Group item indices into batches that respect both the count cap and the
/// state token ceiling. An item larger than the ceiling on its own still
/// gets a batch, since dropping it silently would be worse.
pub fn plan_batches(
    items: usize,
    max_per_batch: usize,
    est_tokens: &[usize],
    token_ceiling: usize,
) -> Vec<Vec<usize>> {
    let mut out: Vec<Vec<usize>> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    let mut cur_tokens = 0usize;
    for i in 0..items {
        let t = est_tokens.get(i).copied().unwrap_or(0);
        let full = cur.len() >= max_per_batch.max(1);
        let over = !cur.is_empty() && cur_tokens + t > token_ceiling;
        if full || over {
            out.push(std::mem::take(&mut cur));
            cur_tokens = 0;
        }
        cur.push(i);
        cur_tokens += t;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Rough token estimate. Four characters per token is close enough to
/// keep a batch under the state ceiling with headroom to spare.
fn estimate_tokens(state: &serde_json::Value) -> usize {
    serde_json::to_string(state).map(|s| s.len() / 4).unwrap_or(0)
}

fn slot_prefix(i: usize) -> String {
    format!("s{i}__")
}

pub struct Driver {
    client: Option<Arc<Client>>,
    cache: Cache,
    cfg: JevConfig,
}

/// One finding's prepared work: its state, its cache key, and where it
/// sits in the report.
struct Prepared {
    index: usize,
    state: serde_json::Value,
    key: String,
    tokens: usize,
}

impl Driver {
    pub fn new(client: Option<Client>, cache: Cache, cfg: JevConfig) -> Driver {
        Driver {
            client: client.map(Arc::new),
            cache,
            cfg,
        }
    }

    /// Judge every finding a registered judge claims. Findings the model
    /// never reaches, for any reason, are left exactly as the engine
    /// reported them.
    pub async fn run(&self, report: &mut Report, ctx: &RepoContext) -> RunStats {
        let mut stats = RunStats::default();
        let Some(client) = self.client.clone() else {
            return stats;
        };

        for judge in registry() {
            let judge: Arc<dyn Judge> = Arc::from(judge);
            let questions = judge.questions();

            // Prepare every finding this judge claims.
            let mut prepared: Vec<Prepared> = Vec::new();
            for (index, f) in report.findings.iter().enumerate() {
                if f.rule != judge.rule() {
                    continue;
                }
                let state = judge.state_for(f, ctx);
                let key = cache_key(
                    judge.rule(),
                    judge.version(),
                    &self.cfg.model,
                    &state,
                    &questions,
                );
                let tokens = estimate_tokens(&state);
                prepared.push(Prepared { index, state, key, tokens });
            }
            if prepared.is_empty() {
                continue;
            }

            // Cache split. Hits are applied now and never batched.
            let mut misses: Vec<Prepared> = Vec::new();
            for p in prepared {
                match self.cache.get(&p.key) {
                    Some(entry) => {
                        stats.from_cache += 1;
                        self.apply_one(
                            report,
                            p.index,
                            &*judge,
                            &entry.answers,
                            &entry.model,
                            &mut stats,
                        );
                    }
                    None => misses.push(p),
                }
            }
            if misses.is_empty() {
                continue;
            }

            // Batch the misses and fire them concurrently.
            let est: Vec<usize> = misses.iter().map(|p| p.tokens).collect();
            let batches = plan_batches(
                misses.len(),
                self.cfg.batch_findings,
                &est,
                TOKEN_CEILING,
            );

            let sem = Arc::new(tokio::sync::Semaphore::new(self.cfg.concurrency.max(1)));
            let mut tasks = Vec::new();
            for batch in batches {
                let mut state = serde_json::Map::new();
                let mut qs: BTreeMap<String, Question> = BTreeMap::new();
                for (slot, &mi) in batch.iter().enumerate() {
                    state.insert(format!("finding_{slot}"), misses[mi].state.clone());
                    for (name, q) in &questions {
                        qs.insert(format!("{}{}", slot_prefix(slot), name), q.clone());
                    }
                }
                let req = SystemOneRequest {
                    state: serde_json::Value::Object(state),
                    model: self.cfg.model.clone(),
                    questions: qs,
                };
                let client = client.clone();
                let sem = sem.clone();
                tasks.push(async move {
                    let _permit = sem.acquire().await.expect("semaphore is never closed");
                    (batch, client.ask(&req).await)
                });
            }

            let results = futures::future::join_all(tasks).await;

            for (batch, result) in results {
                let resp = match result {
                    Ok(r) => r,
                    Err(_) => {
                        stats.failed += 1;
                        continue;
                    }
                };
                stats.requested += 1;
                stats.input_tokens += resp.usage.input_tokens;

                for (slot, &mi) in batch.iter().enumerate() {
                    let prefix = slot_prefix(slot);
                    // Strip the slot prefix so a cache entry is independent
                    // of the batch position this finding happened to get.
                    let answers: BTreeMap<String, Answer> = resp
                        .answers
                        .iter()
                        .filter_map(|(k, v)| {
                            k.strip_prefix(&prefix).map(|n| (n.to_string(), v.clone()))
                        })
                        .collect();
                    if answers.is_empty() {
                        continue;
                    }
                    let _ = self.cache.put(
                        &misses[mi].key,
                        judge.rule(),
                        judge.version(),
                        &resp.model,
                        &answers,
                        resp.usage,
                    );
                    self.apply_one(
                        report,
                        misses[mi].index,
                        &*judge,
                        &answers,
                        &resp.model,
                        &mut stats,
                    );
                }
            }
        }

        stats
    }

    fn apply_one(
        &self,
        report: &mut Report,
        index: usize,
        judge: &dyn Judge,
        answers: &BTreeMap<String, Answer>,
        model: &str,
        stats: &mut RunStats,
    ) {
        let mut judgment: Judgment = judge.judge(answers, &self.cfg.thresholds);
        if judgment.model.is_empty() {
            judgment.model = if model.is_empty() {
                self.cfg.model.clone()
            } else {
                model.to_string()
            };
        }
        let label = match judgment.verdict {
            Verdict::Dismiss => "dismiss",
            Verdict::Weaken => "weaken",
            Verdict::Keep => "keep",
            Verdict::Strengthen => "strengthen",
        };
        *stats.by_verdict.entry(label.to_string()).or_insert(0) += 1;
        stats.judged += 1;
        if let Some(f) = report.findings.get_mut(index) {
            apply(f, &judgment, judge.full_strength());
        }
    }
}
```

Add to `crates/stratify-jev-judge/src/lib.rs`:

```rust
pub mod driver;
```

Add `#[derive(Clone)]` to `Finding` and `Report` if not already present (Task 1 derives `Clone` on both).

- [ ] **Step 5: Run to verify it passes**

Run: `cargo test -p stratify-jev-judge`
Expected: PASS, 41 tests.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
feat: driver with per-finding cache split, batching, and concurrency

Findings are keyed and cache-checked individually, then only the misses
get packed into batches, so editing one function costs one request rather
than invalidating its nine batch mates.

Question names are namespaced per batch slot on the wire and stripped
before caching, so a cache entry never depends on the slot a finding
happened to occupy.

A failed batch increments `failed` and leaves its findings exactly as the
engine reported them.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 10: Output renderers

**Files:**
- Create: `crates/stratify-jev-cli/Cargo.toml`
- Create: `crates/stratify-jev-cli/src/main.rs` (stub for now)
- Create: `crates/stratify-jev-cli/src/output/mod.rs`
- Create: `crates/stratify-jev-cli/src/output/human.rs`
- Create: `crates/stratify-jev-cli/src/output/json.rs`
- Modify: `Cargo.toml` (workspace members)
- Test: inline `#[cfg(test)] mod tests` in `human.rs` and `json.rs`

**Interfaces:**
- Consumes: `stratify_jev_judge::model::{Report, Finding, Confidence, Severity}`.
- Produces: `human::render(report: &Report, min: Confidence, show_dismissed: bool) -> String`, `json::render(report: &Report, tool_version: &str) -> String`, and `visible(f: &Finding, min: Confidence, show_dismissed: bool) -> bool`.

- [ ] **Step 1: Create the crate**

```bash
mkdir -p ~/dev/stratify-jev/crates/stratify-jev-cli/src/output
```

`crates/stratify-jev-cli/Cargo.toml`:

```toml
[package]
name = "stratify-jev-cli"
edition.workspace = true
version.workspace = true
license.workspace = true

[[bin]]
name = "stratify-jev"
path = "src/main.rs"

[dependencies]
stratify-jev-judge = { path = "../stratify-jev-judge" }
jev-client = { path = "../jev-client" }
serde.workspace = true
serde_json.workspace = true
clap = { version = "4", features = ["derive"] }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

Workspace members become:

```toml
members = ["crates/jev-client", "crates/stratify-jev-judge", "crates/stratify-jev-cli"]
```

`crates/stratify-jev-cli/src/output/mod.rs`:

```rust
pub mod human;
pub mod json;
```

`crates/stratify-jev-cli/src/main.rs` (temporary stub, replaced in Task 11):

```rust
mod output;

fn main() {
    eprintln!("stratify-jev: not wired yet");
}
```

- [ ] **Step 2: Write the failing tests**

Append to `crates/stratify-jev-cli/src/output/human.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use stratify_jev_judge::model::{Confidence, Finding, Report, Severity, Span};

    fn finding(conf: Confidence, judged: bool) -> Finding {
        let mut extra = serde_json::Map::new();
        if judged {
            extra.insert(
                "judgment".into(),
                serde_json::json!({
                    "verdict": "dismiss",
                    "reason": "reached by a framework (0.91)"
                }),
            );
        }
        Finding {
            rule: "dead_code".into(),
            severity: Severity::Info,
            message: "possibly unused function `helper`".into(),
            span: Span {
                file: "src/a.rs".into(),
                start_byte: 0,
                end_byte: 1,
                start_line: 6,
            },
            confidence: conf,
            extra,
        }
    }

    fn report(findings: Vec<Finding>) -> Report {
        Report {
            schema_version: 1,
            findings,
            extra: serde_json::Map::new(),
        }
    }

    #[test]
    fn dismissed_findings_are_hidden_at_the_default_threshold() {
        let r = report(vec![
            finding(Confidence::Unknown, true),
            finding(Confidence::Likely, false),
        ]);
        let out = render(&r, Confidence::Likely, false);
        assert_eq!(out.matches("possibly unused").count(), 1, "got:\n{out}");
    }

    #[test]
    fn show_dismissed_brings_them_back() {
        let r = report(vec![finding(Confidence::Unknown, true)]);
        let out = render(&r, Confidence::Likely, true);
        assert!(out.contains("possibly unused"), "got:\n{out}");
    }

    #[test]
    fn a_judged_finding_prints_its_reason_underneath() {
        let r = report(vec![finding(Confidence::Likely, true)]);
        let out = render(&r, Confidence::Unknown, false);
        assert!(out.contains("jev: reached by a framework (0.91)"), "got:\n{out}");
    }

    #[test]
    fn the_summary_names_shown_and_dismissed_counts() {
        let r = report(vec![
            finding(Confidence::Unknown, true),
            finding(Confidence::Unknown, true),
            finding(Confidence::Likely, false),
        ]);
        let out = render(&r, Confidence::Likely, false);
        assert!(out.contains("3 findings, 1 shown, 2 dismissed"), "got:\n{out}");
    }

    #[test]
    fn an_empty_report_says_so_without_a_dismissed_hint() {
        let out = render(&report(vec![]), Confidence::Likely, false);
        assert!(out.contains("0 findings"), "got:\n{out}");
        assert!(!out.contains("--show-dismissed"), "got:\n{out}");
    }
}
```

Append to `crates/stratify-jev-cli/src/output/json.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use stratify_jev_judge::model::{Confidence, Finding, Report, Severity, Span};

    #[test]
    fn output_keeps_the_findings_shape_and_names_the_tool() {
        let r = Report {
            schema_version: 1,
            findings: vec![Finding {
                rule: "dead_code".into(),
                severity: Severity::Info,
                message: "possibly unused function `helper`".into(),
                span: Span {
                    file: "src/a.rs".into(),
                    start_byte: 0,
                    end_byte: 1,
                    start_line: 6,
                },
                confidence: Confidence::Unknown,
                extra: serde_json::Map::new(),
            }],
            extra: serde_json::Map::new(),
        };
        let v: serde_json::Value = serde_json::from_str(&render(&r, "0.1.0")).unwrap();
        assert_eq!(v["schema_version"], 1);
        assert_eq!(v["judged_by"], "stratify-jev/0.1.0");
        assert_eq!(v["findings"][0]["rule"], "dead_code");
        assert_eq!(v["findings"][0]["confidence"], "unknown");
    }

    #[test]
    fn dismissed_findings_are_never_dropped_from_json() {
        let r = Report {
            schema_version: 1,
            findings: vec![],
            extra: serde_json::Map::new(),
        };
        let v: serde_json::Value = serde_json::from_str(&render(&r, "0.1.0")).unwrap();
        assert!(v["findings"].is_array());
    }
}
```

- [ ] **Step 3: Run to verify they fail**

Run: `cargo test -p stratify-jev-cli`
Expected: FAIL, `cannot find function render in this scope`.

- [ ] **Step 4: Write the renderers**

Prepend to `crates/stratify-jev-cli/src/output/human.rs`:

```rust
use stratify_jev_judge::model::{Confidence, Finding, Report, Severity};

/// A finding is shown when its confidence reaches the threshold, or when
/// the caller asked to see everything.
pub fn visible(f: &Finding, min: Confidence, show_dismissed: bool) -> bool {
    show_dismissed || f.confidence >= min
}

fn label(s: Severity) -> &'static str {
    match s {
        Severity::Info => "info",
        Severity::Warning => "warn",
        Severity::Error => "error",
    }
}

fn reason_of(f: &Finding) -> Option<&str> {
    f.extra.get("judgment")?.get("reason")?.as_str()
}

/// Human output keeps the engine's line shape and adds a reason under
/// anything the model moved, then one summary line.
pub fn render(report: &Report, min: Confidence, show_dismissed: bool) -> String {
    let mut out = String::new();
    let mut shown = 0usize;

    for f in &report.findings {
        if !visible(f, min, show_dismissed) {
            continue;
        }
        shown += 1;
        out.push_str(&format!(
            "{:<5} {}:{}  {}\n",
            label(f.severity),
            f.span.file,
            f.span.start_line,
            f.message
        ));
        if let Some(r) = reason_of(f) {
            out.push_str(&format!("      jev: {r}\n"));
        }
    }

    let total = report.findings.len();
    let dismissed = total - shown;
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(&format!("{total} findings, {shown} shown, {dismissed} dismissed"));
    if dismissed > 0 && !show_dismissed {
        out.push_str(". Re-run with --show-dismissed to see them");
    }
    out.push_str(".\n");
    out
}
```

Prepend to `crates/stratify-jev-cli/src/output/json.rs`:

```rust
use stratify_jev_judge::model::Report;

/// Keeps the engine's findings shape so existing consumers still parse,
/// and never drops a finding: a dismissed one is present with its
/// confidence lowered and its judgment attached.
pub fn render(report: &Report, tool_version: &str) -> String {
    let mut v = serde_json::to_value(report).unwrap_or_else(|_| serde_json::json!({}));
    if let Some(obj) = v.as_object_mut() {
        obj.insert(
            "judged_by".to_string(),
            serde_json::Value::String(format!("stratify-jev/{tool_version}")),
        );
    }
    serde_json::to_string_pretty(&v).unwrap_or_default()
}
```

- [ ] **Step 5: Run to verify they pass**

Run: `cargo test -p stratify-jev-cli`
Expected: PASS, 7 tests.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
feat(cli): human and JSON renderers with a display threshold

Noise reduction happens at the threshold, never by deletion. A dismissed
finding leaves human output and stays in the JSON with its judgment, so
nothing the model hides is untraceable.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 11: CLI wiring, end to end

**Files:**
- Modify: `crates/stratify-jev-cli/src/main.rs` (replaces the Task 10 stub)
- Create: `crates/stratify-jev-cli/tests/cli.rs`
- Create: `tests/fixtures/report-dead-code.json`
- Create: `README.md`
- Modify: `crates/stratify-jev-cli/Cargo.toml`

**Interfaces:**
- Consumes: everything above.
- Produces: the `stratify-jev` binary.

- [ ] **Step 1: Add the integration test dependency**

`crates/stratify-jev-cli/Cargo.toml` gains:

```toml
[dev-dependencies]
assert_cmd = "2"
predicates = "3"
tempfile = "3"
```

- [ ] **Step 2: Create the report fixture**

`tests/fixtures/report-dead-code.json`:

```json
{
  "schema_version": 1,
  "findings": [
    {
      "rule": "dead_code",
      "severity": "info",
      "message": "possibly unused function `helper`",
      "span": { "file": "src/lib.rs", "start_byte": 44, "end_byte": 53, "start_line": 5 },
      "confidence": "likely"
    },
    {
      "rule": "complexity",
      "severity": "warning",
      "message": "function `used` has cyclomatic complexity 12",
      "span": { "file": "src/lib.rs", "start_byte": 0, "end_byte": 30, "start_line": 1 },
      "confidence": "certain"
    }
  ]
}
```

- [ ] **Step 3: Write the failing integration test**

`crates/stratify-jev-cli/tests/cli.rs`:

```rust
use assert_cmd::Command;
use std::path::PathBuf;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn report_json() -> String {
    std::fs::read_to_string(fixtures().join("report-dead-code.json")).unwrap()
}

#[test]
fn without_an_api_key_the_report_passes_through_and_exits_zero() {
    let out = Command::cargo_bin("stratify-jev")
        .unwrap()
        .env_remove("TYPESAFE_API_KEY")
        .args([
            "--root",
            fixtures().join("sample-repo").to_str().unwrap(),
            "--format",
            "json",
        ])
        .write_stdin(report_json())
        .assert()
        .success();

    let body = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["findings"].as_array().unwrap().len(), 2);
    assert!(v["findings"][0].get("judgment").is_none());
}

#[test]
fn a_missing_key_warns_on_stderr_without_failing() {
    Command::cargo_bin("stratify-jev")
        .unwrap()
        .env_remove("TYPESAFE_API_KEY")
        .args(["--root", fixtures().join("sample-repo").to_str().unwrap()])
        .write_stdin(report_json())
        .assert()
        .success()
        .stderr(predicates::str::contains("TYPESAFE_API_KEY"));
}

#[test]
fn dry_run_reports_planned_requests_and_sends_nothing() {
    Command::cargo_bin("stratify-jev")
        .unwrap()
        .env("TYPESAFE_API_KEY", "not-a-real-key")
        .args([
            "--root",
            fixtures().join("sample-repo").to_str().unwrap(),
            "--dry-run",
        ])
        .write_stdin(report_json())
        .assert()
        .success()
        .stdout(predicates::str::contains("1 request"));
}

#[test]
fn fail_on_warning_exits_nonzero_when_a_warning_survives() {
    Command::cargo_bin("stratify-jev")
        .unwrap()
        .env_remove("TYPESAFE_API_KEY")
        .args([
            "--root",
            fixtures().join("sample-repo").to_str().unwrap(),
            "--fail-on",
            "warning",
        ])
        .write_stdin(report_json())
        .assert()
        .failure();
}

#[test]
fn an_unknown_schema_version_passes_through_with_a_warning() {
    let body = r#"{"schema_version":99,"findings":[]}"#;
    Command::cargo_bin("stratify-jev")
        .unwrap()
        .env_remove("TYPESAFE_API_KEY")
        .args(["--root", fixtures().join("sample-repo").to_str().unwrap()])
        .write_stdin(body)
        .assert()
        .success()
        .stderr(predicates::str::contains("schema_version 99"));
}
```

- [ ] **Step 4: Run to verify it fails**

Run: `cargo test -p stratify-jev-cli --test cli`
Expected: FAIL, the stub binary prints "not wired yet" and produces no JSON.

- [ ] **Step 5: Write main.rs**

Replace `crates/stratify-jev-cli/src/main.rs`:

```rust
mod output;

use clap::{Parser, ValueEnum};
use jev_client::Client;
use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;
use stratify_jev_judge::cache::Cache;
use stratify_jev_judge::config::JevConfig;
use stratify_jev_judge::context::RepoContext;
use stratify_jev_judge::driver::Driver;
use stratify_jev_judge::model::{Confidence, Report, Severity};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum Format {
    Human,
    Json,
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum ConfLevel {
    Unknown,
    Likely,
    Certain,
}

impl From<ConfLevel> for Confidence {
    fn from(c: ConfLevel) -> Confidence {
        match c {
            ConfLevel::Unknown => Confidence::Unknown,
            ConfLevel::Likely => Confidence::Likely,
            ConfLevel::Certain => Confidence::Certain,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum FailOn {
    Never,
    Info,
    Warning,
    Error,
}

/// Judge a Stratify report with Jev. Reads the report on stdin.
#[derive(Parser)]
#[command(name = "stratify-jev", version)]
struct Args {
    /// Repository root, used for source reads and the file walk.
    #[arg(long, default_value = ".")]
    root: PathBuf,

    /// Read the report from a file instead of stdin.
    #[arg(long)]
    input: Option<PathBuf>,

    #[arg(long, value_enum, default_value_t = Format::Human)]
    format: Format,

    /// Hide findings below this confidence.
    #[arg(long, value_enum, default_value_t = ConfLevel::Likely)]
    min_confidence: ConfLevel,

    #[arg(long)]
    show_dismissed: bool,

    #[arg(long, value_enum, default_value_t = FailOn::Never)]
    fail_on: FailOn,

    /// Build every request, report the plan, and send nothing.
    #[arg(long)]
    dry_run: bool,

    #[arg(long)]
    no_cache: bool,

    #[arg(long, default_value = ".stratify/jev-cache")]
    cache_dir: PathBuf,

    #[arg(long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();

    let mut raw = String::new();
    let read = match &args.input {
        Some(p) => std::fs::read_to_string(p).map(|t| {
            raw = t;
        }),
        None => std::io::stdin().read_to_string(&mut raw).map(|_| ()),
    };
    if let Err(e) = read {
        eprintln!("stratify-jev: cannot read the report: {e}");
        return ExitCode::from(2);
    }

    let mut report: Report = match serde_json::from_str(&raw) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("stratify-jev: the input is not a Stratify JSON report: {e}");
            return ExitCode::from(2);
        }
    };

    if report.schema_version > Report::KNOWN_SCHEMA_VERSION {
        eprintln!(
            "stratify-jev: schema_version {} is newer than this build understands, \
             passing the report through unchanged",
            report.schema_version
        );
        print!("{}", render(&args, &report));
        return exit_code(&args, &report);
    }

    let ctx = match RepoContext::new(args.root.clone()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("stratify-jev: cannot read {}: {e}", args.root.display());
            return ExitCode::from(2);
        }
    };
    let cfg = JevConfig::load(&args.root);

    if args.dry_run {
        let planned = plan_only(&report, &cfg);
        println!("{planned} request(s) planned, nothing sent.");
        return ExitCode::SUCCESS;
    }

    match Client::from_env() {
        None => {
            eprintln!(
                "stratify-jev: TYPESAFE_API_KEY is not set, passing the report through unchanged"
            );
        }
        Some(client) => {
            let cache = Cache::new(args.root.join(&args.cache_dir), !args.no_cache);
            let driver = Driver::new(Some(client), cache, cfg);
            let stats = driver.run(&mut report, &ctx).await;
            if args.verbose {
                eprintln!(
                    "stratify-jev: {} judged, {} from cache, {} request(s), {} failed, {} input tokens",
                    stats.judged,
                    stats.from_cache,
                    stats.requested,
                    stats.failed,
                    stats.input_tokens
                );
            }
            if stats.failed > 0 {
                eprintln!(
                    "stratify-jev: {} request(s) failed, those findings are unchanged",
                    stats.failed
                );
            }
        }
    }

    print!("{}", render(&args, &report));
    exit_code(&args, &report)
}

/// Count the requests a real run would send, without building a client.
fn plan_only(report: &Report, cfg: &JevConfig) -> usize {
    let claimed = report
        .findings
        .iter()
        .filter(|f| f.rule == "dead_code")
        .count();
    if claimed == 0 {
        return 0;
    }
    claimed.div_ceil(cfg.batch_findings.max(1))
}

fn render(args: &Args, report: &Report) -> String {
    match args.format {
        Format::Human => {
            output::human::render(report, args.min_confidence.into(), args.show_dismissed)
        }
        Format::Json => output::json::render(report, VERSION),
    }
}

/// Exit code follows --fail-on over post-judgment findings, so the tool
/// works as a gate without ever failing a build because TypeSafe was down.
fn exit_code(args: &Args, report: &Report) -> ExitCode {
    let floor = match args.fail_on {
        FailOn::Never => return ExitCode::SUCCESS,
        FailOn::Info => Severity::Info,
        FailOn::Warning => Severity::Warning,
        FailOn::Error => Severity::Error,
    };
    let hit = report.findings.iter().any(|f| {
        f.severity >= floor && output::human::visible(f, args.min_confidence.into(), args.show_dismissed)
    });
    if hit {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
```

- [ ] **Step 6: Run to verify it passes**

Run: `cargo test`
Expected: PASS, all tests across three crates.

- [ ] **Step 7: Write the README**

`README.md`:

````markdown
# stratify-jev

Judgment layer for [Stratify](https://github.com/stratify-dev/stratify). Reads a
Stratify JSON report, asks [Jev](https://docs.typesafe.ai) what static analysis
could not prove, and moves each finding along the confidence ladder the engine
already uses.

```sh
export TYPESAFE_API_KEY=...
stratify check . --format json | stratify-jev --root .
```

```
info  src/lib.rs:6  possibly unused function `helper`
      jev: reached by a framework (0.91)

73 findings, 9 shown, 64 dismissed. Re-run with --show-dismissed to see them.
```

## What it does

Stratify is precise about what it can prove. When cross-file resolution is
uncertain, it reports `possibly unused` rather than a false `unused`. The
residue is noise a reviewer dismisses in one second: framework hooks, test
helpers, and symbols reached only through reflection.

`stratify-jev` asks Jev about that residue. A finding is never deleted. A
dismissed finding drops below the display threshold and keeps its full
judgment, every raw probability included, in the JSON output.

## Without an API key

The report passes through unchanged and the exit code still follows
`--fail-on`. The same holds for a network failure. Judgment is additive, never
a dependency of the scan.

## Options

Run `stratify-jev --help`.

## Cache

Answers cache under `.stratify/jev-cache/`, keyed on judge version, model,
state, and question set. Commit the directory: CI then runs without calling the
API, results stay deterministic, and a change to question wording shows up as a
reviewable diff of verdicts.
````

- [ ] **Step 8: Verify the whole pipeline against the sample repo**

```bash
cd ~/dev/stratify-jev
cargo build --release
./target/release/stratify-jev --root tests/fixtures/sample-repo --dry-run \
  < tests/fixtures/report-dead-code.json
```

Expected: `1 request(s) planned, nothing sent.`

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
feat(cli): wire stdin, judging, and output end to end

A missing key, an unreadable root, a newer schema_version, and a failed
request all pass the report through and keep the exit code on --fail-on.
Judgment is additive: nothing about a scan depends on TypeSafe being up.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Live verification (after Task 11)

With a real key, against the engine's own repository:

```bash
cd ~/dev/stratify
stratify check . --format json > /tmp/stratify-self.json

cd ~/dev/stratify-jev
export TYPESAFE_API_KEY=...

# Cost preview first.
./target/release/stratify-jev --root ~/dev/stratify --dry-run < /tmp/stratify-self.json

# Then the real run.
./target/release/stratify-jev --root ~/dev/stratify --verbose < /tmp/stratify-self.json
```

**Expected result, and the measure of whether the question design works:** all 15
`dead_code` findings carry a live call site, verified by grep in the spec. A
correct run dismisses most of them. Anything it keeps or strengthens is a
question-design bug worth reading closely before Plan 2 starts.

Check the reasons, not only the verdicts. `run_stdio`, `emit`, and `complexities`
should read as ordinary cross-crate calls the resolver missed rather than
framework hooks, so `explanation` landing on `framework_invoked` for those is a
signal the Choice options need a `resolver_limitation` case added in Plan 2.

---

## Self-Review

**Spec coverage for Plan 1 scope:**

| Spec section | Task |
|---|---|
| Crate layout | 1, 3, 10 |
| Report model, unknown-field round-trip | 1 |
| RepoContext, ignore globs, source reads | 2 |
| `jev-client` request and answer types | 3 |
| Transport, auth, retry on 429/529 | 4 |
| Verdict model, Weaken floor at Info | 5 |
| Per-finding cache, raw answers only | 6 |
| `[jev]` config, thresholds, override file | 7 |
| dead_code state, questions, policy | 8 |
| Batching, token ceiling, concurrency | 9 |
| Display threshold, human and JSON output | 10 |
| CLI, dry-run, fail-on, pass-through | 11 |

Deferred by design to Plans 2 and 3, and stated in the plan header: the other
four judgments, the `Producer` trait, hotspot re-ranking, SARIF suppressions,
the labelled corpus, and the `eval` subcommand.

**Known deviation from the spec, resolved here:** the spec says "cache lookup
per finding" while also describing per-request batching. Task 6 resolves this
explicitly: the key is per finding, questions are namespaced per batch slot on
the wire, and the prefix is stripped before caching. Roll this correction into
the spec when Plan 2 starts.

**Type consistency:** `Judge::judge` takes `&Thresholds` in the trait and
`&DeadCodeThresholds` in the inherent method, which Task 8 Step 3 calls out
explicitly so the implementer writes both. `RunStats::by_verdict` is keyed by
`String`, not `Verdict`, so it serializes without a custom map key impl.
`Confidence` and `Severity` derive `Ord`, which the display threshold and
`--fail-on` both rely on.
