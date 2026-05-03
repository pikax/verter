//! ComponentConfig theme variant fast path (Issue #6).
//!
//! Materialises `Foo['variants'][literal_name]` and `Foo['slots']`
//! field types directly from the prepared `theme` value declaration's
//! object shape when `Foo` is a workspace-owned alias of
//! `ComponentConfig<typeof theme, AppConfig, key>` AND the strict
//! legality predicate fires. On hit, the fast path publishes the
//! candidate type and skips the rescue + member-route pipeline.
//!
//! ## Strict legality (per sidecar §6.2)
//!
//! Fires only when ALL of:
//! - the field's raw type is an `IndexedAccess` route on a `Ref`
//!   whose name resolves to a workspace-owned type alias
//! - that alias body is exactly
//!   `Ref { name: "ComponentConfig", type_arguments: [typeof theme,
//!   AppConfig, key] }` (modulo alias-of-alias / parenthesized)
//! - `theme` (third type argument's value declaration) resolves to a
//!   value declaration with a literal `object_shape` (Object / TypeOf
//!   resolved object), NOT an index signature
//! - `AppConfig` parameter is `Record<...>` (Path A — strict legality
//!   for this landing). Path B (proof-cache hits keyed on
//!   `(app_config_decl_id, component_key_literal)`) is deferred until
//!   the `IndexedReady::declares_interface_app_config` shallow flag
//!   lands per the §17.7 deviation note. The cache-consultation hook
//!   is wired but always misses currently.
//! - the indexed path is exactly `['variants', literal_name]` or
//!   `['slots']` — `name` and `key` MUST be literal strings at the
//!   alias declaration site, NOT generics
//! - the alias root canonical-source is workspace-owned per
//!   `WorkspaceAccess::is_workspace_owned` (NOT path-substring
//!   `node_modules`)
//!
//! Counterfixtures (per §6.2 disallowed shapes — see
//! `component_meta_component_config_fast_path_tests`):
//! 1. project-local `AppConfig { ui?: {...} }` interface — declines
//!    until proof cache populated
//! 2. interface merging across files — declines until proof cache
//!    populated
//! 3. module augmentation adding `ui[key]` — declines until proof
//!    cache populated
//! 4. generic defaults: `AppConfig = DefaultConfig` where `DefaultConfig`
//!    has `ui[key]` — declines (default is not Record)
//! 5. index signature on prepared theme value — declines (no literal
//!    member to project)
//! 6. `key` parameter is generic / not a literal at the alias decl
//!    site — declines (no literal index path)
//! 7. workspace-package-inside-node_modules — disallows path-substring
//!    shortcut; routes through `is_workspace_owned`
//!
//! Cache backfill is constrained to the EXACT
//! `(alias_decl_id, literal_indexed_path)` MaterializeMemoDb key the
//! fast path computed; broader `Navigate` keys are NOT populated.

use std::sync::Arc;

use verter_semantic::analysis::type_expr::{
    LiteralValue, ObjectExpr, ObjectMember, TypeExpr, ValueRef,
};
use verter_semantic::analysis::type_solver::prepared::PreparedTypeDecl;

use crate::resolver_core::ResolverContext;

/// Public counter name read by tests and assertions.
pub(crate) const COMPONENT_CONFIG_FAST_PATH_HITS_COUNTER: &str =
    "component_config_theme_variant_fast_path_hits";

/// Result of a fast-path attempt.
pub(crate) enum FastPathOutcome {
    /// Fast path fired and produced a candidate type. Caller publishes
    /// it via `field_state.set_current_type(candidate)` and `continue`s.
    Hit(TypeExpr),
    /// Fast path declined — slow path must run.
    Miss,
}

/// Indexed-access path classification (`['variants', name]` or
/// `['slots']`).
enum IndexedPath {
    Variants(String),
    Slots,
}

