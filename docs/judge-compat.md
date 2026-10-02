# Judge compatibility

Phase 1 exit test: `stratify-judge` gives the same verdicts on a report whether or not it carries `related`, `path` and `reasons`. This is a live run against the model, not a dry run.

- Date: 2026-10-02
- Stratify commits: runs 1 and 2 used 8e918f8 with the script files uncommitted. Run 3 used c6aea0b plus the uncommitted `--out` change to the script, which is committed in the commit that adds this section.
- Judge commit: 3a6e9f0
- Backend and model: jev, `jev-latest` (the judge README default)
- Scope: the judge judges `dead_code` findings only, so the comparison covers those.

## Command

```
set -a; source ~/.local_config >/dev/null 2>&1; set +a
python3 scripts/judge_compat.py --stratify target/release/stratify \
  --judge ~/dev/stratify-jev/target/release/stratify-judge \
  --root /Users/dynaum/dev/stratify --model jev-latest
```

The script runs the judge with `--no-cache --show-dismissed --min-confidence unknown`. If any verdict differs, it reruns the stripped variant once. A finding is unstable when the evidence run agrees with the rerun but not the first stripped run. It is changed when the evidence run differs from both stripped runs.

## Results

| Run | Compared | Matched | Changed | Unstable |
| --- | --- | --- | --- | --- |
| 1 | 72 | 69 | 1 | 2 |
| 2 | 72 | 71 | 0 | 1 |
| 3 | 74 | 71 | 2 | 1 |

Run 1 was piped through `tail`, so its non-matching rows were not kept. Only the counts are recorded. Run 2 kept the full table. Its one non-match:

- `crates/stratify-analysis/src/lib.rs:65` `file_sym`: evidence dismiss, first stripped keep, rerun dismiss. Unstable.

Run 3 wrote every row to `docs/judge-compat-run3.json`. Its non-matching rows:

- `crates/stratify-analysis/src/lib.rs:65` `file_sym`: evidence dismiss, first stripped keep, rerun dismiss. Unstable.
- `scripts/test_judge_compat.py:10` `finding`: evidence keep, stripped dismiss, rerun dismiss. Changed.
- `scripts/test_judge_compat.py:16` `judged`: evidence keep, stripped dismiss, rerun dismiss. Changed.

The script exits 1 when any row is unstable or unverified, not only when a row is changed. Runs 1 to 3 all exited 1 under the script's own criterion, so no live run passed it. Run 3 has 74 rows because the tree gained the test helpers between runs.

## Why verdicts cannot depend on the evidence fields

Checked in the judge repo at commit 3a6e9f0, under `crates/stratify-judge-core/src`:

- `Finding` has no typed evidence fields. `model.rs:49` declares `extra` with `#[serde(flatten)]`, so `related`, `path` and `reasons` deserialize into `extra`. `Report` does the same at `model.rs:57`.
- Outside tests, the one place the code touches `Finding.extra` is `verdict.rs:50`, which inserts `judgment`. The `contains_key("judgment")` filter in `driver.rs` sits inside `mod tests`, which starts at `driver.rs:485`.
- `judges/` and `context.rs` never read `extra`, so no model prompt carries these fields.

With and without the fields, the judge sends the same prompts.

## Request capture

The request bodies confirm the code reading directly. No real API call was made.

Method:

1. Build the full report with `target/release/stratify check . --format json`. Derive the stripped copy by removing `related`, `path` and `reasons` from every finding.
2. Run a local HTTP server on 127.0.0.1:18765. For each POST it writes the request body to a numbered file and returns HTTP 500.
3. Run the judge once per variant: `TYPESAFE_API_KEY=dummy-capture stratify-judge --root . --input <variant>.json --no-cache --model jev-latest --base-url http://127.0.0.1:18765 --format json`. The key is a dummy value.
4. Parse each captured body as JSON, dump it with sorted keys, and sort the resulting list. Compare the two variants.

Result (stratify release build at 197d25e, judge 3a6e9f0, 2026-10-02): each variant produced 32 request bodies, from 8 requests plus retries after the 500 responses. The two sorted sets are identical. Every request the judge sends is the same with or without the evidence fields. The run 3 changed rows are therefore model nondeterminism, not an effect of the evidence.

## Reading the result

Both variants send identical prompts, so any verdict difference is model noise. That covers the changed rows in runs 1 and 3. The changed rows in run 3 are the two new test helpers, where the stripped runs agreed with each other and the evidence run differed. That pattern is what noise looks like on borderline findings, and the identical prompts rule out the evidence fields as the cause. Run 2 had 0 changed.

Conclusion: the Phase 1 judge-compatibility test passes by code inspection and request capture. The three live runs show only model noise (run 3 had 2 changed rows), and all three exited 1 under the script's own criterion.
