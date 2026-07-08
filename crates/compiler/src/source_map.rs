//! Source Map v3 encoder used by the `from_string_with_source_map` /
//! `from_path_with_source_map` prototype (Plan 013 design spike, wired up in
//! Plan 062). Hand-rolled rather than pulling in a dependency: the VLQ
//! alphabet is ~70 lines total and this repo is deliberately dep-skeptical
//! (see `Cargo.toml`).

use std::{path::PathBuf, sync::Arc};

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// A single (generated position -> source position) mapping, gathered while
/// the serializer writes CSS. Line/column are both 0-indexed, matching both
/// `codemap::LineCol` and the Source Map v3 spec.
#[derive(Debug, Clone)]
pub(crate) struct RawMapping {
    pub dst_line: usize,
    pub dst_col: usize,
    /// Index into the `sources` array this mapping was built from.
    pub src_file_idx: usize,
    pub src_line: usize,
    pub src_col: usize,
}

/// Computes `pos`'s column on line `line` of `file` in UTF-16 code units,
/// matching the Source Map v3 spec (and dart-sass/JS tooling in general).
/// `codemap::File::find_line_col` (used by `CodeMap::look_up_pos`) instead
/// counts Unicode scalar values (`str::chars().count()`), which undercounts
/// by one per supplementary-plane character (e.g. an emoji) — verified
/// against dart-sass with a fixture where a preceding emoji shifts a same-line
/// mapping's column by 2, not 1 (see `crates/lib/tests/cli_source_map.rs`).
/// Built entirely from `codemap`'s public API (`File::line_span`/
/// `File::source_slice`), so this needs no patch to the pinned dependency.
pub(crate) fn utf16_column(file: &codemap::File, line: usize, pos: codemap::Pos) -> usize {
    let line_span = file.line_span(line);
    let byte_col = (pos - line_span.low()) as usize;
    let line_text = file.source_slice(line_span);
    line_text[..byte_col].encode_utf16().count()
}

/// Encode a single signed value as Base64 VLQ, appending to `out`.
fn encode_vlq(value: i64, out: &mut String) {
    // Sign lives in the low bit of the first quintet; the magnitude fills the
    // rest, least-significant quintet first.
    let mut num = if value < 0 {
        ((-value) as u64) << 1 | 1
    } else {
        (value as u64) << 1
    };

    loop {
        let mut digit = (num & 0b1_1111) as u8;
        num >>= 5;
        if num > 0 {
            digit |= 0b10_0000;
        }
        out.push(BASE64_ALPHABET[digit as usize] as char);
        if num == 0 {
            break;
        }
    }
}

/// A built Source Map v3 document, prior to any CLI-level `sources` URL
/// rewriting (`--source-map-urls=absolute`) or `file`/`sourcesContent`
/// decoration. `sources` and `sources_content` are `pub` and index-parallel
/// (same length, same order) so a downstream crate (the CLI) can rewrite
/// `sources` in place — e.g. to absolute `file://` URLs, or a path relative
/// to the output `.map` file's directory — without needing any accessor
/// beyond direct field access.
#[derive(Debug, Clone)]
pub struct SourceMapData {
    /// Deduplicated, first-appearance-ordered source file names/URLs.
    pub sources: Vec<String>,
    /// Source file handles, parallel to `sources`. Kept as cheap `Arc`
    /// clones rather than owned text so a maps-on-but-not-embedded compile
    /// never deep-copies file contents; `to_json` reads `.source()` (a
    /// borrow, no clone) only when `embed_sources` is true.
    pub sources_content: Vec<Arc<codemap::File>>,
    /// The full set of files loaded during this compile via `@use`/
    /// `@forward`/`@import` (plus the entry file itself), deduplicated and
    /// sorted for a deterministic order. Unlike `sources`, this is *not*
    /// limited to files that contributed an emitted CSS mapping -- e.g. a
    /// `@use`d partial containing only variables never appears in `sources`
    /// but does appear here. Intended for precise dependency tracking (e.g.
    /// `--watch`), not for anything source-map-spec-shaped.
    pub loaded_files: Vec<PathBuf>,
    /// Pre-encoded VLQ `mappings` string. Computed once at construction time
    /// since it depends only on the numeric fields of each `RawMapping`
    /// (line/column/file-index deltas), never on the string contents of
    /// `sources`, so rewriting `sources` afterward cannot invalidate it.
    encoded_mappings: String,
}

