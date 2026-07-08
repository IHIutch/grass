//! `--watch`/`--poll`: recompiles a single INPUT->OUTPUT pair whenever a
//! `.scss`/`.sass` file changes, matching `npx sass@1.97.3 --watch`'s
//! message format and lifecycle (see the probe transcripts on solo todo
//! #227 and the tests in crates/lib/tests/cli.rs):
//!
//! - an initial compile, then a `Sass is watching for changes. Press Ctrl-C
//!   to stop.` banner (printed exactly once, after the first compile
//!   attempt, success or failure);
//! - each subsequent compile (triggered by a relevant file change) only
//!   prints its own `[timestamp] Compiled <input> to <output>.` line (or,
//!   on failure, the error -- already handled by `write_compile_result`,
//!   which also does the error-CSS overwrite/delete) and keeps watching;
//! - Ctrl-C (SIGINT) is left to the default OS disposition (exit 130,
//!   verified via npx) -- no signal handler is installed.
//!
//! ## Dependency tracking: per-loaded-file directories (todo #274)
//!
//! `SourceMapData::loaded_files` (crates/compiler, wired up for exactly this
//! use case on todo #274) lists every file the most recent compile actually
//! loaded via `@use`/`@forward`/`@import` -- including `@use`d partials that
//! contain only variables/mixins/functions and never contribute an emitted
//! CSS mapping (unlike `SourceMapData::sources`, which is scoped to mapping
//! emission and silently misses exactly that case; confirmed empirically
//! during #227). `main.rs` forces `Options::source_map(true)` whenever
//! `--watch` is passed, independent of `--source-map`, purely so this list
//! is always populated; whether a `.map` is actually written to disk still
//! depends only on `--source-map`.
//!
//! After every compile (initial and every recompile), the watch set is
//! rebuilt from `loaded_files`: each loaded file's *parent directory* is
//! watched non-recursively (watching directories rather than individual
//! file paths survives editors' atomic-save patterns -- temp-file-then-
//! rename replaces the original inode, which a direct per-file watch can
//! miss). This is still directory-level, not a precise per-file diff --
//! an unrelated `.scss`/`.sass` file that happens to sit in the *same*
//! directory as a real dependency still triggers a recompile -- but it's
//! scoped to only the directories of files actually loaded, rather than the
//! entire entry-file directory tree.
//!
//! Every `-I`/`--load-path` directory stays watched recursively for the
//! whole session, as a fallback for files that might *start* mattering (e.g.
//! a new partial created to satisfy a currently-failing `@use`). And if a
//! compile fails (or, defensively, if `loaded_files` comes back empty --
//! `loaded_files` unavailable or partial), the entry file's own directory
//! falls back to a recursive watch until a compile succeeds again, so a
//! broken state still recovers regardless of which file the fix lands in.
//!
//! Events are filtered to paths with a `.scss`/`.sass` extension before
//! triggering a recompile -- both to cut noise (editor swap files, the
//! `.css`/`.css.map` output this same process is writing into that same
//! directory) and, importantly, to avoid a self-feedback loop when OUTPUT
//! lives alongside INPUT (a very common layout): without the extension
//! filter, this process's own writes to the `.css`/`.css.map` files would
//! re-trigger themselves indefinitely.
//!
//! Timestamps are UTC (`[YYYY-MM-DD HH:MM]`), not dart's local wall-clock
//! time -- matching the host's local timezone would need a chrono/time-crate
//! dependency beyond the `notify` this feature was scoped to add.

use std::{
    collections::HashSet,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::mpsc::{channel, RecvTimeoutError},
    time::Duration,
};

use notify::{Config as NotifyConfig, Event, EventKind, PollWatcher, RecommendedWatcher, RecursiveMode, Watcher};

use grass::{from_path_with_source_map, Options};

use crate::{write_compile_result, WriteConfig};

pub(crate) struct WatchArgs<'a> {
    pub(crate) input: &'a str,
    pub(crate) output: &'a str,
    pub(crate) options: &'a Options<'a>,
    pub(crate) write_config: WriteConfig<'a>,
    pub(crate) load_paths: &'a [&'a std::path::Path],
    pub(crate) poll: bool,
}

