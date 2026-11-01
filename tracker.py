"""AI usage tracker: scans local Claude Code and Codex logs, checks live plan limits, writes an HTML dashboard.

Usage:  python tracker.py [--out dashboard.html] [--no-open]
"""
import argparse
import openai_analytics
import re
import contextlib
import io
import json
import os
import sys
import urllib.error
import urllib.request
import webbrowser
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path
from pathlib import PureWindowsPath

import api_billing
import limits_watch

HOME = Path.home()
CLAUDE_DIR = Path(os.environ.get("CLAUDE_CONFIG_DIR", HOME / ".claude")) / "projects"
CODEX_DIR = Path(os.environ.get("CODEX_HOME", HOME / ".codex")) / "sessions"
# Prompt history; only slash-command names are read from it, never the prompt text.
CLAUDE_HISTORY = CLAUDE_DIR.parent / "history.jsonl"
CODEX_HISTORY = CODEX_DIR.parent / "history.jsonl"
# When frozen by PyInstaller: user files (pricing.json, dashboard.html) sit beside the exe,
# bundled files (template.html) are unpacked to sys._MEIPASS.
HERE = Path(sys.executable).parent if getattr(sys, "frozen", False) else Path(__file__).resolve().parent
RES = Path(getattr(sys, "_MEIPASS", HERE))
# Last good live reading per service, shown (with its age) when a live check fails.
LIMITS_CACHE = Path(os.environ.get("LOCALAPPDATA", HOME)) / "AIUsageTracker" / "limits_cache.json"

FIELDS = ("input", "cache_write", "cache_read", "output")
SCAN_WARNINGS = []


def local_day(ts):
    """ISO timestamp (UTC 'Z') -> local YYYY-MM-DD, or None."""
    if not ts:
        return None
    try:
        dt = datetime.fromisoformat(ts.replace("Z", "+00:00"))
    except ValueError:
        return None
    return dt.astimezone().strftime("%Y-%m-%d")


def local_time(ts):
    """ISO timestamp -> (local YYYY-MM-DD, hour 0-23, weekday 0=Mon), or (None, None, None)."""
    try:
        dt = datetime.fromisoformat(ts.replace("Z", "+00:00")).astimezone()
    except (AttributeError, ValueError):
        return None, None, None
    return dt.strftime("%Y-%m-%d"), dt.hour, dt.weekday()


def claude_tool_events(name, args):
    """Activity events for one Claude Code tool call: the tool, plus agent type, skill or MCP server."""
    if name.startswith("mcp__"):
        server = name.split("__")[1] if name.count("__") >= 2 else name[5:]
        return [("tool", f"MCP: {server}"), ("mcp", server)]
    events = [("tool", name)]
    if name in ("Agent", "Task"):
        events.append(("agent", args.get("subagent_type") or "general-purpose"))
    elif name == "Skill" and args.get("skill"):
        events.append(("skill", str(args["skill"])))
    return events


COMMAND = re.compile(r"^/[A-Za-z][\w:.-]*$")


def scan_commands(events):
    """Slash commands typed in Claude Code and Codex, by name only."""
    for tool, path, text_key, time_of in (
            ("Claude Code", CLAUDE_HISTORY, "display", lambda r: (r.get("timestamp") or 0) / 1000),
            ("Codex", CODEX_HISTORY, "text", lambda r: r.get("ts") or 0)):
        if not path.exists():
            continue
        for row in iter_jsonl(path):
            words = str(row.get(text_key) or "").split(maxsplit=1)
            if not words or not COMMAND.match(words[0]):
                continue
            try:
                day = datetime.fromtimestamp(time_of(row)).strftime("%Y-%m-%d")
            except (OSError, OverflowError, ValueError, TypeError):
                continue
            events.append({"tool": tool, "day": day, "kind": "command", "name": words[0]})


def working_now(window=120, now=None):
    """Working sessions per tool (a subagent counts as part of its parent session)."""
    active = {"Claude Code": 0, "Codex": 0}
    for info in working_details(window, now):
        active[info["tool"]] += 1
    return active


