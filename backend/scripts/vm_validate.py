#!/usr/bin/env python3
"""Run practical qfs backend scenarios across three existing Lima guests."""

from __future__ import annotations

import argparse
import json
import os
import queue
import re
import shlex
import subprocess
import threading
import time
from datetime import datetime, timezone
from pathlib import Path


DEFAULT_HOSTS = {"host": "192.168.64.2", "a": "192.168.64.3", "b": "192.168.64.4"}
DEFAULT_BIN = "/tmp/qfs-bin"


class Remote:
    def __init__(self, name: str, lima_home: Path):
        self.name = name
        self.config = lima_home / f"qfs-{name}" / "ssh.config"
        self.target = f"lima-qfs-{name}"

    def argv(self, command: str) -> list[str]:
        return ["ssh", "-F", str(self.config), self.target, "--", command]

    def run(self, command: str, timeout: float = 30, check: bool = True):
        result = subprocess.run(
            self.argv(command), text=True, capture_output=True, timeout=timeout
        )
        if check and result.returncode:
            raise RuntimeError(
                f"{self.name}: command failed ({result.returncode}): {command}\n"
                f"stdout={result.stdout!r}\nstderr={result.stderr!r}"
            )
        return result


class Process:
    def __init__(self, remote: Remote, label: str, command: str, log_dir: Path, pidfile: str):
        self.remote = remote
        self.label = label
        self.pidfile = pidfile
        wrapped = f"echo $$ > {shlex.quote(pidfile)}; exec {command}"
        self.proc = subprocess.Popen(
            remote.argv(wrapped), stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, text=True, bufsize=1,
        )
        self.outq: queue.Queue[str] = queue.Queue()
        self.lines: list[str] = []
        log_dir.mkdir(parents=True, exist_ok=True)
        self._threads = [
            threading.Thread(target=self._pump, args=(self.proc.stdout, log_dir / f"{label}.stdout.log", True), daemon=True),
            threading.Thread(target=self._pump, args=(self.proc.stderr, log_dir / f"{label}.stderr.log", False), daemon=True),
        ]
        for thread in self._threads:
            thread.start()

    def _pump(self, stream, path: Path, parse: bool):
        with path.open("a", encoding="utf-8") as log:
            for line in stream:
                log.write(line)
                log.flush()
                self.lines.append(line.rstrip())
                if parse:
                    self.outq.put(line.rstrip())

    def send(self, command: str, timeout: float = 30) -> dict:
        if self.proc.poll() is not None:
            raise RuntimeError(f"{self.label} exited with {self.proc.returncode}")
        assert self.proc.stdin is not None
        self.proc.stdin.write(command + "\n")
        self.proc.stdin.flush()
        deadline = time.monotonic() + timeout
        op = command.split()[0]
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError(f"{self.label}: timeout waiting for {op}")
            line = self.outq.get(timeout=remaining)
            try:
                value = json.loads(line)
            except json.JSONDecodeError:
                continue
            if value.get("op") == op:
                if not value.get("ok"):
                    raise RuntimeError(f"{self.label} {op}: {value.get('error')}")
                return value

    def ready(self, timeout: float = 30) -> dict:
        deadline = time.monotonic() + timeout
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError(f"{self.label}: ready timeout; output={self.lines[-20:]}")
            if self.proc.poll() is not None:
                raise RuntimeError(f"{self.label} exited before ready: {self.lines[-20:]}")
            try:
                line = self.outq.get(timeout=min(remaining, 0.2))
                value = json.loads(line)
            except queue.Empty:
                continue
            except json.JSONDecodeError:
                continue
            if value.get("op") == "ready":
                if not value.get("ok"):
                    raise RuntimeError(f"{self.label}: {value.get('error')}")
                return value

    def wait_log(self, pattern: str, timeout: float = 30) -> re.Match:
        compiled = re.compile(pattern)
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            for line in list(self.lines):
                found = compiled.search(line)
                if found:
                    return found
            if self.proc.poll() is not None:
                raise RuntimeError(f"{self.label} exited: {self.lines[-20:]}")
            time.sleep(0.05)
        raise TimeoutError(f"{self.label}: log timeout for {pattern!r}: {self.lines[-20:]}")

    def stop(self, graceful: bool = True, kill: bool = False):
        if graceful and self.proc.poll() is None and self.proc.stdin:
            try:
                self.proc.stdin.write("quit\n")
                self.proc.stdin.flush()
                self.proc.wait(timeout=5)
            except (BrokenPipeError, subprocess.TimeoutExpired):
                pass
        if self.proc.poll() is None:
            signal = "KILL" if kill else "TERM"
            self.remote.run(
                f"if test -f {shlex.quote(self.pidfile)}; then kill -{signal} $(cat {shlex.quote(self.pidfile)}) 2>/dev/null || true; fi",
                check=False,
            )
            try:
                self.proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.proc.terminate()
        self.remote.run(f"rm -f {shlex.quote(self.pidfile)}", check=False)


