import unittest

from precision import classify, label_matches, language_of, summarize, validate_labels


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


if __name__ == "__main__":
    unittest.main()
