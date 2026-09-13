//! The one canonical compile request each framework's workload cases issue.
//!
//! A template is IMMUTABLE and carried verbatim into the summary and every
//! observation artifact beside its digest, so a reader can tell what was asked
//! without trusting the runner's description of it. There is exactly one
//! substitution — `identity.filename` — and it is applied to the template
//! text, never to a re-serialized object: a request the lane reports and a
//! request the lane sent cannot drift apart.
//!
//! A template omits every optional option deliberately. Absent means "the
//! host's own default", which is what makes a request comparable with the
//! reference compiler's own default compile: the defaults are the compiler's
//! to choose, not the probe's to assert.
//!
//! `framework` is a case attribute here too: which template a case issues is
//! looked up from its framework through [`template_for`], and the lookup is
//! exhaustive over the closed framework set, so a new framework cannot reach
//! the driver carrying another framework's request.

use sha2::{Digest, Sha256};

use crate::manifest::Framework;

/// The placeholder `identity.filename` holds in every template.
pub const FILENAME_PLACEHOLDER: &str = "<case relative path>";

/// The canonical `HostCompileRequest` template for the `vue` arm: canonical
/// JSON, keys sorted at every level, no whitespace.
pub const REQUEST_VUE: &str = concat!(
    r#"{"framework":"vue","#,
    r#""identity":{"componentId":null,"filename":"<case relative path>","forceJs":false,"isProduction":false},"#,
    r#""options":{"babelParserPlugins":[],"backend":"vdom","isCustomElement":[],"ssr":false},"#,
    r#""products":[{"inline":null,"kind":"runtimeClient","runtimeSourceMap":false}]}"#,
);

/// The canonical `HostCompileRequest` template for the `svelte` arm: the same
/// canonical JSON shape, the same single `runtimeClient` product, and an EMPTY
/// options object — every optional Svelte option is intentionally omitted so
/// the host's own defaults apply.
pub const REQUEST_SVELTE: &str = concat!(
    r#"{"framework":"svelte","#,
    r#""identity":{"componentId":null,"filename":"<case relative path>","forceJs":false,"isProduction":false},"#,
    r#""options":{},"#,
    r#""products":[{"inline":null,"kind":"runtimeClient","runtimeSourceMap":false}]}"#,
);

/// The canonical request template a framework's cases issue.
pub const fn template_for(framework: Framework) -> &'static str {
    match framework {
        Framework::Vue => REQUEST_VUE,
        Framework::Svelte => REQUEST_SVELTE,
    }
}

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

/// A framework's template digest, recorded once per summary and per
/// observation artifact header.
pub fn template_digest(framework: Framework) -> String {
    sha256_hex(template_for(framework).as_bytes())
}

/// The framework's template with `identity.filename` set to `relative_path`.
///
/// The path is JSON-escaped rather than interpolated raw: a corpus path is
/// upstream-controlled text, and a quote or backslash in one must produce an
/// escaped string, never a request whose shape the corpus decided.
pub fn substitute(framework: Framework, relative_path: &str) -> String {
    template_for(framework).replace(
        &json_string(FILENAME_PLACEHOLDER),
        &json_string(relative_path),
    )
}

/// The digest of one case's substituted request, recorded on its summary row
/// and every observation row.
pub fn request_digest(framework: Framework, relative_path: &str) -> String {
    sha256_hex(substitute(framework, relative_path).as_bytes())
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
