# Follow-up QA Gaps: Roadmap Phase 0 and Phase 1

Date: 2026-10-02  
Related review: [`2026-10-02-roadmap-phase-0-1-qa.md`](2026-10-02-roadmap-phase-0-1-qa.md)  
Roadmap: [`../roadmap-full-stack-static-analysis.md`](../roadmap-full-stack-static-analysis.md)

## Status

The follow-up work resolves the previous scan-statistics mismatch and large-repository performance blocker. The Phase 0 benchmark is now recorded, and the dead-code optimization has output-equivalence evidence. Phase 0 remains open because precision coverage is incomplete and the scan-time budget has not been set.

This note records the remaining QA gaps. It does not replace the implementation plan or claim release sign-off.

## Gaps to close

### 1. Precision label schema validation is not strict enough

[`scripts/precision.py`](../../scripts/precision.py) checks for required label keys, but does not validate all field types or reject an empty `match` string. For example, a label with an integer `match` passes `validate_labels()` and then raises `TypeError` in `label_matches()` during classification. This bypasses the intended clear validation diagnostic and can produce a traceback.

**Required work:**

- Validate every label field before classification: `rule`, `file`, `match`, `label`, and `note` must be non-empty strings; `line` must be a positive integer and must not accept booleans as integers.
- Validate each `complete_rules` entry as a string and a known rule id.
- Validate repo manifest fields and applicability entry field types before using them.
- Ensure malformed JSON/schema input produces the documented non-zero status and a concise error, not a traceback.
- Add tests for wrong field types, empty `match`/`note`, boolean or non-positive line numbers, malformed repo entries, and malformed applicability values.

### 2. The QA review document mixes the initial review with resolved facts

The opening verdict and the original performance finding in [`2026-10-02-roadmap-phase-0-1-qa.md`](2026-10-02-roadmap-phase-0-1-qa.md) say the large-repo benchmark has not completed. Its later resolution section records the completed Kubernetes run and the performance fix. The two statements refer to different points in time, but the document does not label the first section clearly as the initial snapshot.

**Required work:**

- Label the initial findings and verification as the state at the time of the first review; or update the top-level verdict to current status and retain the original findings under a dated “Initial review” section.
- Make the current status prominent: the large-repo benchmark and performance fix are complete; precision and scan-time budget remain open; Phase 1 hosted CI is pending.

### 3. The roadmap describes the remote branch inaccurately

The current local branch is `main`, tracking `origin/main` and ahead by 50 commits. The roadmap says “The branch has no remote,” which does not match the Git state. The operational issue is that the new commits have not been pushed, so hosted CI has not run and the local corpus pin is not reachable from a clean clone of the default remote branch.

**Required work:**

- Replace “the branch has no remote” with precise language stating that local `main` tracks `origin/main`, is ahead, and has not been pushed.
- Keep the corpus pin limitation and pending CI status explicit until the commits are pushed and the default clone can fetch the pinned revision.
- Do not push or rewrite history as part of this documentation fix unless separately authorized.

### 4. Hosted CI has not verified the configured checks

The latest repository status is `main` ahead of `origin/main`; no hosted CI run is available for these commits. Local formatting, Rust tests, Clippy, Python tests, and the Action wrapper pass. In the latest local Python run, the two SARIF schema tests were skipped because the `jsonschema` dependency was not installed. The CI workflow now installs Python and configures schema validation, but the configured step is not equivalent to a passing CI run.

**Required work:**

- Push the agreed branch through the normal repository workflow.
- Confirm the hosted CI run passes Rust formatting, Clippy, locked tests, SARIF schema validation, script tests, and dogfood.
- Update the roadmap and QA status with the CI run result. Until then, describe the checks as configured or passing locally, not as verified by CI.

### 5. Phase 0 precision evidence remains insufficient

The applicability policy is explicit and currently lists no inapplicable pairs, so all 36 rule-language pairs remain in scope. The corpus currently has one repository and labels only Rust `dead_code`; zero pairs are gate-ready. The minimum evidence bar remains two repositories and 30 fully labelled findings per applicable pair.

**Required work:**

- Add real, pinned repositories that cover every applicable pair. Follow the sparse-rule evidence plan for cycles and boundaries.
- Fully label each pair before claiming its precision. Keep `no-data`, `incomplete`, and `insufficient` visible; they do not pass the precision gate.
- Validate the complete corpus from a clean checkout, including clone, checkout, scan, label schema, stale-label, duplicate-label, applicability, and coverage failure paths.
- Keep Phase 0 open until every applicable pair is gate-ready and all approved not-applicable entries carry reviewed reasons.

### 6. The Phase 0 scan-time budget is still unset

The Kubernetes v1.31.0 release scan now completes in 90.27 seconds with 8,186.5 MiB peak RSS on the recorded Apple M5 machine. The roadmap assigns the actual budget to the owner and currently sets no value.

**Required work:**

- Have the owner set an explicit scan-time and memory budget using the recorded benchmark and expected hardware variability.
- Record the chosen threshold and the measurement environment in the roadmap.
- Keep Phase 0 open until the budget is set.

## Verification from this QA round

Passed locally:

- `~/.cargo/bin/cargo fmt --all -- --check`
- `~/.cargo/bin/cargo test --locked --workspace`
- `~/.cargo/bin/cargo clippy --locked --workspace --all-targets -- -D warnings`
- Python script tests: 41 passed, 2 skipped because `jsonschema` was not installed
- `bash scripts/test_action_run.sh`

The label-validation gap was reproduced directly: a numeric `match` field passes `validate_labels()` and raises `TypeError` during classification. The current branch is `main`, tracking `origin/main`, ahead by 50 commits. The checkout was clean after the verification commands.

## Sign-off

Do not claim full Phase 0 completion until corpus precision criteria, the scan-time budget, and the completed large-repository benchmark are all satisfied. Do not claim hosted-CI verification until the branch is pushed and the configured CI job passes. Fix and test strict corpus-label validation before relying on the precision tool as a gate.

## Resolution (2026-10-03)

- [x] 1. Label schema validation: `0a64a92` and `ddcb203`. All corpus inputs validate before any clone. Malformed input of any kind, including unparseable JSON, exits 2 with a concise error. Exit 1 covers only clone, checkout, and scan failures.
- [x] 2. QA review status: `docs/superpowers/2026-10-02-roadmap-phase-0-1-qa.md` has a current-status section and labels the original sections as the initial review.
- [x] 3. Remote wording: `docs/roadmap-full-stack-static-analysis.md` now says local `main` tracks `origin/main`, is ahead, and is not pushed.
- [ ] 4. Hosted CI: open. The owner decided that nothing is pushed in this round. The roadmap keeps the wording "configured and passing locally, hosted run pending" and notes the 2 SARIF schema tests skip locally without `jsonschema`.
- [ ] 5. Precision evidence: open. The owner decided to plan the corpus growth now and label later. See `docs/superpowers/plans/2026-10-03-corpus-growth.md`. 0 of 36 pairs are gate-ready.
- [x] 6. Scan-time budget: set by the owner at 180 s wall and 16 GiB peak RSS, recorded in the roadmap Phase 0 section. The reference run (90.27 s, 8,186.5 MiB at `090da17`) meets it.
