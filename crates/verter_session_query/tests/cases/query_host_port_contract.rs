//! Contract proof for the [`QueryHostPort`] seam: a mock host implements
//! the port and a caller drives it GENERICALLY (through the trait, not the
//! concrete type), proving the seam is a real, usable inversion of control
//! — the query side programs against neutral locators, neutral typed IR,
//! and the neutral cache-admission signal only.
//!
//! Discriminating: the mock branches on the locator (known anchor → a
//! lowering echoing that anchor; unknown symbol in a served file → a typed
//! genuine miss; unknown producing canonical → the no-serve non-result)
//! and maps its serve-publication bit onto the admission signal for EVERY
//! outcome arm, so a seam that dropped the locator input, the lowering
//! payload, the typed error channel, or the admission signal on either
//! arm fails the assertions (or fails to compile).

use std::sync::Arc;

use verter_session_query::{
    AuthoredBodyLowering, AuthoredBodyShape, QueryHostAdmission, QueryHostError, QueryHostPort,
    QueryHostServe,
};
use verter_type_expr::locators::{
    AuthoredAnchor, AuthoredBodyLocator, LocatorSymbolSpace, TypeBodySlot, TypeParamVisibility,
};
use verter_type_expr::{PrimitiveName, TypeExpr, TypeParam};

const KNOWN_CANONICAL: &str = "/ws/props.ts";
const KNOWN_SYMBOL: &str = "Props";

/// Mock host mirroring the real host's per-path admission mapping:
///
/// * unknown producing canonical → NO serve happened, so there is no
///   publication status to map — [`QueryHostAdmission::ReturnOnly`] with
///   the transient [`QueryHostError::UnknownFile`] non-result;
/// * served file (the known canonical) → the serve-publication bit
///   (`store_published`) maps onto the admission signal for EVERY outcome
///   arm (`false` = a FENCED serve that published nothing), and the
///   outcome is the canned lowering for exactly one known type anchor or
///   the typed genuine miss for any other symbol.
struct MockHost {
    store_published: bool,
}

impl QueryHostPort for MockHost {
    fn lower_authored_body(&self, locator: &AuthoredBodyLocator) -> QueryHostServe {
        let anchor = match locator {
            AuthoredBodyLocator::DeclBody(slot) => &slot.anchor,
            AuthoredBodyLocator::AugmentationBody(aug) => &aug.anchor,
            AuthoredBodyLocator::JsdocTypedefBody(typedef) => &typedef.anchor,
            AuthoredBodyLocator::MacroPayload(payload) => &payload.anchor,
        };
        if anchor.canonical_id.as_ref() != KNOWN_CANONICAL {
            return QueryHostServe {
                admission: QueryHostAdmission::ReturnOnly,
                outcome: Err(QueryHostError::UnknownFile),
            };
        }
        let admission = QueryHostAdmission::from_store_published(self.store_published);
        let outcome = match locator {
            AuthoredBodyLocator::DeclBody(slot)
                if slot.anchor.symbol.as_ref() == KNOWN_SYMBOL
                    && slot.anchor.space == LocatorSymbolSpace::Type =>
            {
                Ok(AuthoredBodyLowering {
                    shape: AuthoredBodyShape::Single(TypeExpr::Primitive(PrimitiveName::String)),
                    type_parameters: vec![TypeParam {
                        name: "T".to_string(),
                        constraint: None,
                        default: None,
                    }],
                    visibility: TypeParamVisibility::Body,
                })
            }
            _ => Err(QueryHostError::UnknownSymbol),
        };
        QueryHostServe { admission, outcome }
    }
}

/// Drives the port through the TRAIT — the exact consumption shape of the
/// query layer, which never names the implementor's concrete type.
fn lower_via_port<P: QueryHostPort>(port: &P, locator: &AuthoredBodyLocator) -> QueryHostServe {
    port.lower_authored_body(locator)
}

/// The consumer-side warm-admission gate the admission signal exists for:
/// a value derived from the serve may enter a shared cache warm ONLY when
/// the serve is [`QueryHostAdmission::Cacheable`] AND the outcome's own
/// cache-semantics class is cacheable (a lowering, or a genuine
/// cacheable miss). The two axes are ANDed, never substituted for one
/// another.
fn may_admit_warm(serve: &QueryHostServe) -> bool {
    let outcome_class_is_cacheable = match &serve.outcome {
        Ok(_) => true,
        Err(error) => error.is_cacheable_miss(),
    };
    serve.admission == QueryHostAdmission::Cacheable && outcome_class_is_cacheable
}

