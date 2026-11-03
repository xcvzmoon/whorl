#!/usr/bin/env python3
"""Run against a fresh disposable PostgreSQL database supplied by the caller."""
import json
import os
from pathlib import Path
import signal
import subprocess
import time

ROOT = Path(__file__).resolve().parent
URL = os.environ['WHORL_QUEUE_TEST_URL']
BIN = ROOT / 'target/debug/queue-spike'
processes = []
results = {}


def sql(statement):
    return subprocess.check_output(
        ['psql', '-X', '-w', '-qAt', '-v', 'ON_ERROR_STOP=1', URL, '-c', statement],
        text=True, timeout=10).strip()


def wait_for(query, expected, timeout=20):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if sql(query) == expected:
            return
        time.sleep(0.1)
    raise AssertionError((query, expected, sql(query)))


def worker(poll=False):
    env = dict(os.environ)
    env.pop('QUEUE_POLL_ONLY', None)
    if poll:
        env['QUEUE_POLL_ONLY'] = '1'
    p = subprocess.Popen([str(BIN), 'run'], env=env)
    processes.append(p)
    time.sleep(1)
    assert p.poll() is None
    return p


def stop(p):
    p.send_signal(signal.SIGTERM)
    p.wait(timeout=12)
    assert p.returncode == 0


def enqueue(label, seconds=0, fail=False, attempts=3, key=None, schedule='now()'):
    payload = json.dumps(dict(label=label, seconds=seconds, fail=fail))
    key_sql = 'NULL' if key is None else "'" + key + "'"
    return sql(f"SELECT (queue_spike.add_job('probe', '{payload}'::json, "
               f"run_at := {schedule}, max_attempts := {attempts}, job_key := {key_sql})).id")


def count(label, event):
    return f"SELECT count(*) FROM queue_probe_events WHERE label='{label}' AND event='{event}'"


try:
    subprocess.run([str(BIN), 'migrate'], check=True)
    sql('CREATE TABLE queue_probe_events(label text, event text, job_id bigint, at timestamptz DEFAULT clock_timestamp())')
    a = enqueue('key', key='pending-key')
    b = enqueue('key', key='pending-key')
    assert a == b
    results['pending_key_same_id'] = True
    sql("BEGIN; SELECT queue_spike.add_job('probe', '{\"label\":\"rollback\",\"seconds\":0,\"fail\":false}'); ROLLBACK;")
    assert sql("SELECT count(*) FROM queue_spike._private_jobs WHERE payload->>'label'='rollback'") == '0'
    results['transaction_rollback'] = True
    subprocess.run([str(BIN), 'utility'], check=True)
    subprocess.run(['psql', '-X', '-w', URL, '-f', str(ROOT / 'privileges.sql')], check=True)
    results['utility_transaction_rollback'] = True
    results['enqueue_role_grants_and_rls'] = True
    p = worker()
    wait_for(count('key', 'finish'), '1')
    wait_for('SELECT count(*) FROM queue_spike._private_jobs', '0')
    results['success_deleted'] = True
    start = time.monotonic()
    enqueue('notification')
    wait_for(count('notification', 'finish'), '1')
    results['notification_observed_seconds'] = round(time.monotonic() - start, 3)
    enqueue('retry', fail=True)
    wait_for(count('retry', 'start'), '3', 35)
    wait_for("SELECT count(*) FROM queue_spike._private_jobs WHERE attempts=3 AND last_error IS NOT NULL AND locked_at IS NULL", '1')
    results['retry_intervals_seconds'] = sql("SELECT string_agg(delta::text, ',') FROM (SELECT round(extract(epoch FROM at-lag(at) OVER (ORDER BY at))::numeric,3) delta FROM queue_probe_events WHERE label='retry' AND event='start') s")
    stop(p)
    p = worker(True)
    enqueue('poll', schedule="now() + interval '1 second'")
    wait_for(count('poll', 'finish'), '1')
    results['poll_fallback'] = True
    first = enqueue('running-key', seconds=3, key='running-key')
    wait_for(count('running-key', 'start'), '1')
    second = enqueue('running-key', seconds=3, key='running-key')
    assert first != second
    wait_for(count('running-key', 'finish'), '2')
    results['running_key_creates_successor'] = True
    enqueue('long', seconds=185)
    wait_for(count('long', 'start'), '1')
    q = worker()
    deadline = time.monotonic() + 180
    while time.monotonic() < deadline:
        assert sql(count('long', 'start')) == '1'
        assert p.poll() is None and q.poll() is None
        time.sleep(1)
    wait_for(count('long', 'finish'), '1', 15)
    results['live_job_185_seconds_single_start'] = True
    stop(q)
    enqueue('killed', seconds=30)
    wait_for(count('killed', 'start'), '1')
    p.kill()
    p.wait()
    start = time.monotonic()
    q = worker()
    wait_for(count('killed', 'start'), '2', 20)
    results['kill_recovery_seconds'] = round(time.monotonic() - start, 3)
    wait_for(count('killed', 'finish'), '1', 40)
    enqueue('shutdown', seconds=30)
    wait_for(count('shutdown', 'start'), '1')
    start = time.monotonic()
    stop(q)
    results['shutdown_seconds'] = round(time.monotonic() - start, 3)
    assert sql("SELECT count(*) FROM queue_spike._private_jobs WHERE payload->>'label'='shutdown' AND locked_at IS NULL AND run_at > now()") == '1'
    results['shutdown_released_with_delay'] = True
    assert sql("SELECT count(*) FROM queue_spike._private_jobs WHERE payload->>'label'='retry' AND attempts=3 AND last_error IS NOT NULL") == '1'
    results['failed_retained_after_minutes'] = True
    print(json.dumps(results, indent=2), flush=True)
finally:
    for p in processes:
        if p.poll() is None:
            p.terminate()
            try:
                p.wait(timeout=12)
            except subprocess.TimeoutExpired:
                p.kill()
                p.wait()
