# Roadmap: Full-Stack Static Analysis

Date: 2026-10-01
Status: Draft, pending review
Baseline: engine v0.6.0

## Goal

Move Stratify from a polyglot repo-health scanner to a static-analysis tool with semantic resolution, flow analysis, and security rules. Each step ships on its own and meets a measured exit test before the next step starts.

The gap is depth of semantics. Language count is not the gap.

## Where Stratify stands today

Measured against v0.6.0, not assumed.

| Area | State | Source |
|------|-------|--------|
| Analyses | Six: dead code, duplication, complexity, churn hotspots, cycles, layer boundaries | `crates/stratify-analysis/src/` |
| Languages | All six languages get six of six | `README.md` |
| IR | Symbols, references, normalized tokens, one complexity number per function. No types, no statements, no control flow | `crates/stratify-core/src/ir.rs` |
| Call resolution | Cross-file calls match by bare function name and land at `Likely` | `crates/stratify-analysis/src/resolve.rs` |
| Findings | Five fields: rule, severity, message, span, confidence, plus optional `related`, `path`, and `reasons`. Rule is a free string | `crates/stratify-core/src/finding.rs` |
| Gate | `--fail-on` severity threshold, `--baseline` new-findings gate, `stratify-ignore` comment suppression | `crates/stratify-cli/src/main.rs` |
| LSP | Rescans the whole workspace on every open and save | `crates/stratify-cli/src/lsp.rs` |
| Judgment layer | `stratify-judge` ships one judge, for `dead_code` | sibling repo |

Self-scan of this repo (about 8,700 lines of Rust) on 2026-10-01:

- 73 findings in 0.37 seconds.
- 15 `dead_code` findings, all Info and `Likely`. Every one has a live caller. Bare-name resolution is the cause.
- 40 duplication, 11 hotspot, 7 complexity.

Dead-code precision on this repo is 0 of 15. Resolution quality is the first thing to fix.

## Core shift

The IR today is a symbol and reference graph. Flow analysis needs three layers on top:

1. Resolved calls: each call edge points at one target, with a confidence the resolver earned.
2. Statement-level facts: assignments, calls, returns, and branches inside each function.
3. Framework models: which calls produce untrusted data and which calls are dangerous to feed.

Layer 1 improves the six existing analyses. Layers 2 and 3 enable new ones. Build them in this order.

## Constraints

These hold through every phase.

- One binary. No servers. No accounts.
- Analyses read only the IR and never branch on language. Framework knowledge lives in data files, not in analysis code.
- Every finding carries a confidence. A heuristic result reports `Likely`. Nothing reports `Certain` unless every hop resolved `Certain`.
- JSON schema changes are additive. `stratify-judge` reads the report over a pipe and must keep working.
- A new rule reaches Warning severity only after the rule meets the precision target on the labelled corpus.

## Open decisions

Settle these before Phase 2 starts.

| Decision | Options | Recommendation |
|----------|---------|----------------|
| Source of type facts | tree-sitter only, or import facts from external indexers (SCIP, `tsc`, `gopls`) | tree-sitter first. Add an optional SCIP import when a language stalls below the precision target |
| First flow language | TypeScript, Python, Go, Java, Ruby | TypeScript: explicit imports, type annotations in source, high demand for security rules. Go is the lower-risk alternative |
| Precision target | Any number | 80 percent per rule, per language, before Warning |
| Scan-time budget | Any number | Set in Phase 0 from a measured run on a repo above 500k lines. The row exists (kubernetes, 90.27 s). The owner chooses the number |

## Phase 0: Baseline and measurement

Status: in progress.

Shipped:

- Rust cycles and layer boundaries.
- The precision tool, `scripts/precision.py`. It exits 0 on a clean run, 1 on a validation failure, and 2 on a clone, checkout, or scan failure.
- The applicability policy. `corpus/applicability.json` exists with no entries, so all 36 pairs stay applicable (`5c21b48`). Pairs have the statuses `gate-ready`, `insufficient`, `no-data`, `incomplete`, and `not-applicable`. The gate-ready bar is unchanged. Sparse rules follow an evidence plan in the corpus docs.
- The first labels. The latest run of the tool reads:

  `gate-ready: 0 of 36 applicable pairs (0 not applicable). Precision targets apply only to gate-ready pairs.`

  The run used `target/release/stratify` at `90d3a73` against a local clone of this repo, because local `main` is not pushed and the default clone of the corpus URL cannot reach the pinned commit `a809171`. Only Rust `dead_code` has labels: tp 0, fp 15, unlabelled 0, repos 1, coverage 100%, precision 0%, status `insufficient`. Rust `duplication`, `complexity`, `hotspot`, and `cycle` are `incomplete`. Every other pair is `no-data`. The tool measures no pair.
