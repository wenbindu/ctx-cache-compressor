#!/usr/bin/env python3
"""Replay real session-level JSONL data and collect summary quality artifacts."""

from __future__ import annotations

import argparse
import json
import time
from pathlib import Path
from typing import Any
from urllib import error, request

CONTEXT_SUMMARY_PREFIX = "[CONTEXT SUMMARY]"


def http_json(method: str, url: str, payload: dict | None = None) -> tuple[int, dict]:
    data = None if payload is None else json.dumps(payload, ensure_ascii=False).encode("utf-8")
    req = request.Request(url=url, data=data, method=method)
    req.add_header("Content-Type", "application/json")
    try:
        with request.urlopen(req, timeout=60) as resp:
            body = resp.read().decode("utf-8")
            parsed = json.loads(body) if body else {}
            return resp.getcode(), parsed
    except error.HTTPError as exc:
        body = exc.read().decode("utf-8", errors="replace")
        try:
            parsed = json.loads(body) if body else {}
        except Exception:
            parsed = {"raw": body}
        return exc.code, parsed


def count_turns(messages: list[dict[str, Any]]) -> int:
    return sum(1 for m in messages if m.get("role") == "assistant")


def load_candidates(
    sessions_path: Path,
    max_sessions: int,
    min_turns: int,
    max_turns: int,
    max_messages: int,
) -> list[dict[str, Any]]:
    candidates: list[dict[str, Any]] = []

    with sessions_path.open("r", encoding="utf-8") as f:
        for line in f:
            if not line.strip():
                continue
            row = json.loads(line)

            raw_messages = row.get("messages", [])
            messages: list[dict[str, str]] = []
            for m in raw_messages:
                role = m.get("role")
                content = str(m.get("content", "")).strip()
                if role not in ("user", "assistant"):
                    continue
                if not content:
                    continue
                messages.append({"role": role, "content": content})

            if len(messages) < 2 or len(messages) > max_messages:
                continue

            turns = count_turns(messages)
            if turns < min_turns or turns > max_turns:
                continue

            if messages[-1]["role"] != "assistant":
                continue

            candidates.append(
                {
                    "source_session_id": row.get("session_id"),
                    "start_time": row.get("start_time"),
                    "end_time": row.get("end_time"),
                    "message_count": len(messages),
                    "turn_count": turns,
                    "messages": messages,
                }
            )

            if len(candidates) >= max_sessions:
                break

    return candidates


def extract_latest_summary(messages: list[dict[str, Any]]) -> str:
    for m in reversed(messages):
        if m.get("role") != "system":
            continue
        content = str(m.get("content", ""))
        if content.startswith(CONTEXT_SUMMARY_PREFIX):
            return content
    return ""


def tail_preview(messages: list[dict[str, str]], n: int = 6) -> list[str]:
    out = []
    for m in messages[-n:]:
        out.append(f"{m['role']}: {m['content'][:120]}")
    return out


def replay_one_session(
    base_url: str,
    source: dict[str, Any],
    poll_interval: float,
    poll_timeout: float,
) -> dict[str, Any]:
    status, create_resp = http_json(
        "POST",
        f"{base_url}/sessions",
        {"system_prompt": "你是一个上下文压缩测试助手。"},
    )
    if status != 200:
        return {
            "source_session_id": source["source_session_id"],
            "ok": False,
            "error": f"create failed: status={status}, resp={create_resp}",
        }

    replay_session_id = create_resp.get("session_id", "")
    triggers: list[dict[str, Any]] = []

    for idx, m in enumerate(source["messages"], start=1):
        status, append_resp = http_json(
            "POST",
            f"{base_url}/sessions/{replay_session_id}/messages",
            {"role": m["role"], "content": m["content"]},
        )

        if status != 200:
            return {
                "source_session_id": source["source_session_id"],
                "replay_session_id": replay_session_id,
                "ok": False,
                "error": f"append failed at step={idx}, status={status}, resp={append_resp}",
            }

        if append_resp.get("compression_triggered"):
            status_ctx, ctx = http_json(
                "GET",
                f"{base_url}/sessions/{replay_session_id}/context",
                None,
            )
            if status_ctx != 200:
                return {
                    "source_session_id": source["source_session_id"],
                    "replay_session_id": replay_session_id,
                    "ok": False,
                    "error": f"context fetch failed after trigger, status={status_ctx}, resp={ctx}",
                }

            pre_count = len(ctx.get("messages", []))

            t0 = time.time()
            while ctx.get("is_compressing", False):
                if time.time() - t0 > poll_timeout:
                    break
                time.sleep(poll_interval)
                status_ctx, ctx = http_json(
                    "GET",
                    f"{base_url}/sessions/{replay_session_id}/context",
                    None,
                )
                if status_ctx != 200:
                    break

            triggers.append(
                {
                    "step": idx,
                    "turn_count": append_resp.get("turn_count"),
                    "message_count": append_resp.get("message_count"),
                    "context_message_count_before_done": pre_count,
                    "context_message_count_after_done": len(ctx.get("messages", [])),
                    "compressed_turns_after_done": ctx.get("compressed_turns"),
                    "summary_after_done": extract_latest_summary(ctx.get("messages", [])),
                }
            )

    final_status, final_ctx = http_json(
        "GET",
        f"{base_url}/sessions/{replay_session_id}/context",
        None,
    )

    if final_status != 200:
        return {
            "source_session_id": source["source_session_id"],
            "replay_session_id": replay_session_id,
            "ok": False,
            "error": f"final context failed: status={final_status}, resp={final_ctx}",
        }

    return {
        "source_session_id": source["source_session_id"],
        "start_time": source.get("start_time"),
        "end_time": source.get("end_time"),
        "source_message_count": source.get("message_count"),
        "source_turn_count": source.get("turn_count"),
        "replay_session_id": replay_session_id,
        "ok": True,
        "trigger_count": len(triggers),
        "triggers": triggers,
        "final_snapshot": {
            "turn_count": final_ctx.get("turn_count"),
            "compressed_turns": final_ctx.get("compressed_turns"),
            "is_compressing": final_ctx.get("is_compressing"),
            "context_message_count": len(final_ctx.get("messages", [])),
            "token_estimate": final_ctx.get("token_estimate"),
            "summary_present": bool(extract_latest_summary(final_ctx.get("messages", []))),
            "latest_summary": extract_latest_summary(final_ctx.get("messages", [])),
        },
        "source_tail_preview": tail_preview(source["messages"], n=6),
    }


