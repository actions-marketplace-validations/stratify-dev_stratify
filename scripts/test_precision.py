import json
import os
import stat
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from precision import (
    classify, label_matches, language_of, not_applicable_findings, summarize,
    validate_applicability, validate_labels, validate_manifest,
)

PRECISION = Path(__file__).resolve().parent / "precision.py"


def finding(rule, file, line, message):
    return {"rule": rule, "message": message, "span": {"file": file, "start_line": line}}


def label(rule, file, line, match, value="fp"):
    return {"rule": rule, "file": file, "line": line, "match": match, "label": value, "note": "n"}


class LanguageAndMatching(unittest.TestCase):
    def test_language_of(self):
        self.assertEqual(language_of("a/b.rs"), "rust")
        self.assertEqual(language_of("x.tsx"), "typescript")
        self.assertEqual(language_of("README.md"), "other")

    def test_match_requires_rule_file_line_and_substring(self):
        lab = label("dead_code", "a.rs", 3, "foo")
        self.assertTrue(label_matches(lab, finding("dead_code", "a.rs", 3, "unused function `foo`")))
        self.assertFalse(label_matches(lab, finding("dead_code", "a.rs", 4, "unused function `foo`")))
        self.assertFalse(label_matches(lab, finding("cycle", "a.rs", 3, "foo")))
        self.assertFalse(label_matches(lab, finding("dead_code", "a.rs", 3, "unused function `bar`")))


class Validation(unittest.TestCase):
    def test_schema_errors(self):
        doc = {"complete_rules": ["nope"], "labels": [
            {"rule": "dead_code", "file": "a.rs", "line": "3", "match": "x", "label": "maybe", "note": ""},
            {"rule": "dead_code"},
        ]}
        errors = validate_labels(doc)
        self.assertTrue(any("unknown rule in complete_rules" in e for e in errors), errors)
        self.assertTrue(any("line must be an integer" in e for e in errors), errors)
        self.assertTrue(any("label must be tp or fp" in e for e in errors), errors)
        self.assertTrue(any("missing" in e for e in errors), errors)

    def test_stale_ambiguous_duplicate_and_incomplete(self):
        doc = {"complete_rules": ["dead_code"], "labels": [
            label("dead_code", "a.rs", 1, "x"),
            label("dead_code", "a.rs", 1, "x"),        # duplicate of the first
            label("dead_code", "a.rs", 9, "gone"),     # stale
            label("complexity", "b.rs", 2, ""),        # ambiguous: matches two findings
        ]}
        findings = [
            finding("dead_code", "a.rs", 1, "x"),
            finding("dead_code", "a.rs", 5, "unlabelled"),  # incomplete: rule is complete
            finding("complexity", "b.rs", 2, "one"),
            finding("complexity", "b.rs", 2, "two"),
        ]
        _, errors = classify(findings, doc, "r")
        text = "\n".join(errors)
        self.assertIn("matches 2 labels", text)
        self.assertIn("stale", text)
        self.assertIn("ambiguous", text)
        self.assertIn("complete_rules", text)

    def test_partial_rules_allow_unlabelled(self):
        doc = {"complete_rules": [], "labels": [label("cycle", "a.rs", 1, "x", "tp")]}
        outcomes, errors = classify([finding("cycle", "a.rs", 1, "x"), finding("cycle", "a.rs", 2, "y")], doc, "r")
        self.assertEqual(errors, [])
        self.assertEqual(sorted(o for _, _, o in outcomes), ["tp", "unlabelled"])


class SchemaHardening(unittest.TestCase):
    def test_missing_or_non_list_labels(self):
        self.assertTrue(any("labels must be a list" in e for e in validate_labels({"complete_rules": []})))
        self.assertTrue(any("labels must be a list" in e for e in validate_labels({"complete_rules": [], "labels": {}})))

    def test_non_object_label(self):
        errors = validate_labels({"complete_rules": [], "labels": ["x", 3, None]})
        self.assertEqual(len([e for e in errors if "must be an object" in e]), 3, errors)

    def test_non_object_document(self):
        self.assertTrue(validate_labels([]))