- The benchmark tool, `scripts/bench.py`, and these release rows in `docs/benchmarks.md`, both at `090da17` on Darwin arm64, Apple M5:
  - stratify self: 50 files, 11,984 lines, 0.71 s, 36.2 MB peak RSS.
  - kubernetes v1.31.0 (`9edcffc`): 15,372 files, 4,520,718 lines, 90.27 s, 8,186.5 MB peak RSS. The scan completed.
- The performance fix (`71b5005`). A profile showed 100% of samples in `deadcode::analyze`. The fix indexes edges by source. The self-scan and 23 fixtures give byte-identical output.
- Scan statistics describe one population. A file that is not valid UTF-8 is skipped and counted in `files_skipped`, and lines count only for parsed files (`2ac3fef`).

Remaining:

- Labelled repos for every applicable pair. Today 0 of 36 are gate-ready, and 30 pairs have no labels at all.
- The scan-time budget. The large-repo row now exists. Choosing the budget from it is the owner's decision. This document sets none.
- Pushing `main`, so the corpus pin `a809171` is reachable from the default clone URL.

Phase 0 stays open until every applicable pair is gate-ready and the owner sets the scan-time budget. The large-repo benchmark is done.

Work:

1. Add Rust `use` and `mod` resolution so Rust gets cycles and layer boundaries. The README already promises this.
2. Commit a labelled corpus: real repos per language, each finding marked true or false. The August tool-comparison fixtures were never committed. Start from them.
3. Add a script that compares a scan against the labels and validates the labels. It rejects stale, ambiguous and duplicate labels, malformed label files, and unlabelled findings of a rule declared fully labelled. It reports precision, coverage and status for every rule-language pair, and marks pairs listed in `corpus/applicability.json` as `not-applicable`.
4. Record scan time and peak memory on one repo above 500k lines. Use the file and line counts Stratify reports scanning.

A rule-language pair is gate-ready when three conditions hold:

- every finding of that rule in that language in the corpus carries a label;
- the labels come from 2 or more repos;
- 30 or more findings are labelled.

Precision counts labelled findings only. A pair that is not gate-ready reports a sample, not a measurement. Every precision target in this roadmap applies to gate-ready pairs only.

A pair is applicable unless `corpus/applicability.json` lists it with a product reason and a reviewer. The tool reports a listed pair as `not-applicable`. A listed pair with corpus findings is a validation error. The bar of 2 repos and 30 labelled findings is never lowered, and synthetic fixtures never count as corpus evidence.

Exit:

- Rust gets six of six analyses.
- Every applicable rule-language pair is gate-ready, each with a precision number.
- Every not-applicable pair carries a reviewed reason in `corpus/applicability.json`.
- The Phase 0 status lists every unmeasured pair explicitly.
- A scan-time and memory baseline exists for one repo above 500k lines.

## Phase 1: Findings model and adoption basics

Status: shipped: rule registry, evidence fields with the SARIF mapping, comment suppression, and the baseline gate with the hardened Action. SARIF output validates against the official OASIS 2.1.0 schema, and CI runs the check (`3f02fcf`). Unknown rule ids in suppression markers print a warning on stderr (`39c2e10`). The judge-compatibility test passes. `docs/judge-compat.md` records the result: code inspection shows the judge never reads the evidence fields, a request capture showed 32 identical bodies per variant, and three live runs show only model noise (`c6aea0b`, `197d25e`, `90d3a73`).

Objective: give findings enough structure to explain flow results, and give teams a way to adopt a new rule without a wall of old findings.

Work:

1. Rule registry: id, default severity, category, supported languages, tags, remediation text.
2. Finding evidence, in three optional fields:
   - `related`: supporting spans with roles, in no particular order.
   - `path`: the ordered steps of a flow. A cycle's files now, a taint path's hops in Phase 5.
   - `reasons`: an ordered reason chain.

   SARIF mapping:
   - `related` becomes `relatedLocations`.
   - `path` becomes one `codeFlows` entry holding one thread flow, with one location per step in path order and `executionOrder` counting from 1.
   - `reasons` becomes `properties.reasons`, and also the code flow's message when a path exists.

   A reason has no location, so it never becomes a thread-flow step.
3. Optional evidence fields on schema version 1. `stratify-judge` skips judgment on any schema version it does not know, so additive fields do not bump the version.
4. Line-level suppression comment with a required rule id. The marker counts only inside a comment node of the file's grammar, never inside a string literal.
5. Baseline file and a gate mode failing only on findings absent from the baseline.

Exit:

- All six analyses emit evidence. A cycle finding carries every file in the cycle as an ordered path. A duplication finding lists every clone site.
- The GitHub Action gates on new findings only.
- `stratify-judge` judges a report carrying evidence fields with no change in verdicts.

## Phase 2: Call and type resolution

Objective: replace bare-name matching with import-aware and receiver-aware resolution.

Work:

