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
//! ## Dependency tracking: directories, not precise files
//!
//! dart-sass's own watcher has full access to its import graph and watches
//! exactly the files a compile actually loaded. grass's public API doesn't
//! expose that: `SourceMapData::sources` (the one loaded-file list
//! reachable from crates/lib without touching crates/compiler, which is out
//! of this feature's territory) only lists files that contributed at least
//! one *emitted CSS mapping* -- a `@use`d partial containing only variables,
//! mixins, or functions (an extremely common pattern, e.g. `_variables.scss`)
//! never appears in it at all, so watching just that list would silently
//! miss most real dependency edits. (Confirmed empirically: a `@use`d
//! variable-only partial's edits were never observed by a `sources`-based
//! watch set in manual testing.)
//!
//! So instead, this watches, *recursively*, every directory that could
//! plausibly contain a dependency: the entry file's own directory plus every
//! `-I`/`--load-path` directory. Events are filtered to paths with a
//! `.scss`/`.sass` extension before triggering a recompile -- both to cut
//! noise (editor swap files, the `.css`/`.css.map` output this same process
//! is writing into that same directory) and, importantly, to avoid a
//! self-feedback loop when OUTPUT lives alongside INPUT (a very common
//! layout): without the extension filter, this process's own writes to the
//! `.css`/`.css.map` files would re-trigger themselves indefinitely.
//!
//! The tradeoff: an unrelated `.scss`/`.sass` file elsewhere in a watched
//! directory tree (not actually `@use`d by the compile) also triggers a
//! recompile. That's the "minimum viable parity" call made here -- precise,
//! zero-false-positive tracking would need crates/compiler to expose its
//! full import graph (an `Options`/`Visitor` change), which is out of this
//! feature's territory; see the final report on todo #226/#227 for a
//! follow-up suggestion.
//!
//! Timestamps are UTC (`[YYYY-MM-DD HH:MM]`), not dart's local wall-clock
//! time -- matching the host's local timezone would need a chrono/time-crate
//! dependency beyond the `notify` this feature was scoped to add.

use std::{
    collections::HashSet,
    io::{self, Write},
    path::PathBuf,
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

pub(crate) fn run(args: WatchArgs) -> io::Result<()> {
    let (tx, rx) = channel::<notify::Result<Event>>();

    let mut watcher: Box<dyn Watcher> = if args.poll {
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

    let mut roots: HashSet<PathBuf> = HashSet::new();
    if let Some(dir) = crate::absolute_source_path(args.input, &cwd).parent() {
        roots.insert(dir.to_path_buf());
    }
    for load_path in args.load_paths {
        roots.insert(crate::absolute_source_path(&load_path.to_string_lossy(), &cwd));
    }
    for root in &roots {
        watcher.watch(root, RecursiveMode::Recursive).map_err(notify_to_io_err)?;
    }

    compile_and_announce(&args)?;
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

        compile_and_announce(&args)?;
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

fn compile_and_announce(args: &WatchArgs) -> io::Result<()> {
    let compile_result = from_path_with_source_map(args.input, args.options);

    if write_compile_result(compile_result, &args.write_config)? {
        println!("{} Compiled {} to {}.", timestamp::now_utc_minute(), args.input, args.output);
    }
    // On failure, the error was already printed to stderr by
    // `write_compile_result`; watch mode just keeps going.

    io::stdout().flush()?;
    Ok(())
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