def tail_rows(path, size=512 * 1024):
    """JSON rows from the last `size` bytes of a log, oldest first; a cut-off first line is skipped."""
    try:
        with open(path, "rb") as fh:
            fh.seek(0, os.SEEK_END)
            start = max(fh.tell() - size, 0)
            fh.seek(start)
            lines = fh.read().decode("utf-8", errors="replace").splitlines()
    except OSError:
        return []
    rows = []
    for line in lines[1:] if start else lines:
        try:
            row = json.loads(line)
        except ValueError:
            continue
        if isinstance(row, dict):
            rows.append(row)
    return rows


def pretty_model(model):
    """'claude-opus-5-5' -> 'Opus 5.5'; other names are kept as they are."""
    if not model or not model.startswith("claude-"):
        return model or "unknown model"
    words, version = [], []
    for part in model[7:].split("-"):
        (version if part.isdigit() else words).append(part)
    return " ".join([w.capitalize() for w in words] + ([".".join(version)] if version else []))


EFFORT = {"medium": "med", "xhigh": "xhigh", "minimal": "min"}


def short_text(text, limit=90):
    text = " ".join(str(text or "").split())
    return text if len(text) <= limit else text[:limit - 1] + "…"


def codex_titles():
    titles = {}
    for row in iter_jsonl(CODEX_DIR.parent / "session_index.jsonl") if (CODEX_DIR.parent / "session_index.jsonl").exists() else ():
        if row.get("id") and row.get("thread_name"):
            titles[row["id"]] = row["thread_name"]
    return titles


def claude_session_info(path):
    info = {"tool": "Claude Code", "model": None, "effort": None, "project": None, "title": None, "task": None}
    for row in tail_rows(path):
        if row.get("cwd"):
            info["project"] = PureWindowsPath(row["cwd"]).name
        kind = row.get("type")
        if kind == "assistant" and isinstance(row.get("message"), dict) and row["message"].get("model") not in (None, "<synthetic>"):
            info["model"] = row["message"]["model"]
            info["effort"] = row.get("effort") or info["effort"]
        elif kind == "custom-title" and row.get("customTitle"):
            info["title"] = row["customTitle"]
        elif kind == "agent-name" and row.get("agentName") and not info["title"]:
            info["title"] = row["agentName"]
        elif kind == "last-prompt" and row.get("lastPrompt"):
            info["task"] = short_text(row["lastPrompt"])
    return info


def codex_session_info(path, titles):
    info = {"tool": "Codex", "model": None, "effort": None, "project": None, "title": None, "task": None,
            "id": None, "parent": None, "nickname": None, "source": None}
    for row in iter_jsonl(path):  # session_meta is the first line
        meta = row.get("payload") if row.get("type") == "session_meta" and isinstance(row.get("payload"), dict) else {}
        info.update(id=meta.get("id"), parent=meta.get("parent_thread_id"), nickname=meta.get("agent_nickname"),
                    source=meta.get("thread_source"))
        if meta.get("cwd"):
            info["project"] = PureWindowsPath(meta["cwd"]).name
        break

    def read(rows):
        for row in rows:
            payload = row.get("payload") if isinstance(row.get("payload"), dict) else {}
            if row.get("type") == "turn_context":
                info["model"] = payload.get("model") or info["model"]
                info["effort"] = payload.get("effort") or info["effort"]
                if payload.get("cwd"):
                    info["project"] = PureWindowsPath(payload["cwd"]).name
            elif row.get("type") == "response_item" and payload.get("type") == "message" and payload.get("role") == "user":
                text = " ".join(c.get("text", "") for c in payload.get("content") or [] if isinstance(c, dict))
                if text.strip() and not text.lstrip().startswith("<"):  # skip injected context blocks
                    info["task"] = short_text(text)
    read(tail_rows(path))
    if not info["model"]:  # turn contexts are rare in long subagent logs; look through the whole file
        read(iter_jsonl(path))
    info["title"] = titles.get(info["id"])
    return info


