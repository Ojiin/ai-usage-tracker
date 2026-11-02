"""Activity regressions: tool calls, agents, MCP servers, slash commands, hours, working-now.
Uses temp folders only; never the user's logs."""
import contextlib
import json
import sqlite3
import os
import tempfile
import time
import unittest
from pathlib import Path
from unittest.mock import patch

import tracker


def write(path, rows):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text('\n'.join(json.dumps(r) for r in rows) + '\n', encoding='utf-8')


def assistant(msg_id, blocks, ts='2026-11-01T10:00:00Z'):
    return {'type': 'assistant', 'timestamp': ts, 'requestId': 'r-' + msg_id, 'cwd': '/p/demo',
            'message': {'id': msg_id, 'model': 'claude-opus-5-5', 'content': blocks,
                        'usage': {'input_tokens': 10, 'output_tokens': 5}}}


def tool_use(call_id, name, **args):
    return {'type': 'tool_use', 'id': call_id, 'name': name, 'input': args}


def counts(events, kind):
    out = {}
    for e in events:
        if e['kind'] == kind:
            out[(e['name'], e['tool'])] = out.get((e['name'], e['tool']), 0) + 1
    return out


class ActivityTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        root = Path(self.tmp.name)
        self.claude, self.codex = root / 'claude' / 'projects', root / 'codex' / 'sessions'
        self.patches = [patch.object(tracker, 'CLAUDE_DIR', self.claude), patch.object(tracker, 'CODEX_DIR', self.codex),
                        patch.object(tracker, 'CLAUDE_HISTORY', root / 'claude' / 'history.jsonl'),
                        patch.object(tracker, 'CODEX_HISTORY', root / 'codex' / 'history.jsonl')]
        for p in self.patches:
            p.start()

    def tearDown(self):
        for p in self.patches:
            p.stop()
        self.tmp.cleanup()

    def test_claude_tool_calls_counted_once_with_agents_skills_and_mcp(self):
        # Claude Code repeats a message on one line per content block; the same tool_use id must count once.
        call = tool_use('t1', 'Bash', command='ls')
        write(self.claude / 'proj' / 's1.jsonl', [
            assistant('m1', [call]), assistant('m1', [call]),
            assistant('m2', [tool_use('t2', 'Agent', subagent_type='Explore'), tool_use('t3', 'Task'),
                             tool_use('t4', 'Skill', skill='pdf'), tool_use('t5', 'mcp__rea__open_binary')]),
        ])
        events = []
        records = tracker.scan_claude(events)
        self.assertEqual(len(records), 2)
        self.assertEqual(counts(events, 'tool'), {('Bash', 'Claude Code'): 1, ('Agent', 'Claude Code'): 1,
                                                  ('Task', 'Claude Code'): 1, ('Skill', 'Claude Code'): 1,
                                                  ('MCP: rea', 'Claude Code'): 1})
        self.assertEqual(counts(events, 'agent'), {('Explore', 'Claude Code'): 1, ('general-purpose', 'Claude Code'): 1})
        self.assertEqual(counts(events, 'skill'), {('pdf', 'Claude Code'): 1})
        self.assertEqual(counts(events, 'mcp'), {('rea', 'Claude Code'): 1})
        self.assertEqual(tracker.scan_claude(), records)  # events are optional

    def test_codex_calls_mcp_namespace_agents_and_fork_history(self):
        ts = '2026-11-01T10:05:00Z'
        def call(call_id, name, namespace=None, when=ts):
            return {'type': 'response_item', 'timestamp': when,
                    'payload': {'type': 'function_call', 'name': name, 'namespace': namespace, 'call_id': call_id}}
        write(self.codex / 'a.jsonl', [
            {'type': 'session_meta', 'timestamp': ts, 'payload': {'id': 's', 'cwd': '/p/x'}},
            call('c1', 'exec'), call('c1', 'exec'), call('c2', 'js', 'mcp__cua_repl'), call('c3', 'spawn_agent', 'collaboration'),
        ])
        write(self.codex / 'b.jsonl', [  # a fork: copied parent history before its start is not counted again
            {'type': 'session_meta', 'timestamp': ts, 'payload': {'id': 'f', 'forked_from_id': 's', 'timestamp': ts}},
            call('c9', 'exec', when='2026-11-01T10:00:00Z'), call('c10', 'exec', when='2026-11-01T10:06:00Z'),
        ])
        events = []
        tracker.scan_codex(events)
        self.assertEqual(counts(events, 'tool'), {('exec', 'Codex'): 2, ('MCP: cua_repl', 'Codex'): 1, ('spawn_agent', 'Codex'): 1})
        self.assertEqual(counts(events, 'mcp'), {('cua_repl', 'Codex'): 1})
        self.assertEqual(counts(events, 'agent'), {('Codex subagent', 'Codex'): 1})

    def test_only_slash_command_names_are_read_from_history(self):
        ms = int(time.mktime((2026, 11, 1, 9, 0, 0, 0, 0, -1)) * 1000)
        write(tracker.CLAUDE_HISTORY, [{'display': '/model opus', 'timestamp': ms}, {'display': 'fix the bug', 'timestamp': ms},
                                       {'display': '/c/Users/me/file.txt is broken', 'timestamp': ms}])
        write(tracker.CODEX_HISTORY, [{'text': '/review please', 'ts': ms // 1000}])
        events = []
        tracker.scan_commands(events)
        self.assertEqual(events, [{'tool': 'Claude Code', 'day': '2026-11-01', 'kind': 'command', 'name': '/model'},
                                  {'tool': 'Codex', 'day': '2026-11-01', 'kind': 'command', 'name': '/review'}])

    def test_aggregate_activity_hours_and_year_top(self):
        day, hour, dow = tracker.local_time('2026-11-01T10:00:00Z')
        rec = {'tool': 'Claude Code', 'day': day, 'hour': hour, 'dow': dow, 'model': 'm', 'project': 'p',
               'session': 's', 'input': 10, 'cache_write': 0, 'cache_read': 0, 'output': 5}
        events = [{'tool': 'Codex', 'day': day, 'kind': 'tool', 'name': 'exec'}] * 3
        out = tracker.aggregate([rec, rec], {}, events)
        self.assertEqual(out['day_hours'][day]['Claude Code'][hour], 30)
        self.assertEqual(sum(out['day_hours'][day]['Claude Code']), 30)
        self.assertEqual(out['activity'], {'tool': {'exec|Codex': {day: 3}}})
        self.assertEqual(out['year_top'][day[:4]]['model'], [['m|Claude Code', 30]])

    def test_longest_turn_from_turn_duration_rows_counted_once(self):
        turn = {'type': 'system', 'subtype': 'turn_duration', 'durationMs': 90000, 'uuid': 'u1', 'timestamp': '2026-11-01T10:00:00Z'}
        write(self.claude / 'proj' / 's1.jsonl', [turn, turn, dict(turn, uuid='u2', durationMs=30000), assistant('m1', [])])
        events = []
        records = tracker.scan_claude(events)
        self.assertEqual(len(records), 1)
        out = tracker.aggregate(records, {}, events)
        self.assertEqual(out['longest_turn_ms'], {'Claude Code': 90000})
        self.assertNotIn('turn', out['activity'])

    def test_claude_account_split_and_extra_usage_spend(self):
        body = {'limits': [{'kind': 'weekly_all', 'percent': 30, 'resets_at': '2026-11-03T00:00:00Z'}],
                'seven_day_breakdown': {'rows': [{'key': 'claude_code', 'display_name': 'Claude Code', 'percent': 25}]},
                'spend': {'enabled': True, 'used': {'amount_minor': 1234, 'currency': 'USD', 'exponent': 2}}}
        creds = Path(self.tmp.name) / 'creds.json'
        creds.write_text(json.dumps({'claudeAiOauth': {'accessToken': 'x', 'expiresAt': 9e15}}), encoding='utf-8')
        with patch.object(tracker, 'CLAUDE_CREDENTIALS', creds), patch.object(tracker, 'get_json', return_value=(body, None)):
            live, error = tracker.claude_live()
        self.assertIsNone(error)
        self.assertEqual(live['breakdown'], [{'key': 'claude_code', 'name': 'Claude Code', 'percent': 25}])
        self.assertEqual(live['spend'], {'used': 12.34, 'currency': 'USD'})
        body['spend']['enabled'] = False
        with patch.object(tracker, 'CLAUDE_CREDENTIALS', creds), patch.object(tracker, 'get_json', return_value=(body, None)):
            self.assertIsNone(tracker.claude_live()[0]['spend'])

    def test_working_details_describe_model_effort_project_and_subagents(self):
        main = self.claude / 'proj' / 's1.jsonl'
        write(main, [
            {'type': 'custom-title', 'customTitle': 'Usage tracker'},
            {'type': 'last-prompt', 'lastPrompt': 'add a hover card'},
            dict(assistant('m1', []), effort='medium'),
        ])
        sub = self.claude / 'proj' / 's1' / 'subagents' / 'agent-a.jsonl'
        write(sub, [assistant('m2', [])])
        sub.with_suffix('.meta.json').write_text(json.dumps({'agentType': 'Explore', 'description': 'find the widget code'}), encoding='utf-8')
        parent_id, child_id = 'p-1', 'c-1'
        write(self.codex / 'parent.jsonl', [
            {'type': 'session_meta', 'payload': {'id': parent_id, 'cwd': 'C:\\\\work\\\\demo'}},
            {'type': 'turn_context', 'payload': {'model': 'gpt-6.1-sol', 'effort': 'high', 'cwd': 'C:\\\\work\\\\demo'}}])
        write(self.codex / 'child.jsonl', [
            {'type': 'session_meta', 'payload': {'id': child_id, 'parent_thread_id': parent_id, 'agent_nickname': 'Ada',
                                                 'thread_source': 'subagent'}},
            {'type': 'turn_context', 'payload': {'model': 'gpt-6.1-sol', 'effort': 'medium'}}])
        write(self.codex.parent / 'session_index.jsonl', [{'id': parent_id, 'thread_name': 'Speed up import'}])
        details = {d['tool']: d for d in tracker.working_details()}
        self.assertEqual(len(details), 2)  # subagents fold into their parent sessions
        claude = tracker.describe_session(details['Claude Code']).split('\n')
        self.assertEqual(claude[0], 'Opus 5.5 med working on demo')
        self.assertIn('  “Usage tracker”', claude)
        self.assertIn('  Last prompt: add a hover card', claude)
        self.assertIn('  ↳ Explore agent · Opus 5.5: find the widget code', claude)
        codex = tracker.describe_session(details['Codex']).split('\n')
        self.assertEqual(codex[0], 'gpt-6.1-sol high working on demo')
        self.assertIn('  “Speed up import”', codex)
        self.assertIn('  ↳ Ada (subagent) · gpt-6.1-sol med', codex)
        self.assertEqual(tracker.working_now(), {'Claude Code': 1, 'Codex': 1})

    def test_pretty_model_names(self):
        self.assertEqual(tracker.pretty_model('claude-opus-5-5'), 'Opus 5.5')
        self.assertEqual(tracker.pretty_model('claude-sonnet-5'), 'Sonnet 5')
        self.assertEqual(tracker.pretty_model('gpt-6.1-sol'), 'gpt-6.1-sol')

    def test_codex_database_threads_count_only_while_a_turn_is_in_progress(self):
        # Newer Codex writes turns to thread_history_N.sqlite, not its rollout log, while it works.
        now = time.time()
        done = '01a19c84-0000-7000-8000-000000000001'  # rollout logs end in the thread's 36-character id
        home = self.codex.parent
        home.mkdir(parents=True, exist_ok=True)
        with contextlib.closing(sqlite3.connect(home / 'state_5.sqlite')) as db:
            db.execute('CREATE TABLE threads (id TEXT, history_mode TEXT, model TEXT, reasoning_effort TEXT, cwd TEXT, name TEXT, '
                       'title TEXT, thread_source TEXT, agent_nickname TEXT, attached_thread_parent_id TEXT, updated_at_ms INTEGER)')
            db.execute('CREATE TABLE thread_spawn_edges (parent_thread_id TEXT, child_thread_id TEXT, status TEXT)')
            db.executemany('INSERT INTO threads VALUES (?,?,?,?,?,?,?,?,?,?,?)', [
                ('t-main', 'paginated', 'gpt-6-astra', 'ultra', 'C:\\work\\photo', None, 'Review Photul', 'user', None, None, now * 1000),
                ('t-kid', 'paginated', 'gpt-6-astra', 'high', 'C:\\work\\photo', None, '', 'subagent', 'Faraday', None, now * 1000),
                (done, 'paginated', 'gpt-6-astra', 'medium', 'C:\\work\\x', None, 'Finished', 'user', None, None, now * 1000),
                ('t-crashed', 'paginated', 'gpt-6-astra', 'medium', 'C:\\work\\x', None, 'Old', 'user', None, None, (now - 7200) * 1000)])
            db.execute("INSERT INTO thread_spawn_edges VALUES ('t-main', 't-kid', 'open')")
            db.commit()
        with contextlib.closing(sqlite3.connect(home / 'thread_history_1.sqlite')) as db:
            db.execute('CREATE TABLE thread_turns (thread_id TEXT, status TEXT)')
            db.execute('CREATE TABLE thread_items (thread_id TEXT, item_type TEXT, item_json TEXT, rollout_ordinal INTEGER, created_at_ms INTEGER)')
            db.executemany('INSERT INTO thread_turns VALUES (?,?)', [
                ('t-main', 'completed'), ('t-main', 'inProgress'), ('t-kid', 'inProgress'), (done, 'completed'), ('t-crashed', 'inProgress')])
            prompt = lambda text: json.dumps({'type': 'userMessage', 'content': [{'type': 'text', 'text': text}]})
            db.executemany('INSERT INTO thread_items VALUES (?,?,?,?,?)', [
                ('t-main', 'userMessage', prompt('first ask'), 1, now * 1000), ('t-main', 'userMessage', prompt('find the bugs'), 5, now * 1000),
                ('t-kid', 'userMessage', prompt('check the exporter'), 1, now * 1000)])
            db.commit()
        write(self.codex / '2026' / f'rollout-2026-11-02T08-00-00-{done}.jsonl', [{}])  # rewritten just now, yet idle
        self.assertEqual(tracker.working_now(), {'Claude Code': 0, 'Codex': 1})
        lines = tracker.describe_session(tracker.working_details()[0]).split('\n')
        self.assertEqual(lines[0], 'gpt-6-astra ultra working on photo')
        self.assertIn('  “Review Photul”', lines)
        self.assertIn('  Last prompt: find the bugs', lines)
        self.assertIn('  ↳ Faraday (subagent) · gpt-6-astra high: check the exporter', lines)

    def test_working_now_counts_recent_sessions_and_subagents_as_parent(self):
        fresh, old = time.time(), time.time() - 600
        for path, when in ((self.claude / 'p' / 's1.jsonl', fresh), (self.claude / 'p' / 's1' / 'subagents' / 'a.jsonl', fresh),
                           (self.claude / 'p' / 's2.jsonl', old), (self.codex / '2026' / 'c.jsonl', fresh)):
            write(path, [{}])
            os.utime(path, (when, when))
        self.assertEqual(tracker.working_now(), {'Claude Code': 1, 'Codex': 1})


if __name__ == '__main__':
    unittest.main()
