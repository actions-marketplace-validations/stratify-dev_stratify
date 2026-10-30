import json
import os
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
BINARY = os.path.join(ROOT, "target", "debug", "stratify")
TESTS = os.path.join(ROOT, "crates", "stratify-cli", "tests")
# sample-complex yields a complexity finding, sample-suppress carries
# stratify-ignore markers (the renderer drops suppressed findings).
FIXTURES = [
    "sample-rustcycle",
    "sample-dup",
    "sample-boundary",
    "sample-java",
    "sample-complex",
    "sample-suppress",
]

try:
    import jsonschema  # noqa: F401
    HAVE_JSONSCHEMA = True
except ImportError:
    HAVE_JSONSCHEMA = False

SKIP_JSONSCHEMA = "skipped: jsonschema is not installed (pip install jsonschema)"
SKIP_BINARY = "skipped: build the debug binary first (cargo build -p stratify-cli)"


class ValidateSarifTest(unittest.TestCase):
    def run_validator(self, *paths):
        return subprocess.run(
            [sys.executable, os.path.join(HERE, "validate_sarif.py"), *paths],
            capture_output=True,
            text=True,
        )

    @unittest.skipUnless(HAVE_JSONSCHEMA, SKIP_JSONSCHEMA)
    def test_rejects_invalid_sarif(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "bad.sarif")
            with open(path, "w") as f:
                json.dump({"version": "2.1.0", "runs": [{"results": "nope"}]}, f)
            proc = self.run_validator(path)
        self.assertEqual(proc.returncode, 1, proc.stdout + proc.stderr)
        self.assertIn("runs", proc.stderr)

    @unittest.skipUnless(HAVE_JSONSCHEMA, SKIP_JSONSCHEMA)
    @unittest.skipUnless(os.path.exists(BINARY), SKIP_BINARY)
    def test_generated_sarif_matches_schema(self):
        with tempfile.TemporaryDirectory() as tmp:
            for name in FIXTURES:
                with self.subTest(fixture=name):
                    out = subprocess.run(
                        [BINARY, "check", os.path.join(TESTS, name), "--format", "sarif"],
                        capture_output=True,
                        text=True,
                    )
                    path = os.path.join(tmp, name + ".sarif")
                    with open(path, "w") as f:
                        f.write(out.stdout)
                    proc = self.run_validator(path)
                    self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
                    self.assertIn("ok " + path, proc.stdout)


if __name__ == "__main__":
    unittest.main()
