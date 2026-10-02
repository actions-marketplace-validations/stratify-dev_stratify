# Benchmarks

Full `stratify check --format json` runs measured with `scripts/bench.py`.
Files and lines are what Stratify reports scanning (`--stats`), after
`.gitignore`, hidden-file skips, and `[ignore]` globs. The roadmap's
scan-time budget comes from the large-repo row.

| Date | Stratify | Build | Repo | Files | Lines | Wall (s) | Peak RSS (MB) | Machine | Method | Completed |
|------|----------|-------|------|-------|-------|----------|---------------|---------|--------|-----------|
| 2026-10-02 | 090da17 | release | stratify (self) | 50 | 11984 | 0.71 | 36.2 | Darwin arm64, Apple M5 | `scripts/bench.py` | yes |
| 2026-10-02 | 090da17 | release | kubernetes v1.31.0 | 15372 | 4520718 | 90.27 | 8186.5 | Darwin arm64, Apple M5 | `scripts/bench.py` | yes |
| 2026-10-01 (historical) | b0ab79f | not recorded | stratify (self) | 48 | 10896 | 0.34 | 34.5 | Darwin arm64, Apple M5 | not recorded | yes |
| 2026-10-01 (historical) | b0ab79f | not recorded | kubernetes v1.31.0 | not measured: scan did not finish within 30 min (stopped by the run's time limit) | | | | Darwin arm64, Apple M5 | not recorded | no |

The two `2026-10-02` rows come from a release build at commit `090da17`, after the dead-code BFS fix (`71b5005`). Peak RSS is `ru_maxrss` converted to MiB. The `b0ab79f` rows are historical and predate the build and method columns.

Historical note, kubernetes v1.31.0 at `b0ab79f` (15,365 Go files): the scan ran single-threaded at 100% CPU with 2.2 GB RSS at 20 minutes and did not finish. The suspected cause is the reachability loop in `crates/stratify-analysis/src/deadcode.rs`, which scans every reference for each visited node (O(nodes x references)). The cause is not confirmed.
