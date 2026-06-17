//! The shared, tsgo-free `source_admission_digest` derivation
//! (`docs/arch/u0-oracle-harness-design.md` §Q1 / §Q2).
//!
//! ONE owner for the source-side admission digest: the `oracle-gen` generator
//! ASSEMBLES it into the snapshot, and the default-feature consumption guard
//! `source_admission_digest_consistent` RE-DERIVES it and cross-checks the
//! checked-in value. Both build from the SAME live source-side walk through the
//! one shared resolver (`resolve_source_declarations`) plus the deterministic,
//! offline OXC re-parse for the declaration span — never a second resolution
//! engine, never tsgo. The guard re-deriving through this module is exactly the
//! `source_admission_digest_consistent` redrive the gen-side doc comments name.
//!
//! Available to BOTH cfg contexts (`#[cfg(any(test, feature = "oracle-gen"))]`):
//! the `#[cfg(test)]` consumption driver and the `#[cfg(feature = "oracle-gen")]`
//! generator. It touches NO tsgo and adds NO query-time resolution path.

use std::sync::Arc;

use oxc_allocator::Allocator;
use oxc_ast::ast::{Declaration, Statement};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};
use serde_json::{json, Value};
use verter_compiler::utils::oxc::script::raw_surface::{
    RawDeclKind, RawKey, RawMemberKind, RawSourceSurface, SymbolSpace as RawSymbolSpace,
    TupleElementShape,
};

use crate::resolver_core::{CanonicalCompletionOverlay, HostResolverContext};
use crate::types::{HostConfig, UpsertRequest};
use crate::VerterHost;

use super::admission::{SourceContributor, SourceWalkResult};
use super::identity;
use super::query_specs::{QuerySpec, SymbolSpace};
use super::source_walk::{resolve_source_declarations, SourceLocator};

/// Why the shared digest derivation could not produce a `source_admission_digest`
/// for a spec. The generator maps these onto its own `GenError`; the consumption
/// guard surfaces them as a digest-consistency failure.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SourceDigestError {
    /// The source-side walk did not resolve to a contributor vector
    /// (`Unresolved` / `Cycle`) — the re-derivation could not reach the queried
    /// declaration the snapshot recorded.
    WalkNotResolved(String),
    /// A contributor's defining file is absent from the spec's `workspace_files`
    /// (the registry is the source-byte authority).
    MissingFile(String),
    /// The queried declaration's span could not be located by re-parsing the
    /// defining file with OXC.
    DeclSpanNotFound(String),
}

/// Build a standalone `VerterHost` from the spec's workspace files and walk the
/// queried symbol's source-side declaration graph through the shared resolver
/// (`resolve_source_declarations`). This is the SAME construction the consumption
/// path uses; it adds NO tsgo and NO query-time resolution beyond the shared
/// resolver. Only the `standalone` host kind is first-class (§Scope).
pub(crate) fn source_side_walk(spec: &QuerySpec) -> SourceWalkResult {
    let host = build_source_host(spec);
    // Build a quiescent owned view over the freshly-constructed standalone host.
    // The raw-view escape hatch is allowlisted for this driver-snapshot rail.
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(CanonicalCompletionOverlay::new());
    let ctx = HostResolverContext::new(&host, &store_view, overlay);
    let locator = SourceLocator {
        reference_canonical: spec.source_locator.reference_canonical.to_string(),
        reference_name: spec.source_locator.reference_name.to_string(),
        symbol_space: to_walk_space(spec.source_locator.symbol_space),
    };
    resolve_source_declarations(&ctx, &locator)
}

/// Construct the standalone footprint host for the source-side walk and upsert
/// every workspace file (the `make_host_with_footprint` shape — the only
/// admissible host class currently). `workspace_footprint` / package-backed
/// kinds are deferred (§Scope); a spec carrying one still constructs a host so
/// the walk runs, but the snapshot's `host_setup_kind` will fail the
/// `standalone_host_is_default_canonical_config` guard.
pub(crate) fn build_source_host(spec: &QuerySpec) -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        footprint_capture: true,
        ..HostConfig::default()
    }));
    for f in spec.workspace_files {
        let _ = host.upsert(UpsertRequest {
            canonical_id: Some(f.path.to_string()),
            input_id: f.path.to_string(),
            source: Arc::from(f.source),
            file_language: verter_language::LanguageRegistry::global()
                .classify_static(f.path)
                .static_resolution(),
            aliases: Vec::new(),
        });
    }
    host
}

/// Map the registry's `SymbolSpace` onto the resolver's raw-surface `SymbolSpace`
/// (the `SourceLocator` axis). Two distinct enums, one meaning.
pub(crate) fn to_walk_space(space: SymbolSpace) -> RawSymbolSpace {
    match space {
        SymbolSpace::Type => RawSymbolSpace::Type,
        SymbolSpace::Value => RawSymbolSpace::Value,
    }
}

