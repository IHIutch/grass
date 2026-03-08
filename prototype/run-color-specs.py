#!/usr/bin/env python3
"""Run sass-spec color tests against grass binary."""
import subprocess, os, sys, re
from pathlib import Path

GRASS = "./target/release/grass"
SPEC_DIR = "sass-spec/spec/core_functions/color"
LOAD_PATH = "sass-spec/spec"

def parse_hrx(path):
    files = {}
    with open(path) as f:
        content = f.read()
    entries = re.split(r'^<===>\s*', content, flags=re.MULTILINE)
    for entry in entries:
        entry = entry.strip()
        if not entry or entry.startswith('='):
            continue
        lines = entry.split('\n')
        filename = lines[0].strip()
        if filename.startswith('='):
            continue
        body_lines = [l for l in lines[1:] if not l.strip().startswith('=====')]
        body = '\n'.join(body_lines)
        body = body.rstrip('\n') + '\n' if body.strip() else ''
        files[filename] = body
    return files

def extract_tests(files):
    tests = []
    input_files = [f for f in files if f.endswith('/input.scss') or f == 'input.scss']
    for input_file in input_files:
        prefix = input_file.rsplit('input.scss', 1)[0]
        output_file = f"{prefix}output.css"
        error_file = f"{prefix}error"
        if output_file in files:
            tests.append({
                'name': prefix.rstrip('/') or 'root',
                'input': files[input_file],
                'expected': files[output_file],
                'type': 'success'
            })
        elif error_file in files:
            tests.append({
                'name': prefix.rstrip('/') or 'root',
                'input': files[input_file],
                'expected_error': files[error_file],
                'type': 'error'
            })
    return tests

def run_test(test):
    try:
        result = subprocess.run(
            [GRASS, "--stdin", "--style=expanded", "--load-path", LOAD_PATH],
            input=test['input'],
            capture_output=True,
            text=True,
            timeout=5
        )
        if test['type'] == 'success':
            actual = result.stdout.rstrip('\n') + '\n' if result.stdout.strip() else ''
            expected = test['expected'].rstrip('\n') + '\n' if test['expected'].strip() else ''
            if result.returncode != 0:
                return False, f"ERROR: {result.stderr[:200]}", expected.rstrip()
            return actual == expected, actual.rstrip(), expected.rstrip()
        else:
            if result.returncode != 0:
                return True, result.stderr.rstrip(), test['expected_error'].rstrip()
            return False, f"Expected error, got: {result.stdout[:100]}", ""
    except subprocess.TimeoutExpired:
        return False, "TIMEOUT", ""
    except Exception as e:
        return False, str(e), ""

def main():
    category = None
    show_failures = False
    limit = None
    args = sys.argv[1:]
    i = 0
    while i < len(args):
        if args[i] == "--failures":
            show_failures = True
        elif args[i] == "--limit":
            i += 1; limit = int(args[i])
        elif not args[i].startswith("--"):
            category = args[i]
        i += 1

    spec_dir = Path(SPEC_DIR)
    if category:
        spec_dir = spec_dir / category

    hrx_files = sorted(spec_dir.rglob("*.hrx"))
    total = passed = 0
    failed_tests = []
    category_stats = {}

    for hrx in hrx_files:
        rel_path = hrx.relative_to(Path(SPEC_DIR))
        cat = str(rel_path).split('/')[0]
        files = parse_hrx(hrx)
        tests = extract_tests(files)
        for test in tests:
            total += 1
            ok, actual, expected = run_test(test)
            if cat not in category_stats:
                category_stats[cat] = [0, 0]
            category_stats[cat][1] += 1
            if ok:
                passed += 1
                category_stats[cat][0] += 1
            else:
                test_name = f"{rel_path}::{test['name']}"
                failed_tests.append((test_name, actual, expected, test['type']))

    if total == 0:
        print("No tests found!"); return

    print(f"\nResults: {passed}/{total} passed ({100*passed/total:.1f}%)")
    print(f"Failed: {total - passed}")
    print("\nBy category:")
    for cat in sorted(category_stats):
        p, t = category_stats[cat]
        pct = 100*p/t if t else 0
        fail = t - p
        print(f"  {cat}: {p}/{t} ({pct:.0f}%) [{fail} failures]")

    if show_failures:
        shown = failed_tests[:limit] if limit else failed_tests[:100]
        print(f"\n--- Failures (showing {len(shown)}/{len(failed_tests)}) ---")
        for name, actual, expected, ttype in shown:
            print(f"\n[FAIL] {name} ({ttype})")
            if len(actual) < 300 and len(expected) < 300:
                print(f"  Expected: {expected[:300]}")
                print(f"  Actual:   {actual[:300]}")

if __name__ == '__main__':
    main()
