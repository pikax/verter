//! Svelte framework-surface CORPUS — hermetic, vendored fixtures resolved
//! end-to-end through the registered Svelte adapter.
//!
//! Each fixture under `tests/framework_corpus/svelte/` is loaded into a fresh
//! host, its framework surfaces resolved via
//! `resolve_framework_surface_with_audit`, and the per-kind support + member
//! names asserted against the `CORPUS` expectation table. Hermetic: the fixtures
//! are locally-authored `.svelte` files, so the suite runs with no third-party
//! repo present (Testing-Hermeticity).

use std::path::PathBuf;
use std::sync::Arc;

use verter_protocol::typeinfo::graph::{
    self as wire, FrameworkSurfaceKind, FrameworkSurfaceKindSupport,
};
use verter_protocol::verter::v1::{
    type_info_graph_request as wire_request, type_info_graph_response,
};
use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};

/// One corpus fixture's expectations.
struct CorpusCase {
    /// The fixture file name under `tests/framework_corpus/svelte/`.
    fixture: &'static str,
    /// Prop member names that MUST appear in PROPS.
    props_contains: &'static [&'static str],
    /// EMITS event names that MUST appear (the DERIVED callback-prop event index
    /// — an `on${E}` callback prop surfaces as event `E`).
    emits_contains: &'static [&'static str],
    /// Member names that MUST be ABSENT from EMITS (the prop NAME itself stays a
    /// PROP — `onClose` the prop name never appears in EMITS; the derived EVENT
    /// is named `Close`).
    emits_excludes: &'static [&'static str],
    /// MODEL binding names that MUST appear.
    model_contains: &'static [&'static str],
    /// EXPOSE member names that MUST appear.
    expose_contains: &'static [&'static str],
    /// Member names that MUST be ABSENT from EXPOSE (a legacy `export let` prop
    /// is a PROP, not an EXPOSE member).
    expose_excludes: &'static [&'static str],
}

/// THE Svelte surface corpus expectation table.
const CORPUS: &[CorpusCase] = &[
    CorpusCase {
        fixture: "runes_props.svelte",
        // A runes callback prop stays a PROP (authoritative surface) AND surfaces
        // as a DERIVED event in EMITS: `onClose` → event `Close`.
        props_contains: &["title", "count", "onClose"],
        // The DERIVED callback-prop event index: `onClose` → event `Close`.
        emits_contains: &["Close"],
        // The prop NAME `onClose` is NOT an EMITS member (the event is `Close`).
        emits_excludes: &["onClose"],
        model_contains: &[],
        expose_contains: &[],
        expose_excludes: &["title", "onClose"],
    },
    CorpusCase {
        fixture: "legacy_export_let.svelte",
        props_contains: &["name", "count"],
        emits_contains: &[],
        emits_excludes: &[],
        model_contains: &[],
        expose_contains: &[],
        // A legacy `export let` prop must NOT also surface under EXPOSE.
        expose_excludes: &["name", "count"],
    },
    CorpusCase {
        fixture: "bindable_model.svelte",
        props_contains: &["value", "label"],
        emits_contains: &[],
        emits_excludes: &[],
        model_contains: &["value"],
        expose_contains: &[],
        expose_excludes: &["value", "label"],
    },
    CorpusCase {
        fixture: "instance_expose.svelte",
        props_contains: &["name"],
        emits_contains: &[],
        emits_excludes: &[],
        model_contains: &[],
        // `export function focus` / `export const ready` ARE instance EXPOSE
        // members; `name` is a prop and must NOT appear in EXPOSE.
        expose_contains: &["focus", "ready"],
        expose_excludes: &["name"],
    },
    CorpusCase {
        fixture: "pure_markup.svelte",
        props_contains: &[],
        emits_contains: &[],
        emits_excludes: &[],
        model_contains: &[],
        expose_contains: &[],
        expose_excludes: &[],
    },
];

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/framework_corpus/svelte")
}