def write_markdown(results: list[dict[str, Any]], output_md: Path) -> None:
    lines: list[str] = []
    lines.append("# Real JSONL Summary Evaluation")
    lines.append("")
    ok_count = sum(1 for r in results if r.get("ok"))
    lines.append(f"- Total replayed: {len(results)}")
    lines.append(f"- Success: {ok_count}")
    lines.append("")
    lines.append("| source_session_id | source_turns | triggers | final_context_msgs | summary_present |")
    lines.append("|---|---:|---:|---:|---|")
    for r in results:
        if not r.get("ok"):
            lines.append(
                f"| {r.get('source_session_id')} | - | - | - | error |"
            )
            continue
        lines.append(
            "| {sid} | {turns} | {triggers} | {final_msgs} | {present} |".format(
                sid=r.get("source_session_id"),
                turns=r.get("source_turn_count"),
                triggers=r.get("trigger_count"),
                final_msgs=r.get("final_snapshot", {}).get("context_message_count"),
                present=r.get("final_snapshot", {}).get("summary_present"),
            )
        )

    for idx, r in enumerate(results, start=1):
        lines.append("")
        lines.append(f"## Session {idx}: {r.get('source_session_id')}")
        lines.append("")
        if not r.get("ok"):
            lines.append(f"- Status: failed")
            lines.append(f"- Error: {r.get('error')}")
            continue

        lines.append(f"- Source turns: {r.get('source_turn_count')}")
        lines.append(f"- Source messages: {r.get('source_message_count')}")
        lines.append(f"- Trigger count: {r.get('trigger_count')}")
        lines.append(
            f"- Trigger turns: {[t.get('turn_count') for t in r.get('triggers', [])]}"
        )
        lines.append(
            f"- Final context messages: {r.get('final_snapshot', {}).get('context_message_count')}"
        )
        lines.append(
            f"- Final compressed turns: {r.get('final_snapshot', {}).get('compressed_turns')}"
        )

        lines.append("")
        lines.append("### Latest Summary")
        lines.append("")
        lines.append("```text")
        lines.append(r.get("final_snapshot", {}).get("latest_summary", ""))
        lines.append("```")

        lines.append("")
        lines.append("### Source Tail Preview")
        lines.append("")
        for row in r.get("source_tail_preview", []):
            lines.append(f"- {row}")

        lines.append("")
        lines.append("### Trigger Details")
        lines.append("")
        for t in r.get("triggers", []):
            lines.append(
                "- step={step}, turn_count={turn_count}, msg_count={msg_count}, ctx_before={before}, ctx_after={after}, compressed_turns={compressed}".format(
                    step=t.get("step"),
                    turn_count=t.get("turn_count"),
                    msg_count=t.get("message_count"),
                    before=t.get("context_message_count_before_done"),
                    after=t.get("context_message_count_after_done"),
                    compressed=t.get("compressed_turns_after_done"),
                )
            )

    output_md.write_text("\n".join(lines) + "\n", encoding="utf-8")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--sessions", default="data/jsonl/sessions_user_assistant.jsonl")
    parser.add_argument("--base-url", default="http://127.0.0.1:8080")
    parser.add_argument("--max-sessions", type=int, default=5)
    parser.add_argument("--min-turns", type=int, default=6)
    parser.add_argument("--max-turns", type=int, default=12)
    parser.add_argument("--max-messages", type=int, default=40)
    parser.add_argument("--poll-interval", type=float, default=0.5)
    parser.add_argument("--poll-timeout", type=float, default=90.0)
    parser.add_argument("--output-json", default="data/reports/real_jsonl_summary_eval.json")
    parser.add_argument("--output-md", default="data/reports/real_jsonl_summary_eval.md")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    sessions_path = Path(args.sessions)
    output_json = Path(args.output_json)
    output_md = Path(args.output_md)
    output_json.parent.mkdir(parents=True, exist_ok=True)

    candidates = load_candidates(
        sessions_path=sessions_path,
        max_sessions=args.max_sessions,
        min_turns=args.min_turns,
        max_turns=args.max_turns,
        max_messages=args.max_messages,
    )

    results = []
    for source in candidates:
        results.append(
            replay_one_session(
                base_url=args.base_url,
                source=source,
                poll_interval=args.poll_interval,
                poll_timeout=args.poll_timeout,
            )
        )

    report = {
        "settings": {
            "sessions": str(sessions_path),
            "max_sessions": args.max_sessions,
            "min_turns": args.min_turns,
            "max_turns": args.max_turns,
            "max_messages": args.max_messages,
            "base_url": args.base_url,
        },
        "results": results,
    }

    output_json.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    write_markdown(results, output_md)

    ok_count = sum(1 for r in results if r.get("ok"))
    print(f"candidates={len(candidates)}")
    print(f"ok={ok_count}")
    print(f"output_json={output_json}")
    print(f"output_md={output_md}")


if __name__ == "__main__":
    main()
