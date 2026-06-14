#![deny(missing_docs)]
//! The Svelte public-API projector leg.
//!
//! A PURE declaration-shim renderer over the carrier's SHALLOW inventory + the
//! synthesized `default` symbol/export inventory. It runs NO `Instantiate`, NO
//! semantic dispatch, and NO OXC at render time (static-guarded by
//! `non_vue_api_projector_has_no_dispatch_or_oxc`): every input is already-cached
//! shallow state. It produces the content behind B8c's `Foo.svelte.ts` api file.
//!
//! Rendered declarations, in order:
//! 1. the D-at TYPE-ONLY import / re-export prelude — minimal `import type` lines
//!    derived from the carrier's shallow import facts for every PRESERVED type
//!    reference (unused imports dropped);
//! 2. `type __VerterProps` — the `$props()` type / legacy export-let object (refs
//!    preserved verbatim, never eagerly inlined);
//! 3. `interface __VerterInstance { $props: __VerterProps; …instance exports }`;
//! 4. `declare const __VerterComponent: { new (...args: any[]): __VerterInstance }`;
//! 5. `export default __VerterComponent`.
//!
//! `PublicApiMode::Testing` returns `None` (the testing surface is Vue-only,
//! D-ak/D-al). No new content cache — a pure cheap render over already-cached
//! shallow inputs.

use std::collections::BTreeSet;
use std::sync::Arc;

use verter_type_expr::{ObjectMember, TypeExpr};

use crate::framework::api_projector::{ComponentApiProjector, ComponentApiProjectorCtx};
use crate::resolver_core::surface_projector::render_type_expr_display;
use crate::types::{PublicApiMode, TscResponse};

/// The Svelte component-API projector.
#[derive(Debug, Default)]
pub struct SvelteComponentApiProjector;

impl ComponentApiProjector for SvelteComponentApiProjector {
    fn render_api(&self, cx: ComponentApiProjectorCtx<'_>) -> Option<TscResponse> {
        let ComponentApiProjectorCtx {
            host,
            resolved_canonical,
            file_language,
            mode,
            profile: _,
        } = cx;

        // Carrier-narrowness: the public-API surface is produced only for the
        // Svelte CARRIER row (the descriptor's carrier language), never a
        // same-adapter non-carrier row.
        let descriptor = crate::framework::descriptor::svelte_descriptor();
        if file_language.carrier_language_id() != descriptor.carrier_language.as_ref() {
            return None;
        }

        // The testing-API surface is Vue-only (D-ak/D-al) — Svelte returns None
        // for `Testing`, distinct from the `Public` mode's `Some`.
        if mode == PublicApiMode::Testing {
            return None;
        }

        // Read the ALREADY-CACHED shallow state for the resolved canonical — NO
        // OXC, NO Instantiate, NO dispatch at render time.
        let indexed = host.ensure_indexed_ready(resolved_canonical)?;
        let shallow = &indexed.shallow_state;

        // The synthesized `default` carries the instance shape
        // (`{ $props: Props, …exports }`). A `.svelte` with no synth default
        // (no props, no exports) projects no public API.
        let default_symbol = shallow.value_symbol("default")?;
        if !default_symbol.is_synthesised_component_default {
            return None;
        }
        let instance_shape = default_symbol
            .signatures
            .first()
            .and_then(|sig| sig.return_type.as_ref())?;
        let TypeExpr::Object(instance_obj) = instance_shape else {
            return None;
        };

        // Split the instance shape into the `$props` member type and the
        // instance-script export members.
        let mut props_type: Option<&TypeExpr> = None;
        let mut export_members: Vec<(&str, &TypeExpr)> = Vec::new();
        for member in &instance_obj.properties {
            if let ObjectMember::Property(prop) = member {
                if prop.name == "$props" {
                    props_type = Some(&prop.ty);
                } else {
                    export_members.push((prop.name.as_str(), &prop.ty));
                }
            }
        }

        // Collect the PRESERVED type-reference names (top-level refs in the
        // props type + export member types) so the prelude imports ONLY the
        // referenced types (unused imports dropped).
        let mut referenced: BTreeSet<String> = BTreeSet::new();
        if let Some(props) = props_type {
            collect_top_level_refs(props, &mut referenced);
        }
        for (_, ty) in &export_members {
            collect_top_level_refs(ty, &mut referenced);
        }

        let mut out = ShimBuilder::default();

        // 1. The D-at type-only import / re-export prelude — minimal
        //    `import type` lines for each PRESERVED reference whose binding the
        //    shallow import facts resolve to an import.
        for name in &referenced {
            if let Some(import) = shallow.import_target(name) {
                out.line(&render_type_only_import(name, import));
            }
        }
        if !referenced.is_empty() {
            out.blank();
        }

        // 2. `type __VerterProps = <props type>` — refs preserved verbatim,
        //    object surfaces rendered shallowly (member refs un-inlined).
        let props_text = props_type
            .map(render_shim_type)
            .unwrap_or_else(|| "{}".to_string());
        out.line(&format!("type __VerterProps = {props_text};"));

        // 3. `interface __VerterInstance { $props: __VerterProps; …exports }`.
        //    Each exported instance-script binding is an instance member. Its
        //    VALUE type stays shallow (it is resolved on demand by a consumer);
        //    the load-bearing contract is the member's PRESENCE on the instance.
        out.line("interface __VerterInstance {");
        out.line("  $props: __VerterProps;");
        for (name, _ty) in &export_members {
            out.line(&format!("  {name}: unknown;"));
        }
        out.line("}");

        // 4 + 5. The component value + default export.
        out.line("declare const __VerterComponent: { new (...args: any[]): __VerterInstance };");
        out.line("export default __VerterComponent;");

        // 6. `<script module>` exports as TOP-LEVEL named declarations. A
        //    top-level export of the shallow state that is NOT an INSTANCE member
        //    of the component is a module-script export — surface it as a
        //    top-level `export declare const` so the api file exposes the
        //    module's named exports alongside the default component.
        let instance_member_names: std::collections::BTreeSet<&str> =
            export_members.iter().map(|(name, _)| *name).collect();
        let mut module_exports: Vec<&str> = shallow
            .exports
            .keys()
            .map(String::as_str)
            .filter(|name| *name != "default" && !instance_member_names.contains(name))
            .collect();
        module_exports.sort_unstable();
        for name in module_exports {
            out.line(&format!("export declare const {name}: unknown;"));
        }

        Some(TscResponse {
            code: Arc::from(out.finish().as_str()),
            source_map: None,
        })
    }
}

