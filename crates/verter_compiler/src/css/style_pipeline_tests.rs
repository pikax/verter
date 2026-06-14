//! Owner-path tests for the single CSS style processor.
//!
//! These pin the contracts of `process_style`:
//! 1. Zero-marker passthrough is zero-copy — it borrows the input
//!    (`Cow::Borrowed`) and never normalizes when no scoped/deep/slotted/module/
//!    v-bind marker is present.
//! 2. The result carries the structural facts the owner path discovered
//!    (`scoped`, `has_deep`, `has_slotted`, `normalization_needed`) so consumers
//!    read them instead of re-scanning the CSS text.
//! 3. lightningcss normalization runs exactly once, and only when a transform
//!    (CSS modules or scoped attribute insertion) actually needs a flattened AST.
//! 4. Byte-for-byte emitted CSS for the transformed surface (scoped, modules,
//!    deep/slotted/global, v-bind, nested), including the CSS-module
//!    hashing/mapping.

use super::*;
use std::borrow::Cow;

fn opts(scoped: bool, is_module: bool) -> ProcessStyleOptions<'static> {
    ProcessStyleOptions {
        scope_id: "a4f2eed6",
        scoped,
        is_module,
        module_name: None,
        filename: None,
        sourcemap: false,
    }
}

/// Normalize line endings so a CRLF checkout cannot perturb a byte comparison.
fn norm(s: &str) -> String {
    s.replace("\r\n", "\n")
}

// ───────────────────────────────────────────────────────────────────────────
// Zero-copy passthrough.
// ───────────────────────────────────────────────────────────────────────────

/// A `<style>` with no scoped/module flag and no v-bind/deep/slotted marker is
/// returned by borrowing the input buffer: no prepass copy, no normalization.
#[test]
fn zero_marker_style_borrows_input_zero_copy() {
    let css = ".box { color: red; }\n.card { display: flex; }";
    let result = process_style(css, &opts(false, false)).unwrap();

    match &result.code {
        Cow::Borrowed(borrowed) => {
            assert_eq!(
                borrowed.as_ptr(),
                css.as_ptr(),
                "zero-marker passthrough must borrow the input buffer (zero-copy)"
            );
            assert_eq!(
                borrowed.len(),
                css.len(),
                "borrowed slice must span the whole input"
            );
        }
        Cow::Owned(owned) => panic!(
            "zero-marker style must return a borrowed (zero-copy) code, got Owned({owned:?})"
        ),
    }
    // And the bytes are exactly the input (no normalization on the passthrough).
    assert_eq!(norm(&result.code), norm(css));
    assert!(result.module_classes.is_empty());
    assert!(result.module_name.is_none());
    assert!(result.v_bind_vars.is_empty());
}

/// The prepass — the structural owner of marker detection — borrows when it
/// finds no Vue syntax and only allocates when it actually rewrites something.
#[test]
fn prepass_borrows_marker_free_css_and_owns_on_transform() {
    let marker_free = ".box { color: red; } .card { display: flex; }";
    let r = prepass::prepass(marker_free, "a4f2eed6");
    match &r.css {
        Cow::Borrowed(b) => assert_eq!(b.as_ptr(), marker_free.as_ptr()),
        Cow::Owned(o) => panic!("marker-free CSS must borrow, got Owned({o:?})"),
    }
    assert!(r.v_bind_vars.is_empty());

    let with_vbind = ".box { color: v-bind(primary); }";
    let r = prepass::prepass(with_vbind, "a4f2eed6");
    assert!(
        matches!(r.css, Cow::Owned(_)),
        "a v-bind rewrite must produce an owned buffer"
    );
    assert_eq!(r.v_bind_vars.len(), 1);
}

// ───────────────────────────────────────────────────────────────────────────
// Returned structural facts — the owner path reports scoped/deep/slotted and
// whether normalization ran, so consumers never re-scan the CSS text.
// ───────────────────────────────────────────────────────────────────────────

