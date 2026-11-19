"""Builds the throwaway HOME the tracker port is recorded against.

    python3 make_home.py <home-dir>

Deterministic: fixed timestamps inside the logs and fixed file times (T0 = 1795000000,
2026-11-18T10:26:40Z), so `working_details(window, now)` with an explicit `now` near T0 is stable.
No usable login is created: the Claude one is expired and the Codex one has no token, so neither
live check reaches the network.
"""
import contextlib
import json
import os
import sqlite3
import sys
from pathlib import Path

T0 = 1795000000
home = Path(sys.argv[1])
claude, codex = home / ".claude", home / ".codex"
UUID1 = "01a19c84-0000-7000-8000-000000000001"
UUID2 = "01a19c84-0000-7000-8000-000000000002"
UUID3 = "01a19c84-0000-7000-8000-000000000003"
UUID4 = "01a19c84-0000-7000-8000-000000000004"


def write(path, rows, tail=b"", mtime=T0 - 86400, compressed=False):
    path.parent.mkdir(parents=True, exist_ok=True)
    data = b"".join((r if isinstance(r, bytes) else json.dumps(r, ensure_ascii=False).encode()) + b"\n" for r in rows) + tail
    if compressed:
        import zstandard
        data = zstandard.ZstdCompressor().compress(data)
    path.write_bytes(data)
    os.utime(path, (mtime, mtime))


def assistant(msg_id, blocks, ts="2026-11-01T10:00:00Z", model="claude-opus-5-5", cwd="C:\\work\\demo", **usage):
    row = {"type": "assistant", "timestamp": ts, "requestId": "r-" + str(msg_id),
           "message": {"id": msg_id, "model": model, "content": blocks,
                       "usage": usage or {"input_tokens": 10, "output_tokens": 5}}}
    if cwd:
        row["cwd"] = cwd
    return row


def tool_use(call_id, name, **args):
    return {"type": "tool_use", "id": call_id, "name": name, "input": args}


turn = {"type": "system", "subtype": "turn_duration", "durationMs": 90000, "uuid": "u1", "timestamp": "2026-11-01T10:00:00Z"}
bash = tool_use("t1", "Bash", command="ls")
long_prompt = "añade una tarjeta — 日本語のテキスト " + "palabra " * 20
write(claude / "projects" / "C--work-demo" / "s1.jsonl", [
    {"type": "custom-title", "customTitle": "Usage tracker ✓", "cwd": "C:\\work\\demo"},
    {"type": "agent-name", "agentName": "ignored once titled"},
    {"type": "last-prompt", "lastPrompt": long_prompt},
    turn, turn, dict(turn, uuid="u2", durationMs=30000.5), dict(turn, uuid="u3", durationMs="long"),
    assistant("m1", [bash]), assistant("m1", [bash]),
    assistant("m2", [tool_use("t2", "Agent", subagent_type="Explore"), tool_use("t3", "Task"),
                     tool_use("t4", "Skill", skill="pdf"), tool_use("t5", "mcp__rea__open_binary"),
                     tool_use("t6", "mcp__solo"), tool_use("t7", "Skill"), {"type": "text", "text": "héllo"},
                     {"type": "tool_use", "id": "t8", "name": None, "input": "not a dict"}, "stray"],
              ts="2026-11-01T23:30:00.250Z", input_tokens=1200, cache_creation_input_tokens=300,
              cache_read_input_tokens=45000, output_tokens=800),
    assistant("m3", [], model="<synthetic>"),
    assistant(None, [tool_use("t9", "Read")], ts="2026-11-02T08:15:00+02:00", model="claude-sonnet-5", cwd=None),
    assistant(None, [], ts="2026-11-02T08:15:00+02:00", model="claude-sonnet-5", cwd=None),
    assistant("m4", [tool_use("t10", "Edit")], ts="not a time", model=None, cwd="/home/me/proj two/"),
    dict(assistant("m5", [], ts="2027-01-03T00:00:01Z", cwd="/home/me/other"), effort="xhigh"),
    {"type": "user", "timestamp": "2026-11-01T10:00:00Z", "message": {"role": "user", "content": "plain text"}},
    {"type": "assistant", "message": "not a dict"},
    b'{"type": "last-prompt", "lastPrompt": "line\xe2\x80\xa8separator inside"}',
    b'{"type": "note", "text": "bad byte \xff here"}',
    b"[1, 2, 3]", b"", b"   ", b"not json",
], tail=b'{"type": "assistant", "mess', mtime=T0 - 20)
sub = claude / "projects" / "C--work-demo" / "s1" / "subagents" / "agent-a.jsonl"
write(sub, [assistant("m6", [tool_use("t11", "Grep")], ts="2026-11-01T10:05:00Z", model="claude-haiku-5-5"),
            assistant("m1", [bash])], mtime=T0 - 5)
