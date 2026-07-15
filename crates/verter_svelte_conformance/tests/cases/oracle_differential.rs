//! The DISPOSITION-AWARE oracle differential over the committed corpus,
//! scoped to the CSS-CONFORMANCE axes — never the full codegen topology
//! (that is the runtime corpus's differential job; a non-CSS codegen diff is
//! out of scope here).
//!
//! # Backend framing (honest scope)
//!
//! Verter's Svelte pipeline is CLIENT-ONLY — there is no Verter server/SSR
//! compile path. The differential is therefore exactly two comparisons, never
//! a pretended two-backend Verter diff:
//!
//! 1. **The Verter↔official CLIENT differential** — Verter's compiled client
//!    output against the committed OFFICIAL CLIENT golden
//!    (`corpus/goldens/<slug>.client.json`).
//! 2. **The official backend-independence INVARIANT** — CSS scoping is
//!    backend-independent in the official compiler, so the committed SERVER
//!    golden's css payload (and a rejected golden's diagnostic) must equal
//!    the CLIENT golden's. This is a golden-vs-golden pin on the OFFICIAL
//!    artifacts: it keeps the server golden a real, verified pin without
//!    fabricating a Verter server comparison. A violation here is golden
//!    corruption or an oracle semantics change — always a hard FAIL, never
//!    ledgerable. A future Verter SSR path adds a real server differential
//!    then.
//!
//! # Per-disposition method
//!
//! - **Supported** — compile the fixture with Verter (mirroring the client
//!   golden's compile options) and compare against the CLIENT golden:
//!   1. the scoped `css` payload, normalized EXACTLY as the goldens are
//!      (`{present, hash, code}` with every `svelte-<hash>` occurrence in
//!      `code` masked to `svelte-<scoped>` and `hash` = the first observable
//!      token, `None` when none survives pruning);
//!   2. the scoped-class TOPOLOGY: every golden client template whose html
//!      carries the masked `svelte-<scoped>` class must appear VERBATIM in
//!      Verter's masked module code (official scoped the element ⇒ Verter
//!      scoped the same element in the same emitted template payload);
//!   3. the SCOPE-TOKEN DELIVERY signature over the golden `clientModule` vs
//!      Verter's masked module: every masked scope-token occurrence,
//!      classified by its carrier — a complete-string runtime hash argument
//!      (`$.attribute_effect(..., 'svelte-<scoped>')`,
//!      `$.set_class(..., 'svelte-<scoped>')`, the injected `$$css` `hash:`),
//!      an embedded css-text payload, template-literal html, or unquoted —
//!      must agree in kind and count. This is what pins the RUNTIME-carried
//!      delivery the dynamic / `svelte:element` / injected cases use (their
//!      `templates` inventory carries no token), without absorbing general
//!      runtime-codegen topology: only scope-token occurrences are compared;
//!   4. the INJECTED `$$css` payload: the `{hash, code}` object the injected
//!      route hoists must agree between the golden `clientModule` and
//!      Verter's masked module — a declared-`Injected` case must carry
//!      exactly one on BOTH sides (empty-`code` after full pruning included),
//!      a declared-`External` case none;
//!   5. Verter's matcher tri-state: the observed [`MatchCertainty`] pattern
//!      must be consistent with the fixture's DECLARED `expected_outcome`
//!      (the shared `common::expected_certainty_pattern` expectation).
//! - **Refused** — UNINHABITED (`RefusalKind` has no variants): every
//!   officially-compilable covering cell is Supported, and the empty match in
//!   the disposition arm keeps the rail closed (a future refusal kind must
//!   land with its own typed evidence rule + self-tests).
//! - **Oracle-rejected** — the official goldens are the captured diagnostic
//!   (`{rejected: true, diagnostic}`, backend-independent); Verter must ALSO
//!   reject, with the diagnostic identity (`code` equality) of the CLIENT
//!   golden. Official-rejects-but-Verter-accepts (and vice-versa) is a hard
//!   FAIL.
//!
//! Divergence policy: a genuine CLIENT-differential divergence on a supported
//! fixture is NEVER a weakened assertion — it lands as a typed
//! [`KNOWN_DIVERGENCES`] ledger row (per-fixture `(slug, axis)`; every Verter
//! comparison is against the client golden today) and is reported for
//! defect-vs-cosmetic classification. A STALE row (one that no longer
//! diverges) FAILS the gate, so the ledger can only shrink truthfully. The
//! ledger starts — and currently stays — EMPTY.
//!
//! Hermetic: committed goldens + in-process Verter lowering only (no node,
//! no live official compiler; the live-oracle reconciliation is the JS
//! `--check` job).

use crate::common;

use std::collections::{BTreeMap, BTreeSet};

use common::{assert_no_violations, case_runtime_options, corpus_root, expected_certainty_pattern};
use oxc_allocator::Allocator;
use serde::Deserialize;
use verter_compiler::svelte::parser::parse_svelte;
use verter_compiler::svelte::runtime::conformance_trace::{
    compile_client_with_conformance_trace, ConformanceTrace, MatchCertainty,
};
use verter_compiler::svelte::runtime::{
    ClientCompileError, ClientModule, UnsupportedSvelteRuntimeSurface,
};
use verter_svelte_conformance::manifest::{manifest, ManifestCase};
use verter_svelte_conformance::model::{CompileTarget, CssSource, Disposition, MatchOutcome};

/// The committed corpus size this differential runs over — the shared
/// test-side pin (`common/case_count.rs`). A manifest change legitimately
/// moves it, in lockstep across every conformance gate.
use common::case_count::CASE_COUNT;

/// The goldens' scope-hash mask token.
const SCOPE_MASK: &str = "svelte-<scoped>";

// ---------------------------------------------------------------------------
// The committed golden schema (CLOSED per disposition: every field mandatory,
// unknown fields rejected — a drifted golden is a schema violation here, not
// a silent skip).
// ---------------------------------------------------------------------------

/// A compiled (supported / refused) golden. The non-CSS codegen fields are
/// SCHEMA-CAPTURE only (mandatory + closed, so a drifted golden fails the
/// parse) — this differential never compares them; full codegen topology is
/// out of its CSS-conformance scope.
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct CompiledGolden {
    slug: String,
    backend: String,
    oracle_version: String,
    #[allow(dead_code)]
    imports: serde_json::Value,
    #[allow(dead_code)]
    export_default: serde_json::Value,
    #[allow(dead_code)]
    helper_sequence: serde_json::Value,
    #[allow(dead_code)]
    helper_set: serde_json::Value,
    #[allow(dead_code)]
    helper_counts: serde_json::Value,
    #[allow(dead_code)]
    delegated_events: serde_json::Value,
    templates: Vec<GoldenTemplate>,
    /// The FULL normalized official client module (client goldens only; the
    /// server golden carries `null`) — the scope-token delivery oracle.
    client_module: Option<String>,
    /// Closed-schema capture of the normalized semantic-comment topology.
    /// Full comment-topology comparison belongs to the compiler conformance
    /// gate; this CSS-focused differential still requires the field so a
    /// generator/schema drift cannot pass silently.
    #[allow(dead_code)]
    semantic_comment_signature: serde_json::Value,
    css: GoldenCss,
}

