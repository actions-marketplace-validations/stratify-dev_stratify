# stratify-jev: A Jev Judgment Layer Over Stratify Findings

**Date:** 2026-09-20
**Status:** Approved (design), pending implementation plan
**Target repo:** `stratify-dev/stratify-jev` (new, sibling to `stratify-dev/stratify`)

## Goal

Stratify proves what static analysis allows and reports the rest at reduced confidence. The residue is noise a reviewer dismisses in one second and code never will. `stratify-jev` reads that residue, asks TypeSafe's Jev model a small set of typed questions per finding, and moves each finding along the confidence ladder the engine already uses.

The engine keeps its promise of no servers and no accounts. The judgment layer ships as a separate binary that consumes `stratify check --format json` over a pipe.

## Evidence for the premise

Engine v0.6.0 against its own repository (9,452 lines of Rust, 12 crates) produced 73 findings:

| rule | count | reported as |
|------|-------|-------------|
| duplication | 40 | warning / certain |
| dead_code | 15 | info / likely |
| hotspot | 11 | info / certain |
| complexity | 7 | info, one warning |

All 15 dead-code findings were traced to a live call site:

- `complexities` at `run.rs:72`, `run_stdio` at `main.rs:117` and `main.rs:124`, `emit` at `main.rs:192`. Cross-crate method calls resolve only as `Likely`, so public symbols in library mode settle at Info.
- `file_import_graph` at `boundaries.rs:155`, `file_spans` at `boundaries.rs:156`, `fqn_spans` at `cycles.rs:11`.
- `resource` and `header_map` are private, called four times inside `emit.rs`, and surface only because their sole caller was already unreachable. One unresolved edge cascades a whole subtree into the report.
- `sample_finding`, `span`, `add_sym`, `add_ref`, `file_sym`, `add_import` are test-only helpers behind `#[cfg(test)]`.

Duplication shows the same shape from the other side. The repo's own `stratify.toml` raises `min_tokens` to 100 with a comment conceding the problem, and `docs/backlog/` carries three open items about clone detection firing on structurally parallel code.

## Why the fit is structural

Every symbol, reference, and finding already carries a `Confidence` enum (`Unknown < Likely < Certain`, `crates/stratify-core/src/confidence.rs`), and the user-facing vocabulary rests on it: `unused` at Warning against `possibly unused` at Info. Jev returns calibrated probabilities. The work is mapping a float onto an enum the type system already holds.

The pipeline runs one direction with no back edges: source to IR, IR to six analyses, analyses to findings, findings to surfaces. A judgment layer sits between findings and surfaces and touches zero analysis code, for the same reason a sixth language touched zero analysis code.

## Settled decisions

| Decision | Choice | Reason |
|----------|--------|--------|
| Product shape | Sibling binary, own repo, pipe-driven | Engine changes by zero lines. Independent release. A bad question set costs nothing. |
| Language | Rust | Matches stratify's single-binary distribution. `reqwest` and `tokio` already in the lock file through `opentelemetry-otlp`. `cargo-dist` config to copy. |
| Model authority | Confidence moves both directions, severity follows, nothing deleted | Uses the ladder the engine already has. Every change is auditable. |
| v0.1 scope | All five judgments | Complete story on release. Risk of five untested question designs is managed by the eval harness below. |
| File inventory | `stratify-jev` walks the repo itself | Disk access is needed for source anyway. Layer assignment then works on repos matching no preset. |
| Accuracy | Labelled corpus plus an `eval` subcommand | Question rewording produces a number, not a vibe. |
| Pass architecture | Judges keyed by rule | Cost is five cents on a 500k-LOC repo, so paying complexity to optimize tokens is a bad trade. |

## Budget

Jev 1.13 charges $0.042 per million input tokens. Output tokens are free. Context is 64k, with 32k for state plus the longest single question.

- This repo, full scan: 15 dead-code findings at roughly 400 tokens each, about $0.00025.
- A 500k-LOC monorepo: 2,000 findings, roughly 1.2M tokens, about $0.05.
- Binding constraint is requests, not tokens: 1,200 per minute. Ten findings per request puts 2,000 findings at 200 requests, finishing in 10 to 20 seconds.

## Architecture

