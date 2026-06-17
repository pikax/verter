//! Admission-honesty guard for the `#[ignore]`-row oracle-admissibility claims.
//!
//! The pre-existing manifest guard `every_manifest_row_unblocker_matches_live_ignore_reason`
//! pins each manifest row's `unblocker` to its live `#[ignore = "..."]` reason
//! byte-for-byte — it proves the manifest MIRRORS the source text, but it does
//! NOT prove the text is TRUE. A row could (and several did) cite an oracle
//! REJECTION the live two-sided admission gate never actually applies:
//!
//!   * `Reject(Callable)` — `RejectReason::Callable` is declared but NEVER
//!     constructed; the gate admits a clean callable per-signature (it only
//!     rejects via the callable's constituent constructs — `any` param, `keyof`
//!     return, …). A row blaming "the oracle rejects (Reject(Callable))" cites a
//!     blocker the engine cannot emit.
//!   * "`unknown[]` carries UnknownKeyword" — `unknown` / `unknown[]` is ON the
//!     positive allowlist (admitted); the real blocker for `Parameters<any>` is
//!     the SOURCE-side `any` type argument (`AnyKeyword`), not the result.
//!
//! This module closes that hole. For every `#[ignore]` row that makes an
//! oracle-admissibility claim it MEASURES the LIVE two-sided admission verdict
//! through the ONE shared resolver (tsgo-free: the SOURCE side via
//! [`resolve_source_declarations`] + [`admit_source_walk`], the VALUE side via
//! `resolve_named_symbol_with_audit` + `project_node_to_type_expr` +
//! [`admit_type_expr`] — the exact pair `gen::preflight_reduces_clean` and
//! `admit_query` compose, minus the tsgo hover the reducible rows make
//! redundant) and asserts:
//!
//!   1. the measured combined verdict EQUALS the row's declared verdict
//!      (`AdmitsBothSides` ⇒ the engine admits both sides; `Rejects(r)` ⇒ the
//!      engine rejects with exactly `r`) — the claim is GROUNDED in the engine,
//!      it cannot be rubber-stamped;
//!   2. the row's live `#[ignore]` text NAMES the real reason — a `Rejects(r)`
//!      row's text must carry `r`'s construct token, an `AdmitsBothSides` row's
//!      text must carry a liftability/admits marker and must NOT cite a phantom
//!      rejection.
//!
//! The registry pairs the five F9/F10 rows under correction with honest CONTROL
//! rows whose text already names a genuine rejection (`NeverKeyword`,
//! `AnyKeyword`, source `indexed-access`). The controls prove the guard
//! DISCRIMINATES — it is not a blanket "everything admits": a control's genuine
//! rejection is measured and required, so a regression that flattened the gate
//! to "always Admit" would fail the controls.

use std::sync::Arc;

use super::oracle;
use super::support::*;

use crate::resolver_core::{CanonicalCompletionOverlay, HostResolverContext};
use oracle::admission::{
    admit_source_walk, admit_type_expr, lower_hover_rhs, AdmissionVerdict, RejectReason,
};
use oracle::source_walk::{resolve_source_declarations, SourceLocator};
use verter_compiler::utils::oxc::script::raw_surface::SymbolSpace;

const MAPPED_TEMPLATE: &str = include_str!("fixtures/mapped_template.ts");
const INDEXED_UTILITIES: &str = include_str!("fixtures/indexed_utilities.ts");
const UTILITY_TOP_BOTTOM: &str = include_str!("fixtures/utility_top_bottom.ts");

/// The specific positive-allowlist reject reason a `Rejects` row's source/value
/// body produces. A CLOSED mapping to both the measured [`RejectReason`] and the
/// construct token the row's text must name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Expect {
    /// `Reject(DeferredConstruct("indexed-access"))` — a non-carve-out
    /// indexed-access source body (the index is not a same-file string-literal
    /// chain).
    IndexedAccess,
    /// `Reject(AnyKeyword)` — an `any` in a concrete position (here always the
    /// SOURCE-side `<any>` utility argument; the `unknown[]` RESULT is admitted).
    AnyKeyword,
    /// `Reject(NeverKeyword)` — a `never` result outside an empty union.
    NeverKeyword,
}