/// entry point. Returns `FastPathOutcome::Hit(candidate)` when
/// every legality predicate passes; `FastPathOutcome::Miss` otherwise.
///
/// Caller (`materialize_component_meta_field_types`) invokes this
/// BEFORE `rescue_field` for each field. On hit, the caller
/// `field_state.set_current_type(candidate)` and skips the rescue +
/// member-route pipeline by `continue`ing the field loop.
pub(crate) fn component_config_theme_variant_fast_path(
    raw: &TypeExpr,
    scope_canonical_id: &str,
    ctx: &dyn ResolverContext,
) -> FastPathOutcome {
    // Step 1: classify the indexed-access path.
    let Some((root_name, path)) = collect_component_config_indexed_path(raw) else {
        return FastPathOutcome::Miss;
    };

    // Step 2: resolve the alias declaration on the owner scope.
    // First try same-file lookup; if that misses, follow an import
    // through `resolve_owner_direct_import`. The alias may live in a
    // different workspace-owned file (e.g. a `types.ts` module that
    // the SFC imports).
    let (alias_decl, alias_source_owned) =
        match ctx.prepared_type_decl(scope_canonical_id, root_name.as_str()) {
            Some(decl) => {
                let canonical_id = decl.root_identity.canonical_id.as_str();
                let source = if canonical_id.is_empty() {
                    scope_canonical_id.to_string()
                } else {
                    canonical_id.to_string()
                };
                (decl, source)
            }
            None => {
                let Some((import_canonical, exported_name)) =
                    ctx.resolve_owner_direct_import(scope_canonical_id, root_name.as_str())
                else {
                    return FastPathOutcome::Miss;
                };
                ctx.ensure_loaded(import_canonical.as_str());
                let Some(decl) =
                    ctx.prepared_type_decl(import_canonical.as_str(), exported_name.as_str())
                else {
                    return FastPathOutcome::Miss;
                };
                (decl, import_canonical)
            }
        };

    // Step 3: confirm alias source is workspace-owned (not
    // path-substring-based).
    let alias_source = alias_source_owned.as_str();
    if !ctx.workspace_is_workspace_owned(alias_source) {
        return FastPathOutcome::Miss;
    }

    // Step 4: classify the alias body. The alias must be a non-generic
    // `ComponentConfig<typeof theme, AppConfig, key>` (modulo single
    // alias-of-alias indirection).
    let Some(cc) = component_config_alias_classification(&alias_decl, alias_source, ctx) else {
        return FastPathOutcome::Miss;
    };

    // Step 5: AppConfig is Record<...> (Path A — strict legality).
    // Path B (proof-cache hits) is deferred until the
    // `IndexedReady::declares_interface_app_config` shallow flag is
    // available.
    if !app_config_is_record_only_or_proven(&cc, alias_source, ctx) {
        return FastPathOutcome::Miss;
    }

    // Step 6: resolve the prepared theme value to its literal Object
    // shape.
    let Some(theme_object_expr) = prepared_theme_value_object(&cc.theme_path, alias_source, ctx)
    else {
        return FastPathOutcome::Miss;
    };

    // Step 7: project the literal indexed path into the theme object
    // shape. The component key literal (third type argument) selects
    // a member; the indexed path selects further into that member.
    let TypeExpr::Object(theme_obj) = &theme_object_expr else {
        return FastPathOutcome::Miss;
    };
    let Some(component_key_member) =
        find_object_member(theme_obj.as_ref(), cc.component_key_literal.as_str())
    else {
        return FastPathOutcome::Miss;
    };
    let candidate = match path {
        IndexedPath::Variants(name) => {
            // Project `theme[key].variants[name]`.
            //
            // For the strict-legality landing the component_key_literal
            // is treated as the projection root over the theme value.
            // Two arrangements appear in practice:
            //   (a) `ComponentConfig<typeof theme, AppConfig, 'button'>`
            //       projects `theme.button.variants[name]` — i.e. the
            //       theme has a top-level component sub-shape keyed by
            //       the literal.
            //   (b) `ComponentConfig<typeof theme, AppConfig, 'variants'>`
            //       projects `theme.variants[name]` — i.e. the literal
            //       names a top-level slot of the theme directly.
            //
            // If `component_key_member` is already a literal Object
            // and the literal is `"variants"`, project `name` from it
            // directly. Otherwise descend through `.variants`.
            let TypeExpr::Object(component_obj) = component_key_member else {
                return FastPathOutcome::Miss;
            };
            if cc.component_key_literal == "variants" {
                let Some(named) = find_object_member(component_obj.as_ref(), name.as_str()) else {
                    return FastPathOutcome::Miss;
                };
                named.clone()
            } else {
                let Some(variants_member) = find_object_member(component_obj.as_ref(), "variants")
                else {
                    return FastPathOutcome::Miss;
                };
                let TypeExpr::Object(variants_obj) = variants_member else {
                    return FastPathOutcome::Miss;
                };
                let Some(named) = find_object_member(variants_obj.as_ref(), name.as_str()) else {
                    return FastPathOutcome::Miss;
                };
                named.clone()
            }
        }
        IndexedPath::Slots => {
            // Project `theme[key].slots` (arrangement a) OR
            // `theme.slots` directly (arrangement b — when the
            // literal is `"slots"`).
            let TypeExpr::Object(component_obj) = component_key_member else {
                return FastPathOutcome::Miss;
            };
            if cc.component_key_literal == "slots" {
                TypeExpr::Object(component_obj.clone())
            } else {
                let Some(slots_member) = find_object_member(component_obj.as_ref(), "slots") else {
                    return FastPathOutcome::Miss;
                };
                slots_member.clone()
            }
        }
    };

    FastPathOutcome::Hit(candidate)
}