### Crates

- **`jev-client`** — typed Rust client over `POST https://api.typesafe.ai/v1/systemone`. Noul, Choice, and Score request and response types, bearer auth, exponential backoff on 429 and 529. Publishable on its own as the first Rust SDK for TypeSafe.
- **`stratify-jev-judge`** — the `Judge` trait, five implementations, state builders, verdict policy, the cache.
- **`stratify-jev-cli`** — argument parsing, stdin reading, repo walk, output formats, the `eval` subcommand.

### The pass

1. Read a `Report` from stdin, or from `--input <FILE>`.
2. Walk `--root` (default `.`) for the file inventory, honoring `stratify.toml` `[ignore] paths` with the same globset logic the engine uses, so both tools agree on scope.
3. Group findings by rule and hand each group to its judge. Hand the file inventory to each producer.
4. Each judge packs up to ten findings into one request, building nested JSON state and three or four questions per finding. The layers producer batches files the same way, at up to twenty-five per request since per-file state is much smaller. Batch sizes are a config key, and the packer splits a batch whose estimated state exceeds 24k tokens, leaving headroom under the 32k state limit.
5. Cache lookup per finding. Hits skip the network.
6. Misses fire concurrently behind a semaphore, default 8.
7. Answers map back to a verdict per finding.
8. Verdicts apply, findings re-sort, output renders.

### The Judge trait

Four of the judgments re-judge findings the engine produced. The layers judgment instead classifies files and produces findings of its own. Two traits rather than one, so neither carries a method the other leaves unimplemented.

```rust
/// Adjusts findings the engine already produced. Implemented by
/// dead_code, duplication, complexity, and cycles.
pub trait Judge {
    /// Stable id matching the engine's rule name, e.g. "dead_code".
    fn rule(&self) -> &'static str;

    /// Bumped whenever state shape or question text changes. Part of the cache key.
    fn version(&self) -> u32;

    /// Severity this rule carries at full strength, restored by Verdict::Strengthen.
    fn full_strength(&self) -> Severity;

    /// Build one request covering a batch of findings.
    fn build(&self, batch: &[&Finding], ctx: &RepoContext) -> Request;

    /// Map one finding's raw answers to a verdict plus a reason string.
    fn judge(&self, finding: &Finding, answers: &Answers, cfg: &Thresholds) -> Judgment;
}

/// Produces findings from repo state rather than adjusting existing ones.
/// Implemented by layers alone in v0.1.
pub trait Producer {
    fn rule(&self) -> &'static str;
    fn version(&self) -> u32;

    /// Build one request covering a batch of files.
    fn build(&self, batch: &[&FileEntry], ctx: &RepoContext) -> Request;

    /// Emit findings from the answers. Every finding produced here caps at
    /// Confidence::Likely and carries `source: "jev"`.
    fn produce(&self, batch: &[&FileEntry], answers: &Answers, ctx: &RepoContext) -> Vec<Finding>;
}
```

`RepoContext` carries the root path, the file inventory, a source reader keyed on span, and the resolved `[jev]` config.

Engine-produced boundary findings still run through a `Judge` implementation, so the layers rule appears on both sides: `Producer` for repos where the engine found nothing, `Judge` for repos where it did.

### The verdict model

```rust
pub enum Verdict { Dismiss, Weaken, Keep, Strengthen }
```

- `Dismiss` sets confidence to `Unknown`, severity to `Info`.
- `Weaken` sets confidence to `Likely` and severity one step down: `Error` to `Warning`, `Warning` to `Info`, `Info` stays `Info`. Severity has no floor below `Info`, so a weakened Info finding is distinguished from a dismissed one by confidence alone, which is exactly what the display threshold reads.
- `Keep` changes nothing.
- `Strengthen` sets confidence to `Certain`, severity to the judge's `full_strength()`.

Each judge declares its own full strength because the engine's severity mapping is rule-specific: duplication reports warning/certain, complexity ranges by magnitude. A blanket confidence-to-severity rule would break both. `full_strength()` is a single value per judge, so strengthening a complexity finding the engine reported at Info raises it to Warning rather than restoring a magnitude-derived level. That is intended: the model asserting high cognitive load is a stronger signal than the cyclomatic band that produced the original Info.

