import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import tracker


def meta(session='s', fork=None):
    return {'type': 'session_meta', 'timestamp': '2026-11-01T10:00:00Z',
            'payload': {'id': session, 'cwd': '/projects/demo', 'forked_from_id': fork,
                        'timestamp': '2026-11-01T10:00:00Z'}}


def usage(n, last=None, timestamp='2026-11-01T10:01:00Z'):
    def counts(x):
        return {'input_tokens': x * 10, 'cached_input_tokens': x * 4,
                'output_tokens': x * 2, 'reasoning_output_tokens': x,
                'total_tokens': x * 12}
    return {'type': 'event_msg', 'timestamp': timestamp,
            'payload': {'type': 'token_count', 'info': {
                'total_token_usage': counts(n), 'last_token_usage': counts(last if last is not None else n)}}}


class AccountingTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.p = patch.object(tracker, 'CODEX_DIR', self.root / 'sessions')
        self.p.start()
        tracker.SCAN_WARNINGS.clear()

    def tearDown(self):
        self.p.stop()
        self.tmp.cleanup()

    def write(self, name, rows, compressed=False):
        p = self.root / name
        p.parent.mkdir(parents=True, exist_ok=True)
        data = ('\n'.join(json.dumps(r) for r in rows) + '\n').encode()
        if compressed:
            import zstandard
            data = zstandard.ZstdCompressor().compress(data)
        p.write_bytes(data)

    def records(self):
        return tracker.scan_codex()[0]

    def test_first_snapshot_counts_all_not_only_last(self):
        self.write('sessions/a.jsonl', [meta(), usage(5, last=2)])
        r = self.records()[0]
        self.assertEqual((r['input'], r['cache_read'], r['output'], r['reasoning']), (30, 20, 10, 5))
        self.assertEqual(tracker.aggregate([r], {})['tool'][0]['total'], 60)

    def test_repeats_and_counter_reset(self):
        self.write('sessions/a.jsonl', [meta(), usage(5), usage(5), usage(7), usage(2)])
        self.assertEqual(sum(r['output'] for r in self.records()), 18)

    def test_archived_compressed_copy_counted_once(self):
        rows = [meta(), usage(5), usage(7)]
        self.write('sessions/a.jsonl', rows)
        self.write('archived_sessions/a.jsonl.zst', rows, True)
        self.assertEqual(sum(r['output'] for r in self.records()), 14)

    def test_fork_copied_history_is_baseline(self):
        self.write('sessions/fork.jsonl', [meta('child', 'parent'),
                   usage(5, timestamp='2026-11-01T09:59:59.999Z'), usage(7)])
        self.assertEqual(sum(r['output'] for r in self.records()), 4)

    def test_resumed_session_in_separate_file_uses_existing_total(self):
        self.write('sessions/a.jsonl', [meta(), usage(5)])
        self.write('sessions/b.jsonl', [meta(), usage(7, timestamp='2026-11-01T10:02:00Z')])
        self.assertEqual(sum(r['output'] for r in self.records()), 14)

    def test_fork_without_history_uses_last(self):
        self.write('sessions/fork.jsonl', [meta('child', 'parent'), usage(7, last=2)])
        self.assertEqual(sum(r['output'] for r in self.records()), 4)

    def test_model_switch_and_session_identity(self):
        self.write('sessions/a.jsonl', [meta(), {'type': 'turn_context', 'payload': {'model': 'gpt-6-astra'}},
                   usage(2), {'type': 'turn_context', 'payload': {'model': 'gpt-6.1-sol'}}, usage(5)])
        rows = self.records()
        self.assertEqual([r['model'] for r in rows], ['gpt-6-astra', 'gpt-6.1-sol'])
        self.assertEqual({r['session'] for r in rows}, {'s'})

    def test_truncated_line_and_corrupt_compressed_warning(self):
        self.write('sessions/a.jsonl', [meta(), usage(2)])
        with (self.root / 'sessions/a.jsonl').open('a') as f:
            f.write('{"type":')
        (self.root / 'sessions/b.jsonl.zst').write_bytes(b'corrupt')
        self.assertEqual(len(self.records()), 1)
        self.assertTrue(tracker.SCAN_WARNINGS)

    def test_multiple_limit_buckets(self):
        a = usage(2)
        a['payload']['rate_limits'] = {'limit_id': 'codex', 'primary': {'used_percent': 24, 'window_minutes': 10080}}
        b = usage(3)
        b['payload']['rate_limits'] = {'limit_id': 'spark', 'primary': {'used_percent': 12, 'window_minutes': 15}}
        self.write('sessions/a.jsonl', [meta(), a, b])
        limits = tracker.codex_log_limits(tracker.scan_codex()[1])
        self.assertEqual([w['label'] for w in limits['windows']], ['7-day', 'spark: 15-minute'])

    def test_empty_live_windows_dont_replace_log_data(self):
        with patch.object(tracker, 'CODEX_DIR', self.root / 'sessions'), patch.object(tracker, 'get_json', return_value=({}, None)):
            (self.root / 'auth.json').write_text(json.dumps({'tokens': {'access_token': 'test'}}))
            live, error = tracker.codex_live()
            self.assertIsNone(live)
            self.assertIn('no usage windows', error)

    def test_live_credits_and_review_bucket(self):
        window = {'used_percent': 30, 'limit_window_seconds': 900, 'reset_at': 1793540000}
        body = {'plan_type': 'business', 'rate_limit': {'primary_window': window},
                'code_review_rate_limit': {'primary_window': window},
                'credits': {'balance': '42.5'}, 'spend_control_reached': True}
        (self.root / 'auth.json').write_text(json.dumps({'tokens': {'access_token': 'test'}}))
        with patch.object(tracker, 'get_json', return_value=(body, None)):
            live, error = tracker.codex_live()
        self.assertIsNone(error)
        self.assertEqual([w['label'] for w in live['windows']], ['15-minute', 'Code review: 15-minute'])
        self.assertIn('Credit balance: 42.5', live['details'])
        self.assertIn('Workspace spend control reached', live['details'])


class CostTests(unittest.TestCase):
    def test_daily_cost_split_by_tool_and_unpriced_flagged(self):
        rec = lambda tool, model, day: {'tool': tool, 'model': model, 'day': day, 'project': 'p', 'session': 's',
                                         'input': 1_000_000, 'cache_write': 0, 'cache_read': 2_000_000, 'output': 100_000}
        pricing = {'claude-opus-5-5': {'input': 4, 'cache_write': 8, 'cache_read': 0.2, 'output': 20}}
        out = tracker.aggregate([rec('Claude Code', 'claude-opus-5-5', '2026-11-01'),
                                 rec('Claude Code', 'claude-opus-5-5', '2026-11-01'),
                                 rec('Codex', 'mystery-model', '2026-11-01')], pricing)
        self.assertAlmostEqual(out['daily_tool_cost']['2026-11-01']['Claude Code'], 2 * (4 + 0.4 + 2))
        self.assertNotIn('Codex', out['daily_tool_cost']['2026-11-01'])
        self.assertEqual(out['daily_tool_unpriced']['2026-11-01'], ['Codex'])


if __name__ == '__main__':
    unittest.main()