class StrictLabels(unittest.TestCase):
    def doc(self, **kw):
        return {"complete_rules": [], "labels": [{**label("dead_code", "a.rs", 1, "x"), **kw}]}

    def test_valid_label_passes(self):
        self.assertEqual(validate_labels(self.doc()), [])

    def test_wrong_field_types(self):
        for key, bad in [("match", 5), ("rule", 5), ("file", ["a"]), ("note", None), ("label", 1), ("match", None)]:
            errors = validate_labels(self.doc(**{key: bad}))
            self.assertTrue(any(key in e for e in errors), (key, bad, errors))

    def test_empty_strings(self):
        for key in ("match", "note", "file"):
            for bad in ("", "   "):
                errors = validate_labels(self.doc(**{key: bad}))
                self.assertTrue(any(key in e and "non-empty" in e for e in errors), (key, bad, errors))

    def test_bad_lines(self):
        for bad in (True, False, 0, -1, "3", 1.5, None):
            errors = validate_labels(self.doc(line=bad))
            self.assertTrue(any("line must be" in e for e in errors), (bad, errors))

    def test_complete_rules_entries(self):
        for bad in (5, None, ["dead_code"]):
            errors = validate_labels({"complete_rules": [bad], "labels": []})
            self.assertTrue(errors, bad)
        errors = validate_labels({"complete_rules": ["cycle", "cycle"], "labels": []})
        self.assertTrue(any("duplicate" in e for e in errors), errors)

    def test_non_list_labels_value(self):
        self.assertTrue(any("labels must be a list" in e for e in validate_labels({"complete_rules": [], "labels": "x"})))


class ManifestValidation(unittest.TestCase):
    def entry(self, **kw):
        return {"name": "r", "url": "u", "commit": "c", **kw}

    def test_valid(self):
        self.assertEqual(validate_manifest([self.entry(), self.entry(name="s")]), [])

    def test_not_a_list(self):
        for bad in ({}, "x", None, 3):
            self.assertTrue(any("must be a list" in e for e in validate_manifest(bad)), bad)

    def test_entry_not_object(self):
        errors = validate_manifest(["x", 3, None])
        self.assertEqual(len([e for e in errors if "must be an object" in e]), 3, errors)

    def test_missing_or_bad_fields(self):
        for key in ("name", "url", "commit"):
            for bad in (None, 5, "", "  "):
                entry = self.entry(**{key: bad})
                self.assertTrue(any(key in e for e in validate_manifest([entry])), (key, bad))
            entry = self.entry()
            del entry[key]
            self.assertTrue(any(key in e for e in validate_manifest([entry])), key)

    def test_duplicate_names(self):
        errors = validate_manifest([self.entry(), self.entry()])
        self.assertTrue(any("duplicate" in e for e in errors), errors)

    def test_case_duplicate_names(self):
        errors = validate_manifest([self.entry(name="Foo"), self.entry(name="foo")])
        self.assertTrue(any("duplicate" in e for e in errors), errors)

    def test_dash_leading_url_and_commit(self):
        for key in ("url", "commit"):
            errors = validate_manifest([self.entry(**{key: "--upload-pack=x"})])
            self.assertTrue(any(key in e and "must not start with" in e for e in errors), (key, errors))

    def test_unsafe_names(self):
        for bad in ("a/b", "..", ".hidden", "a\\b", "../x"):
            errors = validate_manifest([self.entry(name=bad)])
            self.assertTrue(any("name" in e and "safe" in e for e in errors), (bad, errors))


class ApplicabilityStrict(unittest.TestCase):
    def test_malformed_values(self):
        base = {"rule": "cycle", "language": "go", "reason": "r", "reviewed_by": "w"}
        for key in ("rule", "language", "reason", "reviewed_by"):
            for bad in (None, 5, [], "", "  "):
                errors, _ = validate_applicability({"not_applicable": [{**base, key: bad}]})
                self.assertTrue(any(key in e or "unknown" in e for e in errors), (key, bad, errors))
        errors, _ = validate_applicability({"not_applicable": [{"rule": ["cycle"], "language": {}, "reason": "r", "reviewed_by": "w"}]})
        self.assertTrue(errors)