/// Tracks the directories currently watched for a single `--watch` session
/// and rebuilds that set after every compile from the compile's
/// `loaded_files` (see the module doc comment). `-I`/`--load-path`
/// directories are watched recursively for the whole session and never
/// change; everything else is adjusted incrementally so a long-running
/// session doesn't repeatedly watch/unwatch directories that didn't change
/// between compiles.
struct DepWatcher {
    watcher: Box<dyn Watcher>,
    entry_dir: Option<PathBuf>,
    load_path_roots: HashSet<PathBuf>,
    /// Non-recursively watched directories derived from the last compile's
    /// `loaded_files`, excluding anything already covered by
    /// `load_path_roots`.
    precise_dirs: HashSet<PathBuf>,
    /// Whether `entry_dir` currently has the failure-fallback recursive
    /// watch installed (see the module doc comment).
    fallback_active: bool,
}

impl DepWatcher {
    fn new(watcher: Box<dyn Watcher>, entry_dir: Option<PathBuf>, load_paths: &[&Path], cwd: &Path) -> io::Result<Self> {
        let mut this = Self {
            watcher,
            entry_dir,
            load_path_roots: HashSet::new(),
            precise_dirs: HashSet::new(),
            fallback_active: false,
        };

        for load_path in load_paths {
            this.load_path_roots
                .insert(crate::absolute_source_path(&load_path.to_string_lossy(), cwd));
        }
        for root in &this.load_path_roots {
            this.watcher.watch(root, RecursiveMode::Recursive).map_err(notify_to_io_err)?;
        }

        // No compile has run yet, so there's no `loaded_files` to be
        // precise about -- start in fallback mode, matching the pre-#274
        // behavior for the very first compile.
        if let Some(dir) = &this.entry_dir {
            this.watcher.watch(dir, RecursiveMode::Recursive).map_err(notify_to_io_err)?;
            this.fallback_active = true;
        }

        Ok(this)
    }

    /// `loaded_files` is `Some` (even if empty) exactly when the most recent
    /// compile succeeded and returned `SourceMapData`; `None` on a failed
    /// compile.
    fn update(&mut self, loaded_files: Option<Vec<PathBuf>>) {
        let Some(files) = loaded_files.filter(|f| !f.is_empty()) else {
            // Compile failed, or (defensively) reported no loaded files at
            // all -- neither should leave us with less coverage than we
            // already had, so keep whatever was watched before.
            if let Some(dir) = self.entry_dir.clone() {
                if !self.fallback_active && self.watcher.watch(&dir, RecursiveMode::Recursive).is_ok() {
                    self.fallback_active = true;
                }
            }
            return;
        };

        // Drop the failure-fallback watch on `entry_dir` *before* applying
        // the precise diff below -- `entry_dir` is very likely also one of
        // `dirs` (the entry file itself is always in `loaded_files`), and
        // re-watching a path non-recursively while it's still registered
        // recursively, then unwatching, would cancel the new watch instead
        // of the stale recursive one (`notify` keys a path's watch by path,
        // not by registration order).
        if self.fallback_active {
            if let Some(dir) = &self.entry_dir {
                let _ = self.watcher.unwatch(dir);
            }
            self.fallback_active = false;
        }

        let dirs: HashSet<PathBuf> = files
            .iter()
            .filter_map(|f| f.parent())
            .map(Path::to_path_buf)
            .filter(|d| !self.load_path_roots.contains(d))
            .collect();

        for dir in self.precise_dirs.difference(&dirs) {
            let _ = self.watcher.unwatch(dir);
        }
        for dir in dirs.difference(&self.precise_dirs) {
            let _ = self.watcher.watch(dir, RecursiveMode::NonRecursive);
        }
        self.precise_dirs = dirs;
    }
}

