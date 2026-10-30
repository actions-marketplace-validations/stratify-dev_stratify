# Roadmap Phase 0-1 QA Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Resolve every finding and open checklist item in the 2026-10-02 QA review. Profile and fix the large-repo scan, and re-measure it. Do not call Phase 0 complete unless the evidence supports it.

**Architecture:** The fixes are small and focused, and each lives where the existing code lives:
- Scan accounting stays in `stratify-cli/src/run.rs`.
- Corpus policy stays in `corpus/` and `scripts/precision.py`.
- SARIF validation becomes a script plus a CI step.
- Performance work goes wherever profiling shows the cost.
- Judge compatibility becomes a reproducible script, run once with the real model.

**Tech Stack:** Rust workspace, Python 3 standard library (plus `jsonschema` for SARIF validation only), bash, macOS `sample` for profiling.

**Spec:** `docs/superpowers/2026-10-02-roadmap-phase-0-1-qa.md` (the QA review), `docs/roadmap-full-stack-static-analysis.md`.

## Global Constraints

- Work on branch `roadmap-qa-fixes`. The owner chose this branch, and it merges to `main` only after review. Never commit to `main`, and never rewrite history.
- One binary. No servers, no accounts. Analyses read only the IR and never branch on language.
- `Report::SCHEMA_VERSION` stays `1`. JSON changes stay additive.
- Every change passes these checks:
  - `cargo fmt --all -- --check`
  - `cargo clippy --all-targets -- -D warnings`
  - `cargo test --workspace --locked`
  - `bash scripts/test_action_run.sh`
  - `cd scripts && python3 -m unittest discover -p 'test_*.py'`
- Before any cargo command, run `export PATH="$HOME/.cargo/bin:$PATH"`.
- Commit subjects use repo style (`feat(scope): ...`, `fix(scope): ...`, `docs: ...`, `perf(scope): ...`). No trailer.
- Status language rules:
  - Phase 0 stays "in progress" unless every applicable rule-language pair is gate-ready AND a completed benchmark exists on a repo above 500k scanned lines.
  - A timeout is never a benchmark result.
  - A dry run never counts as the judge-compatibility test.
- Secrets: the TypeSafe key comes only from `set -a; source ~/.local_config; set +a`. Never print the key, write it to a file, or commit it.

## Review Focus

1. A non-UTF-8 source file in the scanned tree. Today `std::fs::read_to_string(path)?` aborts the whole scan. Expected: the scan skips the file, counts it as skipped, and finishes. Covered in Task 1.
2. A pair listed as not applicable that has findings in the corpus. Expected: a validation error (exit 2), never a silent `not-applicable`. Covered in Task 3.
3. A perf fix that changes which findings are reported. Expected: identical findings before and after on every fixture and on this repo's self-scan. Covered in Task 5.
4. SARIF from a finding with an ordered path. Expected: it validates against the official SARIF 2.1.0 schema. Covered in Task 4.
5. A judge comparison whose second run hits the cache. Expected: both runs pass `--no-cache`, so every verdict is a fresh model answer. Covered in Task 7.

---

### Task 1: Consistent scan statistics (QA finding 2)

**Files:** `crates/stratify-cli/src/run.rs`, `crates/stratify-cli/src/main.rs`, `crates/stratify-cli/tests/e2e_stats.rs`, `README.md` (the `--stats` section), `CHANGELOG.md`.

**Interfaces produced:**
- `ScanStats.lines_scanned` counts only files that parsed successfully. It covers the same population as `files_scanned`.
- New field `ScanStats.files_skipped: u64`. It counts source files the walker reached, that passed the ignore globs, and that a registered adapter handles, but that could not be read as UTF-8 or that the adapter returned `Err` for.
- `--stats` JSON gains `"files_skipped"`.

**Behavior change:** an unreadable or non-UTF-8 source file no longer aborts `stratify check`. Today `read_to_string(path)?` returns the error for the whole scan. Instead, print `stratify: skipped <rel>: <reason>` to stderr, count the file in `files_skipped`, and continue. Files excluded by ignore globs, by the walker (gitignored or hidden), or by having no adapter stay out of all three counts.

