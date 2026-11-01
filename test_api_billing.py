import io
import json
import tempfile
import unittest
import urllib.error
import urllib.parse
from datetime import datetime, timedelta, timezone
from pathlib import Path
from unittest.mock import patch

import api_billing

NOW = datetime.now(timezone.utc)
TODAY = NOW.strftime('%Y-%m-%d')
YESTERDAY = (NOW - timedelta(days=1)).strftime('%Y-%m-%d')


class FakeResponse(io.BytesIO):
    def __enter__(self):
        return self

    def __exit__(self, *a):
        self.close()


def anthropic_server(req, timeout=None):
    url = urllib.parse.urlparse(req.full_url)
    q = urllib.parse.parse_qs(url.query)
    assert req.get_header('X-api-key') == 'sk-ant-admin-test'
    if url.path.endswith('/cost_report'):
        if 'page' not in q:  # first page: yesterday, with a second page to follow
            body = {'data': [{'starting_at': f'{YESTERDAY}T00:00:00Z', 'results': [{'amount': '250.00', 'currency': 'USD'}]}],
                    'has_more': True, 'next_page': 'p2'}
        else:
            body = {'data': [{'starting_at': f'{TODAY}T00:00:00Z',
                              'results': [{'amount': '100.00', 'currency': 'USD'}, {'amount': '50.50', 'currency': 'USD'}]}],
                    'has_more': False, 'next_page': None}
    else:
        assert q.get('group_by[]') == ['model']
        body = {'data': [{'starting_at': f'{TODAY}T00:00:00Z', 'results': [{
            'model': 'claude-opus-5-5', 'uncached_input_tokens': 1000, 'cache_read_input_tokens': 5000,
            'cache_creation': {'ephemeral_1h_input_tokens': 200, 'ephemeral_5m_input_tokens': 300}, 'output_tokens': 400}]}],
            'has_more': False, 'next_page': None}
    return FakeResponse(json.dumps(body).encode())


def openai_server(req, timeout=None):
    url = urllib.parse.urlparse(req.full_url)
    q = urllib.parse.parse_qs(url.query)
    assert req.get_header('Authorization') == 'Bearer sk-admin-test'
    start = int(NOW.replace(hour=0, minute=0, second=0, microsecond=0).timestamp())
    if url.path.endswith('/costs'):
        body = {'data': [{'start_time': start, 'results': [{'amount': {'value': 1.25, 'currency': 'usd'}}]}], 'has_more': False}
    else:
        assert q.get('group_by') == ['model']
        body = {'data': [{'start_time': start, 'results': [{'model': 'gpt-6.1', 'input_tokens': 900, 'input_cached_tokens': 300,
                                                             'output_tokens': 100, 'num_model_requests': 7}]}], 'has_more': False}
    return FakeResponse(json.dumps(body).encode())


class ApiBillingTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        root = Path(self.tmp.name)
        self.patches = [patch.object(api_billing, 'BALANCES', root / 'balances.json'),
                        patch.object(api_billing, 'API_CACHE', root / 'cache.json')]
        for p in self.patches:
            p.start()

    def tearDown(self):
        for p in self.patches:
            p.stop()
        self.tmp.cleanup()

    def run_with(self, keys, server):
        with patch.object(api_billing, 'admin_key', lambda env: keys.get(env)), \
             patch('urllib.request.urlopen', server):
            return {p['provider']: p for p in api_billing.api_usage(log=lambda *a: None)}

    def test_unconfigured_providers_are_listed_but_not_fetched(self):
        out = self.run_with({}, lambda *a, **k: self.fail('no request expected'))
        self.assertFalse(out['Anthropic']['configured'])
        self.assertEqual(out['OpenAI']['env'], 'OPENAI_ADMIN_KEY')

    def test_anthropic_cents_pagination_and_tokens(self):
        a = self.run_with({'ANTHROPIC_ADMIN_KEY': 'sk-ant-admin-test'}, anthropic_server)['Anthropic']
        self.assertEqual(a['daily'], {YESTERDAY: 2.5, TODAY: 1.505})
        self.assertAlmostEqual(a['today'], 1.505)
        self.assertAlmostEqual(a['last30'], 4.005)
        self.assertEqual(a['tokens_30d']['cache_write'], 500)
        self.assertEqual(a['models'][0]['model'], 'claude-opus-5-5')
        self.assertEqual(a['models'][0]['total'], 1000 + 5000 + 500 + 400)
        self.assertIsNone(a['error'])

    def test_openai_dollars_and_cached_split(self):
        o = self.run_with({'OPENAI_ADMIN_KEY': 'sk-admin-test'}, openai_server)['OpenAI']
        self.assertAlmostEqual(o['today'], 1.25)
        self.assertEqual(o['tokens_30d']['input'], 600)
        self.assertEqual(o['tokens_30d']['cache_read'], 300)
        self.assertEqual(o['tokens_30d']['requests'], 7)

    def test_balance_subtracts_only_spend_after_entry(self):
        # $1.00 of today's $1.505 had already happened when the balance was read
        api_billing.set_balance('Anthropic', 20.0, today_spend=1.0)
        a = self.run_with({'ANTHROPIC_ADMIN_KEY': 'sk-ant-admin-test'}, anthropic_server)['Anthropic']
        self.assertEqual(a['balance']['spent_since'], 0.5)  # 1.505 - 1.0, yesterday excluded
        self.assertEqual(a['balance']['remaining'], 19.5)
        api_billing.set_balance('Anthropic', None)
        self.assertIsNone(self.run_with({'ANTHROPIC_ADMIN_KEY': 'sk-ant-admin-test'}, anthropic_server)['Anthropic']['balance'])

    def test_failure_falls_back_to_last_good_report(self):
        self.run_with({'ANTHROPIC_ADMIN_KEY': 'sk-ant-admin-test'}, anthropic_server)

        def rejected(req, timeout=None):
            raise urllib.error.HTTPError(req.full_url, 401, 'Unauthorized', {}, None)
        a = self.run_with({'ANTHROPIC_ADMIN_KEY': 'sk-ant-admin-test'}, rejected)['Anthropic']
        self.assertEqual(a['source'], 'cached')
        self.assertIn('admin key', a['error'])
        self.assertAlmostEqual(a['today'], 1.505)


if __name__ == '__main__':
    unittest.main()
