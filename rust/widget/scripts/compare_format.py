"""Differential check: the Rust widget must word everything exactly as widget.py does.

    python scripts/compare_format.py            # prints N/N per group, exit code 1 on any difference
    python scripts/compare_format.py --seed 7 --views 500

Two kinds of cases, thousands of each, all sent to the Rust example `format_cases`
(src-tauri/examples/format_cases.rs) and compared with the Python original:

1. The pure functions of widget.py: `fmt`, `until`, `money`, plus the inline formatting rules of
   its render methods (`:.0f` percentages, `:,` counts, `[:60]`, the strftime patterns, the
   fromisoformat/astimezone conversion), with ties, negatives and boundaries.
2. Whole cards: `Widget.show(data)` runs against a recording stand-in for tkinter, and every
   label it creates (text and colour, in order) is compared with the Rust view model
   (`View::labels()`), for the fixtures and for randomly varied scans, in several time zones.

widget.py is imported as it is; tkinter, winreg and the tray are replaced in sys.modules
because they need Windows or a display. Nothing reads real logs or touches the network.
"""
import argparse
import copy
import inspect
import json
import math
import os
import random
import struct
import subprocess
import sys
import time
import types
from datetime import datetime, timedelta, timezone
from pathlib import Path
from unittest.mock import Mock, patch

WIDGET = Path(__file__).resolve().parent.parent
REPO = WIDGET.parent.parent
ZONES = ("UTC", "America/New_York", "Asia/Kolkata", "Pacific/Auckland")


# --- widget.py with a recording tkinter ---------------------------------------------
class Node:
    """Stands in for any tkinter widget and remembers the labels made, in order."""
    made = []

    def __init__(self, parent=None, **options):
        self.options, self.children, self.bound = options, [], {}
        if isinstance(parent, Node):
            parent.children.append(self)
        if type(self) in (Label, Canvas):
            Node.made.append(self)

    def configure(self, **options):
        self.options.update(options)

    def bind(self, sequence, handler):
        self.bound[sequence] = handler

    def winfo_children(self):
        return list(self.children)

    def winfo_exists(self):
        return True

    def destroy(self):
        pass

    def pack(self, *args, **options):
        pass

    grid = pack


class Label(Node):
    def seen(self):
        return [self.options.get("text", ""), self.options.get("fg", "")]


class Frame(Node):
    pass


class Canvas(Node):
    def seen(self):
        # the bar is drawn by a <Configure> handler; its fill and colour are what that handler closed over
        closed = inspect.getclosurevars(self.bound["<Configure>"]).nonlocals
        return ["bar", max(min(closed["pct"], 100), 0), closed["color"]]


def load_widget():
    tk = types.ModuleType("tkinter")
    tk.Label, tk.Frame, tk.Canvas = Label, Frame, Canvas
    for name in ("Tk", "Menu", "BooleanVar", "StringVar", "Toplevel", "OptionMenu", "Entry", "Button"):
        setattr(tk, name, Mock())
    tray = types.ModuleType("tray")
    tray.WindowsTray = Mock()
    sys.modules.update({"tkinter": tk, "winreg": Mock(), "tray": tray})
    sys.path.insert(0, str(REPO))
    import widget
    return widget


class Clock(datetime):
    """widget.datetime with a `now()` the check controls."""
    fixed = 0.0

    @classmethod
    def now(cls, tz=None):
        return cls.fromtimestamp(cls.fixed, tz)


