# Agent Instructions

This project uses **bd** (beads) for issue tracking. Run `bd onboard` to get started.

## Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work atomically
bd close <id>         # Complete work
```

## Non-Interactive Shell Commands

**ALWAYS use non-interactive flags** with file operations to avoid hanging on confirmation prompts.

Shell commands like `cp`, `mv`, and `rm` may be aliased to include `-i` (interactive) mode on some systems, causing the agent to hang indefinitely waiting for y/n input.

**Use these forms instead:**
```bash
# Force overwrite without prompting
cp -f source dest           # NOT: cp source dest
mv -f source dest           # NOT: mv source dest
rm -f file                  # NOT: rm file

# For recursive operations
rm -rf directory            # NOT: rm -r directory
cp -rf source dest          # NOT: cp -r source dest
```

**Other commands that may prompt:**
- `scp` - use `-o BatchMode=yes` for non-interactive
- `ssh` - use `-o BatchMode=yes` to fail instead of prompting
- `apt-get` - use `-y` flag
- `brew` - use `HOMEBREW_NO_AUTO_UPDATE=1` env var

<!-- BEGIN BEADS INTEGRATION -->
## Issue Tracking with bd (beads)

**IMPORTANT**: This project uses **bd (beads)** for ALL issue tracking. Do NOT use markdown TODOs, task lists, or other tracking methods.

### Why bd?

- Dependency-aware: Track blockers and relationships between issues
- Version-controlled: Built on Dolt with cell-level merge
- Agent-optimized: JSON output, ready work detection, discovered-from links
- Prevents duplicate tracking systems and confusion

### Quick Start

**Check for ready work:**

```bash
bd ready --json
```

**Create new issues:**

```bash
bd create "Issue title" --description="Detailed context" -t bug|feature|task -p 0-4 --json
bd create "Issue title" --description="What this issue is about" -p 1 --deps discovered-from:bd-123 --json
```

**Claim and update:**

```bash
bd update <id> --claim --json
bd update bd-42 --priority 1 --json
```

**Complete work:**

```bash
bd close bd-42 --reason "Completed" --json
```

### Issue Types

- `bug` - Something broken
- `feature` - New functionality
- `task` - Work item (tests, docs, refactoring)
- `epic` - Large feature with subtasks
- `chore` - Maintenance (dependencies, tooling)

### Priorities

- `0` - Critical (security, data loss, broken builds)
- `1` - High (major features, important bugs)
- `2` - Medium (default, nice-to-have)
- `3` - Low (polish, optimization)
- `4` - Backlog (future ideas)

### Workflow for AI Agents

1. **Check ready work**: `bd ready` shows unblocked issues
2. **Claim your task atomically**: `bd update <id> --claim`
3. **Work on it**: Implement, test, document
4. **Discover new work?** Create linked issue:
   - `bd create "Found bug" --description="Details about what was found" -p 1 --deps discovered-from:<parent-id>`
5. **Complete**: `bd close <id> --reason "Done"`

### Auto-Sync

bd automatically syncs with git:

- Exports to `.beads/issues.jsonl` after changes (5s debounce)
- Imports from JSONL when newer (e.g., after `git pull`)
- No manual export/import needed!

### Important Rules

- ✅ Use bd for ALL task tracking
- ✅ Always use `--json` flag for programmatic use
- ✅ Link discovered work with `discovered-from` dependencies
- ✅ Check `bd ready` before asking "what should I work on?"
- ❌ Do NOT create markdown TODO lists
- ❌ Do NOT use external issue trackers
- ❌ Do NOT duplicate tracking systems

For more details, see README.md and docs/QUICKSTART.md.

## Landing the Plane (Session Completion)

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `git push` succeeds.

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** - Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **PUSH TO REMOTE** - This is MANDATORY:
   ```bash
   git pull --rebase
   git push
   git status  # MUST show "up to date with origin"
   ```
5. **Clean up** - Clear stashes, prune remote branches
6. **Verify** - All changes committed AND pushed
7. **Hand off** - Provide context for next session

**CRITICAL RULES:**
- Work is NOT complete until `git push` succeeds
- NEVER stop before pushing - that leaves work stranded locally
- NEVER say "ready to push when you are" - YOU must push
- If push fails, resolve and retry until it succeeds

## Issue Tracking Discipline — MANDATORY

**🚨 ALWAYS update issues. This is NOT optional. Every task must have a corresponding issue update.**

Updating issues is a **blocking requirement** — not a cleanup step you do at the end. If you wrote code, you MUST update issues before responding to the user. No exceptions.

### At every step:

- **Before starting work**: Claim an issue (`bd update <id> --claim`) or create one if none exists. Never write code without a tracked issue.
- **With every commit**: Update the relevant issue's notes with what was done, what remains, and current test counts.
- **After fixing tests**: Update the issue **description** (not just notes) to reflect the new failure count. Run sass-spec to get accurate numbers.
- **When partially completing an issue**: Add notes summarizing what's fixed, what's still broken, and what blocks the rest — so the next session can pick up without re-exploring.
- **When closing**: Include a close reason summarizing total impact (e.g., "Fixed 35 sass-spec tests: ...").
- **When discovering new work**: Create linked issues immediately (`bd create ... --deps discovered-from:<id>`).

### At session boundaries:

- **Session start**: Run `bd list --status=open` and verify issue descriptions still reflect reality. Update counts and priorities if they've drifted.
- **Session end**: All issues touched during the session must have current, accurate descriptions and notes before pushing.

### What "update" means:

An issue update is NOT just adding notes. It means the issue's **title, description, and status** accurately reflect the current state of the world. If an issue says "~1,259 failures" and you fixed 135 of them, the title and description must be updated to say "~1,124 failures".

<!-- END BEADS INTEGRATION -->

# grass - Sass compiler in Rust

## Build & Test
- `cargo build --release` - release build
- `cargo clippy --features=macro -- -D warnings` - lint check
- `cargo test --features=macro` - run all tests
- Rust MSRV: see rust-version in crates/*/Cargo.toml

## Testing Strategy
- **Iterate with sass-spec first** when working on features targeting spec compliance. sass-spec tests run much faster than `cargo test` and are the source of truth for correctness.
- **Run `cargo test --features=macro` as a final gate** before committing to catch regressions across the full test suite.

## Development Workflow

### Adding a New Feature or Fixing a Bug

1. **Check if dart-sass has it** — look at [dart-sass source](https://github.com/sass/dart-sass) to understand expected behavior
2. **Search the sass-spec test suite** — before writing code, search `sass-spec/spec/` for related test files (see below)
3. **Add tests first** — put test cases in the appropriate `crates/lib/tests/*.rs` file using the `test!` macro
4. **Implement** — most changes are in `crates/compiler/src/` (parser, evaluator, or builtins)
5. **Run sass-spec tests** to verify correctness, then `cargo test` as a final gate

### Searching the sass-spec Test Suite

**This is a required step for all feature work and bug fixes.** Before writing code, search `sass-spec/spec/` for tests related to whatever you're working on. Search broadly, not just for exact feature names.

For example, when working on `@extend`:
- Search for `extend` in test directory and file names
- Search for error messages like `"can't extend"` to find validation tests
- Check for edge cases like nested selectors, media boundaries, chained extends

**Why this matters:** grass aims to match dart-sass behavior exactly. If sass-spec has a test for it, we should pass it. Missing this step has caused silent behavioral differences.

When you find relevant sass-spec tests, use them to guide implementation and add equivalent unit tests in `crates/lib/tests/`:
```rust
// Based on sass-spec: spec/css/if/sass.hrx
test!(css_if_sass_true, "a {b: if(sass(true): c; else: d)}", "a {\n  b: c;\n}\n");
```

**Search locally with `find` and `grep`:**
```bash
# Find test files by topic
find sass-spec/spec -name "*.hrx" | grep -i "extend"

# Search test content for specific behavior
grep -r "error" sass-spec/spec/css/if/ --include="*.hrx" -l
```

**Search the dart-sass repo with `gh` for implementation details and tests:**
```bash
# Search dart-sass source for how a feature is implemented
gh search code "visitIfExpression" --repo sass/dart-sass --limit 10

# Search for test cases related to a feature
gh search code "if()" --repo sass/sass-spec --filename "*.hrx" --limit 20

# Search for specific error messages
gh search code "may not contain" --repo sass/dart-sass --limit 10
```

### Running Tests

**Iterate with sass-spec first, cargo test last.** The full `cargo test` suite is slow to start up. When working on spec compliance, test against sass-spec directly using the release binary, then run `cargo test` as a final regression gate before committing.

```bash
# Build release binary for sass-spec testing
~/.cargo/bin/cargo build --release

# Test against sass-spec with the binary
echo "a { b: c }" | ./target/release/grass --stdin --style=expanded

# Run full test suite (final gate before committing)
~/.cargo/bin/cargo test --features=macro

# Run a single test file
~/.cargo/bin/cargo test --features=macro --test css_if
```

### Verifying Test Expectations

**NEVER change a test expectation based on reasoning alone.** Always verify against dart-sass before changing what a test expects:

```bash
# Check expected output for any Sass input (use the version matching our target)
echo 'a { color: rgb(1.5, 1.5, 1.5); }' | npx sass@1.97.3 --stdin --style=expanded
```

When modifying test expectations:
1. Run the input through dart-sass to get ground truth
2. Use that exact output as the expected value
3. Note in the commit message that expectations were verified against dart-sass

## Project Structure
- `crates/compiler/` - core compiler (grass_compiler crate)
- `crates/lib/` - public library + CLI binary (grass crate)
- `crates/lib/pkg-publish/` - npm package (WASM + napi-rs fallback)
- `crates/napi/` - napi-rs native Node.js addon (grass_napi crate)
- `crates/include_sass/` - proc macro crate
- `crates/lib/tests/` - integration tests organized by feature
- `prototype/` - USWDS test project, benchmarks, perf baseline
- `sass-spec/` - git submodule of the official Sass spec tests

## Workflow
- Commit at logical intervals — each fix, feature, or refactor should be its own commit
- Run `cargo test --features=macro` before every commit to ensure nothing is broken
- Use `~/.cargo/bin/cargo` if `cargo` is not on PATH

### Pre-Commit Performance Check

**Before every commit that touches compiler code** (`crates/compiler/`), run the performance check:

```bash
# Build release binary first
~/.cargo/bin/cargo build --release

# Run perf check (compiles USWDS 3x, reports median, compares to baseline)
cd prototype && ./perf-check.sh
```

This compiles USWDS with the release binary and compares against the saved baseline in `prototype/.perf-baseline`. If performance regresses by >5%, investigate before committing.

To update the baseline after intentional changes:
```bash
echo "<new_median_ms>" > prototype/.perf-baseline
```

For a full cross-engine benchmark (native vs WASM vs sass-embedded):
```bash
cd prototype && node bench.js 2>/dev/null
```

## Session Discipline

### Time-box investigations: 15 minutes max
If a fix isn't converging after 15 minutes, **stop**. Commit what works, file the rest as a beads issue, and move on. A 100-minute commit should have been 3-4 separate commits.

### Smaller commits, more often
Commit each independent fix immediately. Don't bundle unrelated fixes into one commit. If you're fixing NaN handling AND adjust/change semantics AND none keyword support, those are 3 commits.

### Abandon and document, don't revert silently
When an approach doesn't work (e.g., you try a fix, discover it cascades, and revert), **file a beads issue** with what you learned so the next session doesn't repeat the exploration. Use `bd create --title="..." --description="Attempted X, failed because Y. Approach Z might work." -t task -p 3`.

### Batch edits before building
Make all obvious/mechanical edits before the first `cargo build`. If you're adding the same validation to 16 functions, edit all 16 files first, then build once. Don't build-edit-build-edit sequentially.

### sass-spec test runner
The sass-spec color test runner lives at `prototype/run-color-specs.py` (NOT `/tmp/`). It survives context compaction.

```bash
# Run all color tests (~38s)
python3 prototype/run-color-specs.py

# Run a specific category
python3 prototype/run-color-specs.py hwb

# Show failures
python3 prototype/run-color-specs.py --failures --limit 20
```

### Known blocked test categories (skip list)
These test failures are blocked by unimplemented features. Don't investigate them:
- **calc()/var()/attr() passthrough** (~170 tests) — requires expression passthrough in color functions
- **relative_color** (~60 tests) — `color(from ...)` syntax not yet implemented
- **deprecation warnings** (~77 tests) — requires deprecation warning infrastructure
- **pre-existing**: `module_functions_builtin` test failure — unrelated to color work

## Conventions
- Tests use a `test!` macro comparing Sass input to expected CSS output
- `#[ignore = "reason"]` marks known-failing tests with explanation
- Targets feature parity with dart-sass reference implementation
- Error message and span differences from dart-sass are acceptable