- [ ] Write the failing unit test in `run.rs` `stats_tests`. It builds a temp dir holding a 4-line `a.rb`, a `b.rb` made of the bytes `[0x64, 0x65, 0x66, 0x20, 0xff, 0xfe, 0x0a]` (invalid UTF-8), and a `notes.txt`. It calls `analyze_repo_with_stats` and asserts `Ok`, `files_scanned == 1`, `lines_scanned == 4`, and `files_skipped == 1`.
- [ ] Extend `e2e_stats.rs`. Assert that the stats JSON contains `"files_skipped": 0` for `sample-ruby`.
- [ ] Implement:
  - Replace the `?` on `read_to_string` with a match that skips and counts the file.
  - Move `lines_scanned +=` inside the `Ok(file_graph)` branch.
  - Count adapter `Err` as skipped, with the same stderr line.
- [ ] Update the README `--stats` section and add a `### Fixed` CHANGELOG bullet stating that a non-UTF-8 source file is skipped and counted instead of aborting the scan.
- [ ] Run all checks, then commit `fix(cli): skip unreadable source files and keep scan counts consistent`.

### Task 2: Warn on unknown rule ids in suppression markers

**Files:** `crates/stratify-cli/src/run.rs`, `crates/stratify-cli/src/main.rs`, a new e2e test, `README.md`, `CHANGELOG.md`.

**Requirement:** the QA asks that the unknown-rule case "behave as documented". Today a marker such as `stratify-ignore: deadcode` (a typo) silently suppresses nothing.

New behavior: `stratify check` prints `warning: <file>:<line>: unknown rule id \`<id>\` in stratify-ignore marker` to stderr for each rule id in a recorded suppression that `stratify_core::rule::rule(id)` does not know. It still suppresses nothing for that id. LSP and MCP stay silent, because the warning belongs to the CLI only.

Carry the data out of `analyze_repo_with_stats` in `ScanStats` as `unknown_suppression_rules: Vec<(String, usize, String)>` (file, start line, id).

- [ ] Failing e2e test: a temp repo whose `a.rb` holds `# stratify-ignore: deadcode` above an unused method. Assert stderr contains ``unknown rule id `deadcode` `` and that the finding is still reported.
- [ ] Implement it. Then add one README sentence ("An unknown rule id prints a warning and suppresses nothing") and a CHANGELOG bullet.
- [ ] Run all checks, then commit `feat(cli): warn on unknown rule ids in stratify-ignore markers`.

### Task 3: Corpus acceptance policy and an end-to-end precision tool (QA finding 1, Phase 0 checklist)

**Files:**
- `corpus/applicability.json` (new)
- `corpus/README.md`
- `scripts/precision.py`
- `scripts/test_precision.py`
- `docs/roadmap-full-stack-static-analysis.md` (Phase 0 work item 3 and exit)

**Policy (record it in the corpus README and the roadmap):**
- A pair (rule, language) is **applicable** unless `corpus/applicability.json` lists it with a `reason` (the product reason) and a `reviewed_by` (who agreed). Today all 36 pairs are applicable, so the file starts as `{"not_applicable": []}`.
- The statuses stay `gate-ready`, `insufficient`, `incomplete` and `no-data`. Add `not-applicable` for listed pairs.
- A listed pair that has corpus findings is a validation error. Findings prove the rule applies.
- The bar stays at 2 repos and 30 labelled findings. It is never lowered to make the exit pass. Synthetic fixtures never count as corpus evidence.
- Sparse rules (`cycle`, `boundary`) get an evidence plan in the corpus README: pick corpus repos known to carry real import cycles, and repos with a real layer config (a `stratify.toml`, a Rails app, or a Maven layered app). Then label those rules exhaustively in those repos.
- Phase 0 exit (roadmap): every **applicable** pair is gate-ready. Every not-applicable pair carries a reviewed reason. The roadmap status lists every unmeasured pair explicitly.

**Tool changes:**
- Load `corpus/applicability.json`. Validate each entry: known rule, known language, non-empty `reason` and `reviewed_by`.
- Mark listed pairs `not-applicable`.
- Change the footer to `gate-ready: N of M applicable pairs (K not applicable).`
- Harden the earlier deferred minors:
  - A missing `labels` key or a non-list `labels` is a schema error.
  - A non-object label is a schema error, never a `TypeError`.
  - A missing labels file, or scan output that is not valid JSON, becomes a `precision:` message with exit 1, never a traceback.
  - Schema errors from earlier repos survive a later clone failure. Print them, then exit 1.