/// One extracted template row of a compiled golden.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GoldenTemplate {
    factory: String,
    html: String,
    /// Schema-capture only (the template-flag axis is codegen topology).
    #[allow(dead_code)]
    flag: serde_json::Value,
}

/// The normalized scoped-css payload of a compiled golden — the shape the
/// Verter side re-derives via [`verter_css_payload`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct GoldenCss {
    present: bool,
    hash: Option<String>,
    code: Option<String>,
}

/// An oracle-rejected golden: the captured official diagnostic.
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RejectedGolden {
    slug: String,
    backend: String,
    oracle_version: String,
    rejected: bool,
    diagnostic: GoldenDiagnostic,
}

/// The official diagnostic identity of a rejected golden.
#[derive(Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct GoldenDiagnostic {
    code: String,
    message: String,
}

/// A committed golden, disposition-discriminated by shape.
#[derive(Deserialize)]
#[serde(untagged)]
enum Golden {
    Rejected(RejectedGolden),
    Compiled(Box<CompiledGolden>),
}

/// Read + parse one committed golden.
fn read_golden(slug: &str, backend: CompileTarget) -> Golden {
    let path = corpus_root()
        .join("goldens")
        .join(format!("{slug}.{}.json", backend.id()));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("committed golden {} reads: {error}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|error| {
        panic!(
            "golden {} matches a closed disposition schema: {error}",
            path.display()
        )
    })
}

// ---------------------------------------------------------------------------
// Scope-hash masking (the Rust mirror of the goldens' normalization: every
// `svelte-` + non-empty `[0-9a-z]+` run becomes `svelte-<scoped>`).
// ---------------------------------------------------------------------------

/// Mask every scope-hash token, exactly as the committed goldens are masked.
fn mask_scope_hash(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        if text[i..].starts_with("svelte-") {
            let start = i + "svelte-".len();
            let mut end = start;
            while end < bytes.len()
                && (bytes[end].is_ascii_lowercase() || bytes[end].is_ascii_digit())
            {
                end += 1;
            }
            if end > start {
                out.push_str(SCOPE_MASK);
                i = end;
                continue;
            }
        }
        // `svelte-` is ASCII, so advancing by one full char never splits a
        // candidate token start.
        let ch = text[i..].chars().next().expect("in-bounds char");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// The first observable scope-hash token of a text, or `None` (the goldens'
/// `hash` rule: pruning can leave no observable token).
fn extract_scope_hash(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if text[i..].starts_with("svelte-") {
            let start = i + "svelte-".len();
            let mut end = start;
            while end < bytes.len()
                && (bytes[end].is_ascii_lowercase() || bytes[end].is_ascii_digit())
            {
                end += 1;
            }
            if end > start {
                return Some(text[i..end].to_string());
            }
        }
        let ch = text[i..].chars().next().expect("in-bounds char");
        i += ch.len_utf8();
    }
    None
}

/// Normalize Verter's scoped-css artifact into the goldens' `{present, hash,
/// code}` shape (`None` artifact ⇔ `present: false`, exactly the official
/// external-vs-injected routing the goldens record).
fn verter_css_payload(module: &ClientModule) -> GoldenCss {
    match &module.css {
        None => GoldenCss {
            present: false,
            hash: None,
            code: None,
        },
        Some(css) => GoldenCss {
            present: true,
            hash: extract_scope_hash(&css.code),
            code: Some(mask_scope_hash(&css.code)),
        },
    }
}

// ---------------------------------------------------------------------------
// Scope-token delivery: a JS-string-aware scan of a MASKED module classifying
// every scope-token occurrence by its carrier. Token-scoped by construction —
// nothing but `svelte-<scoped>` occurrences is compared, so the signature
// pins CSS scope-token DELIVERY without absorbing codegen topology.
// ---------------------------------------------------------------------------

/// The carrier of one masked scope-token occurrence.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum ScopeTokenCarrier {
    /// A complete single-/double-quoted JS string literal that IS exactly the
    /// token — the runtime hash-argument delivery shape (a trailing
    /// `$.attribute_effect` / `$.set_class` hash argument, the `$$css`
    /// `hash:` value).
    HashStringArgument,
    /// The token embedded inside a LONGER quoted string — the injected
    /// css-text payload shape (`code: '.x.svelte-<scoped> {…}'`).
    EmbeddedInStringPayload,
    /// The token inside template-literal TEXT (the static
    /// `$.from_html(`…`)` html path; interpolated `${…}` code is scanned as
    /// code, not template text).
    TemplateHtml,
    /// The token outside any string literal (no emitter produces this today;
    /// a one-sided appearance is still a divergence).
    Unquoted,
}

