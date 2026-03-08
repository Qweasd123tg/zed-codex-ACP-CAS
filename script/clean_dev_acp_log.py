#!/usr/bin/env python3
"""
Clean noisy ACP dev logs.

Default behavior:
- drops streaming text-chunk events from `session/update`
- keeps plan/tool/file events
- redacts bulky text payloads (file contents, rawInput/rawOutput, diff old/new text)

Usage:
  python3 script/clean_dev_acp_log.py raw.log > clean.json
  python3 script/clean_dev_acp_log.py raw.log clean.json
"""

from __future__ import annotations

import argparse
import copy
import json
import sys
from typing import Any, Iterable


TEXT_CHUNK_UPDATES = {
    "agent_message_chunk",
    "user_message_chunk",
    "agent_thought_chunk",
    "reasoning_text_delta",
    "reasoning_summary_text_delta",
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Clean ACP dev logs from noisy text chunks.")
    parser.add_argument("input", help="Path to raw ACP log (JSON array or JSONL).")
    parser.add_argument(
        "output",
        nargs="?",
        default="-",
        help="Output path (default: stdout).",
    )
    parser.add_argument(
        "--keep-text-chunks",
        action="store_true",
        help="Do not remove streaming text chunk updates.",
    )
    parser.add_argument(
        "--jsonl",
        action="store_true",
        help="Write JSONL instead of a single JSON array.",
    )
    return parser.parse_args()


def load_events(path: str) -> list[dict[str, Any]]:
    with open(path, "r", encoding="utf-8") as f:
        raw = f.read().strip()

    if not raw:
        return []

    # 1) Full JSON value (array/object)
    try:
        parsed = json.loads(raw)
        if isinstance(parsed, list):
            return [item for item in parsed if isinstance(item, dict)]
        if isinstance(parsed, dict):
            if isinstance(parsed.get("events"), list):
                return [item for item in parsed["events"] if isinstance(item, dict)]
            return [parsed]
    except json.JSONDecodeError:
        pass

    # 2) JSON Lines fallback
    events: list[dict[str, Any]] = []
    for line in raw.splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            item = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(item, dict):
            events.append(item)
    return events


def is_text_chunk_update(event: dict[str, Any]) -> bool:
    if event.get("method") != "session/update":
        return False
    params = event.get("params")
    if not isinstance(params, dict):
        return False
    update = params.get("update")
    if not isinstance(update, dict):
        return False
    kind = update.get("sessionUpdate")
    return isinstance(kind, str) and kind in TEXT_CHUNK_UPDATES


def redact_string(value: Any) -> Any:
    if isinstance(value, str):
        return f"<omitted {len(value)} chars>"
    return value


def sanitize_tool_update(update: dict[str, Any]) -> dict[str, Any]:
    clean = copy.deepcopy(update)

    if "rawInput" in clean:
        clean["rawInput"] = "<omitted>"
    if "rawOutput" in clean:
        clean["rawOutput"] = "<omitted>"

    content = clean.get("content")
    if isinstance(content, list):
        new_content = []
        for item in content:
            if not isinstance(item, dict):
                continue
            item_type = item.get("type")
            # Streaming/content text inside tool updates is usually very noisy.
            if item_type == "content":
                continue
            if item_type == "diff":
                item = copy.deepcopy(item)
                if "oldText" in item:
                    item["oldText"] = redact_string(item.get("oldText"))
                if "newText" in item:
                    item["newText"] = redact_string(item.get("newText"))
            new_content.append(item)
        clean["content"] = new_content

    return clean


def sanitize_event(event: dict[str, Any]) -> dict[str, Any]:
    clean = copy.deepcopy(event)
    method = clean.get("method")

    # Trim huge file payloads in direct fs RPC requests.
    if isinstance(method, str) and method.startswith("fs/"):
        params = clean.get("params")
        if isinstance(params, dict) and "content" in params:
            params["content"] = redact_string(params.get("content"))
            clean["params"] = params

    if method == "session/update":
        params = clean.get("params")
        if isinstance(params, dict):
            update = params.get("update")
            if isinstance(update, dict):
                kind = update.get("sessionUpdate")
                if kind in {"tool_call", "tool_call_update"}:
                    params["update"] = sanitize_tool_update(update)
                    clean["params"] = params

    return clean


def clean_events(events: Iterable[dict[str, Any]], keep_text_chunks: bool) -> list[dict[str, Any]]:
    out: list[dict[str, Any]] = []
    for event in events:
        if not keep_text_chunks and is_text_chunk_update(event):
            continue
        out.append(sanitize_event(event))
    return out


def write_output(path: str, events: list[dict[str, Any]], as_jsonl: bool) -> None:
    if path == "-":
        writer = sys.stdout
        close_after = False
    else:
        writer = open(path, "w", encoding="utf-8")
        close_after = True

    try:
        if as_jsonl:
            for event in events:
                writer.write(json.dumps(event, ensure_ascii=False))
                writer.write("\n")
        else:
            json.dump(events, writer, ensure_ascii=False, indent=2)
            writer.write("\n")
    finally:
        if close_after:
            writer.close()


def main() -> int:
    args = parse_args()
    events = load_events(args.input)
    cleaned = clean_events(events, keep_text_chunks=args.keep_text_chunks)
    write_output(args.output, cleaned, as_jsonl=args.jsonl)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
