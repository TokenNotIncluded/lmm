#!/usr/bin/env python3
"""Schedule synthetic file and ancestor swaps before the real CLI's openat calls."""

import argparse
import json
from pathlib import Path
import re
import subprocess
import tempfile
import time


def sandbox(case, evidence, binary, injection, trace_name):
    trace_args = ["/usr/bin/strace", "-f", "-qq", "-yy", "-s", "512",
                  "-e", "trace=openat,read", "-o", "/trace/" + trace_name]
    if injection is not None:
        trace_args += ["-e", f"inject=openat:delay_enter=1000ms:when={injection}"]
    target = ["/lmm", "status", "pi", "--instance", "/case/project",
              "--pi-package", "/case/sdk", "--agent-dir", "/case/agent", "--json"]
    return ["/usr/bin/bwrap", "--unshare-user", "--unshare-pid", "--unshare-net",
            "--unshare-ipc", "--unshare-uts", "--die-with-parent", "--new-session",
            "--cap-drop", "ALL", "--ro-bind", "/usr", "/usr",
            "--symlink", "usr/bin", "/bin", "--symlink", "usr/bin", "/sbin",
            "--symlink", "usr/lib", "/lib", "--symlink", "usr/lib", "/lib64",
            "--proc", "/proc", "--dev", "/dev", "--dir", "/etc", "--dir", "/tmp",
            "--bind", str(case), "/case", "--bind", str(evidence), "/trace",
            "--ro-bind", str(binary), "/lmm", "--chdir", "/case", "--clearenv",
            "--setenv", "HOME", "/case/home", "--setenv", "PATH", "",
            *trace_args, *target]


def open_calls(trace):
    return [line for line in trace.splitlines() if re.search(r"\bopenat\(", line)]


def prepare(case):
    for name in ["home", "agent", "project/.pi", "sdk", "private"]:
        (case / name).mkdir(parents=True, exist_ok=True)
    (case / "sdk/package.json").write_text(json.dumps({
        "name": "@earendil-works/pi-coding-agent", "version": "1.2.0"}))
    (case / "agent/settings.json").write_text('{"defaultProvider":"lmm"}')
    (case / "project/.pi/settings.json").write_text('{}')
    (case / "private/auth.json").write_text('{"defaultProvider":"other","secret":"SYNTHETIC_PRIVATE"}')
    (case / "private/settings.json").write_text('{"defaultProvider":"other","secret":"SYNTHETIC_PRIVATE"}')


def run(binary, evidence):
    results = []
    with tempfile.TemporaryDirectory(prefix="lmm-pi-race-") as temporary:
        case = Path(temporary) / "case"
        case.mkdir()
        prepare(case)
        baseline_cmd = sandbox(case, evidence, binary, None, "race-baseline.trace")
        initial = subprocess.run(baseline_cmd, capture_output=True, text=True, timeout=20)
        (evidence / "race-baseline.stdout").write_text(initial.stdout)
        (evidence / "race-baseline.stderr").write_text(initial.stderr)
        assert initial.returncode == 0, f"baseline failed: {initial.stderr}"
        calls = open_calls((evidence / "race-baseline.trace").read_text())
        file_index = next(i for i, line in enumerate(calls, 1)
                          if '</case/agent>, "settings.json"' in line)
        parent_index = max(i for i, line in enumerate(calls[:file_index - 1], 1)
                           if '</case>, "agent"' in line)
        assert "O_NOFOLLOW" in calls[file_index - 1], calls[file_index - 1]
        assert "O_NOFOLLOW" in calls[parent_index - 1], calls[parent_index - 1]
        assert "O_DIRECTORY" in calls[parent_index - 1], calls[parent_index - 1]
        for name, index in [("file", file_index), ("ancestor", parent_index)]:
            trace = evidence / f"race-{name}.trace"
            cmd = sandbox(case, evidence, binary, index, trace.name)
            (evidence / f"race-{name}.command.json").write_text(json.dumps(cmd, indent=2) + "\n")
            process = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            deadline = time.monotonic() + 12
            swapped = False
            while time.monotonic() < deadline and process.poll() is None:
                observed = open_calls(trace.read_text()) if trace.exists() else []
                if len(observed) >= index - 1:
                    if name == "file":
                        settings = case / "agent/settings.json"
                        settings.rename(case / "agent/settings-before-swap.json")
                        settings.symlink_to("../private/auth.json")
                    else:
                        (case / "agent").rename(case / "agent-before-swap")
                        (case / "agent").symlink_to("private", target_is_directory=True)
                    swapped = True
                    break
                time.sleep(0.002)
            try:
                stdout, stderr = process.communicate(timeout=12)
            except subprocess.TimeoutExpired:
                process.kill()
                stdout, stderr = process.communicate()
            (evidence / f"race-{name}.stdout").write_text(stdout)
            (evidence / f"race-{name}.stderr").write_text(stderr)
            trace_text = trace.read_text() if trace.exists() else ""
            report = json.loads(stdout) if stdout else {}
            forbidden_reads = [line for line in trace_text.splitlines()
                               if re.search(r"\bread\(\d+</case/private/(auth|settings)\.json>", line)]
            result = {"case": name, "delayed_openat_index": index,
                      "swap_completed": swapped, "exit": process.returncode,
                      "configuration": report.get("targets", [{}])[0].get("verification", {}).get("configuration"),
                      "private_file_reads": forbidden_reads,
                      "secret_in_output": "SYNTHETIC_PRIVATE" in stdout + stderr}
            result["passed"] = (swapped and process.returncode == 3
                                and result["configuration"] == "unknown"
                                and not forbidden_reads and not result["secret_in_output"])
            results.append(result)
            (evidence / f"race-{name}.result.json").write_text(json.dumps(result, indent=2) + "\n")
            if swapped:
                if name == "file":
                    (case / "agent/settings.json").unlink()
                    (case / "agent/settings-before-swap.json").rename(case / "agent/settings.json")
                else:
                    (case / "agent").unlink()
                    (case / "agent-before-swap").rename(case / "agent")
    print(json.dumps(results, indent=2))
    return all(result["passed"] for result in results)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    args = parser.parse_args()
    if not args.binary.is_absolute() or not args.evidence.is_absolute() or not args.evidence.is_dir():
        parser.error("absolute binary path and existing evidence directory required")
    raise SystemExit(0 if run(args.binary, args.evidence) else 1)
