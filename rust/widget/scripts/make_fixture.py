"""Builds the widget's fake-data fixtures by running the Python original on made-up logs.

    python scripts/make_fixture.py --home /some/throwaway/folder

Writes fixtures/sample-data.json (exactly what tracker.collect() returned for the fake logs) and
fixtures/sample-full.json (the same, edited below so every part of the widget has something to
show: live plan limits with pace, API spend and balance, account analytics, working sessions).

It never reads real logs or logins: HOME, LOCALAPPDATA and the Claude/Codex folders all point
into the throwaway folder before tracker.py is imported, the admin keys are removed from the
environment, and with no saved logins there the live checks stop before any network call.
"""
import argparse
import copy
import json
import os
import sys
import time
from datetime import datetime, timedelta, timezone
from pathlib import Path

WIDGET = Path(__file__).resolve().parent.parent
REPO = WIDGET.parent.parent


def iso(dt):
    return dt.astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def write(path, rows):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(json.dumps(r) for r in rows) + "\n", encoding="utf-8")


def claude_rows(model, when, n, cwd, effort=None):
    rows = []
    for i in range(n):
        row = {"type": "assistant", "timestamp": iso(when + timedelta(minutes=3 * i)), "requestId": f"r-{when:%d%H}-{i}",
               "cwd": cwd, "message": {"id": f"m-{when:%d%H}-{i}", "model": model, "content": [],
                                       "usage": {"input_tokens": 1800 + 40 * i, "cache_creation_input_tokens": 5200,
                                                 "cache_read_input_tokens": 96000 + 900 * i, "output_tokens": 2400 + 15 * i}}}
        if effort:
            row["effort"] = effort
        rows.append(row)
    return rows


def codex_usage(n, when):
    counts = {"input_tokens": n * 9000, "cached_input_tokens": n * 6000, "output_tokens": n * 1100,
              "reasoning_output_tokens": n * 400, "total_tokens": n * 10100}
    return {"type": "event_msg", "timestamp": iso(when),
            "payload": {"type": "token_count", "info": {"total_token_usage": counts, "last_token_usage": counts}}}