/// Step 1 / `collect_indexed_path` — return `(root_name,
/// IndexedPath)` when `expr` is one of:
/// - `Foo['variants'][literal_name]` (or with parenthesizes /
///   alias-of-alias)
/// - `Foo['slots']`
fn collect_component_config_indexed_path(expr: &TypeExpr) -> Option<(String, IndexedPath)> {
    fn unwrap_paren(expr: &TypeExpr) -> &TypeExpr {
        match expr {
            TypeExpr::Parenthesized(inner) => unwrap_paren(inner),
            _ => expr,
        }
    }

    fn ref_name(expr: &TypeExpr) -> Option<&str> {
        match unwrap_paren(expr) {
            TypeExpr::Ref {
                name,
                type_arguments,
            } if type_arguments.is_empty() => Some(name.as_ref()),
            _ => None,
        }
    }

    fn literal_string(expr: &TypeExpr) -> Option<&str> {
        match unwrap_paren(expr) {
            TypeExpr::Literal(LiteralValue::String(s)) => Some(s.as_str()),
            _ => None,
        }
    }

    match unwrap_paren(expr) {
        TypeExpr::IndexedAccess { object, index } => {
            // Try `Foo['slots']` (single hop).
            if let Some(root) = ref_name(object) {
                if let Some(s) = literal_string(index) {
                    if s == "slots" {
                        return Some((root.to_string(), IndexedPath::Slots));
                    }
                    // Could be `Foo['variants']` for the partial path —
                    // not a complete fast-path target.
                }
            }
            // Try `Foo['variants'][literal_name]` (two hops).
            if let TypeExpr::IndexedAccess {
                object: inner_object,
                index: inner_index,
            } = unwrap_paren(object)
            {
                if let (Some(root), Some(s_outer), Some(s_inner)) = (
                    ref_name(inner_object),
                    literal_string(inner_index),
                    literal_string(index),
                ) {
                    if s_outer == "variants" {
                        return Some((
                            root.to_string(),
                            IndexedPath::Variants(s_inner.to_string()),
                        ));
                    }
                }
            }
            None
        }
        _ => None,
    }
}

/// Carries the structural classification of a `ComponentConfig<typeof
/// theme, AppConfig, key>` alias body.
struct ComponentConfigClassification {
    /// `typeof theme` value-ref path (e.g. `["theme"]`).
    theme_path: ValueRef,
    /// AppConfig type-arg position (resolved).
    app_config_type_arg: Arc<TypeExpr>,
    /// The literal component key.
    component_key_literal: String,
}

