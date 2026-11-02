import argparse
import bisect
from collections import defaultdict
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import resource
import signal
import subprocess
import tempfile
import time
import unicodedata

ROOT = Path(__file__).resolve().parent
NORMALIZATION = "corpus-nfc-quotes-hyphenation-whitespace-v1"
LIMITS = {"wall_seconds": 60, "cpu_seconds": 60, "output_file_bytes": 16 * 1024 * 1024,
          "input_bytes": 20971520, "archive_bytes": 67108864, "archive_member_bytes": 33554432,
          "archive_ratio": 100, "archive_entries": 4096, "xml_depth": 128, "image_pixels": 25000000,
          "decoded_image_bytes": 268435456, "pdf_pages": 100, "sheets": 256,
          "cells_per_sheet": 1000000, "text_bytes": 8388608}


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def normalize(text, fold=False):
    text = unicodedata.normalize("NFC", text).translate(str.maketrans({"‘": "'", "’": "'", "“": '"', "”": '"'}))
    text = re.sub(r"(?<=\w)-\r?\n(?=\w)", "", text)
    text = " ".join(text.split())
    return text.lower() if fold else text


def distance(a, b):
    if a == b:
        return 0
    prefix = 0
    while prefix < min(len(a), len(b)) and a[prefix] == b[prefix]:
        prefix += 1
    a, b = a[prefix:], b[prefix:]
    suffix = 0
    while suffix < min(len(a), len(b)) and a[-suffix - 1] == b[-suffix - 1]:
        suffix += 1
    if suffix:
        a, b = a[:-suffix], b[:-suffix]
    if len(a) < len(b):
        a, b = b, a
    row = list(range(len(b) + 1))
    for i, x in enumerate(a, 1):
        new = [i]
        for j, y in enumerate(b, 1):
            new.append(min(row[j] + 1, new[-1] + 1, row[j - 1] + (x != y)))
        row = new
    return row[-1]


def lcs_length(a, b):
    if a == b:
        return len(a)
    positions = defaultdict(list)
    for index, token in enumerate(b):
        positions[token].append(index)
    tails = []
    for token in a:
        for index in reversed(positions[token]):
            place = bisect.bisect_left(tails, index)
            if place == len(tails):
                tails.append(index)
            else:
                tails[place] = index
    return len(tails)


def metadata_value(value):
    if isinstance(value, str):
        return normalize(value)
    if isinstance(value, list):
        return [metadata_value(item) for item in value]
    return value


def metrics(case, result):
    expected = case["expected"]
    fields = expected.get("metadata", {})
    metadata = result.get("metadata", {})
    output = {"wer": None, "cer": None, "coverage": None, "metadata_accuracy": None,
              "comparison_scope": expected.get("comparison"), "null_reasons": {}}
    if result["outcome"] not in ("success", "no_extractable_text"):
        output["null_reasons"] = {key: "failed" for key in ("wer", "cer", "coverage", "metadata_accuracy")}
        return output
    if fields:
        output["metadata_accuracy"] = sum(metadata_value(metadata.get(k)) == metadata_value(v) for k, v in fields.items()) / len(fields)
    else:
        output["null_reasons"]["metadata_accuracy"] = "no_expected_fields"
    if not expected.get("text_path"):
        output["null_reasons"].update({key: "no_reference" for key in ("wer", "cer", "coverage")})
        return output
    reference = (ROOT / expected["text_path"]).read_text()
    fold = expected.get("comparison") == "ocr-page"
    actual = normalize(result.get("text", ""), fold)
    target = normalize(reference, fold)
    if not target:
        output["null_reasons"].update({key: "empty_reference" for key in ("wer", "cer", "coverage")})
    elif expected["comparison"] in ("anchors", "unordered-anchors"):
        anchors = [normalize(line) for line in reference.splitlines() if normalize(line)]
        output["coverage"] = sum(anchor in actual for anchor in anchors) / len(anchors)
        output["null_reasons"].update(wer="partial_reference", cer="partial_reference")
    else:
        words, actual_words = target.split(), actual.split()
        output["wer"] = distance(words, actual_words) / len(words)
        output["cer"] = distance(target, actual) / len(target)
        output["coverage"] = lcs_length(words, actual_words) / len(words)
    return output