/// The per-carrier occurrence counts of the masked scope token in a MASKED
/// JS module. Both sides of the differential run through this same scan, so
/// cosmetic formatting differences (whitespace, statement layout) cannot
/// move the signature — only token deliveries can.
fn scope_token_signature(masked_code: &str) -> BTreeMap<ScopeTokenCarrier, usize> {
    let mut signature: BTreeMap<ScopeTokenCarrier, usize> = BTreeMap::new();
    let mut count = |carrier: ScopeTokenCarrier, occurrences: usize| {
        if occurrences > 0 {
            *signature.entry(carrier).or_insert(0) += occurrences;
        }
    };
    let occurrences_in = |text: &str| text.matches(SCOPE_MASK).count();

    #[derive(Clone, Copy, PartialEq)]
    enum State {
        /// Top-level code, or code inside a `${…}` interpolation (the
        /// carried depth tracks nested braces within the interpolation).
        Code {
            interpolation_depth: Option<u32>,
        },
        Single,
        Double,
        Template,
    }
    let mut stack: Vec<State> = vec![State::Code {
        interpolation_depth: None,
    }];
    // Literal-text accumulators for nested template literals.
    let mut template_texts: Vec<String> = Vec::new();
    // Content accumulator for the innermost quoted string.
    let mut string_text = String::new();

    let bytes = masked_code.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let state = *stack.last().expect("scanner stack is never empty");
        match state {
            State::Code {
                interpolation_depth,
            } => {
                match bytes[i] {
                    b'\'' => {
                        stack.push(State::Single);
                        string_text.clear();
                    }
                    b'"' => {
                        stack.push(State::Double);
                        string_text.clear();
                    }
                    b'`' => {
                        stack.push(State::Template);
                        template_texts.push(String::new());
                    }
                    b'{' if interpolation_depth.is_some() => {
                        let top = stack.last_mut().expect("non-empty");
                        *top = State::Code {
                            interpolation_depth: interpolation_depth.map(|d| d + 1),
                        };
                    }
                    b'}' if interpolation_depth.is_some() => {
                        let depth = interpolation_depth.expect("checked");
                        if depth == 0 {
                            // Interpolation closed — back to the template.
                            stack.pop();
                        } else {
                            let top = stack.last_mut().expect("non-empty");
                            *top = State::Code {
                                interpolation_depth: Some(depth - 1),
                            };
                        }
                    }
                    _ => {
                        if masked_code[i..].starts_with(SCOPE_MASK) {
                            count(ScopeTokenCarrier::Unquoted, 1);
                            i += SCOPE_MASK.len();
                            continue;
                        }
                    }
                }
                i += 1;
            }
            State::Single | State::Double => {
                let close = if state == State::Single { b'\'' } else { b'"' };
                if bytes[i] == b'\\' {
                    // Escapes never spell the mask token (it has no escape
                    // form) — record verbatim and skip the escaped char.
                    string_text.push('\\');
                    i += 1;
                    if i < bytes.len() {
                        let ch = masked_code[i..].chars().next().expect("in-bounds");
                        string_text.push(ch);
                        i += ch.len_utf8();
                    }
                } else if bytes[i] == close {
                    let occurrences = occurrences_in(&string_text);
                    if string_text == SCOPE_MASK {
                        count(ScopeTokenCarrier::HashStringArgument, 1);
                    } else {
                        count(ScopeTokenCarrier::EmbeddedInStringPayload, occurrences);
                    }
                    string_text.clear();
                    stack.pop();
                    i += 1;
                } else {
                    let ch = masked_code[i..].chars().next().expect("in-bounds");
                    string_text.push(ch);
                    i += ch.len_utf8();
                }
            }
            State::Template => {
                if bytes[i] == b'\\' {
                    let text = template_texts.last_mut().expect("template text open");
                    text.push('\\');
                    i += 1;
                    if i < bytes.len() {
                        let ch = masked_code[i..].chars().next().expect("in-bounds");
                        text.push(ch);
                        i += ch.len_utf8();
                    }
                } else if bytes[i] == b'`' {
                    let text = template_texts.pop().expect("template text open");
                    count(ScopeTokenCarrier::TemplateHtml, occurrences_in(&text));
                    stack.pop();
                    i += 1;
                } else if bytes[i] == b'$' && bytes.get(i + 1) == Some(&b'{') {
                    stack.push(State::Code {
                        interpolation_depth: Some(0),
                    });
                    i += 2;
                } else {
                    let ch = masked_code[i..].chars().next().expect("in-bounds");
                    template_texts
                        .last_mut()
                        .expect("template text open")
                        .push(ch);
                    i += ch.len_utf8();
                }
            }
        }
    }
    signature
}

/// One hoisted injected-css payload (`const $$css = {hash: '…', code: '…'}`),
/// extracted from a MASKED module.
#[derive(Clone, Debug, Eq, PartialEq)]
struct InjectedCssPayload {
    hash: String,
    code: String,
}

/// Read the JS string literal starting at byte `i` (which must be a quote),
/// returning its content and the byte index just past the closing quote.
fn read_js_string_at(code: &str, i: usize) -> Option<(String, usize)> {
    let bytes = code.as_bytes();
    let quote = *bytes.get(i)?;
    if quote != b'\'' && quote != b'"' {
        return None;
    }
    let mut content = String::new();
    let mut j = i + 1;
    while j < bytes.len() {
        if bytes[j] == b'\\' {
            content.push('\\');
            j += 1;
            if j < bytes.len() {
                let ch = code[j..].chars().next().expect("in-bounds");
                content.push(ch);
                j += ch.len_utf8();
            }
        } else if bytes[j] == quote {
            return Some((content, j + 1));
        } else {
            let ch = code[j..].chars().next().expect("in-bounds");
            content.push(ch);
            j += ch.len_utf8();
        }
    }
    None
}

/// The string value of `key: '…'` inside an object-literal text, or `None`
/// when the key or its string value is absent.
fn object_string_property(object_text: &str, key: &str) -> Option<String> {
    let bytes = object_text.as_bytes();
    let needle = format!("{key}:");
    let mut start = 0;
    while let Some(found) = object_text[start..].find(&needle) {
        let key_at = start + found;
        // Key position: at the object start or after `{`, `,`, or whitespace.
        let boundary = key_at == 0 || matches!(bytes[key_at - 1], b'{' | b',' | b' ' | b'\t');
        if boundary {
            let mut value_at = key_at + needle.len();
            while value_at < bytes.len() && bytes[value_at].is_ascii_whitespace() {
                value_at += 1;
            }
            if let Some((content, _)) = read_js_string_at(object_text, value_at) {
                return Some(content);
            }
        }
        start = key_at + needle.len();
    }
    None
}

/// Every hoisted `$$css = {…}` payload of a MASKED module, in source order.
/// A malformed payload object (missing `hash`/`code` string values) is
/// reported as an error string so the caller can surface it as a violation
/// rather than silently comparing nothing.
fn injected_css_payloads(masked_code: &str) -> Result<Vec<InjectedCssPayload>, String> {
    let mut payloads = Vec::new();
    let bytes = masked_code.as_bytes();
    let mut start = 0;
    while let Some(found) = masked_code[start..].find("$$css") {
        let at = start + found;
        start = at + "$$css".len();
        // Identifier boundary (skip `$$cssX` look-alikes and the `$$css`
        // USES like `$.append_styles($$anchor, $$css)`).
        let before_ok = at == 0
            || !(bytes[at - 1].is_ascii_alphanumeric()
                || bytes[at - 1] == b'_'
                || bytes[at - 1] == b'$');
        let mut j = at + "$$css".len();
        if !before_ok
            || (j < bytes.len()
                && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_' || bytes[j] == b'$'))
        {
            continue;
        }
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        if j >= bytes.len() || bytes[j] != b'=' {
            continue;
        }
        j += 1;
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        if j >= bytes.len() || bytes[j] != b'{' {
            continue;
        }
        // The matching close brace, string-aware.
        let object_start = j;
        let mut depth = 0u32;
        let mut end = None;
        while j < bytes.len() {
            match bytes[j] {
                b'\'' | b'"' => {
                    let (_, next) = read_js_string_at(masked_code, j)
                        .ok_or_else(|| "unterminated string in a $$css payload".to_string())?;
                    j = next;
                    continue;
                }
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(j + 1);
                        break;
                    }
                }
                _ => {}
            }
            j += 1;
        }
        let end = end.ok_or_else(|| "unterminated $$css payload object".to_string())?;
        let object_text = &masked_code[object_start..end];
        let hash = object_string_property(object_text, "hash")
            .ok_or_else(|| format!("$$css payload without a string `hash`: {object_text}"))?;
        let code = object_string_property(object_text, "code")
            .ok_or_else(|| format!("$$css payload without a string `code`: {object_text}"))?;
        payloads.push(InjectedCssPayload { hash, code });
        start = end;
    }
    Ok(payloads)
}