Noise reduction happens at the display threshold, never by deletion. Human and SARIF output default to `--min-confidence likely`, so dismissed findings drop from view while remaining in the JSON with their probabilities, the questions asked, and the reason. `--show-dismissed` brings them back.

## The judgments

Five judgments carry questions: dead_code, duplication, complexity, cycles, and layers. Hotspot is listed below as well, and costs no inference at all: it re-ranks on a score the complexity judge already produced.

Rules shared across all of them:

- State goes as named JSON fields, never one flat string.
- Questions reference nested state with backticked paths such as `` `finding_3.function.source` ``.
- Question ids never reach the model, so every question carries its full meaning in the instructions.
- Every Choice includes a `cannot_tell` option.
- Every threshold lives in config, never in code. Raw answers cache and policy does not, so retuning a threshold re-scores instantly with no network call.
- Each judge asks three or four questions per finding in one request. Speculative questions are cheap and give the reason string real substance.

### 1. dead_code

**State per finding:** function name, fqn, visibility, language, file, line, full source, attributes or decorators on the function and its enclosing type, the file's import list, and project markers (`Cargo.toml` `publish`, `package.json` `private`, detected Rails app, detected framework names).

**Questions:**

- `framework_invoked` (Noul): would a framework, dependency-injection container, reflection call, route table, serializer, or plugin registry invoke this function without an explicit call appearing anywhere in source?
- `test_only` (Noul): does this function exist only to support tests?
- `external_api` (Noul): is this function part of a public API surface intended for consumers outside this repository?
- `explanation` (Choice): `framework_invoked`, `test_support`, `public_api`, `entrypoint`, `genuinely_unused`, `cannot_tell`.

**Policy:** `max(framework_invoked, test_only) > dismiss_at` gives `Dismiss`. `external_api > api_at` gives `Weaken`. All three below `low_at` plus `explanation == genuinely_unused` above its confidence floor gives `Strengthen`. Otherwise `Keep`.

**Full strength:** Warning.

### 2. duplication

**State per finding:** both snippets with file paths and languages, the normalized token count, and whether either path matches a generated or vendored pattern.

**Questions:**

- `extraction_value` (Score, 4 levels, each describing a concrete situation):
  - 0: the two blocks look alike only because the language forces the shape, such as a match over similar arms or a series of accessors. Extracting would abstract over things that are not related.
  - 1: genuinely similar, but short or trivial, and each copy reads fine where it sits.
  - 2: real copy-paste of meaningful logic. Extracting removes a maintenance burden and clarifies both call sites.
  - 3: the copies encode one rule that must stay consistent. Changing one alone is a bug.
- `same_rule` (Noul): do both blocks encode the same rule, so a change to one requires the same change to the other?
- `generated` (Noul): is either block generated, vendored, or a test fixture rather than hand-maintained source?

**Policy:** `generated > generated_at` gives `Dismiss` regardless of score. Otherwise score below `dismiss_below` gives `Dismiss`, below `weaken_below` gives `Weaken`, at or above `strengthen_above` or `same_rule > same_rule_at` gives `Strengthen`.

**Full strength:** Warning.

### 3. complexity

**State per finding:** function source, language, file, the engine's cyclomatic number, and the enclosing type name.

**Questions:**

- `cognitive_load` (Score, 5 levels, described as situations from a flat dispatch reading top to bottom through nested conditionals with interleaved state mutation where understanding one branch requires holding the others in mind).
- `change_risk` (Noul): would an ordinary change to this function likely introduce a bug?
- `shape` (Choice): `flat_dispatch`, `guard_clauses`, `nested_state`, `long_procedure`, `cannot_tell`.

**Policy:** `shape == flat_dispatch` with `cognitive_load < flat_at` gives `Dismiss`. `cognitive_load >= strengthen_at` or `change_risk > risk_at` gives `Strengthen`. Otherwise `Weaken` or `Keep` by score band.

**Full strength:** Warning.

The `cognitive_load` value attaches to the finding regardless of verdict. This fills the gap the engine's README names: a twenty-arm match scores high on cyclomatic paths and low on reading difficulty.

### 4. hotspot