def working_details(window=120, now=None):
    """What each working session is doing: model, effort, project, title, latest prompt, and its subagents.

    Reads the end of each recently written log only; nothing is sent anywhere.
    """
    now = now or datetime.now().timestamp()
    sessions, subagents = {}, {}
    titles = None
    for tool, root in (("Claude Code", CLAUDE_DIR), ("Codex", CODEX_DIR)):
        if not root.exists():
            continue
        for path in root.rglob("*.jsonl"):
            try:
                age = int(now - path.stat().st_mtime)
            except OSError:
                continue
            if age > window:
                continue
            if tool == "Claude Code" and path.parent.name == "subagents":
                parent = str(path.parent.parent.with_suffix(".jsonl"))
                try:
                    meta = json.loads(path.with_suffix(".meta.json").read_text(encoding="utf-8"))
                except (OSError, ValueError):
                    meta = {}
                sub = claude_session_info(path)
                subagents.setdefault(parent, []).append({
                    "type": f'{meta.get("agentType") or "sub"} agent', "description": short_text(meta.get("description"), 70),
                    "model": sub["model"], "effort": sub["effort"], "age": age})
                if parent not in sessions:  # parent idle while its subagent works: describe it from its own log
                    sessions[parent] = dict(claude_session_info(Path(parent)), age=age)
                sessions[parent]["age"] = min(sessions[parent]["age"], age)
            elif tool == "Codex":
                titles = codex_titles() if titles is None else titles
                info = codex_session_info(path, titles)
                if info["parent"]:
                    kind = "review agent" if info["source"] == "guardian_review" else "subagent"
                    subagents.setdefault("codex:" + info["parent"], []).append({
                        "type": f'{info["nickname"]} ({kind})' if info["nickname"] else kind,
                        "description": info["task"], "model": info["model"], "effort": info["effort"], "age": age,
                        "project": info["project"]})
                else:
                    sessions["codex:" + (info["id"] or str(path))] = dict(info, age=age)
            else:
                sessions.setdefault(str(path), dict(claude_session_info(path), age=age))
                sessions[str(path)]["age"] = min(sessions[str(path)]["age"], age)
    for key, subs in subagents.items():
        if key not in sessions:  # a Codex subagent whose parent thread is quiet
            sessions[key] = {"tool": "Codex", "model": None, "effort": None, "project": subs[0].get("project"),
                             "title": (titles or {}).get(key[6:]), "task": None, "age": min(x["age"] for x in subs)}
        sessions[key]["subagents"] = sorted(subs, key=lambda x: x["age"])
    out = [dict(info, subagents=info.get("subagents", [])) for info in sessions.values()]
    for info in out:
        for k in ("id", "parent", "nickname", "source"):
            info.pop(k, None)
    return sorted(out, key=lambda i: i["age"])


def describe_session(info):
    """'Opus 5.5 med working on scripts' plus detail lines, for the widget's hover card."""
    effort = EFFORT.get(info.get("effort"), info.get("effort"))
    head = " ".join(x for x in (pretty_model(info["model"]) if info.get("model") else info.get("tool"), effort) if x)
    lines = [f'{head} working on {info.get("project") or "an unknown project"}']
    if info.get("title"):
        lines.append(f'  “{info["title"]}”')
    if info.get("task"):
        lines.append(f'  Last prompt: {info["task"]}')
    for sub in info.get("subagents", []):
        model = " ".join(x for x in (pretty_model(sub.get("model")) if sub.get("model") else None,
                                     EFFORT.get(sub.get("effort"), sub.get("effort"))) if x)
        lines.append(f'  ↳ {sub["type"]}' + (f" · {model}" if model else "") +
                     (f': {sub["description"]}' if sub.get("description") else ""))
    lines.append(f'  {info["tool"]} · last write {info["age"]}s ago')
    return "\n".join(lines)


def iter_jsonl(path):
    try:
        with contextlib.ExitStack() as stack:
            if path.suffix == ".zst":
                import zstandard
                fh = stack.enter_context(open(path, "rb"))
                stream = stack.enter_context(io.TextIOWrapper(
                    zstandard.ZstdDecompressor().stream_reader(fh), encoding="utf-8", errors="replace"))
            else:
                stream = stack.enter_context(open(path, encoding="utf-8", errors="replace"))
            for line in stream:
                try:
                    row = json.loads(line)
                    if isinstance(row, dict):
                        yield row
                except json.JSONDecodeError:
                    continue  # a running session may end with an incomplete line
    except Exception as exc:
        SCAN_WARNINGS.append(f"Could not fully read {path.name} ({type(exc).__name__}); totals may be incomplete.")


