#!/usr/bin/env python3
"""Summarize local timing captures; durations and spacings are in milliseconds."""
import argparse
import json
import math
import statistics
from collections import defaultdict
from pathlib import Path


def summarize(values):
    values = sorted(values)
    if not values:
        return {"count": 0}

    def percentile(fraction):
        index = (len(values) - 1) * fraction
        low, high = math.floor(index), math.ceil(index)
        return values[low] + (values[high] - values[low]) * (index - low)

    return {
        "count": len(values),
        "min_ms": min(values),
        "mean_ms": statistics.mean(values),
        "median_ms": statistics.median(values),
        "p95_ms": percentile(.95),
        "p99_ms": percentile(.99),
        "max_ms": max(values),
        "stddev_ms": statistics.pstdev(values),
    }


def report(trace):
    groups = defaultdict(list)
    for sample in trace["samples"]:
        groups[sample["kind"]].append(sample)
        if sample["kind"] == "write":
            groups[f"write_light_{sample['light']}"].append(sample)
    result = {
        "duration_seconds": trace["duration_seconds"],
        "sample_limit_reached": trace["sample_limit_reached"],
        "failed_samples": sum(not sample["ok"] for sample in trace["samples"]),
        "durations": {},
        "spacing": {},
    }
    for name, samples in sorted(groups.items()):
        if name not in ("publish", "event_emit"):
            result["durations"][name] = summarize([s["duration_ms"] for s in samples])
        if name in ("apply", "publish", "event_emit") or name.startswith("write_light_"):
            for point in ("start", "finish") if name not in ("publish", "event_emit") else ("start",):
                timestamps = sorted(s["at_ms"] + (s["duration_ms"] if point == "finish" else 0) for s in samples)
                gaps = [b - a for a, b in zip(timestamps, timestamps[1:])]
                summary = summarize(gaps)
                summary.update({f"below_{cutoff}_ms": sum(gap < cutoff for gap in gaps) for cutoff in (175, 250, 300)})
                summary["above_400_ms"] = sum(gap > 400 for gap in gaps)
                result["spacing"][f"{name}_{point}"] = summary
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("trace", type=Path)
    args = parser.parse_args()
    print(json.dumps(report(json.loads(args.trace.read_text())), indent=2))
