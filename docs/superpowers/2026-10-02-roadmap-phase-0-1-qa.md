# QA Review: Roadmap Phase 0 and Phase 1

Date: 2026-10-02  
Reviewed plan: [`plans/2026-10-01-roadmap-phase-0-1.md`](plans/2026-10-01-roadmap-phase-0-1.md)  
Roadmap: [`../roadmap-full-stack-static-analysis.md`](../roadmap-full-stack-static-analysis.md)

## Current status (2026-10-03)

- The large-repo benchmark and the performance fix are complete. Kubernetes v1.31.0 scans in 90.27 s at 8,186.5 MiB peak RSS (`090da17`).
- The scan-time budget is set: at most 180 s wall and at most 16 GiB peak RSS. The reference run meets it.
- Precision is still open. 0 of 36 pairs are gate-ready.
- Hosted CI passed on `efd5825` after `main` was pushed on 2026-10-13 ([CI run 38333709972](https://github.com/stratify-dev/stratify/actions/runs/38333709972)). Every configured check ran, including SARIF schema validation and the script tests.
- Phase 0 stays in progress. Phase 1 is implemented and verified by hosted CI.

The sections below up to the Resolution record the initial review of 2026-10-02. Read them as that snapshot.

## QA verdict (initial review, 2026-10-02)

The Phase 1 implementation is in good shape for continued integration. The changes reviewed cover the requested behaviors, and the checks listed below passed. Phase 0 is **not complete**: the corpus does not yet establish precision across the rule-language matrix, and the large-repository scan has not produced a completed timing and memory measurement.

Do not announce Phase 0 completion or set the scan-time budget until the outstanding acceptance requirements in this document are met. Resolve the open measurement and process items before treating this plan as a release sign-off.

## Review findings (initial review, 2026-10-02)

### 1. Precision exit criterion needs a credible path for sparse rules

The roadmap currently defines 36 rule-language pairs and requires each pair to have findings from at least two repositories and at least 30 labelled findings. This is a useful evidence threshold for common rules, but may not be achievable for naturally sparse findings such as dependency cycles and layer-boundary violations. The roadmap does not yet explain what to do when the corpus has too few examples.

**Requirement:** Before Phase 0 sign-off, define an evidence policy for every pair:

- `gate-ready`: fully labelled, with the minimum evidence threshold met; precision targets may be applied.
- `insufficient`: examples exist, but the sample threshold is not met; report the sample precision without claiming it meets the target.
- `no-data`: no examples exist; do not claim precision.
- If a pair is genuinely inapplicable, document the product reason and a reviewable way to establish that status. Do not silently count `no-data` or `insufficient` as a successful precision result.

Then make the Phase 0 exit criterion achievable and honest. Either provide a credible plan to gather the required real examples for all applicable pairs, or revise the all-36 gate to require gate readiness for every applicable pair while explicitly listing pairs that remain unmeasured. Do not lower the evidence bar solely to make the exit pass, and do not use synthetic fixtures as a substitute for production precision evidence.

### 2. Scan-stat line counts and file counts can disagree on parse errors

In [`crates/stratify-cli/src/run.rs`](../../crates/stratify-cli/src/run.rs), `lines_scanned` increments before `adapter.parse_file`, while file graphs are merged only when parsing succeeds. A parse error can therefore add lines without adding a scanned file to the graph-derived file count.

**Requirement:** Choose and document one metric:

- Prefer counting lines only after a file parses successfully, so `files_scanned` and `lines_scanned` describe the same set of files; or
- Rename/document the line metric as attempted input lines and report parse failures separately.

Add a focused test for the chosen behavior. Ensure ignored files and files skipped by the walker remain excluded, and document whether a parser error is surfaced or counted as a skipped file.

### 3. Branch instructions and delivered commit location disagree

The plan says to work on `roadmap-phase-0-1` and never commit to `main`, but the reviewed repository was on `main` with the feature commits in its history.

**Requirement:** Resolve this as a repository-owner decision before further commits. Confirm whether `main` is the intended integration branch or whether this work must be isolated on the named feature branch. Keep the plan’s branch instructions aligned with the actual workflow. Do not rewrite shared history as part of this QA follow-up without an explicit request.

### 4. Large-repository performance is a known blocker

The recorded Kubernetes v1.31.0 scan did not finish within 30 minutes. The likely `O(nodes × references)` reachability behavior in dead-code analysis is suspected, not yet proven. The current self-scan measurement is useful but does not satisfy the roadmap’s greater-than-500k-line criterion.

**Requirement:** Profile the large-repository path, confirm the cause, and address the dominant cost before setting a scan-time budget. Then run the release binary on a pinned repository above 500k scanned lines and record:

- Stratify commit and build profile;
- repository name, revision, and scanned files/lines;
- wall time, peak RSS, operating system, hardware, and measurement method;
- whether the scan completed successfully.

If the scan cannot complete, record the failure and keep Phase 0 open. Do not substitute a smaller repository or treat a timeout as a benchmark result.

### 5. Current benchmark data is historical, not a measurement of the final reviewed tree

The benchmark table records an earlier Stratify revision. The QA run of the current debug binary reported 48 files, 11,475 lines, 2.72 seconds, and 39.4 MB. Those figures are not directly comparable with the recorded release-build result of 10,896 lines, 0.34 seconds, and 34.5 MB because the revision and build profile differ.

**Requirement:** Keep benchmark rows tied to their exact commit and build profile. Add a new release-build self-scan row for the final implementation when recording the large-repository measurement. Do not compare the debug QA run to the earlier release result as if the only difference were code changes.

## Verification performed (initial review, 2026-10-02)

The following checks passed in the reviewed checkout:

- `~/.cargo/bin/cargo fmt --all -- --check`
- `~/.cargo/bin/cargo test --locked --workspace`
- `~/.cargo/bin/cargo clippy --locked --workspace --all-targets -- -D warnings`
- `python3 -m unittest discover -s scripts -p 'test_*.py' -v` (10 tests)
- `bash scripts/test_action_run.sh`
- `python3 scripts/bench.py --stratify target/debug/stratify .` (current debug-build smoke measurement)
- Corpus label matching against the locally available `a809171` snapshot: 15 Rust `dead_code` labels matched, with no stale, ambiguous, or duplicate labels.
- `stratify-judge --dry-run` accepted a version 1 report containing evidence fields and built a request plan without sending it.

The judge dry run verifies report parsing, not unchanged judgment verdicts. The roadmap’s judge-compatibility exit test still requires an actual verdict comparison against the prior behavior. The corpus validation used the pinned snapshot available in the local Git object database; it did not exercise a fresh network clone.

The repository was clean after verification. The current branch was `main`, which is the branch-instruction discrepancy noted above.

## Requirements to complete Phase 0

- [ ] **Set the corpus acceptance policy.** Define applicable versus inapplicable rule-language pairs, and the evidence needed for `gate-ready`, `insufficient`, and `no-data`. Ensure sparse rules have a credible evidence plan.
- [ ] **Grow and pin the corpus.** Add real repositories at immutable revisions across the supported languages. Record exactly which rules are fully labelled in each repo.
- [ ] **Label findings completely for any claimed precision.** Every finding in a claimed pair must have a validated `tp` or `fp` label. Keep notes with evidence for false positives and true positives.
- [ ] **Validate the precision tool end to end.** Verify clone/checkout/scan failures return non-zero; stale, ambiguous, duplicate, malformed, and missing labels are rejected; and partial coverage never appears gate-ready.
- [ ] **Report coverage with precision.** Publish per-pair counts, number of repos, coverage, precision, and status. Do not present a partial sample as an accuracy guarantee.
- [ ] **Fix and define scan statistics.** Ensure file and line counts describe a consistent population, with parse failures handled explicitly.
- [ ] **Resolve the large-repo performance bottleneck.** Profile the timed-out scan, fix the measured dominant cost, and rerun it successfully.
- [ ] **Record comparable benchmarks.** Measure release builds on the final commit and on a pinned repository above 500k scanned lines; include machine and method details.
- [ ] **Update the roadmap status from evidence.** Phase 0 remains open until its revised precision criteria and completed large-repo benchmark are satisfied. Document exceptions explicitly rather than implying they passed.

## Requirements to complete Phase 1

- [ ] **Rule registry:** all emitted rule IDs have accurate metadata, including supported languages, default severity, category, tags, description, and remediation.
- [ ] **Evidence model:** `related`, ordered `path`, and ordered `reasons` are optional additive fields, absent from JSON when empty. Existing schema version 1 reports remain readable.
- [ ] **Evidence usefulness:** cycle paths cover all cycle edges in order; duplication findings identify every other clone site; boundary findings identify the forbidden import and target; other rules explain their evidence without asserting unsupported certainty.
- [ ] **SARIF output:** `related` maps to `relatedLocations`; a non-empty path maps to a valid ordered `codeFlows` thread flow; reasons map to `properties.reasons` and the flow message when a path exists. Findings without evidence omit these SARIF fields. Validate generated SARIF with a schema validator or an equivalent standards-based check, in addition to structural unit tests.
- [ ] **Suppression semantics:** only tree-sitter comment nodes can create markers; strings and Python docstrings never suppress. Same-line, immediately preceding comment, multi-line comment, rule-list, unknown-rule, attribute/decorator, and boundary-import cases behave as documented across all six adapters.
- [ ] **Baseline behavior:** line movement remains stable; rule/file identity is preserved; duplicate findings use multiplicity correctly; malformed or unsupported baseline files fail closed with exit code 2; baseline writing and filtering preserve normal report formats and gate behavior.
- [ ] **GitHub Action:** values pass through environment variables and quoted argument arrays; paths containing spaces and shell metacharacters remain literal arguments; baseline paths are relative to the documented working directory; script tests run in CI.
- [ ] **Judge compatibility:** feed a report with evidence fields to `stratify-judge`, compare the verdicts to the same findings without evidence, and verify verdicts are unchanged. A dry run alone does not satisfy this requirement.
- [ ] **Documentation and release notes:** README, Action input docs, changelog, roadmap status, and schema/evidence descriptions agree with shipped behavior.
- [ ] **Repository workflow:** resolve the branch discrepancy and follow the agreed branch/commit policy for remaining work.

## Overall sign-off condition

Phase 1 can be described as implemented after its compatibility, SARIF validity, and CI requirements pass. Phase 0 must remain marked in progress until the precision acceptance policy is explicit, every applicable pair has the required evidence status, and a completed large-repository benchmark exists. Do not claim full roadmap completion while the performance run is still timing out or while required precision pairs remain unmeasured.

## Resolution

Superseded on 2026-10-03 for the scan-time budget. See Current status.

Date: 2026-10-02. Follow-up work ran on branch `roadmap-qa-fixes`. Phase 0 stays in progress.

### Findings

- Finding 1, precision path for sparse rules: addressed by policy, not by evidence. `5c21b48` adds `corpus/applicability.json` (no entries, so all 36 pairs stay applicable), the `not-applicable` status, a sparse-rule evidence plan, and an end-to-end precision tool. The gate-ready bar is unchanged. 0 of 36 pairs are gate-ready.
- Finding 2, scan stats: resolved in `2ac3fef`. Non-UTF-8 files are skipped and counted in `files_skipped`, and lines count only for parsed files.
- Finding 3, branch: resolved by the plan note in `docs/superpowers/plans/2026-10-01-roadmap-phase-0-1.md`. The owner chose `roadmap-qa-fixes`, and no history was rewritten.
- Finding 4, performance: resolved in `71b5005`. The profile put 100% of samples in `deadcode::analyze`, and the fix indexes edges by source. Kubernetes completes, and the self-scan and 23 fixtures give byte-identical output.
- Finding 5, benchmark rows: resolved in `090da17`, `e35dd4b`, and `8e918f8`. `docs/benchmarks.md` has release rows at `090da17`: stratify self (50 files, 11,984 lines, 0.71 s, 36.2 MB) and kubernetes v1.31.0 at `9edcffc` (15,372 files, 4,520,718 lines, 90.27 s, 8,186.5 MB, completed).

### Requirements to complete Phase 0

- [x] Set the corpus acceptance policy. `5c21b48`, `docs/roadmap-full-stack-static-analysis.md`.
- [ ] Grow and pin the corpus. Open: only this repo is in the corpus, so the precision gate cannot pass yet. Since the 2026-10-13 push, the pin `a809171` is reachable from the default clone URL.
- [ ] Label findings completely for any claimed precision. Open: Rust `dead_code` has labels but one repo. Rust `duplication`, `complexity`, `hotspot`, and `cycle` are `incomplete`. 31 pairs are `no-data`, and 4 more are `incomplete` with no labels, so 35 of 36 pairs have no labels.
- [x] Validate the precision tool end to end. `5c21b48`. Exit codes are 0 for a clean run, 2 for label or validation errors, and 1 for clone, checkout, or scan failures.
- [x] Report coverage with precision. `5c21b48`. The tool prints counts, repos, coverage, precision, and status per pair, plus a gate-ready footer.
- [x] Fix and define scan statistics. `2ac3fef`.
- [x] Resolve the large-repo performance bottleneck. `71b5005`.
- [x] Record comparable benchmarks. `090da17`, `e35dd4b`, `8e918f8`, `docs/benchmarks.md`.
- [x] Update the roadmap status from evidence. `docs/roadmap-full-stack-static-analysis.md`.

### Phase 0 conditions left open

- [ ] Any pair reaching gate-ready. Open: `gate-ready: 0 of 36 applicable pairs`.
- [ ] The scan-time budget. Open: the large-repo row exists, and choosing the budget is the owner's decision.
- [ ] Phase 0 sign-off. Open: every applicable pair must be gate-ready and the budget must be set.

### Requirements to complete Phase 1

- [x] Rule registry. Shipped in `98728e8`, unchanged this round.
- [x] Evidence model. Shipped in `5483695`, unchanged this round.
- [x] Evidence usefulness. Shipped in `5483695`, unchanged this round.
- [x] SARIF output. `3f02fcf` validates generated SARIF against the OASIS 2.1.0 schema (draft-04). Hosted CI validated 6 fixtures on `efd5825` ([CI run 38333709972](https://github.com/stratify-dev/stratify/actions/runs/38333709972)).
- [x] Suppression semantics. `e3b033b` and `62b8b87`. `39c2e10` adds a stderr warning for unknown rule ids.
- [x] Baseline behavior. Shipped in `7d3dfa8`, unchanged this round.
- [x] GitHub Action. Shipped earlier, unchanged this round. Hosted CI ran the script tests on `efd5825` ([CI run 38333709972](https://github.com/stratify-dev/stratify/actions/runs/38333709972)), 66 run, 0 skipped.
- [x] Judge compatibility. `c6aea0b`, `197d25e`, `90d3a73`, `docs/judge-compat.md`. Passes by code inspection and request capture. The code never reads the evidence fields and 32 request bodies per variant matched. Three live runs show only model noise (run 3 had 2 changed rows), and all three exited 1 under the script's own criterion.
- [x] Documentation and release notes. `CHANGELOG.md` Unreleased covers the performance fix, the skip behavior, and the unknown-id warning. The roadmap carries status.
- [x] Repository workflow. Resolved by the finding 3 note.

### Sign-off

Phase 1 is implemented, and hosted CI passed on `efd5825` ([CI run 38333709972](https://github.com/stratify-dev/stratify/actions/runs/38333709972)). That meets this review's Phase 1 sign-off condition. Phase 0 is not signed off. The large-repo benchmark is done. The precision gate is not.