impl Expect {
    /// Whether the MEASURED verdict is exactly this rejection.
    fn matches(self, verdict: &AdmissionVerdict) -> bool {
        match (self, verdict) {
            (
                Expect::IndexedAccess,
                AdmissionVerdict::Reject(RejectReason::DeferredConstruct(s)),
            ) => *s == "indexed-access",
            (Expect::AnyKeyword, AdmissionVerdict::Reject(RejectReason::AnyKeyword)) => true,
            (Expect::NeverKeyword, AdmissionVerdict::Reject(RejectReason::NeverKeyword)) => true,
            _ => false,
        }
    }

    /// The construct token the row's `#[ignore]` text MUST contain so a reader
    /// can see the REAL blocker (not a phantom one).
    fn required_text_token(self) -> &'static str {
        match self {
            Expect::IndexedAccess => "indexed-access",
            Expect::AnyKeyword => "AnyKeyword",
            Expect::NeverKeyword => "NeverKeyword",
        }
    }
}

/// What a row's `#[ignore]` text CLAIMS about oracle admissibility — pinned to
/// BOTH the live engine (verdict) and the text (tokens).
#[derive(Clone, Copy, Debug)]
enum Claim {
    /// Both sides of the two-sided gate ADMIT the queried symbol: the row is NOT
    /// blocked by oracle admission. It is liftable modulo tsgo snapshot
    /// generation, OR ignored for a non-admission reason (shallow-publication).
    AdmitsBothSides,
    /// The two-sided gate REJECTS the queried symbol with this reason.
    Rejects(Expect),
}

/// One admission-claiming `#[ignore]` row plus how to drive its query.
struct Case {
    /// The `*.rs` test file (for reading the live `#[ignore]` reason).
    row_file: &'static str,
    /// The `#[ignore]`d test function.
    row_function: &'static str,
    /// The canonical path the test upserts the fixture under.
    canonical: &'static str,
    /// The fixture source.
    fixture: &'static str,
    /// The symbol the row's claim concerns (the test's queried alias).
    symbol: &'static str,
    /// The claim the row's text makes.
    claim: Claim,
}