def python_view(widget, data, now):
    """Every label Widget.show(data) creates, as [[text, colour], ...], and the status text."""
    Node.made = []
    app = widget.Widget.__new__(widget.Widget)
    app.root = Frame()
    app.today, app.month, app.month_sub, app.status = Label(), Label(), Label(), Label()
    app.today_split, app.costs, app.limits, app.api = Frame(), Frame(), Frame(), Frame()
    app.tray, app.alerts, app.menu = Mock(), Mock(), Mock()
    fixed = Node.made[:]
    Clock.fixed = now
    with patch.object(widget, "datetime", Clock), patch.object(widget.limits_watch, "due_alerts", return_value=[]):
        app.show(data)
    created = [n.seen() for n in Node.made if n not in fixed]
    split = 2 * len(widget.TOOL_COLORS)  # show() makes the today split first, then everything else
    labels = ([[app.today.options["text"], widget.TEXT]] + created[:split]
              + [[app.month.options["text"], widget.TEXT], [app.month_sub.options["text"], widget.MUTED]] + created[split:])
    return {"labels": labels, "status": app.status.options["text"]}


def same_view(python, rust):
    if not isinstance(rust, dict) or python["status"] != rust.get("status") or len(python["labels"]) != len(rust["labels"]):
        return False
    for mine, theirs in zip(python["labels"], rust["labels"]):
        if mine[0] == "bar":  # ["bar", fill, colour] against "bar <fill> <colour>"
            word, fill, color = theirs[0].split(" ")
            if word != "bar" or float(fill) != float(mine[1]) or color != mine[2]:
                return False
        elif mine != theirs:
            return False
    return True


# --- cases ------------------------------------------------------------------------------
def exact(n):
    """A number for the Rust side without any rounding on the way: ints as digits, floats as their bits."""
    if isinstance(n, int):
        return {"int": str(n)}
    return {"bits": str(struct.unpack("<Q", struct.pack("<d", n))[0])}


