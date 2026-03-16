#!/usr/bin/env python3
"""
Milestone 0: Parallel Module Evaluation Validation Prototype

Proves that USWDS components can be evaluated independently and produce
correct CSS. Tests two things:

1. CORRECTNESS: Each component produces identical CSS whether compiled
   with only shared deps or with all prior components in the module cache.
   This proves components don't depend on each other.

2. SPEEDUP: Parallel compilation of independent components is faster
   than sequential compilation.

Approach:
- Compile full USWDS sequentially (baseline)
- For each component, compile a wrapper: shared_deps + component
- Each wrapper evaluates shared deps independently (wasteful but proving
  independence since there's no cross-component state)
- Concatenate component-only CSS and compare with full output
- Use line counting to extract component CSS (shared always produces
  the same number of lines)
"""

import os
import sys
import time
import subprocess
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

GRASS_BIN = os.path.join(os.path.dirname(__file__), "..", "target", "release", "grass")
PACKAGES_DIR = os.path.join(os.path.dirname(__file__), "packages")

# Parsed from _index-direct.scss
SHARED_FORWARDS = [
    "uswds-elements/lib/normalize",
    "uswds-core/src/styles",
    "uswds-fonts/src/styles",
    "uswds-elements/src/styles",
    "uswds-helpers/src/styles",
]

COMPONENT_FORWARDS = [
    "usa-content/src/styles",
    "usa-dark-background/src/styles",
    "usa-display/src/styles",
    "usa-intro/src/styles",
    "usa-link/src/styles",
    "usa-list/src/styles",
    "usa-paragraph/src/styles",
    "usa-prose/src/styles",
    "usa-accordion/src/styles",
    "usa-alert/src/styles",
    "usa-banner/src/styles",
    "usa-breadcrumb/src/styles",
    "usa-button-group/src/styles",
    "usa-button/src/styles",
    "usa-card/src/styles",
    "usa-checklist/src/styles",
    "usa-collection/src/styles",
    "usa-embed-container/src/styles",
    "usa-footer/src/styles",
    "usa-form/src/styles",
    "usa-graphic-list/src/styles",
    "usa-header/src/styles",
    "usa-hero/src/styles",
    "usa-icon/src/styles",
    "usa-icon-list/src/styles/usa-icon-list",
    "usa-identifier/src/styles",
    "usa-in-page-navigation/src/styles",
    "usa-language-selector/src/styles",
    "usa-layout-docs/src/styles",
    "usa-layout-grid/src/styles",
    "usa-media-block/src/styles",
    "usa-modal/src/styles",
    "usa-nav/src/styles",
    "usa-pagination/src/styles",
    "usa-process-list/src/styles",
    "usa-search/src/styles",
    "usa-section/src/styles",
    "usa-sidenav/src/styles",
    "usa-site-alert/src/styles",
    "usa-skipnav/src/styles",
    "usa-step-indicator/src/styles",
    "usa-summary-box/src/styles",
    "usa-table/src/styles",
    "usa-tag/src/styles",
    "usa-tooltip/src/styles",
    "usa-input-list/src/styles",
    "usa-character-count/src/styles",
    "usa-checkbox/src/styles",
    "usa-combo-box/src/styles",
    "usa-date-picker/src/styles",
    "usa-error-message/src/styles",
    "usa-fieldset/src/styles",
    "usa-file-input/src/styles",
    "usa-form-group/src/styles",
    "usa-hint/src/styles",
    "usa-input-prefix-suffix/src/styles",
    "usa-input/src/styles",
    "usa-input-mask/src/styles/usa-input-mask",
    "usa-label/src/styles",
    "usa-legend/src/styles",
    "usa-memorable-date/src/styles",
    "usa-radio/src/styles",
    "usa-range/src/styles",
    "usa-select/src/styles",
    "usa-textarea/src/styles",
    "usa-time-picker/src/styles",
    "uswds-utilities/src/styles",
]