/// Step 4 helper — returns `Some(...)` only when the alias body is
/// `Ref { name: "ComponentConfig", type_arguments: [typeof theme,
/// AppConfig, "literal-key"] }` (modulo paren / alias-of-alias).
///
/// Returns `None` on any disallowed shape.
fn component_config_alias_classification(
    decl: &Arc<PreparedTypeDecl>,
    alias_source: &str,
    ctx: &dyn ResolverContext,
) -> Option<ComponentConfigClassification> {
    fn unwrap_paren(expr: &TypeExpr) -> &TypeExpr {
        match expr {
            TypeExpr::Parenthesized(inner) => unwrap_paren(inner),
            _ => expr,
        }
    }

    let body = unwrap_paren(&decl.body);
    // alias-of-alias: if body is a non-generic Ref to another local
    // alias, follow once. (Single-step is sufficient for the v0
    // landing; deeper chains fall back to the slow path.)
    let body = match body {
        TypeExpr::Ref {
            name,
            type_arguments,
        } if type_arguments.is_empty() => {
            if let Some(next) = ctx.prepared_type_decl(alias_source, name.as_ref()) {
                let next_body = unwrap_paren(&next.body);
                // Use a short-lived owned clone to escape the inner
                // borrow (we only need to inspect structurally).
                next_body.clone()
            } else {
                return None;
            }
        }
        other => other.clone(),
    };

    let TypeExpr::Ref {
        name,
        type_arguments,
    } = unwrap_paren(&body)
    else {
        return None;
    };

    if name.as_ref() != "ComponentConfig" {
        return None;
    }
    // Strict shape: 3 type args (no defaulting).
    if type_arguments.len() != 3 {
        return None;
    }

    // Type arg 0: `typeof theme`.
    let theme_path = match unwrap_paren(&type_arguments[0]) {
        TypeExpr::TypeOf(value_ref) => value_ref.clone(),
        _ => return None,
    };

    // Type arg 1: AppConfig (kept for downstream check).
    let app_config_type_arg = Arc::new(type_arguments[1].clone());

    // Type arg 2: literal string.
    let component_key_literal = match unwrap_paren(&type_arguments[2]) {
        TypeExpr::Literal(LiteralValue::String(s)) => s.clone(),
        _ => return None,
    };

    Some(ComponentConfigClassification {
        theme_path,
        app_config_type_arg,
        component_key_literal,
    })
}

/// Step 6 helper — "AppConfig is Record<...>" (Path A — strict
/// legality) OR "proof cache says no override" (Path B — deferred).
///
/// Path A: `Ref { name: "Record", type_arguments: [_, _] }` with no
/// further structural constraints. The argument may itself be a
/// `Ref` to a workspace-local alias whose body is `Record<...>` —
/// follow one alias-of-alias hop. If the alias's body is anything
/// other than `Record<...>` (e.g. an interface, an intersection, a
/// generic default chain), Path A declines.
///
/// Path B: consults
/// [`crate::project_type_store::ProjectTypeStore::app_config_no_override_proof_db`]
/// keyed on `(app_config_decl_canonical_id, component_key_literal)`.
/// The slow-path-side population is a no-op until
/// `IndexedReady::declares_interface_app_config` lands; the cache
/// always misses today and the function returns `false` on Path B.
fn app_config_is_record_only_or_proven(
    cc: &ComponentConfigClassification,
    alias_source: &str,
    ctx: &dyn ResolverContext,
) -> bool {
    fn unwrap_paren(expr: &TypeExpr) -> &TypeExpr {
        match expr {
            TypeExpr::Parenthesized(inner) => unwrap_paren(inner),
            _ => expr,
        }
    }

    fn body_is_record(expr: &TypeExpr) -> bool {
        if let TypeExpr::Ref {
            name,
            type_arguments,
        } = unwrap_paren(expr)
        {
            if name.as_ref() == "Record" && type_arguments.len() == 2 {
                return true;
            }
        }
        false
    }

    let arg = unwrap_paren(cc.app_config_type_arg.as_ref());
    if body_is_record(arg) {
        return true;
    }
    // Follow one alias-of-alias hop on a workspace-local alias.
    if let TypeExpr::Ref {
        name,
        type_arguments,
    } = arg
    {
        if type_arguments.is_empty() {
            // Resolve through the alias source first; if missing,
            // follow direct-import and look in the imported file.
            let prepared = ctx
                .prepared_type_decl(alias_source, name.as_ref())
                .or_else(|| {
                    ctx.resolve_owner_direct_import(alias_source, name.as_ref())
                        .and_then(|(canonical, exported)| {
                            ctx.prepared_type_decl(canonical.as_str(), exported.as_str())
                        })
                });
            if let Some(decl) = prepared {
                if body_is_record(unwrap_paren(&decl.body)) {
                    return true;
                }
            }
        }
    }

    // Path B (deferred). The proof cache hookup will go here:
    //
    //   let key = lookup_app_config_no_override_proof_key(cc, ctx);
    //   if let Some(_proof) = ctx.project_type_store()
    //       .app_config_no_override_proof_db()
    //       .peek(&key, |sig| ctx.validate_dep_signature(sig)) {
    //       return true;
    //   }
    //
    // TODO(follow-up): wire once the
    // `interface_merging_of_app_config_generation` counter is
    // available on the workspace API and `IndexedReady` records
    // `declares_interface_app_config`.
    false
}

