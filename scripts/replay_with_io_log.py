#!/usr/bin/env python3
"""Replay a sample and log input/output for every API call."""

from __future__ import annotations

import argparse
import json
import time
from datetime import datetime
from pathlib import Path
from typing import Any
from urllib import error, request


def now() -> str:
    return datetime.utcnow().isoformat(timespec="seconds") + "Z"


def http_json(method: str, url: str, payload: dict | None = None) -> tuple[int, dict]:
    data = None if payload is None else json.dumps(payload, ensure_ascii=False).encode("utf-8")
    req = request.Request(url=url, data=data, method=method)
    req.add_header("Content-Type", "application/json")

    try:
        with request.urlopen(req, timeout=30) as resp:
            body = resp.read().decode("utf-8")
            parsed = json.loads(body) if body else {}
            return resp.getcode(), parsed
    except error.HTTPError as e:
        body = e.read().decode("utf-8", errors="replace")
        parsed: dict[str, Any]
        try:
            parsed = json.loads(body) if body else {}
        except Exception:
            parsed = {"raw": body}
        return e.code, parsed


def load_sample(samples_path: Path, sample_id: str | None, sample_index: int) -> dict:
    samples = [json.loads(line) for line in samples_path.read_text(encoding="utf-8").splitlines() if line.strip()]

    if sample_id:
        for s in samples:
            if s.get("sample_id") == sample_id:
                return s
        raise RuntimeError(f"sample_id not found: {sample_id}")

    if sample_index < 1 or sample_index > len(samples):
        raise RuntimeError(f"sample_index out of range: {sample_index}, total={len(samples)}")

    return samples[sample_index - 1]


def write_call(logf, seq: int, method: str, path: str, payload: dict | None, status: int, resp: dict) -> None:
    logf.write(f"\n=== CALL {seq:04d} {method} {path} @ {now()} ===\n")
    logf.write("INPUT:\n")
    if payload is None:
        logf.write("null\n")
    else:
        logf.write(json.dumps(payload, ensure_ascii=False, indent=2) + "\n")
    logf.write(f"OUTPUT (status={status}):\n")
    logf.write(json.dumps(resp, ensure_ascii=False, indent=2) + "\n")
    logf.flush()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--samples", default="data/jsonl/compression_test_samples_20.jsonl")
    parser.add_argument("--sample-id", default=None)
    parser.add_argument("--sample-index", type=int, default=1)
    parser.add_argument("--base-url", default="http://127.0.0.1:8080")
    parser.add_argument("--system-prompt", default="你是一个上下文压缩测试助手。")
    parser.add_argument("--output", default="")
    parser.add_argument("--poll-interval", type=float, default=0.5)
    parser.add_argument("--poll-timeout", type=float, default=60.0)
    args = parser.parse_args()

    sample = load_sample(Path(args.samples), args.sample_id, args.sample_index)
    sample_id = sample["sample_id"]
    out_path = Path(args.output or f"data/reports/replay_io_{sample_id}.log")
    out_path.parent.mkdir(parents=True, exist_ok=True)

    seq = 0

    with out_path.open("w", encoding="utf-8") as logf:
        logf.write("# replay_with_io_log\n")
        logf.write(f"sample_id={sample['sample_id']}\n")
        logf.write(f"source_session_id={sample['source_session_id']}\n")
        logf.write(f"message_count={sample['message_count']}\n")

        # Create session
        seq += 1
        path = "/sessions"
        payload = {"system_prompt": args.system_prompt}
        status, resp = http_json("POST", f"{args.base_url}{path}", payload)
        write_call(logf, seq, "POST", path, payload, status, resp)
        if status != 200:
            raise RuntimeError(f"create session failed with status={status}")
        session_id = resp["session_id"]
        logf.write(f"replay_session_id={session_id}\n")

        # Replay each message
        for i, msg in enumerate(sample["messages"], start=1):
            seq += 1
            path = f"/sessions/{session_id}/messages"
            payload = {"role": msg["role"], "content": msg["content"]}
            status, append_resp = http_json("POST", f"{args.base_url}{path}", payload)
            write_call(logf, seq, "POST", path, payload, status, append_resp)

            seq += 1
            cpath = f"/sessions/{session_id}/context"
            cstatus, ctx = http_json("GET", f"{args.base_url}{cpath}", None)
            write_call(logf, seq, "GET", cpath, None, cstatus, ctx)

            if append_resp.get("compression_triggered"):
                t0 = time.time()
                while True:
                    cstatus2, ctx2 = http_json("GET", f"{args.base_url}{cpath}", None)
                    if not ctx2.get("is_compressing", False):
                        seq += 1
                        write_call(logf, seq, "GET", cpath, None, cstatus2, ctx2)
                        break
                    if time.time() - t0 > args.poll_timeout:
                        seq += 1
                        write_call(logf, seq, "GET", cpath, None, cstatus2, ctx2)
                        break
                    time.sleep(args.poll_interval)

        # Final context snapshot
        seq += 1
        fpath = f"/sessions/{session_id}/context"
        fstatus, fresp = http_json("GET", f"{args.base_url}{fpath}", None)
        write_call(logf, seq, "GET", fpath, None, fstatus, fresp)

    print(f"sample_id={sample_id}")
    print(f"replay_session_id={session_id}")
    print(f"log_file={out_path}")


if __name__ == "__main__":
    main()
