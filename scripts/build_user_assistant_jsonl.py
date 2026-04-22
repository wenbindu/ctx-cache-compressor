#!/usr/bin/env python3
"""Convert legacy .xls chat exports into CSV and session-level user/assistant JSONL."""

from __future__ import annotations

import argparse
import csv
import glob
import json
import os
from collections import defaultdict
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Any

from python_calamine import CalamineWorkbook


@dataclass
class MessageRow:
    source_file: str
    message_id: str
    session_id: str
    role: str
    content: str
    create_time: str
    create_dt: datetime


def to_datetime(value: Any) -> datetime | None:
    if isinstance(value, datetime):
        return value

    if value is None:
        return None

    text = str(value).strip()
    if not text:
        return None

    # Try common datetime patterns.
    for fmt in (
        "%Y-%m-%d %H:%M:%S",
        "%Y/%m/%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y/%m/%d %H:%M",
    ):
        try:
            return datetime.strptime(text, fmt)
        except ValueError:
            pass

    # Last resort: ISO parser.
    try:
        return datetime.fromisoformat(text)
    except ValueError:
        return None


def normalize_role(value: Any) -> str:
    role = str(value or "").strip().lower()
    if role in {"user", "assistant"}:
        return role
    return ""


def normalize_message(value: Any) -> str:
    text = str(value or "")
    # Keep original content semantics, only trim outer whitespace.
    return text.strip()


def stringify(value: Any) -> str:
    if value is None:
        return ""
    if isinstance(value, datetime):
        return value.isoformat(sep=" ", timespec="seconds")
    return str(value)


def load_sheet_rows(path: str) -> tuple[list[str], list[list[Any]]]:
    wb = CalamineWorkbook.from_path(path)
    if not wb.sheet_names:
        return [], []
    sheet = wb.get_sheet_by_name(wb.sheet_names[0])
    rows = sheet.to_python()
    if not rows:
        return [], []

    header = [str(col).strip() for col in rows[0]]
    data_rows = rows[1:]
    return header, data_rows


