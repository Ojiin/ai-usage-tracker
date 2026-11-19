"""Writes what the Python original gives for the fixture home, for the port's `fixture_home` test.

    HOME=<home-dir> python3 make_expected.py <repo-root> <expected.json>

Covers what the bridge cannot show: the events the scans append, the scan warnings, Codex session
details, the plan limits with their cache, `collect` (with the two other modules replaced) and the
dashboard page. The limits cache and history are written to a temporary folder.
"""
import json
import sys
import tempfile
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, sys.argv[1])
import api_billing  # noqa: E402
import limits_watch  # noqa: E402
import openai_analytics  # noqa: E402
import tracker  # noqa: E402

T0 = 1795000000
out = {}
events = []
tracker.SCAN_WARNINGS.clear()
out["claude"] = tracker.scan_claude(events)
out["codex"] = tracker.scan_codex(events)
tracker.scan_commands(events)
out["events"] = events
out["warnings"] = list(tracker.SCAN_WARNINGS)
titles = tracker.codex_titles()
logs = sorted(tracker.CODEX_DIR.rglob("*.jsonl"))
out["sessions"] = {p.relative_to(tracker.CODEX_DIR).as_posix(): tracker.codex_session_info(p, titles) for p in logs}
out["working"] = tracker.working_details(100000, T0 + 30)
live = {"plan": "Max", "windows": [{"label": "5-hour session", "used_percent": 40, "resets_at": T0 + 3600.0}]}
stale = {"Codex": {"plan": "Plus", "windows": [], "as_of": "2026-11-02T10:00:00.000Z", "source": "live"}, "Other": 1}
for name, cache, claude, codex in (("fresh", None, (live, None), (None, "live check failed (offline?)")),
                                   ("cached", stale, (None, "no Claude Code login found"), (None, "live check failed (HTTP 500)")),
                                   ("broken", "not json", (None, "x"), ({"plan": None, "windows": [], "details": []}, None))):
    with tempfile.TemporaryDirectory() as tmp:
        path, lines = Path(tmp) / "nested" / "limits_cache.json", []
        if cache is not None:
            path.parent.mkdir()
            path.write_text(cache if isinstance(cache, str) else json.dumps(cache), encoding="utf-8")
        with patch.object(tracker, "LIMITS_CACHE", path), patch.object(tracker, "claude_live", lambda: claude), \
                patch.object(tracker, "codex_live", lambda: codex), \
                patch.object(tracker, "utc_now_iso", lambda: "2026-11-03T00:00:00.000Z"):
            entries = tracker.plan_limits(out["codex"][1], lines.append)
        out["plan_" + name] = {"cache": cache, "claude": claude, "codex": codex, "entries": entries, "log": lines,
                               "saved": json.loads(path.read_text(encoding="utf-8"))}


def part(name):
    def run(log):
        log(f"Reading {name} ...")
        return {"stub": name}
    return run


with tempfile.TemporaryDirectory() as tmp:
    lines = []
    with patch.object(tracker, "LIMITS_CACHE", Path(tmp) / "limits_cache.json"), \
            patch.object(limits_watch, "HISTORY", Path(tmp) / "limits_history.json"), \
            patch.object(api_billing, "api_usage", part("api")), patch.object(openai_analytics, "collect", part("openai")):
        data = tracker.collect(lines.append)
        page = Path(tmp) / "dashboard.html"
        tracker.write_dashboard(data, page)
        out["dashboard"] = page.read_bytes().decode("utf-8")
    out["collect"], out["collect_log"] = data, lines
Path(sys.argv[2]).write_text(json.dumps(out, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
print("expected results written:", ", ".join(out))
