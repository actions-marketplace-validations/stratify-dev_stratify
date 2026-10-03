# Corpus Growth Plan

Date: 2026-10-03
Closes: gap 5 of [`../2026-10-02-followup-qa-gaps.md`](../2026-10-02-followup-qa-gaps.md)
Evidence rules: [`../../../corpus/README.md`](../../../corpus/README.md)

This document is a plan only. It clones nothing and labels nothing. It adds no pin. Every pin below reads "to be chosen at labelling time".

## Goal

Every applicable rule-language pair becomes gate-ready. A pair is gate-ready when:

- every finding of that rule in that language in the corpus carries a label,
- the labels come from 2 or more repos,
- 30 or more findings are labelled.

Today 0 of 36 pairs are gate-ready and `corpus/applicability.json` lists no inapplicable pair. The bar does not drop. Synthetic fixtures do not count.

## Candidate repositories

The six rules are `dead_code`, `duplication`, `complexity`, `hotspot`, `cycle`, and `boundary`. The table gives 3 candidates per language. Each language needs at least 2 repos in the end, and the third is a spare in case a repo yields too few findings of a rule.

Every pin is "to be chosen at labelling time". Pick a release tag, record its full commit SHA in `corpus/repos.json`, and verify the SHA exists in the clone.

| Language | Repo | URL | Why it suits | Sparse-rule role |
|----------|------|-----|--------------|------------------|
| Rust | tokio | https://github.com/tokio-rs/tokio | Large async runtime with many modules, so duplication, complexity, and hotspot produce volume | Cycle candidate: module-level cycles inside the crate |
| Rust | ripgrep | https://github.com/BurntSushi/ripgrep | Multi-crate workspace with real crate and module boundaries | Boundary candidate if a layer config fits the workspace |
| Rust | cargo | https://github.com/rust-lang/cargo | Very large code base with long history, so hotspot and complexity yield many findings | Cycle candidate |
| TypeScript | vscode | https://github.com/microsoft/vscode | Layered by design (`base`, `platform`, `editor`, `workbench`), with a documented layer rule | Boundary: real layer config. Cycle candidate |
| TypeScript | nest | https://github.com/nestjs/nest | Monorepo of packages with module structure | Cycle candidate, because it handles circular module imports in its own design |
| TypeScript | typeorm | https://github.com/typeorm/typeorm | Medium size with many internal imports, so dead code and duplication appear | Cycle candidate |
| Python | django | https://github.com/django/django | Mature code base with many modules and a long history | Cycle candidate: known import cycles resolved by local imports |
| Python | flask | https://github.com/pallets/flask | Small and idiomatic, so dead code and complexity results are reviewable by hand | Spare for sparse rules |
| Python | requests | https://github.com/psf/requests | Small library with stable structure | Spare for sparse rules |
| Go | kubernetes | https://github.com/kubernetes/kubernetes | Already the benchmark repo (v1.31.0, `9edcffc`). Huge, so every rule has volume | Boundary: staging layout and import rules. Cycle: none expected, since Go forbids import cycles |
| Go | prometheus | https://github.com/prometheus/prometheus | Mid-size service with clear package boundaries | Boundary candidate |
| Go | hugo | https://github.com/gohugoio/hugo | Mid-size application with many packages | Boundary candidate |
| Java | spring-petclinic | https://github.com/spring-projects/spring-petclinic | Small layered Spring app (controller, service, repository) | Boundary: real layered app |
| Java | guava | https://github.com/google/guava | Large library, so duplication and complexity yield volume | Cycle candidate: package-level cycles |
| Java | kafka | https://github.com/apache/kafka | Large multi-module Maven and Gradle code base with long history | Cycle and boundary candidate |
| Ruby | rails | https://github.com/rails/rails | Large library family with many modules | Cycle candidate |
| Ruby | discourse | https://github.com/discourse/discourse | Rails application with real app layers (models, controllers, services) | Boundary: real Rails app |
| Ruby | mastodon | https://github.com/mastodon/mastodon | Rails application with services and workers | Boundary: real Rails app |

Notes on sparse rules:

- Go has no import cycles at the package level. Plan the Go `cycle` pair as a likely `not-applicable` candidate only if scans of all three repos find none and the owner reviews the reason. A listed pair with corpus findings is a validation error, so scan first.
- `boundary` needs a real layer config. Use repos that ship one, or repos whose structure maps to a Stratify config the reviewers agree on. Record the config in the repo's labels `note` fields.
- List `cycle` and `boundary` in `complete_rules` for the repos chosen above, and label every finding.

## Labelling workflow

Run these steps for each repo.

1. Scan. Pin the commit, build `stratify` in release mode, and run it against the clone. Save the findings.
2. Label every finding of a rule in that repo. Add the rule to `complete_rules` only after all of its findings carry a label. Do not label a sample and call the rule complete.
3. Cite evidence in `note`. A `tp` names the reason a maintainer would act. An `fp` names the call site or config that proves the finding wrong.
4. A second reviewer spot-checks a sample. Draw at least 10 percent of the labels per pair, with a floor of 10 labels. Settle each disagreement in `note`, and recheck the whole rule if the reviewers disagree on more than 1 in 10.
5. Run `scripts/precision.py` from a clean clone after `main` is pushed. The pins must be reachable from the default remote branch. Expect exit 0 and a gate-ready count that matches the plan.

Work one language at a time so the table moves pair by pair.

## Effort estimate

The estimate counts labelled findings. It assumes about 2 to 5 minutes per finding, with sparse rules at the slow end.

- Per pair: 30 labels minimum. Plan for 40 to 80 in order to leave margin when a repo yields few findings.
- Per language with six pairs: 240 to 480 labels.
- Whole corpus of 36 pairs: roughly 1,400 to 2,900 labelled findings, minus any pair the owner approves as not applicable.
- Time: roughly 70 to 240 hours of labelling and review at the rate above.

Volume rules (`dead_code`, `duplication`, `complexity`, `hotspot`) can produce more than 80 findings in a large repo. Labelling every finding is required for a `complete_rules` entry. If a repo yields thousands of findings of one rule, choose a smaller repo or a smaller pinned snapshot for that rule. Do not label a subset and claim completeness.

## Exit

The 36-pair table in `scripts/precision.py` output reaches gate-ready. That means:

- the footer reads `gate-ready: N of N applicable pairs`, with N equal to 36 minus the reviewed not-applicable pairs,
- every not-applicable pair has a `reason` and a `reviewed_by` in `corpus/applicability.json`,
- the run happens from a clean clone with `main` pushed.

Phase 0 closes when this exit holds. The scan-time budget is already set.
