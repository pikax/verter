//! Architecture guard: the `conformance-trace` feature is a side channel the
//! production Svelte IR never carries. HONEST SCOPE — this guard is
//! belt-and-suspenders over a closed set of structural facts; it is NOT a
//! whole-program zero-cost proof. The REAL zero-cost rail is the `#[cfg]`
//! gating plus the compiler itself: with the feature OFF the
//! `conformance_trace` module does not exist, so any ungated production
//! reference is a COMPILE ERROR.
//!
//! Verified rails:
//!
//! 1. **Prod-IR token absence**: the production IR / fact files (`ir.rs`,
//!    `css/types.rs`, `entity_decode.rs`) must never mention the trace at
//!    all — gated or not. TWO detectors, so a rename cannot dodge the scan:
//!    the literal `conformance` token, AND every type NAME the
//!    `conformance_trace` module itself declares (parsed live from its
//!    `pub struct` / `pub enum` declarations — a trace type MOVED into a
//!    prod file, even without the word "conformance", fails). Comments are
//!    stripped through the SHARED string-aware scanner, so prose mentions
//!    stay allowed. Types the trace module merely RE-EXPORTS (`pub use`,
//!    e.g. the production matcher's `MatchCertainty`) are deliberately NOT
//!    banned — they are production types.
//! 2. **Producer-boundary gating**: `pub mod conformance_trace;` in
//!    `runtime/mod.rs` must sit DIRECTLY under its exact
//!    `#[cfg(feature = "conformance-trace")]` attribute line — an ungated
//!    declaration would compile the module into every build. The check is
//!    EXACT-LINE (trimmed equality on both the declaration and its
//!    attribute), so neither a look-alike preceding line nor the expected
//!    text embedded in an unrelated expression / raw string satisfies it.
//! 3. **Layout + field-inventory proof** (runs in the DEFAULT feature-off
//!    suite): `StaticAttrValue` is exactly its decoded `String`
//!    (size-layout), AND the attribute-IR surface (`StaticAttrValue`,
//!    `AttrIr`, `MixedAttrPart`, `StyleDirectiveValue`) has a CLOSED field /
//!    variant inventory enforced by exhaustive `..`-free destructuring — a
//!    smuggled field fails to COMPILE even when it is a ZST or
//!    padding-neutral (a size check alone cannot see those). The feature-ON
//!    half of the size proof lives in the feature-gated lib test
//!    `prod_ir_static_attr_value_carries_no_trace_field_under_the_feature`.
//! 4. **Matcher-sink gating**: the trace-only `MatchSink.selector_certainties`
//!    row in `css/match.rs` must carry its exact
//!    `#[cfg(any(test, feature = "conformance-trace"))]` attribute, and the
//!    `MatchSink` field set is CLOSED — a new (even neutrally-named) field on
//!    the hot matcher sink fails the guard.
//! 5. **Single-pass provenance**: the trace module never invokes the entity
//!    decoder itself — provenance facts are EMITTED by the producer's single
//!    decode pass, never recovered by a second scan over the raw value.
//! 6. **Feature-off-by-construction manifest facts**: the EXECUTABLE
//!    feature-off evidence complementing these structural checks is the
//!    isolated CI gate in `.github/workflows/ci.yml` (`rust-test` job:
//!    `cargo build -p verter_compiler` + `cargo test -p verter_compiler
//!    --lib`) — the workspace test run UNIFIES features
//!    (`verter_svelte_conformance` dev-deps this crate with
//!    `features = ["conformance-trace"]`), so only the isolated
//!    `-p verter_compiler` invocation builds and tests the DEFAULT trace-free
//!    compiler. The manifest guard here keeps that isolation honest: the
//!    crate declares the `conformance-trace` feature, declares NO `default`
//!    feature set (a default set could transitively re-enable the trace and
//!    silently flip every `-p verter_compiler` build to feature-on), and
//!    carries no dev-dependency channel (`conformance-trace` mention or a
//!    `verter_svelte_conformance` dev-dep cycle) that would re-enable the
//!    trace for `-p verter_compiler` test builds.

mod svelte_guard_support;

use std::collections::BTreeSet;
use std::fs;
use std::mem::size_of;
use std::path::PathBuf;

use svelte_guard_support::strip_rust_comments;
use verter_compiler::svelte::runtime::ir::{
    AttrIr, MixedAttrPart, StaticAttrValue, StyleDirectiveValue,
};

/// The production IR / fact files that must never mention the trace.
const TRACE_FREE_PROD_FILES: &[&str] = &[
    "src/svelte/runtime/ir.rs",
    "src/svelte/runtime/css/types.rs",
    "src/svelte/runtime/entity_decode.rs",
];

/// The trace module whose DECLARED type names feed the second detector.
const TRACE_MODULE_FILE: &str = "src/svelte/runtime/conformance_trace.rs";

