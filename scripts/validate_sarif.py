#!/usr/bin/env python3
"""Validate SARIF files against the official OASIS SARIF 2.1.0 JSON schema.

Usage: validate_sarif.py <file>...

Exits 1 with the failing JSON path on the first error, 2 if jsonschema is missing.
"""
# Schema source: https://raw.githubusercontent.com/oasis-tcs/sarif-spec/main/sarif-2.1/schema/sarif-schema-2.1.0.json
# Downloaded: 2026-10-02
import json
import os
import sys

SCHEMA = os.path.join(os.path.dirname(os.path.abspath(__file__)), "schemas", "sarif-schema-2.1.0.json")


def json_path(error):
    path = "$"
    for part in error.absolute_path:
        path += f"[{part}]" if isinstance(part, int) else f".{part}"
    return path


def main(files):
    try:
        import jsonschema
    except ImportError:
        print("jsonschema is missing. Install it with: pip install jsonschema", file=sys.stderr)
        return 2
    if not files:
        print("usage: validate_sarif.py <file>...", file=sys.stderr)
        return 2
    with open(SCHEMA) as f:
        schema = json.load(f)
    validator = jsonschema.validators.validator_for(schema)(schema)
    for path in files:
        try:
            with open(path) as f:
                doc = json.load(f)
        except (OSError, ValueError) as e:
            print(f"{path}: cannot read JSON: {e}", file=sys.stderr)
            return 1
        error = next(iter(validator.iter_errors(doc)), None)
        if error is not None:
            print(f"{path}: {json_path(error)}: {error.message}", file=sys.stderr)
            return 1
        print(f"ok {path}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
