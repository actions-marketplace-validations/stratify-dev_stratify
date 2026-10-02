#!/usr/bin/env python3
"""Compare stratify-judge verdicts with and without the evidence fields.

Runs `stratify check`, then judges the full report and a copy with
`related`, `path` and `reasons` stripped. Exits 0 only if every judged
finding keeps its verdict. Never prints environment or headers.
"""
import argparse
import copy
import json
import subprocess
import sys
import tempfile
from pathlib import Path

EVIDENCE_KEYS = ("related", "path", "reasons")


def strip_evidence(report):
    out = copy.deepcopy(report)
    for f in out.get("findings", []):
        for k in EVIDENCE_KEYS:
            f.pop(k, None)
    return out


def pair_key(f):
    span = f.get("span", {})
    return (f.get("rule"), span.get("file"), span.get("start_line"), f.get("message"))


def verdicts(report):
    """Map pair key to verdict for findings that carry a judgment."""
    out = {}
    for f in report.get("findings", []):
        j = f.get("judgment")
        if j is not None:
            out[pair_key(f)] = j.get("verdict")
    return out


def compare(a, b):
    """Return the keys judged in either report and those whose verdicts differ."""
    va, vb = verdicts(a), verdicts(b)
    compared = sorted(set(va) | set(vb), key=str)
    differs = {k: (va.get(k), vb.get(k)) for k in compared if va.get(k) != vb.get(k)}
    return {"compared": compared, "differs": differs}


def classify(evidence, stripped1, stripped2):
    """match, changed, unstable, or unverified (no rerun available)."""
    if evidence == stripped1:
        return "match"
    if stripped2 is None:
        return "unverified"
    if evidence == stripped2:
        return "unstable"
    return "changed"


def run_judge(judge, root, path, model):
    cmd = [judge, "--root", root, "--input", str(path), "--format", "json",
           "--no-cache", "--show-dismissed", "--min-confidence", "unknown"]
    if model:
        cmd += ["--model", model]
    p = subprocess.run(cmd, capture_output=True, text=True)
    if p.returncode != 0:
        sys.exit(f"judge failed (exit {p.returncode}): {p.stderr.strip()[-500:]}")
    return json.loads(p.stdout)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--stratify", required=True)
    ap.add_argument("--judge", required=True)
    ap.add_argument("--root", required=True)
    ap.add_argument("--model")
    args = ap.parse_args()

    p = subprocess.run([args.stratify, "check", args.root, "--format", "json"],
                       capture_output=True, text=True)
    try:
        report = json.loads(p.stdout)
    except json.JSONDecodeError:
        sys.exit(f"stratify check produced no JSON (exit {p.returncode})")

    with tempfile.TemporaryDirectory() as tmp:
        full, bare = Path(tmp) / "full.json", Path(tmp) / "stripped.json"
        full.write_text(json.dumps(report))
        bare.write_text(json.dumps(strip_evidence(report)))
        ev = run_judge(args.judge, args.root, full, args.model)
        s1 = run_judge(args.judge, args.root, bare, args.model)
        diff = compare(ev, s1)
        s2 = None
        if diff["differs"]:
            s2 = run_judge(args.judge, args.root, bare, args.model)

    vev, vs1 = verdicts(ev), verdicts(s1)
    vs2 = verdicts(s2) if s2 is not None else None
    keys = diff["compared"]
    counts = {"match": 0, "changed": 0, "unstable": 0, "unverified": 0}
    print(f"{'verdict':10} {'evidence':10} {'stripped':10} {'rerun':10} finding")
    for k in keys:
        c = classify(vev.get(k), vs1.get(k), vs2.get(k) if vs2 is not None else None)
        counts[c] += 1
        print(f"{c:10} {str(vev.get(k)):10} {str(vs1.get(k)):10} "
              f"{str(vs2.get(k) if vs2 is not None else '-'):10} {k[1]}:{k[2]} {k[3]}")
    print(f"compared={len(keys)} matched={counts['match']} changed={counts['changed']} "
          f"unstable={counts['unstable']} unverified={counts['unverified']}")
    return 0 if counts["match"] == len(keys) else 1


if __name__ == "__main__":
    sys.exit(main())