No questions and no requests. Hotspot findings re-rank on `cognitive_load` times churn, reusing the score the complexity judge already produced for the same function. When no complexity finding exists for that function, the hotspot finding is left untouched.

This is the composite-scoring pattern paying off: raw judgments stay reusable, and a sixth behavior costs zero additional inference.

### 5. cycles

**State per finding:** the files in the cycle, each file's import list, the first 40 lines of each, and the detected package or module names.

**Questions:**

- `cycle_kind` (Choice): `architectural_inversion`, `cohesive_module`, `interface_and_impl`, `incidental`, `cannot_tell`.
- `severity` (Score, 3 levels from harmless to actively blocking change).

**Policy:** `cohesive_module` or `interface_and_impl` above the confidence floor gives `Dismiss` or `Weaken` by severity. `architectural_inversion` gives `Strengthen`.

**Full strength:** Warning.

### 6. layers

The only judgment whose unit is a file rather than a finding, and the only one that creates findings rather than adjusting them.

**State per file:** path, import list, top-level declaration names and kinds, and the configured layer names with their descriptions from `[jev.layers]`.

**Questions:**

- `layer` (Choice over the configured layer names plus `none`).

**Policy:** assign the layer only when the returned Choice confidence exceeds `assign_at`. Run the resolved `[[forbid]]` rules over the assigned layers. Emit any violation as a finding.

**Bound on created findings:** a boundary finding derived from Jev-assigned layers caps at `Likely` confidence, never `Certain`, renders as Info, and carries `"source": "jev"` in its JSON. The model never asserts a certainty the engine could not reach, and every created finding is filterable on one field. Engine-produced boundary findings are re-judged normally and keep `"source": "stratify"`.

This is what the repo walk buys: repos matching no preset get no boundary checks at all today.

## Configuration

Read `[jev]` from `stratify.toml` when present, overridden by `stratify-jev.toml` when that file exists. Each engine loader deserializes only its own table through a `#[serde(default)]` wrapper and serde ignores unknown fields, so adding `[jev]` to an existing `stratify.toml` breaks nothing in the engine.

Every threshold named in a policy above appears here. These are the shipped defaults, chosen conservatively, and they move only on eval evidence.

```toml
[jev]
model = "jev-latest"
concurrency = 8
batch_findings = 10
batch_files = 25

[jev.thresholds.dead_code]
dismiss_at = 0.75       # framework_invoked or test_only above this -> Dismiss
api_at = 0.75           # external_api above this -> Weaken
low_at = 0.25           # all three Nouls below this is a precondition for Strengthen
explanation_at = 0.70   # Choice confidence floor for genuinely_unused -> Strengthen

[jev.thresholds.duplication]
generated_at = 0.70     # generated above this -> Dismiss, score ignored
dismiss_below = 0.8
weaken_below = 1.8
strengthen_above = 2.6
same_rule_at = 0.80     # same_rule above this -> Strengthen regardless of band

[jev.thresholds.complexity]
flat_at = 1.5           # flat_dispatch shape below this cognitive_load -> Dismiss
weaken_below = 2.0
strengthen_at = 3.5
risk_at = 0.75          # change_risk above this -> Strengthen

[jev.thresholds.cycles]
kind_at = 0.70          # cycle_kind Choice confidence floor
dismiss_below = 0.8     # severity score bands
weaken_below = 1.6

[jev.thresholds.layers]
assign_at = 0.80        # Choice confidence floor to assign a file to a layer

[jev.layers]
controllers = "HTTP entry points parsing requests and rendering responses"
domain = "Business rules with no knowledge of transport or storage"
```

The API key comes from `TYPESAFE_API_KEY` only. No config field and no flag, so it never lands in a committed file or in shell history.

## CLI

```
stratify check . --format json | stratify-jev --root .
```

