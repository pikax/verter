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
    CorpusCase {
        fixture: "prop_defaults.svelte",
        // All three named props surface; `disabled = $bindable(false)` is BOTH a
        // prop AND a MODEL binding.
        props_contains: &["size", "disabled", "label"],
        emits_contains: &[],
        emits_excludes: &[],
        model_contains: &["disabled"],
        expose_contains: &[],
        expose_excludes: &["size", "label"],
    },
    CorpusCase {
        fixture: "renamed_destructuring.svelte",
        // The PROP name is `size` (from the Props type), NOT the local rename `s`.
        props_contains: &["size", "count"],
        emits_contains: &[],
        emits_excludes: &[],
        model_contains: &[],
        expose_contains: &[],
        expose_excludes: &["s"],
    },
    CorpusCase {
        fixture: "rest_props.svelte",
        // The named props surface; the `...rest` binding is not a named prop.
        props_contains: &["id", "title"],
        emits_contains: &[],
        emits_excludes: &[],
        model_contains: &[],
        expose_contains: &[],
        expose_excludes: &["rest"],
    },
    CorpusCase {
        fixture: "generic_props.svelte",
        props_contains: &["items", "selected"],
        emits_contains: &[],
        emits_excludes: &[],
        model_contains: &[],
        expose_contains: &[],
        expose_excludes: &[],
    },
    CorpusCase {
        fixture: "intersection_union_props.svelte",
        props_contains: &["base", "mode"],
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

/// The `required` flag of a named member on a per-kind surface, or `None` when
/// the member is absent.
fn member_required(
    payload: &wire::FrameworkSurfacePayload,
    kind: FrameworkSurfaceKind,
    member_name: &str,
) -> Option<bool> {
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
        .and_then(|e| {
            e.members
                .iter()
                .find(|m| strings.get(m.name_id as usize).map(|s| s.as_str()) == Some(member_name))
        })
        .map(|m| m.required)
}

/// The interned string-table entries of the response graph.
fn string_table(payload: &wire::FrameworkSurfacePayload) -> Vec<String> {
    payload
        .graph
        .as_ref()
        .and_then(|g| g.strings.as_ref())
        .map(|t| t.entries.clone())
        .unwrap_or_default()
}

/// The wire member of a given name on a per-kind surface, or `None`.
fn member_by_name<'a>(
    payload: &'a wire::FrameworkSurfacePayload,
    kind: FrameworkSurfaceKind,
    member_name: &str,
) -> Option<&'a wire::FrameworkSurfaceMember> {
    let strings = string_table(payload);
    payload
        .surfaces
        .iter()
        .find(|e| e.kind == kind as i32)
        .and_then(|e| {
            e.members
                .iter()
                .find(|m| strings.get(m.name_id as usize).map(|s| s.as_str()) == Some(member_name))
        })
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

#[test]
fn svelte_prop_defaults_make_props_optional_on_the_surface() {
    // Framework-surface DATA (DISCRIMINATING): a runes prop with a
    // destructuring default (`size = 'md'`) or a `$bindable(<default>)` fallback
    // (`disabled = $bindable(false)`) is OPTIONAL on the framework-surface wire
    // (`required = false`), while a prop WITHOUT a default (`label`) stays
    // REQUIRED. This pins the default → optionality application directly on the
    // observable wire. RED against the pre-change tree (defaults were never
    // applied to the Svelte props surface).
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let path = corpus_dir().join("prop_defaults.svelte");
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read prop_defaults fixture: {e}"));
    let canonical = "/prop_defaults.svelte".to_string();
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

    // A destructuring-defaulted prop is OPTIONAL (required = false).
    assert_eq!(
        member_required(payload, FrameworkSurfaceKind::Props, "size"),
        Some(false),
        "a destructuring-defaulted prop `size = 'md'` must be optional (required = false)"
    );
    // A `$bindable(<default>)`-defaulted prop is OPTIONAL too.
    assert_eq!(
        member_required(payload, FrameworkSurfaceKind::Props, "disabled"),
        Some(false),
        "a `$bindable(false)`-defaulted prop must be optional (required = false)"
    );
    // A prop WITHOUT a default stays REQUIRED (discriminating negative).
    assert_eq!(
        member_required(payload, FrameworkSurfaceKind::Props, "label"),
        Some(true),
        "a prop without a default (`label`) must stay required"
    );
    // `disabled` is ALSO a MODEL binding (the `$bindable` marks it bindable).
    let model = member_names(payload, FrameworkSurfaceKind::Model);
    assert!(
        model.iter().any(|m| m == "disabled"),
        "`disabled = $bindable(false)` is a MODEL binding, got {model:?}"
    );
}