/// The upserted source bytes the spec's registry entry carries for `canonical`
/// (the registry is the source-byte authority).
pub(crate) fn workspace_file_source<'a>(spec: &'a QuerySpec, canonical: &str) -> Option<&'a str> {
    spec.workspace_files
        .iter()
        .find(|f| f.path == canonical)
        .map(|f| f.source)
}

/// Re-derive the full `source_admission_digest` `Value` for a spec from its
/// CURRENT registry source bytes through the shared source-side walk. The
/// consumption guard (`source_admission_digest_consistent`) compares this against
/// the snapshot's checked-in digest under canonical JSON — so a hand-edited
/// locator / content-hash / contributor raw-surface / lowered-body / verdict /
/// single-contributor count is caught. A walk that no longer resolves (the source
/// drifted out from under the snapshot) is itself a hard failure.
///
/// (`allow(dead_code)`: consumed by the `#[cfg(test)]` consumption guard; in the
/// non-test `oracle-gen` bin build the generator assembles the digest directly
/// via `build_source_digest`, so this redrive entry is unreferenced there.)
#[allow(dead_code)]
pub(crate) fn rederive_source_digest(spec: &QuerySpec) -> Result<Value, SourceDigestError> {
    let walk = source_side_walk(spec);
    let contributors = match &walk {
        SourceWalkResult::Resolved { contributors } => contributors.as_slice(),
        other => return Err(SourceDigestError::WalkNotResolved(format!("{other:?}"))),
    };
    build_source_digest(spec, contributors)
}

/// Assemble the `source_admission_digest` (§Q1) from the resolved contributor(s).
/// For the admitted single-contributor class the digest carries exactly one
/// contributor entry; its `decl_span` is recovered by re-parsing the defining
/// fixture source (a deterministic, offline-reproducible step), and its
/// `raw_surface` is rendered canonically from the parse-time raw-fact record.
pub(crate) fn build_source_digest(
    spec: &QuerySpec,
    contributors: &[SourceContributor],
) -> Result<Value, SourceDigestError> {
    let locator = &spec.source_locator;
    let space_tag = symbol_space_tag(locator.symbol_space);

    // The observed source-declaration files: each recorded contributor's defining
    // file, with the content hash of the registry source bytes for that path.
    let mut observed: Vec<(String, String)> = Vec::new();
    let mut entries: Vec<Value> = Vec::new();
    for c in contributors {
        // The defining file. `RawSourceSurface.decl_canonical` is stamped by the
        // file-aware storage layer and may be empty on this read path; for the
        // admitted PROVABLY-SINGLE-CONTRIBUTOR class (no import / re-export hop)
        // the defining file IS the locator's reference file (§Scope), so fall
        // back to it when the stamp is absent.
        let decl_canonical = if c.raw_surface.decl_canonical.is_empty() {
            locator.reference_canonical.to_string()
        } else {
            c.raw_surface.decl_canonical.clone()
        };
        let source = workspace_file_source(spec, &decl_canonical)
            .ok_or_else(|| SourceDigestError::MissingFile(decl_canonical.clone()))?;
        let (start, end) = find_decl_span(source, locator.reference_name, locator.symbol_space)
            .ok_or_else(|| {
                SourceDigestError::DeclSpanNotFound(locator.reference_name.to_string())
            })?;
        let content_hash = identity::content_hash(source);
        if !observed.iter().any(|(p, _)| p == &decl_canonical) {
            observed.push((decl_canonical.clone(), content_hash.clone()));
        }
        entries.push(json!({
            "contributor_ordinal": c.ordinal,
            "decl_span": { "file": decl_canonical, "start": start, "end": end },
            "decl_canonical": decl_canonical,
            "name": locator.reference_name,
            "symbol_space": space_tag,
            "decl_kind": raw_decl_kind_tag(c.raw_surface.decl_kind),
            "raw_surface": raw_surface_to_json(&c.raw_surface),
            "lowered_body": c.lowered_body.to_json_value(),
            "verdict": "Admit",
        }));
    }

    let observed_source_files: Vec<Value> = observed
        .iter()
        .map(|(path, hash)| json!({ "path": path, "content_hash": hash }))
        .collect();

    Ok(json!({
        "source_locator": {
            "reference_canonical": locator.reference_canonical,
            "reference_name": locator.reference_name,
            "symbol_space": space_tag,
        },
        "observed_source_files": observed_source_files,
        "contributors": entries,
        "final_verdict": "Admit",
    }))
}

