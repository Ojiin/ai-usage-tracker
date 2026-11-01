"""API spend, token usage and an estimated remaining balance for Anthropic and OpenAI.

Spend and usage come from each provider's official admin reports, which need an *admin* key
(ANTHROPIC_ADMIN_KEY / OPENAI_ADMIN_KEY as Windows user environment variables). Neither provider
exposes the prepaid credit balance, so the balance is an estimate: the amount the user entered,
minus the spend reported since they entered it.
"""
import json
import os
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime, timedelta, timezone
from pathlib import Path

STATE_DIR = Path(os.environ.get("LOCALAPPDATA", Path.home())) / "AIUsageTracker"
BALANCES = STATE_DIR / "api_balances.json"
API_CACHE = STATE_DIR / "api_cache.json"

PROVIDERS = {
    "Anthropic": {"env": "ANTHROPIC_ADMIN_KEY", "console": "https://platform.claude.com/settings/admin-keys"},
    "OpenAI": {"env": "OPENAI_ADMIN_KEY", "console": "https://platform.openai.com/settings/organization/admin-keys"},
}
DAYS = 31  # history shown in the dashboard; both reports allow 31 daily buckets per page


def admin_key(env):
    """Environment first; then the user's saved variables, so a running widget sees a key set after it started."""
    key = os.environ.get(env)
    if key:
        return key.strip()
    try:
        import winreg
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, "Environment") as k:
            return str(winreg.QueryValueEx(k, env)[0]).strip() or None
    except (ImportError, OSError):
        return None


def utc_day(dt):
    return dt.astimezone(timezone.utc).strftime("%Y-%m-%d")


def load_json(path):
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}


def save_json(path, data):
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(data, indent=1), encoding="utf-8")
    except OSError:
        pass


class ApiError(Exception):
    pass


def get_json(url, headers, provider):
    try:
        with urllib.request.urlopen(urllib.request.Request(url, headers=headers), timeout=20) as resp:
            return json.loads(resp.read())
    except urllib.error.HTTPError as e:
        if e.code in (401, 403):
            raise ApiError(f"{provider} rejected the admin key (HTTP {e.code}); it needs an admin key, not a normal API key")
        raise ApiError(f"{provider} report failed (HTTP {e.code})")
    except (urllib.error.URLError, OSError, ValueError):
        raise ApiError(f"{provider} report failed (offline?)")


def paged(base_url, params, headers, provider, page_param="page"):
    """Yield buckets from a paginated report (both providers use data / has_more / next_page)."""
    params = dict(params)
    for _ in range(50):  # hard stop: 50 pages of 31 days is over four years
        body = get_json(base_url + "?" + urllib.parse.urlencode(params, doseq=True), headers, provider)
        yield from body.get("data") or []
        if not body.get("has_more") or not body.get("next_page"):
            return
        params[page_param] = body["next_page"]
    raise ApiError(f"{provider} report had too many pages")


def bucket_day(bucket):
    """UTC day of a bucket; Anthropic uses RFC 3339 strings, OpenAI uses unix seconds."""
    start = bucket.get("starting_at") or bucket.get("start_time")
    if isinstance(start, (int, float)):
        return datetime.fromtimestamp(start, timezone.utc).strftime("%Y-%m-%d")
    return str(start)[:10] if start else None


def new_tokens():
    return {"input": 0, "cache_write": 0, "cache_read": 0, "output": 0, "requests": 0}


# --- Anthropic ----------------------------------------------------------------
def anthropic_reports(key, cost_start, usage_start):
    headers = {"x-api-key": key, "anthropic-version": "2023-06-01", "User-Agent": "AIUsageTracker/1.0"}
    base = "https://api.anthropic.com/v1/organizations"
    cost = {}
    for b in paged(f"{base}/cost_report", {"starting_at": cost_start, "bucket_width": "1d", "limit": 31}, headers, "Anthropic"):
        # amount is a decimal string in cents: "123.45" means $1.23
        cost[bucket_day(b)] = cost.get(bucket_day(b), 0.0) + sum(float(r.get("amount") or 0) for r in b.get("results") or []) / 100
    tokens, models = {}, {}
    params = {"starting_at": usage_start, "bucket_width": "1d", "limit": 31, "group_by[]": ["model"]}
    for b in paged(f"{base}/usage_report/messages", params, headers, "Anthropic"):
        day = tokens.setdefault(bucket_day(b), new_tokens())
        for r in b.get("results") or []:
            cc = r.get("cache_creation") or {}
            row = {"input": r.get("uncached_input_tokens") or 0,
                   "cache_write": (cc.get("ephemeral_1h_input_tokens") or 0) + (cc.get("ephemeral_5m_input_tokens") or 0),
                   "cache_read": r.get("cache_read_input_tokens") or 0,
                   "output": r.get("output_tokens") or 0, "requests": 0}
            for target in (day, models.setdefault(r.get("model") or "unknown", new_tokens())):
                for f, v in row.items():
                    target[f] += v
    return cost, tokens, models