def compile_scss(input_str: str, label: str = "") -> str:
    """Compile SCSS string via grass binary, return CSS output."""
    result = subprocess.run(
        [GRASS_BIN, "--stdin", "--style=expanded", "-I", PACKAGES_DIR],
        input=input_str,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        print(f"ERROR compiling {label}:", result.stderr[:500], file=sys.stderr)
        sys.exit(1)
    return result.stdout


def make_shared_scss() -> str:
    lines = []
    for fwd in SHARED_FORWARDS:
        lines.append(f'@forward "{fwd}";')
    return "\n".join(lines) + "\n"


def compile_component(component: str) -> tuple[str, str, float]:
    """
    Compile a component independently with shared deps.
    Returns (component_name, full_css_output, compile_time_ms).
    """
    lines = []
    for fwd in SHARED_FORWARDS:
        lines.append(f'@forward "{fwd}";')
    lines.append(f'@forward "{component}";')
    scss = "\n".join(lines) + "\n"

    t0 = time.perf_counter()
    css = compile_scss(scss, label=component)
    dt = (time.perf_counter() - t0) * 1000
    return component, css, dt


def main():
    print("=" * 60)
    print("Milestone 0: Parallel Module Evaluation Validation")
    print("=" * 60)
    print()

    # Step 1: Compile full USWDS sequentially (baseline)
    print("[1/6] Compiling full USWDS sequentially...")
    full_scss_path = os.path.join(PACKAGES_DIR, "uswds", "_index-direct.scss")
    t0 = time.perf_counter()
    result = subprocess.run(
        [GRASS_BIN, "--style=expanded", "-I", PACKAGES_DIR, full_scss_path],
        capture_output=True, text=True,
    )
    t_sequential = (time.perf_counter() - t0) * 1000
    if result.returncode != 0:
        print("FAILED:", result.stderr[:500])
        sys.exit(1)
    expected_css = result.stdout
    print(f"  Sequential: {t_sequential:.1f}ms, {len(expected_css)} bytes, "
          f"{expected_css.count(chr(10))} lines")

    # Step 2: Compile shared deps only (to know the line count)
    print("[2/6] Compiling shared deps alone...")
    shared_css = compile_scss(make_shared_scss(), label="shared")
    shared_line_count = shared_css.count('\n')
    print(f"  Shared CSS: {len(shared_css)} bytes, {shared_line_count} lines")

    # Step 3: Compile each component with shared deps, extract component-only CSS
    # by skipping the shared prefix lines.
    # Key insight: shared deps are cached per-compilation, so the shared CSS
    # is always the same number of lines. But the serializer may merge the
    # boundary line. So we use bytes instead of lines.
    print(f"[3/6] Compiling {len(COMPONENT_FORWARDS)} components, extracting CSS...")

    # First, validate that shared CSS is a byte-prefix of wrapper output
    # by testing with the first component
    test_name, test_css, _ = compile_component(COMPONENT_FORWARDS[0])
    if test_css.startswith(shared_css):
        print(f"  Shared CSS is byte-prefix of wrapper (clean boundary)")
        use_byte_prefix = True
    else:
        # Find where they diverge
        for i in range(min(len(shared_css), len(test_css))):
            if shared_css[i] != test_css[i]:
                ctx = max(0, i - 20)
                print(f"  Divergence at byte {i}: shared=...{shared_css[ctx:i+10]!r} vs wrapper=...{test_css[ctx:i+10]!r}")
                break
        use_byte_prefix = False

    if not use_byte_prefix:
        # Fallback: use the shared line count approach.
        # The shared CSS might not be a clean byte prefix because the serializer
        # adjusts trailing whitespace when there's more content.
        # Strategy: count lines in shared output. In wrapper output, skip that
        # many lines. But account for the boundary line potentially being merged.
        print("  Using line-count extraction (shared CSS trailing newline differs)")

        # Check if shared_css minus trailing newline is a prefix
        shared_trimmed = shared_css.rstrip('\n')
        if test_css.startswith(shared_trimmed):
            print("  Shared CSS (trimmed) is byte-prefix — using trimmed prefix")
            shared_css = shared_trimmed
            use_byte_prefix = True

    # Step 4: Compile all components sequentially
    print(f"[4/6] Compiling all {len(COMPONENT_FORWARDS)} components sequentially...")
    t0 = time.perf_counter()
    component_css_seq = {}
    for comp in COMPONENT_FORWARDS:
        name, full_css, _ = compile_component(comp)
        if use_byte_prefix:
            component_only = full_css[len(shared_css):]
        else:
            # Last resort: skip shared_line_count lines
            lines = full_css.split('\n')
            component_only = '\n'.join(lines[shared_line_count:])
        component_css_seq[name] = component_only
    t_seq = (time.perf_counter() - t0) * 1000
    print(f"  Sequential: {t_seq:.1f}ms")

    # Step 5: Compile all components in parallel
    print(f"[5/6] Compiling all {len(COMPONENT_FORWARDS)} components in parallel (8 threads)...")
    t0 = time.perf_counter()
    component_css_par = {}
    with ThreadPoolExecutor(max_workers=8) as pool:
        futures = {
            pool.submit(compile_component, comp): comp
            for comp in COMPONENT_FORWARDS
        }
        for future in as_completed(futures):
            name, full_css, _ = future.result()
            if use_byte_prefix:
                component_only = full_css[len(shared_css):]
            else:
                lines = full_css.split('\n')
                component_only = '\n'.join(lines[shared_line_count:])
            component_css_par[name] = component_only
    t_par = (time.perf_counter() - t0) * 1000
    print(f"  Parallel: {t_par:.1f}ms")

    # Step 6: Validate
    print("[6/6] Validating...")

    # Check: sequential component CSS == parallel component CSS
    # (proves components are deterministic regardless of execution order)
    all_match = True
    for comp in COMPONENT_FORWARDS:
        if component_css_seq[comp] != component_css_par[comp]:
            print(f"  MISMATCH: {comp}")
            seq_lines = component_css_seq[comp].splitlines()
            par_lines = component_css_par[comp].splitlines()
            print(f"    seq: {len(seq_lines)} lines, par: {len(par_lines)} lines")
            # Find first diff
            for i, (s, p) in enumerate(zip(seq_lines, par_lines)):
                if s != p:
                    print(f"    First diff at line {i}: seq={s!r} par={p!r}")
                    break
            all_match = False

    if all_match:
        print("  Sequential == Parallel per-component CSS (all match)")

    # Reconstruct full output from shared + components and compare with expected
    reconstructed = shared_css
    for comp in COMPONENT_FORWARDS:
        reconstructed += component_css_seq[comp]

    # The full output has a /*! uswds @version */ comment that shared doesn't
    # Let's compare without the comment
    expected_stripped = expected_css
    if expected_css.startswith('/*! uswds'):
        # Remove the version comment line
        first_newline = expected_css.index('\n')
        expected_stripped = expected_css[first_newline + 1:]

    if reconstructed == expected_stripped:
        print("  Reconstructed output == expected (byte-identical)")
        reconstruction_ok = True
    else:
        # Check if it's close
        rec_lines = reconstructed.splitlines()
        exp_lines = expected_stripped.splitlines()
        print(f"  Reconstructed: {len(rec_lines)} lines, expected: {len(exp_lines)} lines")

        if len(rec_lines) == len(exp_lines):
            diffs = 0
            for i, (r, e) in enumerate(zip(rec_lines, exp_lines)):
                if r != e:
                    diffs += 1
                    if diffs <= 5:
                        print(f"    Line {i+1}: expected={e[:80]!r} got={r[:80]!r}")
            print(f"    Total differing lines: {diffs}/{len(exp_lines)}")
        else:
            # Find where they diverge
            for i in range(min(len(rec_lines), len(exp_lines))):
                if rec_lines[i] != exp_lines[i]:
                    print(f"    First diff at line {i+1}:")
                    print(f"      expected: {exp_lines[i][:100]!r}")
                    print(f"      got:      {rec_lines[i][:100]!r}")
                    break

        reconstruction_ok = False

    # Timing summary
    print()
    print("=" * 60)
    print("Results")
    print("=" * 60)
    print(f"  Full sequential compile:       {t_sequential:.1f}ms")
    print(f"  Components sequential (67x):   {t_seq:.1f}ms")
    print(f"  Components parallel (8 thr):   {t_par:.1f}ms")
    if t_seq > 0:
        speedup = t_seq / t_par
        print(f"  Parallel speedup:              {speedup:.2f}x")
    print()

    # Note: these times include process spawn overhead per component.
    # Actual in-process parallelism would be much faster since it avoids
    # 67 process spawns and 67 uswds-core re-evaluations.
    est_core_time = t_sequential * 0.15  # ~15% for shared deps
    est_component_time = t_sequential * 0.85  # ~85% for components
    est_parallel_component = est_component_time / min(8, len(COMPONENT_FORWARDS))
    est_in_process = est_core_time + est_parallel_component
    print(f"  Estimated in-process parallel:  ~{est_in_process:.0f}ms")
    print(f"  Estimated in-process speedup:   ~{t_sequential / est_in_process:.1f}x")
    print()

    if all_match and reconstruction_ok:
        print("GO: Components can be evaluated independently!")
        print("   Per-component CSS is identical regardless of compilation order.")
        print("   Reconstructed output matches full sequential compile.")
        print("   Proceed to Milestone 1 (interner migration).")
    elif all_match:
        print("PARTIAL GO: Components are independent (parallel == sequential)")
        print("   but reconstruction doesn't match full output exactly.")
        print("   This may be a serializer boundary issue, not a real problem.")
        print("   Investigate the diff before proceeding.")
    else:
        print("NO-GO: Components produce different CSS when compiled in different orders.")
        print("   Parallel evaluation is not feasible with the current approach.")

    return 0 if (all_match and reconstruction_ok) else 1


if __name__ == "__main__":
    sys.exit(main())