fn member_names(
    payload: &wire::FrameworkSurfacePayload,
    kind: FrameworkSurfaceKind,
) -> Vec<String> {
    let strings = payload
        .graph
        .as_ref()
        .and_then(|g| g.strings.as_ref())
        .map(|t| t.entries.clone())
        .unwrap_or_default();
    payload
        .surfaces
        .iter()
        .find(|e| e.kind == kind as i32)
        .map(|e| {
            e.members
                .iter()
                .map(|m| strings.get(m.name_id as usize).cloned().unwrap_or_default())
                .collect()
        })
        .unwrap_or_default()
}

fn kind_support(payload: &wire::FrameworkSurfacePayload, kind: FrameworkSurfaceKind) -> i32 {
    payload
        .surfaces
        .iter()
        .find(|e| e.kind == kind as i32)
        .and_then(|e| e.status.as_ref())
        .map(|s| s.support)
        .unwrap_or(-1)
}

fn envelope(canonical: &str) -> wire::TypeInfoGraphRequest {
    wire::TypeInfoGraphRequest {
        schema_version: 3,
        operation: wire::Operation::FrameworkSurfaces as i32,
        payload: Some(wire_request::Payload::FrameworkSurface(
            wire::FrameworkSurfaceRequest {
                selector: Some(wire::ComponentSelector {
                    canonical_id: canonical.to_string(),
                    export_name: String::new(),
                    has_export_name: false,
                    framework_adapter_id: "svelte".to_string(),
                }),
                context: Some(wire::ProjectionReductionContext {
                    mode: wire::ProjectionMode::Expanded as i32,
                    demand: wire::ReductionDemand::Published as i32,
                }),
                closure: Some(wire::ClosurePolicy {
                    kind: Some(
                        verter_protocol::verter::v1::graph_closure_policy::Kind::OneLevel(
                            wire::ClosureOneLevel {},
                        ),
                    ),
                }),
                display_policy: Some(wire::DisplayPolicy {
                    qualification: wire::DisplayQualification::Qualified as i32,
                    branding: wire::DisplayBranding::On as i32,
                    budgets: Some(wire::DisplayBudgets {
                        max_string_length: 4096,
                        max_depth: 16,
                    }),
                }),
                include_provenance: false,
                include_diagnostics: false,
                include_projection: vec![],
                schema_version: 3,
            },
        )),
    }
}

#[test]
fn svelte_callback_prop_events_structural_not_nominal() {
    // F13 STRUCTURAL classification (discriminating): the derived callback-prop
    // event index is STRUCTURAL, not nominal — an `on${E}` prop with a NON-EMPTY
    // suffix AND a FUNCTION-LIKE value IS an event (`onselect` → event `select`);
    // an arbitrary NON-`on` function prop (`inflate`) is NOT an event; an
    // `on`-prefixed NON-function prop (`online: boolean`) is NOT an event. The
    // `select` event's payload preserves the callback's PARAMETERS directly (NO
    // event-name strip): `(row: Row) => void` → payload carries `row`.
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let path = corpus_dir().join("callback_events.svelte");
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read callback_events fixture: {e}"));
    let canonical = "/callback_events.svelte".to_string();
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.clone()),
            input_id: canonical.clone(),
            source: Arc::from(source.as_str()),
            file_language: FileLanguage::svelte(),
            aliases: Vec::new(),
        })
        .unwrap_or_else(|e| panic!("upsert: {e:?}"));

    let result = host.resolve_framework_surface_with_audit(envelope(&canonical));
    let response = result.as_result().expect("structural response");
    let payload = match &response.kind {
        Some(type_info_graph_response::Kind::FrameworkSurface(p)) => p,
        other => panic!("expected framework_surface arm, got {other:?}"),
    };

    let emits = member_names(payload, FrameworkSurfaceKind::Emits);
    // `onselect` (function-like) IS an event named `select`.
    assert!(
        emits.iter().any(|e| e == "select"),
        "an `onselect` function prop must surface as event `select`, got {emits:?}"
    );
    // `inflate` (NOT `on`-prefixed) is NOT an event — arbitrary function props
    // are never mined.
    assert!(
        !emits.iter().any(|e| e == "inflate"),
        "an arbitrary non-`on` function prop must NOT be an event, got {emits:?}"
    );
    // `online: boolean` (`on`-prefixed but NOT function-like) is NOT an event.
    assert!(
        !emits.iter().any(|e| e == "line"),
        "an `on`-prefixed NON-function prop must NOT be an event, got {emits:?}"
    );
    // `label` (a plain prop) is never an event.
    assert!(
        !emits.iter().any(|e| e == "label"),
        "a plain prop must NOT be an event, got {emits:?}"
    );
    // The callback props ALSO stay PROPS (the derived index is non-authoritative).
    let props = member_names(payload, FrameworkSurfaceKind::Props);
    for want in ["label", "onselect", "inflate", "online"] {
        assert!(
            props.iter().any(|p| p == want),
            "callback props stay PROPS — `{want}` must be in PROPS, got {props:?}"
        );
    }
}