/// A minimal line-oriented shim builder. The api-projector renders into it with
/// CodeTransform-style inserts (append-only line writes) — there is no
/// post-hoc string rewrite of already-built content.
#[derive(Default)]
struct ShimBuilder {
    lines: Vec<String>,
}

impl ShimBuilder {
    fn line(&mut self, text: &str) {
        self.lines.push(text.to_string());
    }
    fn blank(&mut self) {
        self.lines.push(String::new());
    }
    fn finish(self) -> String {
        let mut s = self.lines.join("\n");
        s.push('\n');
        s
    }
}

/// Render a `$props()` type for the shim, SHALLOWLY: a named ref renders as its
/// name (un-inlined), an object surface renders its members one level (each
/// member value via [`render_type_expr_display`], which itself preserves refs).
/// A type the display renderer cannot represent falls back to `unknown` — never
/// a silent inline.
fn render_shim_type(ty: &TypeExpr) -> String {
    match ty {
        TypeExpr::Object(obj) => {
            if obj.properties.is_empty() {
                return "{}".to_string();
            }
            let mut parts: Vec<String> = Vec::new();
            for member in &obj.properties {
                if let ObjectMember::Property(prop) = member {
                    let value =
                        render_type_expr_display(&prop.ty).unwrap_or_else(|| "unknown".to_string());
                    let opt = if prop.optional { "?" } else { "" };
                    parts.push(format!("{}{opt}: {value}", prop.name));
                }
            }
            format!("{{ {} }}", parts.join("; "))
        }
        // Refs / primitives / unions / etc. render through the display renderer
        // (which preserves refs un-inlined).
        _ => render_type_expr_display(ty).unwrap_or_else(|| "unknown".to_string()),
    }
}

/// Render a type-only import line from a shallow import target.
///
/// `import type { <imported> as <local> } from '<source>'` — or the plain form
/// when the local name equals the imported name. A default import
/// (`imported_name == "default"`) renders `import type <local> from '<source>'`.
fn render_type_only_import(
    local: &str,
    import: &crate::resolver_core::shallow_file_state::ImportTarget,
) -> String {
    let source = &import.source_specifier;
    if import.imported_name == "default" {
        format!("import type {local} from '{source}';")
    } else if import.imported_name == "*" {
        format!("import type * as {local} from '{source}';")
    } else if import.imported_name == local {
        format!("import type {{ {local} }} from '{source}';")
    } else {
        format!(
            "import type {{ {} as {local} }} from '{source}';",
            import.imported_name
        )
    }
}

/// Collect the TOP-LEVEL named-reference identifiers of a type expression — the
/// names that may resolve to an import (so the prelude imports only them). It
/// does NOT recurse into object member values (those stay shallow); it walks
/// unions/intersections/arrays/refs' type arguments shallowly.
fn collect_top_level_refs(expr: &TypeExpr, out: &mut BTreeSet<String>) {
    match expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            out.insert(name.to_string());
            for arg in type_arguments.iter() {
                collect_top_level_refs(arg, out);
            }
        }
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            for ty in types.iter() {
                collect_top_level_refs(ty, out);
            }
        }
        TypeExpr::Array { element, .. } => collect_top_level_refs(element, out),
        TypeExpr::Object(obj) => {
            // An object props surface (`{ row: Snippet }`, the legacy
            // export-let object) is rendered ONE level into the shim, so its
            // member value refs are preserved references that the prelude must
            // import. The walk stays one level — it does NOT recurse into a
            // member's own object body (that stays shallow / re-resolved).
            for member in &obj.properties {
                if let ObjectMember::Property(prop) = member {
                    if let TypeExpr::Ref {
                        name,
                        type_arguments,
                    } = &prop.ty
                    {
                        out.insert(name.to_string());
                        for arg in type_arguments.iter() {
                            collect_top_level_refs(arg, out);
                        }
                    }
                }
            }
        }
        // Every other node kind carries no top-level import reference.
        _ => {}
    }
}
