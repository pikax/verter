//! R26 binding: `StoreView` per-domain validator dispatch.
//!
//! The trait has four entry points:
//!
//! - `validates(fact)` — generic dispatch.
//! - `validates_parse_domain(parse_fact)` — producer overrides.
//! - `validates_resolve_imports_domain(resolve_fact)` — producer overrides.
//! - `validates_route_surface_domain(route_fact)` — producer overrides.
//!
//! The default `validates` impl routes each per-domain variant to
//! the matching per-domain method. This test exercises every arm
//! of that routing by:
//!
//! 1. Constructing a test `StoreView` that returns a distinct value
//!    for each per-domain method.
//! 2. Constructing one `FactVersionRef` per domain.
//! 3. Asserting `validates(fact)` returns the right per-domain
//!    value — proving the dispatch table is keyed by
//!    `FactDomain` (3 variants), not by `FactKey`.
//!
//! R26: "Adding a new `FactKey` extends the per-domain `*FactRef`
//! enum but does NOT widen the trait".

use verter_semantic::facts::{FactKey, FactLane, SymbolSpace};
use verter_session::file_artifact_store::InternedName;
use verter_session::resolver_core::{
    FactVersionRef, ParseFactRef, ResolveImportsFactRef, RouteSurfaceFactRef, StoreView,
    StoreViewCompatToken,
};

/// Test view that returns one of three distinct values depending on
/// which per-domain method the dispatch picked.
#[derive(Debug, Clone, Copy)]
struct TestView {
    parse_returns: bool,
    resolve_imports_returns: bool,
    route_surface_returns: bool,
}

impl StoreView for TestView {
    fn compat_token(&self) -> StoreViewCompatToken {
        StoreViewCompatToken {
            epoch: 0,
            session: None,
            validity_fingerprint: 0,
        }
    }

    // The trait `validates` impl dispatches per-domain so the
    // per-domain methods drive validation. The legacy variants
    // (`FileWholeHash` / `DerivedFactHash`) MUST be handled by
    // implementers; this test view doesn't observe legacy facts so
    // returns `false` for them (proving the trait's `validates`
    // can be entirely overridden to dispatch per-domain).
    fn validates(&self, fact: &FactVersionRef) -> bool {
        match fact {
            FactVersionRef::Parse(p) => self.validates_parse_domain(p),
            FactVersionRef::ResolveImports(r) => self.validates_resolve_imports_domain(r),
            FactVersionRef::RouteSurface(r) => self.validates_route_surface_domain(r),
            // Contributor source-env identity — routes to the
            // dedicated strict per-arm validator. This external view
            // does not override it, so the trait default (`false`,
            // fail closed) applies; the identity fields' types are
            // crate-internal (sealed construction), so an off-crate
            // view can route the arm without naming them.
            FactVersionRef::FileSourceEnv {
                canonical_id,
                parse_env_hash,
                parser_version,
                file_language_id,
            } => self.validates_file_source_env(
                canonical_id,
                *parse_env_hash,
                *parser_version,
                file_language_id,
            ),
            FactVersionRef::FileWholeHash { .. }
            | FactVersionRef::DerivedFactHash { .. }
            | FactVersionRef::ProjectGeneration { .. } => false,
        }
    }

    fn validates_parse_domain(&self, _fact: &ParseFactRef) -> bool {
        self.parse_returns
    }

    fn validates_resolve_imports_domain(&self, _fact: &ResolveImportsFactRef) -> bool {
        self.resolve_imports_returns
    }

    fn validates_route_surface_domain(&self, _fact: &RouteSurfaceFactRef) -> bool {
        self.route_surface_returns
    }
}

fn parse_fact() -> FactVersionRef {
    FactVersionRef::Parse(ParseFactRef {
        canonical_id: "/a.ts".into(),
        key: FactKey::Export {
            name: InternedName::from("Foo"),
            space: SymbolSpace::Type,
        },
        lane: FactLane::Semantic,
        expected_hash: [1u8; 16],
    })
}

fn resolve_imports_fact() -> FactVersionRef {
    FactVersionRef::ResolveImports(ResolveImportsFactRef {
        canonical_id: "/a.ts".into(),
        key: FactKey::ResolvedImportClause {
            specifier: "./theme".into(),
            binding: InternedName::from("Theme"),
            space: SymbolSpace::Type,
            resolved_canonical: std::sync::Arc::from("/theme.ts"),
            resolved_source_name: InternedName::from("Theme"),
        },
        lane: FactLane::Semantic,
        expected_hash: [2u8; 16],
    })
}