// ---------------------------------------------------------------------------
// The divergence ledger.
// ---------------------------------------------------------------------------

/// One LEDGERABLE CSS-conformance comparison axis of the CLIENT differential
/// (supported fixtures only; every Verter comparison is against the client
/// golden — Verter has no server pipeline). A DISPOSITION contradiction or a
/// backend-independence violation on the official goldens is deliberately
/// NOT an axis: those are always hard FAILs.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum DivergenceAxis {
    /// The normalized scoped-css payload (`{present, hash, code}`).
    CssPayload,
    /// The scoped-class topology (golden template html ⊆ Verter module).
    ScopedClassTopology,
    /// The per-carrier scope-token delivery signature (golden `clientModule`
    /// vs Verter's masked module).
    ScopeTokenDelivery,
    /// The hoisted injected `$$css` payload objects.
    InjectedCssPayload,
    /// The matcher tri-state vs the declared expected outcome.
    MatcherCertainty,
}

/// One ledgered, KNOWN Verter↔official divergence: tolerated by the gate,
/// reported for defect-vs-cosmetic classification, and guarded against
/// staleness (a row that no longer diverges FAILS).
#[derive(Clone, Copy, Debug)]
struct KnownDivergence {
    /// The fixture slug.
    slug: &'static str,
    /// The diverging axis (of the CLIENT differential).
    axis: DivergenceAxis,
    /// Why the divergence is tolerated (review-facing).
    #[allow(dead_code)]
    reason: &'static str,
}

/// The ledger. EMPTY: every committed supported fixture currently agrees
/// with its official client golden on every CSS-conformance axis. Add a row
/// ONLY for a genuine, reviewed divergence — never to mute a regression.
const KNOWN_DIVERGENCES: &[KnownDivergence] = &[];

/// The ledger key of an observed divergence.
type DivergenceKey = (String, DivergenceAxis);

/// Route one observed divergence: ledgered ⇒ tolerated (recorded for the
/// staleness check), otherwise a violation.
fn route_divergence(
    key: DivergenceKey,
    detail: String,
    observed: &mut BTreeSet<DivergenceKey>,
    violations: &mut Vec<String>,
) {
    let ledgered = KNOWN_DIVERGENCES
        .iter()
        .any(|row| row.slug == key.0 && row.axis == key.1);
    if ledgered {
        observed.insert(key);
    } else {
        violations.push(detail);
    }
}

// ---------------------------------------------------------------------------
// Per-disposition comparisons.
// ---------------------------------------------------------------------------

/// Non-vacuity facts of one supported-case comparison (tallied corpus-wide so
/// the new axes can never silently go vacuous).
#[derive(Clone, Copy, Debug, Default)]
struct SupportedAxisFacts {
    /// The official client golden delivers the scope token through a
    /// RUNTIME-carried shape (a complete-string hash argument or an embedded
    /// css payload — not template html).
    official_runtime_delivery: bool,
    /// The golden's `templates` inventory carries NO scope token while its
    /// `clientModule` does — the dynamic / `svelte:element` / injected class
    /// the template axis alone can never see.
    template_token_free_module_delivery: bool,
    /// The official client golden hoists ≥ 1 injected `$$css` payload.
    official_injected_payload: bool,
}

/// Compare one SUPPORTED fixture's Verter CLIENT compile against its official
/// CLIENT golden on the CSS-conformance axes.
fn check_supported_against_golden(
    case: &ManifestCase,
    golden: &CompiledGolden,
    module: &ClientModule,
    observed: &mut BTreeSet<DivergenceKey>,
    violations: &mut Vec<String>,
) -> SupportedAxisFacts {
    let mut facts = SupportedAxisFacts::default();

    // Axis 1: the normalized scoped-css payload.
    let verter_css = verter_css_payload(module);
    if verter_css != golden.css {
        route_divergence(
            (case.slug.clone(), DivergenceAxis::CssPayload),
            format!(
                "{} CssPayload: official {:?} vs Verter {:?}",
                case.slug, golden.css, verter_css
            ),
            observed,
            violations,
        );
    }

    // Axis 2: the scoped-class topology — every official template that
    // carries the scoped class must appear verbatim in Verter's masked
    // module (static scoped-class emission parity on the matched elements).
    let masked_module = mask_scope_hash(&module.code);
    let templates_carry_token = golden
        .templates
        .iter()
        .any(|template| template.html.contains(SCOPE_MASK));
    for template in &golden.templates {
        if !template.html.contains(SCOPE_MASK) {
            continue;
        }
        if !masked_module.contains(&template.html) {
            route_divergence(
                (case.slug.clone(), DivergenceAxis::ScopedClassTopology),
                format!(
                    "{} ScopedClassTopology: official scoped template {:?} (factory `{}`) \
                     absent from Verter's masked module",
                    case.slug, template.html, template.factory
                ),
                observed,
                violations,
            );
        }
    }

    // Axes 3 + 4 read the golden's clientModule — mandatory on a client
    // golden (the generator writes `null` only for the server backend).
    let Some(client_module) = &golden.client_module else {
        violations.push(format!(
            "{} golden integrity: a client golden must carry `clientModule` \
             (the scope-token delivery oracle)",
            case.slug
        ));
        return facts;
    };

    // Axis 3: the per-carrier scope-token delivery signature.
    let official_signature = scope_token_signature(client_module);
    let verter_signature = scope_token_signature(&masked_module);
    if official_signature != verter_signature {
        route_divergence(
            (case.slug.clone(), DivergenceAxis::ScopeTokenDelivery),
            format!(
                "{} ScopeTokenDelivery: official {:?} vs Verter {:?}",
                case.slug, official_signature, verter_signature
            ),
            observed,
            violations,
        );
    }
    facts.official_runtime_delivery = official_signature
        .keys()
        .any(|carrier| *carrier != ScopeTokenCarrier::TemplateHtml);
    facts.template_token_free_module_delivery =
        !templates_carry_token && !official_signature.is_empty();

    // Axis 4: the injected `$$css` payload objects, plus the declared-cell
    // presence rule (Injected ⇒ exactly one payload on BOTH sides — an
    // all-pruned empty `code` still ships the payload; External ⇒ none).
    let official_payloads = match injected_css_payloads(client_module) {
        Ok(payloads) => payloads,
        Err(error) => {
            violations.push(format!(
                "{} golden integrity: malformed $$css payload in clientModule: {error}",
                case.slug
            ));
            return facts;
        }
    };
    let verter_payloads = match injected_css_payloads(&masked_module) {
        Ok(payloads) => payloads,
        Err(error) => {
            violations.push(format!(
                "{} InjectedCssPayload: malformed $$css payload in Verter's module: {error}",
                case.slug
            ));
            return facts;
        }
    };
    let expected_payloads = match case.levels.css_source {
        CssSource::Injected => 1,
        CssSource::External => 0,
    };
    if official_payloads.len() != expected_payloads {
        violations.push(format!(
            "{} golden integrity: declared {:?} css expects {expected_payloads} hoisted \
             $$css payload(s) in the official clientModule, found {}",
            case.slug,
            case.levels.css_source,
            official_payloads.len()
        ));
    }
    if verter_payloads != official_payloads {
        route_divergence(
            (case.slug.clone(), DivergenceAxis::InjectedCssPayload),
            format!(
                "{} InjectedCssPayload: official {:?} vs Verter {:?}",
                case.slug, official_payloads, verter_payloads
            ),
            observed,
            violations,
        );
    }
    facts.official_injected_payload = !official_payloads.is_empty();

    facts
}

