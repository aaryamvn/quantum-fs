#!/usr/bin/env python3
"""Run the destructive, disposable-VM network validation scenario."""

import argparse
import ipaddress
import json
import queue
import re
import shlex
import subprocess
import sys
import threading
import time
from pathlib import Path, PurePosixPath


SAFE_NAME = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_.-]{0,63}$")
SAFE_INTERFACE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_.:-]{0,31}$")
NETEM_HANDLE = "1a1a:"


def arguments():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--lima-home", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--host-ip", default="192.168.104.1")
    parser.add_argument(
        "--guest-root", default="/home/justinschwartzreich.guest/qfs-netfaults"
    )
    parser.add_argument(
        "--bin-dir", default="/home/justinschwartzreich.guest/qfs-tools"
    )
    parser.add_argument("--run-id", default=str(int(time.time())))
    parser.add_argument("--interface", default="eth0")
    args = parser.parse_args()
    args.host_ip = str(ipaddress.ip_address(args.host_ip))
    if not SAFE_NAME.fullmatch(args.run_id):
        parser.error("--run-id must be a safe 1-64 character identifier")
    if not SAFE_INTERFACE.fullmatch(args.interface):
        parser.error("--interface is not a safe interface name")
    root = PurePosixPath(args.guest_root)
    if not root.is_absolute() or ".." in root.parts or str(root) in ("/", "/tmp"):
        parser.error("--guest-root must be a specific absolute path without '..'")
    args.remote_root = str(root) + "-" + args.run_id
    bin_dir = PurePosixPath(args.bin_dir)
    if not bin_dir.is_absolute() or ".." in bin_dir.parts or str(bin_dir) == "/":
        parser.error("--bin-dir must be a specific absolute path without '..'")
    args.qfsd = str(bin_dir / "qfsd")
    args.vm_probe = str(bin_dir / "vm_probe")
    for vm in ("qfs-host", "qfs-a"):
        if not (args.lima_home / vm / "ssh.config").is_file():
            parser.error(f"missing Lima SSH config for {vm}")
    return args


class Scenario:
    def __init__(self, args):
        self.args = args
        self.results = []
        self.processes = []
        self.netem_baseline = None
        self.netem_installed = False
        self.drop_installed = False
        self.logs = args.output.parent / (args.output.stem + "-logs")
        self.logs.mkdir(parents=True, exist_ok=True)

    def ssh(self, vm, command, *, check=True, timeout=None):
        completed = subprocess.run(
            [
                "ssh",
                "-F",
                str(self.args.lima_home / vm / "ssh.config"),
                "lima-" + vm,
                command,
            ],
            text=True,
            capture_output=True,
            check=False,
            timeout=timeout,
        )
        if check and completed.returncode:
            raise RuntimeError(
                f"ssh {vm} failed ({completed.returncode}): "
                f"stdout={completed.stdout[-1000:]!r} stderr={completed.stderr[-1000:]!r}"
            )
        return completed.stdout.strip()

    def record(self, name, passed, **details):
        row = {"name": name, "passed": bool(passed), **details}
        self.results.append(row)
        print(json.dumps(row, ensure_ascii=False), flush=True)

    def iptables_args(self):
        return [
            "-p", "tcp", "-d", self.args.host_ip, "--dport", "18447",
            "-m", "comment", "--comment", "qfs-vm-" + self.args.run_id,
            "-j", "DROP",
        ]

    def install_netem(self):
        shown = self.ssh(
            "qfs-a", f"sudo tc qdisc show dev {shlex.quote(self.args.interface)}"
        )
        kinds = re.findall(r"^qdisc\s+(\S+).*\sroot(?:\s|$)", shown, flags=re.MULTILINE)
        if len(kinds) != 1 or kinds[0] not in ("fq_codel", "noqueue"):
            raise RuntimeError(f"refusing to replace custom qdisc: {shown!r}")
        self.netem_baseline = kinds[0]
        command = shlex.join(
            ["sudo", "tc", "qdisc", "replace", "dev", self.args.interface,
             "root", "handle", NETEM_HANDLE, "netem", "delay", "50ms", "5ms",
             "loss", "1%"]
        )
        self.ssh("qfs-a", command)
        self.netem_installed = True

    def remove_netem(self):
        if not self.netem_installed:
            return
        shown = self.ssh(
            "qfs-a", f"sudo tc qdisc show dev {shlex.quote(self.args.interface)}"
        )
        if f"qdisc netem {NETEM_HANDLE}" not in shown:
            raise RuntimeError(f"installed netem qdisc was replaced externally: {shown!r}")
        if self.netem_baseline == "fq_codel":
            command = shlex.join(
                ["sudo", "tc", "qdisc", "replace", "dev", self.args.interface,
                 "root", "fq_codel"]
            )
        else:
            command = shlex.join(
                ["sudo", "tc", "qdisc", "del", "dev", self.args.interface, "root"]
            )
        self.ssh("qfs-a", command)
        self.netem_installed = False

    def install_drop(self):
        rule = self.iptables_args()
        self.ssh("qfs-a", shlex.join(["sudo", "iptables", "-I", "OUTPUT", *rule]))
        self.drop_installed = True

    def remove_drop(self):
        if not self.drop_installed:
            return
        rule = self.iptables_args()
        check = self.ssh(
            "qfs-a", shlex.join(["sudo", "iptables", "-C", "OUTPUT", *rule]), check=False
        )
        del check
        self.ssh("qfs-a", shlex.join(["sudo", "iptables", "-D", "OUTPUT", *rule]))
        self.drop_installed = False

    def cleanup(self):
        for action, label in ((self.remove_drop, "iptables"), (self.remove_netem, "qdisc")):
            try:
                action()
            except Exception as error:  # cleanup errors must affect the overall result
                self.record("cleanup_" + label, False, error=str(error))
        for process in reversed(self.processes):
            try:
                process.stop()
            except Exception as error:
                self.record("cleanup_process_" + process.name, False, error=str(error))

    def write_results(self):
        failures = sum(not row["passed"] for row in self.results)
        summary = {"name": "overall", "passed": failures == 0, "failures": failures}
        self.results.append(summary)
        print(json.dumps(summary), flush=True)
        self.args.output.parent.mkdir(parents=True, exist_ok=True)
        self.args.output.write_text(json.dumps(self.results, indent=2) + "\n")
        return failures