def candidates(case):
    return {"pdf": ["lopdf", "pdfium"] + (["pdfium-ocrs", "pdfium-tesseract"] if case["id"] == "scan-pdf" else []),
            "docx": ["ooxml"], "pptx": ["ooxml"], "xlsx": ["calamine"],
            "png": ["image-ocrs", "image-ocrs-filtered", "image-tesseract", "image-tesseract-sparse"], "jpeg": ["image-ocrs", "image-ocrs-filtered", "image-tesseract", "image-tesseract-sparse"]}[case["format"]]


def child_limits():
    resource.setrlimit(resource.RLIMIT_CPU, (LIMITS["cpu_seconds"], LIMITS["cpu_seconds"] + 1))
    resource.setrlimit(resource.RLIMIT_FSIZE, (LIMITS["output_file_bytes"], LIMITS["output_file_bytes"]))
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))


def group_rss(group):
    rss = 0
    for entry in Path("/proc").iterdir():
        if not entry.name.isdigit():
            continue
        try:
            stat = (entry / "stat").read_text().rsplit(")", 1)[1].split()
            if int(stat[2]) == group:
                rss += int(stat[21]) * os.sysconf("SC_PAGE_SIZE")
        except (FileNotFoundError, ProcessLookupError, PermissionError):
            pass
    return rss


def bounded_process(command, stdout, stderr, scratch, wall_seconds):
    start = time.monotonic()
    process = subprocess.Popen(command, stdout=stdout, stderr=stderr, cwd=scratch,
                               start_new_session=True, preexec_fn=child_limits)
    peak = 0
    timed_out = False
    while process.poll() is None:
        peak = max(peak, group_rss(process.pid))
        if time.monotonic() - start > wall_seconds:
            timed_out = True
            os.killpg(process.pid, signal.SIGKILL)
            break
        time.sleep(0.002)
    process.wait()
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    elapsed = (time.monotonic() - start) * 1000
    return process.returncode, timed_out, elapsed, peak


def run_trial(case, candidate, repetition, destination):
    key = f"{case['id']}/{candidate}/{repetition}"
    directory = destination / "raw" / key
    directory.mkdir(parents=True, exist_ok=False)
    command = ["/usr/bin/time", "-v", "-o", "usage.txt", "/usr/local/bin/extraction-spike", candidate, case["format"], str(ROOT / case["path"])]
    started = datetime.now(timezone.utc).isoformat()
    with tempfile.TemporaryDirectory() as scratch, (directory / "stdout.json").open("wb") as stdout, (directory / "stderr.txt").open("wb") as stderr:
        returncode, timed_out, elapsed, peak = bounded_process(command, stdout, stderr, scratch, LIMITS["wall_seconds"])
        usage = Path(scratch) / "usage.txt"
        usage_text = usage.read_text() if usage.exists() else ""
        (directory / "usage.txt").write_text(usage_text)
        match = re.search(r"Maximum resident set size \(kbytes\): (\d+)", usage_text)
        individual_peak = int(match.group(1)) * 1024 if match else None
        peak = max(peak, individual_peak or 0)
    result = {"outcome": "extraction_limit" if timed_out else "extraction_failed", "text": "", "metadata": {}}
    if returncode == 0:
        try:
            result = json.loads((directory / "stdout.json").read_text())
        except (ValueError, UnicodeError):
            pass
    (directory / "text.txt").write_text(result.get("text", ""))
    (directory / "metadata.json").write_text(json.dumps(result.get("metadata", {}), ensure_ascii=False, indent=2) + "\n")
    artifacts = {}
    for name in ("stdout.json", "stderr.txt", "text.txt", "metadata.json", "usage.txt"):
        file = directory / name
        artifacts[name] = {"path": str(file.relative_to(destination)), "sha256": sha(file)}
    return {"case_id": case["id"], "sha256": case["sha256"], "candidate": candidate,
            "machine_id": "linux-arm64-2cpu", "started_at": started, "repetition": repetition,
            "warmup": repetition == 0, "cache_state": "OS cache uncontrolled; process and models cold",
            "command": command, "limits": LIMITS, "wall_ms": elapsed, "peak_process_tree_rss_bytes": peak,
            "max_individual_rss_bytes": individual_peak,
            "rss_collector": "max of >=2ms /proc process-group summed RSS samples and GNU time maximum individual high-water RSS (KiB converted to bytes); shared pages counted per process; simultaneous short peaks may be missed",
            "exit_status": returncode, "timed_out": timed_out,
            "outcome": result["outcome"], "error": result.get("error"), "artifacts": artifacts,
            "normalization_version": NORMALIZATION, **metrics(case, result)}