def scan_claude(events=None):
    """One record per API response; Claude Code writes a line per content block with the same usage.

    Tool calls (each counted once by its id) are appended to `events` when given.
    """
    records, seen, calls = [], set(), set()
    if not CLAUDE_DIR.exists():
        return records
    for path in CLAUDE_DIR.rglob("*.jsonl"):
        project = path.relative_to(CLAUDE_DIR).parts[0]
        session = path.parent.parent.name if path.parent.name == "subagents" else path.stem
        for row in iter_jsonl(path):
            if events is not None and row.get("type") == "system" and row.get("subtype") == "turn_duration":
                if isinstance(row.get("durationMs"), (int, float)) and row.get("uuid") not in calls:
                    calls.add(row.get("uuid"))
                    events.append({"tool": "Claude Code", "day": local_day(row.get("timestamp")), "kind": "turn", "ms": row["durationMs"]})
                continue
            msg = row.get("message")
            if events is not None and isinstance(msg, dict) and isinstance(msg.get("content"), list):
                for block in msg["content"]:
                    if isinstance(block, dict) and block.get("type") == "tool_use" and block.get("id") not in calls:
                        calls.add(block.get("id"))
                        args = block.get("input") if isinstance(block.get("input"), dict) else {}
                        day = local_day(row.get("timestamp"))
                        if day:
                            events.extend({"tool": "Claude Code", "day": day, "kind": k, "name": n}
                                          for k, n in claude_tool_events(str(block.get("name") or "?"), args))
            if not isinstance(msg, dict) or not isinstance(msg.get("usage"), dict):
                continue
            model = msg.get("model") or "unknown"
            if model == "<synthetic>":
                continue
            key = (msg.get("id"), row.get("requestId"))
            if key[0] and key in seen:
                continue
            seen.add(key)
            u = msg["usage"]
            day, hour, dow = local_time(row.get("timestamp"))
            records.append({
                "tool": "Claude Code",
                "day": day, "hour": hour, "dow": dow,
                "model": model,
                "project": Path(row["cwd"]).name if row.get("cwd") else project,
                "session": session,
                "input": u.get("input_tokens") or 0,
                "cache_write": u.get("cache_creation_input_tokens") or 0,
                "cache_read": u.get("cache_read_input_tokens") or 0,
                "output": u.get("output_tokens") or 0,
            })
    return records