/// The official backend-independence INVARIANT on a compiled golden pair:
/// CSS scoping is backend-independent in the official compiler, so the
/// server golden's normalized css payload must equal the client golden's.
/// Golden-vs-golden only (Verter has no server pipeline) — a violation is a
/// hard FAIL, never ledgerable.
fn check_compiled_backend_independence(
    slug: &str,
    client: &CompiledGolden,
    server: &CompiledGolden,
    violations: &mut Vec<String>,
) {
    if client.css != server.css {
        violations.push(format!(
            "{slug} backend-independence: the official SERVER golden's css payload \
             {:?} must equal the official CLIENT golden's {:?} (CSS scoping is \
             backend-independent; a mismatch is golden corruption or an oracle \
             semantics change)",
            server.css, client.css
        ));
    }
}

/// The official backend-independence INVARIANT on a rejected golden pair:
/// the captured diagnostic identity must not depend on the backend.
fn check_rejected_backend_independence(
    slug: &str,
    client: &RejectedGolden,
    server: &RejectedGolden,
    violations: &mut Vec<String>,
) {
    if client.diagnostic != server.diagnostic {
        violations.push(format!(
            "{slug} backend-independence: the official SERVER golden's diagnostic \
             ({}, {:?}) must equal the official CLIENT golden's ({}, {:?})",
            server.diagnostic.code,
            server.diagnostic.message,
            client.diagnostic.code,
            client.diagnostic.message
        ));
    }
}

/// Assert Verter's matcher tri-state is consistent with the fixture's
/// DECLARED expected outcome (the shared per-cell certainty expectation).
fn check_matcher_certainty(
    case: &ManifestCase,
    trace: &ConformanceTrace,
    observed: &mut BTreeSet<DivergenceKey>,
    violations: &mut Vec<String>,
) {
    let observed_pattern: Option<Vec<MatchCertainty>> = match trace.style_matches.as_slice() {
        [style] => Some(
            style
                .selector_certainties
                .iter()
                .map(|fact| fact.certainty)
                .collect(),
        ),
        _ => None,
    };
    let expected = expected_certainty_pattern(case);
    if observed_pattern.as_deref() != Some(expected.as_slice()) {
        route_divergence(
            (case.slug.clone(), DivergenceAxis::MatcherCertainty),
            format!(
                "{} MatcherCertainty: declared outcome {:?} expects {expected:?}, observed {:?}",
                case.slug, case.expected_outcome, observed_pattern
            ),
            observed,
            violations,
        );
    }
}

// ---------------------------------------------------------------------------
// The differential gate.
// ---------------------------------------------------------------------------

