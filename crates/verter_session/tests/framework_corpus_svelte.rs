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
    /// Member names that MUST be ABSENT from EMITS (the runes-callback rule).
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
        // A runes callback prop stays a PROP and is absent from EMITS.
        props_contains: &["title", "count", "onClose"],
        emits_excludes: &["onClose"],
        model_contains: &[],
        expose_contains: &[],
        expose_excludes: &["title", "onClose"],
    },
    CorpusCase {
        fixture: "legacy_export_let.svelte",
        props_contains: &["name", "count"],
        emits_excludes: &[],
        model_contains: &[],
        expose_contains: &[],
        // A legacy `export let` prop must NOT also surface under EXPOSE.
        expose_excludes: &["name", "count"],
    },
    CorpusCase {
        fixture: "bindable_model.svelte",
        props_contains: &["value", "label"],
        emits_excludes: &[],
        model_contains: &["value"],
        expose_contains: &[],
        expose_excludes: &["value", "label"],
    },
    CorpusCase {
        fixture: "instance_expose.svelte",
        props_contains: &["name"],
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
        for forbidden in case.emits_excludes {
            assert!(
                !emits.iter().any(|e| e == forbidden),
                "{}: EMITS must NOT carry `{forbidden}` (runes callbacks stay PROPS), got {emits:?}",
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
