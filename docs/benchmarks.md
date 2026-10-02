# Benchmarks

Full `stratify check --format json` runs measured with `scripts/bench.py`.
Files and lines are what Stratify reports scanning (`--stats`), after
`.gitignore`, hidden-file skips, and `[ignore]` globs. The roadmap's
scan-time budget comes from the large-repo row.

| Date | Stratify | Repo | Files | Lines | Wall (s) | Peak RSS (MB) | Machine |
|------|----------|------|-------|-------|----------|---------------|---------|
| 2026-10-01 | b0ab79f | stratify (self) | 48 | 10896 | 0.34 | 34.5 | Darwin arm64, Apple M5 |
| 2026-10-01 | b0ab79f | kubernetes v1.31.0 | not measured: scan did not finish within 30 min (stopped by the run's time limit) | | | | Darwin arm64, Apple M5 |

Kubernetes v1.31.0 (15,365 Go files): the scan ran single-threaded at 100% CPU with 2.2 GB RSS at 20 minutes and did not finish. The suspected cause is the reachability loop in `crates/stratify-analysis/src/deadcode.rs`, which scans every reference for each visited node (O(nodes x references)). The cause is not confirmed.
