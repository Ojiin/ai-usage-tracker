"""Writes python_expected.json: what the Python original requests, returns, logs and saves in
the scenarios of test_api_billing.py (and a few more), with the clock fixed and urlopen served by
canned servers. The Rust unit test in src/port.rs replays every step and compares.
Run from the repository root:  python3 rust/api-billing-rs/testdata/make_expected.py
No network is used: every request goes to a canned server."""
import json
import sys
import tempfile
import urllib.error
from datetime import datetime, timedelta, timezone
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, ".")
import api_billing
import test_api_billing as t

NOW = datetime(2026, 11, 19, 20, 41, 22, tzinfo=timezone.utc)


class Clock(datetime):
    @classmethod
    def now(cls, tz=None):
        return NOW.astimezone(tz)


api_billing.datetime = Clock
t.NOW, t.TODAY, t.YESTERDAY = NOW, NOW.strftime("%Y-%m-%d"), (NOW - timedelta(days=1)).strftime("%Y-%m-%d")
ANT, OAI = {"ANTHROPIC_ADMIN_KEY": "sk-ant-admin-test"}, {"OPENAI_ADMIN_KEY": "sk-admin-test"}
real_get_json, requests = api_billing.get_json, []


def get_json(url, headers, provider):
    """The original's get_json, noting each request and what came back."""
    seen = {"url": url, "headers": sorted(headers.values()), "provider": provider}
    requests.append(seen)
    try:
        seen["body"] = real_get_json(url, headers, provider)
        return seen["body"]
    except api_billing.ApiError as e:
        seen["error"] = str(e)
        raise


def status(code):
    def serve(req, timeout=None):
        raise urllib.error.HTTPError(req.full_url, code, "refused", {}, None)
    return serve


def body(text):
    return lambda req, timeout=None: t.FakeResponse(text.encode())


def offline(req, timeout=None):
    raise urllib.error.URLError("no route")


def scenario(steps):
    done = []
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        with patch.object(api_billing, "BALANCES", root / "api_balances.json"), \
             patch.object(api_billing, "API_CACHE", root / "api_cache.json"), \
             patch.object(api_billing, "get_json", get_json):
            for step in steps:
                if step[0] == "write":
                    api_billing.save_json(api_billing.BALANCES, step[1])
                    done.append({"write": step[1]})
                    continue
                if step[0] == "balance":
                    api_billing.set_balance(*step[1:])
                    done.append({"balance": list(step[1:]) + [None] * (4 - len(step))})
                    continue
                keys, server = step
                log = []
                requests.clear()
                with patch.object(api_billing, "admin_key", lambda env: keys.get(env)), \
                     patch("urllib.request.urlopen", server):
                    entries = api_billing.api_usage(log=log.append)
                done.append({"keys": keys, "requests": list(requests), "entries": entries, "log": log})
            return {"steps": done, "balances": api_billing.load_json(api_billing.BALANCES),
                    "cache": api_billing.load_json(api_billing.API_CACHE)}


ODD = json.dumps({"data": [
    {"start_time": 1795046400.5, "results": [{"amount": {"value": "0.105"}}, {"amount": None}, {"amount": 2}]},
    {"start_time": 1795046400, "results": [{"amount": {"value": 0.2}}]},
    {"start_time": 1700000000, "results": None},
    {"starting_at": "2026-10-21T00:00:00Z", "results": [
        {"model": None, "input_tokens": 5, "input_cached_tokens": 9, "output_tokens": 1, "num_model_requests": 2},
        {"model": "b", "input_tokens": 50, "output_tokens": None, "num_model_requests": 1},
        {"model": "a", "input_tokens": 500, "input_cached_tokens": 100, "output_tokens": 30}]},
    {"starting_at": "2026-10-20T00:00:00Z", "results": [{"model": "a", "input_tokens": 7}]}]})
out = {
    "unconfigured": scenario([({}, None)]),
    "anthropic": scenario([(ANT, t.anthropic_server)]),
    "openai": scenario([(OAI, t.openai_server)]),
    "both": scenario([({**ANT, **OAI}, lambda req, timeout=None: (
        t.anthropic_server if "anthropic" in req.full_url else t.openai_server)(req, timeout))]),
    "balance": scenario([("balance", "Anthropic", 20.0, 1.0), (ANT, t.anthropic_server),
                         ("balance", "OpenAI", 12.345678), ("balance", "Anthropic", None),
                         (ANT, t.anthropic_server), ("balance", "Anthropic", 3, None), (ANT, t.anthropic_server)]),
    "old balance": scenario([("write", {"Anthropic": {"amount": 50.0, "as_of": "2026-09-03T10:00:00Z", "day_baseline": 0.0},
                                        "OpenAI": {"amount": 8, "as_of": "2026-11-19T01:00:00Z"}}),
                             ({**ANT, **OAI}, lambda req, timeout=None: (
                                 t.anthropic_server if "anthropic" in req.full_url else t.openai_server)(req, timeout))]),
    "fallback": scenario([(ANT, t.anthropic_server), (ANT, status(401)), (OAI, status(403)), (OAI, status(500)),
                          (ANT, offline), (ANT, body("not json"))]),
    "empty": scenario([("balance", "OpenAI", 5), (OAI, body('{"data": [], "has_more": false}'))]),
    "odd": scenario([("balance", "OpenAI", 9.999, 0.25), (OAI, body(ODD))]),
    "endless": scenario([(ANT, body('{"data": [{"starting_at": "2026-11-19T00:00:00Z"}], "has_more": true, "next_page": "a b&c/é"}'))]),
}
Path(__file__).with_name("python_expected.json").write_text(json.dumps(out, indent=1) + "\n", encoding="utf-8")
