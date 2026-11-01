# AI Usage Tracker

Reads the logs Claude Code and Codex keep on this PC and builds `dashboard.html`:
tokens per day, per model and per project, the token mix, and your Claude Code and Codex plan limits.

An unofficial, independent tool for Windows. It isn't made or endorsed by Anthropic or OpenAI.

## What it reads, and what it sends

Please read this before using it:

- **Local logs:** it reads the session logs and prompt history that Claude Code (`~/.claude`) and Codex
  (`~/.codex`) keep on your PC. Only slash-command names are taken from prompt history, never your prompts. The
  hover card shows your latest prompt and session titles on your own screen only.
- **Your logins:** to show live plan limits, it reads the login each app saved
  (`~/.claude/.credentials.json`, `~/.codex/auth.json`) and uses it to ask each service for your current usage.
  It never renews, changes or copies these logins, and never writes them to any file.
- **Undocumented endpoints:** those plan-limit checks call usage addresses that Claude Code and Codex use
  internally (listed under [Plan limits](#plan-limits-live)). They aren't public APIs, may change or stop
  working at any time, and using them may not be permitted by each service's terms. The Claude check sends a
  `claude-cli` User-Agent, as Claude Code does. If you'd rather not make these calls, don't use the widget or
  dashboard, or remove `claude_live` and `codex_live` from `tracker.py`.
- **Codex account analytics:** it starts the installed Codex app-server briefly to read OpenAI's documented
  account usage report.
- **Admin keys (optional):** if you set `ANTHROPIC_ADMIN_KEY` or `OPENAI_ADMIN_KEY`, they're sent only to their
  own provider's official usage and cost reports.
- **Nothing else leaves your PC.** There's no telemetry. Everything the tracker saves stays in
  `%LOCALAPPDATA%\AIUsageTracker\` and in `dashboard.html` next to the app. The dashboard holds your usage
  details, so don't share or commit it.

Dollar figures labelled "API-equivalent" are estimates from `pricing.json`, not what you were billed.

## Widget (AIUsageTracker.exe)

Double-click `AIUsageTracker.exe`. A small always-on-top card shows today's tokens, this month's tokens
and your Claude Code and Codex plan limits. It's a single file with no install, so you can copy it anywhere.

- **Left-drag** anywhere on it to move it; it reopens where you left it.
- **⟳** rescans now (it also rescans every 5 minutes), **↗** opens the full dashboard, **✕** closes it.
- **Right-click** for the same actions plus "Always on top" and "Limit alerts" toggles.
- A green **● N working** next to the title counts Claude Code and Codex sessions whose log changed in the
  last 2 minutes (a subagent counts as its parent session). It's checked every 30 seconds.
  **Hover over it** for a card listing each working session, e.g. "Opus 5.5 med working on scripts", with:
  - the session title;
  - your last prompt (shortened);
  - each subagent with its type or nickname, model and task;
  - how long ago its log was last written.

  Model and effort come from the session log: Claude Code's assistant entries, or Codex's turn context.
  The project is the session's working folder. Titles come from Claude Code's session title or Codex's
  thread name. Only the end of each recently written log is read, and nothing leaves your PC.

It writes `dashboard.html` next to itself, and reads `pricing.json` from there if present.
Click **−** or right-click **Hide to tray (keep tracking)** to remove the window from the screen
and taskbar. Tracking and dashboard updates continue every 5 minutes while hidden.
Click the **AI Usage Tracker** icon in the notification area by the clock (possibly under **^**)
to show it again. **Rest the pointer on that icon** to peek: the whole widget pops up above it
and hides again when the pointer moves away (move up into it to keep it open, e.g. to hover "● N working").
Drag the icon out of **^** onto the taskbar so it's one hover away. Its right-click menu also offers Show tracker, Refresh now, Open dashboard, and Exit.
The **✕** button and **Exit** quit the app completely. If Windows cannot create the tray icon,
the widget stays visible so you can still reach it.
Its window position is saved in `%LOCALAPPDATA%\AIUsageTracker\widget.json`.
**Start with Windows:** right-click → **Start with Windows** turns starting at sign-in on or off. It adds or
removes an `AIUsageTracker` value under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, your
account's own startup list, so no admin rights are needed. It also shows in Task Manager → Startup apps,
where you can turn it off too. If you move the exe, open it once from its new place and the entry
follows it.

After changing the code, run `build.bat` to rebuild the exe.

## Dashboard only

Double-click `run.bat`, or:

```
pip install zstandard   # Codex compresses older session logs
python tracker.py            # writes dashboard.html and opens it
python tracker.py --no-open  # just regenerate
```

An open dashboard tab reloads itself every 5 minutes and keeps your chosen date range.

Besides tokens per day, models and projects, the dashboard shows:

- **Activity, last 12 months:** a calendar heatmap shaded by tokens per day, with current and longest
  streak, active days and busiest day.
- **Time of day and day of week:** tokens by local hour and weekday, per tool.
- **Tools, subagents and skills, MCP servers, slash commands:** call counts. Tool calls are counted once
  each from the session logs (Claude Code `tool_use` blocks; Codex function and custom tool calls, without
  a fork's copied history). MCP calls are grouped by server. Slash commands come from
  `~/.claude/history.jsonl` and `~/.codex/history.jsonl`; only the command name is read, never the prompt.
  Codex's own subagents show as "Codex subagent" because its logs don't name a type.
- **Year in review:** the year's tokens, API-equivalent cost, active days, longest streak, busiest day,
  peak hour and weekday, and top models, projects, tools, subagents, skills and commands. Pick a year when
  there's more than one.

Everything except the heatmap, streaks and year in review follows the 7d/30d/90d/All range.

## Claude account analytics

The dashboard has a matching Claude section, above the OpenAI one, laid out the same way. It shows lifetime tokens, peak daily tokens, current and longest streak, longest turn, the latest active day, a daily chart, and model and project rankings with each one's share. The widget has a matching "Claude Code (this PC)" line.

Anthropic has no account token-history report like OpenAI's: the only account data Claude Code's login can read is the plan-limit response. So those figures come from the Claude Code logs on this PC and are labelled "this PC"; other PCs, claude.ai chat and Cowork aren't included. The longest turn comes from the `turn_duration` entries Claude Code writes after each turn. Two parts are account-reported, from the same live check as the plan limits:

- **This week by surface:** how the weekly limit splits across Claude Code, chat, Cowork and other.
- **Extra usage:** the amount used this period, when extra usage is turned on for the account.

## OpenAI account analytics

The dashboard now reads the documented `account/usage/read` report through the installed Codex app-server using your existing Codex login. No admin key is needed for this account report. It shows lifetime tokens, peak daily tokens, current/longest streaks, longest turn, and the daily history OpenAI returns. Missing values display as unavailable; missing dates are not treated as zero. The latest reported date is shown because the report can lag behind today's local activity.

The adjacent model/project rankings explain local Codex usage on this PC, including each item's token share. They are labelled all-time and are not represented as account-wide attribution. Account totals overlap local totals and are never added to them. API organization spending remains in its separate existing section and still requires an OpenAI admin key.

The tracker starts a short-lived hidden Codex app-server only to read this report, with a 25-second timeout, then closes that process. It does not start chats or consume resets. If the report fails, the rest of the tracker continues and the account section shows an unavailable message.

Reference: [OpenAI account usage API](https://learn.chatgpt.com/docs/app-server#7-token-usage-chatgpt).

## Where the numbers come from

- **Claude Code**: `~/.claude/projects/**/*.jsonl` (including subagents). Each API response is counted once.
- **Codex**: `~/.codex/sessions/**/*.jsonl[.zst]` and `~/.codex/archived_sessions/**/*.jsonl[.zst]`, or the same folders under `CODEX_HOME`. Includes subagents. Usage is the change in each session's running token total, using the session ID across continued files. Copies and inherited fork history are excluded. A normal session's first snapshot counts its complete total; a fork without copied history counts only its last request to avoid charging its parent's history again.
- Cached input is separated from uncached input. Reasoning is displayed separately but is already included in output and is never added twice. Usage records count log updates; an update can combine multiple requests.
- Compressed logs are streamed. Unreadable files produce an incomplete-data warning. A trailing partial JSON line in an active log is ignored until the next scan.
## Plan limits (live)

Each scan asks both services for your current plan limits, the same way their own apps do, using the
login each app saved on this PC. The tracker only reads those logins and never renews or changes them.

| Service | Address | Login read from |
|---|---|---|
| Claude Code | `https://api.anthropic.com/api/oauth/usage` | `~/.claude/.credentials.json` |
| Codex | `https://chatgpt.com/backend-api/wham/usage` | `~/.codex/auth.json` |

Live limits can include activity absent from local logs, such as other devices and service-hosted tasks. Codex shows the actual window durations returned by the service, additional buckets and Code Review limits when provided, credit balances, and explicit blocking/spend-control states. Tokens cannot be converted into subscription percentages; credit balances are separate units.
Claude also reports how your weekly use splits across Claude Code, chat and Cowork.

Both addresses are undocumented and may change. When a check fails (offline, login expired, address changed),
the widget shows the last live reading (saved in `%LOCALAPPDATA%\AIUsageTracker\limits_cache.json`) or, for
Codex, the latest session log, whichever is newer, labelled with its age, plus a warning saying why. An empty live response does not erase a useful earlier reading. After a reading's reset time passes, its last percentage remains visible with "current usage unknown"; it is never assumed to be zero.
Claude Code's saved login only lasts a few hours and is renewed while you use Claude Code; open Claude Code
(or Codex) to renew an expired one.

### Alerts, countdown and pace

- Each limit shows a live countdown to its reset ("resets Mon 02:00 · in 10h 06m").
- **Pace:** every live reading is saved in `%LOCALAPPDATA%\AIUsageTracker\limits_history.json` (8 days).
  From how fast a limit has risen recently (last hour; last day for weekly limits with over 5 hours left)
  the widget and dashboard show "At this pace: 100% by 16:01" in orange, or "On pace for ~60% by reset".
  It needs two live readings at least 10 minutes apart in the current window, and hides when the newest
  reading is over 30 minutes old or usage isn't rising.
- **Alerts:** a Windows notification when a limit passes 80%, 95% and 100%, once per threshold per window,
  including the pace when known. Only fresh live readings alert. The latest alert also stays in the widget's
  status line, in case Focus Assist hid the notification. Right-click → **Limit alerts** turns them off;
  what was already crossed is remembered in `limit_alerts.json`, so turning them back on doesn't replay old alerts.

Token totals cover the local logs only. Cloud-only activity and activity on other PCs can be missing. The service determines what contributes to each plan-limit bucket.

## API spend and balance

The widget and dashboard also show your **Anthropic API** and **OpenAI API** spend (today, this month,
last 30 days, daily chart) and token usage by model, from each provider's official admin usage reports.

1. Create an **admin** key (normal API keys are rejected):
   - Anthropic: [platform.claude.com/settings/admin-keys](https://platform.claude.com/settings/admin-keys)
     (needs an organization; individual accounts don't have admin keys)
   - OpenAI: [platform.openai.com/settings/organization/admin-keys](https://platform.openai.com/settings/organization/admin-keys)
2. Save it as a Windows **user** environment variable: `ANTHROPIC_ADMIN_KEY` and/or `OPENAI_ADMIN_KEY`
   (Start → "Edit environment variables for your account"). A running widget picks it up on its next scan.

Admin keys can read and manage your whole organization; the tracker only calls the read-only usage and
cost reports. Keys never leave your PC except to the provider they belong to, and are not written to any file.

**Balance (estimate):** neither provider publishes your prepaid credit balance through an API. Right-click
the widget → **Set API balance…** and type the balance your billing page shows now (or run
`python tracker.py --set-balance anthropic 25.00`). The tracker subtracts spend reported after that moment
(it remembers how much of today's spend was already in the balance). Re-enter it after a top-up or refund.
The reports run a few minutes behind and use UTC days. Saved in `%LOCALAPPDATA%\AIUsageTracker\api_balances.json`;
the last good report is kept in `api_cache.json` for when a check fails.

## Optional: API-equivalent cost

Fill in USD-per-million-token rates in `pricing.json` to add a cost column.

## Validation

Run `python -m unittest -v test_tracker test_api_billing test_widget test_limits_watch test_activity test_openai_analytics test_startup`
for accounting, limit-parser, pace/alert, activity, account analytics, start-with-Windows, API billing, and
background/tray lifecycle regressions. The tests use temporary folders and a throwaway registry key, never your own logs.

Official references: [Codex App Server](https://learn.chatgpt.com/docs/app-server) documents separate metered limit buckets, credit details, and window durations. [Pricing](https://learn.chatgpt.com/docs/pricing) explains tokens versus credits and why subscription consumption is not a fixed token allowance. The direct usage endpoint and local log formats remain implementation details that may change.

## License

[MIT](LICENSE)
