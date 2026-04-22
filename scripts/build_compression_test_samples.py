#!/usr/bin/env python3
"""Build 20 high-quality replayable samples for compression service testing."""

from __future__ import annotations

import argparse
import csv
import json
from pathlib import Path


def is_replayable(messages: list[dict]) -> bool:
    if len(messages) < 10 or len(messages) % 2 != 0:
        return False

    roles = [m.get("role") for m in messages]
    if not roles or roles[0] != "user" or roles[-1] != "assistant":
        return False

    for i in range(1, len(roles)):
        if roles[i] == roles[i - 1]:
            return False

    # Basic content quality gate.
    for m in messages:
        content = str(m.get("content", "")).strip()
        if not content:
            return False

    return True


def pick_diverse_sessions(candidates: list[dict], k: int) -> list[dict]:
    if len(candidates) <= k:
        return candidates

    # Sort by length then time for stable deterministic pick.
    ordered = sorted(
        candidates,
        key=lambda x: (x.get("message_count", 0), x.get("start_time", ""), x.get("session_id", "")),
    )

    picked = []
    n = len(ordered)
    for i in range(k):
        idx = round(i * (n - 1) / (k - 1)) if k > 1 else 0
        picked.append(ordered[idx])

    # Deduplicate in rare collision due rounding.
    uniq = {}
    for item in picked:
        uniq[item["session_id"]] = item

    if len(uniq) < k:
        for item in ordered:
            if item["session_id"] not in uniq:
                uniq[item["session_id"]] = item
            if len(uniq) == k:
                break

    return list(uniq.values())[:k]


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--input",
        default="data/jsonl/sessions_user_assistant.jsonl",
        help="Input session-level jsonl",
    )
    parser.add_argument(
        "--output",
        default="data/jsonl/compression_test_samples_20.jsonl",
        help="Output sampled jsonl",
    )
    parser.add_argument(
        "--summary-csv",
        default="data/csv/compression_test_samples_20_summary.csv",
        help="Output sample summary csv",
    )
    parser.add_argument("--count", type=int, default=20)
    args = parser.parse_args()

    source = Path(args.input)
    if not source.exists():
        raise SystemExit(f"input file not found: {source}")

    candidates = []
    total = 0
    with source.open("r", encoding="utf-8") as f:
        for line in f:
            total += 1
            session = json.loads(line)
            messages = session.get("messages", [])
            if is_replayable(messages):
                candidates.append(session)

    picked = pick_diverse_sessions(candidates, args.count)

    out_path = Path(args.output)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    with out_path.open("w", encoding="utf-8") as f:
        for i, session in enumerate(picked, start=1):
            record = {
                "sample_id": f"sample_{i:02d}",
                "source_session_id": session["session_id"],
                "start_time": session.get("start_time"),
                "end_time": session.get("end_time"),
                "message_count": session.get("message_count", len(session.get("messages", []))),
                "turn_count": len(session.get("messages", [])) // 2,
                "messages": [
                    {
                        "role": m["role"],
                        "content": m["content"],
                        "time": m.get("time"),
                        "source_file": m.get("source_file"),
                        "message_id": m.get("message_id"),
                    }
                    for m in session.get("messages", [])
                ],
            }
            f.write(json.dumps(record, ensure_ascii=False) + "\n")

    summary_path = Path(args.summary_csv)
    summary_path.parent.mkdir(parents=True, exist_ok=True)
    with summary_path.open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f)
        writer.writerow(
            [
                "sample_id",
                "source_session_id",
                "start_time",
                "end_time",
                "message_count",
                "turn_count",
                "first_user_preview",
            ]
        )
        for i, session in enumerate(picked, start=1):
            messages = session.get("messages", [])
            preview = ""
            for m in messages:
                if m.get("role") == "user":
                    preview = str(m.get("content", "")).replace("\n", " ")[:60]
                    break
            writer.writerow(
                [
                    f"sample_{i:02d}",
                    session["session_id"],
                    session.get("start_time", ""),
                    session.get("end_time", ""),
                    len(messages),
                    len(messages) // 2,
                    preview,
                ]
            )

    print(f"total_sessions={total}")
    print(f"replayable_candidates={len(candidates)}")
    print(f"samples_written={len(picked)}")
    print(f"output_jsonl={out_path}")
    print(f"summary_csv={summary_path}")


if __name__ == "__main__":
    main()
