# Labelled corpus

Real repositories at pinned commits, with Stratify findings marked true
(`tp`) or false (`fp`). `scripts/precision.py` scans each repo and prints
one row per rule and language.

    cargo build --release -p stratify-cli
    python3 scripts/precision.py --stratify target/release/stratify

## Files

- `repos.json`: name, clone URL, pinned commit.
- `labels/<name>.json`: `repo`, `complete_rules` (rules labelled
  exhaustively in this repo), and `labels`, one per finding: `rule`, `file`,
  `line`, `match` (a substring of the message), `label`, `note`.

## Matching and validation

A label matches a finding when rule, file, and line are equal and `match`
is a substring of the message. The script exits 2 and lists every problem
when a label is malformed, matches no finding (stale), matches several
findings (ambiguous), when a finding matches several labels, or when a
finding of a `complete_rules` rule has no label.

## Rules for labelling

- `tp` means a maintainer would act on the finding.
- `fp` means the finding is wrong. Say why in `note`, with the call site or
  config that proves it.
- A label is tied to a pinned commit. Moving the commit means relabelling.

## When a precision number counts

Precision counts labelled findings only. A rule and language pair is
`gate-ready` when every finding of that rule in that language is labelled,
the labels come from 2 or more repos, and 30 or more findings are labelled.
The roadmap's precision targets apply only to gate-ready pairs. Every other
pair reports `no-data`, `incomplete`, or `insufficient`, and its precision
number is a sample, not a measurement.

## Coverage

| Repo | Language | Rules labelled exhaustively |
|------|----------|-----------------------------|
| stratify @ a809171 | rust | dead_code |

Gate-ready pairs: 0 of 36. Phase 0 needs all 36.
