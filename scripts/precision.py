#!/usr/bin/env python3
"""Measure Stratify precision per rule and language against the labelled corpus.

Usage: python3 scripts/precision.py --stratify target/release/stratify [--work DIR] [--corpus DIR] [--json]

For each repo in <corpus>/repos.json: clone it into the work dir (once), check out
the pinned commit, scan it, and match findings to <corpus>/labels/<name>.json.
Precision counts labelled findings only; coverage shows how much of a rule and
language pair is labelled. Precision targets apply only to gate-ready pairs.
Pairs listed in <corpus>/applicability.json report not-applicable.

Exit codes: 0 valid corpus, 2 invalid corpus input (malformed or unreadable files,
label errors), 1 clone/checkout/scan failure only (including malformed scan output). Every input is validated
before any clone.
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


def non_empty(value):
    return isinstance(value, str) and bool(value.strip())


def validate_labels(doc):
    """Schema errors in one labels document, as strings."""
    if not isinstance(doc, dict):
        return ["labels document must be an object"]
    errors = []
    complete = doc.get("complete_rules")
    if not isinstance(complete, list):
        errors.append("complete_rules must be a list")
    else:
        seen = set()
        for r in complete:
            if not isinstance(r, str):
                errors.append(f"complete_rules entry {r!r} must be a string")
            elif r not in RULES:
                errors.append(f"unknown rule in complete_rules: {r}")
            elif r in seen:
                errors.append(f"duplicate rule in complete_rules: {r}")
            else:
                seen.add(r)
    labels = doc.get("labels")
    if not isinstance(labels, list):
        errors.append("labels must be a list")
        labels = []
    for i, lab in enumerate(labels):
        if not isinstance(lab, dict):
            errors.append(f"label {i}: must be an object")
            continue
        missing = [k for k in LABEL_FIELDS if k not in lab]
        if missing:
            errors.append(f"label {i}: missing {', '.join(missing)}")
            continue
        for key in ("rule", "file", "match", "label", "note"):
            if not non_empty(lab[key]):
                errors.append(f"label {i}: {key} must be a non-empty string")
        if non_empty(lab["rule"]) and lab["rule"] not in RULES:
            errors.append(f"label {i}: unknown rule {lab['rule']}")
        line = lab["line"]
        if not isinstance(line, int) or isinstance(line, bool) or line < 1:
            errors.append(f"label {i}: line must be an integer of at least 1")
        if non_empty(lab["label"]) and lab["label"] not in ("tp", "fp"):
            errors.append(f"label {i}: label must be tp or fp")
    return errors


def validate_manifest(doc):
    """Schema errors in repos.json, as strings."""
    if not isinstance(doc, list):
        return ["repos.json must be a list"]
    errors, names = [], set()
    for i, entry in enumerate(doc):
        if not isinstance(entry, dict):
            errors.append(f"repo entry {i}: must be an object")
            continue
        for key in ("name", "url", "commit"):
            if not non_empty(entry.get(key)):
                errors.append(f"repo entry {i}: {key} must be a non-empty string")
        for key in ("url", "commit"):
            value = entry.get(key)
            if non_empty(value) and value.startswith("-"):
                errors.append(f"repo entry {i}: {key} must not start with '-'")
        name = entry.get("name")
        if non_empty(name):
            if "/" in name or "\\" in name or name.startswith(".") or name != name.strip():
                errors.append(f"repo entry {i}: name {name!r} is not safe as a single path component")
            elif name.casefold() in names:
                errors.append(f"repo entry {i}: duplicate name {name}")
            names.add(name.casefold())
    return errors


def validate_applicability(doc):
    """Returns (errors, {(rule, language)}) for an applicability document."""
    if not isinstance(doc, dict) or not isinstance(doc.get("not_applicable"), list):
        return ["applicability.json must be an object with a not_applicable list"], set()
    errors, pairs = [], set()
    for i, entry in enumerate(doc["not_applicable"]):
        if not isinstance(entry, dict):
            errors.append(f"applicability entry {i}: must be an object")
            continue
        rule, lang = entry.get("rule"), entry.get("language")
        ok = True
        if not isinstance(rule, str) or rule not in RULES:
            errors.append(f"applicability entry {i}: unknown rule {rule}")
            ok = False
        if not isinstance(lang, str) or lang not in LANGUAGES:
            errors.append(f"applicability entry {i}: unknown language {lang}")
            ok = False
        for key in ("reason", "reviewed_by"):
            value = entry.get(key)
            if not non_empty(value):
                errors.append(f"applicability entry {i}: {key} must be a non-empty string")
        if ok:
            if (rule, lang) in pairs:
                errors.append(f"applicability entry {i}: duplicate pair {rule} {lang}")
            pairs.add((rule, lang))
    return errors, pairs


def not_applicable_findings(pairs, per_repo):
    """A listed pair with corpus findings is an error: findings prove the rule applies."""
    errors = []
    for repo, outcomes in per_repo.items():
        for rule, lang in sorted(pairs):
            n = sum(1 for r, l, _ in outcomes if r == rule and l == lang)
            if n:
                errors.append(f"{repo}: {n} finding(s) for {rule} {lang}, which applicability.json lists as not applicable")
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


def summarize(per_repo, not_applicable=frozenset()):
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
            if (rule, lang) in not_applicable:
                status = "not-applicable"
            elif total == 0:
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
        run(["git", "clone", "--quiet", "--", repo["url"], str(dest)])
    run(["git", "-C", str(dest), "checkout", "--quiet", repo["commit"], "--"])
    return dest


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--stratify", required=True)
    parser.add_argument("--work", default=str(ROOT / "target" / "corpus"))
    parser.add_argument("--corpus", default=str(ROOT / "corpus"))
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()

    work, corpus = Path(args.work), Path(args.corpus)
    work.mkdir(parents=True, exist_ok=True)
    errors, na_pairs, docs, repos = [], set(), {}, []

    def load(path):
        try:
            return json.loads(path.read_text()), True
        except (OSError, ValueError, RecursionError) as e:
            errors.append(f"{path}: cannot read: {e}")
            return None, False

    doc, ok = load(corpus / "applicability.json")
    if ok:
        na_errors, na_pairs = validate_applicability(doc)
        errors += na_errors
    manifest, ok = load(corpus / "repos.json")
    if ok:
        manifest_errors = validate_manifest(manifest)
        errors += manifest_errors
        if not manifest_errors:
            repos = manifest
            for repo in repos:
                path = corpus / "labels" / f"{repo['name']}.json"
                if not path.is_file():
                    errors.append(f"{repo['name']}: labels file {path} does not exist")
                    continue
                doc, ok = load(path)
                if not ok:
                    continue
                schema = validate_labels(doc)
                errors += [f"{repo['name']}: {e}" for e in schema]
                if not schema:
                    docs[repo["name"]] = doc
    if errors:
        for e in errors:
            print(f"precision: {e}", file=sys.stderr)
        sys.exit(2)

    per_repo = {}
    try:
        for repo in repos:
            out = run([args.stratify, "check", str(checkout(repo, work)), "--format", "json"])
            try:
                findings = json.loads(out)["findings"]
            except (ValueError, KeyError, TypeError) as e:
                raise CorpusError(f"{repo['name']}: scan output is not a valid report: {e}")
            try:
                outcomes, errs = classify(findings, docs[repo["name"]], repo["name"])
            except (KeyError, TypeError) as e:
                raise CorpusError(f"{repo['name']}: scan output has a malformed finding: {e!r}")
            per_repo[repo["name"]] = outcomes
            errors += errs
    except CorpusError as e:
        errors.append(str(e))
        for msg in errors:
            print(f"precision: {msg}", file=sys.stderr)
        sys.exit(1)
    errors += not_applicable_findings(na_pairs, per_repo)
    if errors:
        for e in errors:
            print(f"precision: {e}", file=sys.stderr)
        sys.exit(2)

    rows = summarize(per_repo, na_pairs)
    ready = sum(r["status"] == "gate-ready" for r in rows)
    applicable = len(rows) - len(na_pairs)
    if args.json:
        json.dump({"rows": rows, "gate_ready": ready, "pairs": len(rows),
                   "applicable": applicable, "not_applicable": len(na_pairs)}, sys.stdout, indent=2)
        print()
        return
    print(f"{'rule':<12} {'language':<11} {'tp':>4} {'fp':>4} {'unlab':>6} {'repos':>5} {'coverage':>8} {'precision':>9}  status")
    for r in rows:
        cov = "n/a" if r["coverage"] is None else f"{r['coverage']:.0%}"
        prec = "n/a" if r["precision"] is None else f"{r['precision']:.0%}"
        print(f"{r['rule']:<12} {r['language']:<11} {r['tp']:>4} {r['fp']:>4} {r['unlabelled']:>6} "
              f"{r['repos']:>5} {cov:>8} {prec:>9}  {r['status']}")
    print(f"\ngate-ready: {ready} of {applicable} applicable pairs ({len(na_pairs)} not applicable). "
          "Precision targets apply only to gate-ready pairs.")


if __name__ == "__main__":
    main()
