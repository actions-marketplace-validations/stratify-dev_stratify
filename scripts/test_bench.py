import unittest

from bench import parse_stats, peak_rss_mb


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


if __name__ == "__main__":
    unittest.main()
