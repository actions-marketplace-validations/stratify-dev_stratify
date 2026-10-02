#!/usr/bin/env python3
"""Measure Stratify precision per rule and language against the labelled corpus.

Usage: python3 scripts/precision.py --stratify target/release/stratify [--work DIR] [--json]

For each repo in corpus/repos.json: clone it into the work dir (once), check out
the pinned commit, scan it, and match findings to corpus/labels/<name>.json.
Precision counts labelled findings only; coverage shows how much of a rule and
language pair is labelled. Precision targets apply only to gate-ready pairs.

Exit codes: 0 valid corpus, 1 clone/checkout/scan failure, 2 label errors.
"""
import argparse
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EXTENSIONS = {
    "java": "java", "rb": "ruby", "ts": "typescript", "tsx": "typescript",
    "mts": "typescript", "cts": "typescript", "py": "python", "pyi": "python",
    "go": "go", "rs": "rust",
}
LANGUAGES = ["java", "ruby", "typescript", "python", "go", "rust"]
RULES = ["dead_code", "duplication", "complexity", "hotspot", "cycle", "boundary"]
LABEL_FIELDS = ("rule", "file", "line", "match", "label", "note")
MIN_REPOS = 2
MIN_LABELLED = 30


class CorpusError(Exception):
    """A clone, checkout, or scan failed."""


def language_of(path):
    ext = path.rsplit(".", 1)[-1] if "." in path else ""
    return EXTENSIONS.get(ext, "other")


def label_matches(label, finding):
    span = finding["span"]
    return (label["rule"] == finding["rule"] and label["file"] == span["file"]
            and label["line"] == span["start_line"] and label["match"] in finding["message"])


def validate_labels(doc):
    """Schema errors in one labels document, as strings."""
    errors = []
    complete = doc.get("complete_rules")
    if not isinstance(complete, list):
        errors.append("complete_rules must be a list")
    else:
        errors += [f"unknown rule in complete_rules: {r}" for r in complete if r not in RULES]
    for i, lab in enumerate(doc.get("labels", [])):
        missing = [k for k in LABEL_FIELDS if k not in lab]
        if missing:
            errors.append(f"label {i}: missing {', '.join(missing)}")
            continue
        if lab["rule"] not in RULES:
            errors.append(f"label {i}: unknown rule {lab['rule']}")
        if not isinstance(lab["line"], int):
            errors.append(f"label {i}: line must be an integer")
        if lab["label"] not in ("tp", "fp"):
            errors.append(f"label {i}: label must be tp or fp")
    return errors


def classify(findings, doc, repo):
    """Match findings to labels. Returns ([(rule, language, outcome)], errors)
    where outcome is tp, fp, or unlabelled."""
    labels = doc["labels"]
    hits = [0] * len(labels)
    outcomes, errors = [], []
    for f in findings:
        where = f"{f['span']['file']}:{f['span']['start_line']} {f['rule']}"
        matched = [i for i, lab in enumerate(labels) if label_matches(lab, f)]
        for i in matched:
            hits[i] += 1
        if len(matched) > 1:
            errors.append(f"{repo}: finding {where} matches {len(matched)} labels (duplicate)")
        if matched:
            outcome = labels[matched[0]]["label"]
        else:
            outcome = "unlabelled"
            if f["rule"] in doc["complete_rules"]:
                errors.append(f"{repo}: finding {where} has no label, but {f['rule']} is in complete_rules")
        outcomes.append((f["rule"], language_of(f["span"]["file"]), outcome))
    for i, n in enumerate(hits):
        where = f"label {i} ({labels[i]['file']}:{labels[i]['line']})"
        if n == 0:
            errors.append(f"{repo}: {where} matches no finding (stale)")
        elif n > 1:
            errors.append(f"{repo}: {where} matches {n} findings (ambiguous)")
    return outcomes, errors


def summarize(per_repo):
    """per_repo: {repo: [(rule, language, outcome)]}. One row per rule x language."""
    rows = []
    for rule in RULES:
        for lang in LANGUAGES:
            counts = {"tp": 0, "fp": 0, "unlabelled": 0}
            repos = set()
            for repo, outcomes in per_repo.items():
                for r, l, outcome in outcomes:
                    if r == rule and l == lang:
                        counts[outcome] += 1
                        if outcome != "unlabelled":
                            repos.add(repo)
            labelled = counts["tp"] + counts["fp"]
            total = labelled + counts["unlabelled"]
            if total == 0:
                status = "no-data"
            elif counts["unlabelled"]:
                status = "incomplete"
            elif len(repos) < MIN_REPOS or labelled < MIN_LABELLED:
                status = "insufficient"
            else:
                status = "gate-ready"
            rows.append({
                "rule": rule, "language": lang, "findings": total, **counts,
                "repos": len(repos),
                "coverage": round(labelled / total, 3) if total else None,
                "precision": round(counts["tp"] / labelled, 3) if labelled else None,
                "status": status,
            })
    return rows


def run(cmd):
    proc = subprocess.run(cmd, capture_output=True, text=True)
    if proc.returncode != 0:
        raise CorpusError(f"{' '.join(cmd)} exited {proc.returncode}: {proc.stderr.strip()}")
    return proc.stdout


def checkout(repo, work):
    dest = work / repo["name"]
    if not dest.exists():
        run(["git", "clone", "--quiet", repo["url"], str(dest)])
    run(["git", "-C", str(dest), "checkout", "--quiet", repo["commit"]])
    return dest


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--stratify", required=True)
    parser.add_argument("--work", default=str(ROOT / "target" / "corpus"))
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()

    work = Path(args.work)
    work.mkdir(parents=True, exist_ok=True)
    repos = json.loads((ROOT / "corpus" / "repos.json").read_text())
    per_repo, errors = {}, []
    try:
        for repo in repos:
            doc = json.loads((ROOT / "corpus" / "labels" / f"{repo['name']}.json").read_text())
            schema = validate_labels(doc)
            if schema:
                errors += [f"{repo['name']}: {e}" for e in schema]
                continue
            out = run([args.stratify, "check", str(checkout(repo, work)), "--format", "json"])
            outcomes, errs = classify(json.loads(out)["findings"], doc, repo["name"])
            per_repo[repo["name"]] = outcomes
            errors += errs
    except CorpusError as e:
        print(f"precision: {e}", file=sys.stderr)
        sys.exit(1)
    if errors:
        for e in errors:
            print(f"precision: {e}", file=sys.stderr)
        sys.exit(2)

    rows = summarize(per_repo)
    ready = sum(r["status"] == "gate-ready" for r in rows)
    if args.json:
        json.dump({"rows": rows, "gate_ready": ready, "pairs": len(rows)}, sys.stdout, indent=2)
        print()
        return
    print(f"{'rule':<12} {'language':<11} {'tp':>4} {'fp':>4} {'unlab':>6} {'repos':>5} {'coverage':>8} {'precision':>9}  status")
    for r in rows:
        cov = "n/a" if r["coverage"] is None else f"{r['coverage']:.0%}"
        prec = "n/a" if r["precision"] is None else f"{r['precision']:.0%}"
        print(f"{r['rule']:<12} {r['language']:<11} {r['tp']:>4} {r['fp']:>4} {r['unlabelled']:>6} "
              f"{r['repos']:>5} {cov:>8} {prec:>9}  {r['status']}")
    print(f"\ngate-ready: {ready} of {len(rows)} rule-language pairs. "
          "Precision targets apply only to gate-ready pairs.")


if __name__ == "__main__":
    main()