#[test]
fn verter_agrees_with_official_goldens_on_css_conformance_axes() {
    let manifest = manifest();
    let root = corpus_root();
    let mut violations: Vec<String> = Vec::new();
    let mut observed: BTreeSet<DivergenceKey> = BTreeSet::new();
    let (mut supported_seen, mut oracle_rejected_seen) = (0usize, 0usize);
    let mut compiled_count = 0usize;
    let (mut runtime_delivery_cases, mut token_free_module_delivery_cases, mut injected_cases) =
        (0usize, 0usize, 0usize);

    for case in manifest.cases() {
        let path = root.join("fixtures").join(format!("{}.svelte", case.slug));
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("committed fixture {} reads: {error}", path.display()));

        // ONE Verter compile per fixture — the CLIENT module (Verter's Svelte
        // pipeline is client-only; the server golden is pinned by the
        // backend-independence invariant, never by a Verter compile).
        let allocator = Allocator::default();
        let parsed = parse_svelte(&source);
        let options = case_runtime_options(case);
        let (result, trace) =
            compile_client_with_conformance_trace(&source, &parsed, &options, &allocator, false);
        compiled_count += 1;

        let client_golden = read_golden(&case.slug, CompileTarget::Client);
        let server_golden = read_golden(&case.slug, CompileTarget::Server);

        match case.disposition {
            Disposition::Supported => {
                supported_seen += 1;
                let module = match &result {
                    Ok(module) => module,
                    Err(error) => {
                        violations.push(format!(
                            "{} Disposition: official compiles (supported) but Verter failed: \
                             {error:?}",
                            case.slug
                        ));
                        continue;
                    }
                };
                let compiled_pair = match (&client_golden, &server_golden) {
                    (Golden::Compiled(client), Golden::Compiled(server)) => Some((client, server)),
                    _ => {
                        for (backend, golden) in [
                            (CompileTarget::Client, &client_golden),
                            (CompileTarget::Server, &server_golden),
                        ] {
                            if matches!(golden, Golden::Rejected(_)) {
                                violations.push(format!(
                                    "{} [{}] Disposition: declared Supported but the official \
                                     golden is a rejection capture",
                                    case.slug,
                                    backend.id()
                                ));
                            }
                        }
                        None
                    }
                };
                if let Some((client, server)) = compiled_pair {
                    check_golden_identity(
                        &case.slug,
                        CompileTarget::Client,
                        client,
                        &mut violations,
                    );
                    check_golden_identity(
                        &case.slug,
                        CompileTarget::Server,
                        server,
                        &mut violations,
                    );
                    check_compiled_backend_independence(
                        &case.slug,
                        client,
                        server,
                        &mut violations,
                    );
                    let facts = check_supported_against_golden(
                        case,
                        client,
                        module,
                        &mut observed,
                        &mut violations,
                    );
                    runtime_delivery_cases += usize::from(facts.official_runtime_delivery);
                    token_free_module_delivery_cases +=
                        usize::from(facts.template_token_free_module_delivery);
                    injected_cases += usize::from(facts.official_injected_payload);
                }
                check_matcher_certainty(case, &trace, &mut observed, &mut violations);
            }
            // The refusal vocabulary is UNINHABITED — a declared Refused row is
            // impossible by construction (a future refusal kind must land with
            // its own typed evidence rule + observation arm here).
            Disposition::Refused(kind) => match kind {},
            Disposition::OracleRejected(kind) => {
                oracle_rejected_seen += 1;
                let rejected_pair = match (&client_golden, &server_golden) {
                    (Golden::Rejected(client), Golden::Rejected(server)) => Some((client, server)),
                    _ => {
                        for (backend, golden) in [
                            (CompileTarget::Client, &client_golden),
                            (CompileTarget::Server, &server_golden),
                        ] {
                            if matches!(golden, Golden::Compiled(_)) {
                                violations.push(format!(
                                    "{} [{}] Disposition: declared OracleRejected({kind:?}) but \
                                     the official golden is a compiled capture",
                                    case.slug,
                                    backend.id()
                                ));
                            }
                        }
                        None
                    }
                };
                if let Some((client, server)) = rejected_pair {
                    check_rejected_identity(
                        &case.slug,
                        CompileTarget::Client,
                        client,
                        &mut violations,
                    );
                    check_rejected_identity(
                        &case.slug,
                        CompileTarget::Server,
                        server,
                        &mut violations,
                    );
                    check_rejected_backend_independence(
                        &case.slug,
                        client,
                        server,
                        &mut violations,
                    );
                    // Verter must ALSO reject, with the MATCHING diagnostic
                    // identity (code equality against the CLIENT golden's
                    // captured official code — the server capture is pinned
                    // equal by the invariant above).
                    match &result {
                        Err(ClientCompileError::Unsupported(
                            UnsupportedSvelteRuntimeSurface::StyleCssAnalysis { code, .. },
                        )) if *code == client.diagnostic.code => {}
                        other => violations.push(format!(
                            "{} Disposition: official rejects with `{}` ({kind:?}) but Verter \
                             observed {other:?}",
                            case.slug, client.diagnostic.code
                        )),
                    }
                }
            }
            Disposition::Invalid(kind) => violations.push(format!(
                "{}: the manifest selected an Invalid({kind:?}) row as a case",
                case.slug
            )),
        }
    }

    // The differential ran Verter over the FULL committed corpus, with every
    // disposition partition exercised.
    assert_eq!(compiled_count, CASE_COUNT, "Verter compiled every fixture");
    assert_eq!(
        supported_seen + oracle_rejected_seen,
        CASE_COUNT,
        "every committed case must be compared"
    );
    assert!(supported_seen > 0, "no Supported case was compared");
    assert!(
        oracle_rejected_seen > 0,
        "no OracleRejected case was compared"
    );

    // The scope-token delivery axes are NON-VACUOUS over the committed
    // corpus: runtime-carried deliveries, the templates-token-free class the
    // finding targets (dynamic / svelte:element / injected), and injected
    // payloads must each be exercised.
    assert!(
        runtime_delivery_cases > 0,
        "no supported case delivered the scope token through a runtime carrier"
    );
    assert!(
        token_free_module_delivery_cases > 0,
        "no supported case carried the scope token in clientModule with token-free templates"
    );
    assert!(
        injected_cases > 0,
        "no supported case hoisted an injected $$css payload"
    );

    // Ledger staleness: every KNOWN_DIVERGENCES row must have been OBSERVED
    // to diverge in this run — a healed divergence must leave the ledger.
    for row in KNOWN_DIVERGENCES {
        let key = (row.slug.to_string(), row.axis);
        if !observed.contains(&key) {
            violations.push(format!(
                "stale KNOWN_DIVERGENCES row: {} {:?} no longer diverges — remove it",
                row.slug, row.axis
            ));
        }
    }

    assert_no_violations("oracle differential (CSS-conformance axes)", &violations);
}

/// The golden's own identity fields must agree with the artifact it was read
/// as (a mis-keyed golden would silently compare the wrong fixture).
fn check_golden_identity(
    slug: &str,
    backend: CompileTarget,
    golden: &CompiledGolden,
    violations: &mut Vec<String>,
) {
    if golden.slug != slug {
        violations.push(format!(
            "{slug} [{}] golden identity: slug field `{}` mismatches",
            backend.id(),
            golden.slug
        ));
    }
    if golden.backend != backend.id() {
        violations.push(format!(
            "{slug} [{}] golden identity: backend field `{}` mismatches",
            backend.id(),
            golden.backend
        ));
    }
    if golden.oracle_version.is_empty() {
        violations.push(format!(
            "{slug} [{}] golden identity: empty oracleVersion",
            backend.id()
        ));
    }
}

/// The rejected golden's identity + rejection marker integrity.
fn check_rejected_identity(
    slug: &str,
    backend: CompileTarget,
    golden: &RejectedGolden,
    violations: &mut Vec<String>,
) {
    if golden.slug != slug || golden.backend != backend.id() {
        violations.push(format!(
            "{slug} [{}] rejected-golden identity: ({}, {}) mismatches",
            backend.id(),
            golden.slug,
            golden.backend
        ));
    }
    if !golden.rejected {
        violations.push(format!(
            "{slug} [{}] rejected-golden integrity: `rejected` must be true",
            backend.id()
        ));
    }
    if golden.oracle_version.is_empty() || golden.diagnostic.message.is_empty() {
        violations.push(format!(
            "{slug} [{}] rejected-golden integrity: empty oracleVersion/diagnostic message",
            backend.id()
        ));
    }
}

// ---------------------------------------------------------------------------
// Committed RED self-tests: each mutates an in-memory golden/declaration and
// asserts the checker reports the expected violation — a checker silently
// weakened on one of the violation classes exercised below fails IN-TREE,
// without any out-of-tree plant recipe (unexercised classes rely on the
// out-of-tree plant recipes).
// ---------------------------------------------------------------------------

/// Compile one committed fixture through Verter's client pipeline.
fn compile_fixture(
    case: &ManifestCase,
) -> (Result<ClientModule, ClientCompileError>, ConformanceTrace) {
    let path = corpus_root()
        .join("fixtures")
        .join(format!("{}.svelte", case.slug));
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("committed fixture {} reads: {error}", path.display()));
    let allocator = Allocator::default();
    let parsed = parse_svelte(&source);
    let options = case_runtime_options(case);
    compile_client_with_conformance_trace(&source, &parsed, &options, &allocator, false)
}

/// The first manifest case satisfying `predicate` (deterministic: case order
/// is the manifest's ascending row order).
fn find_case(predicate: impl Fn(&ManifestCase) -> bool) -> &'static ManifestCase {
    manifest()
        .cases()
        .iter()
        .find(|case| predicate(case))
        .expect("the committed manifest carries a case for this self-test predicate")
}

/// Read + parse the CLIENT golden of a case as a compiled golden.
fn read_compiled_client_golden(case: &ManifestCase) -> CompiledGolden {
    match read_golden(&case.slug, CompileTarget::Client) {
        Golden::Compiled(golden) => *golden,
        Golden::Rejected(_) => panic!("{}: expected a compiled client golden", case.slug),
    }
}

