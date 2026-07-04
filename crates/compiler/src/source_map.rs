//! Minimal Source Map v3 encoder used by the `from_string_with_source_map`
//! prototype (Plan 013 design spike). Hand-rolled rather than pulling in a
//! dependency: the VLQ alphabet is ~70 lines total and this repo is
//! deliberately dep-skeptical (see `Cargo.toml`).

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

/// Build a Source Map v3 JSON document from collected mappings.
///
/// `mappings` must already be sorted by `(dst_line, dst_col)`; `sources` is
/// the deduplicated, first-appearance-ordered list of source file names that
/// `src_file_idx` indexes into. `output_file` is the `file` field (the name
/// of the generated CSS; may be empty).
pub(crate) fn build_source_map_json(
    mappings: &[RawMapping],
    sources: &[String],
    output_file: &str,
) -> String {
    let mappings_str = encode_mappings(mappings);

    let mut sources_json = String::from("[");
    for (idx, source) in sources.iter().enumerate() {
        if idx > 0 {
            sources_json.push(',');
        }
        sources_json.push('"');
        json_escape_into(source, &mut sources_json);
        sources_json.push('"');
    }
    sources_json.push(']');

    let mut file_json = String::from("\"");
    json_escape_into(output_file, &mut file_json);
    file_json.push('"');

    format!(
        "{{\"version\":3,\"sourceRoot\":\"\",\"sources\":{sources_json},\"names\":[],\"mappings\":\"{mappings_str}\",\"file\":{file_json}}}"
    )
}

fn json_escape_into(input: &str, out: &mut String) {
    for c in input.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            _ => out.push(c),
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
}
