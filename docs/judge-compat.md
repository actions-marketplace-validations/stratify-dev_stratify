# Judge compatibility

Phase 1 exit test: `stratify-judge` gives the same verdicts on a report whether or not it carries `related`, `path` and `reasons`. This is a live run against the model, not a dry run.

- Date: 2026-10-02
- Stratify commit: 8e918f8 (the two script files were untracked at run time)
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

Run 1 was piped through `tail`, so its non-matching rows were not kept. Only the counts are recorded. Run 2 kept the full table. Its one non-match:

- `crates/stratify-analysis/src/lib.rs:65` `file_sym`: evidence dismiss, first stripped keep, rerun dismiss. Unstable.

## Reading the result

Run 1 reported one changed finding, and no run kept its identity. Run 2 did not reproduce it. The data do not prove the evidence fields leave verdicts unchanged. They show 69 to 71 of 72 verdicts match, with the rest consistent with model noise. Treat the single changed finding in run 1 as unresolved until a rerun that keeps the table reproduces it or rules it out.