**Tests:**
- Unit tests for each new validation and for the `not-applicable` status and footer.
- An end-to-end test that runs `precision.py main()` as a subprocess against a temp corpus. It uses a fake `--stratify` script that prints a fixed JSON report, and a local git repo as the clone source. The cases:
  - exit 0 with a valid corpus
  - exit 2 for a stale label
  - exit 1 when the fake binary exits non-zero
  - exit 1 when the repo URL cannot be cloned

Commit `feat(corpus): add an applicability policy and harden the precision tool`.

### Task 4: SARIF schema validation (Phase 1 checklist)

**Files:**
- `scripts/schemas/sarif-schema-2.1.0.json` (new, vendored)
- `scripts/validate_sarif.py` (new)
- `scripts/test_validate_sarif.py` (new)
- `.github/workflows/ci.yml`

**Requirement:** validate the SARIF Stratify generates against the official OASIS SARIF 2.1.0 JSON schema, on top of the structural unit tests.

- Vendor the schema. Download it from `https://raw.githubusercontent.com/oasis-tcs/sarif-spec/main/sarif-2.1/schema/sarif-schema-2.1.0.json`, or the schemastore copy if that URL moves. Record the source URL and download date in a comment line in `validate_sarif.py`.
- `validate_sarif.py <file>...` validates each file with `jsonschema`, using the draft the schema declares. It exits 1 with the failing JSON path on the first error, and prints `ok <file>` otherwise. If `jsonschema` is missing, it exits 2 with an install hint.
- The test generates SARIF with the debug binary for these fixtures:
  - `sample-rustcycle` (ordered `codeFlows`)
  - `sample-dup` (`relatedLocations`)
  - `sample-boundary`
  - `sample-java` (no evidence)
  - a fixture with suppressed and complexity findings

  It validates every result. The test skips when the binary or `jsonschema` is missing, and says so.
- Install `jsonschema` locally in a scratch venv only, never system-wide: `python3 -m venv /tmp/sarif-venv && /tmp/sarif-venv/bin/pip install jsonschema`.
- CI: add a step after `Tests`:

  ```yaml
  - name: SARIF schema validation
    run: |
      pip install jsonschema==4.*
      cargo build -q -p stratify-cli
      for d in sample-rustcycle sample-dup sample-boundary sample-java; do
        target/debug/stratify check crates/stratify-cli/tests/$d --format sarif > /tmp/$d.sarif
      done
      python3 scripts/validate_sarif.py /tmp/*.sarif
  ```

- If validation fails on real output, fix the SARIF renderer in `crates/stratify-report/src/sarif.rs`, add a unit test that pins the fix, and say so in the report.
- Commit `test: validate generated SARIF against the official 2.1.0 schema`.

### Task 5: Profile and fix the large-repo scan (QA finding 4)

**Files:** wherever profiling points. The expected suspect is `crates/stratify-analysis/src/deadcode.rs:55-70`, a BFS that scans every reference for each popped node.

**Process:** use superpowers:systematic-debugging, and measure before changing code.

1. Clone kubernetes into a scratch directory and build release. Run `stratify check <k8s> --format json --stats` in the background. After 3 minutes, take a profile with `sample <pid> 10 -file <scratch>/profile-1.txt`. Record the top frames with self time. Stop the scan afterwards. Do not wait for it to finish.

   ```
   git clone --depth 1 --branch v1.31.0 https://github.com/kubernetes/kubernetes <scratch>/k8s
   ```

