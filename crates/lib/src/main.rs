#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

mod error_css;
mod watch;

use std::{
    fs::OpenOptions,
    io::{stdin, stdout, Read, Write},
    path::{Path, PathBuf},
};

use clap::{builder::PossibleValue, parser::ValueSource, value_parser, Arg, ArgAction, Command, ValueEnum};

use grass::{from_path_with_source_map, from_string_with_source_map, Deprecation, Options, OutputStyle, SourceMapData};

#[derive(Eq, PartialEq, Debug, Clone, Copy)]
pub enum Style {
    Expanded,
    Compressed,
}

impl ValueEnum for Style {
    fn value_variants<'a>() -> &'a [Self] {
        &[Self::Expanded, Self::Compressed]
    }

    fn to_possible_value(&self) -> Option<PossibleValue> {
        Some(match self {
            Self::Expanded => PossibleValue::new("expanded"),
            Self::Compressed => PossibleValue::new("compressed"),
        })
    }
}

#[derive(Eq, PartialEq, Debug, Clone)]
pub enum SourceMapUrls {
    Relative,
    Absolute,
}

impl ValueEnum for SourceMapUrls {
    fn value_variants<'a>() -> &'a [Self] {
        &[Self::Relative, Self::Absolute]
    }

    fn to_possible_value(&self) -> Option<PossibleValue> {
        Some(match self {
            Self::Relative => PossibleValue::new("relative"),
            Self::Absolute => PossibleValue::new("absolute"),
        })
    }
}