#[test]
fn svelte_prop_default_and_origin_are_on_the_public_framework_surface_wire() {
    // THE P0 PROOF (DISCRIMINATING, RED before this fix): a docs / semantic-DB
    // consumer reading the PUBLIC framework-surface graph wire
    // (`FrameworkSurfaceMember`) gets each prop's runtime DEFAULT value source
    // text AND its member-declaration ORIGIN. Before the fix the member shape
    // had no default/origin slot and the encoder dropped both — so this test
    // could not even be written against the old wire. Asserted directly on the
    // wire `FrameworkSurfaceMember.default_value_id` + `.origin`, resolving ids
    // through the response graph string table.
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let path = corpus_dir().join("prop_defaults.svelte");
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read prop_defaults fixture: {e}"));
    let canonical = "/prop_defaults.svelte".to_string();
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
    let strings = string_table(payload);
    let resolve = |id: u32| strings.get(id as usize).cloned().unwrap_or_default();

    // The response graph carries schema 4 (the version that introduced the
    // default/origin member fields).
    assert_eq!(
        payload.graph.as_ref().unwrap().schema_version,
        4,
        "the framework-surface graph is schema 4"
    );

    // `size = 'md'` — the runtime DEFAULT VALUE source text rides the wire
    // member's `default_value_id`.
    let size = member_by_name(payload, FrameworkSurfaceKind::Props, "size")
        .expect("the `size` prop is on PROPS");
    let size_default = size
        .default_value_id
        .expect("the `size` member carries a default on the wire");
    assert_eq!(
        resolve(size_default),
        "'md'",
        "the destructuring default source text is on the public wire"
    );

    // A prop WITHOUT a default (`label`) has NO `default_value_id` (presence-
    // aware: a missing default is `None`, never a stray id-0). DISCRIMINATING.
    let label = member_by_name(payload, FrameworkSurfaceKind::Props, "label")
        .expect("the `label` prop is on PROPS");
    assert!(
        label.default_value_id.is_none(),
        "a prop without a default has NO default_value_id on the wire, got {:?}",
        label.default_value_id
    );

    // Each prop member carries a MEMBER-DECLARATION origin: an inline/local
    // `Props` ⇒ a LOCAL hop whose declaration file is the owner. The origin is
    // on the PUBLIC wire member.
    let origin = size
        .origin
        .as_ref()
        .expect("the `size` member carries an origin");
    assert_eq!(
        origin.chain.len(),
        1,
        "one local hop, got {:?}",
        origin.chain
    );
    let hop = &origin.chain[0];
    assert_eq!(
        hop.kind,
        wire::FrameworkSurfaceOriginHopKind::Local as i32,
        "an inline/local props member is a LOCAL hop"
    );
    let decl = origin
        .declaration
        .as_ref()
        .expect("the origin carries a member declaration");
    assert_eq!(
        resolve(decl.canonical_source_id),
        canonical,
        "the member declaration lives in the owner file"
    );
    assert_eq!(
        resolve(decl.resolved_name_id),
        "size",
        "the member-declaration origin names the MEMBER"
    );
}

#[test]
fn svelte_imported_props_origin_import_hop_on_the_public_wire() {
    // THE P0 PROOF for the cross-file IMPORT hop (DISCRIMINATING): when the
    // props type is an IMPORTED interface, the PUBLIC wire member's origin is an
    // IMPORT hop pointing at the declaring module — recovered by a consumer
    // reading `FrameworkSurfaceMember.origin` off the wire (string ids resolved
    // through the response graph string table). DISCRIMINATING: a LOCAL hop or
    // an owner canonical_source would fail this.
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let component = "/Imported.svelte".to_string();
    let component_src = "<script lang=\"ts\">\n\
             import type { Props } from './types';\n\
             let { box, width }: Props = $props();\n\
             void box; void width;\n\
             </script>\n\
             <div />";
    let types = "/types.ts".to_string();
    let types_src = "export interface Box { w: number }\n\
             export interface Props { box: Box; width: number }\n";
    for (canonical, src, lang) in [
        (component.clone(), component_src, FileLanguage::svelte()),
        (types.clone(), types_src, FileLanguage::script_ts()),
    ] {
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: Some(canonical.clone()),
                input_id: canonical.clone(),
                source: Arc::from(src),
                file_language: lang,
                aliases: Vec::new(),
            })
            .unwrap_or_else(|e| panic!("upsert {canonical}: {e:?}"));
    }

    let result = host.resolve_framework_surface_with_audit(envelope(&component));
    let response = result.as_result().expect("structural response");
    let payload = match &response.kind {
        Some(type_info_graph_response::Kind::FrameworkSurface(p)) => p,
        other => panic!("expected framework_surface arm, got {other:?}"),
    };
    let strings = string_table(payload);
    let resolve = |id: u32| strings.get(id as usize).cloned().unwrap_or_default();

    // Both members (incl. the primitive `width`) carry an IMPORT origin hop.
    for prop in ["box", "width"] {
        let member = member_by_name(payload, FrameworkSurfaceKind::Props, prop)
            .unwrap_or_else(|| panic!("the `{prop}` prop is on PROPS"));
        let origin = member
            .origin
            .as_ref()
            .unwrap_or_else(|| panic!("`{prop}` carries an origin on the public wire"));
        assert_eq!(origin.chain.len(), 1, "one import hop for `{prop}`");
        let hop = &origin.chain[0];
        assert_eq!(
            hop.kind,
            wire::FrameworkSurfaceOriginHopKind::Import as i32,
            "`{prop}` is declared in an imported module ⇒ an IMPORT hop"
        );
        // PRESENCE-AWARE: an IMPORT hop's `from_id` is genuinely present; the
        // unused REEXPORT/ALIAS fields stay absent (None), never id 0.
        let from_id = hop
            .from_id
            .unwrap_or_else(|| panic!("`{prop}`'s IMPORT hop carries a present from_id"));
        assert_eq!(
            resolve(from_id),
            types,
            "`{prop}`'s import source module is the declaring file"
        );
        assert!(
            hop.to_id.is_none() && hop.exported_name_id.is_none() && hop.alias_name_id.is_none(),
            "`{prop}`'s IMPORT hop leaves the REEXPORT/ALIAS fields absent, never id 0"
        );
        let decl = origin
            .declaration
            .as_ref()
            .unwrap_or_else(|| panic!("`{prop}` origin carries a declaration"));
        assert_eq!(
            resolve(decl.canonical_source_id),
            types,
            "`{prop}`'s member declaration lives in the imported module"
        );
    }
}
