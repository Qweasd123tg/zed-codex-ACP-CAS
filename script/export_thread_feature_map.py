#!/usr/bin/env python3
"""Export thread feature map into JSON, Mermaid, and Markmap formats.

Source of truth: docs/thread-feature-map.md (Mermaid flowcharts inside).
"""

from __future__ import annotations

import argparse
import json
import re
from collections import defaultdict
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Iterable


NODE_RE = re.compile(r"^([A-Za-z0-9_]+)(?:\[(.*)\])?$")


@dataclass(frozen=True)
class Edge:
    source: str
    target: str
    label: str | None


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Export docs/thread-feature-map.md into graph artifacts."
    )
    parser.add_argument(
        "--input",
        default="docs/thread-feature-map.md",
        help="Input markdown path.",
    )
    parser.add_argument(
        "--out-json",
        default="docs/thread-feature-map.graph.json",
        help="Output JSON graph path.",
    )
    parser.add_argument(
        "--out-mermaid",
        default="docs/thread-feature-map.graph.mmd",
        help="Output Mermaid graph path.",
    )
    parser.add_argument(
        "--out-markmap",
        default="docs/thread-feature-map.markmap.md",
        help="Output Markmap markdown path.",
    )
    return parser.parse_args()


def extract_mermaid_blocks(markdown: str) -> list[list[str]]:
    blocks: list[list[str]] = []
    inside = False
    current: list[str] = []
    for raw in markdown.splitlines():
        line = raw.rstrip("\n")
        if line.strip() == "```mermaid":
            inside = True
            current = []
            continue
        if inside and line.strip() == "```":
            inside = False
            blocks.append(current[:])
            current = []
            continue
        if inside:
            current.append(line)
    return blocks


def parse_node(token: str) -> tuple[str, str | None]:
    token = token.strip()
    match = NODE_RE.match(token)
    if not match:
        return token, None
    node_id = match.group(1)
    label = match.group(2)
    return node_id, label


def parse_edge_line(line: str) -> tuple[tuple[str, str | None], tuple[str, str | None], str | None] | None:
    if "-->" not in line:
        return None
    left, right = line.split("-->", 1)
    source = parse_node(left.strip())
    edge_label: str | None = None
    rest = right.strip()
    if rest.startswith("|"):
        second_pipe = rest.find("|", 1)
        if second_pipe != -1:
            edge_label = rest[1:second_pipe].strip() or None
            rest = rest[second_pipe + 1 :].strip()
    target = parse_node(rest)
    return source, target, edge_label


def collect_graph(blocks: Iterable[list[str]]) -> tuple[dict[str, str], list[Edge]]:
    labels: dict[str, str] = {}
    edges: list[Edge] = []
    seen_edges: set[tuple[str, str, str | None]] = set()

    for block in blocks:
        for raw in block:
            line = raw.strip()
            if not line or line.startswith("flowchart") or line.startswith("%%"):
                continue
            parsed = parse_edge_line(line)
            if parsed is None:
                continue
            (src_id, src_label), (dst_id, dst_label), edge_label = parsed
            if src_label:
                labels[src_id] = src_label
            labels.setdefault(src_id, src_id)
            if dst_label:
                labels[dst_id] = dst_label
            labels.setdefault(dst_id, dst_id)

            key = (src_id, dst_id, edge_label)
            if key not in seen_edges:
                seen_edges.add(key)
                edges.append(Edge(src_id, dst_id, edge_label))

    return labels, edges


def ensure_parent(path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)


def write_json(path: Path, labels: dict[str, str], edges: list[Edge], source_path: str) -> None:
    out_nodes = []
    for node_id in sorted(labels):
        label = labels[node_id]
        node = {
            "id": node_id,
            "label": label,
            "kind": "file" if label.startswith("src/") else "concept",
        }
        if label.startswith("src/"):
            node["path"] = label
        out_nodes.append(node)

    out_edges = [
        {
            "from": edge.source,
            "to": edge.target,
            **({"label": edge.label} if edge.label else {}),
        }
        for edge in edges
    ]

    payload = {
        "title": "Thread Feature Map (Curated)",
        "source": source_path,
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "stats": {
            "nodes": len(out_nodes),
            "edges": len(out_edges),
        },
        "nodes": out_nodes,
        "edges": out_edges,
    }

    ensure_parent(path)
    path.write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def write_mermaid(path: Path, labels: dict[str, str], edges: list[Edge]) -> None:
    lines = [
        "%% Generated by script/export_thread_feature_map.py",
        "flowchart TD",
    ]
    for node_id in sorted(labels):
        label = labels[node_id]
        safe = label.replace("[", "(").replace("]", ")")
        lines.append(f"    {node_id}[{safe}]")
    lines.append("")
    for edge in edges:
        if edge.label:
            lines.append(f"    {edge.source} -->|{edge.label}| {edge.target}")
        else:
            lines.append(f"    {edge.source} --> {edge.target}")
    lines.append("")

    ensure_parent(path)
    path.write_text("\n".join(lines), encoding="utf-8")


def write_markmap(path: Path, labels: dict[str, str], edges: list[Edge]) -> None:
    outgoing: dict[str, list[Edge]] = defaultdict(list)
    incoming: dict[str, list[Edge]] = defaultdict(list)
    for edge in edges:
        outgoing[edge.source].append(edge)
        incoming[edge.target].append(edge)

    lines: list[str] = [
        "# Thread Feature Map (Curated)",
        "",
        "Paste this file into https://markmap.js.org/repl",
        "",
        "## Legend",
        "- `Depends on` means outgoing edge (`A --> B`).",
        "- `Used by` means incoming edge.",
        "",
    ]

    for node_id in sorted(labels):
        node_label = labels[node_id]
        lines.append(f"## {node_id} - {node_label}")

        depends = sorted(outgoing.get(node_id, []), key=lambda e: (e.target, e.label or ""))
        if depends:
            lines.append("- Depends on")
            for edge in depends:
                target_label = labels.get(edge.target, edge.target)
                if edge.label:
                    lines.append(f"  - {edge.target} - {target_label} (edge: {edge.label})")
                else:
                    lines.append(f"  - {edge.target} - {target_label}")
        else:
            lines.append("- Depends on: none")

        used_by = sorted(incoming.get(node_id, []), key=lambda e: (e.source, e.label or ""))
        if used_by:
            lines.append("- Used by")
            for edge in used_by:
                source_label = labels.get(edge.source, edge.source)
                if edge.label:
                    lines.append(f"  - {edge.source} - {source_label} (edge: {edge.label})")
                else:
                    lines.append(f"  - {edge.source} - {source_label}")
        else:
            lines.append("- Used by: none")
        lines.append("")

    ensure_parent(path)
    path.write_text("\n".join(lines), encoding="utf-8")


def main() -> None:
    args = parse_args()
    input_path = Path(args.input)
    markdown = input_path.read_text(encoding="utf-8")
    blocks = extract_mermaid_blocks(markdown)
    if not blocks:
        raise SystemExit(f"No mermaid blocks found in {input_path}")
    labels, edges = collect_graph(blocks)
    if not labels or not edges:
        raise SystemExit(f"No graph edges extracted from {input_path}")

    write_json(Path(args.out_json), labels, edges, str(input_path))
    write_mermaid(Path(args.out_mermaid), labels, edges)
    write_markmap(Path(args.out_markmap), labels, edges)

    print(
        f"Generated {len(labels)} nodes and {len(edges)} edges:\n"
        f"- {args.out_json}\n"
        f"- {args.out_mermaid}\n"
        f"- {args.out_markmap}"
    )


if __name__ == "__main__":
    main()
