import unittest
from unittest.mock import patch
import openai_analytics as oa


class AnalyticsTests(unittest.TestCase):
    def test_missing_is_not_zero(self):
        data = oa.normalize({'summary': {'lifetimeTokens': 0}})
        self.assertEqual(data['summary']['lifetimeTokens'], 0)
        self.assertIsNone(data['summary']['peakDailyTokens'])
        self.assertIsNone(data['daily'])

    def test_daily_report_preserves_dates_and_gaps(self):
        data = oa.normalize({'dailyUsageBuckets': [
            {'startDate': '2026-10-29', 'tokens': 4}, {'startDate': '2026-10-31', 'tokens': 7},
            {'startDate': '2026-10-31', 'tokens': 2}, {'startDate': 'invalid', 'tokens': 9}]})
        self.assertEqual(data['daily'], {'2026-10-29': 4, '2026-10-31': 9})

    def test_failure_does_not_break_tracker(self):
        with patch.object(oa, 'read_account', side_effect=RuntimeError('Unavailable')):
            data = oa.collect(lambda *a: None)
        self.assertEqual(data['error'], 'Unavailable')
        self.assertIsNone(data['source'])
        self.assertIsNone(data['daily'])

    def test_live_report_does_not_export_extra_fields(self):
        with patch.object(oa, 'read_account', return_value={'summary': {'lifetimeTokens': 123, 'email': 'private'}, 'other': 'private'}):
            data = oa.collect(lambda *a: None)
        self.assertEqual(data['summary']['lifetimeTokens'], 123)
        self.assertNotIn('private', str(data))
        self.assertEqual(data['source'], 'live')


if __name__ == '__main__':
    unittest.main()