impl SourceMapData {
    pub(crate) fn new(
        mappings: &[RawMapping],
        sources: Vec<String>,
        sources_content: Vec<Arc<codemap::File>>,
        loaded_files: Vec<PathBuf>,
    ) -> Self {
        Self {
            sources,
            sources_content,
            loaded_files,
            encoded_mappings: encode_mappings(mappings),
        }
    }

    /// Serialize to a Source Map v3 JSON document.
    ///
    /// `file` is the CLI's output-file-name field; pass `None` to omit it
    /// entirely, matching the JS API's `sourceMap` object (which never has a
    /// `file` key — confirmed via `sass.compileString(..., {sourceMap:
    /// true})`). `embed_sources` toggles whether `sourcesContent` is
    /// emitted (`--embed-sources` / the JS API's `sourceMapIncludeSources`).
    #[must_use]
    pub fn to_json(&self, file: Option<&str>, embed_sources: bool) -> String {
        let mut sources_json = String::from("[");
        for (idx, source) in self.sources.iter().enumerate() {
            if idx > 0 {
                sources_json.push(',');
            }
            sources_json.push('"');
            json_escape_into(source, &mut sources_json);
            sources_json.push('"');
        }
        sources_json.push(']');

        let mut out = format!(
            "{{\"version\":3,\"sourceRoot\":\"\",\"sources\":{sources_json},\"names\":[],\"mappings\":\"{}\"",
            self.encoded_mappings
        );

        if let Some(file) = file {
            out.push_str(",\"file\":\"");
            json_escape_into(file, &mut out);
            out.push('"');
        }

        if embed_sources {
            out.push_str(",\"sourcesContent\":[");
            for (idx, content) in self.sources_content.iter().enumerate() {
                if idx > 0 {
                    out.push(',');
                }
                out.push('"');
                json_escape_into(content.source(), &mut out);
                out.push('"');
            }
            out.push(']');
        }

        out.push('}');
        out
    }
}

/// Percent-encode `input` the way JavaScript's `encodeURI` does (verified
/// byte-for-byte against `sass.compileString(..., {sourceMap: true})`'s
/// `data:` URL sources entry for stdin-style input, including a fixture
/// with `"`, `%`, `<`, `>`, `` ` ``, and other punctuation). Unlike
/// `encodeURIComponent`, this preserves URI-reserved characters
/// (`;,/?:@&=+$#`) and the unreserved set (`A-Za-z0-9-_.!~*'()`) unescaped;
/// everything else (including space, `{`, `}`, and all non-ASCII bytes) is
/// percent-encoded.
pub fn encode_uri(input: &str) -> String {
    const UNESCAPED: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_.!~*'();,/?:@&=+$#";

    const HEX_DIGITS: &[u8; 16] = b"0123456789ABCDEF";

    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        if UNESCAPED.contains(&byte) {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push(HEX_DIGITS[(byte >> 4) as usize] as char);
            out.push(HEX_DIGITS[(byte & 0xF) as usize] as char);
        }
    }
    out
}

/// Builds the `data:` URL dart-sass uses as the `sources` entry for
/// string-only input with no real file path (stdin, or the JS API's
/// `compileString` without a `url` option).
pub(crate) fn stdin_data_url(input: &str) -> String {
    format!("data:;charset=utf-8,{}", encode_uri(input))
}

fn json_escape_into(input: &str, out: &mut String) {
    for c in input.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
}

