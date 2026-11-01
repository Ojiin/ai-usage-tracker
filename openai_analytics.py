"""Read-only Codex account analytics through the documented local app-server API."""
import json
import os
import queue
import shutil
import subprocess
import threading
import time
from datetime import datetime, timezone
from pathlib import Path


def codex_executable():
    found = shutil.which('codex')
    if found:
        return found
    root = Path(os.environ.get('LOCALAPPDATA', Path.home())) / 'OpenAI/Codex/bin'
    candidates = list(root.glob('*/codex.exe'))
    return str(max(candidates, key=lambda p: p.stat().st_mtime)) if candidates else None


def read_account(timeout=25):
    executable = codex_executable()
    if not executable:
        raise RuntimeError('Codex app-server was not found. Open the Codex app or install the Codex CLI.')
    proc = subprocess.Popen([executable, 'app-server'], stdin=subprocess.PIPE,
                            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                            text=True, encoding='utf-8',
                            creationflags=getattr(subprocess, 'CREATE_NO_WINDOW', 0))
    messages = queue.Queue()
    def receive():
        try:
            for line in proc.stdout:
                try:
                    messages.put(json.loads(line))
                except ValueError:
                    pass
        finally:
            messages.put(None)
    reader = threading.Thread(target=receive, daemon=True)
    reader.start()
    deadline = time.monotonic() + timeout
    def send(body):
        proc.stdin.write(json.dumps(body) + '\n')
        proc.stdin.flush()
    def request(identifier, method, params=None):
        send({'id': identifier, 'method': method, 'params': params})
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise RuntimeError('OpenAI account analytics timed out; try the next refresh.')
            try:
                row = messages.get(timeout=remaining)
            except queue.Empty:
                raise RuntimeError('OpenAI account analytics timed out; try the next refresh.') from None
            if row is None:
                raise RuntimeError('Codex app-server closed before returning account analytics.')
            if row.get('id') == identifier:
                if 'error' in row:
                    code = (row['error'] or {}).get('code')
                    raise RuntimeError(f'OpenAI analytics unavailable (RPC {code}). Check Codex login and version.')
                return row.get('result') or {}
    try:
        request(1, 'initialize', {'clientInfo': {'name': 'ai_usage_tracker', 'version': '2.0'},
                                  'capabilities': {'experimentalApi': True}})
        send({'method': 'initialized'})
        usage = request(2, 'account/usage/read')
        return usage
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=3)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait(timeout=3)
        reader.join(timeout=1)
        proc.stdin.close()
        proc.stdout.close()


def normalize(body):
    summary = body.get('summary') or {}
    fields = ('lifetimeTokens', 'peakDailyTokens', 'longestRunningTurnSec',
              'currentStreakDays', 'longestStreakDays')
    clean = {k: v if isinstance(v := summary.get(k), (int, float)) and not isinstance(v, bool) and v >= 0 else None for k in fields}
    buckets = body.get('dailyUsageBuckets')
    daily = None
    if isinstance(buckets, list):
        daily = {}
        for row in buckets:
            day, count = row.get('startDate'), row.get('tokens')
            try:
                datetime.strptime(day, '%Y-%m-%d')
            except (TypeError, ValueError):
                continue
            if isinstance(count, (int, float)) and not isinstance(count, bool) and count >= 0:
                daily[day] = daily.get(day, 0) + count
        daily = dict(sorted(daily.items()))
    return {'summary': clean, 'daily': daily}


def collect(log=print):
    log('Fetching OpenAI account analytics ...')
    try:
        data = normalize(read_account())
        data.update(source='live', as_of=datetime.now(timezone.utc).isoformat(), error=None)
        log('  ok')
        return data
    except (RuntimeError, OSError, ValueError, TypeError) as exc:
        log('  account analytics unavailable')
        return {'summary': {}, 'daily': None, 'source': None, 'as_of': None,
                'error': str(exc) if isinstance(exc, RuntimeError) else 'Could not read OpenAI account analytics.'}