class Applicability(unittest.TestCase):
    def entry(self, **kw):
        return {"rule": "cycle", "language": "go", "reason": "r", "reviewed_by": "who", **kw}

    def test_valid_and_empty(self):
        self.assertEqual(validate_applicability({"not_applicable": []}), ([], set()))
        errors, pairs = validate_applicability({"not_applicable": [self.entry()]})
        self.assertEqual((errors, pairs), ([], {("cycle", "go")}))

    def test_entry_errors(self):
        for bad, text in [
            (self.entry(rule="nope"), "unknown rule"),
            (self.entry(language="cobol"), "unknown language"),
            (self.entry(reason=""), "reason"),
            (self.entry(reviewed_by="  "), "reviewed_by"),
            (self.entry(reason=None), "reason"),
            ("x", "must be an object"),
        ]:
            errors, _ = validate_applicability({"not_applicable": [bad]})
            self.assertTrue(any(text in e for e in errors), (bad, errors))

    def test_document_shape(self):
        self.assertTrue(validate_applicability({})[0])
        self.assertTrue(validate_applicability({"not_applicable": {}})[0])

    def test_duplicate_entry(self):
        errors, _ = validate_applicability({"not_applicable": [self.entry(), self.entry()]})
        self.assertTrue(any("duplicate" in e for e in errors), errors)

    def test_listed_pair_with_findings_is_an_error(self):
        errors = not_applicable_findings({("cycle", "go")}, {"r": [("cycle", "go", "unlabelled")]})
        self.assertEqual(len(errors), 1)
        self.assertIn("cycle", errors[0])
        self.assertEqual(not_applicable_findings({("cycle", "go")}, {"r": [("cycle", "rust", "tp")]}), [])

    def test_status_not_applicable(self):
        rows = summarize({}, {("cycle", "go")})
        row = next(r for r in rows if r["rule"] == "cycle" and r["language"] == "go")
        self.assertEqual(row["status"], "not-applicable")
        self.assertEqual(next(r for r in rows if r["rule"] == "cycle" and r["language"] == "rust")["status"], "no-data")


class Summary(unittest.TestCase):
    def row(self, rows, rule, lang):
        return next(r for r in rows if r["rule"] == rule and r["language"] == lang)

    def test_every_pair_is_reported(self):
        self.assertEqual(len(summarize({})), 36)
        self.assertEqual(self.row(summarize({}), "cycle", "go")["status"], "no-data")

    def test_statuses(self):
        rows = summarize({
            "one": [("dead_code", "rust", "fp")] * 15 + [("cycle", "rust", "tp"), ("cycle", "rust", "unlabelled")],
            "two": [("dead_code", "rust", "tp")] * 15 + [("complexity", "go", "tp")] * 40,
        })
        dead = self.row(rows, "dead_code", "rust")
        self.assertEqual((dead["status"], dead["repos"], dead["precision"], dead["coverage"]), ("gate-ready", 2, 0.5, 1.0))
        cyc = self.row(rows, "cycle", "rust")
        self.assertEqual((cyc["status"], cyc["coverage"]), ("incomplete", 0.5))
        self.assertEqual(self.row(rows, "complexity", "go")["status"], "insufficient")


FAKE_REPORT = {"findings": [
    {"rule": "dead_code", "message": "unused function `foo`", "span": {"file": "a.rs", "start_line": 1}},
]}