fn cli() -> Command {
    Command::new("grass")
        .version(env!("CARGO_PKG_VERSION"))
        .about("A Sass compiler written purely in Rust")
        .disable_version_flag(true)
        .arg(
            Arg::new("version")
                .action(ArgAction::Version)
                .long("version")
                .short('v')
                .global(true)
        )
        .arg(
            Arg::new("STDIN")
                .action(ArgAction::SetTrue)
                .long("stdin")
                .help("Read the stylesheet from stdin"),
        )
        .arg(
            Arg::new("INDENTED")
                .long("indented")
                .hide(true)
                .help("Use the indented syntax for input from stdin"),
        )
        .arg(
            Arg::new("LOAD_PATH")
                .short('I')
                .long("load-path")
                .help("A path to use when resolving imports. May be passed multiple times.")
                .action(ArgAction::Append)
                .value_parser(value_parser!(String))
                .num_args(1)
        )
        .arg(
            Arg::new("STYLE")
                // this is required for compatibility with ruby sass
                .short_alias('t')
                .short('s')
                .long("style")
                .help("Minified or expanded output")
                .default_value("expanded")
                .ignore_case(true)
                .num_args(1)
                .value_parser(value_parser!(Style)),
        )
        .arg(
            Arg::new("NO_CHARSET")
                .action(ArgAction::SetTrue)
                .long("no-charset")
                .help("Don't emit a @charset or BOM for CSS with non-ASCII characters."),
        )
        .arg(
            Arg::new("UPDATE")
                .long("update")
                .hide(true)
                .help("Only compile out-of-date stylesheets."),
        )
        .arg(
            Arg::new("ERROR_CSS")
                .long("error-css")
                .action(ArgAction::SetTrue)
                .overrides_with("NO_ERROR_CSS")
                .help(
                    "When an error occurs, emit a stylesheet describing it. \
                     Defaults to true when compiling to a file.",
                ),
        )
        .arg(
            Arg::new("NO_ERROR_CSS")
                .long("no-error-css")
                .action(ArgAction::SetTrue)
                .overrides_with("ERROR_CSS")
                .help("When an error occurs, don't emit a stylesheet describing it."),
        )
        // Source maps
        .arg(
            Arg::new("NO_SOURCE_MAP")
                .action(ArgAction::SetTrue)
                .long("no-source-map")
                .help("Whether to generate source maps. Defaults to on when writing to a file."),
        )
        .arg(
            Arg::new("SOURCE_MAP_URLS")
                .long("source-map-urls")
                .help("How to link from source maps to source files.")
                .default_value("relative")
                .ignore_case(true)
                .num_args(1)
                .value_parser(value_parser!(SourceMapUrls)),
        )
        .arg(
            Arg::new("EMBED_SOURCES")
                .action(ArgAction::SetTrue)
                .long("embed-sources")
                .help("Embed source file contents in source maps."),
        )
        .arg(
            Arg::new("EMBED_SOURCE_MAP")
                .action(ArgAction::SetTrue)
                .long("embed-source-map")
                .help("Embed source map contents in CSS."),
        )
        // Other
        .arg(
            Arg::new("WATCH")
                .short('w')
                .long("watch")
                .action(ArgAction::SetTrue)
                .help("Watch stylesheets and recompile when they change."),
        )
        .arg(
            Arg::new("POLL")
                .long("poll")
                .action(ArgAction::SetTrue)
                .help("Manually check for changes rather than using a native watcher. Only valid with --watch.")
                .requires("WATCH"),
        )
        .arg(
            Arg::new("NO_STOP_ON_ERROR")
                .long("no-stop-on-error")
                .hide(true)
                .help("Continue to compile more files after error is encountered.")
        )
        .arg(
            Arg::new("INTERACTIVE")
                .short('i')
                .long("interactive")
                .hide(true)
                .help("Run an interactive SassScript shell.")
        )
        .arg(
            Arg::new("NO_COLOR")
                .short('c')
                .action(ArgAction::SetTrue)
                .long("no-color")
                .hide(true)
                .help("Whether to use terminal colors for messages.")
        )
        .arg(
            Arg::new("VERBOSE")
                .action(ArgAction::SetTrue)
                .long("verbose")
                .hide(true)
                .help("Print all deprecation warnings even when they're repetitive.")
        )
        .arg(
            Arg::new("NO_UNICODE")
                .action(ArgAction::SetTrue)
                .long("no-unicode")
                .help("Whether to use Unicode characters for messages.")
        )
        .arg(
            Arg::new("QUIET")
                .action(ArgAction::SetTrue)
                .short('q')
                .long("quiet")
                .help("Don't print warnings."),
        )
        .arg(
            Arg::new("FATAL_DEPRECATION")
                .long("fatal-deprecation")
                .help(
                    "Deprecations to treat as errors. Repeatable and/or comma-separated. \
                     You may also pass a Sass version to include any behavior deprecated \
                     in or before it.",
                )
                .action(ArgAction::Append)
                .value_delimiter(',')
                .num_args(1)
                .value_parser(value_parser!(String)),
        )
        .arg(
            Arg::new("SILENCE_DEPRECATION")
                .long("silence-deprecation")
                .help("Deprecations to ignore. Repeatable and/or comma-separated.")
                .action(ArgAction::Append)
                .value_delimiter(',')
                .num_args(1)
                .value_parser(value_parser!(String)),
        )
        .arg(
            Arg::new("FUTURE_DEPRECATION")
                .long("future-deprecation")
                .help("Opt in to a deprecation early. Repeatable and/or comma-separated.")
                .action(ArgAction::Append)
                .value_delimiter(',')
                .num_args(1)
                .value_parser(value_parser!(String)),
        )
        .arg(
            Arg::new("INPUT")
                .value_parser(value_parser!(String))
                .required_unless_present("STDIN")
                .help("Sass files"),
        )
        .arg(
            Arg::new("OUTPUT")
                .help("Output CSS file")
        )

        // Hidden, legacy arguments
        .arg(
            Arg::new("PRECISION")
                .long("precision")
                .hide(true)
                .num_args(1)
        )
}

