"""Run random balances through api_billing.estimate_balance and the Rust port; fail on any difference.

Usage: python3 scripts/compare_balance.py   (from rust/api-billing-rs, after `cargo build --example balance`)
"""
import json
import random
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT))
import api_billing  # noqa: E402

rng = random.Random(11)


def case():
    days = [f"2026-11-{d:02d}" for d in rng.sample(range(1, 29), rng.randint(0, 20))]
    cost = lambda: rng.choice([rng.uniform(0, 300), round(rng.uniform(0, 50), 2), 0.1, 0.2, 0.3, 1e-9, 1e9, 0.0, 7])
    daily = {d: cost() for d in days}
    if rng.random() < 0.1:
        daily[""] = 5.5
    entry = {"amount": rng.choice([25, 25.0, 1234.56, 0.3, 1e6]), "as_of": f"2026-11-{rng.randint(1, 28):02d}T10:00:00+00:00"}
    if rng.random() < 0.6:
        entry["day_baseline"] = rng.choice([0, 0.0, 1.25, 17.3, None, rng.uniform(0, 40)])
    return {"entry": rng.choice([entry, entry, entry, entry, {}, None]), "daily": daily}


def main():
    cases = [case() for _ in range(4000)]
    expected = [{"ok": api_billing.estimate_balance(c["entry"], c["daily"])} for c in cases]
    exe = Path(__file__).resolve().parents[1] / "target/debug/examples/balance"
    out = subprocess.run([str(exe)], input="".join(json.dumps(c) + "\n" for c in cases), capture_output=True, text=True, check=True).stdout
    got = [json.loads(line) for line in out.splitlines()]
    same = lambda a, b: json.dumps(a) == json.dumps(b)  # int versus float counts
    bad = [(c, e, g) for c, e, g in zip(cases, expected, got) if not same(e, g)]
    for c, e, g in bad[:5]:
        print(f"input   {json.dumps(c)}\n  python  {json.dumps(e)}\n  rust    {json.dumps(g)}")
    print(f"{len(cases) - len(bad)}/{len(cases)} balances match" + ("" if len(got) == len(cases) else f" ({len(got)} answered)"))
    sys.exit(1 if bad or len(got) != len(cases) else 0)


if __name__ == "__main__":
    main()
