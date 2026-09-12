#!/usr/bin/env python3
"""Merge qfsd demo event logs from local or remote nodes."""

from __future__ import annotations

import argparse
from collections import Counter, deque
import os
import queue
import re
import shlex
import signal
import subprocess
import sys
import threading
import time
import unicodedata
from dataclasses import dataclass


CONTROL = re.compile(r"[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]")
KIND = re.compile(r"\[[^]\r\n]+\]\s+\[([^]\r\n]+)\]")
PALETTE = (36, 35, 33, 32, 34, 31)
KIND_COLORS = {
    "SYSTEM": 97,
    "DISCOVERY": 34,
    "SECURE": 35,
    "PEERS": 36,
    "FILE": 32,
    "TRANSFER": 96,
    "SYNC": 33,
    "ATTENTION": 91,
}


@dataclass(frozen=True)
class Source:
    label: str
    location: str


def sanitize(value: str) -> str:
    value = CONTROL.sub("?", value.replace("\r", "").replace("\t", "    "))
    # Format controls include bidi overrides and isolates, which can spoof labels.
    return "".join("?" if unicodedata.category(char) == "Cf" else char for char in value)


def framed_blocks(lines):
    """Yield complete blank-line-delimited records, ignoring a partial head."""
    block: list[str] = []
    synchronized = False
    for raw in lines:
        line = sanitize(raw.rstrip("\n"))
        if not synchronized:
            if not line or line[:1].isspace():
                continue
            synchronized = True
        if not line:
            if block:
                yield tuple(block)
                block = []
            continue
        if line[:1].isspace() and not block:
            continue
        if not line[:1].isspace() and block:
            # Recover gracefully if a producer omitted its separator.
            yield tuple(block)
            block = []
        block.append(line)


def parse_source(value: str) -> Source:
    label, separator, location = value.partition("=")
    if not separator or not label.strip() or not location.strip():
        raise argparse.ArgumentTypeError("source must be LABEL=PATH or LABEL=USER@HOST:/absolute/path")
    if any(c in label for c in "\r\n\x1b"):
        raise argparse.ArgumentTypeError("label contains an unsafe character")
    if ":" in location:
        host, path = location.split(":", 1)
        if (not host or host.startswith("-") or any(c.isspace() or unicodedata.category(c)[0] == "C" for c in host)
                or not path.startswith("/")):
            raise argparse.ArgumentTypeError("remote source must use USER@HOST:/absolute/path")
    return Source(label.strip(), location.strip())


def tail_command(source: Source) -> list[str]:
    if ":" not in source.location:
        return ["tail", "-n", "80", "-F", "--", source.location]
    host, path = source.location.split(":", 1)
    return ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "--", host,
            "tail -n 80 -F -- " + shlex.quote(path)]