// Ground truth verified with dart-sass 1.97.3 (npx sass@1.97.3):
//   --silence-deprecation/--fatal-deprecation/--future-deprecation are all
//   `addMultiOption`s, so both `--flag=a,b` and repeated `--flag=a --flag=b`
//   work and compose.
//   An unrecognized ID is a hard failure before compilation begins:
//     echo "a{b:c}" | npx sass --stdin --silence-deprecation=bogus-id
//     -> "Invalid deprecation "bogus-id"." + usage text, exit 64
//   grass follows its own existing convention of exit 1 for CLI-level
//   failures (see `error_exit_code` in tests/cli.rs) rather than dart's 64,
//   but matches the "hard failure, no compilation" behavior and message text.
//
//   --fatal-deprecation additionally accepts a Dart Sass version (e.g.
//   `1.23.0`) and fatalizes every deprecation introduced at or before it
//   (lib/src/executable/options.dart:576-608, via `Deprecation.forVersion`).
//   The boundary is inclusive (verified: `--fatal-deprecation=1.33.0`
//   fatalizes `slash-div`, introduced in exactly 1.33.0; `1.32.9` does not).
//   Malformed versions (wrong part count, e.g. `1.2` or `1.2.3.4`) are
//   rejected the same as any unrecognized ID (`Invalid deprecation "…".`,
//   exit 64 in dart-sass) — verified via npx.
fn looks_like_version(s: &str) -> bool {
    let core = s.split(['-', '+']).next().unwrap_or(s);
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// Parses the `major.minor.patch` core of a version string already
/// confirmed by `looks_like_version` to have the right shape. Any
/// `-prerelease`/`+build` suffix is ignored (dart-sass's `Deprecation`
/// table has no variant introduced with such a suffix, so this can't affect
/// the range-expansion boundary).
fn parse_version_core(s: &str) -> Option<(u16, u16, u16)> {
    let core = s.split(['-', '+']).next().unwrap_or(s);
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}

/// Parses a repeatable/comma-separated `--*-deprecation` flag's values into
/// `Deprecation`s, exiting with dart-sass's "Invalid deprecation" message on
/// an unrecognized ID. For `--fatal-deprecation` (`allow_version`), a
/// version-shaped value expands to every deprecation introduced at or before
/// it (dart-sass's `Deprecation.forVersion`).
fn parse_deprecations(
    matches: &clap::ArgMatches,
    arg_id: &str,
    allow_version: bool,
) -> Vec<Deprecation> {
    let Some(values) = matches.get_many::<String>(arg_id) else {
        return Vec::new();
    };

    let mut deprecations = Vec::new();
    for id in values {
        if let Some(deprecation) = Deprecation::from_id(id) {
            deprecations.push(deprecation);
        } else if allow_version && looks_like_version(id) {
            // `looks_like_version` already confirmed the 3-part numeric
            // shape, so this parse cannot fail.
            let version = parse_version_core(id).expect("validated by looks_like_version");
            deprecations.extend(Deprecation::for_version(version));
        } else {
            eprintln!("Invalid deprecation \"{id}\".");
            std::process::exit(1);
        }
    }
    deprecations
}

/// Resolves `raw` (as given on the command line, relative or absolute) to an
/// absolute, symlink-resolved path, for use in source-map `sources` URL
/// construction. Falls back to the un-canonicalized absolute join if
/// `canonicalize` fails (broken symlink, file since removed, etc.) rather
/// than erroring — a source-map URL that's merely un-resolved is much
/// better than aborting a successful compile over it.
fn absolute_source_path(raw: &str, cwd: &Path) -> PathBuf {
    let p = Path::new(raw);
    let joined = if p.is_absolute() { p.to_path_buf() } else { cwd.join(p) };
    std::fs::canonicalize(&joined).unwrap_or(joined)
}

/// Computes a `/`-joined relative path from directory `base_dir` to file
/// `target`, matching dart-sass's `--source-map-urls=relative` (the
/// default) convention: no leading `./`, `..` segments for each directory
/// level that must be climbed. Both arguments must already be absolute
/// (see `absolute_source_path`) so a plain component-wise comparison finds
/// the common prefix correctly.
fn relative_source_url(base_dir: &Path, target: &Path) -> String {
    let base_components: Vec<_> = base_dir.components().collect();
    let target_components: Vec<_> = target.components().collect();

    let common = base_components
        .iter()
        .zip(target_components.iter())
        .take_while(|(a, b)| a == b)
        .count();

    let mut parts: Vec<String> = Vec::new();
    for _ in common..base_components.len() {
        parts.push("..".to_owned());
    }
    for comp in &target_components[common..] {
        parts.push(comp.as_os_str().to_string_lossy().into_owned());
    }

    parts.join("/")
}

/// Builds an absolute `file://` URL for `target`, percent-encoding it the
/// same way dart-sass does (verified via `sass --source-map-urls=absolute`
/// against a path containing a space).
fn absolute_source_url(target: &Path) -> String {
    format!("file://{}", grass_compiler::encode_uri(&target.to_string_lossy()))
}

/// Rewrites every non-`data:` entry in `map.sources` to either an absolute
/// `file://` URL or a path relative to `output_dir`, per `--source-map-urls`.
/// `output_dir` is `None` only when writing to stdout with
/// `--embed-source-map` (the one case dart-sass allows without an output
/// file) — relative URLs are impossible there, so absolute is used
/// regardless of `urls`, matching the observed fallback behavior.
fn rewrite_source_map_sources(map: &mut SourceMapData, urls: &SourceMapUrls, output_dir: Option<&Path>, cwd: &Path) {
    for source in map.sources.iter_mut() {
        // stdin's `data:` URL sources entry (built by
        // `from_string_with_source_map`) is never rewritten — there is no
        // real path behind it.
        if source.starts_with("data:") {
            continue;
        }

        let absolute = absolute_source_path(source, cwd);

        *source = match (urls, output_dir) {
            (SourceMapUrls::Relative, Some(dir)) => relative_source_url(dir, &absolute),
            _ => absolute_source_url(&absolute),
        };
    }
}

/// Validates the four source-map CLI flags against dart-sass's own
/// constraints (message text and behavior verified via `npx sass@1.97.3`;
/// see docs/design/source-maps.md). Exits the process on a violation.
/// Returns whether a source map should actually be generated
/// (`Options::source_map`) — `false` whenever `--no-source-map` was passed,
/// or output is going to stdout without `--embed-source-map` (matching
/// dart-sass's silent default-off behavior for that case; no error).
fn validate_source_map_flags(matches: &clap::ArgMatches, writing_to_stdout: bool) -> bool {
    let no_source_map = matches.get_flag("NO_SOURCE_MAP");
    let embed_source_map = matches.get_flag("EMBED_SOURCE_MAP");
    let embed_sources = matches.get_flag("EMBED_SOURCES");
    let urls_explicit = matches.value_source("SOURCE_MAP_URLS") == Some(ValueSource::CommandLine);
    let urls = matches.get_one::<SourceMapUrls>("SOURCE_MAP_URLS").unwrap();

    if no_source_map {
        if embed_source_map {
            eprintln!("--embed-source-map isn't allowed with --no-source-map.");
            std::process::exit(1);
        }
        if embed_sources {
            eprintln!("--embed-sources isn't allowed with --no-source-map.");
            std::process::exit(1);
        }
        if urls_explicit {
            eprintln!("--source-map-urls isn't allowed with --no-source-map.");
            std::process::exit(1);
        }
        return false;
    }

    if writing_to_stdout {
        if urls_explicit && *urls == SourceMapUrls::Relative {
            eprintln!("--source-map-urls=relative isn't allowed when printing to stdout.");
            std::process::exit(1);
        }
        if !embed_source_map {
            if urls_explicit {
                eprintln!("When printing to stdout, --source-map-urls requires --embed-source-map.");
                std::process::exit(1);
            }
            if embed_sources {
                eprintln!("When printing to stdout, --embed-sources requires --embed-source-map.");
                std::process::exit(1);
            }
            // dart-sass's default: no map at all when printing to stdout
            // without explicitly forcing one via --embed-source-map.
            return false;
        }
    }

    true
}

/// Everything needed to turn one compile's `Result` into bytes on disk (or
/// stdout), shared between the single-shot path and every recompile in
/// `--watch` mode. Fields mirror the CLI flags that shape output, minus
/// whatever's already baked into the `Options` used for the compile itself.
pub(crate) struct WriteConfig<'a> {
    pub(crate) output_arg: Option<&'a str>,
    pub(crate) error_css_enabled: bool,
    pub(crate) unicode_error_messages: bool,
    pub(crate) generate_source_map: bool,
    pub(crate) embed_source_map: bool,
    pub(crate) embed_sources: bool,
    pub(crate) source_map_urls: SourceMapUrls,
}

