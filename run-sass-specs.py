#!/usr/bin/env python3
"""General-purpose sass-spec test runner for grass.

Extracts HRX archives to temp directories so multi-file tests
(@use/@forward with companion files) work correctly.

Usage:
    python3 run-sass-specs.py [CATEGORY] [OPTIONS]

    CATEGORY              Filter to spec subdirectory (e.g., "callable", "core_functions/list")
    --failures            Show failed test details
    --limit N             Limit failures shown (default: 100)
    --spec-dir DIR        Override spec root (default: sass-spec/spec)
    --skip-color          Exclude core_functions/color
    --only-multi          Only run tests that have companion files
"""

import subprocess, os, sys, re, tempfile, shutil
from pathlib import Path
from concurrent.futures import ProcessPoolExecutor, as_completed

PROJECT_ROOT = str(Path(__file__).resolve().parent)
GRASS = os.path.join(PROJECT_ROOT, "target/release/grass")
SPEC_DIR = os.path.join(PROJECT_ROOT, "sass-spec/spec")


# ── HRX Parser ──────────────────────────────────────────────────────────────

def parse_hrx(path):
    """Parse an HRX archive into a dict of {filename: content}."""
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


# ── Test Extraction ─────────────────────────────────────────────────────────

def extract_tests(files):
    """Classify HRX entries into tests and companion files.

    Returns (tests, companions) where:
      - tests: list of dicts with name, input_path, input, expected, type
      - companions: dict of {relative_path: content} for non-test .scss/.sass files
    """
    tests = []
    companions = {}
    input_files = sorted(f for f in files
                         if f.endswith('/input.scss') or f == 'input.scss'
                         or f.endswith('/input.sass') or f == 'input.sass')

    # Collect test prefixes to identify companion files
    test_prefixes = set()
    for input_file in input_files:
        prefix = re.sub(r'input\.s[ac]ss$', '', input_file)
        test_prefixes.add(prefix)

    # Identify companions: .scss/.sass files that aren't test inputs/outputs
    for fname, content in files.items():
        if not (fname.endswith('.scss') or fname.endswith('.sass') or fname.endswith('.css')):
            continue
        if fname.endswith('/input.scss') or fname == 'input.scss':
            continue
        if fname.endswith('/input.sass') or fname == 'input.sass':
            continue
        companions[fname] = content

    # Build test list
    for input_file in input_files:
        prefix = re.sub(r'input\.s[ac]ss$', '', input_file)
        output_file = f"{prefix}output.css"
        error_file = f"{prefix}error"
        options_file = f"{prefix}options.yml"

        # Skip :todo: tests
        if options_file in files and ':todo:' in files[options_file]:
            continue

        if output_file in files:
            tests.append({
                'name': prefix.rstrip('/') or 'root',
                'input_path': input_file,
                'input': files[input_file],
                'expected': files[output_file],
                'type': 'success',
            })
        elif error_file in files:
            tests.append({
                'name': prefix.rstrip('/') or 'root',
                'input_path': input_file,
                'input': files[input_file],
                'expected_error': files[error_file],
                'type': 'error',
            })
        # Tests with only warning files or no expected output are skipped

    return tests, companions


# ── Test Executor ───────────────────────────────────────────────────────────