1. Extend the IR with signatures, declared types where the syntax states them, override chains, and import and export visibility.
2. Resolve each call through the caller's imports and the receiver's declared type before falling back to name matching.
3. Keep the fallback at `Likely`. Promote to `Certain` only on a unique, import-backed match.

Per-language work:

| Language | Resolution work | Stays `Likely` |
|----------|-----------------|----------------|
| Rust | `use` paths, trait impl matching, methods on declared types | Macro-generated code |
| Go | Package imports, method sets, interface satisfaction | Reflection |
| TypeScript | Module resolution, re-export chains, annotated types | Inferred types without `tsc` |
| Java | Imports, class hierarchy, overloads by arity | Generics, reflection, annotation-driven wiring |
| Python | Import graph, class hierarchy, method resolution order | Decorators, dynamic attributes |
| Ruby | Modules, mixins, constant lookup | `send`, `method_missing`, metaprogramming |

Exit:

- Self-scan reports zero `dead_code` findings with a live caller. Today the count is 15.
- `dead_code` meets the precision target on the corpus in at least four languages.
- The share of `Certain` call edges is recorded per language.

## Phase 3: Architecture policy

Objective: turn the boundary and cycle analyses into a policy engine. This is the existing differentiator and needs no flow analysis.

Work:

1. Allowlist rules ("this layer imports only these layers") next to the existing `[[forbid]]` rules.
2. Package-level and service-level boundaries for monorepos.
3. Cycle severity by size and by the layers a cycle crosses.
4. Architecture drift: reuse the Phase 1 baseline to report only new violations.

Exit:

- One monorepo in the corpus runs a policy with allowlist rules and zero false violations.
- A pull request adding a forbidden import fails the gate. A repo with old violations in the baseline passes.

## Phase 4: Statement-level IR and control flow, one language

Objective: add flow facts for the first flow language without touching the other five extractors.

Work:

1. IR additions: basic blocks per function, statements (assign, call, return, branch), and def-use links.
2. Extend one extractor to emit them. Functions without flow facts skip flow analyses.
3. First checks: unreachable code, constant conditions, use before assignment.
4. Cache per-file IR by content hash. The LSP rescans only changed files.

Exit:

- Each new check meets the precision target on the corpus.
- Full-scan time stays inside the Phase 0 budget.
- An LSP save on a one-file change re-parses one file.

## Phase 5: Taint analysis and framework models, same language

Objective: report injection flaws with the full source-to-sink path as evidence.

Work:

1. Taint propagation inside one function, then across functions through per-function summaries.
2. Framework model files listing sources, sinks, and sanitizers. Data, not code. Start with two frameworks for the first language.
3. Rule families in order: SQL injection, command injection, path traversal. Then template injection and unsafe deserialization.
4. A taint judge in `stratify-judge` for the `Likely` residue.

Exit:

- Each rule meets the precision target on a public vulnerable-app corpus.
- Recall is measured against Semgrep and CodeQL on the same corpus and published in the docs.
- Every taint finding shows each hop from source to sink.

## Phase 6: Second language and surfaces

Objective: repeat Phases 4 and 5 for a second language and improve the editor and pull-request surfaces.

Work:

1. Second flow language, same exit tests.
2. Pull-request comments on changed lines only.
3. Rule explanations in LSP diagnostics, drawn from the rule registry.

Start this phase only after Phase 5 meets its exit tests.

## Parallel track: pattern rules

These need no flow analysis and start after Phase 1:

- hardcoded secrets
- insecure TLS and crypto settings
- weak configuration defaults

Each follows the same rule: Info until the precision target is met.

## Out of scope

- Rust borrow and lifetime checks. `rustc` already enforces them.
- Java annotation processing and full generics. Both need the compiler.
- Authentication and authorization checks. They need route models. Revisit after Phase 5.
- Organization-level policy management. This needs a server and breaks the no-servers promise.
- Hosted dashboards. OTLP export already feeds Datadog and other backends.
- Auto-fix. The original design excludes code modification.

## Success criteria

- Every rule at Warning severity holds at least 80 percent precision on the labelled corpus.
- Self-scan reports zero findings with a known false label.
- A taint finding names the source, the sink, and each hop between them.
- Full-scan time stays inside the Phase 0 budget on a repo above 500k lines.
- The baseline gate lets a team adopt Stratify on a legacy repo with zero day-one failures.
- Two languages have flow analysis. Six languages have resolution-backed structural analysis.

## Risks

| Risk | Mitigation |
|------|------------|
| tree-sitter resolution stalls below the precision target | Optional SCIP import, decided per language |
| Statement-level IR raises memory use on large repos | Analyze per function, keep summaries, drop bodies after use |
| Framework models go stale | Keep them as versioned data files with corpus tests per framework |
| Six extractors drift apart | Shared walker helpers in `stratify-lang`, same as tokenize and complexity today |
| Scope creep back to "all languages, all rules" | Phase 6 gate: no second language before Phase 5 exit tests pass |