# --- OpenAI -------------------------------------------------------------------
def openai_reports(key, cost_start, usage_start):
    headers = {"Authorization": f"Bearer {key}", "User-Agent": "AIUsageTracker/1.0"}
    base = "https://api.openai.com/v1/organization"
    to_unix = lambda iso: int(datetime.fromisoformat(iso.replace("Z", "+00:00")).timestamp())
    cost = {}
    for b in paged(f"{base}/costs", {"start_time": to_unix(cost_start), "bucket_width": "1d", "limit": 31}, headers, "OpenAI"):
        total = 0.0
        for r in b.get("results") or []:
            amount = r.get("amount")
            total += float((amount.get("value") if isinstance(amount, dict) else amount) or 0)  # already in dollars
        cost[bucket_day(b)] = cost.get(bucket_day(b), 0.0) + total
    tokens, models = {}, {}
    params = {"start_time": to_unix(usage_start), "bucket_width": "1d", "limit": 31, "group_by": ["model"]}
    for b in paged(f"{base}/usage/completions", params, headers, "OpenAI"):
        day = tokens.setdefault(bucket_day(b), new_tokens())
        for r in b.get("results") or []:
            cached = r.get("input_cached_tokens") or 0
            row = {"input": max((r.get("input_tokens") or 0) - cached, 0), "cache_write": 0, "cache_read": cached,
                   "output": r.get("output_tokens") or 0, "requests": r.get("num_model_requests") or 0}
            for target in (day, models.setdefault(r.get("model") or "unknown", new_tokens())):
                for f, v in row.items():
                    target[f] += v
    return cost, tokens, models


FETCHERS = {"Anthropic": anthropic_reports, "OpenAI": openai_reports}


# --- balances -----------------------------------------------------------------
def set_balance(provider, amount, today_spend=None):
    """Record the balance the user read off the provider's billing page right now.

    Reports are daily, so remember how much of today's spend had already happened (and so was
    already reflected in that balance); only spend after it is subtracted later."""
    balances = load_json(BALANCES)
    if amount is None:
        balances.pop(provider, None)
    else:
        if today_spend is None:
            today_spend = (load_json(API_CACHE).get(provider, {}).get("daily") or {}).get(utc_day(datetime.now(timezone.utc)), 0.0)
        balances[provider] = {"amount": round(float(amount), 2),
                              "as_of": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
                              "day_baseline": round(float(today_spend or 0), 4)}
    save_json(BALANCES, balances)


def estimate_balance(entry, daily):
    if not entry:
        return None
    since_day = entry["as_of"][:10]
    spent = sum(v for d, v in daily.items() if d and d >= since_day) - (entry.get("day_baseline") or 0)
    spent = max(spent, 0.0)
    return {"entered": entry["amount"], "as_of": entry["as_of"], "spent_since": round(spent, 2),
            "remaining": round(entry["amount"] - spent, 2)}


# --- summary ------------------------------------------------------------------
def summarize(provider, daily, tokens, models, now):
    today, month = utc_day(now), utc_day(now)[:8] + "01"
    last30 = utc_day(now - timedelta(days=29))
    t30 = new_tokens()
    for d, t in tokens.items():
        if d and d >= last30:
            for f in t30:
                t30[f] += t[f]
    return {
        "provider": provider,
        "today": round(daily.get(today, 0.0), 4),
        "month": round(sum(v for d, v in daily.items() if d and d >= month), 4),
        "last30": round(sum(v for d, v in daily.items() if d and d >= last30), 4),
        "daily": {d: round(v, 4) for d, v in sorted(daily.items()) if d},
        "tokens_30d": t30,
        "models": sorted(({"model": m, **t, "total": t["input"] + t["cache_write"] + t["cache_read"] + t["output"]}
                          for m, t in models.items()), key=lambda r: -r["total"]),
    }


def api_usage(log=print):
    """One entry per provider that has an admin key: spend, tokens, estimated balance. Falls back to the
    last good reading when a report fails."""
    now = datetime.now(timezone.utc)
    balances, cache = load_json(BALANCES), load_json(API_CACHE)
    entries = []
    for provider, cfg in PROVIDERS.items():
        key = admin_key(cfg["env"])
        if not key:
            entries.append({"provider": provider, "configured": False, "env": cfg["env"], "console": cfg["console"],
                            "balance": None})
            continue
        log(f"Fetching {provider} API usage ...")
        window_start = now - timedelta(days=DAYS - 1)
        month_start = now.replace(day=1)
        bal_start = datetime.fromisoformat(balances[provider]["as_of"].replace("Z", "+00:00")) if provider in balances else now
        cost_from = min(window_start, month_start, bal_start).strftime("%Y-%m-%dT00:00:00Z")
        usage_from = window_start.strftime("%Y-%m-%dT00:00:00Z")
        try:
            daily, tokens, models = FETCHERS[provider](key, cost_from, usage_from)
            entry = dict(summarize(provider, daily, tokens, models, now), source="live",
                         as_of=now.strftime("%Y-%m-%dT%H:%M:%SZ"), error=None)
            cache[provider] = {k: entry[k] for k in ("daily", "today", "month", "last30", "tokens_30d", "models", "as_of")}
            log("  ok")
        except ApiError as e:
            log(f"  {e}")
            entry = dict(cache.get(provider) or {"daily": {}, "models": []}, provider=provider, error=str(e),
                         source="cached" if provider in cache else None)
        entry.update(configured=True, env=cfg["env"], console=cfg["console"],
                     balance=estimate_balance(balances.get(provider), entry.get("daily") or {}))
        entries.append(entry)
    save_json(API_CACHE, cache)
    return entries
