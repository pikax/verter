//! Public parse-surface closure: the crate-root export list is the universe.
//!
//! A hand-written name list missed `parse_style_body` and the exported
//! `Parser::parse`. This file derives the live public parse entries from
//! `src/lib.rs` `pub use` groups and asserts they equal the gateway plus the
//! recorded facades. One trybuild compile-fail case per forbidden class.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

/// The one crate-public event-stream gateway.
const GATEWAY: &[&str] = &["parse_with_sink"];

/// Public parse facades retained with a recorded justification. These are
/// not the gateway; they wrap it and return a specialised owned shape.
const RECORDED: &[&str] = &[
    "parse_component_value_tree",
    "parse_inline_style_declarations",
    "parse_style_body",
    "parse_style_ir",
];

/// Sealed routes: must not appear in crate-root re-exports, and each has
/// a trybuild compile-fail at both the crate root and the module path.
const FORBIDDEN_CLASSES: &[&str] = &[
    "parse_lossless",
    "Parser",
    "style_body_reject_code",
    "parse_selector_structure",
];

fn lib_rs() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

/// Every identifier named by a crate-root `pub use` (cfg-gated or not).
/// Derived from the export list, never hand-written.
fn crate_root_reexports(lib: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut rest = lib;
    while let Some(idx) = rest.find("pub use ") {
        rest = &rest[idx + "pub use ".len()..];
        let Some(semi) = rest.find(';') else {
            break;
        };
        let stmt = &rest[..semi];
        rest = &rest[semi + 1..];
        if let Some(brace) = stmt.find('{') {
            let Some(end) = stmt[brace + 1..].find('}') else {
                continue;
            };
            let body = &stmt[brace + 1..brace + 1 + end];
            for item in body.split(',') {
                let ident = item
                    .trim()
                    .trim_end_matches(',')
                    .split_whitespace()
                    .last()
                    .unwrap_or("");
                if !ident.is_empty() {
                    names.insert(ident.to_string());
                }
            }
        } else if let Some(ident) = stmt.rsplit("::").next() {
            let ident = ident.trim();
            if !ident.is_empty() {
                names.insert(ident.to_string());
            }
        }
    }
    names
}

/// Public parse-route names derived from the crate-root export list.
///
/// A route is anything that starts a parse: `parse_*` identifiers, the
/// raw-source helper `style_body_reject_code`, and the `Parser` type
/// (whose `parse` method is the same operation as the gateway).
fn derived_public_parse_entries(lib: &str) -> BTreeSet<String> {
    crate_root_reexports(lib)
        .into_iter()
        .filter(|name| {
            if name.ends_with("_thread_invocations") || name.ends_with("_invocations") {
                return false;
            }
            name.starts_with("parse_") || name == "style_body_reject_code" || name == "Parser"
        })
        .collect()
}

#[test]
fn public_parse_surface_is_exactly_the_gateway() {
    let lib = lib_rs();
    let derived = derived_public_parse_entries(&lib);
    let allowed: BTreeSet<String> = GATEWAY
        .iter()
        .chain(RECORDED.iter())
        .map(|name| (*name).to_string())
        .collect();

    for name in FORBIDDEN_CLASSES {
        assert!(
            !derived.contains(*name),
            "{name} must not be crate-root public — seal it or record a justification"
        );
    }

    assert_eq!(
        derived,
        allowed,
        "crate-root public parse entries must equal the gateway plus recorded facades.\n\
         derived - allowed = {:?}\n\
         allowed - derived = {:?}",
        derived.difference(&allowed).collect::<Vec<_>>(),
        allowed.difference(&derived).collect::<Vec<_>>()
    );
}
