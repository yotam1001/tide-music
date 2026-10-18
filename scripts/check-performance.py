"""Validate an opt-in muted local benchmark; emit only aggregate measurements."""
import argparse
import json
import math
from pathlib import Path
import statistics


def number(value):
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value) or value < 0:
        raise ValueError("Invalid measurement")
    return value


def percentile(values, fraction):
    return sorted(values)[max(0, math.ceil(len(values) * fraction) - 1)]


def summarize(samples, cleanup, playback, max_memory=512, max_start=5000):
    if not isinstance(samples, list) or not 10 <= len(samples) <= 120:
        raise ValueError("Insufficient resource samples")
    metrics = {}
    for field in ("working_set_mib", "private_mib", "cpu_one_core_percent"):
        values = [number(row[field]) for row in samples]
        settled = values[5:]
        metrics[field] = {"peak": max(values), "settled_median": statistics.median(settled),
                          "settled_p95": percentile(settled, .95)}
    observed = cleanup.get("cleanup_observations", [])
    helpers = [row for row in observed if row.get("is_root") is False]
    if cleanup.get("exit_code") != 0 or cleanup.get("helpers_ended") is not True or not helpers or any(row.get("has_exited") is not True for row in helpers):
        raise ValueError("Missing successful process cleanup evidence")
    if playback.get("schema") != 1 or playback.get("workload") != "muted_local_3s":
        raise ValueError("Unsupported benchmark")
    delays = playback.get("request_to_progress_ms", [])
    if not 6 <= len(delays) <= 128 or playback.get("natural_transitions", 0) < 4 or playback.get("skips") != 3:
        raise ValueError("Insufficient playback coverage")
    delays = [number(delay) for delay in delays]
    if metrics["working_set_mib"]["peak"] > max_memory or max(delays) > max_start:
        raise ValueError("Local benchmark exceeded its regression budget")
    return {"schema": 1, "workload": "muted_local_3s", "samples": len(samples),
            "metrics": metrics, "observed_helpers": len(helpers),
            "sampling_races": number(cleanup.get("sampling_races", 0)),
            "natural_transitions": playback["natural_transitions"], "skips": playback["skips"],
            "request_to_progress_ms": {"count": len(delays), "median": statistics.median(delays),
                                       "p95": percentile(delays, .95), "max": max(delays)}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("samples", type=Path)
    parser.add_argument("playback", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    read = lambda path: json.loads(path.read_text(encoding="utf-8-sig"))
    result = summarize(read(args.samples), read(Path(str(args.samples) + ".exit.json")), read(args.playback))
    with args.output.open("x", encoding="utf-8") as target:
        json.dump(result, target, indent=2, allow_nan=False)
    print("PASS: muted local playback, process-tree resource budgets and observed helper cleanup")


if __name__ == "__main__":
    main()