2. Before changing any code, record a baseline: self-scan JSON output and every fixture's JSON output, sorted, in a scratch directory.
3. Fix the dominant cost. If it is deadcode reachability, build a `from -> [reference index]` adjacency map once, before the BFS. Keep semantics identical: the same Certain-vs-any propagation, the same edge kinds.
4. Re-profile. If another stage now dominates and takes minutes (for example duplication's window map, `resolve::cross_file_calls`, or the per-call `enclosing_method_id` scans in adapters), fix the next dominant cost the same way. Stop when a full kubernetes scan completes in a reasonable time on this machine, or when the remaining cost is a deliberate design choice. In that case, record the reason.
5. Prove behavior is unchanged: the self-scan and every fixture's sorted JSON output must be byte-identical to the baseline from step 2. Add a unit test for the new adjacency logic.
6. Commit each fix separately as `perf(analysis): <what>`. Each commit message body names the measured before and after.
7. Write the profiling evidence (top frames before and after, with timings) into the task report, not into the repo.

### Task 6: Comparable benchmarks (QA finding 5)

**Files:** `scripts/bench.py`, `scripts/test_bench.py`, `docs/benchmarks.md`.

- `bench.py` gains `--repo-label` (free text, such as `kubernetes v1.31.0`).
- It records, in its JSON, `stratify_commit` from `git -C <stratify checkout> rev-parse --short HEAD` and `profile` from the binary path. `target/release/...` gives `release` and `target/debug/...` gives `debug`. Any other path gives `unknown`, overridable with `--profile`.
- Add `completed: true`.
- A non-zero exit already fails loudly. Keep it that way.
- `docs/benchmarks.md` gains columns `Build` and `Method` (`scripts/bench.py`). Each row ties to an exact commit.
- Keep the two existing rows as they are, labelled historical: the `b0ab79f` self-scan row and the kubernetes "not measured" row.
- Add new rows measured from a release build of the branch head after Task 5:
  - one self-scan row
  - one kubernetes v1.31.0 row, but only if the scan completed

  Fill every cell: date, commit, build, repo and revision, files, lines, wall, peak RSS, OS and hardware, method, completed. If kubernetes still does not complete, add a row stating the failure and how long it ran.
- Commit `docs: record release benchmarks at the branch head`.

### Task 7: Real judge-compatibility comparison (Phase 1 checklist)

**Files:** `scripts/judge_compat.py` (new), `docs/judge-compat.md` (new, the recorded result).

**Requirement:**
1. Feed `stratify-judge` a report that carries the evidence fields.
2. Feed it the same findings with `related`, `path` and `reasons` stripped.
3. Compare the per-finding verdicts.

The judge only judges `dead_code` today, so the comparison covers the findings it judges.

- `judge_compat.py --stratify <bin> --judge <bin> --root <repo> [--model <id>]`:
  1. Runs `stratify check <root> --format json`.
  2. Writes the full report, plus a stripped copy with the three keys removed from every finding, to a temp dir.
  3. Runs `stratify-judge --root <root> --input <file> --format json --no-cache --show-dismissed --min-confidence unknown` on each.
  4. Pairs findings by (rule, file, start_line, message).
  5. Compares each finding's verdict. Use the judgment fields the judge's JSON output carries; read its JSON output shape first.
  6. Prints a table and exits 0 only if every verdict matches.
- Model noise: a single mismatch can be model nondeterminism. If any finding differs, rerun the stripped variant once. Report a finding as "unstable" when the model disagrees with itself, and as "changed" only when the evidence run differs from both stripped runs.
- Run it once, for real, on this repo at the branch head. The judge's own repo uses `--backend jev`, and its README says Jev needs a model id. Read `~/dev/stratify-jev/README.md` for the default model id. Source the key in the same shell command: `set -a; source ~/.local_config; set +a; python3 scripts/judge_compat.py ...`. Never echo the key.
- Record the result in `docs/judge-compat.md`:
  - date, stratify commit, judge commit (`git -C ~/dev/stratify-jev rev-parse --short HEAD`), and model
  - findings compared, matches, changed, unstable
  - the exact command, with no secrets
- Commit `test: compare judge verdicts with and without evidence fields`.

### Task 8: Docs, status, and QA resolution (QA findings 3 and 5, sign-off)

**Files:**
- `docs/superpowers/plans/2026-10-01-roadmap-phase-0-1.md`: one note under Global Constraints
- `docs/roadmap-full-stack-static-analysis.md`
- `docs/superpowers/2026-10-02-roadmap-phase-0-1-qa.md`: append a `## Resolution` section only. Do not edit the QA text.
- `CHANGELOG.md`

Steps:
- Add this note to the phase 0-1 plan: "The branch was merged to `main` locally on 2026-10-02 at the owner's direction. Follow-up work happens on `roadmap-qa-fixes`." That resolves QA finding 3 without rewriting history.
- Update the roadmap's Phase 0 status, in evidence-based terms only:
  - the applicability policy
  - the precision footer from a real `precision.py` run, using the default clone URL if `main` is pushed, or else `--work` with a local clone, stated as such
  - the new benchmark rows
  - each remaining item, stated explicitly
- Update the roadmap's Phase 1 status: SARIF schema validation is in CI, and the judge-compatibility result is quoted from `docs/judge-compat.md`.
- Phase 0 stays "in progress" unless the evidence meets the global constraint above.
- Append the QA Resolution section: one line per QA finding and per checklist item, naming the commit or doc that resolves it, or saying plainly that it stays open and why.
- Commit `docs: record the QA resolution and evidence-based roadmap status`.
