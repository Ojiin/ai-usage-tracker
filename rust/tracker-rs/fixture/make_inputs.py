"""Writes extra cases for `rust-anything record --inputs`, from the fixture home (run with HOME set to it).

    HOME=<home-dir> python3 make_inputs.py <repo-root> <out.jsonl>

The cases hold absolute paths into that home: write them again after moving it.
"""
import json
import sys
from pathlib import Path

sys.path.insert(0, sys.argv[1])
import tracker  # noqa: E402

T0 = 1795000000
cases = []


def case(fn, *args):
    cases.append({"fn": fn, "args": list(args)})


for window, now in ((120, T0 + 30), (300, T0 + 30.5), (100000, T0 + 30), (45, T0 + 30), (10, T0 + 30), (0, T0 - 5),
                    (120, T0 + 3000), (-20, T0 - 30), (120, float(T0))):
    case("working_details", window, now)
    case("working_now", window, now)
for now in (T0 + 60, T0 + 1795, T0 + 1801, T0 + 0.5, T0 - 100, T0 + 1800.999):
    case("codex_db_working", now)
logs = sorted(str(p) for root in (tracker.CLAUDE_DIR, tracker.CODEX_DIR.parent) for p in root.rglob("*.jsonl*"))
for path in logs:
    case("claude_session_info", path)
    for size in (None, 0, 1, 150, 300, 1000, 100000, -5, True):
        case("tail_rows", path, *([] if size is None else [size]))
for path in (str(tracker.CLAUDE_DIR), str(tracker.CLAUDE_DIR / "missing.jsonl")):
    case("claude_session_info", path)
    case("tail_rows", path)
sessions = tracker.working_details(100000, T0 + 30)
for info in sessions:
    case("describe_session", info)
    case("describe_session", dict(info, effort="minimal", title=None, model="claude-sonnet-5", project=None))
case("describe_session", {"tool": "Codex", "age": 3})
case("describe_session", {"tool": "Codex", "age": 3.5, "effort": "xhigh",
                          "subagents": [{"type": "x agent"}, {"type": 7, "description": 8, "effort": "medium"}]})
case("describe_session", {"tool": "Codex", "age": 3, "subagents": [{}]})
case("describe_session", {"tool": "Codex", "age": 3, "model": 5})
case("describe_session", {"tool": "Codex", "age": 3, "effort": ["x"]})
case("describe_session", {"tool": 4, "age": 3})
events = []
claude = tracker.scan_claude(events)
codex, limits = tracker.scan_codex(events)
tracker.scan_commands(events)
pricing = tracker.load_pricing()
case("aggregate", claude + codex, pricing, events)
case("aggregate", claude + codex, {}, events)
case("aggregate", claude, pricing)
case("aggregate", codex[::-1], pricing, events[::-1])
case("aggregate", [dict(r, hour=-1, input=1.5, reasoning=2.5) for r in codex[:3]], {"gpt-6-astra": {"input": 0.1, "output": 0.7}}, [])
case("aggregate", [dict(codex[0], hour=24)], {})
case("aggregate", [dict(codex[0], hour=None, tool="Other")], {"gpt-6-astra": 0},
     [{"kind": "turn", "tool": "Other", "ms": 5}, {"kind": "turn", "tool": "Other", "ms": 2.5}])
for rec in claude + codex:
    case("cost_of", rec, pricing)
for n in range(12):
    rec = {"model": "m", "input": 10 ** n + 7, "cache_write": 3 * n + 1, "cache_read": 123456789 * (n + 1), "output": 99991 * n}
    case("cost_of", rec, {"m": {"input": 0.1 * (n + 1), "cache_write": 1e16 / 3, "cache_read": 0.3, "output": -0.7 / (n + 1)}})
    case("cost_of", rec, {"m": {"input": 3, "cache_write": 0.1, "cache_read": 1e-9, "output": 2.675}})
    case("cost_of", dict(rec, input=0.1 * n, output=1e15 + 0.3), {"m": {"input": 3, "cache_read": 0.1, "output": 7}})
