#![deny(missing_docs)]
//! The Svelte public-API projector leg.
//!
//! A PURE declaration-shim renderer over the carrier's SHALLOW inventory + the
//! synthesized `default` symbol/export inventory. It runs NO `Instantiate`, NO
//! semantic dispatch, and NO OXC at render time (static-guarded by
//! `non_vue_api_projector_has_no_dispatch_or_oxc`): every input is already-cached
//! shallow state. It produces the content behind the `Foo.svelte.verter.ts`
//! api file.
//!
//! Rendered declarations, in order:
//! 1. the TYPE-ONLY import / re-export prelude — minimal `import type` lines
//!    derived from the carrier's shallow import facts for every PRESERVED type
//!    reference (unused imports dropped);
//! 2. `type __VerterProps` — the `$props()` type / legacy export-let object (refs
//!    preserved verbatim, never eagerly inlined);
//! 3. `interface __VerterInstance { $props: __VerterProps; …instance exports }`;
//! 4. `declare const __VerterComponent: { new (...args: any[]): __VerterInstance }`;
//! 5. `export default __VerterComponent`.
//!
//! `PublicApiMode::Testing` returns `None` (the testing surface is
//! Vue-only). No new content cache — a pure cheap render over already-cached
//! shallow inputs.

use std::collections::BTreeSet;
use std::sync::Arc;

use verter_type_expr::facts::{
    FactOrLocator, LeafTypeFact, ResolvedLocalShape, SemanticTypeSource,
};

use crate::framework::api_projector::{ComponentApiProjector, ComponentApiProjectorCtx};
use crate::types::{PublicApiMode, TscResponse};