/// Anchor names the declared-type parser MUST find — a parser that silently
/// rots (returning an empty or shrunken set) fails here instead of quietly
/// weakening the scan.
const TRACE_TYPE_ANCHORS: &[&str] = &[
    "ConformanceTrace",
    "AttrProvenance",
    "AttrQuoting",
    "AttrSourceRepresentation",
    "SelectorCertaintyFact",
    "ScopedElementFact",
    "StyleMatchTrace",
];

/// The banned token: any reference to the conformance-trace side channel.
const BANNED_TOKEN: &str = "conformance";

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The verdict predicate (shared with the discrimination self-tests): does
/// this code — comments stripped — mention the trace?
fn mentions_trace(code: &str) -> bool {
    strip_rust_comments(code).contains(BANNED_TOKEN)
}

/// Every type name DECLARED by the trace module source (`pub struct X` /
/// `pub enum X`, comments stripped). `pub use` re-exports are NOT declared
/// types and are excluded by construction.
fn declared_trace_type_names(trace_src: &str) -> BTreeSet<String> {
    let stripped = strip_rust_comments(trace_src);
    let mut names = BTreeSet::new();
    for line in stripped.lines() {
        let trimmed = line.trim_start();
        for prefix in ["pub struct ", "pub enum "] {
            if let Some(rest) = trimmed.strip_prefix(prefix) {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if !name.is_empty() {
                    names.insert(name);
                }
            }
        }
    }
    names
}

/// Whether `code` contains `name` as a standalone identifier token (word
/// boundaries on both sides — `AttrQuoting` does not match inside
/// `MyAttrQuotingLike`).
fn contains_word(code: &str, name: &str) -> bool {
    let bytes = code.as_bytes();
    let mut start = 0;
    while let Some(found) = code[start..].find(name) {
        let begin = start + found;
        let end = begin + name.len();
        let boundary_before =
            begin == 0 || !(bytes[begin - 1].is_ascii_alphanumeric() || bytes[begin - 1] == b'_');
        let boundary_after =
            end >= bytes.len() || !(bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_');
        if boundary_before && boundary_after {
            return true;
        }
        start = begin + 1;
    }
    false
}

/// The generalized verdict: the trace-declared type names present in `code`
/// (comments stripped), as violations. Empty = clean.
fn trace_type_mentions(code: &str, trace_types: &BTreeSet<String>) -> Vec<String> {
    let stripped = strip_rust_comments(code);
    trace_types
        .iter()
        .filter(|name| contains_word(&stripped, name))
        .cloned()
        .collect()
}

#[test]
fn prod_ir_files_never_mention_the_conformance_trace() {
    let root = crate_root();
    let trace_src = fs::read_to_string(root.join(TRACE_MODULE_FILE))
        .unwrap_or_else(|e| panic!("the trace module {TRACE_MODULE_FILE} must read: {e}"));
    let trace_types = declared_trace_type_names(&trace_src);
    for anchor in TRACE_TYPE_ANCHORS {
        assert!(
            trace_types.contains(*anchor),
            "the declared-type parser lost the known trace type `{anchor}` — \
             update TRACE_TYPE_ANCHORS alongside a real rename, never by \
             weakening the parser"
        );
    }
    // Re-exports are production types, never banned tokens.
    assert!(
        !trace_types.contains("MatchCertainty"),
        "`MatchCertainty` is a production matcher type the trace RE-EXPORTS; \
         the declared-type parser must not classify re-exports as trace types"
    );

    let mut violations: Vec<String> = Vec::new();
    for rel in TRACE_FREE_PROD_FILES {
        let path = root.join(rel);
        let code = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("the prod file {rel} must exist and read: {e}"));
        assert!(
            !code.trim().is_empty(),
            "the prod file {rel} is empty — the scan set drifted"
        );
        if mentions_trace(&code) {
            violations.push(format!("{rel}: mentions `{BANNED_TOKEN}`"));
        }
        for name in trace_type_mentions(&code, &trace_types) {
            violations.push(format!("{rel}: mentions the trace-declared type `{name}`"));
        }
    }
    assert!(
        violations.is_empty(),
        "the conformance trace is a SIDE CHANNEL: production IR / fact files must \
         carry no trace state or trace references (gated or not). Violations:\n  {}",
        violations.join("\n  ")
    );
}