const CASES: &[Case] = &[
    // ── F9: the two non-liftable template-literal/mapped rows (169, 170). ──
    // Row 168 (`record_with_template_literal_key_union_projects_root_slot`) was
    // the third member here; it is now oracle-LIFTED (its `AdmitsBothSides`
    // claim is proven by `oracle::run_row` against the checked-in tsgo snapshot),
    // so it left this ignored-admission-claim registry. The two rows below stay
    // `#[ignore]`d on genuine source-side indexed-access rejections.
    // 169 — StaticTemplateSlots[TemplateLiteralCellName]: the index is a Ref
    // (template-literal alias), NOT a string literal, so the source body is a
    // non-carve-out indexed access the SOURCE side rejects. (was: false
    // `Reject(Callable)`.)
    Case {
        row_file: "mapped_template.rs",
        row_function: "template_literal_key_alias_projects_static_template_slot",
        canonical: "/fixtures/mapped-template.ts",
        fixture: MAPPED_TEMPLATE,
        symbol: "NameCellRenderer",
        claim: Claim::Rejects(Expect::IndexedAccess),
    },
    // 170 — StaticTemplateSlots[`cell:${...}`]: the index is a template literal,
    // again a non-carve-out indexed-access source body. (was: false
    // `Reject(Callable)`.)
    Case {
        row_file: "mapped_template.rs",
        row_function: "template_literal_union_key_projects_static_slot_union",
        canonical: "/fixtures/mapped-template.ts",
        fixture: MAPPED_TEMPLATE,
        symbol: "StaticTemplateSlotUnion",
        claim: Claim::Rejects(Expect::IndexedAccess),
    },
    // 150 — NestedIndexedUtilitySurface publishes member-level refs SHALLOW, so
    // the direct surface ADMITs both sides; it is ignored for the
    // shallow-publication expectation mismatch, not a callable rejection. (was:
    // false `Reject(Callable)`.) Its terminal members' indexed-access source
    // bodies are the genuine post-member-demand oracle blocker — verified by the
    // NestedSubmitPayload / NestedFirstItem controls below.
    Case {
        row_file: "indexed_utilities.rs",
        row_function: "nested_indexed_utility_surface_resolves_all_terminal_members",
        canonical: "/fixtures/indexed-utilities.ts",
        fixture: INDEXED_UTILITIES,
        symbol: "NestedIndexedUtilitySurface",
        claim: Claim::AdmitsBothSides,
    },
    // Row 312 (`template_literal_key_remap_capitalises_each_event_key`,
    // `EventHandlers<"inc" | "dec">`) was also an `AdmitsBothSides` member here;
    // it is now oracle-LIFTED (proven by `oracle::run_row` against the checked-in
    // tsgo snapshot), so it left this registry too.
    // ── F10: the two `Parameters<any>`/`ConstructorParameters<any>` rows ──
    // 346 — the `unknown[]` RESULT is admitted; the real blocker is the SOURCE
    // `<any>` argument (`AnyKeyword`). (was: false "unknown[] carries
    // UnknownKeyword".)
    Case {
        row_file: "utility_top_bottom.rs",
        row_function: "utility_top_bottom_utb07_parameters_of_any_is_unknown_array",
        canonical: "/fixtures/utility_top_bottom.ts",
        fixture: UTILITY_TOP_BOTTOM,
        symbol: "Utb07ParametersOfAny",
        claim: Claim::Rejects(Expect::AnyKeyword),
    },
    // 348 — same source-`any` blocker for `ConstructorParameters<any>`. (was:
    // false "unknown[] carries UnknownKeyword".)
    Case {
        row_file: "utility_top_bottom.rs",
        row_function: "utility_top_bottom_utb11_constructor_parameters_any_is_unknown_array",
        canonical: "/fixtures/utility_top_bottom.ts",
        fixture: UTILITY_TOP_BOTTOM,
        symbol: "Utb11ConstructorParametersAny",
        claim: Claim::Rejects(Expect::AnyKeyword),
    },
    // ── Honest CONTROLS (already-correct text) — prove the guard discriminates ──
    // utb08: `Parameters<never>` = `never` — a genuine value-side NeverKeyword.
    Case {
        row_file: "utility_top_bottom.rs",
        row_function: "utility_top_bottom_utb08_parameters_of_never_is_never",
        canonical: "/fixtures/utility_top_bottom.ts",
        fixture: UTILITY_TOP_BOTTOM,
        symbol: "Utb08ParametersOfNever",
        claim: Claim::Rejects(Expect::NeverKeyword),
    },
    // utb12: `InstanceType<any>` = `any` — a genuine value-side AnyKeyword.
    Case {
        row_file: "utility_top_bottom.rs",
        row_function: "utility_top_bottom_utb12_instance_type_any_is_any",
        canonical: "/fixtures/utility_top_bottom.ts",
        fixture: UTILITY_TOP_BOTTOM,
        symbol: "Utb12InstanceTypeAny",
        claim: Claim::Rejects(Expect::AnyKeyword),
    },
    // utb13: `ConstructorParameters<new (...args: any[]) => any>` = `any[]` — a
    // genuine value-side AnyKeyword (the result itself carries `any`).
    Case {
        row_file: "utility_top_bottom.rs",
        row_function: "utility_top_bottom_utb13_constructor_parameters_any_ctor_is_any_array",
        canonical: "/fixtures/utility_top_bottom.ts",
        fixture: UTILITY_TOP_BOTTOM,
        symbol: "Utb13ConstructorParametersAnyCtor",
        claim: Claim::Rejects(Expect::AnyKeyword),
    },
    // NestedSubmitPayload: `Parameters<...>[0]` — a genuine source indexed-access
    // (the post-member-demand blocker row 150 cites).
    Case {
        row_file: "indexed_utilities.rs",
        row_function: "nested_parameters_nonnullable_indexed_payload_resolves",
        canonical: "/fixtures/indexed-utilities.ts",
        fixture: INDEXED_UTILITIES,
        symbol: "NestedSubmitPayload",
        claim: Claim::Rejects(Expect::IndexedAccess),
    },
    // NestedFirstItem: `NonNullable<...>[number]` — a genuine source indexed-access.
    Case {
        row_file: "indexed_utilities.rs",
        row_function: "nested_nonnullable_array_indexed_access_resolves_element",
        canonical: "/fixtures/indexed-utilities.ts",
        fixture: INDEXED_UTILITIES,
        symbol: "NestedFirstItem",
        claim: Claim::Rejects(Expect::IndexedAccess),
    },
];