class EndToEnd(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.dir = Path(self.tmp.name)
        self.source = self.dir / "source"
        self.source.mkdir()
        git = ["git", "-C", str(self.source), "-c", "user.name=t", "-c", "user.email=t@t"]
        subprocess.run(["git", "init", "--quiet", str(self.source)], check=True)
        (self.source / "a.rs").write_text("fn foo() {}\n")
        subprocess.run(git + ["add", "."], check=True)
        subprocess.run(git + ["commit", "--quiet", "-m", "init"], check=True)
        self.commit = subprocess.run(
            ["git", "-C", str(self.source), "rev-parse", "HEAD"], capture_output=True, text=True, check=True
        ).stdout.strip()
        self.corpus = self.dir / "corpus"
        (self.corpus / "labels").mkdir(parents=True)
        (self.corpus / "applicability.json").write_text('{"not_applicable": []}')
        self.write_corpus(str(self.source), [label("dead_code", "a.rs", 1, "foo", "fp")])

    def write_corpus(self, url, labels):
        (self.corpus / "repos.json").write_text(json.dumps([{"name": "r", "url": url, "commit": self.commit}]))
        (self.corpus / "labels" / "r.json").write_text(
            json.dumps({"repo": "r", "complete_rules": ["dead_code"], "labels": labels}))

    def fake(self, body, code=0):
        path = self.dir / "fake-stratify"
        path.write_text(f"#!{sys.executable}\nimport sys\nprint({body!r})\nsys.exit({code})\n")
        path.chmod(path.stat().st_mode | stat.S_IEXEC)
        return path

    def run_tool(self, fake, *extra):
        return subprocess.run(
            [sys.executable, str(PRECISION), "--stratify", str(fake), "--corpus", str(self.corpus),
             "--work", str(self.dir / "work"), *extra],
            capture_output=True, text=True)

    def test_valid_corpus_exits_zero(self):
        proc = self.run_tool(self.fake(json.dumps(FAKE_REPORT)))
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertIn("gate-ready: 0 of 36 applicable pairs (0 not applicable).", proc.stdout)

    def test_stale_label_exits_two(self):
        self.write_corpus(str(self.source), [label("dead_code", "a.rs", 9, "gone")])
        proc = self.run_tool(self.fake(json.dumps(FAKE_REPORT)))
        self.assertEqual(proc.returncode, 2, proc.stderr)
        self.assertIn("stale", proc.stderr)

    def test_failing_binary_exits_one(self):
        proc = self.run_tool(self.fake("{}", code=3))
        self.assertEqual(proc.returncode, 1)
        self.assertIn("precision:", proc.stderr)
        self.assertNotIn("Traceback", proc.stderr)

    def test_invalid_json_exits_one(self):
        proc = self.run_tool(self.fake("not json"))
        self.assertEqual(proc.returncode, 1)
        self.assertIn("precision:", proc.stderr)
        self.assertNotIn("Traceback", proc.stderr)

    def test_unclonable_url_exits_one(self):
        self.write_corpus(str(self.dir / "missing"), [])
        proc = self.run_tool(self.fake(json.dumps(FAKE_REPORT)))
        self.assertEqual(proc.returncode, 1)
        self.assertIn("precision:", proc.stderr)

    def test_missing_labels_file_exits_two(self):
        (self.corpus / "labels" / "r.json").unlink()
        proc = self.run_tool(self.fake(json.dumps(FAKE_REPORT)))
        self.assertEqual(proc.returncode, 2)
        self.assertIn("precision:", proc.stderr)
        self.assertNotIn("Traceback", proc.stderr)

    def test_schema_errors_are_reported_before_any_clone(self):
        (self.corpus / "repos.json").write_text(json.dumps([
            {"name": "r", "url": str(self.source), "commit": self.commit},
            {"name": "s", "url": str(self.dir / "missing"), "commit": self.commit},
        ]))
        (self.corpus / "labels" / "r.json").write_text(json.dumps({"complete_rules": [], "labels": ["x"]}))
        (self.corpus / "labels" / "s.json").write_text(json.dumps({"complete_rules": [], "labels": []}))
        proc = self.run_tool(self.fake(json.dumps(FAKE_REPORT)))
        self.assertEqual(proc.returncode, 2)
        self.assertIn("must be an object", proc.stderr)
        self.assertFalse((self.dir / "work" / "r").exists())
        self.assertFalse((self.dir / "work" / "s").exists())

    def test_integer_match_exits_two_without_traceback(self):
        self.write_corpus(str(self.source), [{**label("dead_code", "a.rs", 1, "foo"), "match": 5}])
        proc = self.run_tool(self.fake(json.dumps(FAKE_REPORT)))
        self.assertEqual(proc.returncode, 2, proc.stderr)
        self.assertIn("precision:", proc.stderr)
        self.assertNotIn("Traceback", proc.stderr)

    def test_malformed_manifest_exits_two_without_traceback(self):
        (self.corpus / "repos.json").write_text(json.dumps([{"name": "../x", "url": 3}, "x"]))
        proc = self.run_tool(self.fake(json.dumps(FAKE_REPORT)))
        self.assertEqual(proc.returncode, 2, proc.stderr)
        self.assertIn("precision:", proc.stderr)
        self.assertNotIn("Traceback", proc.stderr)

    def test_manifest_object_exits_two_without_traceback(self):
        (self.corpus / "repos.json").write_text("{}")
        proc = self.run_tool(self.fake(json.dumps(FAKE_REPORT)))
        self.assertEqual(proc.returncode, 2, proc.stderr)
        self.assertNotIn("Traceback", proc.stderr)

    def test_malformed_applicability_exits_two_without_traceback(self):
        (self.corpus / "applicability.json").write_text(json.dumps({"not_applicable": [
            {"rule": ["cycle"], "language": 5, "reason": "r", "reviewed_by": "w"}]}))
        proc = self.run_tool(self.fake(json.dumps(FAKE_REPORT)))
        self.assertEqual(proc.returncode, 2, proc.stderr)
        self.assertIn("precision:", proc.stderr)
        self.assertNotIn("Traceback", proc.stderr)

    def test_invalid_json_in_each_corpus_file_exits_two(self):
        for rel in ("repos.json", "applicability.json", "labels/r.json"):
            with self.subTest(rel):
                self.setUp()
                (self.corpus / rel).write_text("{not json")
                proc = self.run_tool(self.fake(json.dumps(FAKE_REPORT)))
                self.assertEqual(proc.returncode, 2, proc.stderr)
                self.assertIn(f"{rel}: cannot read", proc.stderr)
                self.assertNotIn("Traceback", proc.stderr)

    def test_broken_applicability_and_label_error_are_both_reported(self):
        (self.corpus / "applicability.json").write_text("{not json")
        self.write_corpus(str(self.source), [{**label("dead_code", "a.rs", 1, "foo"), "match": 5}])
        proc = self.run_tool(self.fake(json.dumps(FAKE_REPORT)))
        self.assertEqual(proc.returncode, 2, proc.stderr)
        self.assertIn("applicability.json: cannot read", proc.stderr)
        self.assertIn("match must be a non-empty string", proc.stderr)
        self.assertNotIn("Traceback", proc.stderr)

    def test_not_applicable_pair_with_findings_exits_two(self):
        (self.corpus / "applicability.json").write_text(json.dumps({"not_applicable": [
            {"rule": "dead_code", "language": "rust", "reason": "r", "reviewed_by": "w"}]}))
        proc = self.run_tool(self.fake(json.dumps(FAKE_REPORT)))
        self.assertEqual(proc.returncode, 2, proc.stderr)

    def test_deeply_nested_json_exits_two(self):
        (self.corpus / "repos.json").write_text("[" * 1000000 + "]" * 1000000)
        proc = self.run_tool(self.fake(json.dumps(FAKE_REPORT)))
        self.assertEqual(proc.returncode, 2, proc.stderr)
        self.assertIn("repos.json: cannot read", proc.stderr)
        self.assertNotIn("Traceback", proc.stderr)

    def test_dash_leading_url_and_commit_exit_two_without_clone(self):
        for key in ("url", "commit"):
            with self.subTest(key):
                entry = {"name": "r", "url": str(self.source), "commit": self.commit, key: "-x"}
                (self.corpus / "repos.json").write_text(json.dumps([entry]))
                proc = self.run_tool(self.fake(json.dumps(FAKE_REPORT)))
                self.assertEqual(proc.returncode, 2, proc.stderr)
                self.assertNotIn("Traceback", proc.stderr)
                self.assertFalse((self.dir / "work" / "r").exists())

    def test_case_duplicate_names_exit_two(self):
        (self.corpus / "repos.json").write_text(json.dumps([
            {"name": "Foo", "url": str(self.source), "commit": self.commit},
            {"name": "foo", "url": str(self.source), "commit": self.commit}]))
        proc = self.run_tool(self.fake(json.dumps(FAKE_REPORT)))
        self.assertEqual(proc.returncode, 2, proc.stderr)
        self.assertIn("duplicate", proc.stderr)

    def test_finding_missing_span_exits_one(self):
        proc = self.run_tool(self.fake(json.dumps({"findings": [{"rule": "dead_code"}]})))
        self.assertEqual(proc.returncode, 1, proc.stderr)
        self.assertIn("precision:", proc.stderr)
        self.assertNotIn("Traceback", proc.stderr)

    def test_not_applicable_footer(self):
        (self.corpus / "applicability.json").write_text(json.dumps({"not_applicable": [
            {"rule": "cycle", "language": "go", "reason": "r", "reviewed_by": "w"}]}))
        proc = self.run_tool(self.fake(json.dumps(FAKE_REPORT)))
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertIn("gate-ready: 0 of 35 applicable pairs (1 not applicable).", proc.stdout)
        self.assertIn("not-applicable", proc.stdout)


if __name__ == "__main__":
    unittest.main()