| Flag | Default | Purpose |
|------|---------|---------|
| `--root <PATH>` | `.` | Source reads and the file walk |
| `--input <FILE>` | stdin | Report source |
| `--format` | `human` | `human`, `json`, `sarif` |
| `--min-confidence` | `likely` | Display threshold |
| `--show-dismissed` | off | Include dismissed findings in human and SARIF output |
| `--fail-on` | `never` | `never`, `info`, `warning`, `error`, over post-judgment findings |
| `--judges <LIST>` | all | Subset, e.g. `dead_code,layers` |
| `--concurrency <N>` | 8 | In-flight requests |
| `--model <ID>` | `jev-latest` | Model override |
| `--no-cache` | off | Skip cache reads and writes |
| `--cache-dir <PATH>` | `.stratify/jev-cache` | Cache location |
| `--dry-run` | off | Build every request, print count and token estimate, send nothing |
| `--verbose` | off | Per-finding verdict with probabilities |

Subcommand: `stratify-jev eval [--corpus <DIR>]`.

## Cache

`.stratify/jev-cache/`, one JSON file per entry, intended to be committed.

Key: SHA-256 over judge id, judge version, model id, the exact state JSON, and the question set.

Entry stores raw answers and usage, never verdicts, so a policy change re-scores from disk.

Committing the cache buys three properties: CI never calls the API, runs are byte-for-byte deterministic, and a change to question wording appears as a reviewable diff of verdicts in the pull request.

## Output

**Human** keeps the engine's shape, adds a reason line under anything the model moved, and ends with a summary:

```
info  App.java:6  possibly unused function `helper`
      jev: reached by a framework (0.91) - @EventListener on the enclosing class

73 findings, 9 shown, 64 dismissed. Re-run with --show-dismissed to see them.
```

**JSON** keeps the findings shape so existing consumers still parse, adds `judged_by`, and attaches per finding:

```json
"judgment": {
  "judge": "dead_code",
  "verdict": "dismiss",
  "original": { "severity": "info", "confidence": "likely" },
  "answers": {
    "framework_invoked": 0.91,
    "test_only": 0.04,
    "external_api": 0.12,
    "explanation": { "choice": "framework_invoked", "confidence": 0.88 }
  },
  "reason": "reached by a framework",
  "model": "jev-1.13.0"
}
```

**SARIF** emits dismissed findings through the 2.1.0 `suppressions` array with `kind: "external"` and the reason as justification, the mechanism GitHub code scanning already honors.

## Failure behavior

Every failure is a pass-through. The tool never fails a build because TypeSafe was unavailable.

- No `TYPESAFE_API_KEY`: report passes through unchanged, exit 0, one line on stderr. This matches the telemetry module's precedent of emitting only when configured.
- Network failure after retries: that batch's findings pass through unchanged, stderr names the count.
- Unparseable or missing answer: treated as `Keep` and counted, surfaced under `--verbose`.
- Exit code follows `--fail-on` over post-judgment findings.

## Testing and measurement

**Unit tests, no network.** Verdict mapping, threshold policy, cache keying, and state builders are pure functions.

**Client tests against a mock server.** Auth header, backoff on 429 and 529, timeout, malformed body, and a 422 validation failure.

**Corpus.** `corpus/` holds small fixture repos per supported language plus `labels.toml` giving the expected verdict per finding. Seeded from the 15 dead-code findings in this repo, every one verified by grep.

**`stratify-jev eval`** replays cached answers against the labels and prints precision, recall, and a confusion matrix over the four verdicts, per judge. Runs in CI at zero cost, fully deterministic.

**`--record`** refreshes the cache against the live API, run by hand. CI fails when a judge's precision falls below the committed baseline.

This closes the loop that shipping five judgments at once opens: reword a question, re-record, watch the number move.

## Out of scope for v0.1

- MCP server. Planned next, and the surface where triaged findings help most.
- GitHub Action.
- Editor or LSP integration.
- Folding the judge into the engine behind `stratify check --judge`. Revisit once accuracy baselines hold.
- Any judgment that rewrites a finding's message or proposes a fix.

## Risks

- **Five untested question designs shipping together.** Mitigated by the eval harness and the committed cache, which make every wording change measurable before release.
- **Scope drift between the two tools.** The repo walk must honor `stratify.toml` `[ignore] paths` or the tools disagree about what is in scope. Covered by a test asserting both walks produce the same file set.
- **Version skew.** A report from a newer engine with an unknown `schema_version` is passed through unchanged with a stderr warning rather than misread.
- **Model jaggedness.** TypeSafe documents known Jev 1.13 edge cases. Thresholds start conservative and move only on eval evidence.