def scan_codex(events=None):
    """Count cumulative usage once, including archives, excluding inherited fork history.

    Tool calls are appended to `events` when given (inherited fork history excluded).
    """
    records, limits_by_id, seen, session_totals, calls = [], {}, set(), {}, set()
    paths = set()
    for root in (CODEX_DIR, CODEX_DIR.parent / "archived_sessions"):
        if root.exists():
            paths.update(p for p in root.rglob("*.jsonl*") if p.name.endswith((".jsonl", ".jsonl.zst")))
    for path in sorted(paths, key=lambda p: (p.name.removesuffix('.zst'), str(p))):
        model, project, prev = "unknown", "unknown", None
        session, fork_time = path.name.split(".")[0], None
        for row in iter_jsonl(path):
            payload = row.get("payload") or {}
            if not isinstance(payload, dict):
                continue
            kind = row.get("type")
            if kind == "session_meta":
                session = payload.get("id") or session
                if payload.get("forked_from_id"):
                    fork_time = to_epoch(payload.get("timestamp") or row.get("timestamp"))
                if payload.get("cwd"):
                    project = PureWindowsPath(payload["cwd"]).name
            elif kind == "turn_context":
                model = payload.get("model") or model
                if payload.get("cwd"):
                    project = PureWindowsPath(payload["cwd"]).name
            elif kind == "response_item" and events is not None and payload.get("type") in ("function_call", "custom_tool_call"):
                call_id = payload.get("call_id") or payload.get("id")
                day = local_day(row.get("timestamp"))
                inherited = fork_time is not None and (to_epoch(row.get("timestamp")) or 0) < fork_time
                if day and not inherited and (call_id is None or (session, call_id) not in calls):
                    calls.add((session, call_id))
                    name, namespace = str(payload.get("name") or "?"), str(payload.get("namespace") or "")
                    found = [("tool", name)]
                    if namespace.startswith("mcp__"):
                        server = namespace[5:].strip("_")
                        found = [("tool", f"MCP: {server}"), ("mcp", server)]
                    elif name == "spawn_agent":
                        found.append(("agent", "Codex subagent"))
                    events.extend({"tool": "Codex", "day": day, "kind": k, "name": n} for k, n in found)
            elif kind == "event_msg" and payload.get("type") == "token_count":
                limits = payload.get("rate_limits")
                has_window = limits and (limits.get("primary") or limits.get("secondary"))
                limit_id = (limits or {}).get("limit_id") or "codex"
                prior = limits_by_id.get(limit_id)
                if has_window and (prior is None or (to_epoch(row.get("timestamp")) or 0) >= (to_epoch(prior["timestamp"]) or 0)):
                    limits_by_id[limit_id] = dict(limits, timestamp=row.get("timestamp", ""))
                info = payload.get("info") or {}
                total = info.get("total_token_usage")
                if not total:
                    continue
                inherited = fork_time is not None and (to_epoch(row.get("timestamp")) or 0) < fork_time
                if inherited:
                    prev = total
                    continue
                key = (session, row.get("timestamp"), json.dumps(total, sort_keys=True))
                if key in seen:
                    continue
                seen.add(key)
                event_time = to_epoch(row.get("timestamp")) or 0
                if session in session_totals:
                    previous_time, previous_total = session_totals[session]
                    if event_time < previous_time:
                        continue  # an older copy of an already scanned session
                    prev = previous_total
                if prev is None:
                    # A normal session's first snapshot can include multiple requests.
                    # A fork with no copied baseline can include its parent's usage.
                    delta = (info.get("last_token_usage") or {}) if fork_time is not None else total
                elif any(total.get(k, 0) < prev.get(k, 0) for k in ("input_tokens", "output_tokens")):
                    delta = info.get("last_token_usage") or total
                else:
                    delta = {k: max(total.get(k, 0) - prev.get(k, 0), 0) for k in total}
                prev = total
                session_totals[session] = (event_time, total)
                if not (delta.get("input_tokens", 0) or delta.get("output_tokens", 0)):
                    continue  # repeated snapshot, no new usage
                cached = min(max(delta.get("cached_input_tokens", 0), 0), delta.get("input_tokens", 0))
                day, hour, dow = local_time(row.get("timestamp"))
                records.append({
                    "tool": "Codex",
                    "day": day, "hour": hour, "dow": dow,
                    "model": model,
                    "project": project,
                    "session": session,
                    "input": max(delta.get("input_tokens", 0) - cached, 0),
                    "cache_write": delta.get("cache_write_input_tokens", 0),
                    "cache_read": cached,
                    "output": delta.get("output_tokens", 0),
                    "reasoning": delta.get("reasoning_output_tokens", 0),
                })
    return records, limits_by_id or None


# Plan limits. Both apps fetch these from undocumented endpoints with the login they saved on disk;
# we do the same, read-only: logins are never renewed or rewritten, so the apps' own sessions are untouched.
CODEX_USAGE_URL = "https://chatgpt.com/backend-api/wham/usage"
CLAUDE_USAGE_URL = "https://api.anthropic.com/api/oauth/usage"
CLAUDE_CREDENTIALS = Path(os.environ.get("CLAUDE_CONFIG_DIR", HOME / ".claude")) / ".credentials.json"


def utc_now_iso():
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.000Z")


def to_epoch(iso):
    try:
        return datetime.fromisoformat(iso.replace("Z", "+00:00")).timestamp()
    except (AttributeError, ValueError):
        return None


def pretty_plan(raw):
    return " ".join(w.capitalize() for w in raw.replace("_", " ").split()) if raw else None


def window_label(minutes):
    if not minutes:
        return "Usage window"
    if minutes % 1440 == 0:
        return f"{minutes // 1440}-day"
    return f"{minutes // 60}-hour" if minutes % 60 == 0 else f"{minutes}-minute"


def get_json(url, headers, app):
    """(body, None) or (None, error message)."""
    try:
        with urllib.request.urlopen(urllib.request.Request(url, headers=headers), timeout=15) as resp:
            return json.loads(resp.read()), None
    except urllib.error.HTTPError as e:
        if e.code in (401, 403):
            return None, f"{app} login expired, open {app} to renew it"
        return None, f"live check failed (HTTP {e.code})"
    except (urllib.error.URLError, OSError, ValueError):
        return None, "live check failed (offline?)"


