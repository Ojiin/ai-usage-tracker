"""Plan-limit history, pace projection and alert thresholds.

Each scan's limit readings are saved per window ("Claude Code|5-hour session") with the time
the service reported them, so re-served cached readings add nothing. The recent rate of change
projects when a window would reach 100%. Alerts fire once per threshold per window.
"""
import json
import os
from datetime import datetime, timezone
from pathlib import Path

STORE = Path(os.environ.get("LOCALAPPDATA", Path.home())) / "AIUsageTracker"
HISTORY = STORE / "limits_history.json"
ALERT_STATE = STORE / "limit_alerts.json"

KEEP_SECONDS = 8 * 86400       # weekly windows need a week of samples
SAME_WINDOW = 15 * 60          # resets_at jitter between polls of the same window
MIN_SPAN = 10 * 60             # need at least this much time between samples to trust a rate
MAX_AGE = 30 * 60              # no projection from a reading older than this
THRESHOLDS = (80, 95, 100)


def to_epoch(iso):
    try:
        return int(datetime.fromisoformat(str(iso).replace("Z", "+00:00")).astimezone(timezone.utc).timestamp())
    except ValueError:
        return None


def _load(path):
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}


def _save(path, data):
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(data), encoding="utf-8")
    except OSError:
        pass


def same_window(a, b):
    return a is not None and b is not None and abs(a - b) <= SAME_WINDOW


def lookback(now, resets_at):
    """An hour of history for short windows (or a long one about to reset), a day otherwise."""
    return 3600 if resets_at - now <= 5 * 3600 else 86400


def project(samples, now, resets_at):
    """Projection for one window from [t, pct, resets_at] samples, or None if there's no reliable pace.

    Returns {"pct", "rate_per_hour", "hits_at"} when 100% comes before the reset,
    else {"pct", "rate_per_hour", "at_reset"} with the expected percentage at reset.
    """
    if not resets_at or resets_at <= now:
        return None
    window = [s for s in samples if same_window(s[2], resets_at) and s[0] >= now - lookback(now, resets_at)]
    window.sort()
    # A drop inside one window means the service corrected itself; only trust what came after.
    for i in range(len(window) - 1, 0, -1):
        if window[i][1] < window[i - 1][1]:
            window = window[i:]
            break
    if len(window) < 2:
        return None
    (t0, p0, _), (t1, p1, _) = window[0], window[-1]
    if t1 - t0 < MIN_SPAN or now - t1 > MAX_AGE or p1 >= 100:
        return None
    rate = (p1 - p0) / (t1 - t0)
    if rate <= 0:
        return None
    out = {"pct": p1, "rate_per_hour": round(rate * 3600, 2)}
    hits_at = t1 + (100 - p1) / rate
    if hits_at < resets_at:
        out["hits_at"] = int(hits_at)
    else:
        out["at_reset"] = round(min(p1 + rate * (resets_at - t1), 100), 1)
    return out


def window_key(entry, w):
    return f'{entry["service"]}|{w.get("label", "?")}'


def record_and_project(entries, now, path=None):
    """Save this scan's readings and attach w["projection"] to each window that has a reliable pace."""
    path = path or HISTORY
    history = _load(path)
    for entry in entries:
        for w in entry.get("windows") or []:
            t = to_epoch(w.get("as_of") or entry.get("as_of"))
            if t is None or w.get("used_percent") is None:
                continue
            samples = history.setdefault(window_key(entry, w), [])
            if not any(s[0] == t for s in samples):
                samples.append([t, float(w["used_percent"]), w.get("resets_at")])
    for key in list(history):
        history[key] = sorted(s for s in history[key] if s[0] >= now - KEEP_SECONDS)
        if not history[key]:
            del history[key]
    _save(path, history)
    for entry in entries:
        for w in entry.get("windows") or []:
            w.pop("projection", None)
            if entry.get("source") == "live":
                p = project(history.get(window_key(entry, w), []), now, w.get("resets_at"))
                if p:
                    w["projection"] = p


def pace_text(p, fmt_time):
    if not p:
        return ""
    if "hits_at" in p:
        return f"At this pace: 100% by {fmt_time(p['hits_at'])}"
    return f"On pace for ~{p['at_reset']:.0f}% by reset"


def due_alerts(entries, now, path=None, fmt_time=None):
    """Alerts (title, text) for thresholds crossed since the last call; each fires once per window.

    Only fresh live readings count: a cached or expired reading never alerts.
    """
    path = path or ALERT_STATE
    fmt_time = fmt_time or (lambda t: datetime.fromtimestamp(t).strftime("%H:%M"))
    state = _load(path)
    alerts = []
    for entry in entries:
        if entry.get("source") != "live" or entry.get("error"):
            continue
        for w in entry.get("windows") or []:
            resets_at, pct = w.get("resets_at"), float(w.get("used_percent") or 0)
            if not resets_at or resets_at <= now:
                continue
            key = window_key(entry, w)
            seen = state.get(key) or {}
            level = seen.get("level", 0) if same_window(seen.get("resets_at"), resets_at) else 0
            crossed = [t for t in THRESHOLDS if level < t <= pct]
            if crossed:
                top = crossed[-1]
                label = w.get("label", "limit")
                reset = fmt_time(resets_at)
                if top >= 100:
                    text = f"{label} limit reached. Resets {reset}."
                else:
                    pace = pace_text(w.get("projection"), fmt_time)
                    text = f"{label} at {pct:.0f}%. " + (f"{pace}; resets {reset}." if pace else f"Resets {reset}.")
                alerts.append((f'{entry["service"]} {top}% used' if top < 100 else f'{entry["service"]} limit reached', text))
                level = top
            state[key] = {"resets_at": resets_at, "level": level}
    _save(path, state)
    return alerts