/// The gating verdict for the `conformance_trace` module declaration in
/// `runtime/mod.rs` source — `None` when correctly gated, `Some(violation)`
/// otherwise. EXACT-LINE discipline (comments stripped first): the
/// declaration line must trim-equal `pub mod conformance_trace;` exactly
/// once, and the nearest preceding non-blank line must trim-equal
/// `#[cfg(feature = "conformance-trace")]`. Trimmed EQUALITY (not
/// `contains`) means a raw-string / unrelated-expression line that merely
/// EMBEDS the expected text can never satisfy the check, and a second
/// ungated declaration cannot hide behind a gated first one.
fn module_gating_violation(code: &str) -> Option<String> {
    const DECL: &str = "pub mod conformance_trace;";
    const GATE: &str = "#[cfg(feature = \"conformance-trace\")]";
    let stripped = strip_rust_comments(code);
    let lines: Vec<&str> = stripped.lines().collect();
    let decl_lines: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.trim() == DECL)
        .map(|(i, _)| i)
        .collect();
    let decl_line = match decl_lines.as_slice() {
        [] => {
            return Some(format!(
                "no line trim-equals `{DECL}` — the module declaration moved or \
                 was reshaped; update the guard alongside a real move, never by \
                 weakening it"
            ));
        }
        [one] => *one,
        many => {
            return Some(format!(
                "{} lines trim-equal `{DECL}` — a duplicate declaration can \
                 smuggle an ungated copy",
                many.len()
            ));
        }
    };
    let preceding = lines[..decl_line]
        .iter()
        .rev()
        .find(|line| !line.trim().is_empty())
        .copied()
        .unwrap_or("");
    if preceding.trim() == GATE {
        None
    } else {
        Some(format!(
            "`{DECL}` must sit DIRECTLY under its exact `{GATE}` attribute line — \
             an ungated declaration compiles the trace into every build (found \
             preceding line: {preceding:?})"
        ))
    }
}

#[test]
fn conformance_trace_module_declaration_is_feature_gated() {
    let path = crate_root().join("src/svelte/runtime/mod.rs");
    let code = fs::read_to_string(&path).expect("runtime/mod.rs reads");
    if let Some(violation) = module_gating_violation(&code) {
        panic!("runtime/mod.rs: {violation}");
    }
}

// ─────────────────── single-pass provenance (no decoder re-scan) ───────────────────

/// The entity-decoder entry points the trace module must NEVER invoke:
/// provenance representation facts are EMITTED by the producer boundary's
/// SINGLE decode pass (the `DecodedAttrValue::decode` observer), so any
/// decoder call inside the trace module is a second lexical pass over bytes
/// the producer already scanned. (`decode_observing` is a banned
/// REINTRODUCTION name for a trace-side observed decode.)
const DECODER_ENTRY_POINTS: &[&str] = &[
    "decode_one_entity",
    "decode_entities",
    "decode_attr_entities",
    "decode_text_entities",
    "decode_observing",
    "DecodedAttrValue",
];

/// The decoder entry points `code` mentions (comments stripped, word
/// boundaries). Empty = clean.
fn decoder_mentions(code: &str) -> Vec<String> {
    let stripped = strip_rust_comments(code);
    DECODER_ENTRY_POINTS
        .iter()
        .filter(|name| contains_word(&stripped, name))
        .map(ToString::to_string)
        .collect()
}

#[test]
fn trace_module_never_invokes_the_entity_decoder() {
    let root = crate_root();
    let code = fs::read_to_string(root.join(TRACE_MODULE_FILE))
        .unwrap_or_else(|e| panic!("the trace module {TRACE_MODULE_FILE} must read: {e}"));
    let mentions = decoder_mentions(&code);
    assert!(
        mentions.is_empty(),
        "single-pass provenance: the trace module must READ producer-emitted \
         facts, never re-scan the raw value through the entity decoder. \
         Decoder mentions found in {TRACE_MODULE_FILE}: {mentions:?}"
    );
}

// ───────────────────────── MatchSink gating + closed field set ─────────────────────────

/// The production matcher source whose sink the gating check parses.
const MATCH_FILE: &str = "src/svelte/runtime/css/match.rs";

/// The CLOSED `MatchSink` field inventory: the three production verdict sets
/// plus the ONE trace/test-gated observability row.
const MATCH_SINK_PROD_FIELDS: &[&str] = &["used_selectors", "scoped_selectors", "scoped_elements"];
const MATCH_SINK_GATED_FIELD: &str = "selector_certainties";
const MATCH_SINK_GATE: &str = "#[cfg(any(test, feature = \"conformance-trace\"))]";