fn decl_body_locator(canonical: &str, symbol: &str) -> AuthoredBodyLocator {
    AuthoredBodyLocator::DeclBody(TypeBodySlot {
        anchor: AuthoredAnchor {
            canonical_id: Arc::from(canonical),
            symbol: Arc::from(symbol),
            space: LocatorSymbolSpace::Type,
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        },
        path: Arc::from(Vec::new()),
    })
}

#[test]
fn port_serves_neutral_lowering_for_known_locator() {
    let serve = lower_via_port(
        &MockHost {
            store_published: true,
        },
        &decl_body_locator(KNOWN_CANONICAL, KNOWN_SYMBOL),
    );

    assert_eq!(
        serve.admission,
        QueryHostAdmission::Cacheable,
        "a store-published serve surfaces the Cacheable admission"
    );
    let lowering = serve.outcome.expect("known anchor must lower");
    match &lowering.shape {
        AuthoredBodyShape::Single(TypeExpr::Primitive(PrimitiveName::String)) => {}
        other => panic!("expected the canned Single(string) body shape, got {other:?}"),
    }
    assert_eq!(
        lowering.type_parameters,
        vec![TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }],
        "the lowering must carry the owning declaration's type parameters"
    );
}

/// The serve wrapper carries a consumer-readable admission signal on the
/// SUCCESS arm, and the signal tracks the host's serve-publication bit.
/// Discriminating: a mock (or a port) that hardcoded either admission,
/// dropped the wrapper, or inverted `from_store_published` fails one of
/// the per-serve assertions.
#[test]
fn serve_wrapper_carries_consumer_readable_admission() {
    let locator = decl_body_locator(KNOWN_CANONICAL, KNOWN_SYMBOL);

    let fenced = lower_via_port(
        &MockHost {
            store_published: false,
        },
        &locator,
    );
    assert_eq!(
        fenced.admission,
        QueryHostAdmission::ReturnOnly,
        "an unpublished (fenced) serve must surface ReturnOnly"
    );
    assert!(
        !may_admit_warm(&fenced),
        "a ReturnOnly serve must never be admitted warm"
    );

    let published = lower_via_port(
        &MockHost {
            store_published: true,
        },
        &locator,
    );
    assert_eq!(
        published.admission,
        QueryHostAdmission::Cacheable,
        "a store-published serve must surface Cacheable"
    );
    assert!(
        may_admit_warm(&published),
        "a Cacheable serve may be admitted warm"
    );

    // Admission is serve metadata, not part of the lowering product: the
    // same authored body flows either way.
    assert_eq!(fenced.outcome, published.outcome);
    assert!(
        fenced.outcome.is_ok(),
        "a fenced serve still answers the requesting caller's read"
    );
}

