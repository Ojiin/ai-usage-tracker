# AI Usage Tracker widget (Rust)

The desktop widget of AI Usage Tracker, rewritten from Python/tkinter as a
[Tauri 2](https://tauri.app) app: Rust for everything that decides something, and a plain
HTML/CSS/JS page that only draws. It replaces `widget.py`, `tray.py` and `startup.py`.

It targets **Windows**. It also builds and runs on Linux, which is how it was developed and
checked; the Windows-only parts are listed in the [manual test checklist](#manual-test-checklist-windows).

![The widget on sample data](docs/widget.png)

The hover card of "● 2 working": [docs/hover.png](docs/hover.png).

## Build on Windows

You need:

- [Rust](https://rustup.rs) (stable, the default MSVC toolchain) and the "Desktop development
  with C++" build tools it asks for;
- [Node.js](https://nodejs.org) 18 or later (only for the Tauri command line);
- the WebView2 runtime, which Windows 10 and 11 already have.

```
cd rust\widget
npm install
npm run build
```

The app is `src-tauri\target\release\AIUsageTracker.exe`: one file, no installer, copy it
anywhere. Put `pricing.json` next to it; it writes `dashboard.html` next to itself.

```
npm run dev     # run from source, rebuilding on change
npm test        # the Rust unit tests
```

> **Before shipping: the exe must not need Python, and today it does.** The core crates under
> `rust/` still carry their PyO3 bridge, and the widget links `limits-watch-rs`. Built for
> `x86_64-pc-windows-gnu` on Linux, both the debug and the release exe import `python3.dll`,
> so they will not start on a PC that has no `python3.dll` on its path; see
> [Verification](#verification). The MSVC build could not be tried here: after `npm run build`,
> run `dumpbin /dependents src-tauri\target\release\AIUsageTracker.exe` and look for
> `python3.dll`. The fix belongs in the core crates, not here: make `pyo3` an optional feature
> that only the bridge build turns on, so `lib.rs`'s `#[pymodule]` is not compiled into the app.
> Until then the build may also want a Python installation for PyO3's build script.

### Run it on sample data

Until the core is wired (below), and whenever you work on the page, set `AIUT_FAKE_DATA` to a
file holding what `tracker.collect()` returns:

```
set AIUT_FAKE_DATA=%CD%\fixtures\sample-full.json
npm run dev
```

- `fixtures/sample-data.json`: exactly what the Python `tracker.collect()` returned for a
  throwaway home folder with made-up Claude Code and Codex logs (no logins there, so no live
  limits, no API spend and no account analytics).
- `fixtures/sample-full.json`: the same, edited so every part of the card has something to
  show: live limits with pace, an expired reading, API spend and balances, account analytics,
  two working sessions.
- `python scripts/make_fixture.py --home <throwaway folder>` rebuilds both from the Python
  original. It never reads real logs or logins and makes no network calls.

In this mode times are moved to "now" so countdowns run, nothing is read from your real logs,
no alert state is written to `%LOCALAPPDATA%\AIUsageTracker`, and "Set API balance…" saves
nothing. Without the variable the widget shows `Scan failed: tracker core not wired; set
AIUT_FAKE_DATA` until `core.rs` is wired.

## What moved where

| Python | Rust / page |
|---|---|
| `widget.py`: `fmt`, `until`, `money`, the inline format rules | `src-tauri/src/format.rs` |
| `widget.py`: `show`, `_tool_cost`, `_service_limits`, `_limit_bar`, `_countdown`, `_api_section`, `_working`, `_show_working` (the wording) | `src-tauri/src/view.rs` (the view model) |
| the tkinter widgets those methods created | `ui/index.html`, `ui/style.css`, `ui/app.js` |
| `widget.py`: `_show_working` hover window | `ui/tip.html`, `place::tip_position`, `lib.rs` (`working_enter`, `tip_ready`, `track_hover`) |
| `widget.py`: `balance_dialog` | `ui/balance.html`, `lib.rs` (`balance_save`, `balance_clear`), `format::parse_amount`, `view::today_spend` |
| `widget.py`: `load_settings`, `save_settings` | `src-tauri/src/settings.rs` |
| `widget.py`: `virtual_screen`, `_start_position`, `peek` geometry, `_track_peek` test | `src-tauri/src/place.rs` |
| `widget.py`: `hide_to_tray`, `restore`, `peek`, `poll` (tray events), `refresh` guard | `src-tauri/src/lifecycle.rs` |
| `widget.py`: `refresh`, `_scan`, `auto_refresh`, `tick`, `_alerts`, menu, drag, toggles | `src-tauri/src/lib.rs` |
| `tray.py` | `src-tauri/src/tray.rs` (Tauri's tray API; no Win32 calls of our own) |
| `startup.py` | `src-tauri/src/autostart.rs` (`winreg` crate under `#[cfg(windows)]`) |
| calls into `tracker.`, `api_billing.`, `limits_watch.` | `src-tauri/src/core.rs`, and only there |
| `test_widget.py` | tests in `lifecycle.rs`, `tray.rs`, `place.rs` |
| `test_startup.py` | tests in `autostart.rs` (the registry ones run on Windows only) |

```
rust/widget/
  package.json            @tauri-apps/cli; scripts dev / build / test
  ui/                     the three pages: index.html (card), tip.html (hover card),
                          balance.html (dialog), with style.css and app.js
  fixtures/               sample-data.json, sample-full.json
  scripts/
    make_fixture.py       builds the fixtures by running the Python original on fake logs
    compare_format.py     differential check, Python widget against this one
    screenshot.sh         runs the app under xvfb and saves docs/*.png
  docs/                   widget.png, hover.png
  src-tauri/
    Cargo.toml, build.rs, tauri.conf.json, capabilities/default.json, icons/, app-icon.svg
    examples/format_cases.rs   the Rust half of compare_format.py
    src/main.rs, lib.rs, core.rs, view.rs, format.rs, lifecycle.rs, place.rs,
        settings.rs, autostart.rs, tray.rs
```

## How it fits together

Rust builds a complete **view model** from the scan: every string already worded, every colour
and bar width decided (`view::View`, sent to the page as the `screen` event). `ui/app.js` turns
it into elements and sends clicks back as commands (`press`, `drag`, `menu`, …). Nothing is
formatted or computed in the page, except that it measures its own content so the window can
take that size.

Settings stay in `%LOCALAPPDATA%\AIUsageTracker\widget.json` with the keys the Python widget
used (`x`, `y`, `topmost`, `alerts`; positions in physical pixels), so an existing file carries
over, and going back to the Python widget works too. "Start with Windows" uses the same
`AIUsageTracker` value under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` with the
same quoting, so an entry made by the Python exe is recognised and re-pointed at this exe on
first launch. The webview's own cache goes to `%LOCALAPPDATA%\AIUsageTracker\webview`.

## The seam to the core: `src-tauri/src/core.rs`

`core.rs` is the only file that calls the ported crates. Wired today: `limits-watch-rs`
(`store`, `due_alerts`, `pace_text`). Each function still waiting is marked `TODO(core)` and
names the call it expects; wiring one means replacing its body with that call and adding the
crate to `Cargo.toml`. The fake-data branch stays.

| `core.rs` function | waits for |
|---|---|
| `collect(log) -> Result<Value, String>` | `tracker_rs::port::native::collect(log: &mut dyn FnMut(&str)) -> Result<Value, String>` |
| `write_dashboard(&data, &path) -> Result<(), String>` | `tracker_rs::port::native::write_dashboard(data: &Value, out_path: &Path) -> Result<(), String>` |
| `working_now() -> Result<Value, String>` | `tracker_rs::port::native::working_now() -> Result<Value, String>` (`{"Claude Code": n, "Codex": n}`) |
| `working_details() -> Result<Vec<Value>, String>` | `tracker_rs::port::native::working_details() -> Result<Vec<Value>, String>` |
| `describe_session(&info) -> Result<String, String>` | `tracker_rs::port::native::describe_session(info: &Value) -> Result<String, String>` |
| `providers() -> Vec<String>` | `api_billing_rs::port::native::providers() -> Vec<String>` (`list(PROVIDERS)`) |
| `utc_day(now) -> String` | `api_billing_rs::port::native::utc_day(now: f64) -> String` |
| `set_balance(provider, amount, today_spend) -> Result<(), String>` | `api_billing_rs::port::native::set_balance(provider: &str, amount: Option<f64>, today_spend: Option<f64>) -> Result<(), String>` |

Things the real `collect` must do, because the widget relies on them:

- read `pricing.json` from the folder of the exe (`core::exe_dir()`, Python's `tracker.HERE`);
- include `limits_watch.record_and_project`, as `tracker.collect` does, so windows carry
  `projection`;
- return errors as `"ExceptionName: message"`; the widget shows `Scan failed: <that>`, cut to
  60 characters.

`write_dashboard` in fake-data mode embeds `template.html` from the repository root at compile
time (the Python build also bundled it into the exe).

## Verification

Run on Linux (rustc 1.97, Tauri 2.12); nothing here was run on Windows.

| Check | Command | Result |
|---|---|---|
| Unit tests | `cargo test` in `src-tauri` | 44 passed |
| Format and lint | `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` | clean |
| Lint for Windows | `cargo clippy --target x86_64-pc-windows-gnu --all-targets -- -D warnings` | clean |
| Type-check for Windows | `cargo check --target x86_64-pc-windows-gnu` | passes |
| Link for Windows | `cargo build [--release] --target x86_64-pc-windows-gnu` | links; GUI subsystem (no console); but see the PyO3 note |
| Python against Rust | `python scripts/compare_format.py` | 51875/51875 (52275/52275 with `--seed 99 --views 400`) |
| The real window | `sh scripts/screenshot.sh` (xvfb) | `docs/widget.png`, `docs/hover.png` |

`compare_format.py` imports `widget.py` unchanged (tkinter, winreg and the tray are stubbed in
`sys.modules`) and compares, case by case, with `examples/format_cases.rs`:

- `fmt`, `until`, `money` and the inline rules (`:.0f`, `:,`, `[:60]`, the strftime patterns,
  `fromisoformat(...).astimezone()`): about 50,000 inputs with ties, negatives and boundaries;
- whole cards: `Widget.show(data)` runs against a recording stand-in for tkinter, and every
  label it creates (text and colour, in order) must equal the Rust view model's, for the
  fixtures and 1,200 randomly varied scans in four time zones.

It found one real difference while this was written: Python 3.12 and later add floats in
`sum()` with a running correction, so a month of costs could differ from a plain loop by a
cent. `format::Num::sum` now adds the same way. The other ported crates use `sum()` on floats
too (`cost_of`, `estimate_balance`, `summarize`) and need the same care.

Also exercised under xvfb with a window manager (openbox) and xdotool: left-drag moves the
window and the position lands in `widget.json`; a second start reopens there; the right-click
menu opens with the right items and ticks; "Always on top" is saved; "Set API balance…" opens
40 px below the widget's top; the hover card opens under the label, clamped to the screen, and
closes when the pointer leaves; `dashboard.html` appears next to the exe.

**PyO3 note.** The Linux binary has no Python symbols (`nm -D` shows none). The
`x86_64-pc-windows-gnu` exe imports `python3.dll` (`objdump -p`): 214 symbols in the debug
build, 93 in the release build. The linker keeps `PyInit__native`, the `#[pymodule]` entry
point in `limits-watch-rs/src/lib.rs`, and with it the whole bridge. The widget itself calls
only `port::native::{store, due_alerts, pace_text}`, which use no Python. Do the `dumpbin`
check above on the MSVC build.

## Where it knowingly differs from the Python widget

- **Width** is fixed at 290 CSS pixels; tkinter sized the window to its widest label. Height
  follows the content as before.
- **Default position** is still top-right of the primary screen, 300 px in and 60 px down, but
  counted in the screen's scale, so the card stays on screen at 150%.
- **Peek** starts when the pointer has stayed on the tray icon for 0.4 s. The Python widget
  got a "pointer is resting" message from the shell and suppressed the plain tooltip; Tauri
  reports the pointer arriving, and the icon's tooltip (same text) appears as well.
- **Tray icon** has its own image (the Python one used the generic Windows application icon).
  Tauri reports mouse clicks on it, not keyboard activation: Enter on the focused icon may do
  nothing; its menu still opens.
- **Alerts** are Windows toasts from `tauri-plugin-notification`, not balloons from the tray
  icon, and titles and texts are no longer cut to 63 and 255 characters.
- **Menus** are native menus with the same items in the same order; "Show tracker" is not bold.
- **The position is saved** 0.4 s after the window stops moving, not at button release.
- **"Set API balance…"** is a small page in its own window. It refuses `inf`, `nan`,
  `1_000` and non-ASCII digits, which Python's `float()` accepted.
- **Scan errors** show the Rust core's error text where Python showed the last line of its
  traceback.
- **"Start with Windows"** always registers the exe; there is no "running from source with
  pythonw" case.
- **Opening the dashboard** asks Windows to open the file; Python went through `webbrowser`.
- **Sums** follow Python 3.12+ (see above); a widget frozen with an older Python could show a
  cent less or more.
- It needs the WebView2 runtime; the PyInstaller exe needed nothing.

## Manual test checklist (Windows)

None of this could be run on Linux. Start with sample data (`AIUT_FAKE_DATA`, above) unless
the core is wired.

Start-up and window

- [ ] Double-click the exe: no console window; one frameless dark card near the top-right, no
      taskbar button, slightly see-through, text crisp at 100%, 125% and 150% scaling.
- [ ] "Scanning…" appears first, then the card fills in and the status reads `Updated HH:MM`.
- [ ] Left-drag anywhere except the four buttons moves it. Exit and restart: it reopens there.
      `%LOCALAPPDATA%\AIUsageTracker\widget.json` holds `x` and `y`.
- [ ] Start once with a `widget.json` written by the Python widget: same place, same toggles.
- [ ] Drag it onto a second monitor, exit, unplug that monitor, start: it appears top-right of
      the primary screen.
- [ ] It stays above other windows. Right-click → Always on top (untick): other windows now
      cover it; the tick and `topmost` in `widget.json` follow; it survives a restart.

Buttons and menu

- [ ] ⟳ rescans (status `Scanning…`), ↗ opens `dashboard.html` (next to the exe) in the
      browser, ✕ quits and the tray icon goes away.
- [ ] Right-click shows: Hide to tray (keep tracking), Refresh now, Open dashboard, Set API
      balance…, Always on top, Limit alerts, Start with Windows, Exit. Each does its job.
- [ ] Delete `dashboard.html` while a scan cannot succeed, press ↗: status says
      `Dashboard not built yet — wait for the scan`.

Tray

- [ ] An "AI Usage Tracker" icon is in the notification area (maybe under ^) with the tooltip
      "AI Usage Tracker - hover to peek, click to show, right-click for menu".
- [ ] − hides the window; scans continue (watch `dashboard.html`'s modified time, 5 minutes).
- [ ] Left-click the icon: the window returns where it was. Double-click also works.
- [ ] While hidden, rest the pointer on the icon: the widget appears just above it (below it
      with the taskbar on top), on top of everything; move up into it and it stays; hover
      "● N working" there and the card of sessions opens; move away and both disappear.
- [ ] While the window is visible, hovering the icon does not move it.
- [ ] Right-click the icon: Show tracker, Refresh now, Open dashboard, Exit.
- [ ] Restart Explorer (Task Manager → Windows Explorer → Restart): the icon comes back.
- [ ] If the icon cannot be created (hard to provoke; e.g. Explorer not running), the widget
      stays visible, − is refused with "Tray icon unavailable — please try again".

Working sessions

- [ ] With a Claude Code or Codex session running, a green "● N working" shows within 30 s;
      hovering it opens the card below the label (above it near the bottom of the screen),
      inside the screen; it closes when the pointer leaves the label. (Needs the core wired, or
      the fixture.)

Limits, alerts, countdown

- [ ] Each limit shows `resets Mon 02:00 · in 10h 06m`; the "in …" part changes within 30 s
      without a rescan.
- [ ] First run on `sample-full.json`: a Windows notification "Claude Code 80% used" and the
      status line `⚠ Claude Code 80% used` in orange. Check with Focus Assist on: the status
      line still shows it. (A toast from an exe that was never installed may show without the
      app's name; if no toast appears at all, see tauri-plugin-notification's notes on app IDs.)
- [ ] Untick Limit alerts, delete `%TEMP%\AIUsageTracker-fake\limit_alerts.json`, restart: no
      notification, status `Updated HH:MM`; tick it again: nothing is replayed.

Start with Windows

- [ ] Right-click → Start with Windows: status `Will start with Windows`; `reg query
      HKCU\Software\Microsoft\Windows\CurrentVersion\Run /v AIUsageTracker` shows the exe's
      full path in double quotes; Task Manager → Startup apps lists it; sign out and in: it starts.
- [ ] Move the exe, start it from the new place: the registry value now points there.
- [ ] Untick: status `Won't start with Windows`; the value is gone.
- [ ] With the Python widget's entry present, first start of this exe re-points it.
- [ ] `cargo test` on Windows also runs the registry tests (throwaway key
      `HKCU\Software\AIUsageTrackerTest`, removed afterwards).

Set API balance…

- [ ] Opens a small always-on-top dialog 40 px below the widget's top-left. "abc" → "Enter a
      number, e.g. 25.00"; `$1,234.50` is accepted; Enter saves, Esc and the window's ✕ cancel;
      after Save or Clear the widget rescans. (Saving really needs the core wired.)
