#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::{
    fs::OpenOptions,
    io::{stdin, stdout, Read, Write},
    path::Path,
};

use clap::{builder::PossibleValue, value_parser, Arg, ArgAction, Command, ValueEnum};

use grass::{from_path, from_string, Deprecation, Options, OutputStyle};

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
            Arg::new("NO_ERROR_CSS")
                .long("no-error-css")
                .hide(true)
                .help("When an error occurs, don't emit a stylesheet describing it."),
        )
        // Source maps
        .arg(
            Arg::new("NO_SOURCE_MAP")
                .long("no-source-map")
                .hide(true)
                .help("Whether to generate source maps."),
        )
        .arg(
            Arg::new("SOURCE_MAP_URLS")
                .long("source-map-urls")
                .hide(true)
                .help("How to link from source maps to source files.")
                .default_value("relative")
                .ignore_case(true)
                .num_args(1)
                .value_parser(value_parser!(SourceMapUrls)),
        )
        .arg(
            Arg::new("EMBED_SOURCES")
                .long("embed-sources")
                .hide(true)
                .help("Embed source file contents in source maps."),
        )
        .arg(
            Arg::new("EMBED_SOURCE_MAP")
                .long("embed-source-map")
                .hide(true)
                .help("Embed source map contents in CSS."),
        )
        // Other
        .arg(
            Arg::new("WATCH")
                .long("watch")
                .hide(true)
                .help("Watch stylesheets and recompile when they change."),
        )
        .arg(
            Arg::new("POLL")
                .long("poll")
                .hide(true)
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

fn main() -> std::io::Result<()> {
    let matches = cli().get_matches();

    let load_paths = matches
        .get_many::<String>("LOAD_PATH")
        .map_or_else(Vec::new, |vals| vals.map(Path::new).collect());

    let style = match &matches.get_one::<Style>("STYLE").unwrap() {
        Style::Expanded => OutputStyle::Expanded,
        Style::Compressed => OutputStyle::Compressed,
    };

    let mut options = Options::default()
        .load_paths(&load_paths)
        .style(style)
        .quiet(matches.get_flag("QUIET"))
        .unicode_error_messages(!matches.get_flag("NO_UNICODE"))
        .allows_charset(!matches.get_flag("NO_CHARSET"));

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

    let (mut stdout_write, mut file_write);
    let buf_out: &mut dyn Write = if let Some(path) = matches.get_one::<String>("OUTPUT") {
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

    let css = if let Some(name) = matches.get_one::<String>("INPUT") {
        from_path(name, options)
    } else if matches.get_flag("STDIN") {
        from_string(
            {
                let mut buffer = String::new();
                stdin().read_to_string(&mut buffer)?;
                buffer
            },
            options,
        )
    } else {
        unreachable!()
    }
    .unwrap_or_else(|e| {
        eprintln!("{}", e);
        std::process::exit(1)
    });

    // dart-sass's CLI always appends a trailing newline to non-empty output
    // (compile_stylesheet.dart writes `css + "\n"` / uses `print`, which adds
    // one); the library's returned CSS string itself never has one. grass's
    // library output already ends in `\n` for expanded style, so only append
    // when missing to avoid doubling it.
    let mut bytes = css.into_bytes();
    if !bytes.is_empty() && bytes.last() != Some(&b'\n') {
        bytes.push(b'\n');
    }
    buf_out.write_all(&bytes)?;
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