/// THE completion-fence-on-the-error-arm proof: a FENCED serve
/// (`store_published == false`) whose deref lands a GENUINE cacheable
/// miss must surface `ReturnOnly`, and the consumer-side warm-admission
/// gate must refuse it — the miss was observed against superseded state,
/// so warm-admitting it would seed a shared cache with an entry the
/// read-side fact rail cannot reject. Discriminating three ways: a port
/// whose answer degenerates to a bare `Result` (dropping admission on the
/// error arm) fails to compile against the trait; one that hardcodes
/// `Cacheable` for error outcomes fails the fenced assertions; one that
/// RECLASSIFIES the miss (e.g. `UnknownSymbol` into a no-warm class) to
/// smuggle the fence through the error class fails the CONTROL, which
/// pins the same miss as warm-ADMISSIBLE off a store-published serve.
#[test]
fn fenced_serve_keeps_genuine_miss_return_only() {
    let miss_locator = decl_body_locator(KNOWN_CANONICAL, "Nope");

    let fenced = lower_via_port(
        &MockHost {
            store_published: false,
        },
        &miss_locator,
    );
    assert_eq!(
        fenced.admission,
        QueryHostAdmission::ReturnOnly,
        "a fenced serve's genuine miss surfaces ReturnOnly on the error arm"
    );
    assert!(
        !may_admit_warm(&fenced),
        "a cacheable-class miss off a fenced serve must never be admitted warm"
    );
    let fenced_error = fenced
        .outcome
        .expect_err("unknown symbol in a served file is a typed miss");
    assert_eq!(fenced_error, QueryHostError::UnknownSymbol);
    assert!(
        fenced_error.is_cacheable_miss(),
        "the fence rides the admission axis; the error class is never reclassified"
    );

    // CONTROL: the SAME miss off a store-published serve is Cacheable and
    // the gate ADMITS it — proving the refusal above is driven by the
    // serve admission, not by the error class.
    let published = lower_via_port(
        &MockHost {
            store_published: true,
        },
        &miss_locator,
    );
    assert_eq!(
        published.admission,
        QueryHostAdmission::Cacheable,
        "the same genuine miss off a published serve surfaces Cacheable"
    );
    assert!(
        may_admit_warm(&published),
        "a cacheable-class miss off a published serve is warm-admissible"
    );
    assert_eq!(
        published
            .outcome
            .expect_err("same typed miss as the fenced serve"),
        QueryHostError::UnknownSymbol
    );
}

/// The no-serve arm: a locator whose producing canonical the host cannot
/// materialize answers ReturnOnly + the transient no-warm file miss — a
/// non-result on BOTH axes. The mock's publication bit is `true` here on
/// purpose: no serve happened, so there is no publication status to map,
/// and the ReturnOnly must come from the no-serve arm itself.
#[test]
fn unknown_file_answer_is_return_only_transient_miss() {
    let serve = lower_via_port(
        &MockHost {
            store_published: true,
        },
        &decl_body_locator("/ws/elsewhere.ts", KNOWN_SYMBOL),
    );

    assert_eq!(
        serve.admission,
        QueryHostAdmission::ReturnOnly,
        "no serve happened; the answer must be return-only regardless of the bit"
    );
    assert!(
        !may_admit_warm(&serve),
        "an unknown-file non-result must never be admitted warm"
    );
    let error = serve
        .outcome
        .expect_err("unknown producing canonical is a typed non-result");
    assert_eq!(error, QueryHostError::UnknownFile);
    assert!(
        error.is_transient_no_warm() && !error.is_cacheable_miss(),
        "an unknown file is the transient no-warm class, never a cacheable miss"
    );
}

#[test]
fn port_surfaces_typed_error_for_unknown_locator() {
    let serve = lower_via_port(
        &MockHost {
            store_published: true,
        },
        &decl_body_locator(KNOWN_CANONICAL, "Nope"),
    );
    let error = serve
        .outcome
        .expect_err("unknown anchor must be a typed miss");

    assert_eq!(
        error,
        QueryHostError::UnknownSymbol,
        "a genuine unknown-symbol miss keeps its own error class"
    );
    // The load-bearing cache-semantics distinction: a genuine cacheable
    // miss is never the transient no-warm lease signal — readable both as
    // variants and through the class helpers a consumer branches on.
    assert_ne!(error, QueryHostError::LeaseMiss);
    assert!(
        error.is_cacheable_miss() && !error.is_transient_no_warm(),
        "an unknown symbol is a cacheable miss, never a no-warm signal"
    );
    assert!(
        !error.to_string().is_empty(),
        "the neutral error renders a human-readable message"
    );
}

#[test]
fn merged_shape_preserves_contributor_order() {
    // The Merged carrier is a DISTINCT shape (never an intersection): a
    // port result carrying merged contributors must preserve source order.
    let contributors = vec![
        TypeExpr::Primitive(PrimitiveName::String),
        TypeExpr::Primitive(PrimitiveName::Number),
    ];
    let lowering = AuthoredBodyLowering {
        shape: AuthoredBodyShape::Merged(contributors.clone()),
        type_parameters: Vec::new(),
        visibility: TypeParamVisibility::Body,
    };

    match &lowering.shape {
        AuthoredBodyShape::Merged(got) => {
            assert_eq!(
                got, &contributors,
                "merged contributors keep their source order"
            );
        }
        other => panic!("expected the Merged carrier, got {other:?}"),
    }
}
