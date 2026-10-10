"""Summarize process-tree samples, excluding the first five startup samples."""
import argparse
import json
import math
import statistics
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("samples", nargs="+", type=Path)
    parser.add_argument("--discard", type=int, default=5)
    args = parser.parse_args()
    if args.discard < 0:
        parser.error("--discard must be nonnegative")
    for path in args.samples:
        rows = json.loads(path.read_text(encoding="utf-8-sig"))
        if isinstance(rows, dict):
            rows = [rows]
        rows = rows[args.discard:]
        if not rows:
            parser.error(f"{path.name}: no settled samples remain")
        summary = {"file": path.name, "settled_samples": len(rows)}
        for key in ("private_mib", "working_set_mib", "cpu_one_core_percent"):
            values = sorted(float(row[key]) for row in rows)
            summary[key] = {
                "median": round(statistics.median(values), 3),
                "p95": values[math.ceil(len(values) * 0.95) - 1],
                "peak": max(values),
            }
        print(json.dumps(summary))


if __name__ == "__main__":
    main()