def process_hrx(hrx_path, spec_dir, grass_binary):
    """Process all tests in a single HRX file.

    Creates a temp directory, writes companions and inputs, runs grass
    on each test with --load-path pointing to both temp and spec dirs.

    Returns list of (test_name, passed, actual, expected, test_type).
    """
    hrx_path = Path(hrx_path)
    spec_dir = Path(spec_dir)

    # hrx_base: e.g., "callable/arguments" from "sass-spec/spec/callable/arguments.hrx"
    hrx_rel = hrx_path.relative_to(spec_dir)
    hrx_base = str(hrx_rel.with_suffix(''))

    files = parse_hrx(hrx_path)
    tests, companions = extract_tests(files)

    if not tests:
        return []

    results = []
    tmpdir = tempfile.mkdtemp(prefix='grass_spec_')

    try:
        # Write companion files at temp/<hrx_base>/<companion_path>
        for comp_path, content in companions.items():
            full_path = Path(tmpdir) / hrx_base / comp_path
            full_path.parent.mkdir(parents=True, exist_ok=True)
            full_path.write_text(content)

        # Copy on-disk sibling .scss/.sass/.css files into tmpdir
        # so relative @use/@import (e.g., '../test-hue') can find them.
        hrx_parent_rel = str(hrx_rel.parent)  # e.g., "core_functions/color/hwb/three_args/w3c"
        dest_parent = Path(tmpdir) / hrx_parent_rel
        dest_parent.mkdir(parents=True, exist_ok=True)
        for sibling in hrx_path.parent.iterdir():
            if sibling.suffix in ('.scss', '.sass', '.css') and sibling.is_file():
                dest = dest_parent / sibling.name
                if not dest.exists():
                    shutil.copy2(str(sibling), str(dest))

        for test in tests:
            test_name = f"{hrx_rel}::{test['name']}"
            input_rel = Path(hrx_base) / test['input_path']
            input_full = Path(tmpdir) / input_rel
            input_full.parent.mkdir(parents=True, exist_ok=True)
            input_full.write_text(test['input'])

            try:
                proc = subprocess.Popen(
                    [grass_binary, str(input_full),
                     '--load-path', tmpdir,
                     '--load-path', str(spec_dir),
                     '--load-path', str(hrx_path.parent),
                     '--style=expanded'],
                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                    start_new_session=True,
                )
                try:
                    stdout, stderr = proc.communicate(timeout=5)
                except subprocess.TimeoutExpired:
                    import os, signal
                    try:
                        os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                    try:
                        proc.wait(timeout=1)
                    except subprocess.TimeoutExpired:
                        pass  # Zombie process — can't reap, but don't block
                    results.append((test_name, False, "TIMEOUT", "", test['type']))
                    continue

                if test['type'] == 'success':
                    actual = stdout.rstrip('\n') + '\n' if stdout.strip() else ''
                    expected = test['expected'].rstrip('\n') + '\n' if test['expected'].strip() else ''
                    if proc.returncode != 0:
                        results.append((test_name, False,
                                        f"ERROR: {stderr[:200]}",
                                        expected.rstrip(), 'success'))
                    else:
                        passed = actual == expected
                        results.append((test_name, passed,
                                        actual.rstrip(), expected.rstrip(), 'success'))
                else:  # error test
                    passed = proc.returncode != 0
                    actual = stderr.rstrip() if proc.returncode != 0 else f"Expected error, got: {stdout[:100]}"
                    results.append((test_name, passed,
                                    actual, test.get('expected_error', '').rstrip(), 'error'))

            except Exception as e:
                results.append((test_name, False, str(e), "", test['type']))

    finally:
        shutil.rmtree(tmpdir, ignore_errors=True)

    return results


def process_disk_test(test_dir, spec_dir, grass_binary):
    """Process an on-disk directory test (not HRX).

    Some tests under libsass-closed-issues exist as directories with
    input.scss + output.css files directly on disk.
    """
    test_dir = Path(test_dir)
    spec_dir = Path(spec_dir)

    # Find the input file
    input_file = test_dir / 'input.scss'
    if not input_file.exists():
        input_file = test_dir / 'input.sass'
    if not input_file.exists():
        return []

    # Check for :todo:
    options = test_dir / 'options.yml'
    if options.exists() and ':todo:' in options.read_text():
        return []

    output_file = test_dir / 'output.css'
    error_file = test_dir / 'error'
    rel_path = test_dir.relative_to(spec_dir)
    test_name = str(rel_path)

    results = []
    try:
        proc = subprocess.Popen(
            [grass_binary, str(input_file),
             '--load-path', str(spec_dir),
             '--style=expanded'],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
            start_new_session=True,
        )
        try:
            stdout, stderr = proc.communicate(timeout=5)
        except subprocess.TimeoutExpired:
            import os, signal
            try:
                os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
            except ProcessLookupError:
                pass
            try:
                proc.wait(timeout=1)
            except subprocess.TimeoutExpired:
                pass
            results.append((test_name, False, "TIMEOUT", "", 'success'))
            return results

        if output_file.exists():
            expected = output_file.read_text()
            actual = stdout.rstrip('\n') + '\n' if stdout.strip() else ''
            expected_norm = expected.rstrip('\n') + '\n' if expected.strip() else ''
            if proc.returncode != 0:
                results.append((test_name, False,
                                f"ERROR: {stderr[:200]}",
                                expected_norm.rstrip(), 'success'))
            else:
                passed = actual == expected_norm
                results.append((test_name, passed,
                                actual.rstrip(), expected_norm.rstrip(), 'success'))
        elif error_file.exists():
            expected_error = error_file.read_text()
            passed = proc.returncode != 0
            actual = stderr.rstrip() if proc.returncode != 0 else f"Expected error, got: {stdout[:100]}"
            results.append((test_name, passed, actual, expected_error.rstrip(), 'error'))

    except Exception as e:
        results.append((test_name, False, str(e), "", 'success'))

    return results


# ── Orchestrator ────────────────────────────────────────────────────────────