/// The `MatchSink` verdict over the matcher source — `None` when the struct
/// body carries EXACTLY the closed field set and the gated observability row
/// sits DIRECTLY under its exact `#[cfg(any(test, feature =
/// "conformance-trace"))]` attribute line; `Some(violation)` otherwise. A
/// neutrally-named extra field (ZST included — a size check cannot see one,
/// and `MatchSink` is private so no out-of-crate size check exists) fails
/// the closed-inventory arm; a de-gated `selector_certainties` fails the
/// attribute arm.
fn match_sink_violation(code: &str) -> Option<String> {
    let stripped = strip_rust_comments(code);
    let start = stripped.find("struct MatchSink")?;
    let open = start + stripped[start..].find('{')?;
    let mut depth = 0usize;
    let mut end = None;
    for (offset, ch) in stripped[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(open + offset);
                    break;
                }
            }
            _ => {}
        }
    }
    let body = &stripped[open + 1..end?];

    // Parse the body into (attribute-above, field-name) rows: an attribute
    // line applies to the next field line; field lines are `name: Type,`.
    let mut pending_attr: Option<String> = None;
    let mut fields: Vec<(Option<String>, String)> = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("#[") {
            pending_attr = Some(trimmed.to_string());
            continue;
        }
        if let Some(colon) = trimmed.find(':') {
            let name = trimmed[..colon].trim().trim_start_matches("pub ").trim();
            if name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !name.is_empty() {
                fields.push((pending_attr.take(), name.to_string()));
                continue;
            }
        }
        // Any other body line (a multi-line type, an expression) — the
        // parser is stricter than the language on purpose: fail closed.
        return Some(format!(
            "unparseable MatchSink body line {trimmed:?} — keep the sink's \
             field declarations single-line so the guard can verify them"
        ));
    }

    let expected: BTreeSet<&str> = MATCH_SINK_PROD_FIELDS
        .iter()
        .copied()
        .chain([MATCH_SINK_GATED_FIELD])
        .collect();
    let found: BTreeSet<&str> = fields.iter().map(|(_, name)| name.as_str()).collect();
    if found != expected {
        return Some(format!(
            "MatchSink field inventory drifted: expected exactly {expected:?}, \
             found {found:?} — the hot matcher sink is a CLOSED production \
             struct; new observability rows must be feature-gated AND added to \
             this guard deliberately"
        ));
    }
    let (gate, _) = fields
        .iter()
        .find(|(_, name)| name == MATCH_SINK_GATED_FIELD)
        .expect("inventory equality above guarantees the gated field row");
    if gate.as_deref() != Some(MATCH_SINK_GATE) {
        return Some(format!(
            "`MatchSink.{MATCH_SINK_GATED_FIELD}` must sit DIRECTLY under its \
             exact `{MATCH_SINK_GATE}` attribute line (found: {gate:?}) — an \
             ungated row ships trace state in every production matcher"
        ));
    }
    for (attr, name) in &fields {
        if name != MATCH_SINK_GATED_FIELD {
            if let Some(attr) = attr {
                return Some(format!(
                    "production MatchSink field `{name}` carries an unexpected \
                     attribute {attr:?} — production rows are attribute-free"
                ));
            }
        }
    }
    None
}

#[test]
fn match_sink_certainty_field_is_gated_and_inventory_closed() {
    let path = crate_root().join(MATCH_FILE);
    let code = fs::read_to_string(&path).expect("css/match.rs reads");
    if let Some(violation) = match_sink_violation(&code) {
        panic!("{MATCH_FILE}: {violation}");
    }
}

// ───────────────────── prod attribute-IR closed field inventory ─────────────────────

/// COMPILE-TIME field/variant inventory of the production attribute-IR
/// surface: exhaustive, `..`-free patterns fail to COMPILE when ANY field is
/// added (a ZST or padding-neutral field included — the size-layout proof
/// below cannot see those) or any variant appears. This is the
/// discrimination: smuggling `pub trace_marker: ()` onto `StaticAttrValue`
/// (or a new field/variant onto `AttrIr` / `MixedAttrPart` /
/// `StyleDirectiveValue`) breaks this guard's compilation, which fails the
/// default gate.
fn static_attr_value_inventory(v: &StaticAttrValue) {
    let StaticAttrValue { value: _ } = v;
}

fn attr_ir_inventory(attr: &AttrIr) {
    match attr {
        AttrIr::Static { name: _, value: _ }
        | AttrIr::Dynamic { name: _, expr: _ }
        | AttrIr::Mixed { name: _, parts: _ }
        | AttrIr::Spread { expr: _ }
        | AttrIr::Class {
            name: _,
            condition: _,
        }
        | AttrIr::Style {
            property: _,
            value: _,
            important: _,
        }
        | AttrIr::Bind { target: _, expr: _ }
        | AttrIr::Event {
            event_type: _,
            handler: _,
            delegated: _,
            capture: _,
            modifiers: _,
            passive: _,
            origin: _,
        }
        | AttrIr::Use { expr: _, arg: _ }
        | AttrIr::Transition {
            kind: _,
            name: _,
            expr: _,
            global: _,
        }
        | AttrIr::Animate { name: _, expr: _ }
        | AttrIr::Attach { expr: _ }
        | AttrIr::Let { name: _, expr: _ } => {}
    }
}

fn mixed_attr_part_inventory(part: &MixedAttrPart) {
    match part {
        MixedAttrPart::Literal(_) | MixedAttrPart::Expr(_) => {}
    }
}

fn style_directive_value_inventory(value: &StyleDirectiveValue) {
    match value {
        StyleDirectiveValue::Expr(_)
        | StyleDirectiveValue::Text(_)
        | StyleDirectiveValue::Mixed(_) => {}
    }
}

#[test]
fn prod_attr_ir_field_inventory_is_closed() {
    // The assertion is the COMPILATION of the exhaustive `..`-free inventory
    // functions above; anchoring them here keeps them live and documents
    // where the compile-time proof lives.
    let _: fn(&StaticAttrValue) = static_attr_value_inventory;
    let _: fn(&AttrIr) = attr_ir_inventory;
    let _: fn(&MixedAttrPart) = mixed_attr_part_inventory;
    let _: fn(&StyleDirectiveValue) = style_directive_value_inventory;
}

