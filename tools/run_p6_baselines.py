#!/usr/bin/env python3
"""Run isolated P6 processes, preserving raw timings and OS peak RSS separately."""
from __future__ import annotations
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import signal
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]
SCENARIOS = ["long-1k", "long-10k", "code-128k", "table-20x20", "table-50x20", "multi-4x1k", "multi-4x10k"]


def bounded_process(command: list[str], timeout: int) -> subprocess.CompletedProcess:
    """Terminate the whole measurement tree, including /usr/bin/time's child."""
    options = {"start_new_session": True} if os.name != "nt" else {"creationflags": subprocess.CREATE_NEW_PROCESS_GROUP}
    with subprocess.Popen(command, cwd=ROOT, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, **options) as process:
        try:
            stdout, stderr = process.communicate(timeout=timeout)
        except subprocess.TimeoutExpired as error:
            if os.name == "nt":
                subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"], capture_output=True, check=False)
            else:
                os.killpg(process.pid, signal.SIGKILL)
            stdout, stderr = process.communicate()
            raise subprocess.TimeoutExpired(command, timeout, output=stdout, stderr=stderr) from error
        return subprocess.CompletedProcess(command, process.returncode, stdout, stderr)


def peak_rss(stderr: str, system: str) -> int | None:
    """Normalize documented time(1) units; absent measurements stay absent."""
    if system == "Darwin":
        match = re.search(r"(\d+)\s+maximum resident set size", stderr)
        return int(match[1]) if match else None
    if system == "Linux":
        match = re.search(r"Maximum resident set size \(kbytes\):\s*(\d+)", stderr)
        return int(match[1]) * 1024 if match else None
    return None


def run(args: argparse.Namespace) -> None:
    output = Path(args.output).resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    source = subprocess.check_output(["git", "ls-files", "--cached", "--others", "--exclude-standard"], cwd=ROOT, text=True)
    fingerprint = hashlib.sha256()
    for name in sorted(set(source.splitlines())):
        p = ROOT / name
        if p.suffix in {".rs", ".toml", ".py"} or p.name == "Cargo.lock":
            fingerprint.update(name.encode() + b"\0" + p.read_bytes())
    record = {
        "schema": 1, "kind": args.kind, "utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "base_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "source_sha256": fingerprint.hexdigest(),
        "working_tree": subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True).splitlines(),
        "binary_sha256": hashlib.sha256(Path(args.binary).read_bytes()).hexdigest(),
        "platform": platform.platform(), "machine": platform.machine(),
        "rustc": subprocess.check_output(["rustc", "-Vv"], cwd=ROOT, text=True),
        "profile": "release", "sample_count": args.samples, "history_edits": args.history if args.kind == "runtime" else None,
        "repetitions": args.repeat, "runs": [],
    }
    if platform.system() == "Darwin":
        record["hardware"] = subprocess.check_output(["sysctl", "-n", "machdep.cpu.brand_string", "hw.memsize"], text=True).splitlines()
    for scenario in args.scenarios:
        for repetition in range(args.repeat):
            command = [str(Path(args.binary).resolve()), scenario, str(args.samples), str(args.history)]
            if platform.system() == "Darwin":
                command = ["/usr/bin/time", "-l"] + command
            elif platform.system() == "Linux" and Path("/usr/bin/time").exists():
                command = ["/usr/bin/time", "-v"] + command
            start = time.monotonic()
            try:
                result = bounded_process(command, args.timeout)
                peak = peak_rss(result.stderr, platform.system())
                lines = [line for line in result.stdout.splitlines() if line.startswith('{')]
                value = json.loads(lines[-1]) if result.returncode == 0 and lines else None
                entry = {"scenario": scenario, "repetition": repetition, "exit_code": result.returncode,
                         "wall_seconds": time.monotonic() - start, "peak_rss_bytes": peak,
                         "result": value, "stderr": result.stderr}
            except (subprocess.TimeoutExpired, json.JSONDecodeError) as error:
                entry = {"scenario": scenario, "repetition": repetition, "timeout_seconds": args.timeout,
                         "result": None, "stderr": str(error), "partial_stdout": getattr(error, "stdout", None), "partial_stderr": getattr(error, "stderr", None)}
            record["runs"].append(entry)
            output.write_text(json.dumps(record, ensure_ascii=False, indent=2) + "\n")
            print(f"{scenario} repeat={repetition + 1}: {'ok' if entry['result'] else 'FAILED'}", flush=True)
            if entry["result"] is None:
                raise SystemExit(f"Measurement failed; partial evidence saved to {output}")
    print(output, flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--kind", choices=["runtime", "native"], default="runtime")
    parser.add_argument("--scenarios", nargs="+", choices=SCENARIOS, default=SCENARIOS)
    parser.add_argument("--samples", type=int, default=30)
    parser.add_argument("--history", type=int, default=256)
    parser.add_argument("--repeat", type=int, default=3)
    parser.add_argument("--timeout", type=int, default=300)
    options = parser.parse_args()
    if not (1 <= options.samples <= 1000 and 1 <= options.history <= 4096 and 1 <= options.repeat <= 20 and 1 <= options.timeout <= 3600):
        parser.error("samples/history/repeat out of bounds")
    run(options)
