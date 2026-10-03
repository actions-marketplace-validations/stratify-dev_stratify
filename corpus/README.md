# Labelled corpus

Real repositories at pinned commits, with Stratify findings marked true
(`tp`) or false (`fp`). `scripts/precision.py` scans each repo and prints
one row per rule and language.

    cargo build --release -p stratify-cli
    python3 scripts/precision.py --stratify target/release/stratify

## Files

- `applicability.json`: `not_applicable`, a list of rule and language pairs
  with a `reason` (the product reason) and `reviewed_by` (who agreed).
- `repos.json`: name, clone URL, pinned commit.
- `labels/<name>.json`: `repo`, `complete_rules` (rules labelled
  exhaustively in this repo), and `labels`, one per finding: `rule`, `file`,
  `line`, `match` (a substring of the message), `label`, `note`.

## Matching and validation

A label matches a finding when rule, file, and line are equal and `match`
is a substring of the message. The script validates every input before it
clones anything. It exits 2 and lists every problem when:

- a label is malformed, matches no finding (stale), matches several findings
  (ambiguous), or a finding matches several labels
- a finding of a `complete_rules` rule has no label
- `repos.json` is malformed, including duplicate or unsafe names
- `applicability.json` is malformed
- a repo has no labels file
- any of these files is not valid JSON or cannot be read

Exit 1 means only a clone, checkout, or scan failure.

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

## Applicability

A pair (rule, language) is applicable unless `applicability.json` lists it
with a `reason` and a `reviewed_by`. Today all 36 pairs are applicable, so
the file reads `{"not_applicable": []}`.

- A listed pair reports `not-applicable`.
- A listed pair that has corpus findings is a validation error. Findings
  prove the rule applies.
- The bar stays at 2 repos and 30 labelled findings. Nobody lowers it to
  make the Phase 0 exit pass.
- Synthetic fixtures never count as corpus evidence.

The footer reads `gate-ready: N of M applicable pairs (K not applicable).`

## Evidence plan for sparse rules

`cycle` and `boundary` produce few findings on ordinary repos. Plan their
evidence on purpose:

- `cycle`: pick corpus repos known to carry real import cycles, in each
  language.
- `boundary`: pick repos with a real layer config. That means a
  `stratify.toml`, a Rails app, or a Maven layered app.
- Label these rules exhaustively in those repos and list them in
  `complete_rules`.

## Coverage

| Repo | Language | Rules labelled exhaustively |
|------|----------|-----------------------------|
| stratify @ a809171 | rust | dead_code |

Gate-ready pairs: 0 of 36. Phase 0 needs all 36.
