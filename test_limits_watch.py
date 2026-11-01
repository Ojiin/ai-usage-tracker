"""Projection and alert regressions; uses temp files, never the real history or alert state."""
import tempfile
import unittest
from datetime import datetime, timezone
from pathlib import Path

import limits_watch as lw

NOW = 1_800_000_000
RESET = NOW + 3 * 3600  # 5-hour window with 3h left


def iso(t):
    return datetime.fromtimestamp(t, timezone.utc).isoformat()


def entry(pct, t=NOW, resets_at=RESET, source="live", service="Claude Code"):
    return {"service": service, "source": source, "as_of": iso(t), "error": None,
            "windows": [{"label": "5-hour session", "used_percent": pct, "resets_at": resets_at}]}


class ProjectTests(unittest.TestCase):
    def test_hits_limit_before_reset(self):
        # 40% -> 60% in 30 min = 40%/h; 40% left takes 1h, well before the reset in 3h.
        p = lw.project([[NOW - 1800, 40, RESET], [NOW, 60, RESET]], NOW, RESET)
        self.assertEqual(p["hits_at"], NOW + 3600)
        self.assertEqual(p["rate_per_hour"], 40)

    def test_on_pace_below_limit(self):
        p = lw.project([[NOW - 3600, 10, RESET], [NOW, 15, RESET]], NOW, RESET)
        self.assertNotIn("hits_at", p)
        self.assertEqual(p["at_reset"], 30)  # 15% + 5%/h * 3h

    def test_no_projection_without_enough_span_or_rate(self):
        self.assertIsNone(lw.project([[NOW - 300, 10, RESET], [NOW, 30, RESET]], NOW, RESET))  # 5 min apart
        self.assertIsNone(lw.project([[NOW - 1800, 30, RESET], [NOW, 30, RESET]], NOW, RESET))  # idle
        self.assertIsNone(lw.project([[NOW - 3600, 10, RESET], [NOW - 2400, 20, RESET]], NOW, RESET))  # stale

    def test_previous_window_is_ignored(self):
        old = RESET - 5 * 3600
        p = lw.project([[NOW - 1800, 90, old], [NOW - 1200, 2, RESET], [NOW, 6, RESET]], NOW, RESET)
        self.assertAlmostEqual(p["rate_per_hour"], 12)

    def test_drop_inside_a_window_discards_earlier_samples(self):
        samples = [[NOW - 3000, 50, RESET], [NOW - 2000, 20, RESET], [NOW, 30, RESET]]
        self.assertAlmostEqual(lw.project(samples, NOW, RESET)["rate_per_hour"], 18)


class RecordTests(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.path = Path(self.dir.name) / "history.json"

    def tearDown(self):
        self.dir.cleanup()

    def test_projection_attached_only_to_live_readings_and_cached_repeats_add_nothing(self):
        lw.record_and_project([entry(40, NOW - 1800)], NOW - 1800, self.path)
        live = [entry(60)]
        lw.record_and_project(live, NOW, self.path)
        self.assertEqual(live[0]["windows"][0]["projection"]["hits_at"], NOW + 3600)
        cached = [entry(60, source="cached")]
        lw.record_and_project(cached, NOW + 60, self.path)
        self.assertNotIn("projection", cached[0]["windows"][0])
        history = lw._load(self.path)
        self.assertEqual(len(history["Claude Code|5-hour session"]), 2)


class AlertTests(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.path = Path(self.dir.name) / "alerts.json"
        self.fmt = lambda t: "T"

    def tearDown(self):
        self.dir.cleanup()

    def alerts(self, entries, now=NOW):
        return lw.due_alerts(entries, now, self.path, self.fmt)

    def test_each_threshold_fires_once_per_window(self):
        self.assertEqual(self.alerts([entry(50)]), [])
        first = self.alerts([entry(82)])
        self.assertEqual([a[0] for a in first], ["Claude Code 80% used"])
        self.assertEqual(self.alerts([entry(85)]), [])
        self.assertEqual([a[0] for a in self.alerts([entry(100)])], ["Claude Code limit reached"])
        # The next window starts over.
        nxt = RESET + 5 * 3600
        self.assertEqual([a[0] for a in self.alerts([entry(81, resets_at=nxt)], RESET + 60)], ["Claude Code 80% used"])

    def test_jump_past_several_thresholds_sends_one_alert(self):
        self.assertEqual([a[0] for a in self.alerts([entry(97)])], ["Claude Code 95% used"])

    def test_alert_text_includes_pace(self):
        e = entry(82)
        e["windows"][0]["projection"] = {"pct": 82, "rate_per_hour": 20, "hits_at": NOW + 3240}
        self.assertEqual(self.alerts([e])[0][1], "5-hour session at 82%. At this pace: 100% by T; resets T.")

    def test_cached_expired_and_errored_readings_never_alert(self):
        self.assertEqual(self.alerts([entry(99, source="cached")]), [])
        self.assertEqual(self.alerts([entry(99, resets_at=NOW - 10)]), [])
        failed = entry(99)
        failed["error"] = "login expired"
        self.assertEqual(self.alerts([failed]), [])


if __name__ == "__main__":
    unittest.main()
