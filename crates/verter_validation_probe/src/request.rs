//! The one canonical compile request every Vue workload case issues.
//!
//! The template is IMMUTABLE and carried verbatim into the summary and every
//! observation artifact beside its digest, so a reader can tell what was asked
//! without trusting the runner's description of it. There is exactly one
//! substitution — `identity.filename` — and it is applied to the template
//! text, never to a re-serialized object: a request the lane reports and a
//! request the lane sent cannot drift apart.
//!
//! The template omits every optional option deliberately. Absent means "the
//! host's own default", which is what makes this request comparable with the
//! reference compiler's `dev`, non-SSR, VDOM compile: the defaults are the
//! compiler's to choose, not the probe's to assert.

use sha2::{Digest, Sha256};

/// The placeholder `identity.filename` holds in the template.
pub const FILENAME_PLACEHOLDER: &str = "<case relative path>";

/// The canonical `HostCompileRequest` template for the `vue` arm: canonical
/// JSON, keys sorted at every level, no whitespace.
pub const REQUEST_VUE: &str = concat!(
    r#"{"framework":"vue","#,
    r#""identity":{"componentId":null,"filename":"<case relative path>","forceJs":false,"isProduction":false},"#,
    r#""options":{"babelParserPlugins":[],"backend":"vdom","isCustomElement":[],"ssr":false},"#,
    r#""products":[{"inline":null,"kind":"runtimeClient","runtimeSourceMap":false}]}"#,
);

/// Lowercase hex of `bytes`' SHA-256.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push(char::from_digit((byte >> 4) as u32, 16).expect("nibble"));
        out.push(char::from_digit((byte & 0x0f) as u32, 16).expect("nibble"));
    }
    out
}

/// The template's own digest, recorded once per summary and per observation
/// artifact header.
pub fn template_digest() -> String {
    sha256_hex(REQUEST_VUE.as_bytes())
}

/// The template with `identity.filename` set to `relative_path`.
///
/// The path is JSON-escaped rather than interpolated raw: a corpus path is
/// upstream-controlled text, and a quote or backslash in one must produce an
/// escaped string, never a request whose shape the corpus decided.
pub fn substitute(relative_path: &str) -> String {
    REQUEST_VUE.replace(
        &json_string(FILENAME_PLACEHOLDER),
        &json_string(relative_path),
    )
}

/// The digest of one case's substituted request, recorded on its summary row
/// and every observation row.
pub fn request_digest(relative_path: &str) -> String {
    sha256_hex(substitute(relative_path).as_bytes())
}

/// `value` as a JSON string literal, including its quotes.
fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str(r#"\""#),
            '\\' => out.push_str(r"\\"),
            '\n' => out.push_str(r"\n"),
            '\r' => out.push_str(r"\r"),
            '\t' => out.push_str(r"\t"),
            ch if (ch as u32) < 0x20 => {
                let _ = std::fmt::Write::write_fmt(&mut out, format_args!(r"\u{:04x}", ch as u32));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}