case("cost_of", claude[0], {})
case("cost_of", {"model": None}, {"m": 1})
case("cost_of", {"model": "m", "input": 1}, {"m": {"input": 1}})
case("codex_log_limits", limits)
case("codex_log_limits", limits["codex"])
case("codex_log_limits", {
    "a": {"timestamp": "2026-11-01T10:00:00Z", "plan_type": "team", "limit_name": "Team", "limit_reached": True,
          "credits": {"has_credits": False}, "primary": {"used_percent": 0, "window_minutes": 90.5, "resets_at": 5},
          "secondary": {"used_percent": None, "window_minutes": 60}},
    "b": {"timestamp": "2026-11-01T10:00:00Z", "plan_type": "pro", "primary": {"used_percent": 1.5, "window_minutes": 43200}}})
for body in ({"credits": {"unlimited": True}, "rate_limit": {"allowed": False}}, {"credits": {"balance": 0}, "rate_limit_reached_type": "x"},
             {"credits": {"has_credits": False}, "allowed": False, "limit_reached": 1}, {"credits": {"balance": 12.50, "unlimited": 0}},
             {"credits": 5}, {"rate_limit": "x"}, {"credits": {"has_credits": 0}}):
    case("codex_details", body)
for text in ("  several   words\there\n", "x" * 90, "y" * 91, "añade — 日本語 " * 20, "a b\x1cc\x85d", "", None, 0, 12.5,
             ["a", 1], {"k": None, "t": True}):
    for limit in (90, 70, 5, 1, 0, -3):
        case("short_text", text, limit)
for minutes in (0, 15, 60, 90, 300, 1440, 10080, 43200, 1500, -60, -90, 60.0, 90.5, 2880.0, 0.0, 7, 1441, 1e16):
    case("window_label", minutes)
for model in ("claude-opus-5-5", "claude-sonnet-5", "claude-fable-5-1-20261101", "claude-", "claude--5", "claude-3-HAIKU-x2",
              "gpt-6.1-sol", "CLAUDE-x", "", None):
    case("pretty_model", model)
for plan in ("pro_plus", "default_claude_max_20x", " team  PLAN ", "", None, "éCOLE_ünï", "a__b"):
    case("pretty_plan", plan)
stamps = ("2026-11-01T10:00:00Z", "2026-11-01T23:30:00.250Z", "2026-11-02T08:15:00+02:00", "2026-11-01 10:00:00",
          "2026-11-01T10:00:00.123456", "2026-11-01", "2026-12-31T23:59:59.999999-11:00", "1969-12-31T23:59:59.5+00:00",
          "2026-11-01T10:00", "not a time", "", "2026-13-01T00:00:00Z", "2026-11-01T10:00:00+0530", "2027-01-01T00:00:00.1Z", None, 5)
for ts in stamps:
    for fn in ("to_epoch", "local_day", "local_time"):
        case(fn, ts)
for name, args in (("Bash", {}), ("Agent", {"subagent_type": "Explore"}), ("Task", {"subagent_type": ""}), ("Agent", {"subagent_type": 7}),
                   ("Skill", {"skill": "pdf"}), ("Skill", {"skill": 12.5}), ("Skill", {}), ("mcp__rea__open", {}), ("mcp__solo", 5),
                   ("mcp____x", {}), ("mcp__a__b__c", {}), ("Agent", 5), ("Skill", None), ("", {})):
    case("claude_tool_events", name, args)
for fn in ("scan_claude", "scan_codex", "scan_commands"):
    case(fn, [])
    case(fn, [{"kept": True}])
# `aggregate` is not bridged: its cases go to a file of their own (they are among the recorded cases
# in aggregate-cases.jsonl, which the port's `aggregate_matches_its_recorded_cases` test replays).
out = Path(sys.argv[2])
for name, wanted in ((out, False), (out.with_name("aggregate-inputs.jsonl"), True)):
    chosen = [c for c in cases if (c["fn"] == "aggregate") == wanted]
    name.write_text("".join(json.dumps(c, ensure_ascii=False) + "\n" for c in chosen), encoding="utf-8")
    print(f"{len(chosen)} cases in {name}")