/// Measure the LIVE two-sided admission verdict for `case.symbol`, source-first
/// (mirroring `admit_query`): the SOURCE walk's verdict if it rejects, else the
/// reduced VALUE's verdict. tsgo-free — the reducible rows make Verter's own
/// `Expanded` projection a faithful stand-in for the tsgo hover the value side
/// would otherwise parse.
fn measure(case: &Case) -> AdmissionVerdict {
    let host = make_host_with_footprint();
    upsert_ts(&host, case.canonical, case.fixture);

    // VALUE side: resolve + project + positive-allowlist predicate.
    let (value, _record) = resolve_expr(
        &host,
        case.canonical,
        case.symbol,
        &[],
        ProjectionMode::Expanded,
    );
    let value_verdict = admit_type_expr(&value);

    // SOURCE side: bind the declaration through the shared resolver and run the
    // two-sided source-walk admission over the real defining contributor(s).
    host.ensure_indexed_ready(case.canonical).expect("indexed");
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(CanonicalCompletionOverlay::new());
    let ctx = HostResolverContext::new(&host, &store_view, overlay);
    let locator = SourceLocator {
        reference_canonical: case.canonical.to_string(),
        reference_name: case.symbol.to_string(),
        symbol_space: SymbolSpace::Type,
    };
    let source_verdict = admit_source_walk(&resolve_source_declarations(&ctx, &locator));

    if matches!(source_verdict, AdmissionVerdict::Admit) {
        value_verdict
    } else {
        source_verdict
    }
}

/// The sibling test sources, embedded at COMPILE time (`include_str!`, not
/// `std::fs` — the resolver `src/` tree forbids runtime disk I/O, and embedding
/// makes the guard recompile + re-check whenever a row's `#[ignore]` text is
/// edited).
fn embedded_source(row_file: &str) -> &'static str {
    match row_file {
        "mapped_template.rs" => include_str!("mapped_template.rs"),
        "indexed_utilities.rs" => include_str!("indexed_utilities.rs"),
        "utility_top_bottom.rs" => include_str!("utility_top_bottom.rs"),
        other => panic!("admission-honesty registry references unembedded source {other:?}"),
    }
}

/// Read the live `#[ignore = "..."]` reason for `row_function` from the embedded
/// `src/typeinfo/typeinfo_tests/<row_file>` source.
fn live_ignore_reason(row_file: &str, row_function: &str) -> String {
    let source = embedded_source(row_file);
    let lines: Vec<&str> = source.lines().collect();
    let needle = format!("fn {row_function}(");
    for (i, line) in lines.iter().enumerate() {
        if !line.trim_start().starts_with("fn ") || !line.contains(&needle) {
            continue;
        }
        // Scan backward for the nearest `#[ignore = "..."]` attribute.
        for prev in lines[..i].iter().rev() {
            let t = prev.trim_start();
            if let Some(rest) = t.strip_prefix("#[ignore") {
                let rest = rest.trim_start();
                if let Some(after_eq) = rest.strip_prefix('=') {
                    if let Some(after_quote) = after_eq.trim_start().strip_prefix('"') {
                        if let Some(end) = after_quote.rfind('"') {
                            return after_quote[..end].to_string();
                        }
                    }
                }
                break;
            }
            // Stop at a blank line or another `fn`/closing brace boundary — the
            // attribute must be adjacent to the function.
            if t.is_empty() || t.starts_with("fn ") || t == "}" {
                break;
            }
        }
    }
    panic!("no `#[ignore = \"...\"]` reason found for {row_file}::{row_function}");
}