#[test]
fn svelte_surface_corpus_resolves_every_fixture() {
    for case in CORPUS {
        let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
        let path = corpus_dir().join(case.fixture);
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read corpus fixture {}: {e}", path.display()));
        let canonical = format!("/{}", case.fixture);
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: Some(canonical.clone()),
                input_id: canonical.clone(),
                source: Arc::from(source.as_str()),
                file_language: FileLanguage::svelte(),
                aliases: Vec::new(),
            })
            .unwrap_or_else(|e| panic!("upsert {canonical}: {e:?}"));

        let result = host.resolve_framework_surface_with_audit(envelope(&canonical));
        let response = result
            .as_result()
            .unwrap_or_else(|e| panic!("{}: structural response, got {e:?}", case.fixture));
        let payload = match &response.kind {
            Some(type_info_graph_response::Kind::FrameworkSurface(p)) => p,
            other => panic!(
                "{}: expected framework_surface arm, got {other:?}",
                case.fixture
            ),
        };

        // EXACTLY one entry per known kind; OPTIONS is the only UNSUPPORTED kind.
        assert_eq!(
            payload.surfaces.len(),
            6,
            "{}: one entry per kind",
            case.fixture
        );
        assert_eq!(
            kind_support(payload, FrameworkSurfaceKind::Options),
            FrameworkSurfaceKindSupport::Unsupported as i32,
            "{}: OPTIONS is structurally UNSUPPORTED",
            case.fixture
        );
        for kind in [
            FrameworkSurfaceKind::Props,
            FrameworkSurfaceKind::Emits,
            FrameworkSurfaceKind::Slots,
            FrameworkSurfaceKind::Expose,
            FrameworkSurfaceKind::Model,
        ] {
            assert_ne!(
                kind_support(payload, kind),
                FrameworkSurfaceKindSupport::Unsupported as i32,
                "{}: {kind:?} must not be UNSUPPORTED",
                case.fixture
            );
        }

        let props = member_names(payload, FrameworkSurfaceKind::Props);
        for want in case.props_contains {
            assert!(
                props.iter().any(|p| p == want),
                "{}: PROPS must carry `{want}`, got {props:?}",
                case.fixture
            );
        }
        let emits = member_names(payload, FrameworkSurfaceKind::Emits);
        for want in case.emits_contains {
            assert!(
                emits.iter().any(|e| e == want),
                "{}: EMITS must carry the derived callback event `{want}`, got {emits:?}",
                case.fixture
            );
        }
        for forbidden in case.emits_excludes {
            assert!(
                !emits.iter().any(|e| e == forbidden),
                "{}: EMITS must NOT carry `{forbidden}` (the prop name stays a PROP), got {emits:?}",
                case.fixture
            );
        }
        let model = member_names(payload, FrameworkSurfaceKind::Model);
        for want in case.model_contains {
            assert!(
                model.iter().any(|m| m == want),
                "{}: MODEL must carry `{want}`, got {model:?}",
                case.fixture
            );
        }
        let expose = member_names(payload, FrameworkSurfaceKind::Expose);
        for want in case.expose_contains {
            assert!(
                expose.iter().any(|m| m == want),
                "{}: EXPOSE must carry `{want}`, got {expose:?}",
                case.fixture
            );
        }
        for forbidden in case.expose_excludes {
            assert!(
                !expose.iter().any(|m| m == forbidden),
                "{}: EXPOSE must NOT carry `{forbidden}` (a prop is not an EXPOSE member), got {expose:?}",
                case.fixture
            );
        }
    }
}