def numbers(rng, count, top, floats=True, ints=True):
    """Numbers spread over every size up to `top`, with the awkward ones: boundaries, ties, negatives."""
    out = [0, 1, -1, 5, -5, 59, 60, 61, 999, 1000, 1001, 1439, 1440, 1441, 3599, 3600, 86399, 86400, 86401]
    for size in (10 ** 3, 10 ** 6, 10 ** 9):
        for step in (0, 1, -1, 49, 50, 51, -49, -50, -51):
            out += [size + step, size * 999 // 1000 + step, size * 9995 // 10000 + step]
        out += [size * k // 8 for k in range(1, 80)]  # eighths and their neighbours: exact ties
    out += [k * 50 for k in range(1, 400)] + [k * 50_000 for k in range(1, 400)] + [k * 5_000_000 for k in range(1, 400)]
    while len(out) < count:
        magnitude = 10 ** rng.uniform(0, math.log10(top))
        out.append(int(magnitude) * rng.choice((1, 1, 1, -1)))
    if not ints:
        out = []
    if floats:
        frac = [k / 8 for k in range(-40, 80)] + [k / 200 for k in range(-100, 400)] + [0.005, 0.015, 0.025, 1.005, 2.675, -0.0, 0.0, -0.001, -0.004, -0.005, 0.5, 1.5, 2.5, 69.5, 89.5, 99.5, 100.5]
        frac += [float(n) + f for n in out[:300] for f in (0.0, 0.5, 0.25, 0.125, 0.99)]
        while len(frac) < count:
            frac.append(rng.choice((1, 1, 1, -1)) * 10 ** rng.uniform(-3, math.log10(top)))
        out += frac
    return out


def scalar_cases(widget, rng):
    """(group, case for Rust, what Python says)."""
    cases = []
    for n in numbers(rng, 3000, 1e13):
        cases.append(("fmt", {"op": "fmt", **exact(n)}, widget.fmt(n)))
    for n in numbers(rng, 2500, 1e8):
        cases.append(("until", {"op": "until", **exact(n)}, widget.until(n)))
    for n in numbers(rng, 3000, 1e11):
        cases.append(("money", {"op": "money", **exact(n)}, widget.money(n)))
    for n in numbers(rng, 1500, 1e4, ints=False):  # _limit_bar: f"{pct:.0f}%"
        cases.append(("percent", {"op": "percent", **exact(n)}, f"{n:.0f}%"))
    for n in numbers(rng, 1500, 1e15, floats=False):  # show(): f"{calls:,} usage records"
        cases.append(("thousands", {"op": "thousands", **exact(n)}, f"{n:,}"))
    alphabet = "abc XYZ:.'é…—→●🙂\"\\"
    for _ in range(600):  # poll(): f"Scan failed: {payload}"[:60]
        text = "".join(rng.choice(alphabet) for _ in range(rng.randint(0, 70)))
        cases.append(("clip", {"op": "clip", "text": text}, f"Scan failed: {text}"[:60]))
    for text in ("25", "25.00", " $1,234.50 ", "-3.2", "+7", "1e2", ".5", "5.", "", " ", "abc", "12 dollars", "1.2.3", "$", "--1",
                 "0x10", "1,00", "$$5", "1 000", "1e", "e1"):  # not: "inf", "nan", "1_000", "٣" (see README, differences)
        try:  # balance_dialog: float(amount.get().replace("$", "").replace(",", "").strip())
            expected = float(text.replace("$", "").replace(",", "").strip())
        except ValueError:
            expected = None
        cases.append(("amount", {"op": "amount", "text": text}, expected))
    for counts in ({"Claude Code": 0, "Codex": 0}, {"Claude Code": 1, "Codex": 0}, {"Claude Code": 2, "Codex": 3}, {}):
        active = {t: n for t, n in counts.items() if n}  # _working
        total = sum(active.values())
        cases.append(("working", {"op": "working", "active": counts}, f"● {total} working" if total else ""))
    return cases


def time_cases(rng):
    """The strftime patterns of the render methods, and fromisoformat(...).astimezone(). Zone-dependent."""
    cases = []
    patterns = ("resets %a %H:%M", "%H:%M", "%a %H:%M", "%b %d")
    for _ in range(1500):
        epoch = rng.randint(1_577_836_800, 2_051_222_400)  # 2020..2035
        epoch = rng.choice((epoch, epoch + 0.5))
        for pattern in patterns:
            cases.append(("strftime", {"op": "strftime", "pattern": pattern, **exact(epoch)},
                          datetime.fromtimestamp(epoch).strftime(pattern)))
    for _ in range(1500):
        at = datetime(2026, 11, 19, 12, tzinfo=timezone.utc) + timedelta(seconds=rng.randint(-9 * 86400, 9 * 86400))
        text = rng.choice((
            at.strftime("%Y-%m-%dT%H:%M:%SZ"), at.strftime("%Y-%m-%dT%H:%M:%S.000Z"), at.isoformat(),
            at.astimezone(timezone(timedelta(hours=rng.randint(-11, 13), minutes=rng.choice((0, 30))))).isoformat(),
            at.replace(microsecond=rng.randint(0, 999999)).isoformat(), at.strftime("%Y-%m-%d %H:%M:%S"),
            at.strftime("%Y-%m-%dT%H:%M"), at.strftime("%Y-%m-%d")))
        pattern = rng.choice(patterns)
        expected = datetime.fromisoformat(text.replace("Z", "+00:00")).astimezone().strftime(pattern)
        cases.append(("iso", {"op": "iso", "pattern": pattern, "text": text}, expected))
    return cases


def vary(base, rng):
    """One made-up scan: the fixture with its numbers, times and optional parts shuffled."""
    data = copy.deepcopy(base)
    data.pop("_working_details", None)
    now = data["now_epoch"]
    shapes = ("%Y-%m-%dT%H:%M:%SZ", "%Y-%m-%dT%H:%M:%S.000Z", "%Y-%m-%dT%H:%M:%S+00:00", "%Y-%m-%dT%H:%M:%S")
    stamp = lambda: datetime.fromtimestamp(now - rng.randint(0, 6 * 86400), timezone.utc).strftime(rng.choice(shapes))
    percent = lambda: rng.choice((rng.randint(0, 130), round(rng.uniform(0, 120), 1), rng.choice((0.5, 69.5, 70, 89.5, 90, 99.5, 100)), None, 0))
    tokens = lambda: rng.choice((0, rng.randint(0, 2000), int(10 ** rng.uniform(3, 10.5))))
    dollars = lambda: rng.choice((0.0, round(rng.uniform(0, 9), 4), round(10 ** rng.uniform(0, 5), 4), rng.randint(0, 500)))

    days = sorted(data["daily_tool"])
    data["today"] = rng.choice(days + days[-2:] + [(datetime.strptime(days[-1], "%Y-%m-%d") + timedelta(days=1)).strftime("%Y-%m-%d")])
    data["has_pricing"] = rng.random() < 0.8
    data["warnings"] = rng.choice(([], [], ["Could not fully read a.jsonl (OSError); totals may be incomplete."]))
    for row in data["day"]:
        row.update(total=tokens(), cost=dollars(), calls=rng.randint(0, 5000))
    for tools in data["daily_tool"].values():
        for tool in list(tools):
            tools[tool] = tokens()
            if rng.random() < 0.15:
                del tools[tool]
    for tools in data["daily_tool_cost"].values():
        for tool in list(tools):
            tools[tool] = dollars()
    data["daily_tool_unpriced"] = {d: rng.sample(["Claude Code", "Codex"], rng.randint(1, 2)) for d in days if rng.random() < 0.2}
    for tool in data["tool"]:
        tool["total"] = tokens()
    if rng.random() < 0.15:
        data["tool"] = [t for t in data["tool"] if t["key"] != "Claude Code"]

    limits = []
    for service in rng.sample(["Claude Code", "Codex", "Other Service"], rng.randint(0, 3)):
        windows = []
        for i in range(rng.choice((0, 1, 2, 2, 3, 4))):
            w = {"label": rng.choice(("5-hour session", "7-day, all models", "7-day", "spark: 15-minute")), "used_percent": percent(),
                 "resets_at": rng.choice((now + rng.randint(-7000, 7000), now + rng.randint(0, 700000), now + rng.uniform(0, 90000), None, now))}
            if rng.random() < 0.1:
                del w["label"]
            if rng.random() < 0.5:
                w["projection"] = rng.choice(({"pct": 50.0, "rate_per_hour": 4.2, "hits_at": now + rng.randint(0, 90000)},
                                              {"pct": 50.0, "rate_per_hour": 4.2, "at_reset": rng.choice((round(rng.uniform(0, 100), 1), 100, 0.5, 60.5))},
                                              None))
            windows.append(w)
        entry = {"service": service, "windows": windows, "source": rng.choice(("live", "cached", "log", None, "other")),
                 "as_of": rng.choice((stamp(), stamp(), None)), "plan": rng.choice(("Max 20x", "Pro", None, "")),
                 "error": rng.choice((None, None, "live check failed (offline?)", "Codex login expired, open Codex to renew it"))}
        if rng.random() < 0.4:
            entry["details"] = rng.sample(["Credit balance: 42.5", "Usage limit reached", "Workspace spend control reached"], rng.randint(0, 3))
        limits.append(entry)
    data["plan_limits"] = limits

    api = []
    for provider in rng.sample(["Anthropic", "OpenAI"], rng.randint(0, 2)):
        p = {"provider": provider, "configured": rng.random() < 0.8, "balance": None, "source": rng.choice(("live", "cached", None)),
             "as_of": rng.choice((stamp(), None)), "error": rng.choice((None, None, f"{provider} usage report failed (HTTP 500)"))}
        if rng.random() < 0.8:
            p.update(month=dollars(), today=dollars(), last30=dollars())
        if rng.random() < 0.6:
            entered = dollars()
            p["balance"] = {"entered": entered, "as_of": stamp()[:19] + "Z", "spent_since": dollars(),
                            "remaining": rng.choice((round(rng.uniform(-20, 20), 2), round(entered - rng.uniform(0, 3), 2), 5, 4.99, 5.0))}
        api.append(p)
    data["api"] = rng.choice((api, api, None)) if not api else api

    lifetime = rng.choice((None, tokens(), float(tokens()) + 0.5))
    data["openai_analytics"] = rng.choice((
        {"summary": {"lifetimeTokens": lifetime}, "daily": rng.choice((None, {}, {d: tokens() for d in rng.sample(days, min(3, len(days)))}))},
        {"summary": {}, "daily": None, "error": "x"}, None))
    return data, now + rng.choice((0, rng.uniform(0, 600), rng.uniform(600, 8000)))


def view_cases(widget, rng, count):
    full = json.loads((WIDGET / "fixtures" / "sample-full.json").read_text(encoding="utf-8"))
    plain = json.loads((WIDGET / "fixtures" / "sample-data.json").read_text(encoding="utf-8"))
    scans = [(full, full["now_epoch"]), (full, full["now_epoch"] + 20.5), (plain, plain["now_epoch"] + 3),
             (dict(plain, has_pricing=False), plain["now_epoch"])]
    scans += [vary(full, rng) for _ in range(count)]
    cases = []
    for data, now in scans:
        data = json.loads(json.dumps(data))  # both sides get exactly what the JSON file would hold
        cases.append(("view", {"op": "view", "data": data, "now": now}, python_view(widget, copy.deepcopy(data), now)))
    return cases


# --- run --------------------------------------------------------------------------------
def rust_answers(binary, cases, zone):
    lines = "".join(json.dumps(case) + "\n" for _, case, _ in cases)
    done = subprocess.run([str(binary)], input=lines, capture_output=True, text=True, encoding="utf-8",
                          env=dict(os.environ, TZ=zone), check=True)
    answers = [json.loads(line) for line in done.stdout.splitlines()]
    assert len(answers) == len(cases), f"{len(answers)} answers for {len(cases)} cases"
    return answers


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--seed", type=int, default=20261119)
    ap.add_argument("--views", type=int, default=300, help="random whole-card scans per time zone")
    ap.add_argument("--show", type=int, default=5, help="how many differences to print per group")
    args = ap.parse_args()
    subprocess.run(["cargo", "build", "--quiet", "--example", "format_cases"], cwd=WIDGET / "src-tauri", check=True)
    binary = WIDGET / "src-tauri" / "target" / "debug" / "examples" / ("format_cases.exe" if os.name == "nt" else "format_cases")
    widget = load_widget()

    totals, failures = {}, {}

    def compare(cases, zone):
        for (group, case, expected), got in zip(cases, rust_answers(binary, cases, zone)):
            ok = same_view(expected, got) if group == "view" else expected == got
            passed, count = totals.get(group, (0, 0))
            totals[group] = (passed + ok, count + 1)
            if not ok:
                failures.setdefault(group, []).append((zone, case, expected, got))

    compare(scalar_cases(widget, random.Random(args.seed)), "UTC")
    for zone in ZONES if hasattr(time, "tzset") else (os.environ.get("TZ", "UTC"),):
        os.environ["TZ"] = zone
        if hasattr(time, "tzset"):
            time.tzset()
        rng = random.Random(f"{args.seed}-{zone}")
        compare(time_cases(rng), zone)
        compare(view_cases(widget, rng, args.views), zone)

    for group, (passed, count) in totals.items():
        print(f"{group:10} {passed}/{count}")
        for zone, case, expected, got in failures.get(group, [])[:args.show]:
            if group == "view":
                pairs = list(zip(expected["labels"], got.get("labels", []) if isinstance(got, dict) else []))
                differ = [(a, b) for a, b in pairs if a[0] != "bar" and a != b][:3]
                print(f"   [{zone}] status {expected['status']!r} / {got.get('status') if isinstance(got, dict) else got!r};"
                      f" {len(expected['labels'])} / {len(pairs)} labels; first differences (python, rust): {differ}")
            else:
                print(f"   [{zone}] {case}: python {expected!r}, rust {got!r}")
    passed, count = (sum(v[i] for v in totals.values()) for i in (0, 1))
    print(f"total      {passed}/{count}")
    sys.exit(0 if passed == count else 1)


if __name__ == "__main__":
    main()