/// The F13 derived-callback-event helper types rendered into every Svelte
/// `.svelte.verter.ts` shim.
///
/// The `$events` map values are HANDLER types uniformly (the component `on:`
/// helper checks `handler: $events[K]`), produced from the TWO event models:
/// - `__VerterCallbackEvents<P>` — the DERIVED mapped type over the props `P`:
///   each static key matching the `on${E}` callback convention (NON-EMPTY suffix
///   `E`) whose value is function-like remaps to event `E` whose handler type is
///   the callback function ITSELF (already a handler). A non-`on` key, an empty
///   suffix, or a non-function value drops out (`never` key).
/// - `__VerterDispatcherEvents<E>` — over the legacy `createEventDispatcher<E>`
///   map, each event `K` with payload `E[K]` becomes the Svelte legacy handler
///   shape `(e: CustomEvent<E[K]>) => void` (a legacy `on:save` handler receives
///   the dispatched `CustomEvent`). The detail type is EXACT (`CustomEvent<E[K]>`,
///   never `CustomEvent<any>`).
///
/// `__VerterFunction<T>` extracts the function arm of a (possibly optional)
/// callback-prop value. TSGO resolves the mapped types at check time — the
/// projector performs NO type resolution here.
const EVENTS_HELPER_PRELUDE: &str = "type __VerterFunction<T> = Extract<NonNullable<T>, (...a: any[]) => any>;
type __VerterCallbackEvents<P> = {\n  [K in keyof P as K extends `on${infer E}`\n    ? (E extends \"\" ? never : __VerterFunction<P[K]> extends never ? never : E)\n    : never]: __VerterFunction<P[K]>\n};
type __VerterDispatcherEvents<E> = { [K in keyof E]: (e: CustomEvent<E[K]>) => void };";

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
            // The Svelte shim renders purely from cached shallow state — it
            // runs no cross-file macro-type resolution, so the batch-shared
            // cold seed / session view is accepted and ignored here.
            render_seed: _,
        } = cx;

        // Carrier-narrowness: the public-API surface is produced only for the
        // Svelte CARRIER row (the descriptor's carrier language), never a
        // same-adapter non-carrier row.
        let descriptor = crate::framework::descriptor::svelte_descriptor();
        if file_language.carrier_language_id() != descriptor.carrier_language.as_ref() {
            return None;
        }

        // Mode handling is EXPLICIT (no silent fall-through for a future
        // `PublicApiMode` variant):
        // - `Public` and `Declaration` BOTH render the same shim. The Svelte
        //   shim is already a STRICTLY VALID `.d.ts` — pure declarations only
        //   (type-only imports, `type`/`interface`, `declare const … export
        //   default …`), no runtime/value statements — so the declaration
        //   carrier surface reuses it directly.
        // - `Testing` is the Vue-only debug surface — Svelte returns `None`,
        //   distinct from the rendered `Some`.
        match mode {
            PublicApiMode::Public | PublicApiMode::Declaration => {}
            PublicApiMode::Testing => return None,
        }

        // Read the ALREADY-CACHED shallow state for the resolved canonical — NO
        // OXC, NO Instantiate, NO dispatch at render time.
        let indexed = host.ensure_indexed_ready_serve(resolved_canonical)?.indexed;
        let shallow = &indexed.shallow_state;

        // The synthesized `default` carries the instance shape
        // (`{ $props: Props, …exports }`). A `.svelte` with no synth default
        // (no props, no exports) projects no public API.
        let default_symbol = shallow.value_symbol("default")?;
        if !default_symbol.is_synthesised_component_default {
            return None;
        }
        // The instance shape rides the synthesized BODY's annotation-borne
        // closed SOURCE (`LoweredValueDecl.type_annotation.annotation` =
        // `Synthesized(Object(members))`); the synth's construct signature
        // deliberately carries no authored return position (`return_ty` is an
        // honest `None`), so the annotation source is the shape authority.
        let default_body = shallow.value_decl("default")?;
        let instance_source = default_body.type_annotation.annotation.as_ref()?;
        let SemanticTypeSource::Synthesized(ResolvedLocalShape::Object(instance_members)) =
            instance_source
        else {
            return None;
        };

        // Split the instance shape into the `$props` member fact, the synthesized
        // `$events` (legacy dispatcher map) / `$slots` (snippet member keys)
        // surfaces, and the instance-script export members.
        let mut props_type: Option<&FactOrLocator> = None;
        let mut events_type: Option<&FactOrLocator> = None;
        let mut slot_keys: Vec<&str> = Vec::new();
        let mut export_members: Vec<(&str, &FactOrLocator)> = Vec::new();
        for member in instance_members.iter() {
            match member.name.as_str() {
                "$props" => props_type = Some(&member.ty),
                "$events" => events_type = Some(&member.ty),
                "$slots" => {
                    if let FactOrLocator::LeafObject(slots) = &member.ty {
                        for slot in slots.iter() {
                            slot_keys.push(slot.name.as_str());
                        }
                    }
                }
                _ => export_members.push((member.name.as_str(), &member.ty)),
            }
        }

        // Collect the PRESERVED type-reference names (leaf refs in the props
        // fact + the dispatcher event-map fact + export member facts) so the
        // prelude imports ONLY the referenced types (unused imports dropped).
        // Locator-backed facts carry no name — honestly nothing to import.
        let mut referenced: BTreeSet<String> = BTreeSet::new();
        if let Some(props) = props_type {
            collect_fact_refs(props, &mut referenced);
        }
        if let Some(events) = events_type {
            collect_fact_refs(events, &mut referenced);
        }
        for (_, ty) in &export_members {
            collect_fact_refs(ty, &mut referenced);
        }

        let mut out = ShimBuilder::default();

        // 1. The type-only import / re-export prelude — minimal
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

        // 2. `type __VerterProps = <props type>` — leaf refs preserved
        //    verbatim, leaf-object surfaces rendered shallowly (member refs
        //    un-inlined). An authored-payload LOCATOR carries no display text
        //    — it renders the honest `unknown`, never a fabricated shape.
        let props_text = props_type
            .map(render_shim_fact)
            .unwrap_or_else(|| "{}".to_string());
        out.line(&format!("type __VerterProps = {props_text};"));

        // The F13 derived-callback-event helpers. `$events` is a DERIVED,
        // NON-AUTHORITATIVE compatibility index — `$props` stays authoritative for
        // modern event correctness. The callback-prop events are a MAPPED type over
        // `__VerterProps`: each `on${E}` key whose value is function-like maps to
        // event `E` with the callback's parameters as the handler type. TSGO
        // resolves the mapped type at check time (no projector-time dispatch).
        out.line(EVENTS_HELPER_PRELUDE);
        // The shim `$events` index: the derived callback-prop events UNIONed with
        // the legacy dispatcher event map (when present). A consumer's
        // `["$events"][K]` indexes the payload precisely; an unknown event name
        // FAILS the `keyof` index.
        let events_text = match events_type {
            // The dispatcher map carries PAYLOAD types per event; wrap it in
            // `__VerterDispatcherEvents` so its values become the legacy HANDLER
            // shape `(e: CustomEvent<payload>) => void` — uniform with the
            // callback-prop handlers so `$events[K]` is always a handler type.
            Some(events) => format!(
                "__VerterCallbackEvents<__VerterProps> & __VerterDispatcherEvents<{}>",
                render_shim_fact(events)
            ),
            None => "__VerterCallbackEvents<__VerterProps>".to_string(),
        };
        out.line(&format!("type __VerterEventsSurface = {events_text};"));

        // The shim `$slots` index: an EXACT key map whose values are the snippet
        // callables, rendered shallow as `__VerterProps[K]` (the snippet prop's own
        // `Snippet<…>` type — the precise binding type the consumer re-resolves).
        // A consumer's `["$slots"][K]` is name-exact; an unknown slot name FAILS.
        let slots_text = if slot_keys.is_empty() {
            "{}".to_string()
        } else {
            let entries: Vec<String> = slot_keys
                .iter()
                .map(|k| format!("{k}: __VerterProps[\"{k}\"]"))
                .collect();
            format!("{{ {} }}", entries.join("; "))
        };
        out.line(&format!("type __VerterSlotsSurface = {slots_text};"));

        // 3. `interface __VerterInstance { $props; $events; $slots; …exports }`.
        //    Each exported instance-script binding is an instance member. Its
        //    VALUE type stays shallow (it is resolved on demand by a consumer);
        //    the load-bearing contract is the member's PRESENCE on the instance.
        out.line("interface __VerterInstance {");
        out.line("  $props: __VerterProps;");
        out.line("  $events: __VerterEventsSurface;");
        out.line("  $slots: __VerterSlotsSurface;");
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
/// member value via [`render_leaf_display`], which itself preserves refs).
/// A locator-backed fact carries no display text — it renders the honest
/// `unknown`, never a fabricated shape and never a resolve.
fn render_shim_fact(ty: &FactOrLocator) -> String {
    match ty {
        FactOrLocator::LeafObject(members) => {
            if members.is_empty() {
                return "{}".to_string();
            }
            let mut parts: Vec<String> = Vec::new();
            for member in members.iter() {
                let value = render_leaf_display(&member.ty);
                let opt = if member.optional { "?" } else { "" };
                parts.push(format!("{}{opt}: {value}", member.name));
            }
            format!("{{ {} }}", parts.join("; "))
        }
        FactOrLocator::Leaf(leaf) => render_leaf_display(leaf),
        // A closed union of leaves renders each arm's leaf display, joined as
        // the authored union syntax (an empty union has no display text — the
        // honest render is `unknown`).
        FactOrLocator::LeafUnion(leaves) => {
            if leaves.is_empty() {
                return "unknown".to_string();
            }
            leaves
                .iter()
                .map(render_leaf_display)
                .collect::<Vec<_>>()
                .join(" | ")
        }
        // An authored payload / body position is a content-free LOCATOR: no
        // display text exists here, and this projector never resolves — the
        // honest render is `unknown`.
        FactOrLocator::Locator(_) | FactOrLocator::MacroPayload(_) => "unknown".to_string(),
    }
}

/// Render a closed LEAF fact for the shim: primitives by canonical name,
/// literals verbatim, bare refs by name (un-inlined).
fn render_leaf_display(leaf: &LeafTypeFact) -> String {
    match leaf {
        LeafTypeFact::Primitive(name) => name.as_str().to_string(),
        LeafTypeFact::StringLiteral(text) => format!("\"{text}\""),
        LeafTypeFact::NumberLiteral(text) => text.clone(),
        LeafTypeFact::BooleanLiteral(flag) => flag.to_string(),
        LeafTypeFact::Ref(name) => name.clone(),
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

/// Collect the named-reference identifiers of a member FACT — the names that
/// may resolve to an import (so the prelude imports only them). A leaf-object
/// props surface (`{ row: Snippet }`, the legacy export-let map) is rendered
/// ONE level into the shim, so its member value refs are preserved references
/// the prelude must import. A locator-backed fact carries no name — honestly
/// nothing to import (the consumer re-resolves the authored position).
fn collect_fact_refs(fact: &FactOrLocator, out: &mut BTreeSet<String>) {
    match fact {
        FactOrLocator::Leaf(LeafTypeFact::Ref(name)) => {
            out.insert(name.clone());
        }
        // A closed union of leaves contributes each leaf `Ref` name.
        FactOrLocator::LeafUnion(leaves) => {
            for leaf in leaves.iter() {
                if let LeafTypeFact::Ref(name) = leaf {
                    out.insert(name.clone());
                }
            }
        }
        FactOrLocator::LeafObject(members) => {
            for member in members.iter() {
                if let LeafTypeFact::Ref(name) = &member.ty {
                    out.insert(name.clone());
                }
            }
        }
        FactOrLocator::Leaf(_) | FactOrLocator::Locator(_) | FactOrLocator::MacroPayload(_) => {}
    }
}