/// Handles one compile's outcome end to end: on error, prints the message
/// and does the error-CSS overwrite/delete dance (see `error_css.rs`); on
/// success, assembles the final output bytes (trailing newline, source map
/// comment) and writes them to `cfg.output_arg` or stdout.
///
/// Returns `Ok(true)` on a successful compile and `Ok(false)` on a compile
/// error (the caller decides whether that's fatal: the single-shot path
/// exits 1, `--watch` logs and keeps watching).
pub(crate) fn write_compile_result(
    compile_result: grass::Result<(String, Option<SourceMapData>)>,
    cfg: &WriteConfig,
) -> std::io::Result<bool> {
    let (css, map) = match compile_result {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{}", e);
            if let Some(path) = cfg.output_arg {
                if cfg.error_css_enabled {
                    std::fs::write(path, error_css::synthesize(&e.to_string(), cfg.unicode_error_messages))?;
                } else if Path::new(path).exists() {
                    std::fs::remove_file(path)?;
                }
            }
            return Ok(false);
        }
    };

    // dart-sass's CLI always appends a trailing newline to non-empty output
    // (compile_stylesheet.dart writes `css + "\n"` / uses `print`, which adds
    // one); the library's returned CSS string itself never has one. grass's
    // library output already ends in `\n` for expanded style, so only append
    // when missing to avoid doubling it.
    let mut bytes = css.into_bytes();
    if !bytes.is_empty() && bytes.last() != Some(&b'\n') {
        bytes.push(b'\n');
    }

    if cfg.generate_source_map {
        if let Some(mut map) = map {
            let cwd = std::env::current_dir().unwrap_or_default();
            // Present (`Some`) exactly when `output_arg` is: `generate_source_map`
            // only allows a stdout target when `--embed-source-map` was passed,
            // and dart-sass omits both `file` and the ability to use relative
            // URLs in that one case (see `validate_source_map_flags`).
            let output_path = cfg.output_arg.map(|p| absolute_source_path(p, &cwd));
            let output_dir = output_path.as_deref().and_then(Path::parent);

            rewrite_source_map_sources(&mut map, &cfg.source_map_urls, output_dir, &cwd);

            let file_key = cfg
                .output_arg
                .and_then(|p| Path::new(p).file_name())
                .map(|n| n.to_string_lossy().into_owned());
            let json = map.to_json(file_key.as_deref(), cfg.embed_sources);

            if cfg.embed_source_map {
                bytes.extend_from_slice(b"\n/*# sourceMappingURL=data:application/json;charset=utf-8,");
                bytes.extend_from_slice(grass_compiler::encode_uri(&json).as_bytes());
                bytes.extend_from_slice(b" */\n");
            } else {
                // `generate_source_map` guarantees `output_arg` is `Some` here:
                // stdout output only reaches this branch via --embed-source-map.
                let output_path = output_path.expect("non-stdout output guaranteed by validate_source_map_flags");
                let map_file_name = format!("{}.map", output_path.file_name().unwrap_or_default().to_string_lossy());
                let map_path = output_path.with_file_name(&map_file_name);
                std::fs::write(&map_path, json)?;

                bytes.extend_from_slice(b"\n/*# sourceMappingURL=");
                bytes.extend_from_slice(map_file_name.as_bytes());
                bytes.extend_from_slice(b" */\n");
            }
        }
    }

    // The output file is only opened (and truncated) here, after a
    // successful compile -- a failed compile takes the error-css/delete
    // branch above and returns before ever reaching this point.
    let (mut stdout_write, mut file_write);
    let buf_out: &mut dyn Write = if let Some(path) = cfg.output_arg {
        file_write = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)?;
        &mut file_write
    } else {
        stdout_write = stdout();
        &mut stdout_write
    };

    buf_out.write_all(&bytes)?;

    Ok(true)
}