/// The supported-checker violations of one (possibly mutated) golden against
/// the REAL compiled module of `case`.
fn supported_violations_against(case: &ManifestCase, golden: &CompiledGolden) -> Vec<String> {
    let (result, _trace) = compile_fixture(case);
    let module = result.expect("a supported fixture compiles");
    let mut observed = BTreeSet::new();
    let mut violations = Vec::new();
    check_supported_against_golden(case, golden, &module, &mut observed, &mut violations);
    violations
}

/// A mutated css payload on the client golden is reported on the CssPayload
/// axis (and the pristine golden reports nothing).
#[test]
fn self_test_mutated_css_payload_is_detected() {
    let case = find_case(|case| {
        case.disposition == Disposition::Supported
            && case.levels.css_source == CssSource::External
            && case.expected_outcome == MatchOutcome::Match
    });
    let mut golden = read_compiled_client_golden(case);
    assert!(
        supported_violations_against(case, &golden).is_empty(),
        "control: the pristine golden must report no violations"
    );

    golden.css.code = Some(format!(
        "{}.planted-divergence{{color:red}}",
        golden.css.code.clone().unwrap_or_default()
    ));
    let violations = supported_violations_against(case, &golden);
    assert!(
        violations.iter().any(|v| v.contains("CssPayload")),
        "a mutated golden css payload must be reported on the CssPayload axis: {violations:?}"
    );
}

/// A planted scoped template row absent from Verter's module is reported on
/// the ScopedClassTopology axis.
#[test]
fn self_test_planted_scoped_template_is_detected() {
    let case = find_case(|case| {
        case.disposition == Disposition::Supported
            && case.levels.css_source == CssSource::External
            && case.expected_outcome == MatchOutcome::Match
    });
    let mut golden = read_compiled_client_golden(case);
    golden.templates.push(GoldenTemplate {
        factory: "from_html".to_string(),
        html: format!("<div class=\"{SCOPE_MASK} planted\">planted</div>"),
        flag: serde_json::Value::Null,
    });
    let violations = supported_violations_against(case, &golden);
    assert!(
        violations.iter().any(|v| v.contains("ScopedClassTopology")),
        "a planted scoped template must be reported on the ScopedClassTopology axis: \
         {violations:?}"
    );
}

/// Stripping the runtime-carried scope token from the golden `clientModule`
/// (the dynamic / `svelte:element` delivery the templates inventory never
/// sees) is reported on the ScopeTokenDelivery axis — the committed RED
/// proof that the delivery comparison reads `clientModule`, not just
/// `templates`.
#[test]
fn self_test_stripped_client_module_token_delivery_is_detected() {
    let case = find_case(|case| {
        if case.disposition != Disposition::Supported {
            return false;
        }
        let golden = read_compiled_client_golden(case);
        let templates_token_free = golden
            .templates
            .iter()
            .all(|template| !template.html.contains(SCOPE_MASK));
        templates_token_free
            && golden
                .client_module
                .as_deref()
                .is_some_and(|module| module.contains(SCOPE_MASK))
    });
    let mut golden = read_compiled_client_golden(case);
    assert!(
        supported_violations_against(case, &golden).is_empty(),
        "control: the pristine golden must report no violations"
    );

    let stripped = golden
        .client_module
        .as_deref()
        .expect("selected golden carries clientModule")
        .replace(SCOPE_MASK, "unscoped");
    golden.client_module = Some(stripped);
    let violations = supported_violations_against(case, &golden);
    assert!(
        violations.iter().any(|v| v.contains("ScopeTokenDelivery")),
        "a clientModule stripped of its runtime-carried scope token must be reported \
         on the ScopeTokenDelivery axis: {violations:?}"
    );
}

/// A mutated injected `$$css` payload in the golden `clientModule` is
/// reported on the InjectedCssPayload axis.
#[test]
fn self_test_mutated_injected_css_payload_is_detected() {
    let case = find_case(|case| {
        if case.disposition != Disposition::Supported
            || case.levels.css_source != CssSource::Injected
        {
            return false;
        }
        read_compiled_client_golden(case)
            .client_module
            .as_deref()
            .is_some_and(|module| module.contains("code: ''"))
    });
    let mut golden = read_compiled_client_golden(case);
    assert!(
        supported_violations_against(case, &golden).is_empty(),
        "control: the pristine golden must report no violations"
    );

    let mutated = golden
        .client_module
        .as_deref()
        .expect("selected golden carries clientModule")
        .replace("code: ''", "code: '.planted-divergence{color:red}'");
    golden.client_module = Some(mutated);
    let violations = supported_violations_against(case, &golden);
    assert!(
        violations.iter().any(|v| v.contains("InjectedCssPayload")),
        "a mutated injected $$css payload must be reported on the InjectedCssPayload \
         axis: {violations:?}"
    );
}

/// A server golden whose css payload diverges from the client golden's
/// violates the backend-independence invariant (hard fail, not ledgerable).
#[test]
fn self_test_backend_independence_violation_is_detected() {
    let case = find_case(|case| {
        case.disposition == Disposition::Supported
            && case.levels.css_source == CssSource::External
            && case.expected_outcome == MatchOutcome::Match
    });
    let client = read_compiled_client_golden(case);
    let mut server = match read_golden(&case.slug, CompileTarget::Server) {
        Golden::Compiled(golden) => *golden,
        Golden::Rejected(_) => panic!("{}: expected a compiled server golden", case.slug),
    };

    let mut violations = Vec::new();
    check_compiled_backend_independence(&case.slug, &client, &server, &mut violations);
    assert!(
        violations.is_empty(),
        "control: the committed pair must satisfy the invariant"
    );

    server.css.code = Some(format!(
        "{}.planted-divergence{{color:red}}",
        server.css.code.clone().unwrap_or_default()
    ));
    check_compiled_backend_independence(&case.slug, &client, &server, &mut violations);
    assert!(
        violations
            .iter()
            .any(|v| v.contains("backend-independence")),
        "a diverging server css payload must violate the backend-independence \
         invariant: {violations:?}"
    );
}

