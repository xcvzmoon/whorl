"""Measure a running API and independent worker against a stated fixture corpus."""
import argparse
import concurrent.futures
import json
import subprocess
import threading
import time
import urllib.error
import urllib.request
from pathlib import Path


def request(url, data=None, headers=None):
    started = time.monotonic()
    try:
        with urllib.request.urlopen(urllib.request.Request(url, data, headers or {}), timeout=120) as response:
            return response.status, response.read(), time.monotonic() - started
    except urllib.error.HTTPError as error:
        return error.code, error.read(), time.monotonic() - started


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--url", required=True)
    parser.add_argument("--file", required=True, type=Path)
    parser.add_argument("--batches", type=int, default=10)
    parser.add_argument("--concurrency", type=int, default=2)
    parser.add_argument("--database-url", required=True, help="read operational gauges from the acceptance database")
    parser.add_argument("--container", required=True, help="Docker container holding API and worker under a shared cgroup")
    args = parser.parse_args()
    if args.batches < 1 or args.concurrency < 1:
        parser.error("batches and concurrency must be positive")
    document = args.file.read_bytes()
    records = []
    samples = []
    stop = threading.Event()

    def monitor():
        while not stop.is_set():
            raw = subprocess.check_output(["psql", "-X", "-w", "-At", args.database_url, "-c", "SELECT json_build_object('pending',count(*),'oldest_pending_seconds',coalesce(extract(epoch FROM clock_timestamp()-min(created_at)),0)) FROM batches WHERE state IN ('queued','running')"], timeout=10)
            sample = json.loads(raw)
            memory = subprocess.check_output(["docker", "exec", args.container, "cat", "/sys/fs/cgroup/memory.peak"], timeout=10)
            sample["cgroup_memory_peak_bytes"] = int(memory)
            samples.append(sample)
            stop.wait(0.2)

    monitor_pool = concurrent.futures.ThreadPoolExecutor(max_workers=1)
    monitoring = monitor_pool.submit(monitor)
    try:
        for files in [1, 5]:
            body = b"".join(b'--load\r\nContent-Disposition: form-data; name="files"; filename="sample"\r\n\r\n' + document + b'\r\n' for _ in range(files)) + b'--load--\r\n'

            def batch(_):
                started = time.monotonic()
                status, raw, admission = request(args.url + "/v1/checks", body, {"X-Client-Id": "acceptance", "Content-Type": "multipart/form-data; boundary=load"})
                record = {"files": files, "http_status": status, "admission_seconds": admission}
                if status != 202:
                    return record
                admitted = json.loads(raw)
                status_url = args.url + admitted["status_url"]
                health_samples = []
                poll_samples = []
                while time.monotonic() - started < 360:
                    health, _, latency = request(args.url + "/health/ready")
                    health_samples.append(latency)
                    if health != 200:
                        raise RuntimeError(f"health returned {health}")
                    status, raw, latency = request(status_url, headers={"X-Client-Id": "acceptance"})
                    poll_samples.append(latency)
                    if status != 200:
                        raise RuntimeError(f"poll returned {status}")
                    result = json.loads(raw)
                    if all(file["state"] in ("completed", "failed") for file in result["files"]):
                        record.update(total_seconds=time.monotonic() - started, terminal_states=[file["state"] for file in result["files"]], health_max_seconds=max(health_samples), poll_max_seconds=max(poll_samples))
                        return record
                    time.sleep(0.1)
                raise TimeoutError("batch did not finish within 360 seconds")

            with concurrent.futures.ThreadPoolExecutor(max_workers=args.concurrency) as executor:
                records.extend(executor.map(batch, range(args.batches)))
    finally:
        stop.set()
        monitoring.result()
        monitor_pool.shutdown()
    report = {"operational_samples": samples, "file": str(args.file), "bytes_per_file": len(document), "batches_per_size": args.batches, "concurrency": args.concurrency, "records": records}
    print(json.dumps(report, indent=2))
    if any(record["http_status"] != 202 or any(state != "completed" for state in record.get("terminal_states", [])) for record in records):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