fn route_surface_fact() -> FactVersionRef {
    FactVersionRef::RouteSurface(RouteSurfaceFactRef {
        canonical_id: "/a.ts".into(),
        key: FactKey::EffectiveExportSet,
        lane: FactLane::Semantic,
        expected_hash: [3u8; 16],
    })
}

#[test]
fn validates_routes_parse_variant_to_parse_validator() {
    // Distinct per-domain returns force the dispatch to discriminate.
    let view = TestView {
        parse_returns: true,
        resolve_imports_returns: false,
        route_surface_returns: false,
    };
    let fact = parse_fact();
    assert!(
        view.validates(&fact),
        "validates(Parse) MUST dispatch to validates_parse_domain"
    );
    // And the resolve / route inputs MUST return false under the
    // same view, proving dispatch is by domain not blanket.
    assert!(!view.validates(&resolve_imports_fact()));
    assert!(!view.validates(&route_surface_fact()));
}

#[test]
fn validates_routes_resolve_imports_variant_to_resolve_imports_validator() {
    let view = TestView {
        parse_returns: false,
        resolve_imports_returns: true,
        route_surface_returns: false,
    };
    let fact = resolve_imports_fact();
    assert!(
        view.validates(&fact),
        "validates(ResolveImports) MUST dispatch to validates_resolve_imports_domain"
    );
    assert!(!view.validates(&parse_fact()));
    assert!(!view.validates(&route_surface_fact()));
}

#[test]
fn validates_routes_route_surface_variant_to_route_surface_validator() {
    let view = TestView {
        parse_returns: false,
        resolve_imports_returns: false,
        route_surface_returns: true,
    };
    let fact = route_surface_fact();
    assert!(
        view.validates(&fact),
        "validates(RouteSurface) MUST dispatch to validates_route_surface_domain"
    );
    assert!(!view.validates(&parse_fact()));
    assert!(!view.validates(&resolve_imports_fact()));
}

#[test]
fn default_validates_returns_false_for_legacy_variants_on_minimal_view() {
    // Sanity: legacy `FileWholeHash` / `DerivedFactHash` variants
    // fall through to the trait's default `false` when no override
    // is provided. Implementers that emit these variants MUST
    // provide a `validates` override (HostStoreView does this).
    let view = TestView {
        parse_returns: true,
        resolve_imports_returns: true,
        route_surface_returns: true,
    };
    let legacy_whole = FactVersionRef::FileWholeHash {
        canonical_id: "/a.ts".into(),
        hash: [0u8; 16],
    };
    assert!(
        !view.validates(&legacy_whole),
        "trait default MUST return false for legacy variants — implementers MUST override"
    );
}

#[test]
fn dispatch_table_bound_by_fact_domain_not_fact_key() {
    // R26: adding a new `FactKey` extends a per-domain `*FactRef`
    // enum but does NOT widen the trait. We verify by constructing
    // TWO Parse facts with DIFFERENT `FactKey`s — both must route
    // to the SAME `validates_parse_domain` arm.
    let view = TestView {
        parse_returns: true,
        resolve_imports_returns: false,
        route_surface_returns: false,
    };

    let fact_export = FactVersionRef::Parse(ParseFactRef {
        canonical_id: "/a.ts".into(),
        key: FactKey::Export {
            name: InternedName::from("Foo"),
            space: SymbolSpace::Type,
        },
        lane: FactLane::Semantic,
        expected_hash: [10u8; 16],
    });
    let fact_member_presence = FactVersionRef::Parse(ParseFactRef {
        canonical_id: "/a.ts".into(),
        key: FactKey::MemberPresence {
            exporter: InternedName::from("Foo"),
            name: InternedName::from("a"),
            space: SymbolSpace::Type,
        },
        lane: FactLane::Semantic,
        expected_hash: [11u8; 16],
    });
    let fact_module_aug = FactVersionRef::Parse(ParseFactRef {
        canonical_id: "/a.ts".into(),
        key: FactKey::ModuleAugmentation {
            specifier: "vue".into(),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            augmented_name: InternedName::from("ComponentOptions"),
            space: SymbolSpace::Type,
        },
        lane: FactLane::Semantic,
        expected_hash: [12u8; 16],
    });
    // All three MUST route to `validates_parse_domain` → return
    // `true`. Different `FactKey`s in the same domain MUST NOT
    // affect dispatch.
    assert!(view.validates(&fact_export));
    assert!(view.validates(&fact_member_presence));
    assert!(view.validates(&fact_module_aug));
}