/// The DEFAULT-suite (feature-off) layout proof: `StaticAttrValue` is exactly
/// its decoded `String` — the trace added no field to the production IR.
#[test]
fn static_attr_value_is_exactly_its_decoded_string() {
    assert_eq!(
        size_of::<verter_compiler::svelte::runtime::ir::StaticAttrValue>(),
        size_of::<String>(),
        "StaticAttrValue must stay exactly its decoded String — no trace field"
    );
}

// ──────────────── feature-off-by-construction (manifest facts) ────────────────

/// Strip the TOML comment from one line: a `#` OUTSIDE a string starts a
/// comment; `#` inside a basic (`"…"`, `\`-escaped) or literal (`'…'`)
/// string is content.
fn strip_toml_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut in_basic = false;
    let mut in_literal = false;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' if in_basic => index += 1,
            b'"' if !in_literal => in_basic = !in_basic,
            b'\'' if !in_basic => in_literal = !in_literal,
            b'#' if !in_basic && !in_literal => return &line[..index],
            _ => {}
        }
        index += 1;
    }
    line
}

/// Whether a `[…]` section header names a dev-dependencies table (top-level,
/// keyed `[dev-dependencies.foo]`, or target-specific).
fn is_dev_deps_section(header: &str) -> bool {
    let inner = header
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim();
    inner == "dev-dependencies"
        || inner.starts_with("dev-dependencies.")
        || (inner.starts_with("target.")
            && (inner.ends_with(".dev-dependencies") || inner.contains(".dev-dependencies.")))
}

/// The manifest facts that keep the ISOLATED `-p verter_compiler` CI gate
/// genuinely feature-off, as violations over the Cargo.toml text (comments
/// stripped string-aware). Empty = clean. Three rules:
///
/// - `[features]` DECLARES `conformance-trace` (a rename must update this
///   guard deliberately, never silently orphan it);
/// - `[features]` declares NO `default` key at all — a default set can
///   transitively re-enable the trace and silently flips EVERY bare
///   `-p verter_compiler` invocation to feature-on (add a default set only
///   together with a deliberate review of this guard);
/// - no dev-dependencies table mentions `conformance-trace` or depends on
///   `verter_svelte_conformance` — cargo permits DEV-dep cycles, and either
///   channel would build `-p verter_compiler` test targets feature-on.
fn manifest_feature_off_violations(manifest: &str) -> Vec<String> {
    let mut violations: Vec<String> = Vec::new();
    let mut section = String::new();
    let mut trace_feature_declared = false;
    for raw_line in manifest.lines() {
        let line = strip_toml_comment(raw_line).trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            section = line.to_string();
            continue;
        }
        if section == "[features]" {
            if let Some((key, _)) = line.split_once('=') {
                let key = key.trim().trim_matches('"');
                if key == "conformance-trace" {
                    trace_feature_declared = true;
                }
                if key == "default" {
                    violations.push(format!(
                        "[features] declares a `default` set (`{line}`): verter_compiler \
                         declares NO default features — a default set can transitively \
                         re-enable `conformance-trace` and silently flips every bare \
                         `-p verter_compiler` build to feature-on"
                    ));
                }
            }
        } else if is_dev_deps_section(&section) {
            if line.contains("conformance-trace") {
                violations.push(format!(
                    "{section} mentions `conformance-trace` (`{line}`): a dev-dependency \
                     must never re-enable the trace for `-p verter_compiler` test builds"
                ));
            }
            if line.contains("verter_svelte_conformance") {
                violations.push(format!(
                    "{section} depends on `verter_svelte_conformance` (`{line}`): the \
                     dev-dep cycle would build `-p verter_compiler` test targets with \
                     `conformance-trace` unified ON"
                ));
            }
        }
    }
    if !trace_feature_declared {
        violations.push(
            "[features] no longer declares `conformance-trace` — update this guard \
             alongside a real rename, never by orphaning it"
                .to_string(),
        );
    }
    violations
}

/// The isolated CI gate (`cargo build -p verter_compiler` + `cargo test -p
/// verter_compiler --lib`) is feature-off BY CONSTRUCTION only while the
/// manifest holds these facts; this test pins them against the live
/// Cargo.toml.
#[test]
fn manifest_keeps_the_default_build_feature_off_by_construction() {
    let manifest =
        fs::read_to_string(crate_root().join("Cargo.toml")).expect("verter_compiler Cargo.toml");
    let violations = manifest_feature_off_violations(&manifest);
    assert!(
        violations.is_empty(),
        "the DEFAULT `-p verter_compiler` build must stay conformance-trace-OFF by \
         construction (the isolated CI feature-off gate depends on it). Violations:\n  {}",
        violations.join("\n  ")
    );
}

// ───────────────────────── discrimination self-tests ─────────────────────────