/// A rejected golden pair whose diagnostics diverge violates the rejected
/// backend-independence invariant, and a falsified `rejected` marker fails
/// the identity integrity check.
#[test]
fn self_test_mutated_rejected_golden_is_detected() {
    let case = find_case(|case| matches!(case.disposition, Disposition::OracleRejected(_)));
    let (client, server) = match (
        read_golden(&case.slug, CompileTarget::Client),
        read_golden(&case.slug, CompileTarget::Server),
    ) {
        (Golden::Rejected(client), Golden::Rejected(server)) => (client, server),
        _ => panic!("{}: expected rejected goldens on both backends", case.slug),
    };

    let mut violations = Vec::new();
    check_rejected_identity(&case.slug, CompileTarget::Client, &client, &mut violations);
    check_rejected_backend_independence(&case.slug, &client, &server, &mut violations);
    assert!(
        violations.is_empty(),
        "control: the committed rejected pair must be clean"
    );

    // Diverging diagnostic code across backends.
    let mut skewed = server;
    skewed.diagnostic.code = format!("{}-planted", skewed.diagnostic.code);
    check_rejected_backend_independence(&case.slug, &client, &skewed, &mut violations);
    assert!(
        violations
            .iter()
            .any(|v| v.contains("backend-independence")),
        "a diverging rejected diagnostic must violate the invariant: {violations:?}"
    );

    // A falsified rejection marker.
    let mut unrejected = client;
    unrejected.rejected = false;
    let mut marker_violations = Vec::new();
    check_rejected_identity(
        &case.slug,
        CompileTarget::Client,
        &unrejected,
        &mut marker_violations,
    );
    assert!(
        marker_violations
            .iter()
            .any(|v| v.contains("`rejected` must be true")),
        "a falsified rejection marker must fail identity integrity: {marker_violations:?}"
    );
}

/// A contradicted declared outcome is reported on the MatcherCertainty axis:
/// the checker compares the REAL trace against the DECLARED cell, so flipping
/// the declaration must flip the verdict.
#[test]
fn self_test_contradicted_declared_outcome_is_detected() {
    let case = find_case(|case| {
        case.disposition == Disposition::Supported && case.expected_outcome == MatchOutcome::Match
    });
    let (_result, trace) = compile_fixture(case);

    let mut observed = BTreeSet::new();
    let mut violations = Vec::new();
    check_matcher_certainty(case, &trace, &mut observed, &mut violations);
    assert!(
        violations.is_empty(),
        "control: the declared cell must be consistent with the real trace"
    );

    let mut contradicted = case.clone();
    contradicted.levels.outcome = MatchOutcome::NoMatch;
    contradicted.expected_outcome = MatchOutcome::NoMatch;
    check_matcher_certainty(&contradicted, &trace, &mut observed, &mut violations);
    assert!(
        violations.iter().any(|v| v.contains("MatcherCertainty")),
        "a contradicted declared outcome must be reported on the MatcherCertainty \
         axis: {violations:?}"
    );
}

// ---------------------------------------------------------------------------
// scope_token_signature focused unit pins. The JS-string state machine FAILS
// OPEN on constructs it does not model (a blind spot would silently
// reclassify — and thereby mask — a delivery divergence), so every carrier
// shape the official/Verter emitters produce is pinned here directly: a
// scanner refactor that shifts any classification fails these before it can
// bend the differential.
// ---------------------------------------------------------------------------

/// Convenience: the signature as a sorted (carrier, count) list.
fn signature_of(code: &str) -> Vec<(ScopeTokenCarrier, usize)> {
    scope_token_signature(code).into_iter().collect()
}

#[test]
fn scope_token_signature_classifies_each_carrier_shape() {
    // A complete-string runtime hash argument (single- and double-quoted).
    assert_eq!(
        signature_of("$.set_class(div, 1, 'svelte-<scoped>');"),
        vec![(ScopeTokenCarrier::HashStringArgument, 1)]
    );
    assert_eq!(
        signature_of("$.attribute_effect(div, () => ({}), undefined, \"svelte-<scoped>\");"),
        vec![(ScopeTokenCarrier::HashStringArgument, 1)]
    );
    // The token embedded inside a LONGER string — the injected css payload
    // shape; and BOTH shapes together in one $$css object literal.
    assert_eq!(
        signature_of("const x = '.a.svelte-<scoped> {color:red}';"),
        vec![(ScopeTokenCarrier::EmbeddedInStringPayload, 1)]
    );
    assert_eq!(
        signature_of("const $$css = { hash: 'svelte-<scoped>', code: '.a.svelte-<scoped> {}' };"),
        vec![
            (ScopeTokenCarrier::HashStringArgument, 1),
            (ScopeTokenCarrier::EmbeddedInStringPayload, 1),
        ]
    );
    // Template-literal html text.
    assert_eq!(
        signature_of("var root = $.from_html(`<div class=\"a svelte-<scoped>\">x</div>`);"),
        vec![(ScopeTokenCarrier::TemplateHtml, 1)]
    );
    // Outside any literal.
    assert_eq!(
        signature_of("let svelte = svelte-<scoped>;"),
        vec![(ScopeTokenCarrier::Unquoted, 1)]
    );
    // No token, no signature.
    assert!(signature_of("$.set_class(div, 1, 'plain');").is_empty());
}

#[test]
fn scope_token_signature_scans_interpolations_as_code_not_template_text() {
    // A complete-string token inside `${…}` is CODE (a hash argument), not
    // template text.
    assert_eq!(
        signature_of("`<div class=\"${cls('svelte-<scoped>')}\">`"),
        vec![(ScopeTokenCarrier::HashStringArgument, 1)]
    );
    // Nested braces INSIDE the interpolation stay within code state.
    assert_eq!(
        signature_of("`x${fn({ key: 'svelte-<scoped>' })}y`"),
        vec![(ScopeTokenCarrier::HashStringArgument, 1)]
    );
    // A nested template literal inside an interpolation is template TEXT
    // again.
    assert_eq!(
        signature_of("`a${`b svelte-<scoped>`}c`"),
        vec![(ScopeTokenCarrier::TemplateHtml, 1)]
    );
    // Template text on BOTH sides of an interpolation accumulates into one
    // template count.
    assert_eq!(
        signature_of("`svelte-<scoped>${x}svelte-<scoped>`"),
        vec![(ScopeTokenCarrier::TemplateHtml, 2)]
    );
}

#[test]
fn scope_token_signature_handles_escapes_and_multiplicity() {
    // An escaped quote does not close the string: the token stays EMBEDDED
    // in the longer literal.
    assert_eq!(
        signature_of(r"var s = 'pre\' svelte-<scoped>';"),
        vec![(ScopeTokenCarrier::EmbeddedInStringPayload, 1)]
    );
    // Two occurrences inside ONE string are embedded (the literal is not
    // exactly the token), counted per occurrence.
    assert_eq!(
        signature_of("var s = 'svelte-<scoped> svelte-<scoped>';"),
        vec![(ScopeTokenCarrier::EmbeddedInStringPayload, 2)]
    );
    // Mixed carriers across one module accumulate independently.
    assert_eq!(
        signature_of(
            "var root = $.from_html(`<i class=\"svelte-<scoped>\"></i>`);\n\
             $.set_class(el, 1, 'svelte-<scoped>');\n\
             const css = '.x.svelte-<scoped>{}';"
        ),
        vec![
            (ScopeTokenCarrier::HashStringArgument, 1),
            (ScopeTokenCarrier::EmbeddedInStringPayload, 1),
            (ScopeTokenCarrier::TemplateHtml, 1),
        ]
    );
}