/// Locate the `(start, end)` span of the top-level declaration named `name` in
/// `source` by re-parsing it with OXC. A deterministic, offline-reproducible
/// step (the `source_admission_digest_consistent` guard re-derives the same span
/// from current source). Type-space binds a type alias / interface / enum /
/// class; value-space binds a `const`/`let`/`var`, function, or class.
pub(crate) fn find_decl_span(source: &str, name: &str, space: SymbolSpace) -> Option<(u32, u32)> {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    if ret.panicked {
        return None;
    }
    for stmt in &ret.program.body {
        // A top-level declaration is either a bare declaration statement or one
        // wrapped in `export { ... }` / `export default`. Unwrap both.
        let decl: Option<&Declaration> = match stmt {
            Statement::ExportNamedDeclaration(e) => e.declaration.as_ref(),
            other => other.as_declaration(),
        };
        let Some(decl) = decl else { continue };
        let hit = match (space, decl) {
            (SymbolSpace::Type, Declaration::TSTypeAliasDeclaration(d)) => d.id.name == name,
            (SymbolSpace::Type, Declaration::TSInterfaceDeclaration(d)) => d.id.name == name,
            (SymbolSpace::Type, Declaration::TSEnumDeclaration(d)) => d.id.name == name,
            (_, Declaration::ClassDeclaration(d)) => {
                d.id.as_ref().map(|i| i.name == name).unwrap_or(false)
            }
            (SymbolSpace::Value, Declaration::FunctionDeclaration(d)) => {
                d.id.as_ref().map(|i| i.name == name).unwrap_or(false)
            }
            (SymbolSpace::Value, Declaration::VariableDeclaration(d)) => {
                d.declarations.iter().any(|v| {
                    v.id.get_binding_identifier()
                        .map(|i| i.name == name)
                        .unwrap_or(false)
                })
            }
            _ => false,
        };
        if hit {
            let span = decl.span();
            return Some((span.start, span.end));
        }
    }
    None
}

/// Render a `RawSourceSurface` to its canonical JSON (§Q1 example) — the
/// parse-time raw-fact record the digest stores. `RawSourceSurface` carries no
/// `Serialize`, so each field is rendered explicitly; the offline
/// `source_admission_digest_consistent` guard re-derives the SAME shape from
/// current source.
pub(crate) fn raw_surface_to_json(raw: &RawSourceSurface) -> Value {
    json!({
        "raw_member_keys": raw.raw_member_keys.iter().map(raw_key_tag).collect::<Vec<_>>(),
        "member_kinds": raw.member_kinds.iter().map(|k| raw_member_kind_tag(*k)).collect::<Vec<_>>(),
        "member_visibility": raw
            .member_visibility
            .iter()
            .map(|v| member_visibility_tag(*v))
            .collect::<Vec<_>>(),
        "unique_symbol_ops": vec![Value::Null; raw.unique_symbol_ops.len()]
            .iter()
            .map(|_| json!("UniqueSymbol"))
            .collect::<Vec<_>>(),
        "abstract_ctor": raw.abstract_ctor,
        "type_param_modifiers": raw
            .type_param_modifiers
            .iter()
            .map(|m| json!({
                "is_const": m.is_const,
                "variance_in": m.variance_in,
                "variance_out": m.variance_out,
            }))
            .collect::<Vec<_>>(),
        "this_type_or_param": raw.this_type_or_param,
        "value_const_assertion": raw.value_const_assertion,
        "overload_signatures": vec![Value::Null; raw.overload_signatures.len()]
            .iter()
            .map(|_| json!("OverloadSignature"))
            .collect::<Vec<_>>(),
        "tuple_element_shape": raw
            .tuple_element_shape
            .iter()
            .map(|t| tuple_element_shape_tag(*t))
            .collect::<Vec<_>>(),
        "utility_referent_names": raw.utility_referent_names.clone(),
        "transitive_referents": raw
            .transitive_referents
            .iter()
            .map(|r| json!({ "reference_name": r.reference_name }))
            .collect::<Vec<_>>(),
    })
}

fn raw_key_tag(key: &RawKey) -> Value {
    match key {
        RawKey::Static(s) => json!(format!("Static({s})")),
        other => json!(format!("{other:?}")),
    }
}

fn raw_member_kind_tag(kind: RawMemberKind) -> Value {
    json!(format!("{kind:?}"))
}

fn member_visibility_tag(v: verter_type_expr::MemberVisibility) -> Value {
    json!(format!("{v:?}"))
}

fn tuple_element_shape_tag(t: TupleElementShape) -> Value {
    json!(format!("{t:?}"))
}

pub(crate) fn raw_decl_kind_tag(kind: RawDeclKind) -> &'static str {
    match kind {
        RawDeclKind::TypeAlias => "TypeAlias",
        RawDeclKind::Interface => "Interface",
        RawDeclKind::Enum => "Enum",
        RawDeclKind::Class => "Class",
        RawDeclKind::Function => "Function",
        RawDeclKind::Variable => "Variable",
    }
}

pub(crate) fn symbol_space_tag(space: SymbolSpace) -> &'static str {
    match space {
        SymbolSpace::Type => "Type",
        SymbolSpace::Value => "Value",
    }
}