class RemoteProcess:
    def __init__(self, scenario, vm, name, argv):
        self.scenario = scenario
        self.vm = vm
        self.name = name
        self.pidfile = scenario.args.remote_root + "/" + name + ".pid"
        self.messages = queue.Queue()
        self.lines = []
        scenario.ssh(vm, "mkdir -p " + shlex.quote(scenario.args.remote_root))
        remote = "echo $$ > " + shlex.quote(self.pidfile) + "; exec " + shlex.join(argv)
        self.process = subprocess.Popen(
            ["ssh", "-F", str(scenario.args.lima_home / vm / "ssh.config"),
             "lima-" + vm, remote],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )
        self.log = (scenario.logs / (name + ".log")).open("w")
        for stream in (self.process.stdout, self.process.stderr):
            threading.Thread(target=self._read, args=(stream,), daemon=True).start()
        scenario.processes.append(self)

    def _read(self, stream):
        for line in stream:
            line = line.rstrip()
            self.log.write(line + "\n")
            self.log.flush()
            self.lines.append(line)
            self.messages.put(line)

    def wait(self, pattern, timeout=30):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                line = self.messages.get(timeout=0.2)
            except queue.Empty:
                if self.process.poll() is not None:
                    raise RuntimeError(
                        f"{self.name} exited {self.process.returncode}: {self.lines[-8:]}"
                    )
                continue
            if re.search(pattern, line):
                return line
        raise RuntimeError(f"{self.name} timeout waiting for {pattern!r}: {self.lines[-8:]}")

    def command(self, command, timeout=60):
        if self.process.stdin is None:
            raise RuntimeError(f"{self.name} has no stdin")
        self.process.stdin.write(command + "\n")
        self.process.stdin.flush()
        return json.loads(self.wait(r'"op":"' + re.escape(command.split()[0]) + r'"', timeout))

    def stop(self):
        if self.process.poll() is None:
            command = (
                "test ! -f " + shlex.quote(self.pidfile) + " || kill -TERM \"$(cat "
                + shlex.quote(self.pidfile) + ")\" 2>/dev/null || true"
            )
            self.scenario.ssh(self.vm, command)
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.terminate()
                self.process.wait(timeout=5)
        self.log.close()