/// `process_style` returns the correct structural facts for each marker class:
/// plain CSS, scoped, `:deep`, `:slotted`, CSS module, and `v-bind`.
#[test]
fn returned_facts_describe_each_marker_kind() {
    struct Case {
        label: &'static str,
        css: &'static str,
        scoped: bool,
        is_module: bool,
        want_scoped: bool,
        want_deep: bool,
        want_slotted: bool,
        want_v_bind: usize,
        want_normalization: bool,
        want_module_name: Option<&'static str>,
    }

    let cases = [
        Case {
            label: "plain",
            css: ".box { color: red; }",
            scoped: false,
            is_module: false,
            want_scoped: false,
            want_deep: false,
            want_slotted: false,
            want_v_bind: 0,
            want_normalization: false,
            want_module_name: None,
        },
        Case {
            label: "scoped",
            css: ".box { color: red; }",
            scoped: true,
            is_module: false,
            want_scoped: true,
            want_deep: false,
            want_slotted: false,
            want_v_bind: 0,
            want_normalization: true,
            want_module_name: None,
        },
        Case {
            label: "deep",
            css: ":deep(.inner) { color: red; }",
            scoped: true,
            is_module: false,
            want_scoped: true,
            want_deep: true,
            want_slotted: false,
            want_v_bind: 0,
            want_normalization: true,
            want_module_name: None,
        },
        Case {
            label: "slotted",
            css: ":slotted(.slot) { color: red; }",
            scoped: true,
            is_module: false,
            want_scoped: true,
            want_deep: false,
            want_slotted: true,
            want_v_bind: 0,
            want_normalization: true,
            want_module_name: None,
        },
        Case {
            label: "module",
            css: ".btn { color: red; }",
            scoped: false,
            is_module: true,
            want_scoped: false,
            want_deep: false,
            want_slotted: false,
            want_v_bind: 0,
            want_normalization: true,
            want_module_name: Some("$style"),
        },
        Case {
            label: "v-bind",
            css: ".box { color: v-bind(primary); }",
            scoped: false,
            is_module: false,
            want_scoped: false,
            want_deep: false,
            want_slotted: false,
            want_v_bind: 1,
            want_normalization: false,
            want_module_name: None,
        },
    ];

    for c in cases {
        let result = process_style(c.css, &opts(c.scoped, c.is_module)).unwrap();
        assert_eq!(result.scoped, c.want_scoped, "[{}] scoped fact", c.label);
        assert_eq!(result.has_deep, c.want_deep, "[{}] has_deep fact", c.label);
        assert_eq!(
            result.has_slotted, c.want_slotted,
            "[{}] has_slotted fact",
            c.label
        );
        assert_eq!(
            result.v_bind_vars.len(),
            c.want_v_bind,
            "[{}] v_bind count",
            c.label
        );
        assert_eq!(
            result.normalization_needed, c.want_normalization,
            "[{}] normalization_needed fact",
            c.label
        );
        assert_eq!(
            result.module_name.as_deref(),
            c.want_module_name,
            "[{}] module_name",
            c.label
        );
    }
}

/// The pre-pass is the structural owner of deep/slotted detection: it reports a
/// fact only when it actually rewrote that selector kind (both the `:` and
/// legacy `::v-` spellings), and reports neither for plain CSS.
#[test]
fn prepass_reports_deep_and_slotted_facts() {
    let deep = prepass::prepass(":deep(.inner) { color: red; }", "a4f2eed6");
    assert!(deep.has_deep, ":deep must set has_deep");
    assert!(!deep.has_slotted, ":deep must not set has_slotted");

    let slotted = prepass::prepass(":slotted(.slot) { color: red; }", "a4f2eed6");
    assert!(slotted.has_slotted, ":slotted must set has_slotted");
    assert!(!slotted.has_deep, ":slotted must not set has_deep");

    let v_deep = prepass::prepass("::v-deep(.inner) { color: red; }", "a4f2eed6");
    assert!(v_deep.has_deep, "::v-deep must set has_deep");

    let v_slotted = prepass::prepass("::v-slotted(.slot) { color: red; }", "a4f2eed6");
    assert!(v_slotted.has_slotted, "::v-slotted must set has_slotted");

    let plain = prepass::prepass(".box { color: red; }", "a4f2eed6");
    assert!(
        !plain.has_deep && !plain.has_slotted,
        "plain CSS sets neither"
    );

    // An empty `:deep()` passes through unchanged, so it sets no fact.
    let empty_deep = prepass::prepass(":deep() { color: red; }", "a4f2eed6");
    assert!(!empty_deep.has_deep, "empty :deep() sets no fact");
}