class Runner:
    def __init__(self, args):
        self.run_id = args.run_id
        self.hosts = {"host": args.host_ip, "a": args.a_ip, "b": args.b_ip}
        self.bin = args.bin_dir.rstrip("/")
        self.output = args.output.resolve()
        self.log_dir = self.output.parent / f"{self.output.stem}-logs"
        self.remotes = {name: Remote(name, args.lima_home) for name in self.hosts}
        self.base = f"{args.guest_root.rstrip('/')}/qfs-vm-{self.run_id}"
        self.processes: list[Process] = []
        self.results: list[dict] = []
        self.started = datetime.now(timezone.utc).isoformat()

    def record(self, name: str, fn):
        start = time.monotonic()
        try:
            detail = fn() or {}
            status = "passed"
            error = None
        except Exception as exc:  # scenario failures must not abort later scenarios
            status = "failed"
            detail, error = {}, str(exc)
        self.results.append({
            "name": name, "status": status,
            "duration_ms": round((time.monotonic() - start) * 1000),
            "detail": detail, "error": error,
        })

    def skip(self, name: str, reason: str):
        self.results.append({"name": name, "status": "skipped", "reason": reason})

    def start(self, vm: str, label: str, command: str) -> Process:
        pidfile = f"{self.base}/pids/{label}.pid"
        process = Process(self.remotes[vm], label, command, self.log_dir, pidfile)
        self.processes.append(process)
        return process

    def stop(self, process: Process, graceful: bool = True, kill: bool = False):
        process.stop(graceful, kill)
        if process in self.processes:
            self.processes.remove(process)

    def prepare(self):
        for remote in self.remotes.values():
            remote.run(f"mkdir -p {self.base}/pids {self.base}/data")
            result = remote.run(f"test -x {self.bin}/qfsd && test -x {self.bin}/vm_probe", check=False)
            if result.returncode:
                raise RuntimeError(f"{remote.name}: binaries missing or not executable")

    def daemon(self, vm: str, label: str, args: str) -> Process:
        return self.start(vm, label, f"{self.bin}/qfsd {args}")

    def probe(self, vm: str, label: str, data: str, code: str) -> Process:
        p = self.start(vm, label,
            f"{self.bin}/vm_probe member --data-dir {data} --directory-addr {self.hosts['host']}:7440 --join-code {code}")
        p.ready()
        return p

    def inspect(self, vm: str, data: str, root: str, path: str, wait_ms=0) -> dict:
        cmd = " ".join(map(shlex.quote, [f"{self.bin}/vm_probe", "inspect", "--data-dir", data,
            "--root-id", root, "--path", path, "--wait-ms", str(wait_ms)]))
        result = self.remotes[vm].run(cmd, timeout=max(30, wait_ms / 1000 + 5))
        lines = [json.loads(line) for line in result.stdout.splitlines() if line.startswith("{")]
        if not lines or not lines[-1].get("ok"):
            raise RuntimeError(f"inspect failed: {result.stdout} {result.stderr}")
        return lines[-1]

    def actual_daemon_scenarios(self):
        root = f"{self.base}/data/main"
        directory = self.daemon("host", "directory", f"--directory --data-dir {root}/directory --listen-addr 0.0.0.0:7440")
        directory.wait_log(r"directory listening")
        host = self.daemon("host", "host-main",
            f"--create-vault --data-dir {root}/host --listen-addr 0.0.0.0:7447 --advertise-addr {self.hosts['host']}:7447 --directory-addr {self.hosts['host']}:7440")
        code = host.wait_log(r"join code ([A-Z2-7]{26})").group(1)
        host.wait_log(r"vault listening")
        a = self.probe("a", "member-a", f"{root}/a", code)
        b = self.probe("b", "member-b", f"{root}/b", code)
        aid, bid = a.send("id"), b.send("id")
        root_id = bid["root_id"]
        self.record("fresh_members_join", lambda: {"code": code, "a": aid, "b": bid})

        observer_data = f"{root}/daemon-observer"
        observer = self.daemon("b", "daemon-observer",
            f"--data-dir {observer_data} --listen-addr 0.0.0.0:7448 --directory-addr {self.hosts['host']}:7440 --join-code {code}")
        observer.wait_log(r"joined vault")

        def transfer():
            a.send("mkdir /vm")
            saved = a.send("save /vm/blob 3 400000 101")
            b.send("wait /vm/blob present 2000")
            before = b.send("status /vm/blob")
            if before["local_chunks"] != 0:
                raise AssertionError(f"online body unexpectedly automatic: {before}")
            verified = b.send("pull_verify /vm/blob 3 400000 101")
            return {"saved": saved, "before_pull": before, "verified": verified}
        self.record("tcp_save_controls_then_pull", transfer)

        def cadence():
            samples = []
            inspections = []
            for index in range(5):
                path = f"/vm/cadence-{self.run_id}-{index}"
                start = time.monotonic()
                a.send(f"save {path} 1 32 {202 + index}")
                value = self.inspect("b", observer_data, root_id, path, 2000)
                elapsed = round((time.monotonic() - start) * 1000)
                if not value.get("name_present"):
                    raise AssertionError("observer did not persist control")
                samples.append(elapsed)
                inspections.append(value)
            ordered = sorted(samples)
            return {"wall_ms_samples": samples, "wall_ms_min": ordered[0],
                "wall_ms_median": ordered[len(ordered) // 2], "wall_ms_max": ordered[-1],
                "measurement": "writer command + SSH + inspect polling overhead",
                "local_chunks": [value["local_chunks"] for value in inspections]}
        self.record("observer_100ms_cadence", cadence)
        self.stop(observer)

        def mutations():
            a.send("rename /vm/blob /vm/renamed")
            a.send("save /vm/renamed 2 300000 303")
            b.send("wait /vm/renamed present 2000")
            b.send("pull_verify /vm/renamed 2 300000 303")
            a.send("unlink /vm/renamed")
            b.send("wait /vm/renamed absent 2000")
            return {}
        self.record("rename_overwrite_unlink", mutations)

        def eviction():
            a.send("save /vm/evict 3 100000 404")
            removed = a.send("evict /vm/evict")
            empty = a.send("status /vm/evict")
            restored = a.send("pull_verify /vm/evict 3 100000 404")
            if empty["local_chunks"] != 0:
                raise AssertionError(f"eviction retained chunks: {empty}")
            return {"removed": removed, "empty": empty, "restored": restored}
        self.record("evict_and_restore", eviction)

        self.stop(b)
        self.record("immediate_durable_member_rejoin", lambda: self._rejoin_once("b", "member-b-rejoin", f"{root}/b", code))

        for n in range(3):
            self.record(f"offline_save_{n}", lambda n=n: a.send(f"save /vm/offline 1 4096 {500+n}"))
        self.stop(host, graceful=False, kill=True)
        host = self.daemon("host", "host-main-restart",
            f"--data-dir {root}/host --listen-addr 0.0.0.0:7447 --advertise-addr {self.hosts['host']}:7447 --directory-addr {self.hosts['host']}:7440")
        host.wait_log(r"vault listening")
        self.stop(a, graceful=False)
        holders = {}
        for vm in ("a", "b"):
            try:
                holders[vm] = self.probe(vm, f"member-{vm}-post-host", f"{root}/{vm}", code)
                self.record(f"{vm}_rejoin_after_host_restart", lambda vm=vm: holders[vm].send("session"))
            except Exception as exc:
                self.results.append({"name": f"{vm}_rejoin_after_host_restart", "status": "failed", "error": str(exc)})
        if "b" in holders:
            self.record("offline_mailbox_latest_bodies", lambda: holders["b"].send("verify_local /vm/offline 1 4096 502"))
        else:
            self.skip("offline_mailbox_latest_bodies", "B could not rejoin")

        cdata = f"{root}/c"
        self.record("new_member_bootstrap_preexisting_file",
            lambda: self._fresh_bootstrap(cdata, code, "/vm/offline", 1, 4096, 502))

        for p in list(holders.values()):
            self.stop(p, graceful=False)
        self.stop(host)
        ph = self.start("host", "probe-host",
            f"{self.bin}/vm_probe host --data-dir {root}/host --listen-addr 0.0.0.0:7447 --advertise-addr {self.hosts['host']}:7447 --directory-addr {self.hosts['host']}:7440")
        ph.ready()
        joined = {}
        for vm in ("a", "b"):
            try:
                joined[vm] = self.probe(vm, f"kick-{vm}", f"{root}/{vm}", code)
            except Exception as exc:
                self.results.append({"name": f"kick_setup_{vm}", "status": "failed", "error": str(exc)})
        kick_state = {}
        if "b" in joined:
            def kick_flow():
                written = joined["b"].send("save /vm/historical-b 1 2048 707")
                peer = joined["b"].send("id")["peer_id"]
                kicked = ph.send(f"kick {peer}")
                newcode = ph.send("code")["join_code"]
                disconnected = False
                try:
                    joined["b"].send("heartbeat", timeout=5)
                except Exception:
                    disconnected = True
                if not disconnected:
                    raise AssertionError("kicked member connection remained usable")
                remaining = joined.get("a")
                if remaining:
                    deadline = time.monotonic() + 2
                    while True:
                        remaining.send("heartbeat")
                        members = remaining.send("members")
                        if peer not in members["peer_ids"]:
                            break
                        if time.monotonic() >= deadline:
                            raise AssertionError(f"A still lists kicked peer: {members}")
                self.stop(joined.pop("b"), graceful=False)
                denied = {}
                for label, candidate in (("old", code), ("new", newcode)):
                    try:
                        bad = self.probe("b", f"kick-denied-{label}", f"{root}/b", candidate)
                        self.stop(bad)
                        denied[label] = False
                    except Exception:
                        denied[label] = True
                if not all(denied.values()):
                    raise AssertionError(f"kicked identity rejoined: {denied}")
                kick_state["new_code"] = newcode
                return {"written": written, "kicked": kicked, "new_code": newcode,
                    "disconnected": disconnected, "denied": denied}
            self.record("host_kick_member", kick_flow)
        else:
            self.skip("host_kick_member", "B could not rejoin due restart defect")
        for p in list(joined.values()):
            self.stop(p, graceful=False)
        self.stop(ph)
        if "new_code" in kick_state:
            restarted = self.daemon("host", "host-after-kick",
                f"--data-dir {root}/host --listen-addr 0.0.0.0:7447 --advertise-addr {self.hosts['host']}:7447 --directory-addr {self.hosts['host']}:7440")
            restarted.wait_log(r"vault listening")
            try:
                after = self.probe("a", "a-after-kick", f"{root}/a", kick_state["new_code"])
                self.record("host_restart_preserves_historical_writer_file",
                    lambda: after.send("pull_verify /vm/historical-b 1 2048 707"))
                self.stop(after)
            except Exception as exc:
                self.results.append({"name": "host_restart_preserves_historical_writer_file",
                    "status": "failed", "error": str(exc)})
            self.stop(restarted)
        else:
            self.skip("host_restart_preserves_historical_writer_file", "kick scenario unavailable")
        self.stop(directory)

    def _rejoin_once(self, vm, label, data, code):
        p = self.probe(vm, label, data, code)
        value = p.send("session")
        self.stop(p)
        return value

    def _fresh_bootstrap(self, data, code, path, chunks, size, seed):
        p = self.probe("b", "fresh-c", data, code)
        try:
            return p.send(f"verify_local {path} {chunks} {size} {seed}")
        finally:
            self.stop(p)

    def multi_vault(self):
        root = f"{self.base}/data/multi"
        directory = self.daemon("host", "multi-directory", f"--directory --data-dir {root}/directory --listen-addr 0.0.0.0:7440")
        directory.wait_log(r"directory listening")
        def start_host(create, label):
            flag = "--create-vault" if create else ""
            p = self.daemon("host", label,
                f"{flag} --data-dir {root}/host --listen-addr 0.0.0.0:7447 --advertise-addr {self.hosts['host']}:7447 --directory-addr {self.hosts['host']}:7440")
            code = p.wait_log(r"join code ([A-Z2-7]{26})").group(1) if create else None
            p.wait_log(r"vault listening")
            return p, code
        host, code1 = start_host(True, "multi-host-v1")
        a = self.probe("a", "multi-a", f"{root}/a", code1)
        a.send("save /same 1 1024 801")
        a_state = a.send("id")
        a.send("verify_local /same 1 1024 801")
        self.stop(a); self.stop(host)
        host, code2 = start_host(True, "multi-host-v2")
        b = self.probe("b", "multi-b", f"{root}/b", code2)
        b.send("save /same 1 1024 802")
        b_state = b.send("id")
        b.send("verify_local /same 1 1024 802")
        def distinct():
            if code1 == code2 or a_state["root_id"] == b_state["root_id"]:
                raise AssertionError("vault codes or roots were not distinct")
            return {"code1": code1, "code2": code2,
                "root1": a_state["root_id"], "root2": b_state["root_id"]}
        self.record("multi_vault_distinct_codes_payloads", distinct)
        self.stop(b); self.stop(host)
        host, _ = start_host(False, "multi-host-reload")
        self.record("multi_vault_code1_reload", lambda: self._rejoin_verify("a", "multi-a-reload", f"{root}/a", code1, 801))
        self.record("multi_vault_code2_reload", lambda: self._rejoin_verify("b", "multi-b-reload", f"{root}/b", code2, 802))
        self.stop(host); self.stop(directory)

    def _rejoin_verify(self, vm, label, data, code, seed):
        p = self.probe(vm, label, data, code)
        try:
            session = p.send("session")
            verified = p.send(f"verify_local /same 1 1024 {seed}")
            return {"session": session, "verified": verified}
        finally:
            self.stop(p)

    def cleanup(self):
        for process in reversed(self.processes[:]):
            try:
                self.stop(process, graceful=False)
            except Exception:
                pass

    def write(self):
        counts = {}
        for result in self.results:
            counts[result["status"]] = counts.get(result["status"], 0) + 1
        payload = {"run_id": self.run_id, "started_at": self.started,
            "finished_at": datetime.now(timezone.utc).isoformat(), "hosts": self.hosts,
            "counts": counts, "results": self.results, "log_dir": str(self.log_dir)}
        self.output.parent.mkdir(parents=True, exist_ok=True)
        self.output.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--lima-home", type=Path, default=Path("/private/tmp/qfs-vm-validation/lima"))
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--run-id", default=datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ"))
    parser.add_argument("--host-ip", default=DEFAULT_HOSTS["host"])
    parser.add_argument("--a-ip", default=DEFAULT_HOSTS["a"])
    parser.add_argument("--b-ip", default=DEFAULT_HOSTS["b"])
    parser.add_argument("--guest-root", default="/home/justinschwartzreich.guest")
    parser.add_argument("--bin-dir", default=DEFAULT_BIN)
    args = parser.parse_args()
    if not re.fullmatch(r"[A-Za-z0-9_.-]+", args.run_id):
        parser.error("--run-id may contain only letters, digits, dot, underscore and hyphen")
    if not args.guest_root.startswith("/") or not re.fullmatch(r"[A-Za-z0-9_./-]+", args.guest_root):
        parser.error("--guest-root must be a simple absolute path")
    if not args.bin_dir.startswith("/") or not re.fullmatch(r"[A-Za-z0-9_./-]+", args.bin_dir):
        parser.error("--bin-dir must be a simple absolute path")
    return args


def main():
    runner = Runner(parse_args())
    try:
        runner.prepare()
        for name, scenario in (("main_phase", runner.actual_daemon_scenarios),
                               ("multi_vault_phase", runner.multi_vault)):
            try:
                scenario()
            except Exception as exc:
                runner.results.append({"name": name, "status": "failed", "error": str(exc)})
                runner.cleanup()
    except Exception as exc:
        runner.results.append({"name": "orchestration", "status": "failed", "error": str(exc)})
    finally:
        runner.cleanup()
        runner.write()
    return 1 if any(r["status"] == "failed" for r in runner.results) else 0


if __name__ == "__main__":
    raise SystemExit(main())