#[test]
fn detector_flags_a_trace_field_and_a_trace_import() {
    // A trace field smuggled onto a prod IR struct.
    assert!(mentions_trace(
        "pub struct StaticAttrValue { pub value: DecodedAttrValue, pub conformance_provenance: u8 }"
    ));
    // A trace import in a prod file (even feature-gated — prod IR files must
    // not reference the side channel at all).
    assert!(mentions_trace(
        "#[cfg(feature = \"conformance-trace\")]\nuse super::conformance_trace::AttrQuoting;"
    ));
}

#[test]
fn detector_ignores_comment_mentions() {
    assert!(!mentions_trace(
        "// provenance is captured by the conformance trace, NOT stored here\nfn ok() {}"
    ));
    assert!(!mentions_trace(
        "/* see conformance_trace.rs for the side channel */ fn ok() {}"
    ));
}

/// The type-name detector catches a trace type SMUGGLED into prod code even
/// when the word "conformance" never appears — a trace struct moved
/// wholesale into a prod IR file, or a field typed with a trace type through
/// a renamed import path.
#[test]
fn detector_flags_a_trace_type_without_the_conformance_token() {
    let trace_types: BTreeSet<String> = ["AttrQuoting", "StyleMatchTrace"]
        .iter()
        .map(ToString::to_string)
        .collect();

    // The moved-in definition: no "conformance" token anywhere.
    let moved = "pub enum AttrQuoting { Quoted, Unquoted }\n\
                 pub struct StaticAttrValue { pub value: String, pub quoting: AttrQuoting }";
    assert!(
        !mentions_trace(moved),
        "control: the literal token is absent"
    );
    assert_eq!(
        trace_type_mentions(moved, &trace_types),
        vec!["AttrQuoting".to_string()]
    );

    // Word boundaries: a LOOK-ALIKE identifier is not a violation.
    let lookalike = "pub struct MyAttrQuotingLike { pub x: u8 } fn attr_quoting_like() {}";
    assert!(trace_type_mentions(lookalike, &trace_types).is_empty());

    // Comment prose mentioning a trace type stays allowed.
    let prose = "// quoting provenance lives on StyleMatchTrace, NOT here\nfn ok() {}";
    assert!(trace_type_mentions(prose, &trace_types).is_empty());
}

/// The declared-type parser extracts exactly the `pub struct` / `pub enum`
/// declarations — never `pub use` re-exports, never private types.
#[test]
fn declared_type_parser_extracts_declarations_not_reexports() {
    let src = "pub use super::css::matcher::MatchCertainty;\n\
               pub struct AttrProvenance { pub name: String }\n\
               pub enum AttrQuoting { Quoted }\n\
               struct PrivateHelper;\n\
               // pub struct InComment (prose only)\n";
    let names = declared_trace_type_names(src);
    assert_eq!(
        names,
        ["AttrProvenance", "AttrQuoting"]
            .iter()
            .map(ToString::to_string)
            .collect::<BTreeSet<String>>()
    );
}

#[test]
fn scan_set_is_exactly_the_prod_ir_files() {
    let root = crate_root();
    for rel in TRACE_FREE_PROD_FILES {
        assert!(
            root.join(rel).is_file(),
            "prod file {rel} not found — update TRACE_FREE_PROD_FILES alongside the move"
        );
    }
    assert!(
        root.join(MATCH_FILE).is_file(),
        "matcher file {MATCH_FILE} not found — update MATCH_FILE alongside the move"
    );
}

/// The module-gating detector accepts EXACTLY the canonical two-line form
/// and rejects the fooling shapes a substring (`contains`) check on the
/// preceding line would accept: the expected attribute text embedded in a
/// RAW STRING on the preceding line, a look-alike `cfg` value, and a
/// duplicate declaration.
#[test]
fn module_gating_detector_rejects_fooling_shapes() {
    // The canonical form passes.
    let canonical = "#[cfg(feature = \"conformance-trace\")]\npub mod conformance_trace;\n";
    assert_eq!(module_gating_violation(canonical), None);

    // A comment between the attribute and the declaration is stripped and
    // leaves a blank line — still directly attached.
    let commented =
        "#[cfg(feature = \"conformance-trace\")]\n// side channel\npub mod conformance_trace;\n";
    assert_eq!(module_gating_violation(commented), None);

    // UNGATED declaration.
    assert!(module_gating_violation("pub mod conformance_trace;\n").is_some());

    // The expected attribute text embedded in a RAW STRING expression on the
    // preceding line — `contains` was satisfied, trimmed EQUALITY is not.
    let raw_string_fool = "const G: &str = r#\"#[cfg(feature = \"conformance-trace\")]\"#;\npub mod conformance_trace;\n";
    assert!(
        module_gating_violation(raw_string_fool).is_some(),
        "a raw string embedding the attribute text must not satisfy the gate"
    );

    // A look-alike cfg that CONTAINS the feature name but is not the gate.
    let lookalike =
        "#[cfg(any(feature = \"conformance-trace\", test))]\npub mod conformance_trace;\n";
    assert!(
        module_gating_violation(lookalike).is_some(),
        "only the exact single-feature gate is the canonical form"
    );

    // A duplicate declaration (one gated, one not) fails on multiplicity.
    let duplicated = "#[cfg(feature = \"conformance-trace\")]\npub mod conformance_trace;\nmod x;\npub mod conformance_trace;\n";
    assert!(module_gating_violation(duplicated).is_some());
}

