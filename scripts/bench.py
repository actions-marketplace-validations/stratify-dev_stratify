#!/usr/bin/env python3
"""Measure one full `stratify check` run: wall time, peak memory, and the
files and lines Stratify itself reports scanning.

Usage: python3 scripts/bench.py --stratify BIN [--repo-label TEXT] [--profile P] REPO

The JSON also records the Stratify commit (from the checkout holding this
script), the build profile (from the binary path, or --profile), and
completed: true.

Prints one JSON object. Exits non-zero and prints the scan's stderr when the
scan exits non-zero or prints no stats line. Runs the same on macOS and Linux.
"""
import argparse
import json
import os
import resource
import subprocess
import sys
import time

STATS_PREFIX = "stratify-stats "


def parse_stats(stderr):
    for line in reversed(stderr.splitlines()):
        if line.startswith(STATS_PREFIX):
            return json.loads(line[len(STATS_PREFIX):])
    raise ValueError("no stratify-stats line on stderr")


def peak_rss_mb(ru_maxrss, platform):
    """ru_maxrss is in bytes on macOS and in KiB on Linux."""
    divisor = 1024 * 1024 if platform == "darwin" else 1024
    return round(ru_maxrss / divisor, 1)


def profile_from_path(path, override):
    if override:
        return override
    parts = os.path.normpath(path).split(os.sep)
    for name in ("release", "debug"):
        if "target" in parts and name in parts:
            return name
    return "unknown"


def stratify_commit():
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    out = subprocess.run(["git", "-C", root, "rev-parse", "--short", "HEAD"],
                         capture_output=True, text=True)
    return out.stdout.strip() if out.returncode == 0 else "unknown"


def build_result(stats, wall, rss_mb, commit, profile, repo_label):
    return {
        "stratify_commit": commit,
        "profile": profile,
        "repo": repo_label,
        "files": stats["files_scanned"],
        "lines": stats["lines_scanned"],
        "functions": stats["functions"],
        "wall_s": round(wall, 2),
        "peak_rss_mb": rss_mb,
        "completed": True,
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--stratify", required=True)
    parser.add_argument("--repo-label", default=None)
    parser.add_argument("--profile", default=None)
    parser.add_argument("repo")
    args = parser.parse_args()

    cmd = [args.stratify, "check", args.repo, "--format", "json", "--stats"]
    start = time.monotonic()
    proc = subprocess.run(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
    wall = time.monotonic() - start
    if proc.returncode != 0:
        sys.stderr.write(proc.stderr)
        sys.exit(f"bench: {' '.join(cmd)} exited {proc.returncode}")
    try:
        stats = parse_stats(proc.stderr)
    except ValueError as e:
        sys.stderr.write(proc.stderr)
        sys.exit(f"bench: {e}")
    rss = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
    result = build_result(
        stats, wall, peak_rss_mb(rss, sys.platform), stratify_commit(),
        profile_from_path(args.stratify, args.profile), args.repo_label or args.repo)
    print(json.dumps(result))


if __name__ == "__main__":
    main()