sub.with_suffix(".meta.json").write_text(json.dumps({"agentType": "Explore", "description": "find the widget code"}), encoding="utf-8")
write(claude / "projects" / "C--work-demo" / "s1" / "subagents" / "agent-b.jsonl", [assistant("m7", [])], mtime=T0 - 50)
write(claude / "projects" / "-home-me-proj2" / "s2.jsonl", [
    {"type": "agent-name", "agentName": "Named agent"},
    assistant("m8", [tool_use("t1", "Bash")], ts="2026-10-31T22:59:59Z", model="claude-fable-5-1", cwd=None,
              input_tokens=7, output_tokens=3, cache_read_input_tokens=None),
    assistant("m9", [], ts="2025-12-31T12:00:00Z", model="mystery-model", cwd=None)], mtime=T0 - 100)
write(claude / "projects" / "-home-me-proj2" / "old.jsonl", [assistant("m10", [], ts="2026-11-03T05:00:00Z")])
write(claude / "history.jsonl", [
    {"display": "/model opus", "timestamp": 1793523600000}, {"display": "fix the bug", "timestamp": 1793523600000},
    {"display": "/c/Users/me/file.txt is broken", "timestamp": 1793523600000}, {"display": "  /compact  ", "timestamp": 1793610000500},
    {"display": "/zero"}, {"display": "/text-time", "timestamp": "soon"}, {"display": "/huge", "timestamp": 1e30},
    {"display": 42, "timestamp": 1}, {"display": "/ünï", "timestamp": 1793523600000}, {"display": "/a:b.c-d_é", "timestamp": 1793523600000}])


def meta(session, cwd="C:\\work\\demo", fork=None, ts="2026-11-01T10:00:00Z", **extra):
    return {"type": "session_meta", "timestamp": ts,
            "payload": dict({"id": session, "cwd": cwd, "forked_from_id": fork, "timestamp": ts}, **extra)}


def usage(n, last=None, ts="2026-11-01T10:01:00Z", limits=None):
    def counts(x):
        return {"input_tokens": x * 10, "cached_input_tokens": x * 4, "output_tokens": x * 2,
                "reasoning_output_tokens": x, "total_tokens": x * 12}
    payload = {"type": "token_count", "info": {"total_token_usage": counts(n), "last_token_usage": counts(n if last is None else last)}}
    if limits:
        payload["rate_limits"] = limits
    return {"type": "event_msg", "timestamp": ts, "payload": payload}


def call(call_id, name, namespace=None, ts="2026-11-01T10:05:00Z", kind="function_call"):
    return {"type": "response_item", "timestamp": ts, "payload": {"type": kind, "name": name, "namespace": namespace, "call_id": call_id}}


def said(text, role="user"):
    return {"type": "response_item", "timestamp": "2026-11-01T10:05:00Z",
            "payload": {"type": "message", "role": role, "content": [{"type": "input_text", "text": text}, "stray", {"type": "image"}]}}


week = {"limit_id": "codex", "plan_type": "pro_plus", "primary": {"used_percent": 24, "window_minutes": 10080, "resets_at": 1795300000},
        "secondary": {"used_percent": 3.5, "window_minutes": 300, "resets_at": 1795010000}}
spark = {"limit_id": "spark", "limit_name": "Spark", "primary": {"used_percent": 12, "window_minutes": 15}}
main_rows = [
    meta(UUID1), {"type": "turn_context", "payload": {"model": "gpt-6-astra", "effort": "high", "cwd": "C:\\work\\demo"}},
    said("<environment_context>injected</environment_context>"), said("speed up the importer, por favor — ¡rápido!"),
    said("assistant words", role="assistant"),
    usage(5, last=2, limits=week), usage(5), call("c1", "exec"), call("c1", "exec"), call("c2", "js", "mcp__cua_repl__"),
    call("c3", "spawn_agent", "collaboration"), call(None, "apply_patch", kind="custom_tool_call"),
    call(None, "apply_patch", kind="custom_tool_call"), call("c4", None, ts="bad"),
    {"type": "turn_context", "payload": {"model": "gpt-6.1-sol", "cwd": "/srv/apps/api"}},
    usage(7, ts="2026-11-01T23:59:59.999Z", limits=spark), usage(2, last=2, ts="2026-11-02T00:00:00Z"),
    {"type": "event_msg", "timestamp": "2026-11-02T00:00:01Z", "payload": {"type": "token_count", "info": None, "rate_limits": dict(week, primary=None, secondary=None)}},
    {"type": "event_msg", "payload": "not a dict"}, {"type": "event_msg", "payload": None},
]
write(codex / "sessions" / "2026" / "11" / "01" / f"rollout-2026-11-01T10-00-00-{UUID1}.jsonl", main_rows, tail=b'{"type":', mtime=T0 - 10)
write(codex / "archived_sessions" / f"rollout-2026-11-01T10-00-00-{UUID1}.jsonl.zst",
      main_rows + [usage(9, ts="2026-11-02T09:00:00Z", limits=dict(week, primary={"used_percent": 31, "window_minutes": 10080}))], compressed=True)
