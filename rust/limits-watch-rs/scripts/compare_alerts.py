"""Run random alert scenarios through limits_watch.due_alerts and the Rust port; fail on any difference.

Usage: python3 scripts/compare_alerts.py   (from rust/limits-watch-rs, after `cargo build --example alerts`)
"""
import json
import random
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT))
import limits_watch as lw  # noqa: E402

NOW = 1_800_000_000
rng = random.Random(7)


def window():
    w = {"used_percent": rng.choice([0, 12, 50, 79.5, 80, 80.4, 82, 94.5, 95, 97, 99.6, 100, 104, None, 80.5])}
    if rng.random() < 0.9:
        w["label"] = rng.choice(["5-hour session", "Weekly", "Opus weekly é"])
    if rng.random() < 0.95:
        w["resets_at"] = NOW + rng.choice([-10, 0, 600, 3 * 3600, 3 * 3600 + 300, 30 * 3600, 6 * 86400])
    if rng.random() < 0.4:
        w["projection"] = rng.choice([
            {"pct": 82, "rate_per_hour": 20, "hits_at": NOW + 3240},
            {"pct": 40, "rate_per_hour": 2.5, "at_reset": 61.5},
            {"pct": 40, "rate_per_hour": 2.5, "at_reset": 62.5},
            {}, None,
        ])
    return w


def entry():
    e = {"service": rng.choice(["Claude Code", "Codex"]), "source": rng.choice(["live", "live", "live", "cached", "log"]),
         "windows": [window() for _ in range(rng.randint(0, 3))]}
    if rng.random() < 0.15:
        e["error"] = rng.choice(["login expired", "", None])
    if rng.random() < 0.05:
        del e["windows"]
    return e


def main():
    lines, expected = [], []
    with tempfile.TemporaryDirectory() as tmp:
        for run in range(60):
            py_path, rs_path = Path(tmp) / f"py{run}.json", Path(tmp) / f"rs{run}.json"
            now = NOW
            for _ in range(rng.randint(1, 8)):
                now += rng.choice([0, 60, 900, 3600, 5 * 3600])
                entries = [entry() for _ in range(rng.randint(0, 3))]
                alerts = lw.due_alerts(json.loads(json.dumps(entries)), now, py_path, lambda t: f"T{t}")
                expected.append({"alerts": [list(a) for a in alerts], "state": json.loads(py_path.read_text())})
                lines.append(json.dumps({"entries": entries, "now": now, "path": str(rs_path)}))
        out = subprocess.run([str(Path(__file__).resolve().parents[1] / "target/debug/examples/alerts")],
                             input="\n".join(lines) + "\n", capture_output=True, text=True, check=True).stdout
    got = [json.loads(line) for line in out.splitlines()]
    bad = [(i, e, g) for i, (e, g) in enumerate(zip(expected, got)) if e != g]
    for i, e, g in bad[:5]:
        print(f"case {i}\n  input   {lines[i]}\n  python  {json.dumps(e)}\n  rust    {json.dumps(g)}")
    fired = sum(len(e["alerts"]) for e in expected)
    print(f"{len(expected) - len(bad)}/{len(expected)} alert scenarios match, {fired} alerts fired" + ("" if len(got) == len(expected) else f" ({len(got)} answered)"))
    sys.exit(1 if bad or len(got) != len(expected) else 0)


if __name__ == "__main__":
    main()
