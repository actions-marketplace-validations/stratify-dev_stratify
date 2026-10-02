#!/usr/bin/env python3
"""Measure one full `stratify check` run: wall time, peak memory, and the
files and lines Stratify itself reports scanning.

Usage: python3 scripts/bench.py --stratify BIN REPO

Prints one JSON object. Exits non-zero and prints the scan's stderr when the
scan exits non-zero or prints no stats line. Runs the same on macOS and Linux.
"""
import argparse
import json
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


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--stratify", required=True)
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
    print(json.dumps({
        "files": stats["files_scanned"],
        "lines": stats["lines_scanned"],
        "functions": stats["functions"],
        "wall_s": round(wall, 2),
        "peak_rss_mb": peak_rss_mb(rss, sys.platform),
    }))


if __name__ == "__main__":
    main()
