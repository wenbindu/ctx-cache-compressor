#!/usr/bin/env python3
"""Replay one sampled multi-turn conversation to ctx-cache-compressor and record per-step API outputs."""

from __future__ import annotations

import argparse
import json
import time
from pathlib import Path
from urllib import error, request


def http_json(method: str, url: str, payload: dict | None = None) -> dict:
    data = None if payload is None else json.dumps(payload, ensure_ascii=False).encode("utf-8")
    req = request.Request(url=url, data=data, method=method)
    req.add_header("Content-Type", "application/json")

    try:
        with request.urlopen(req, timeout=30) as resp:
            body = resp.read().decode("utf-8")
            return json.loads(body) if body else {}
    except error.HTTPError as e:
        body = e.read().decode("utf-8", errors="replace")
        raise RuntimeError(f"HTTP {e.code} {url}: {body}") from e


def load_sample(path: Path, sample_id: str | None, index: int | None) -> dict:
    samples = []
    with path.open("r", encoding="utf-8") as f:
        for line in f:
            samples.append(json.loads(line))

    if sample_id:
        for s in samples:
            if s.get("sample_id") == sample_id:
                return s
        raise RuntimeError(f"sample_id not found: {sample_id}")

    if index is None:
        index = 1

    if index < 1 or index > len(samples):
        raise RuntimeError(f"sample index out of range: {index}, total={len(samples)}")

    return samples[index - 1]


def summary_flags(context: dict) -> tuple[bool, str]:
    messages = context.get("messages", [])
    summary_present = False
    summary_preview = ""
    for m in messages:
        if m.get("role") == "system":
            content = str(m.get("content", ""))
            if content.startswith("[CONTEXT SUMMARY]"):
                summary_present = True
                summary_preview = content[:160]
                break
    return summary_present, summary_preview


def snapshot_context(ctx: dict) -> dict:
    summary_present, summary_preview = summary_flags(ctx)
    return {
        "turn_count": ctx.get("turn_count"),
        "is_compressing": ctx.get("is_compressing"),
        "compressed_turns": ctx.get("compressed_turns"),
        "token_estimate": ctx.get("token_estimate"),
        "context_message_count": len(ctx.get("messages", [])),
        "summary_present": summary_present,
        "summary_preview": summary_preview,
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--samples", default="data/jsonl/compression_test_samples_20.jsonl")
    parser.add_argument("--sample-id", default=None)
    parser.add_argument("--sample-index", type=int, default=1)
    parser.add_argument("--base-url", default="http://127.0.0.1:8080")
    parser.add_argument("--poll-interval", type=float, default=0.5)
    parser.add_argument("--poll-timeout", type=float, default=60.0)
    parser.add_argument("--output", default="")
    args = parser.parse_args()

    sample = load_sample(Path(args.samples), args.sample_id, args.sample_index)
    sample_label = sample["sample_id"]

    create = http_json("POST", f"{args.base_url}/sessions", {"system_prompt": "你是一个上下文压缩测试助手。"})
    session_id = create["session_id"]

    timeline = []

    for i, msg in enumerate(sample["messages"], start=1):
        append_resp = http_json(
            "POST",
            f"{args.base_url}/sessions/{session_id}/messages",
            {"role": msg["role"], "content": msg["content"]},
        )

        ctx = http_json("GET", f"{args.base_url}/sessions/{session_id}/context")
        step = {
            "step": i,
            "role": msg["role"],
            "content_preview": str(msg["content"]).replace("\n", " ")[:80],
            "append_response": append_resp,
            "context_snapshot": snapshot_context(ctx),
        }

        if append_resp.get("compression_triggered"):
            t0 = time.time()
            while True:
                ctx2 = http_json("GET", f"{args.base_url}/sessions/{session_id}/context")
                if not ctx2.get("is_compressing", False):
                    step["post_compression_snapshot"] = snapshot_context(ctx2)
                    break
                if time.time() - t0 > args.poll_timeout:
                    step["post_compression_snapshot"] = {
                        "timeout": True,
                        "last_snapshot": snapshot_context(ctx2),
                    }
                    break
                time.sleep(args.poll_interval)

        timeline.append(step)

    final_ctx = http_json("GET", f"{args.base_url}/sessions/{session_id}/context")

    report = {
        "sample": {
            "sample_id": sample["sample_id"],
            "source_session_id": sample["source_session_id"],
            "message_count": sample["message_count"],
            "turn_count": sample["turn_count"],
        },
        "replay_session_id": session_id,
        "timeline": timeline,
        "final_context": final_ctx,
        "final_snapshot": snapshot_context(final_ctx),
    }

    output = args.output or f"data/reports/replay_{sample_label}.json"
    output_path = Path(output)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")

    print(f"sample_id={sample_label}")
    print(f"source_session_id={sample['source_session_id']}")
    print(f"replay_session_id={session_id}")
    print(f"steps={len(timeline)}")
    print(f"output={output_path}")
    print("final_snapshot=" + json.dumps(report["final_snapshot"], ensure_ascii=False))


if __name__ == "__main__":
    main()