def run(scenario):
    args = scenario.args
    root = args.remote_root
    directory = RemoteProcess(
        scenario, "qfs-host", "directory",
        [args.qfsd, "--directory", "--data-dir", root + "/directory",
         "--listen-addr", args.host_ip + ":18440"],
    )
    directory.wait("directory listening")
    host = RemoteProcess(
        scenario, "qfs-host", "host",
        [args.qfsd, "--create-vault", "--data-dir", root + "/host",
         "--listen-addr", "0.0.0.0:18447", "--advertise-addr", args.host_ip + ":18447",
         "--directory-addr", args.host_ip + ":18440"],
    )
    code = host.wait("join code ").split()[-1]
    host.wait("vault listening")

    malformed = """import json,socket,struct,time
cases=[('oversized',bytes([1])+struct.pack('>I',1048577)),('unknown_type',bytes([1])+struct.pack('>I',1)+bytes([255])),('wrong_version',bytes([2])+struct.pack('>I',1)+bytes([1]))]
for name,header in cases:
 s=socket.create_connection((HOST,18447),timeout=5);s.settimeout(6);start=time.monotonic();s.sendall(header);n=0
 try:
  while True:
   body=s.recv(65536)
   if not body:break
   n+=len(body)
  print(json.dumps(dict(name=name,closed=True,elapsed_ms=round((time.monotonic()-start)*1000,2),received=n)))
 except ConnectionResetError:print(json.dumps(dict(name=name,closed=True,elapsed_ms=round((time.monotonic()-start)*1000,2),received=n)))
 finally:s.close()
""".replace("HOST", repr(args.host_ip))
    for line in scenario.ssh("qfs-a", "python3 -c " + shlex.quote(malformed)).splitlines():
        row = json.loads(line)
        scenario.record("cross_vm_frame_" + row.pop("name"), row.pop("closed"), **row)

    daemon = RemoteProcess(
        scenario, "qfs-a", "member",
        [args.qfsd, "--data-dir", root + "/member", "--listen-addr",
         "0.0.0.0:18448", "--directory-addr", args.host_ip + ":18440", "--join-code", code],
    )
    daemon.wait("joined vault")
    scenario.record("actual_qfsd_join_after_malformed_frames", True)

    scenario.install_netem()
    probe = RemoteProcess(
        scenario, "qfs-a", "slow-probe",
        [args.vm_probe, "member", "--data-dir", root + "/slow-member",
         "--directory-addr", args.host_ip + ":18440", "--join-code", code],
    )
    probe.wait(r'"op":"ready"')
    for command in ("mkdir /network", "save /network/blob 4 262144 91",
                    "evict /network/blob", "pull_verify /network/blob 4 262144 91"):
        start = time.monotonic()
        try:
            response = probe.command(command)
            scenario.record(
                "delay_loss_" + command.split()[0], response.get("ok", False),
                elapsed_ms=round((time.monotonic() - start) * 1000, 2), response=response,
                exact_bytes=1048576 if command.startswith(("save", "pull_verify")) else None,
            )
        except Exception as error:
            scenario.record("delay_loss_" + command.split()[0], False, error=str(error))
    probe.stop()
    scenario.remove_netem()
    scenario.record(
        "delayed_network_data_integrity",
        all(row["passed"] for row in scenario.results if row["name"].startswith("delay_loss_")),
        one_way_delay_ms=50, jitter_ms=5, loss_percent=1, exact_bytes=1048576,
    )

    scenario.install_drop()
    start = time.monotonic()
    try:
        daemon.wait("(?i)host disconnected", 38)
        scenario.record("silent_partition_detected", True, elapsed_ms=round((time.monotonic() - start) * 1000, 2))
    except Exception as error:
        scenario.record("silent_partition_detected", False, error=str(error))
    scenario.remove_drop()
    start = time.monotonic()
    try:
        daemon.wait("joined vault", 15)
        scenario.record("daemon_rejoins_after_partition", True, elapsed_ms=round((time.monotonic() - start) * 1000, 2))
    except Exception as error:
        scenario.record("daemon_rejoins_after_partition", False, error=str(error))


def main():
    scenario = Scenario(arguments())
    try:
        run(scenario)
    except Exception as error:
        scenario.record("scenario_error", False, error=str(error))
    finally:
        scenario.cleanup()
    return 1 if scenario.write_results() else 0


if __name__ == "__main__":
    sys.exit(main())