/// SELF-JUSTIFYING control: the live admission engine ADMITs the exact
/// constructs the corrected rows stop blaming — proving `Reject(Callable)` and
/// "unknown[] carries UnknownKeyword" are PHANTOM rejections the gate never
/// emits, which is what licenses the manifest-level phantom-citation ban
/// (`no_manifest_row_cites_a_phantom_oracle_reject_reason`).
#[test]
fn engine_admits_clean_callables_and_unknown_array() {
    let clean_fn = lower_hover_rhs("(payload: { value: string; column: \"name\" }) => string")
        .expect("clean function lowers");
    assert_eq!(
        admit_type_expr(&clean_fn),
        AdmissionVerdict::Admit,
        "a clean function value is admitted per-signature — the gate emits no \
         blanket Reject(Callable)",
    );

    let object_with_callables =
        lower_hover_rhs("{ onInc: (payload: \"inc\") => void; onDec: (payload: \"dec\") => void }")
            .expect("object with callable members lowers");
    assert_eq!(
        admit_type_expr(&object_with_callables),
        AdmissionVerdict::Admit,
        "an object whose members are clean callables is admitted — Reject(Callable) \
         is enum-only, never constructed",
    );

    let unknown_array = lower_hover_rhs("unknown[]").expect("unknown[] lowers");
    assert_eq!(
        admit_type_expr(&unknown_array),
        AdmissionVerdict::Admit,
        "`unknown[]` is on the positive allowlist — \"carries UnknownKeyword\" is \
         not a rejection the gate emits",
    );

    // Negative control: a callable is rejected via its CONSTITUENT construct,
    // never a blanket callable reject — `any` param ⇒ AnyKeyword.
    let dirty_fn = lower_hover_rhs("(x: any) => void").expect("lowers");
    assert_eq!(
        admit_type_expr(&dirty_fn),
        AdmissionVerdict::Reject(RejectReason::AnyKeyword),
        "a callable with an `any` param rejects as AnyKeyword (its constituent), \
         not Callable",
    );
}

/// The centerpiece: every admission-claiming `#[ignore]` row's text matches the
/// LIVE two-sided admission verdict measured through the shared resolver.
#[test]
fn oracle_admission_claim_rows_match_live_two_sided_verdict() {
    let mut violations: Vec<String> = Vec::new();

    for case in CASES {
        let verdict = measure(case);
        let reason = live_ignore_reason(case.row_file, case.row_function);

        match case.claim {
            Claim::AdmitsBothSides => {
                // (1) GROUNDING: the engine must actually admit both sides.
                if !matches!(verdict, AdmissionVerdict::Admit) {
                    violations.push(format!(
                        "{}::{}: declared AdmitsBothSides but the live two-sided gate \
                         measured {verdict:?}",
                        case.row_file, case.row_function
                    ));
                }
                // (2) TEXT: must mark liftability/admissibility, never a phantom.
                if !reason.to_lowercase().contains("admit") {
                    violations.push(format!(
                        "{}::{}: AdmitsBothSides row must state the live engine ADMITs \
                         (token \"admit\"); reason was {reason:?}",
                        case.row_file, case.row_function
                    ));
                }
                for phantom in ["Reject(Callable)", "UnknownKeyword"] {
                    if reason.contains(phantom) {
                        violations.push(format!(
                            "{}::{}: AdmitsBothSides row cites phantom blocker {phantom:?} \
                             the gate never emits; reason was {reason:?}",
                            case.row_file, case.row_function
                        ));
                    }
                }
            }
            Claim::Rejects(expect) => {
                // (1) GROUNDING: the engine must reject with exactly `expect`.
                if !expect.matches(&verdict) {
                    violations.push(format!(
                        "{}::{}: declared Rejects({expect:?}) but the live two-sided gate \
                         measured {verdict:?}",
                        case.row_file, case.row_function
                    ));
                }
                // (2) TEXT: must name the REAL reason's construct token.
                let token = expect.required_text_token();
                if !reason.contains(token) {
                    violations.push(format!(
                        "{}::{}: Rejects({expect:?}) row must name the real blocker token \
                         {token:?}; reason was {reason:?}",
                        case.row_file, case.row_function
                    ));
                }
                // (2b) and must not ALSO cite a phantom blocker.
                for phantom in ["Reject(Callable)", "UnknownKeyword"] {
                    if reason.contains(phantom) {
                        violations.push(format!(
                            "{}::{}: Rejects({expect:?}) row also cites phantom blocker \
                             {phantom:?}; reason was {reason:?}",
                            case.row_file, case.row_function
                        ));
                    }
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "admission-honesty: {} ignored-row admission claim(s) disagree with the LIVE \
         two-sided oracle-admission verdict. Each row's `#[ignore]` reason must name the \
         REAL blocker the shared resolver measures (or state the engine admits, for a \
         liftable row), never a phantom (`Reject(Callable)` / `UnknownKeyword`). \
         Violations:\n  {}",
        violations.len(),
        violations.join("\n  "),
    );
}
