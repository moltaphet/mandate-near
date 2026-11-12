#!/usr/bin/env python3
"""Offline tests for the pure guards in scripts/live-price-feed.py. No network."""
import importlib.util
import os
import unittest

spec = importlib.util.spec_from_file_location("feed", os.path.join(os.path.dirname(__file__), "live-price-feed.py"))
feed = importlib.util.module_from_spec(spec)
spec.loader.exec_module(feed)


class Guards(unittest.TestCase):
    def test_deviation_is_relative_to_the_reference(self):
        self.assertAlmostEqual(feed.deviation(102.0, 100.0), 0.02)
        self.assertAlmostEqual(feed.deviation(98.0, 100.0), 0.02)

    def test_limits_catch_a_unit_error_but_not_normal_spread(self):
        self.assertLessEqual(feed.deviation(3.386, 3.381), feed.MAX_SOURCE_DEVIATION)
        self.assertGreater(feed.deviation(338.1, 3.381), feed.MAX_JUMP)

    def test_stale_and_future_quotes_are_refused(self):
        now = 1_800_000_000
        with self.assertRaises(ValueError):
            feed.check_price("NEAR", {"usd": 3.4, "last_updated_at": now - 61}, feed.NEAR_RANGE, now)
        with self.assertRaises(ValueError):
            feed.check_price("NEAR", {"usd": 3.4, "last_updated_at": now + 6}, feed.NEAR_RANGE, now)
        self.assertEqual(feed.check_price("NEAR", {"usd": 3.4, "last_updated_at": now - 5}, feed.NEAR_RANGE, now), 3.4)


if __name__ == "__main__":
    unittest.main()