def codex_live():
    try:
        tokens = json.loads((CODEX_DIR.parent / "auth.json").read_text(encoding="utf-8"))["tokens"]
        token = tokens["access_token"]
    except (OSError, ValueError, KeyError, TypeError):
        return None, "no Codex login found"
    body, error = get_json(CODEX_USAGE_URL, {
        "Authorization": "Bearer " + token,
        "ChatGPT-Account-Id": tokens.get("account_id") or "",
        "User-Agent": "codex_cli_rs",
        "Accept": "application/json",
    }, "Codex")
    if error:
        return None, error
    if not isinstance(body, dict):
        return None, "Codex returned an unrecognized usage response"
    buckets = [("codex", body.get("rate_limit") or {})]
    for extra in body.get("additional_rate_limits") or []:
        if isinstance(extra, dict):
            buckets.append((extra.get("limit_name") or extra.get("metered_feature") or "Additional limit", extra.get("rate_limit") or extra))
    if body.get("code_review_rate_limit"):
        buckets.append(("Code review", body["code_review_rate_limit"]))
    windows = []
    for name, bucket in buckets:
        for w in (bucket.get("primary_window"), bucket.get("secondary_window")):
            if not isinstance(w, dict) or w.get("used_percent") is None:
                continue
            label = window_label((w.get("limit_window_seconds") or 0) // 60)
            windows.append({"label": label if name == "codex" else f"{name}: {label}",
                            "used_percent": w["used_percent"], "resets_at": w.get("reset_at")})
    if not windows:
        return None, "Codex returned no usage windows; showing the last known reading"
    return {"plan": pretty_plan(body.get("plan_type")), "windows": windows,
            "details": codex_details(body)}, None


def codex_details(body):
    """Keep credit units separate from tokens and preserve explicit limit states."""
    details = []
    credits = body.get("credits") or {}
    if credits.get("unlimited"):
        details.append("Credits: unlimited")
    elif credits.get("balance") is not None:
        details.append(f"Credit balance: {credits['balance']}")
    elif credits.get("has_credits") is False:
        details.append("No additional credits available")
    rl = body.get("rate_limit") or body
    if rl.get("limit_reached") or body.get("rate_limit_reached_type"):
        details.append("Usage limit reached")
    if rl.get("allowed") is False:
        details.append("Usage currently blocked by the service")
    if body.get("spend_control_reached"):
        details.append("Workspace spend control reached")
    return details


def claude_live():
    try:
        creds = json.loads(CLAUDE_CREDENTIALS.read_text(encoding="utf-8"))["claudeAiOauth"]
        token = creds["accessToken"]
    except (OSError, ValueError, KeyError, TypeError):
        return None, "no Claude Code login found"
    if (creds.get("expiresAt") or 0) / 1000 < datetime.now().timestamp():
        return None, "Claude Code login expired, open Claude Code to renew it"
    body, error = get_json(CLAUDE_USAGE_URL, {
        "Authorization": "Bearer " + token,
        "anthropic-beta": "oauth-2025-04-20",
        "User-Agent": "claude-cli",
        "Accept": "application/json",
    }, "Claude Code")
    if error:
        return None, error
    names = {"session": "5-hour session", "weekly_all": "7-day, all models"}
    windows = []
    for lim in body.get("limits") or []:
        model = ((lim.get("scope") or {}).get("model") or {}).get("display_name")
        label = names.get(lim.get("kind")) or (f"7-day, {model}" if model else lim.get("kind", "?"))
        windows.append({"label": label, "used_percent": lim.get("percent") or 0, "resets_at": to_epoch(lim.get("resets_at"))})
    if not windows:  # older response shape
        for key, label in (("five_hour", "5-hour session"), ("seven_day", "7-day, all models")):
            if body.get(key):
                windows.append({"label": label, "used_percent": body[key].get("utilization") or 0,
                                "resets_at": to_epoch(body[key].get("resets_at"))})
    breakdown = [{"key": r.get("key"), "name": r.get("display_name"), "percent": r.get("percent")}
                 for r in (body.get("seven_day_breakdown") or {}).get("rows", [])]
    spend = None
    used = (body.get("spend") or {}).get("used") or {}
    if (body.get("spend") or {}).get("enabled") and isinstance(used.get("amount_minor"), int):
        spend = {"used": used["amount_minor"] / 10 ** (used.get("exponent") or 0), "currency": used.get("currency") or ""}
    tier = (creds.get("rateLimitTier") or "").replace("default_claude_", "") or creds.get("subscriptionType")
    return {"plan": pretty_plan(tier), "windows": windows, "breakdown": breakdown, "spend": spend}, None


def codex_log_limits(snapshot):
    """Convert the most recent rate_limits snapshot from the Codex session logs."""
    if not snapshot:
        return None
    buckets = [snapshot] if "timestamp" in snapshot else list(snapshot.values())
    newest = max(buckets, key=lambda b: to_epoch(b.get("timestamp")) or 0)
    windows = []
    for bucket in buckets:
        name = bucket.get("limit_name") or bucket.get("limit_id") or "codex"
        for w in (bucket.get("primary"), bucket.get("secondary")):
            if w and w.get("used_percent") is not None:
                label = window_label(w.get("window_minutes") or 0)
                windows.append({"label": label if name == "codex" else f"{name}: {label}",
                                "used_percent": w["used_percent"], "resets_at": w.get("resets_at"),
                                "as_of": bucket.get("timestamp")})
    return {"plan": pretty_plan(newest.get("plan_type")), "windows": windows,
            "details": codex_details(newest), "as_of": newest.get("timestamp"), "source": "log"}


def plan_limits(codex_snapshot, log=print):
    """One entry per service: live if possible, else the newest of the last live reading and (Codex) the logs."""
    try:
        cache = json.loads(LIMITS_CACHE.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        cache = {}
    entries = []
    for service, fetch, fallback in (("Claude Code", claude_live, None), ("Codex", codex_live, codex_log_limits(codex_snapshot))):
        log(f"Checking live {service} limits ...")
        live, error = fetch()
        log(f"  {error or 'ok'}")
        if live:
            entry = cache[service] = dict(live, as_of=utc_now_iso(), source="live")
        else:
            stale = [e for e in (dict(cache[service], source="cached") if service in cache else None, fallback) if e]
            entry = max(stale, key=lambda e: to_epoch(e.get("as_of")) or 0) if stale else {"windows": []}
        entries.append(dict(entry, service=service, error=error))
    try:
        LIMITS_CACHE.parent.mkdir(parents=True, exist_ok=True)
        LIMITS_CACHE.write_text(json.dumps(cache), encoding="utf-8")
    except OSError:
        pass
    return entries


def load_pricing():
    path = HERE / "pricing.json"
    if not path.exists():
        return {}
    data = json.loads(path.read_text(encoding="utf-8"))
    return {k: v for k, v in data.get("models", {}).items() if any(v.get(f) for f in FIELDS)}


def cost_of(rec, pricing):
    rate = pricing.get(rec["model"])
    if not rate:
        return None
    return sum(rec[f] * rate.get(f, 0) for f in FIELDS) / 1_000_000


def aggregate(records, pricing, events=()):
    def bucket():
        return {**{f: 0 for f in FIELDS}, "reasoning": 0, "total": 0, "cost": 0.0, "priced": True, "calls": 0, "sessions": set()}

    by = {"day": defaultdict(bucket), "model": defaultdict(bucket), "project": defaultdict(bucket), "tool": defaultdict(bucket)}
    daily_tool = defaultdict(lambda: defaultdict(int))
    # API-equivalent dollars per day per tool, plus which days include models with no price
    daily_tool_cost = defaultdict(lambda: defaultdict(float))
    daily_tool_unpriced = defaultdict(set)
    # Tokens per local hour of day, per day and tool, for the time-of-day and weekday charts.
    day_hours = defaultdict(lambda: defaultdict(lambda: [0] * 24))
    year_top = defaultdict(lambda: {"model": defaultdict(int), "project": defaultdict(int)})
    for r in records:
        if not r["day"]:
            continue
        c = cost_of(r, pricing)
        total = sum(r[f] for f in FIELDS)
        daily_tool[r["day"]][r["tool"]] += total
        if r.get("hour") is not None:
            day_hours[r["day"]][r["tool"]][r["hour"]] += total
        year = year_top[r["day"][:4]]
        year["model"][f'{r["model"]}|{r["tool"]}'] += total
        year["project"][f'{r["project"]}|{r["tool"]}'] += total
        if c is None:
            daily_tool_unpriced[r["day"]].add(r["tool"])
        else:
            daily_tool_cost[r["day"]][r["tool"]] += c
        keys = {"day": r["day"], "model": f'{r["model"]}|{r["tool"]}', "project": f'{r["project"]}|{r["tool"]}', "tool": r["tool"]}
        for dim, key in keys.items():
            b = by[dim][key]
            for f in FIELDS:
                b[f] += r[f]
            b["total"] += total
            b["reasoning"] += r.get("reasoning", 0)  # already included in output, never add twice
            b["calls"] += 1
            b["sessions"].add((r["tool"], r["session"]))
            if c is None:
                b["priced"] = False
            else:
                b["cost"] += c
    out = {}
    for dim, rows in by.items():
        out[dim] = [{**{k: v for k, v in b.items() if k != "sessions"}, "key": k, "session_count": len(b["sessions"])} for k, b in rows.items()]
    out["daily_tool"] = {d: dict(t) for d, t in sorted(daily_tool.items())}
    out["daily_tool_cost"] = {d: {k: round(v, 4) for k, v in t.items()} for d, t in sorted(daily_tool_cost.items())}
    out["daily_tool_unpriced"] = {d: sorted(t) for d, t in sorted(daily_tool_unpriced.items())}
    out["day_hours"] = {d: dict(t) for d, t in sorted(day_hours.items())}
    out["year_top"] = {y: {dim: sorted(([k, v] for k, v in t[dim].items()), key=lambda kv: -kv[1])[:5] for dim in t}
                       for y, t in year_top.items()}
    # activity[kind]["name|tool"][day] = count; kinds: tool, agent, skill, mcp, command
    activity = defaultdict(lambda: defaultdict(lambda: defaultdict(int)))
    longest_turn = {}
    for e in events:
        if e["kind"] == "turn":
            longest_turn[e["tool"]] = max(longest_turn.get(e["tool"], 0), e["ms"])
            continue
        activity[e["kind"]][f'{e["name"]}|{e["tool"]}'][e["day"]] += 1
    out["longest_turn_ms"] = longest_turn
    out["activity"] = {k: {n: dict(d) for n, d in names.items()} for k, names in activity.items()}
    return out


def collect(log=print):
    SCAN_WARNINGS.clear()
    log("Scanning Claude Code logs ...")
    events = []
    claude = scan_claude(events)
    log(f"  {len(claude)} API responses")
    log("Scanning Codex logs ...")
    codex, limits = scan_codex(events)
    scan_commands(events)
    log(f"  {len(codex)} turns with usage")
    pricing = load_pricing()
    data = aggregate(claude + codex, pricing, events)
    data["plan_limits"] = plan_limits(limits, log)
    data["api"] = api_billing.api_usage(log)
    data["openai_analytics"] = openai_analytics.collect(log)
    data["has_pricing"] = bool(pricing)
    data["generated"] = datetime.now().strftime("%Y-%m-%d %H:%M")
    data["today"] = datetime.now().strftime("%Y-%m-%d")
    data["now_epoch"] = int(datetime.now(timezone.utc).timestamp())
    limits_watch.record_and_project(data["plan_limits"], data["now_epoch"])
    data["warnings"] = list(SCAN_WARNINGS)
    return data


def write_dashboard(data, out_path):
    template = (RES / "template.html").read_text(encoding="utf-8")
    out_path.write_text(template.replace("/*__DATA__*/null", json.dumps(data).replace("<", "\\u003c")), encoding="utf-8")


def build(out_path):
    write_dashboard(collect(), out_path)
    print(f"Wrote {out_path}")


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--out", default=str(HERE / "dashboard.html"))
    ap.add_argument("--no-open", action="store_true", help="don't open the dashboard in the browser")
    ap.add_argument("--set-balance", nargs=2, metavar=("PROVIDER", "USD"),
                    help="record your current API credit balance, e.g. --set-balance anthropic 25.00 (use 'clear' to remove)")
    args = ap.parse_args()
    if args.set_balance:
        name = {p.lower(): p for p in api_billing.PROVIDERS}.get(args.set_balance[0].lower())
        if not name:
            ap.error(f"provider must be one of: {', '.join(api_billing.PROVIDERS)}")
        amount = None if args.set_balance[1].lower() == "clear" else float(args.set_balance[1].lstrip("$"))
        api_billing.set_balance(name, amount)
        print(f"{name} balance {'cleared' if amount is None else f'set to ${amount:,.2f}'}")
    out = Path(args.out).resolve()
    build(out)
    if not args.no_open:
        webbrowser.open(out.as_uri())


if __name__ == "__main__":
    main()