pub(crate) fn run(args: WatchArgs) -> io::Result<()> {
    let (tx, rx) = channel::<notify::Result<Event>>();

    let watcher: Box<dyn Watcher> = if args.poll {
        Box::new(
            PollWatcher::new(
                move |res| {
                    let _ = tx.send(res);
                },
                NotifyConfig::default().with_poll_interval(Duration::from_millis(1000)),
            )
            .map_err(notify_to_io_err)?,
        )
    } else {
        Box::new(
            RecommendedWatcher::new(
                move |res| {
                    let _ = tx.send(res);
                },
                NotifyConfig::default(),
            )
            .map_err(notify_to_io_err)?,
        )
    };

    let cwd = std::env::current_dir().unwrap_or_default();
    let entry_dir = crate::absolute_source_path(args.input, &cwd)
        .parent()
        .map(Path::to_path_buf);

    let mut dep_watcher = DepWatcher::new(watcher, entry_dir, args.load_paths, &cwd)?;

    let loaded_files = compile_and_announce(&args)?;
    dep_watcher.update(loaded_files);
    // Printed exactly once, right after the first compile attempt --
    // verified via npx sass@1.97.3 (present even when the initial compile
    // fails).
    println!("Sass is watching for changes. Press Ctrl-C to stop.");
    println!();
    io::stdout().flush()?;

    loop {
        let first = match rx.recv() {
            Ok(evt) => evt,
            // The watcher's sender was dropped (platform watcher thread
            // died) -- nothing left to watch.
            Err(_) => return Ok(()),
        };

        let mut changed = event_is_relevant(&first);
        // Coalesce a short burst of events from a single save (e.g. editors
        // that write a temp file then rename it over the target, which
        // shows up as separate Remove/Create events) into one recompile.
        loop {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(evt) => changed |= event_is_relevant(&evt),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            }
        }

        if !changed {
            continue;
        }

        let loaded_files = compile_and_announce(&args)?;
        dep_watcher.update(loaded_files);
    }
}

/// A Sass source file was plausibly modified/created/removed -- see the
/// module doc comment for why this filters on extension rather than a
/// precise dependency set, and why that filter also matters for avoiding a
/// self-feedback loop on this process's own output.
fn event_is_relevant(evt: &notify::Result<Event>) -> bool {
    let Ok(evt) = evt else {
        return false;
    };
    if !matches!(evt.kind, EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)) {
        return false;
    }
    evt.paths
        .iter()
        .any(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("scss") | Some("sass")))
}

/// Returns `Some(loaded_files)` when the compile succeeded and produced
/// `SourceMapData` (guaranteed in `--watch` -- see the module doc comment),
/// `None` on a failed compile. Consumed by `DepWatcher::update` to rebuild
/// the watch set.
fn compile_and_announce(args: &WatchArgs) -> io::Result<Option<Vec<PathBuf>>> {
    let compile_result = from_path_with_source_map(args.input, args.options);
    let loaded_files = match &compile_result {
        Ok((_, Some(map))) => Some(map.loaded_files.clone()),
        Ok((_, None)) | Err(_) => None,
    };

    if write_compile_result(compile_result, &args.write_config)? {
        println!("{} Compiled {} to {}.", timestamp::now_utc_minute(), args.input, args.output);
    }
    // On failure, the error was already printed to stderr by
    // `write_compile_result`; watch mode just keeps going.

    io::stdout().flush()?;
    Ok(loaded_files)
}

fn notify_to_io_err(e: notify::Error) -> io::Error {
    io::Error::other(e.to_string())
}

mod timestamp {
    /// `[YYYY-MM-DD HH:MM]` in UTC, matching dart-sass's watch-message
    /// timestamp format (minute precision, bracketed) but not its timezone
    /// (local wall-clock) -- see the module doc comment.
    pub(super) fn now_utc_minute() -> String {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let total_secs = now.as_secs() as i64;
        let days = total_secs.div_euclid(86400);
        let secs_of_day = total_secs.rem_euclid(86400);
        let (y, m, d) = civil_from_days(days);
        let hh = secs_of_day / 3600;
        let mm = (secs_of_day % 3600) / 60;
        format!("[{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}]")
    }

    /// Howard Hinnant's `civil_from_days` (public domain):
    /// <http://howardhinnant.github.io/date_algorithms.html>
    fn civil_from_days(z: i64) -> (i64, u32, u32) {
        let z = z + 719_468;
        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
        let doe = (z - era * 146_097) as u64;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe as i64 + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        let y = if m <= 2 { y + 1 } else { y };
        (y, m, d)
    }

    #[cfg(test)]
    mod test {
        use super::civil_from_days;

        #[test]
        fn known_date() {
            // 20642 days after 1970-01-01 is 2026-07-08 (verified via
            // Python: `(date(2026, 7, 8) - date(1970, 1, 1)).days`).
            assert_eq!(civil_from_days(20642), (2026, 7, 8));
        }

        #[test]
        fn epoch() {
            assert_eq!(civil_from_days(0), (1970, 1, 1));
        }
    }
}
