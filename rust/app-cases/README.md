# Recorded runs of the original command line

Ten runs of `python3 tracker.py` (dashboards, `--set-balance`, `--help`, usage errors), recorded by
`rust-anything app record` against the fixture home that `rust/tracker-rs/fixture/make_home.py`
builds. Check a build of the Rust app against them with:

```sh
(cd rust/app && cargo build --release)
rust-anything app replay . --build rust/app/target/release/tracker
```

The `.toml` files carry absolute paths (the fixture home, a state folder per case and
`AI_USAGE_TRACKER_DIR`, the folder of `pricing.json`) from the machine they were recorded on. On
another machine, build the fixture home, put its path in place of the recorded one, and replay; or
record again while `tracker.py` is still in the repository:

```sh
python3 rust/tracker-rs/fixture/make_home.py /tmp/aiut-home
rust-anything app record . --no-traps --name tracker --inputs cases.jsonl \
  --normalize 'now_epoch="now_epoch": \d+' --normalize 'generated="generated": "[0-9: -]+"' \
  --normalize 'prog=tracker\.py' --normalize 'set_at="set_at": [0-9.]+' -- python3 tracker.py
```

The dashboards hold the day they were recorded (`today`, streaks, this month's totals), so a replay
on a later day differs there until the cases are recorded again.

`cargo test` does not run these cases: trycmd turns every backslash of an output file into a
slash, and the dashboards contain Windows paths.