/// The single-pass detector flags every decoder entry point (word-boundary,
/// comments stripped) and stays quiet on prose mentions and the
/// producer-emitted-fact vocabulary the trace module legitimately uses.
#[test]
fn decoder_mention_detector_discriminates() {
    // A re-scan loop over the raw value (the shape this guard exists to ban).
    assert_eq!(
        decoder_mentions("if let Some((_, consumed)) = decode_one_entity(&raw[i..], true) {}"),
        vec!["decode_one_entity".to_string()]
    );
    // Invoking the observed decode inside the trace module is STILL a second
    // pass — the producer already decoded.
    assert_eq!(
        decoder_mentions("let v = DecodedAttrValue::decode_observing(raw, &mut |_| {});"),
        vec![
            "decode_observing".to_string(),
            "DecodedAttrValue".to_string()
        ]
    );
    // Comment prose stays allowed.
    assert!(decoder_mentions(
        "// representation facts are emitted by the single decode pass\nfn ok() {}"
    )
    .is_empty());
    // Word boundaries: a look-alike identifier is not a decoder call.
    assert!(decoder_mentions("fn decode_entities_report_shape() {}").is_empty());
    // The producer-emitted fact type is NOT a decoder entry point.
    assert!(decoder_mentions("use super::entity_decode::EntityRefForm;").is_empty());
}

/// The manifest feature-off detector: the canonical clean shape passes; a
/// `default` set (with or without the trace, bare or quoted), a dev-dep
/// feature mention, and a dev-dep cycle through the conformance crate all
/// fail; commented-out shapes and non-dev-dep sections stay clean.
#[test]
fn manifest_feature_off_detector_discriminates() {
    let clean = "[package]\nname = \"verter_compiler\"\n\n[features]\nbench = []\nconformance-trace = []\n\n[dependencies]\nserde = { workspace = true }\n\n[dev-dependencies]\ntempfile = \"3\"\n";
    assert_eq!(manifest_feature_off_violations(clean), Vec::<String>::new());

    // A `default` set is a violation even WITHOUT the trace in it — the
    // no-default rule is deliberately transitive-proof.
    let defaulted = clean.replace("[features]\n", "[features]\ndefault = [\"bench\"]\n");
    assert!(
        manifest_feature_off_violations(&defaulted)
            .iter()
            .any(|v| v.contains("`default` set")),
        "a default feature set must fail"
    );

    // A default set carrying the trace, spelled with a QUOTED key.
    let quoted_default = clean.replace(
        "[features]\n",
        "[features]\n\"default\" = [\"conformance-trace\"]\n",
    );
    assert!(
        manifest_feature_off_violations(&quoted_default)
            .iter()
            .any(|v| v.contains("`default` set")),
        "a quoted default key must fail"
    );

    // A MULTI-LINE default array still fails: the key line alone is the fact.
    let multiline_default = clean.replace(
        "[features]\n",
        "[features]\ndefault = [\n  \"conformance-trace\",\n]\n",
    );
    assert!(
        manifest_feature_off_violations(&multiline_default)
            .iter()
            .any(|v| v.contains("`default` set")),
        "a multi-line default array must fail on its key line"
    );

    // A dev-dep re-enabling the feature.
    let dev_dep_feature = clean.replace(
        "[dev-dependencies]\n",
        "[dev-dependencies]\nsomething = { path = \"../x\", features = [\"conformance-trace\"] }\n",
    );
    assert!(
        manifest_feature_off_violations(&dev_dep_feature)
            .iter()
            .any(|v| v.contains("mentions `conformance-trace`")),
        "a dev-dep feature mention must fail"
    );

    // The dev-dep CYCLE through the conformance crate (cargo permits dev-dep
    // cycles; this one unifies the feature ON for `-p verter_compiler`).
    let dev_dep_cycle = clean.replace(
        "[dev-dependencies]\n",
        "[dev-dependencies]\nverter_svelte_conformance = { path = \"../verter_svelte_conformance\" }\n",
    );
    assert!(
        manifest_feature_off_violations(&dev_dep_cycle)
            .iter()
            .any(|v| v.contains("verter_svelte_conformance")),
        "a dev-dep cycle through the conformance crate must fail"
    );

    // A KEYED dev-dep table and a TARGET-specific dev-dep table are scanned.
    let keyed = clean.replace(
        "[dev-dependencies]\ntempfile = \"3\"\n",
        "[dev-dependencies.verter_svelte_conformance]\npath = \"../verter_svelte_conformance\"\n",
    );
    assert!(!manifest_feature_off_violations(&keyed).is_empty());
    let targeted = format!(
        "{clean}\n[target.'cfg(unix)'.dev-dependencies]\nx = {{ features = [\"conformance-trace\"] }}\n"
    );
    assert!(!manifest_feature_off_violations(&targeted).is_empty());

    // Commented-out shapes stay clean (string-aware `#` stripping).
    let commented = clean.replace(
        "[features]\n",
        "[features]\n# default = [\"conformance-trace\"]\n",
    );
    assert_eq!(
        manifest_feature_off_violations(&commented),
        Vec::<String>::new()
    );

    // The trace token in a NON-dev-dep section (prose in [package], a normal
    // dependency table) is not a re-enable channel.
    let prose = clean.replace(
        "[dependencies]\n",
        "description = \"ships the # conformance-trace feature\"\n\n[dependencies]\n",
    );
    assert_eq!(
        manifest_feature_off_violations(&prose),
        Vec::<String>::new()
    );

    // Losing the feature declaration entirely fails (the guard's subject).
    let undeclared = clean.replace("conformance-trace = []\n", "");
    assert!(
        manifest_feature_off_violations(&undeclared)
            .iter()
            .any(|v| v.contains("no longer declares")),
        "an orphaned guard must fail"
    );
}

