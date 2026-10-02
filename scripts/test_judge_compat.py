import unittest

from judge_compat import classify, compare, pair_key, strip_evidence, verdicts


def finding(rule="dead_code", file="a.rs", line=1, message="unused function `f`", **extra):
    f = {"rule": rule, "message": message, "span": {"file": file, "start_line": line}}
    f.update(extra)
    return f


def judged(verdict, **kw):
    return finding(judgment={"verdict": verdict, "reason": "r"}, **kw)


class JudgeCompatTest(unittest.TestCase):
    def test_strip_removes_the_three_evidence_keys_from_every_finding(self):
        report = {
            "schema_version": 1,
            "findings": [finding(related=[1], path=["a"], reasons=["x"], confidence="certain")],
        }
        stripped = strip_evidence(report)
        self.assertEqual(set(stripped["findings"][0]), {"rule", "message", "span", "confidence"})
        self.assertIn("related", report["findings"][0])  # input untouched

    def test_pair_key_uses_rule_file_line_message(self):
        self.assertEqual(
            pair_key(finding(file="x.rs", line=7)),
            ("dead_code", "x.rs", 7, "unused function `f`"),
        )

    def test_verdicts_keep_only_judged_findings(self):
        report = {"findings": [judged("keep"), finding(line=2)]}
        self.assertEqual(len(verdicts(report)), 1)
        self.assertEqual(list(verdicts(report).values()), ["keep"])

    def test_compare_flags_differing_verdicts(self):
        a = {"findings": [judged("keep"), judged("dismiss", line=2)]}
        b = {"findings": [judged("keep"), judged("keep", line=2)]}
        diff = compare(a, b)
        self.assertEqual(len(diff["compared"]), 2)
        self.assertEqual(list(diff["differs"]), [pair_key(judged("x", line=2))])

    def test_compare_treats_a_missing_counterpart_as_a_difference(self):
        a = {"findings": [judged("keep")]}
        b = {"findings": []}
        self.assertEqual(len(compare(a, b)["differs"]), 1)

    def test_classify(self):
        self.assertEqual(classify("keep", "keep", None), "match")
        self.assertEqual(classify("keep", "keep", "dismiss"), "match")
        self.assertEqual(classify("keep", "dismiss", "keep"), "unstable")
        self.assertEqual(classify("keep", "dismiss", "dismiss"), "changed")
        self.assertEqual(classify("keep", "dismiss", "other"), "changed")
        self.assertEqual(classify("keep", "dismiss", None), "unverified")


if __name__ == "__main__":
    unittest.main()
