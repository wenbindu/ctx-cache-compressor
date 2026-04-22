#!/usr/bin/env python3
"""Project-local Day.app milestone push for ctx-cache-compressor.

This script intentionally does not modify global skill files.
Project policy is locked to:
  - app_name: rs-ctx-cache-compressor
  - level: alert
  - volume: 3
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from urllib.parse import quote, urlencode
from urllib.request import urlopen

DEFAULT_SKILL_CONFIG = Path("/root/.codex/skills/dayapp-mobile-push/config.json")
DEFAULT_APP_NAME = "rs-ctx-cache-compressor"
DEFAULT_LEVEL = "alert"
DEFAULT_VOLUME = "3"
DEFAULT_SOUND = "alarm"


def load_device_id(explicit: str | None) -> str:
    if explicit and explicit.strip():
        return explicit.strip()

    if DEFAULT_SKILL_CONFIG.exists():
        try:
            data = json.loads(DEFAULT_SKILL_CONFIG.read_text(encoding="utf-8"))
            deviceid = str(data.get("deviceid", "")).strip()
            if deviceid:
                return deviceid
        except Exception:
            pass

    raise ValueError(
        "deviceid not found. Pass --device-id or configure "
        "/root/.codex/skills/dayapp-mobile-push/config.json"
    )


def build_url(deviceid: str, task_name: str, task_summary: str) -> str:
    title = quote(f"{DEFAULT_APP_NAME}-{task_name}", safe="")
    body = quote(task_summary, safe="")
    params = {
        "group": DEFAULT_APP_NAME,
        "isArchive": "1",
        "badge": "1",
        "sound": DEFAULT_SOUND,
        "level": DEFAULT_LEVEL,
        "volume": DEFAULT_VOLUME,
    }
    return f"https://api.day.app/{deviceid}/{title}/{body}?{urlencode(params)}"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Send project-local Day.app push")
    parser.add_argument("--task-name", required=True, help="Milestone name")
    parser.add_argument("--task-summary", required=True, help="Milestone summary")
    parser.add_argument("--device-id", default=None, help="Optional Bark device id override")
    parser.add_argument("--dry-run", action="store_true", help="Print URL only")
    return parser.parse_args()


def main() -> int:
    args = parse_args()

    task_name = args.task_name.strip()
    task_summary = args.task_summary.strip()
    if not task_name:
        print("task_name is empty", file=sys.stderr)
        return 1
    if not task_summary:
        print("task_summary is empty", file=sys.stderr)
        return 1

    try:
        deviceid = load_device_id(args.device_id)
        url = build_url(deviceid, task_name, task_summary)
        if args.dry_run:
            print(url)
            return 0

        with urlopen(url, timeout=10) as response:
            body = response.read().decode("utf-8", errors="replace")
            print(f"status={int(response.status)}")
            print(body)
            return 0 if 200 <= int(response.status) < 300 else 1
    except Exception as exc:
        print(f"send failed: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