/// The TOML comment stripper is string-aware: `#` inside basic and literal
/// strings is content, `\"` escapes do not terminate a basic string, and an
/// unquoted `#` cuts the line.
#[test]
fn toml_comment_stripper_discriminates() {
    assert_eq!(
        strip_toml_comment("key = \"value\" # comment"),
        "key = \"value\" "
    );
    assert_eq!(strip_toml_comment("key = \"a # b\""), "key = \"a # b\"");
    assert_eq!(strip_toml_comment("key = 'a # b'"), "key = 'a # b'");
    assert_eq!(
        strip_toml_comment("key = \"a \\\" # b\" # real"),
        "key = \"a \\\" # b\" "
    );
    assert_eq!(strip_toml_comment("# whole line"), "");
    assert_eq!(strip_toml_comment("plain = []"), "plain = []");
}

/// The dev-deps section classifier: every dev-dependencies table form is in,
/// look-alike dependency tables are out.
#[test]
fn dev_deps_section_classifier_discriminates() {
    assert!(is_dev_deps_section("[dev-dependencies]"));
    assert!(is_dev_deps_section("[dev-dependencies.foo]"));
    assert!(is_dev_deps_section("[target.'cfg(unix)'.dev-dependencies]"));
    assert!(is_dev_deps_section(
        "[target.'cfg(unix)'.dev-dependencies.foo]"
    ));
    assert!(!is_dev_deps_section("[dependencies]"));
    assert!(!is_dev_deps_section("[build-dependencies]"));
    assert!(!is_dev_deps_section("[dependencies.dev-dependencies-like]"));
    assert!(!is_dev_deps_section("[features]"));
}

/// The MatchSink detector: the committed shape passes; a smuggled
/// neutrally-named field, a de-gated certainty row, and an unexpected
/// attribute on a production row all fail.
#[test]
fn match_sink_detector_discriminates() {
    let canonical = "struct MatchSink {\n\
                     used_selectors: FxHashSet<Span>,\n\
                     scoped_selectors: FxHashSet<Span>,\n\
                     scoped_elements: FxHashSet<NodeId>,\n\
                     #[cfg(any(test, feature = \"conformance-trace\"))]\n\
                     selector_certainties: Vec<(Span, MatchCertainty)>,\n\
                     }\n";
    assert_eq!(match_sink_violation(canonical), None);

    // A smuggled ZST field with a NEUTRAL name — invisible to both the token
    // scan (no trace vocabulary) and a size assertion (zero-sized).
    let smuggled = canonical.replace(
        "used_selectors: FxHashSet<Span>,",
        "used_selectors: FxHashSet<Span>,\nobservability_marker: (),",
    );
    assert!(
        match_sink_violation(&smuggled).is_some_and(|v| v.contains("observability_marker")),
        "a neutrally-named extra field must fail the closed inventory"
    );

    // The certainty row DE-GATED (attribute dropped).
    let degated = canonical.replace("#[cfg(any(test, feature = \"conformance-trace\"))]\n", "");
    assert!(
        match_sink_violation(&degated).is_some_and(|v| v.contains("selector_certainties")),
        "an ungated certainty row must fail the attribute arm"
    );

    // A WRONG gate (test-only would drop the trace from the feature build;
    // feature-only would compile it into plain `cargo test` matcher tests).
    let wrong_gate = canonical.replace(
        "#[cfg(any(test, feature = \"conformance-trace\"))]",
        "#[cfg(test)]",
    );
    assert!(match_sink_violation(&wrong_gate).is_some());

    // An attribute on a PRODUCTION row.
    let attributed = canonical.replace(
        "scoped_elements: FxHashSet<NodeId>,",
        "#[cfg(feature = \"conformance-trace\")]\nscoped_elements: FxHashSet<NodeId>,",
    );
    assert!(match_sink_violation(&attributed).is_some());
}