def fake_logs(home, now):
    claude, codex = home / ".claude" / "projects", home / ".codex" / "sessions"
    month_start = now.replace(day=1, hour=9, minute=0, second=0, microsecond=0)
    days = [month_start + timedelta(days=d) for d in range(0, (now - month_start).days, 2)]
    for i, day in enumerate(days):
        write(claude / "C--work-usage-tracker" / f"old-{i}.jsonl",
              claude_rows("claude-opus-5-5" if i % 3 else "claude-sonnet-5-5", day, 30 + 7 * i, "C:\\work\\usage-tracker"))
        write(codex / f"{day:%Y}" / f"rollout-{day:%Y-%m-%d}-old-{i}.jsonl", [
            {"type": "session_meta", "timestamp": iso(day), "payload": {"id": f"old-{i}", "cwd": "C:\\work\\photo", "timestamp": iso(day)}},
            {"type": "turn_context", "payload": {"model": "gpt-6.1-sol" if i % 2 else "gpt-5.3-codex-spark"}},
            codex_usage(20 + 5 * i, day + timedelta(minutes=30)), codex_usage(55 + 9 * i, day + timedelta(hours=2))])
    for path in list(claude.rglob("*.jsonl")) + list(codex.rglob("*.jsonl")):  # finished long ago, not "working"
        os.utime(path, (time.time() - 3600, time.time() - 3600))
    # Today's sessions are written last, so they are the "working" ones (changed in the last 2 minutes).
    start = now - timedelta(hours=3)
    main = claude / "C--work-usage-tracker" / "s-today.jsonl"
    write(main, [{"type": "custom-title", "customTitle": "Port the widget to Rust"},
                 {"type": "last-prompt", "lastPrompt": "make the hover card match the tkinter one, then run the differential check again"},
                 *claude_rows("claude-opus-5-5", start, 42, "C:\\work\\usage-tracker", effort="medium")])
    sub = claude / "C--work-usage-tracker" / "s-today" / "subagents" / "agent-a.jsonl"
    write(sub, claude_rows("claude-haiku-5-5", start + timedelta(hours=2), 6, "C:\\work\\usage-tracker"))
    sub.with_suffix(".meta.json").write_text(json.dumps({"agentType": "Explore", "description": "find every strftime call in widget.py"}), encoding="utf-8")
    write(codex / f"{now:%Y}" / "rollout-today-parent.jsonl", [
        {"type": "session_meta", "timestamp": iso(start), "payload": {"id": "p-1", "cwd": "C:\\work\\photo", "timestamp": iso(start)}},
        {"type": "turn_context", "payload": {"model": "gpt-6.1-sol", "effort": "high", "cwd": "C:\\work\\photo"}},
        {"type": "response_item", "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "speed up the RAW importer"}]}},
        codex_usage(40, start + timedelta(minutes=20)), codex_usage(130, start + timedelta(hours=2))])
    write(codex / f"{now:%Y}" / "rollout-today-child.jsonl", [
        {"type": "session_meta", "payload": {"id": "c-1", "parent_thread_id": "p-1", "agent_nickname": "Ada", "thread_source": "subagent"}},
        {"type": "turn_context", "payload": {"model": "gpt-6.1-sol", "effort": "medium"}},
        {"type": "response_item", "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "profile the thumbnail cache"}]}}])
    write(codex.parent / "session_index.jsonl", [{"id": "p-1", "thread_name": "Speed up import"}])
    write(home / ".claude" / "history.jsonl", [{"display": "/model opus", "timestamp": int(time.time() * 1000)}])


def full(data, tracker):
    """sample-data.json plus what a signed-in, busy PC would add."""
    data = copy.deepcopy(data)
    now = data["now_epoch"]
    stamp = lambda ago: iso(datetime.fromtimestamp(now - ago, timezone.utc))
    data["plan_limits"] = [
        {"plan": "Max 20x", "service": "Claude Code", "source": "live", "as_of": stamp(0), "error": None, "breakdown": [], "spend": None,
         "windows": [
             {"label": "5-hour session", "used_percent": 82, "resets_at": now + 2 * 3600 + 14 * 60,
              "projection": {"pct": 82.0, "rate_per_hour": 21.5, "hits_at": now + 50 * 60}},
             {"label": "7-day, all models", "used_percent": 46.4, "resets_at": now + 3 * 86400 + 4 * 3600,
              "projection": {"pct": 46.4, "rate_per_hour": 0.19, "at_reset": 60.8}},
             {"label": "7-day, Opus", "used_percent": 12, "resets_at": now + 3 * 86400 + 4 * 3600}]},
        {"plan": "Pro", "service": "Codex", "source": "cached", "as_of": stamp(26 * 3600), "error": "live check failed (offline?)",
         "details": ["Credit balance: 42.5"],
         "windows": [
             {"label": "5-hour", "used_percent": 93, "resets_at": now - 1800},
             {"label": "7-day", "used_percent": 71, "resets_at": now + 5 * 86400 + 600}]},
    ]
    today = datetime.fromtimestamp(now, timezone.utc).strftime("%Y-%m-%d")
    data["api"] = [
        {"provider": "Anthropic", "configured": True, "env": "ANTHROPIC_ADMIN_KEY", "console": "https://platform.claude.com/settings/admin-keys",
         "today": 3.4182, "month": 1284.5731, "last30": 2011.9, "daily": {today: 3.4182}, "tokens_30d": {}, "models": [],
         "source": "live", "as_of": stamp(0), "error": None,
         "balance": {"entered": 250.0, "as_of": stamp(6 * 86400), "spent_since": 61.37, "remaining": 188.63}},
        {"provider": "OpenAI", "configured": True, "env": "OPENAI_ADMIN_KEY", "console": "https://platform.openai.com/settings/organization/admin-keys",
         "today": 0.0, "month": 18.2, "last30": 44.015, "daily": {}, "tokens_30d": {}, "models": [],
         "source": "cached", "as_of": stamp(2 * 86400 + 3600), "error": "OpenAI usage report failed (HTTP 500)",
         "balance": {"entered": 20.0, "as_of": stamp(9 * 86400), "spent_since": 16.58, "remaining": 3.42}},
    ]
    days = sorted(data["daily_tool"])
    data["openai_analytics"] = {"summary": {"lifetimeTokens": 1843250112, "peakDailyTokens": 91234567},
                                "daily": {d: 1000000 * (i + 3) for i, d in enumerate(days[-5:-1])},
                                "source": "live", "as_of": stamp(0), "error": None}
    sessions = tracker.working_details()
    for age, info in zip((8, 41), sessions):  # written a moment ago; give them believable ages
        info["age"] = age
        for sub in info["subagents"]:
            sub["age"] = age + 5
    for info in sessions:
        info["_describe"] = tracker.describe_session(info)
    # Fake-mode extras (see src-tauri/src/core.rs): not part of what collect() returns.
    data["_working_details"] = sessions
    return data


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--home", required=True, help="throwaway folder to use as HOME; fake logs are written there")
    args = ap.parse_args()
    home = Path(args.home).resolve()
    home.mkdir(parents=True, exist_ok=True)
    for name in ("HOME", "USERPROFILE", "LOCALAPPDATA"):
        os.environ[name] = str(home)
    for name in ("CLAUDE_CONFIG_DIR", "CODEX_HOME", "ANTHROPIC_ADMIN_KEY", "OPENAI_ADMIN_KEY"):
        os.environ.pop(name, None)
    os.environ["PATH"] = os.path.dirname(sys.executable)  # no installed `codex` is started
    fake_logs(home, datetime.now().astimezone())

    sys.path.insert(0, str(REPO))
    import tracker
    assert Path(tracker.HOME) == home, "tracker.py must only see the throwaway folder"
    data = tracker.collect(log=lambda *a: None)
    out = WIDGET / "fixtures"
    out.mkdir(exist_ok=True)
    (out / "sample-data.json").write_text(json.dumps(data, indent=1), encoding="utf-8")
    (out / "sample-full.json").write_text(json.dumps(full(data, tracker), indent=1, ensure_ascii=False), encoding="utf-8")
    print(f"wrote {out / 'sample-data.json'} and {out / 'sample-full.json'}")


if __name__ == "__main__":
    main()
