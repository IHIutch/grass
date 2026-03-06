# Agent Instructions

This project uses **bd** (beads) for issue tracking. Run `bd onboard` to get started.

## Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work atomically
bd close <id>         # Complete work
bd sync               # Sync with git
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
   bd sync
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

<!-- END BEADS INTEGRATION -->

# grass - Sass compiler in Rust

## Build & Test
- `cargo test --features=macro` - run all tests
- `cargo clippy --features=macro -- -D warnings` - lint check
- `cargo build --release` - release build
- Rust MSRV: see rust-version in crates/*/Cargo.toml

## Project Structure
- `crates/compiler/` - core compiler (grass_compiler crate)
- `crates/lib/` - public library + CLI binary (grass crate)
- `crates/include_sass/` - proc macro crate
- `crates/lib/tests/` - integration tests organized by feature
- `sass-spec/` - git submodule of the official Sass spec tests

## Workflow
- Commit at logical intervals — each fix, feature, or refactor should be its own commit
- Run `cargo test --features=macro` before every commit to ensure nothing is broken
- Use `~/.cargo/bin/cargo` if `cargo` is not on PATH

## Conventions
- Tests use a `test!` macro comparing Sass input to expected CSS output
- `#[ignore = "reason"]` marks known-failing tests with explanation
- Targets feature parity with dart-sass reference implementation
- Error message and span differences from dart-sass are acceptable

## Key Architecture Notes
- CSS tree (`css_tree.rs`): During evaluation, CssStmt bodies are always empty — children are tracked in the `parent_to_child` BTreeMap and only assembled in `finish()`. Use `parent_to_child` (not `is_invisible()` on CssStmt) to check visibility during tree building.
- `has_following_sibling()` in css_tree.rs controls whether parent blocks (@media, etc.) get duplicated when child rules are added — it affects splitting, not just newlines.
- ExtensionStore (`selector/extend/mod.rs`): `media_contexts` map is keyed by SelectorList (structural equality), which causes collisions when different selectors have the same structure. This is a known architectural issue blocking the `extend_across_media_boundary` fix.
- `@extend` ordering: Extensions are processed as they're encountered during evaluation. Transitive extends (A extends B extends C) require special handling that is not yet implemented (`extend_extender` test).
- Unknown at-rules: `@keyframes` is stored as `CssStmt::UnknownAtRule`, not a dedicated variant. Check `unvendor(&name) == "keyframes"` to distinguish.