/// Step 7 helper — resolve the `typeof theme` value reference to a
/// value declaration whose body is an `Object` shape with literal
/// members (NOT an index signature). Returns the body wrapped as a
/// `TypeExpr::Object`.
///
/// The value declaration may live in the alias's source file OR in
/// an imported file (the alias source's `import { theme } from
/// '...'`). We follow at most one direct-import hop.
fn prepared_theme_value_object(
    theme_path: &ValueRef,
    alias_source: &str,
    ctx: &dyn ResolverContext,
) -> Option<TypeExpr> {
    // For the strict-legality landing we accept only the simplest
    // form: a single segment naming a value declaration.
    if theme_path.path.len() != 1 {
        return None;
    }
    let theme_name = theme_path.path.first()?;

    // Try same-file lookup first; fall back to value-export
    // resolution which follows the file's value-import edges. If
    // value-export resolution misses (e.g. the file's value-import
    // edges aren't populated yet), fall back to direct-import-edge
    // resolution after `ensure_loaded` so the imported file's
    // prepared decls are visible.
    let same_file = ctx.prepared_value_decl(alias_source, theme_name.as_str());
    let cross_file_value = if same_file.is_none() {
        ctx.resolve_value_export_target(alias_source, theme_name.as_str())
            .and_then(|identity| {
                ctx.prepared_value_decl(identity.canonical_id.as_str(), identity.name.as_str())
            })
    } else {
        None
    };
    let cross_file_direct = if same_file.is_none() && cross_file_value.is_none() {
        ctx.resolve_owner_direct_import(alias_source, theme_name.as_str())
            .and_then(|(canonical, exported)| {
                ctx.ensure_loaded(canonical.as_str());
                ctx.prepared_value_decl(canonical.as_str(), exported.as_str())
            })
    } else {
        None
    };
    let value_decl = same_file.or(cross_file_value).or(cross_file_direct)?;

    fn unwrap_paren(expr: &TypeExpr) -> &TypeExpr {
        match expr {
            TypeExpr::Parenthesized(inner) => unwrap_paren(inner),
            _ => expr,
        }
    }

    // The value must have a literal `Object` shape AND not be hidden
    // by a non-literal type_annotation. TypeScript narrows the value
    // to its annotation when one is present, so:
    //
    //   - `const theme = { ... } as const` → annotation is the
    //     literal Object; both `object_shape` AND `type_annotation`
    //     describe the literal. ACCEPT.
    //   - `const theme: Record<K, V> = { ... }` → annotation is
    //     `Ref<Record, ...>`; `object_shape` may also be present but
    //     the public type is the Record. DECLINE (counterfixture #5
    //     index signature on prepared theme).
    //   - `const theme = { ... }` (no annotation, no `as const`) →
    //     `type_annotation` may be None and `object_shape` carries
    //     the literal. ACCEPT.
    //
    // Reject shapes with index signatures — they are not literal
    // object_shapes and cannot be projected by literal key.
    let annotation = value_decl.type_annotation.as_ref().map(unwrap_paren);
    let object = match (value_decl.object_shape.as_ref(), annotation) {
        // Annotation is a literal Object → use that (it represents
        // both the type and the value).
        (_, Some(TypeExpr::Object(obj))) => obj.as_ref().clone(),
        // Annotation is non-Object (e.g. Record / Ref / generic) →
        // the public type is the annotation, NOT the literal. Decline.
        (_, Some(_)) => return None,
        // No annotation, plain literal object → accept the
        // object_shape.
        (Some(obj), None) => obj.clone(),
        // No annotation, no object_shape → not an object value.
        (None, None) => return None,
    };

    if object
        .properties
        .iter()
        .any(|m| matches!(m, ObjectMember::IndexSignature(_)))
    {
        return None;
    }

    Some(TypeExpr::Object(Arc::new(object)))
}