fn encode_mappings(mappings: &[RawMapping]) -> String {
    let mut out = String::new();

    if mappings.is_empty() {
        return out;
    }

    let last_line = mappings.iter().map(|m| m.dst_line).max().unwrap_or(0);

    // Running state for the three source-side fields; per spec these are
    // cumulative across the *entire* mappings string, not reset per line.
    // `dst_col`, by contrast, resets to 0 at the start of every generated line.
    let mut prev_src_file_idx: i64 = 0;
    let mut prev_src_line: i64 = 0;
    let mut prev_src_col: i64 = 0;

    let mut cursor = 0;
    for line in 0..=last_line {
        if line > 0 {
            out.push(';');
        }

        let mut prev_dst_col: i64 = 0;
        let mut first_on_line = true;

        while cursor < mappings.len() && mappings[cursor].dst_line == line {
            let m = &mappings[cursor];

            if !first_on_line {
                out.push(',');
            }
            first_on_line = false;

            encode_vlq(m.dst_col as i64 - prev_dst_col, &mut out);
            encode_vlq(m.src_file_idx as i64 - prev_src_file_idx, &mut out);
            encode_vlq(m.src_line as i64 - prev_src_line, &mut out);
            encode_vlq(m.src_col as i64 - prev_src_col, &mut out);

            prev_dst_col = m.dst_col as i64;
            prev_src_file_idx = m.src_file_idx as i64;
            prev_src_line = m.src_line as i64;
            prev_src_col = m.src_col as i64;

            cursor += 1;
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vlq_matches_dart_sass_first_mapping() {
        // dart-sass encodes a (0,0,0,0) mapping as "AAAA" (observed via
        // `npx sass@1.97.3 in.scss out.css` on `a {\n  b: c;\n}\n`).
        let mut out = String::new();
        encode_vlq(0, &mut out);
        encode_vlq(0, &mut out);
        encode_vlq(0, &mut out);
        encode_vlq(0, &mut out);
        assert_eq!(out, "AAAA");
    }

    #[test]
    fn vlq_matches_dart_sass_second_mapping() {
        // Same fixture's second mapping ("EACE"): dst_col +2, file +0,
        // src_line +1, src_col +2.
        let mut out = String::new();
        encode_vlq(2, &mut out);
        encode_vlq(0, &mut out);
        encode_vlq(1, &mut out);
        encode_vlq(2, &mut out);
        assert_eq!(out, "EACE");
    }

    #[test]
    fn vlq_negative_value() {
        // From the two-file fixture's third group ("ACCF"): last field is -2.
        let mut out = String::new();
        encode_vlq(-2, &mut out);
        assert_eq!(out, "F");
    }

    #[test]
    fn encode_uri_matches_dart_sass_stdin_data_url() {
        // `echo 'a { b: c; }' | sass --stdin` (dart-sass 1.97.3, verified via
        // the JS API `compileString('a { b: c; }\n', {sourceMap: true})`)
        // produces this exact `sources` entry.
        assert_eq!(
            stdin_data_url("a { b: c; }\n"),
            "data:;charset=utf-8,a%20%7B%20b:%20c;%20%7D%0A"
        );
    }

    #[test]
    fn encode_uri_preserves_reserved_punctuation() {
        // Verified against `encodeURI` in Node, itself verified byte-for-byte
        // against dart-sass's data: URL for a fixture containing this exact
        // punctuation mix (see docs/design/source-maps.md probe notes).
        let src = "/* \"x\" %b <c> = hi?/\\|^`~@!$&()*+,;:='\t */\na{b:c}";
        assert_eq!(
            encode_uri(src),
            "/*%20%22x%22%20%25b%20%3Cc%3E%20=%20hi?/%5C%7C%5E%60~@!$&()*+,;:='%09%20*/%0Aa%7Bb:c%7D"
        );
    }

    #[test]
    fn json_escape_escapes_control_characters() {
        let mut out = String::new();
        json_escape_into("a\nb\tc\rd", &mut out);
        assert_eq!(out, "a\\nb\\tc\\rd");
    }

    fn test_file(name: &str, source: &str) -> Arc<codemap::File> {
        codemap::CodeMap::new().add_file(name.to_owned(), source.to_owned())
    }

    #[test]
    fn to_json_omits_file_when_none() {
        let data = SourceMapData::new(
            &[],
            vec!["stdin".to_owned()],
            vec![test_file("stdin", "")],
            vec![],
        );
        let json = data.to_json(None, false);
        assert!(!json.contains("\"file\""), "got: {json}");
    }

    #[test]
    fn to_json_includes_file_when_given() {
        let data = SourceMapData::new(
            &[],
            vec!["stdin".to_owned()],
            vec![test_file("stdin", "")],
            vec![],
        );
        let json = data.to_json(Some("out.css"), false);
        assert!(json.contains("\"file\":\"out.css\""), "got: {json}");
    }

    #[test]
    fn to_json_embeds_sources_content_only_when_requested() {
        let data = SourceMapData::new(
            &[],
            vec!["in.scss".to_owned()],
            vec![test_file("in.scss", "a { b: c; }")],
            vec![],
        );
        assert!(!data.to_json(None, false).contains("sourcesContent"));
        let embedded = data.to_json(None, true);
        assert!(embedded.contains("\"sourcesContent\":[\"a { b: c; }\"]"));
    }
}
