#!/usr/bin/env python3
"""Run sass-spec and reject failures not listed in the checked-in baseline."""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path


FAILURE_RE = re.compile(r"^\[FAIL\] (.+) \((success|error)\)$", re.MULTILINE)
SUMMARY_RE = re.compile(r"Results: (\d+)/(\d+) passed \(([^)]+)%\)")


def read_baseline(path: Path) -> set[str]:
    lines = [
        line.strip()
        for line in path.read_text().splitlines()
        if line.strip() and not line.lstrip().startswith("#")
    ]
    entries = set(lines)
    if len(entries) != len(lines):
        raise ValueError(f"duplicate identifier in baseline: {path}")
    return entries


def run_runner(root: Path) -> str:
    result = subprocess.run(
        [sys.executable, str(root / "run-sass-specs.py"), "--failures", "--limit", "100"],
        cwd=root,
        capture_output=True,
        text=True,
        check=False,
    )
    report = result.stdout + result.stderr
    print(report, end="")
    if result.returncode:
        raise RuntimeError(f"sass-spec runner exited with status {result.returncode}")
    return report


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--baseline",
        type=Path,
        default=Path("ci/sass-spec-baseline.txt"),
        help="baseline file (default: ci/sass-spec-baseline.txt)",
    )
    parser.add_argument(
        "--report",
        type=Path,
        help="parse a saved runner report instead of running sass-spec",
    )
    args = parser.parse_args()

    root = Path(__file__).resolve().parent.parent
    baseline_path = args.baseline if args.baseline.is_absolute() else root / args.baseline
    try:
        baseline = read_baseline(baseline_path)
        report = args.report.read_text() if args.report else run_runner(root)
        summary = SUMMARY_RE.search(report)
        failures = {match.group(1) for match in FAILURE_RE.finditer(report)}
        if not summary:
            raise ValueError("sass-spec report did not contain a Results summary")
        passed, total = map(int, summary.groups()[:2])
        if passed + len(failures) != total:
            raise ValueError(
                f"sass-spec report lists {passed} passing and {len(failures)} failures "
                f"for {total} tests; expected every failure to have an identifier"
            )
    except (OSError, ValueError, RuntimeError) as error:
        print(f"sass-spec baseline check failed: {error}", file=sys.stderr)
        return 1

    unexpected = sorted(failures - baseline)
    resolved = sorted(baseline - failures)
    print(
        f"sass-spec baseline: {passed}/{total} passed; "
        f"{len(failures)} failures; {len(unexpected)} new"
    )
    if resolved:
        print("Resolved baseline entries:")
        print("\n".join(f"  {entry}" for entry in resolved))
    if unexpected:
        print("New sass-spec failures:", file=sys.stderr)
        print("\n".join(f"  {entry}" for entry in unexpected), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