def write_csv(path: Path, header: list[str], rows: list[list[Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f)
        writer.writerow(header)
        for row in rows:
            padded = row + [""] * (len(header) - len(row))
            writer.writerow([stringify(v) for v in padded[: len(header)]])


def extract_messages(source_file: str, header: list[str], rows: list[list[Any]]) -> list[MessageRow]:
    index = {name: i for i, name in enumerate(header)}
    required = ["message_id", "session_id", "sender", "message", "create_time"]
    missing = [col for col in required if col not in index]
    if missing:
        raise ValueError(f"{source_file}: missing required columns: {missing}")

    out: list[MessageRow] = []
    for row in rows:
        def get(col: str) -> Any:
            pos = index[col]
            return row[pos] if pos < len(row) else None

        session_id = str(get("session_id") or "").strip()
        role = normalize_role(get("sender"))
        content = normalize_message(get("message"))
        dt = to_datetime(get("create_time"))

        # Quality gates
        if not session_id or not role or not content or dt is None:
            continue

        message_id_raw = get("message_id")
        message_id = str(int(message_id_raw)) if isinstance(message_id_raw, float) and message_id_raw.is_integer() else str(message_id_raw)

        out.append(
            MessageRow(
                source_file=os.path.basename(source_file),
                message_id=message_id,
                session_id=session_id,
                role=role,
                content=content,
                create_time=dt.isoformat(sep=" ", timespec="seconds"),
                create_dt=dt,
            )
        )

    return out


def build_session_jsonl(messages: list[MessageRow], out_path: Path) -> dict[str, int]:
    # Deduplicate exact duplicates across source files.
    dedup = {}
    for m in messages:
        key = (m.session_id, m.message_id, m.role, m.content, m.create_time)
        dedup[key] = m

    sorted_msgs = sorted(
        dedup.values(),
        key=lambda m: (m.session_id, m.create_dt, m.message_id),
    )

    sessions: dict[str, list[MessageRow]] = defaultdict(list)
    for m in sorted_msgs:
        sessions[m.session_id].append(m)

    kept_sessions = 0
    skipped_sessions = 0
    total_messages = 0

    out_path.parent.mkdir(parents=True, exist_ok=True)
    with out_path.open("w", encoding="utf-8") as f:
        for session_id, msgs in sorted(sessions.items()):
            roles = {m.role for m in msgs}
            # Keep only meaningful user-assistant conversations.
            if len(msgs) < 2 or not {"user", "assistant"}.issubset(roles):
                skipped_sessions += 1
                continue

            record = {
                "session_id": session_id,
                "start_time": msgs[0].create_time,
                "end_time": msgs[-1].create_time,
                "message_count": len(msgs),
                "messages": [
                    {
                        "message_id": m.message_id,
                        "role": m.role,
                        "content": m.content,
                        "time": m.create_time,
                        "source_file": m.source_file,
                    }
                    for m in msgs
                ],
            }
            f.write(json.dumps(record, ensure_ascii=False) + "\n")
            kept_sessions += 1
            total_messages += len(msgs)

    return {
        "sessions_kept": kept_sessions,
        "sessions_skipped": skipped_sessions,
        "messages_kept": total_messages,
        "messages_total_after_row_filter": len(sorted_msgs),
    }


def build_flat_jsonl(messages: list[MessageRow], out_path: Path) -> int:
    dedup = {}
    for m in messages:
        key = (m.session_id, m.message_id, m.role, m.content, m.create_time)
        dedup[key] = m

    sorted_msgs = sorted(
        dedup.values(),
        key=lambda m: (m.session_id, m.create_dt, m.message_id),
    )

    out_path.parent.mkdir(parents=True, exist_ok=True)
    with out_path.open("w", encoding="utf-8") as f:
        for m in sorted_msgs:
            line = {
                "session_id": m.session_id,
                "message_id": m.message_id,
                "role": m.role,
                "content": m.content,
                "time": m.create_time,
                "source_file": m.source_file,
            }
            f.write(json.dumps(line, ensure_ascii=False) + "\n")

    return len(sorted_msgs)


def main() -> None:
    parser = argparse.ArgumentParser(description="Convert xls chat export to csv + user/assistant jsonl")
    parser.add_argument("--input-glob", default="raw/*.xls", help="Input xls glob")
    parser.add_argument("--csv-dir", default="data/csv", help="Output directory for converted csv files")
    parser.add_argument("--out-session-jsonl", default="data/jsonl/sessions_user_assistant.jsonl")
    parser.add_argument("--out-flat-jsonl", default="data/jsonl/messages_user_assistant.jsonl")
    args = parser.parse_args()

    files = sorted(glob.glob(args.input_glob))
    if not files:
        raise SystemExit(f"No input files matched: {args.input_glob}")

    all_messages: list[MessageRow] = []
    raw_rows_total = 0

    for path in files:
        header, rows = load_sheet_rows(path)
        raw_rows_total += len(rows)

        csv_name = Path(path).with_suffix(".csv").name
        write_csv(Path(args.csv_dir) / csv_name, header, rows)

        extracted = extract_messages(path, header, rows)
        all_messages.extend(extracted)
        print(f"[ok] {path}: rows={len(rows)} extracted_messages={len(extracted)}")

    flat_count = build_flat_jsonl(all_messages, Path(args.out_flat_jsonl))
    session_stats = build_session_jsonl(all_messages, Path(args.out_session_jsonl))

    print("\n=== summary ===")
    print(f"input_files={len(files)}")
    print(f"raw_rows_total={raw_rows_total}")
    print(f"messages_after_row_filter={len(all_messages)}")
    print(f"flat_jsonl_messages={flat_count}")
    for k, v in session_stats.items():
        print(f"{k}={v}")
    print(f"csv_dir={args.csv_dir}")
    print(f"session_jsonl={args.out_session_jsonl}")
    print(f"flat_jsonl={args.out_flat_jsonl}")


if __name__ == "__main__":
    main()