fn main() -> std::io::Result<()> {
    let matches = cli().get_matches();

    let load_paths = matches
        .get_many::<String>("LOAD_PATH")
        .map_or_else(Vec::new, |vals| vals.map(Path::new).collect());

    let style = match &matches.get_one::<Style>("STYLE").unwrap() {
        Style::Expanded => OutputStyle::Expanded,
        Style::Compressed => OutputStyle::Compressed,
    };

    let use_stdin = matches.get_flag("STDIN");

    // clap fills the INPUT positional slot before OUTPUT, but with `--stdin`
    // the stylesheet comes from stdin, so a lone trailing positional is really
    // the OUTPUT file (dart-sass: `sass --stdin out.css` writes to out.css),
    // NOT an input path. Re-map the positionals here.
    let (input_arg, output_arg): (Option<&String>, Option<&String>) = if use_stdin {
        // No input path under --stdin. The positional clap parsed into INPUT is
        // the output target (prefer an explicit second positional if one landed
        // in OUTPUT, matching clap's declaration order).
        let output = matches
            .get_one::<String>("OUTPUT")
            .or_else(|| matches.get_one::<String>("INPUT"));
        (None, output)
    } else {
        (
            matches.get_one::<String>("INPUT"),
            matches.get_one::<String>("OUTPUT"),
        )
    };
    let writing_to_stdout = output_arg.is_none();
    let watch = matches.get_flag("WATCH");

    // dart-sass rejects both of these combinations outright (verified via
    // npx sass@1.97.3: "--watch is not allowed with --stdin." / "--watch is
    // not allowed when printing to stdout.", exit 64) rather than silently
    // ignoring --watch. grass exits 1 (its own convention for CLI-level
    // usage failures; see `error_exit_code`) but matches the message text
    // and the "hard failure before compilation" behavior.
    if watch && use_stdin {
        eprintln!("--watch is not allowed with --stdin.");
        std::process::exit(1);
    }
    if watch && writing_to_stdout {
        eprintln!("--watch is not allowed when printing to stdout.");
        std::process::exit(1);
    }

    // dart-sass's `--[no-]error-css` (default true when writing to a file;
    // irrelevant for stdout, since a failed compile never writes anything to
    // stdout regardless -- verified via npx sass@1.97.3).
    let error_css_enabled = !matches.get_flag("NO_ERROR_CSS");

    let generate_source_map = validate_source_map_flags(&matches, writing_to_stdout);
    let embed_source_map = matches.get_flag("EMBED_SOURCE_MAP");
    let embed_sources = matches.get_flag("EMBED_SOURCES");
    let source_map_urls = matches.get_one::<SourceMapUrls>("SOURCE_MAP_URLS").unwrap().clone();
    let unicode_error_messages = !matches.get_flag("NO_UNICODE");

    // `--watch` always requests the underlying `SourceMapData` (regardless
    // of whether the user passed `--source-map`) so it can read
    // `SourceMapData::loaded_files` for precise dependency tracking -- see
    // `watch.rs`. This is independent of whether a `.map` actually gets
    // written to disk, which stays gated on `write_config.generate_source_map`.
    let mut options = Options::default()
        .load_paths(&load_paths)
        .style(style)
        .quiet(matches.get_flag("QUIET"))
        .unicode_error_messages(unicode_error_messages)
        .allows_charset(!matches.get_flag("NO_CHARSET"))
        .source_map(generate_source_map || watch);

    for deprecation in parse_deprecations(&matches, "SILENCE_DEPRECATION", false) {
        options = options.silence_deprecation(deprecation);
    }
    for deprecation in parse_deprecations(&matches, "FATAL_DEPRECATION", true) {
        options = options.fatal_deprecation(deprecation);
    }
    for deprecation in parse_deprecations(&matches, "FUTURE_DEPRECATION", false) {
        options = options.future_deprecation(deprecation);
    }

    let options = &options;

    let write_config = WriteConfig {
        output_arg: output_arg.map(String::as_str),
        error_css_enabled,
        unicode_error_messages,
        generate_source_map,
        embed_source_map,
        embed_sources,
        source_map_urls,
    };

    if watch {
        // Validated above: watch requires a real INPUT path and a real
        // OUTPUT file target (never --stdin, never stdout).
        let input = input_arg
            .expect("required_unless_present(STDIN); non-stdin guaranteed by --watch reject above");
        let output = output_arg.expect("writing_to_stdout rejected above");
        return watch::run(watch::WatchArgs {
            input,
            output,
            options,
            write_config,
            load_paths: &load_paths,
            poll: matches.get_flag("POLL"),
        });
    }

    let compile_result = if let Some(name) = input_arg {
        from_path_with_source_map(name, options)
    } else if use_stdin {
        from_string_with_source_map(
            {
                let mut buffer = String::new();
                stdin().read_to_string(&mut buffer)?;
                buffer
            },
            options,
        )
    } else {
        unreachable!()
    };

    if !write_compile_result(compile_result, &write_config)? {
        std::process::exit(1);
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use crate::cli;

    #[test]
    fn verify() {
        cli().debug_assert();
    }
}