def machine():
    cgroup = Path("/sys/fs/cgroup")
    return {"id": "linux-arm64-2cpu", "os": platform.platform(), "architecture": platform.machine(),
            "cpu": "Apple M1 via Docker Desktop Linux VM", "available_logical_cpus": os.cpu_count(),
            "allocated_cpu_cores": 2, "memory_limit_bytes": int((cgroup / "memory.max").read_text()),
            "cpu_max": (cgroup / "cpu.max").read_text().strip(), "memory_swap_max": (cgroup / "memory.swap.max").read_text().strip(),
            "vm_memory": next(line for line in Path('/proc/meminfo').read_text().splitlines() if line.startswith('MemTotal:')),
            "target": True, "worker_processes": 1, "OMP_THREAD_LIMIT": os.getenv("OMP_THREAD_LIMIT"),
            "RAYON_NUM_THREADS": os.getenv("RAYON_NUM_THREADS"), "RTEN_NUM_THREADS": os.getenv("RTEN_NUM_THREADS"),
            "python": platform.python_version()}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("destination", type=Path)
    parser.add_argument("--case")
    args = parser.parse_args()
    args.destination.mkdir(parents=True, exist_ok=True)
    manifest = json.loads((ROOT / "manifest.json").read_text())
    trials = []
    provenance = {"lockfile_sha256": sha(ROOT / "Cargo.lock"), "binary_sha256": sha('/usr/local/bin/extraction-spike'),
                  "assets": json.loads((ROOT / "audit/assets.json").read_text()),
                  "source_sha256": {str(p.relative_to(ROOT)): sha(p) for p in sorted((ROOT / "src").glob("*.rs"))},
                  "packages": subprocess.check_output(["dpkg-query", "-W"], text=True),
                  "tesseract": subprocess.check_output(["tesseract", "--version"], text=True),
                  "dependencies": json.loads((ROOT / "dependencies.json").read_text()),
                  "harness_sha256": sha(ROOT / "benchmark.py")}
    output = {"schema_version": 1, "machines": [machine()], "provenance": provenance, "trials": trials, "status": "running"}
    for case in manifest["cases"]:
        if args.case and case["id"] != args.case:
            continue
        if sha(ROOT / case["path"]) != case["sha256"]:
            raise ValueError(f"Fixture checksum mismatch: {case['id']}")
        for candidate in candidates(case):
            for repetition in range(6):
                trial = run_trial(case, candidate, repetition, args.destination)
                trial["lockfile_sha256"] = provenance["lockfile_sha256"]
                names = {"lopdf": ["lopdf"], "pdfium": ["pdfium-render"],
                         "pdfium-ocrs": ["pdfium-render", "image", "ocrs", "rten", "kamadak-exif"],
                         "pdfium-tesseract": ["pdfium-render", "image", "kamadak-exif"],
                         "ooxml": ["zip", "quick-xml"], "calamine": ["calamine", "zip", "quick-xml"],
                         "image-ocrs": ["image", "kamadak-exif", "ocrs", "rten"],
                         "image-ocrs-filtered": ["image", "kamadak-exif", "ocrs", "rten"],
                         "image-tesseract-sparse": ["image", "kamadak-exif"],
                         "image-tesseract": ["image", "kamadak-exif"]}[candidate]
                trial["components"] = [p for p in provenance["dependencies"] if p["name"] in names]
                trial["configuration"] = {"pdf_render_dpi": 300 if candidate.startswith("pdfium-") else None,
                                          "tesseract_psm": (11 if candidate.endswith("-sparse") else 3) if "tesseract" in candidate else None,
                                          "drop_single_character_lines": candidate.endswith("-filtered"),
                                          "tesseract_language": "eng" if "tesseract" in candidate else None,
                                          "models_reference": "provenance.assets", "threads_per_engine": 2}
                trials.append(trial)
                print(case["id"], candidate, repetition, trial["outcome"], round(trial["wall_ms"]), flush=True)
                (args.destination / "measurements.json").write_text(json.dumps(output, indent=2) + "\n")
    output["status"] = "complete"
    (args.destination / "measurements.json").write_text(json.dumps(output, indent=2) + "\n")


if __name__ == "__main__":
    main()