(codex / "archived_sessions" / "rollout-corrupt.jsonl.zst").write_bytes(b"corrupt")
write(codex / "sessions" / "2026" / "11" / "01" / f"rollout-2026-11-01T10-30-00-{UUID2}.jsonl", [
    meta(UUID2, cwd="/home/me/proj2", fork=UUID1, ts="2026-11-01T10:30:00Z", parent_thread_id=UUID1, agent_nickname="Ada", thread_source="subagent"),
    {"type": "turn_context", "payload": {"model": "gpt-5.5", "effort": "medium"}},
    usage(5, ts="2026-11-01T10:29:59.999Z"), call("c9", "exec", ts="2026-11-01T10:00:00Z"),
    call("c10", "exec", ts="2026-11-01T10:31:00Z"), usage(8, ts="2026-11-01T10:31:00Z"), said("check the exporter")], mtime=T0 - 15)
write(codex / "sessions" / "2026" / "11" / "02" / f"rollout-2026-11-02T08-00-00-{UUID3}.jsonl", [
    meta(UUID3, cwd=None, fork="gone", ts="2026-11-02T08:00:00Z", parent_thread_id=UUID4, thread_source="guardian_review"),
    usage(7, last=2, ts="2026-11-02T08:01:00Z"), {"type": "turn_context", "payload": {"model": "codex-auto-review"}},
    usage(9, last=1, ts="2026-11-02T08:02:00Z")], mtime=T0 - 40)
write(codex / "sessions" / "loose.jsonl", [usage(1, ts="2026-11-03T03:00:00Z")], mtime=T0 - 60)
write(codex / "history.jsonl", [{"text": "/review please", "ts": 1793523600}, {"text": "/status", "ts": 1793610000.75}, {"text": "hello"}])
write(codex / "session_index.jsonl", [{"id": UUID1, "thread_name": "Speed up import ✈"}, {"id": UUID4, "thread_name": "Quiet parent"},
                                      {"id": "x"}, {"thread_name": "no id"}])
(codex / "auth.json").write_text(json.dumps({"tokens": {"account_id": "none"}}), encoding="utf-8")
(claude / ".credentials.json").write_text(json.dumps({"claudeAiOauth": {"accessToken": "expired-fixture", "expiresAt": 1000}}), encoding="utf-8")

ms = T0 * 1000
for name in ("state_4.sqlite", "state_5.sqlite", "state_x.sqlite"):
    with contextlib.closing(sqlite3.connect(codex / name)) as db:
        db.execute("CREATE TABLE threads (id TEXT, history_mode TEXT, model TEXT, reasoning_effort TEXT, cwd TEXT, name TEXT, "
                   "title TEXT, thread_source TEXT, agent_nickname TEXT, attached_thread_parent_id TEXT, updated_at_ms INTEGER)")
        db.execute("CREATE TABLE thread_spawn_edges (parent_thread_id TEXT, child_thread_id TEXT, status TEXT)")
        if name == "state_5.sqlite":
            db.executemany("INSERT INTO threads VALUES (?,?,?,?,?,?,?,?,?,?,?)", [
                ("t-main", "paginated", "gpt-6-astra", "ultra", "C:\\work\\photo", None, "Review Photul", "user", None, None, ms),
                ("t-kid", "paginated", "gpt-6-astra", "high", "C:\\work\\photo", None, "", "subagent", "Faraday", None, ms - 5000),
                ("t-attached", "paginated", None, None, None, "Named", "Titled", "guardian_review", None, "t-main", None),
                (UUID3, "paginated", "gpt-6-astra", "medium", "C:\\work\\x", None, "Finished", "user", None, None, ms),
                ("t-legacy", "legacy", "gpt-5.5", "minimal", "/srv/legacy/", "Legacy thread", "t", "user", None, None, ms - 90000),
                ("t-crashed", "paginated", "gpt-6-astra", "medium", "C:\\work\\x", None, "Old", "user", None, None, ms - 7200000)])
            db.execute("INSERT INTO thread_spawn_edges VALUES ('t-main', 't-kid', 'open')")
        db.commit()
with contextlib.closing(sqlite3.connect(codex / "thread_history_1.sqlite")) as db:
    db.execute("CREATE TABLE thread_turns (thread_id TEXT, status TEXT)")
    db.execute("CREATE TABLE thread_items (thread_id TEXT, item_type TEXT, item_json TEXT, rollout_ordinal INTEGER, created_at_ms INTEGER)")
    db.executemany("INSERT INTO thread_turns VALUES (?,?)", [
        ("t-main", "completed"), ("t-main", "inProgress"), ("t-kid", "inProgress"), ("t-attached", "inProgress"),
        (UUID3, "completed"), ("t-legacy", "inProgress"), ("t-crashed", "inProgress"), ("t-unknown", "inProgress")])
    prompt = lambda text: json.dumps({"type": "userMessage", "content": [{"type": "text", "text": text}, "stray"]})
    db.executemany("INSERT INTO thread_items VALUES (?,?,?,?,?)", [
        ("t-main", "userMessage", prompt("first ask"), 1, ms), ("t-main", "userMessage", prompt("find the bügs"), 5, ms + 2500),
        ("t-kid", "userMessage", prompt("check the exporter"), 1, ms), ("t-attached", "userMessage", "not json", 1, ms - 1000),
        ("t-legacy", "userMessage", prompt("   "), 1, ms)])
    db.commit()
print(f"fixture home written to {home} (T0 = {T0})")