def collect_work(spec_dir, category=None, skip_color=False, only_multi=False):
    """Collect HRX files and on-disk test directories to process."""
    root = Path(spec_dir)
    if category:
        root = root / category

    hrx_files = sorted(root.rglob("*.hrx"))

    if skip_color:
        color_prefix = str(Path(spec_dir) / "core_functions" / "color")
        hrx_files = [h for h in hrx_files if not str(h).startswith(color_prefix)]

    if only_multi:
        # Filter to HRX files that have companion .scss/.sass files
        filtered = []
        for hrx in hrx_files:
            files = parse_hrx(hrx)
            _, companions = extract_tests(files)
            if companions:
                filtered.append(hrx)
        hrx_files = filtered

    # Find on-disk directory tests (input.scss not inside HRX)
    disk_tests = []
    for input_file in sorted(root.rglob("input.scss")):
        # Only include if this directory doesn't contain an HRX file
        # (HRX tests are handled separately)
        parent = input_file.parent
        if not any(parent == h.parent for h in hrx_files):
            disk_tests.append(parent)

    # Also check for input.sass
    for input_file in sorted(root.rglob("input.sass")):
        parent = input_file.parent
        if parent not in disk_tests and not any(parent == h.parent for h in hrx_files):
            disk_tests.append(parent)

    return hrx_files, disk_tests


def get_category(name):
    """Extract category (first path component) from a test name or path."""
    # test_name is like "callable/arguments.hrx::mixin/trailing_comma"
    # or for disk tests: "libsass-closed-issues/issue_185"
    rel = name.split('::')[0] if '::' in name else name
    parts = rel.split('/')
    return parts[0] if parts else 'unknown'


def run_all(spec_dir, category=None, skip_color=False, only_multi=False,
            grass_binary=GRASS, max_workers=None):
    """Run all tests in parallel, returning aggregated results."""
    hrx_files, disk_tests = collect_work(spec_dir, category, skip_color, only_multi)

    if not hrx_files and not disk_tests:
        return [], {}

    all_results = []
    workers = max_workers or os.cpu_count() or 4

    with ProcessPoolExecutor(max_workers=workers) as executor:
        futures = {}

        for hrx in hrx_files:
            f = executor.submit(process_hrx, str(hrx), spec_dir, grass_binary)
            futures[f] = str(hrx)

        for test_dir in disk_tests:
            f = executor.submit(process_disk_test, str(test_dir), spec_dir, grass_binary)
            futures[f] = str(test_dir)

        for future in as_completed(futures):
            try:
                results = future.result()
                all_results.extend(results)
            except Exception as e:
                print(f"ERROR processing {futures[future]}: {e}", file=sys.stderr)

    # Aggregate by category
    category_stats = {}
    for test_name, passed, _actual, _expected, _ttype in all_results:
        cat = get_category(test_name)
        if cat not in category_stats:
            category_stats[cat] = [0, 0]
        category_stats[cat][1] += 1
        if passed:
            category_stats[cat][0] += 1

    return all_results, category_stats


# ── Reporter ────────────────────────────────────────────────────────────────

def print_report(all_results, category_stats, show_failures=False, limit=100):
    """Print test results summary and optional failure details."""
    total = len(all_results)
    passed = sum(1 for _, p, _, _, _ in all_results if p)
    failed_tests = [(n, a, e, t) for n, p, a, e, t in all_results if not p]

    if total == 0:
        print("No tests found!")
        return

    pct = 100 * passed / total
    print(f"\nResults: {passed}/{total} passed ({pct:.1f}%)")
    print(f"Failed: {total - passed}")

    print("\nBy category:")
    for cat in sorted(category_stats):
        p, t = category_stats[cat]
        cpct = 100 * p / t if t else 0
        fail = t - p
        print(f"  {cat}: {p}/{t} ({cpct:.0f}%) [{fail} failures]")

    if show_failures and failed_tests:
        shown = failed_tests[:limit]
        print(f"\n--- Failures (showing {len(shown)}/{len(failed_tests)}) ---")
        for name, actual, expected, ttype in shown:
            print(f"\n[FAIL] {name} ({ttype})")
            if len(actual) < 300 and len(expected) < 300:
                print(f"  Expected: {expected[:300]}")
                print(f"  Actual:   {actual[:300]}")


# ── CLI ─────────────────────────────────────────────────────────────────────

def main():
    category = None
    show_failures = False
    limit = 100
    spec_dir = SPEC_DIR
    skip_color = False
    only_multi = False

    args = sys.argv[1:]
    i = 0
    while i < len(args):
        arg = args[i]
        if arg == '--failures':
            show_failures = True
        elif arg == '--limit':
            i += 1
            limit = int(args[i])
        elif arg == '--spec-dir':
            i += 1
            spec_dir = args[i]
        elif arg == '--skip-color':
            skip_color = True
        elif arg == '--only-multi':
            only_multi = True
        elif arg == '--help' or arg == '-h':
            print(__doc__)
            return
        elif not arg.startswith('--'):
            category = arg
        i += 1

    # Verify grass binary exists
    grass = GRASS
    if not os.path.isfile(grass):
        print(f"Error: grass binary not found at {grass}", file=sys.stderr)
        print("Run: cargo build --release", file=sys.stderr)
        sys.exit(1)

    all_results, category_stats = run_all(
        spec_dir, category, skip_color, only_multi, grass)

    print_report(all_results, category_stats, show_failures, limit)


if __name__ == '__main__':
    main()