// ───────────────────────────────────────────────────────────────────────────
// Single normalization — a transform normalizes once; a passthrough never does.
// ───────────────────────────────────────────────────────────────────────────

/// lightningcss normalization runs exactly once when a transform needs a
/// flattened AST (one parse/serialize feeds both the modules and scoped
/// walkers), and never runs on a marker-free or v-bind-only passthrough.
#[test]
fn owner_path_normalizes_once_for_transforms_and_never_on_passthrough() {
    // scoped + module: a single normalization feeds both walkers.
    super::normalize_probe::reset();
    let _ = process_style(".btn { color: red; }", &opts(true, true)).unwrap();
    assert_eq!(
        super::normalize_probe::count(),
        1,
        "scoped+module must normalize exactly once"
    );

    // scoped only: one normalization.
    super::normalize_probe::reset();
    let _ = process_style(".box { color: red; }", &opts(true, false)).unwrap();
    assert_eq!(
        super::normalize_probe::count(),
        1,
        "scoped must normalize exactly once"
    );

    // module only: one normalization.
    super::normalize_probe::reset();
    let _ = process_style(".btn { color: red; }", &opts(false, true)).unwrap();
    assert_eq!(
        super::normalize_probe::count(),
        1,
        "module must normalize exactly once"
    );

    // marker-free passthrough: zero normalizations.
    super::normalize_probe::reset();
    let _ = process_style(".box { color: red; }", &opts(false, false)).unwrap();
    assert_eq!(
        super::normalize_probe::count(),
        0,
        "marker-free passthrough must not normalize"
    );

    // v-bind only (no scoped/module): owned but not normalized.
    super::normalize_probe::reset();
    let _ = process_style(".box { color: v-bind(primary); }", &opts(false, false)).unwrap();
    assert_eq!(
        super::normalize_probe::count(),
        0,
        "v-bind-only must not normalize"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Single public style processor.
// ───────────────────────────────────────────────────────────────────────────

/// `process_style` is the one public CSS style entry point: css/mod.rs exposes
/// exactly one `pub fn process_style…`, so no normalize-skipping sibling
/// processor can sit beside it. A second public entry point starting with
/// `process_style` (the shape of a separate normalize-skipping processor) makes
/// the count exceed one and fails this test.
#[test]
fn exactly_one_public_style_processor() {
    let source = include_str!("mod.rs");
    let count = source.matches("pub fn process_style").count();
    assert_eq!(
        count, 1,
        "css/mod.rs must expose exactly one `pub fn process_style…` entry point, found {count}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Byte-for-byte emitted-CSS corpus.
// Expected values are the locked output of the single owner path.
// ───────────────────────────────────────────────────────────────────────────

fn assert_code(css: &str, scoped: bool, is_module: bool, expected: &str) {
    let result = process_style(css, &opts(scoped, is_module)).unwrap();
    assert_eq!(
        norm(&result.code),
        norm(expected),
        "emitted CSS drifted for input {css:?}"
    );
}

#[test]
fn parity_scoped_basic() {
    assert_code(
        ".box { color: red; }",
        true,
        false,
        ".box[data-v-a4f2eed6]{\n  color: red;\n}\n",
    );
}

#[test]
fn parity_scoped_compound_and_descendant() {
    assert_code(
        ".badge.success { color: green; } .parent .child { color: red; }",
        true,
        false,
        ".badge.success[data-v-a4f2eed6]{\n  color: green;\n}\n\n.parent .child[data-v-a4f2eed6]{\n  color: red;\n}\n",
    );
}

#[test]
fn parity_deep() {
    assert_code(
        ":deep(.inner) { color: red; }",
        true,
        false,
        "[data-v-a4f2eed6] .inner{\n  color: red;\n}\n",
    );
}

#[test]
fn parity_slotted() {
    assert_code(
        ":slotted(.slot) { color: red; }",
        true,
        false,
        ".slot[data-v-a4f2eed6-s]{\n  color: red;\n}\n",
    );
}

#[test]
fn parity_global() {
    assert_code(
        ":global(.reset) { margin: 0; }",
        true,
        false,
        ".reset{\n  margin: 0;\n}\n",
    );
}

#[test]
fn parity_nested_selectors() {
    // lightningcss normalization preserves native nesting and rewrites `blue`
    // → `#00f`; both inner and outer compounds are scoped.
    assert_code(
        ".parent { color: red; & .child { color: blue; } }",
        true,
        false,
        ".parent[data-v-a4f2eed6]{\n  color: red;\n\n  & .child[data-v-a4f2eed6]{\n    color: #00f;\n  }\n}\n",
    );
}

#[test]
fn parity_v_bind_scoped() {
    let css = ".box { color: v-bind(primary); font-size: v-bind('theme.size'); }";
    let result = process_style(css, &opts(true, false)).unwrap();
    assert_eq!(
        norm(&result.code),
        norm(".box[data-v-a4f2eed6]{\n  color: var(--a4f2eed6-primary);\n  font-size: var(--a4f2eed6-theme_size);\n}\n"),
    );
    let vars: Vec<(&str, &str)> = result
        .v_bind_vars
        .iter()
        .map(|v| (v.expression.as_str(), v.var_name.as_str()))
        .collect();
    assert_eq!(
        vars,
        vec![
            ("primary", "--a4f2eed6-primary"),
            ("theme.size", "--a4f2eed6-theme_size"),
        ]
    );
}

/// v-bind present but neither scoped nor module: the buffer is owned (rewritten)
/// yet NOT normalized — the owner path normalizes only when a transform needs it.
#[test]
fn parity_v_bind_without_scope_is_unnormalized() {
    let css = ".box { color: v-bind(primary); }";
    let result = process_style(css, &opts(false, false)).unwrap();
    assert!(
        matches!(result.code, Cow::Owned(_)),
        "v-bind rewrite must own the buffer"
    );
    assert_eq!(
        norm(&result.code),
        norm(".box { color: var(--a4f2eed6-primary); }")
    );
    assert_eq!(result.v_bind_vars.len(), 1);
    assert_eq!(result.v_bind_vars[0].expression, "primary");
}

// ───────────────────────────────────────────────────────────────────────────
// CSS modules — mapping AND emitted CSS. The module class hashing (the SHA
// suffix) is the owner path's locked output.
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn css_module_mapping_and_emitted_css_unchanged() {
    let css = ".btn { color: red; } .card { display: flex; }";
    let result = process_style(css, &opts(false, true)).unwrap();

    assert_eq!(
        norm(&result.code),
        norm(".btn_87199871{\n  color: red;\n}\n\n.card_ec3470e2{\n  display: flex;\n}\n"),
    );
    assert_eq!(
        result.module_classes,
        vec![
            ("btn".to_string(), "btn_87199871".to_string()),
            ("card".to_string(), "card_ec3470e2".to_string()),
        ]
    );
    assert_eq!(result.module_name.as_deref(), Some("$style"));
}

#[test]
fn parity_scoped_and_module_combined() {
    let css = ".btn { color: red; } .card { display: flex; }";
    let result = process_style(css, &opts(true, true)).unwrap();
    assert_eq!(
        norm(&result.code),
        norm(".btn_87199871[data-v-a4f2eed6]{\n  color: red;\n}\n\n.card_ec3470e2[data-v-a4f2eed6]{\n  display: flex;\n}\n"),
    );
    assert_eq!(
        result.module_classes,
        vec![
            ("btn".to_string(), "btn_87199871".to_string()),
            ("card".to_string(), "card_ec3470e2".to_string()),
        ]
    );
}