class Monitor:
    def __init__(self, sources: list[Source], output=sys.stdout, queue_size: int = 256):
        self.sources = sources
        self.output = output
        self.events: queue.Queue = queue.Queue(maxsize=queue_size)
        self.stop = threading.Event()
        self.processes: list[subprocess.Popen] = []
        self.dropped = 0
        self.lock = threading.Lock()
        self.color = os.environ.get("NO_COLOR") is None and (
            os.environ.get("FORCE_COLOR") == "1" or getattr(output, "isatty", lambda: False)()
        )

    def enqueue(self, item) -> None:
        try:
            self.events.put_nowait(item)
        except queue.Full:
            with self.lock:
                self.dropped += 1

    def reader(self, source: Source, process: subprocess.Popen) -> None:
        assert process.stdout is not None
        try:
            for block in framed_blocks(process.stdout):
                if self.stop.is_set():
                    break
                self.enqueue(("block", source, block))
        finally:
            if not self.stop.is_set():
                self.enqueue(("failure", source, process.wait(), ""))

    def stderr_reader(self, source: Source, process: subprocess.Popen) -> None:
        assert process.stderr is not None
        for raw in process.stderr:
            if self.stop.is_set():
                break
            detail = sanitize(raw.rstrip("\n"))
            if detail:
                self.enqueue(("warning", source, detail))

    def paint(self, text: str, color: int, bold: bool = False) -> str:
        if not self.color:
            return text
        return f"\x1b[{1 if bold else 0};{color}m{text}\x1b[0m"

    def render(self, source: Source, block: tuple[str, ...]) -> None:
        index = self.sources.index(source)
        label = self.paint(f"{source.label:>10}", PALETTE[index % len(PALETTE)], True)
        match = KIND.search(block[0])
        kind_color = KIND_COLORS.get(match.group(1).upper(), 97) if match else 97
        first = self.paint(block[0], kind_color, True)
        self.output.write(f"{label}  {first}\n")
        for detail in block[1:]:
            self.output.write(f"{'':>10}  {detail}\n")
        self.output.write("\n")
        self.output.flush()

    def render_notice(self, source: Source, detail: str) -> None:
        line = f"{source.label:>10}  [MONITOR] {detail}"
        self.output.write(self.paint(line, 91, True) + "\n\n")
        self.output.flush()

    def shutdown(self, *_args) -> None:
        self.stop.set()
        for process in self.processes:
            if process.poll() is None:
                try:
                    process.terminate()
                except ProcessLookupError:
                    pass

    def run(self) -> int:
        for source in self.sources:
            try:
                process = subprocess.Popen(
                    tail_command(source), stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                    text=True, bufsize=1,
                )
            except OSError as error:
                self.enqueue(("failure", source, None, sanitize(str(error))))
                continue
            self.processes.append(process)
            threading.Thread(target=self.stderr_reader, args=(source, process), daemon=True).start()
            threading.Thread(target=self.reader, args=(source, process), daemon=True).start()

        window_start = time.monotonic()
        rendered = 0
        deferred = deque(maxlen=256)
        summarized: Counter[tuple[str, str]] = Counter()
        while not self.stop.is_set():
            now = time.monotonic()
            if now - window_start >= 1.0:
                if summarized:
                    details = ", ".join(f"{label}/{kind}: {count}" for (label, kind), count in sorted(summarized.items()))
                    self.render_notice(Source("monitor", ""), "burst summarized; full events remain in node logs — " + details)
                    summarized.clear()
                    rendered = 1
                else:
                    rendered = 0
                window_start = now
            while deferred and rendered < 6:
                source, block = deferred.popleft()
                self.render(source, block)
                rendered += 1
            try:
                event = self.events.get(timeout=0.05)
            except queue.Empty:
                event = None
            with self.lock:
                dropped, self.dropped = self.dropped, 0
            if dropped:
                summarized[("monitor", "OVERFLOW")] += dropped
            if event:
                category, source, *payload = event
                if category == "block":
                    block = payload[0]
                    match = KIND.search(block[0])
                    kind = match.group(1).upper() if match else "EVENT"
                    if rendered < 6:
                        self.render(source, block)
                        rendered += 1
                    elif kind == "TRANSFER":
                        if len(deferred) < deferred.maxlen:
                            deferred.append((source, block))
                        else:
                            summarized[(source.label, kind)] += 1
                    else:
                        summarized[(source.label, kind)] += 1
                elif category == "warning":
                    self.render_notice(source, payload[0])
                else:
                    code, detail = payload
                    suffix = f": {detail}" if detail else ""
                    self.render_notice(source, f"source disconnected (exit {code}){suffix}")
            if all(p.poll() is not None for p in self.processes) and self.events.empty() and not deferred:
                break
        self.shutdown()
        for process in self.processes:
            try:
                process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
        return 0


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Merge blank-line-delimited qfsd demo events without interleaving blocks.",
        epilog=("examples:\n"
                "  demo_monitor.py vault-a=/var/lib/qfs/demo-events.log\n"
                "  demo_monitor.py vm1=demo@10.0.0.11:/var/lib/qfs/demo-events.log "
                "vm2=demo@10.0.0.12:/var/lib/qfs/demo-events.log"),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("source", nargs="+", type=parse_source,
                        metavar="LABEL=PATH|LABEL=USER@HOST:/PATH")
    args = parser.parse_args()
    monitor = Monitor(args.source)
    signal.signal(signal.SIGINT, monitor.shutdown)
    signal.signal(signal.SIGTERM, monitor.shutdown)
    return monitor.run()


if __name__ == "__main__":
    raise SystemExit(main())