/// Object-shape literal-property lookup. Returns the named property's
/// type. Index signatures, methods, and call signatures are skipped.
fn find_object_member<'a>(obj: &'a ObjectExpr, name: &str) -> Option<&'a TypeExpr> {
    for member in &obj.properties {
        if let ObjectMember::Property(p) = member {
            if p.name == name {
                return Some(&p.ty);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use verter_semantic::analysis::type_expr::{ObjectExpr, ObjectMember, TypeExpr};

    fn lit_str(s: &str) -> TypeExpr {
        TypeExpr::Literal(LiteralValue::String(s.to_string()))
    }

    fn ref_unparam(name: &str) -> TypeExpr {
        TypeExpr::Ref {
            name: Arc::from(name),
            type_arguments: Arc::from([] as [TypeExpr; 0]),
        }
    }

    #[test]
    fn collect_component_config_indexed_path_recognises_variants_two_hops() {
        // Foo['variants']['variant']
        let inner = TypeExpr::IndexedAccess {
            object: Arc::new(ref_unparam("Foo")),
            index: Arc::new(lit_str("variants")),
        };
        let outer = TypeExpr::IndexedAccess {
            object: Arc::new(inner),
            index: Arc::new(lit_str("variant")),
        };
        let path = collect_component_config_indexed_path(&outer)
            .expect("two-hop variants path must classify");
        match path {
            (root, IndexedPath::Variants(name)) => {
                assert_eq!(root, "Foo");
                assert_eq!(name, "variant");
            }
            _ => panic!("expected variants path"),
        }
    }

    #[test]
    fn collect_component_config_indexed_path_recognises_slots_single_hop() {
        // Foo['slots']
        let expr = TypeExpr::IndexedAccess {
            object: Arc::new(ref_unparam("Foo")),
            index: Arc::new(lit_str("slots")),
        };
        let path = collect_component_config_indexed_path(&expr).expect("slots path must classify");
        match path {
            (root, IndexedPath::Slots) => assert_eq!(root, "Foo"),
            _ => panic!("expected slots path"),
        }
    }

    #[test]
    fn collect_component_config_indexed_path_rejects_generic_index() {
        // Foo['variants'][K] — K is a TypeParameter, not a literal.
        let inner = TypeExpr::IndexedAccess {
            object: Arc::new(ref_unparam("Foo")),
            index: Arc::new(lit_str("variants")),
        };
        let outer = TypeExpr::IndexedAccess {
            object: Arc::new(inner),
            index: Arc::new(ref_unparam("K")),
        };
        assert!(collect_component_config_indexed_path(&outer).is_none());
    }

    #[test]
    fn find_object_member_finds_named_property() {
        use verter_semantic::analysis::type_expr::ObjectProperty;
        let obj = ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "variant".to_string(),
                ty: lit_str("solid"),
                optional: false,
                readonly: false,
            })],
        };
        let hit = find_object_member(&obj, "variant").expect("must find member");
        assert!(matches!(hit, TypeExpr::Literal(_)));
        assert!(find_object_member(&obj, "missing").is_none());
    }
}
