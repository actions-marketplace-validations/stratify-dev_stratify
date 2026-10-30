import unittest

from bench import build_result, parse_stats, peak_rss_mb, profile_from_path


class BenchTest(unittest.TestCase):
    def test_parse_stats_finds_the_prefixed_line(self):
        stderr = "warning: something\nstratify-stats {\"files_scanned\": 2, \"lines_scanned\": 9}\n"
        self.assertEqual(parse_stats(stderr)["lines_scanned"], 9)

    def test_parse_stats_without_line_raises(self):
        with self.assertRaises(ValueError):
            parse_stats("stratify: path not found\n")

    def test_peak_rss_units(self):
        self.assertEqual(peak_rss_mb(512 * 1024 * 1024, "darwin"), 512.0)  # bytes on macOS
        self.assertEqual(peak_rss_mb(512 * 1024, "linux"), 512.0)          # KiB on Linux

    def test_profile_from_binary_path(self):
        self.assertEqual(profile_from_path("target/release/stratify", None), "release")
        self.assertEqual(profile_from_path("/x/target/debug/stratify", None), "debug")
        self.assertEqual(profile_from_path("/usr/local/bin/stratify", None), "unknown")

    def test_profile_override_wins(self):
        self.assertEqual(profile_from_path("/usr/local/bin/stratify", "release"), "release")

    def test_build_result_records_provenance(self):
        stats = {"files_scanned": 2, "lines_scanned": 9, "functions": 3, "files_skipped": 4}
        result = build_result(stats, 1.234, 512.0, "abc1234", "release", "demo v1")
        self.assertEqual(result["stratify_commit"], "abc1234")
        self.assertEqual(result["profile"], "release")
        self.assertEqual(result["repo"], "demo v1")
        self.assertIs(result["completed"], True)
        self.assertEqual(result["wall_s"], 1.23)
        self.assertEqual(result["lines"], 9)
        self.assertEqual(result["files_skipped"], 4)


if __name__ == "__main__":
    unittest.main()
