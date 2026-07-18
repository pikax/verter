#![deny(missing_docs)]
//! The Svelte public-API projector leg.
//!
//! A declaration-shim renderer over the carrier's cached shallow inventory and
//! typed Svelte script facts. OXC captures `$props()`/dispatcher authored
//! payload locators once during analysis; this projector never reparses or
//! scans source. Local aliases and interfaces are dereferenced through the one
//! shared framework-surface executor, preserving its cycle, budget, and cache
//! semantics. It produces the content behind `Foo.svelte.verter.ts`.
//!
//! Rendered declarations, in order:
//! 1. the TYPE-ONLY import / re-export prelude — minimal `import type` lines
//!    derived from the carrier's shallow import facts for every PRESERVED type
//!    reference (unused imports dropped);
//! 2. an authored-name component value implementing Svelte 5's native callable
//!    `Component<Props, Exports, Bindings>` contract;
//! 3. the authored-name component value as the default export.
//!
//! `PublicApiMode::Testing` returns `None` (the testing surface is Vue-only).
//! No projector-specific content cache is introduced.

use std::collections::BTreeSet;
use std::sync::Arc;

use verter_type_expr::facts::{
    FactOrLocator, LeafTypeFact, ResolvedLocalShape, SemanticTypeSource,
};

use crate::framework::api_projector::{
    ComponentApiProjection, ComponentApiProjector, ComponentApiProjectorCtx,
    ComponentPublicContract, ComponentPublicProp,
};
use crate::types::{PublicApiMode, TscResponse};

/// The Svelte component-API projector.
#[derive(Debug, Default)]
pub struct SvelteComponentApiProjector;

impl ComponentApiProjector for SvelteComponentApiProjector {
    fn render_api(&self, cx: ComponentApiProjectorCtx<'_>) -> Option<ComponentApiProjection> {
        let ComponentApiProjectorCtx {
            host,
            resolved_canonical,
            file_language,
            mode,
            profile: _,
            render_seed,
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

        // Read the already-cached shallow state for the resolved canonical.
        // Source parsing happened during analysis; this render never invokes
        // OXC or scans the source text.
        let indexed = host.ensure_indexed_ready_serve(resolved_canonical)?.indexed;
        let shallow = &indexed.shallow_state;
        let component_generics = svelte_component_generics(&indexed);

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

        // Split the instance shape into props, the legacy dispatcher map, and
        // instance exports. Svelte 5 snippets remain ordinary Props members;
        // the native callable Component has no class-like `$slots` member.
        let mut props_type: Option<&FactOrLocator> = None;
        let mut events_type: Option<&FactOrLocator> = None;
        let mut export_members: Vec<(&str, &FactOrLocator)> = Vec::new();
        for member in instance_members.iter() {
            match member.name.as_str() {
                "$props" => props_type = Some(&member.ty),
                "$events" => events_type = Some(&member.ty),
                "$slots" => {}
                _ => export_members.push((member.name.as_str(), &member.ty)),
            }
        }

        // One request-bound resolver context for every authored payload this
        // declaration needs. The AST capture already found `$props()` and the
        // dispatcher; this context only dereferences/normalizes their typed
        // locators through the shared semantic engine.
        let resolver_ctx = render_seed.as_ref().map(|seed| {
            crate::resolver_core::HostResolverContext::from_cold_seed(
                host,
                seed.cold_seed,
                Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
            )
        });
        let resolver_ctx = resolver_ctx
            .as_ref()
            .map(|ctx| ctx as &dyn crate::resolver_core::ResolverContext);
        let script_facts = resolver_ctx
            .and_then(|ctx| host.resolve_svelte_script_facts_with_ctx(ctx, resolved_canonical));
        let resolved_exports =
            resolver_ctx.and_then(|ctx| resolve_public_exports_text(host, ctx, resolved_canonical));

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
        if let Some(facts) = script_facts.as_ref() {
            referenced.extend(facts.props_type_references.iter().cloned());
            referenced.extend(facts.dispatcher_event_references.iter().cloned());
        }
        if let Some(exports) = resolved_exports.as_ref() {
            referenced.extend(exports.type_references.iter().cloned());
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

        // 2. Render the public component inputs. Leaf refs stay refs and object
        //    surfaces stay shallow. Locator-backed local aliases/interfaces are
        //    dereferenced through the shared Svelte surface executor; the
        //    shallow fact remains the safe fallback for a genuine miss.
        let shallow_props_text = props_type
            .map(render_shim_fact)
            .unwrap_or_else(|| "{}".to_string());
        let captured_props_text = script_facts.as_ref().and_then(|facts| {
            captured_display_without_local_refs(
                facts.props_type_display.as_deref(),
                &facts.props_type_references,
                shallow,
            )
        });
        let resolved_props =
            resolver_ctx.and_then(|ctx| resolve_public_props_text(host, ctx, resolved_canonical));
        let resolved_props_text = resolved_props.as_ref().map(|props| props.text.clone());
        let props_text = captured_props_text
            .or_else(|| resolved_props_text.clone())
            .unwrap_or(shallow_props_text);
        let component_props_text = object_type_text(&props_text)
            .map(str::to_string)
            .or(resolved_props_text)
            .unwrap_or_else(|| render_component_props(props_type, &props_text));
        let dispatcher_text = script_facts
            .as_ref()
            .and_then(|facts| {
                captured_display_without_local_refs(
                    facts.dispatcher_events_display.as_deref(),
                    &facts.dispatcher_event_references,
                    shallow,
                )
            })
            .or_else(|| {
                resolver_ctx
                    .and_then(|ctx| resolve_public_dispatcher_text(host, ctx, resolved_canonical))
            });
        let component_props_text =
            render_native_component_props(&component_props_text, dispatcher_text.as_deref());
        // Prop NAME-token map anchors apply ONLY when the props fragment that
        // ships IS the resolved render byte-for-byte — the captured authored
        // display takes rendering precedence and may differ in formatting, in
        // which case anchors recorded against the resolved render would index
        // the wrong bytes. The legacy dispatcher wrapper re-renders the
        // fragment one byte in (`({props}) & …`). Any other shape ships NO
        // prop mappings — unmapped is honest (the consumer remaps fail
        // closed).
        let (prop_name_mappings, props_fragment_base): (&[PropNameMapping], usize) =
            match resolved_props.as_ref() {
                Some(resolved)
                    if !resolved.name_mappings.is_empty()
                        && component_props_text == resolved.text =>
                {
                    (&resolved.name_mappings, 0)
                }
                Some(resolved)
                    if !resolved.name_mappings.is_empty()
                        && component_props_text.starts_with(&format!("({})", resolved.text)) =>
                {
                    (&resolved.name_mappings, 1)
                }
                _ => (&[], 0),
            };
        let bindings_text = render_native_bindings(script_facts.as_deref());

        // 3. Build an authored-name public value. `Component<Props, Exports,
        //    Bindings>` is Svelte 5's framework-native import contract. Visible
        //    members are inlined so quick-info never leaks projector aliases.
        let component_name = public_component_name(
            resolved_canonical,
            descriptor.carrier_extension().as_deref(),
            referenced.iter().map(String::as_str).chain(
                shallow
                    .exports
                    .keys()
                    .map(String::as_str)
                    .chain(export_members.iter().map(|(name, _)| *name)),
            ),
        );
        let exports_text = resolved_exports
            .as_ref()
            .map(|exports| exports.text.clone())
            .unwrap_or_else(|| render_member_object(&export_members));
        if let Some(generics) = component_generics {
            let native = format!(
                "import(\"svelte\").Component<{component_props_text}, {exports_text}, {bindings_text}>"
            );
            out.line(&format!("declare const {component_name}: {{"));
            // The props fragment appears TWICE in the call signature
            // (`Parameters<…>` and `ReturnType<…>`); both occurrences map to
            // the same authored prop members.
            let signature =
                format!("  <{generics}>(...args: Parameters<{native}>): ReturnType<{native}>;");
            let props_in_native = "import(\"svelte\").Component<".len() + props_fragment_base;
            let first_native = format!("  <{generics}>(...args: Parameters<").len();
            let second_native = first_native + native.len() + "): ReturnType<".len();
            out.line_mapped(
                &signature,
                &[
                    first_native + props_in_native,
                    second_native + props_in_native,
                ],
                prop_name_mappings,
            );
            out.line(&format!("  z_$$bindings?: {bindings_text};"));
            out.line("};");
        } else {
            out.line(&format!(
                "declare const {component_name}: import(\"svelte\").Component<"
            ));
            out.line_mapped(
                &format!("  {component_props_text},"),
                &[2 + props_fragment_base],
                prop_name_mappings,
            );
            out.line(&format!("  {exports_text},"));
            out.line(&format!("  {bindings_text}"));
            out.line(">;");
        }

        // 4. The component value is the module's default public API.
        out.line(&format!("export default {component_name};"));

        // 5. `<script module>` exports as TOP-LEVEL named declarations. A
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

        let (code, name_mappings) = out.finish();
        let source_map = build_api_source_map(
            &code,
            &indexed.raw_source,
            resolved_canonical,
            &name_mappings,
        );

        Some(ComponentApiProjection {
            response: TscResponse {
                code: Arc::from(code.as_str()),
                source_map,
            },
            contract: resolved_props.map(|props| ComponentPublicContract { props: props.props }),
        })
    }
}

struct ResolvedPublicProps {
    text: String,
    props: Vec<ComponentPublicProp>,
    /// Prop NAME-token map anchors: each rendered prop name's byte offset
    /// within [`Self::text`] paired with its authored SFC-absolute byte offset
    /// (the `$props()` annotation member / local interface member name). Only
    /// props with a typed LOCAL origin contribute an anchor; the anchors are
    /// valid ONLY while [`Self::text`] ships byte-identical in the carrier.
    name_mappings: Vec<PropNameMapping>,
}

/// One prop NAME-token source-map anchor: the rendered name token's byte
/// offset within the resolved props fragment plus the prop's authored
/// SFC-absolute byte offset, with the name both sides must byte-match before
/// the mapping is admitted (fail closed on any drift).
#[derive(Debug, Clone)]
struct PropNameMapping {
    /// Byte offset of the rendered name token within the props fragment.
    offset_in_fragment: usize,
    /// Authored SFC-absolute byte offset of the prop-name token.
    source_start: u32,
    /// The prop name both the generated and authored tokens must spell.
    name: String,
}

struct ResolvedPublicExports {
    text: String,
    type_references: BTreeSet<String>,
}

/// Read the authored Svelte tooling `generics="..."` declaration from the
/// typed parse carrier. The parser owns the attribute/value span; this is a
/// direct parse-domain read over the indexed source, never a text rescan.
fn svelte_component_generics(indexed: &crate::project_type_store::IndexedReady) -> Option<String> {
    use verter_compiler::svelte::parser::template_ast::{
        SvelteAttributeKind, SvelteAttributeValue,
    };

    let parsed = indexed
        .framework_parse
        .as_deref()
        .and_then(crate::typeinfo::adapters::svelte::svelte_parse)?;
    let script = parsed.instance_script.as_ref()?;
    script.attributes.iter().find_map(|attribute| {
        let SvelteAttributeKind::Plain {
            name,
            value: Some(value),
            ..
        } = &attribute.kind
        else {
            return None;
        };
        if name != "generics" {
            return None;
        }
        // The parser classifies quoted values containing TypeScript punctuation
        // as `Mixed`, while a simple identifier remains `Text`. In either case
        // the value span is the parser-owned exact interior of the quotes.
        let span = match value {
            SvelteAttributeValue::Text(span) | SvelteAttributeValue::Mixed(span) => span,
            SvelteAttributeValue::Expression(_) => return None,
        };
        let text = indexed
            .raw_source
            .get(span.start as usize..span.end as usize)?
            .trim();
        (!text.is_empty()).then(|| text.to_string())
    })
}

/// Derive a stable user-facing component identity from the authored component file.
/// Occupied imported/exported bindings are avoided deterministically so the
/// declaration shim remains valid even when a component imports its namesake.
fn public_component_name<'a>(
    canonical: &str,
    carrier_extension: Option<&str>,
    occupied: impl IntoIterator<Item = &'a str>,
) -> String {
    let file_name = canonical
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("SvelteComponent");
    let stem = carrier_extension
        .and_then(|extension| file_name.strip_suffix(extension))
        .unwrap_or(file_name);
    let mut base = String::new();
    let mut capitalize = true;
    for ch in stem.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '$' {
            if base.is_empty() && ch.is_ascii_digit() {
                base.push('_');
            }
            if capitalize && ch.is_ascii_lowercase() {
                base.push(ch.to_ascii_uppercase());
            } else {
                base.push(ch);
            }
            capitalize = false;
        } else {
            capitalize = true;
        }
    }
    if base.is_empty() || base == "default" {
        base = "SvelteComponent".to_string();
    }
    let occupied: BTreeSet<&str> = occupied.into_iter().collect();
    let mut candidate = base;
    while occupied.contains(candidate.as_str()) {
        candidate.push_str("Component");
    }
    candidate
}

/// Resolve the public props object through the one shared semantic dispatch.
///
/// This is required for the normal Svelte spelling `interface Props { … };
/// let { … }: Props = $props()`: the shallow synthetic default intentionally
/// carries only the authored payload locator, so rendering that locator as a
/// finished `unknown` would erase the component's public contract. The same
/// framework-surface executor used by component-meta performs the demand and
/// preserves cache/partial semantics; this projector only formats its already
/// resolved one-level rows. A partial outcome contributes its best safe rows
/// and is never admitted by the executor's surface cache.
fn resolve_public_props_text(
    host: &crate::VerterHost,
    ctx: &dyn crate::resolver_core::ResolverContext,
    owner: &str,
) -> Option<ResolvedPublicProps> {
    use crate::typeinfo::framework_surface::{ResolvedOutcome, SvelteSurfaceSource};

    let runes = crate::typeinfo::framework_surface::svelte_exec::resolve_svelte_surface(
        host,
        ctx,
        owner,
        SvelteSurfaceSource::RunesProps,
    );
    let outcome = if matches!(runes, ResolvedOutcome::Missing) {
        crate::typeinfo::framework_surface::svelte_exec::resolve_svelte_surface(
            host,
            ctx,
            owner,
            SvelteSurfaceSource::LegacyExportLet,
        )
    } else {
        runes
    };
    let props = outcome.value()?.props.as_ref()?;

    // The display rows are exact for named properties. Index signatures carry
    // semantic source positions whose faithful rendering belongs to a typed
    // output materializer; until that vocabulary is exposed here, retain the
    // locator-backed shallow fallback instead of silently dropping an index.
    if !props.index_signatures.is_empty() {
        return None;
    }

    let contract_props = props
        .fields
        .iter()
        .map(|field| {
            let has_default = props
                .prop_defaults
                .iter()
                .any(|default| default.key == field.analysis.name);
            ComponentPublicProp {
                name: field.analysis.name.clone(),
                type_annotation: field.analysis.type_annotation.clone(),
                optional: field.analysis.is_optional || has_default,
                has_default,
            }
        })
        .collect::<Vec<_>>();
    // The authored NAME-token span per prop, from the surface's typed
    // member-declaration origins: a prop DECLARED in the owner file (a `Local`
    // hop — the `$props()` annotation member, or a local interface member)
    // carries its SFC-absolute member-declaration span, which STARTS at the
    // member name token. A prop declared in an imported module (an `Import`
    // hop) maps to ANOTHER file — out of scope for this single-source carrier
    // map, so it contributes NO anchor (honest unmapped, never a cross-file
    // guess). The final assembly byte-verifies each anchor against the raw
    // source before admitting it (fail closed on any drift).
    let local_name_starts: std::collections::HashMap<&str, u32> = props
        .prop_origins
        .iter()
        .filter(|entry| {
            entry.origin.declaration.canonical_source == owner
                && matches!(
                    entry.origin.chain.as_slice(),
                    [crate::typeinfo::framework_surface::results::OriginHop::Local]
                )
        })
        .map(|entry| {
            (
                entry.prop_name.as_str(),
                entry.origin.declaration.span.start,
            )
        })
        .collect();
    let mut name_mappings: Vec<PropNameMapping> = Vec::new();
    let mut text = String::new();
    if contract_props.is_empty() {
        text.push_str("{}");
    } else {
        text.push_str("{ ");
        for (index, field) in contract_props.iter().enumerate() {
            if index > 0 {
                text.push_str("; ");
            }
            if let Some(&source_start) = local_name_starts.get(field.name.as_str()) {
                name_mappings.push(PropNameMapping {
                    offset_in_fragment: text.len(),
                    source_start,
                    name: field.name.clone(),
                });
            }
            let name = render_property_name(&field.name);
            let optional = if field.optional { "?" } else { "" };
            let ty = field.type_annotation.as_deref().unwrap_or("unknown");
            text.push_str(&format!("{name}{optional}: {ty}"));
        }
        text.push_str(" }");
    }
    Some(ResolvedPublicProps {
        text,
        props: contract_props,
        name_mappings,
    })
}

/// Resolve a legacy `createEventDispatcher<Events>()` map through the shared
/// Svelte framework-surface executor.
///
/// This is the dispatcher twin of [`resolve_public_props_text`]. A local
/// `interface Events` does not exist in the generated declaration module, so
/// rendering the captured locator/name would be invalid. The shared executor
/// dereferences it once and returns the one-level event rows; partial outcomes
/// retain their best safe rows and are never admitted to the surface cache.
fn resolve_public_dispatcher_text(
    host: &crate::VerterHost,
    ctx: &dyn crate::resolver_core::ResolverContext,
    owner: &str,
) -> Option<String> {
    use crate::typeinfo::framework_surface::SvelteSurfaceSource;

    let outcome = crate::typeinfo::framework_surface::svelte_exec::resolve_svelte_surface(
        host,
        ctx,
        owner,
        SvelteSurfaceSource::LegacyDispatcher,
    );
    let events = outcome.value()?.emits.as_ref()?;
    if !events.index_signatures.is_empty() {
        return None;
    }

    let fields = events
        .fields
        .iter()
        .map(|field| {
            let name = render_property_name(&field.analysis.name);
            let payload = field.analysis.payload_type.as_deref().unwrap_or("unknown");
            format!("{name}: {payload}")
        })
        .collect::<Vec<_>>();
    if fields.is_empty() {
        Some("{}".to_string())
    } else {
        Some(format!("{{ {} }}", fields.join("; ")))
    }
}

/// Resolve public instance value exports through the same typed Svelte surface
/// executor that component-meta uses. Each field is rooted at the local value
/// binding's `typeof`, so an alias export keeps the public key while deriving
/// its type from the real local identity.
fn resolve_public_exports_text(
    host: &crate::VerterHost,
    ctx: &dyn crate::resolver_core::ResolverContext,
    owner: &str,
) -> Option<ResolvedPublicExports> {
    use crate::typeinfo::framework_surface::results::NamedTypeMemberOutput;
    use crate::typeinfo::framework_surface::SvelteSurfaceSource;

    let outcome = crate::typeinfo::framework_surface::svelte_exec::resolve_svelte_surface(
        host,
        ctx,
        owner,
        SvelteSurfaceSource::InstanceExports,
    );
    let expose = outcome.value()?.expose.as_ref()?;
    let mut type_references = BTreeSet::new();
    let fields = expose
        .members
        .iter()
        .map(|member| {
            type_references.extend(member.type_references.iter().cloned());
            let name = render_property_name(&member.name);
            let optional = if member.is_optional { "?" } else { "" };
            let ty = member
                .type_annotation
                .clone()
                .or_else(|| match member.value.as_ref() {
                    Some(NamedTypeMemberOutput::Primitive(name)) => Some(name.as_str().to_string()),
                    Some(NamedTypeMemberOutput::Literal(value)) => Some(match value {
                        verter_type_expr::LiteralValue::String(value) => format!("{value:?}"),
                        verter_type_expr::LiteralValue::Number(value) => value.to_string(),
                        verter_type_expr::LiteralValue::BigInt(value) => value.clone(),
                        verter_type_expr::LiteralValue::Boolean(value) => value.to_string(),
                    }),
                    Some(NamedTypeMemberOutput::EmptyObject) => Some("{}".to_string()),
                    Some(NamedTypeMemberOutput::Ref { name }) => Some(name.to_string()),
                    Some(NamedTypeMemberOutput::Opaque) | None => None,
                })
                .unwrap_or_else(|| "unknown".to_string());
            format!("{name}{optional}: {ty}")
        })
        .collect::<Vec<_>>();
    Some(ResolvedPublicExports {
        text: if fields.is_empty() {
            "{}".to_string()
        } else {
            format!("{{ {} }}", fields.join("; "))
        },
        type_references,
    })
}

/// Use the AST-captured type spelling only when every local reference it names
/// is independently available in the generated module. A local interface/type
/// alias is not copied into the declaration carrier, so that case deliberately
/// falls through to the shared resolved object surface. Imported and ambient
/// references remain valid and keep their exact authored spelling.
fn captured_display_without_local_refs(
    display: Option<&str>,
    references: &[String],
    shallow: &crate::resolver_core::shallow_file_state::ShallowFileState,
) -> Option<String> {
    let display = display?.trim();
    if display.is_empty()
        || references
            .iter()
            .any(|name| shallow.type_symbol_kind(name).is_some())
    {
        return None;
    }
    Some(display.to_string())
}

/// An inline object spelling is a valid `Component<Props>` generic argument.
fn object_type_text(text: &str) -> Option<&str> {
    let text = text.trim();
    (text.starts_with('{') && text.ends_with('}')).then_some(text)
}

/// Render a property key as an identifier when safe, otherwise as a quoted
/// TypeScript string-literal key.
fn render_property_name(name: &str) -> String {
    let mut chars = name.chars();
    let identifier = chars
        .next()
        .is_some_and(|ch| ch == '_' || ch == '$' || ch.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch == '$' || ch.is_ascii_alphanumeric());
    if identifier {
        name.to_string()
    } else {
        format!("{name:?}")
    }
}

/// Render the props argument for Svelte's public `Component` contract.
///
/// Locator-backed props have no shallow display authority. The instance
/// surface retains the honest `unknown`, while this generic leg uses the
/// narrowest safe object bound accepted by Svelte 5's `Component` definition.
fn render_component_props(props: Option<&FactOrLocator>, rendered: &str) -> String {
    match props {
        Some(FactOrLocator::Locator(_) | FactOrLocator::MacroPayload(_)) => {
            "Record<string, unknown>".to_string()
        }
        _ => rendered.to_string(),
    }
}

/// Render the component's public instance exports as an object surface.
fn render_member_object(members: &[(&str, &FactOrLocator)]) -> String {
    if members.is_empty() {
        return "{}".to_string();
    }
    let fields = members
        .iter()
        // Synthesized exports are shallow Ref carriers to their value binding,
        // not type-name references. Until the public value-type projection
        // resolves that binding, `unknown` is the only sound declaration type.
        .map(|(name, _ty)| format!("{name}: unknown"))
        .collect::<Vec<_>>();
    format!("{{ {} }}", fields.join("; "))
}

/// Compose legacy dispatcher events into the Svelte 5 callback-prop surface.
///
/// Native `Component` has no event generic. Svelte 5's
/// public event model is callback props, so an authored legacy dispatcher map
/// is represented as optional `on${name}` props whose handler receives the
/// corresponding `CustomEvent<payload>`. Existing runes callback props and
/// snippet props remain untouched in `props`.
fn render_native_component_props(props: &str, dispatcher: Option<&str>) -> String {
    match dispatcher {
        Some(events) => format!(
            "({props}) & {{ [K in keyof ({events}) as K extends string ? `on${{K}}` : never]?: (event: CustomEvent<({events})[K]>) => void }}"
        ),
        None => props.to_string(),
    }
}

/// Render Svelte 5's third `Component` generic from the captured binding facts.
///
/// Runes components admit only explicitly `$bindable()` props. Legacy
/// `export let` props are all bindable. An empty set is the native sentinel
/// `""`, never the permissive default `string`.
fn render_native_bindings(
    facts: Option<&verter_semantic::analysis::framework_facts::svelte::SvelteScriptFacts>,
) -> String {
    let mut names = BTreeSet::new();
    if let Some(facts) = facts {
        names.extend(facts.bindable_members.iter().map(String::as_str));
        if names.is_empty() {
            names.extend(facts.legacy_props.iter().map(|prop| prop.name.as_str()));
        }
    }
    if names.is_empty() {
        return "\"\"".to_string();
    }
    names
        .into_iter()
        .map(|name| format!("{name:?}"))
        .collect::<Vec<_>>()
        .join(" | ")
}

/// A prop-name anchor rebased to finished-code byte offsets, admitted into
/// the source map only after BOTH endpoints byte-verify against the prop name.
#[derive(Debug, Clone)]
struct RebasedNameMapping {
    /// Byte offset of the rendered name token in the finished code.
    generated_offset: usize,
    /// Authored SFC-absolute byte offset of the prop-name token.
    source_start: u32,
    /// The prop name both the generated and authored tokens must spell.
    name: String,
}

/// Build the carrier's V3 source-map JSON — the same oxc_sourcemap JSON the
/// Vue tsc projector writes (`verter_compiler::tsc::script`) and the carrier
/// store/plugin consume uniformly: one source (the authored component, its
/// exact bytes embedded), one token per admitted prop name.
///
/// An anchor is admitted ONLY when BOTH endpoints byte-match the prop name —
/// the generated token at the recorded offset in `code` AND the authored
/// token at the recorded SFC-absolute offset in `source`. Any drift (a
/// shifted render, a stale span, a non-identifier key rendered quoted) drops
/// the anchor rather than publishing a mis-map. No admitted anchors ⇒ `None`
/// (the projection then behaves exactly as before this map existed).
fn build_api_source_map(
    code: &str,
    source: &str,
    canonical: &str,
    mappings: &[RebasedNameMapping],
) -> Option<Arc<str>> {
    let admitted: Vec<verter_compiler::tsc::script::GeneratedMapping> = mappings
        .iter()
        .filter(|mapping| {
            code.get(mapping.generated_offset..mapping.generated_offset + mapping.name.len())
                == Some(mapping.name.as_str())
                && source.get(
                    mapping.source_start as usize
                        ..mapping.source_start as usize + mapping.name.len(),
                ) == Some(mapping.name.as_str())
        })
        .map(|mapping| verter_compiler::tsc::script::GeneratedMapping {
            generated_offset: mapping.generated_offset,
            source_span: verter_span::Span::new(
                mapping.source_start,
                mapping.source_start + mapping.name.len() as u32,
            ),
        })
        .collect();
    if admitted.is_empty() {
        return None;
    }
    Some(Arc::from(
        verter_compiler::tsc::script::build_tsc_source_map(
            code,
            source,
            Some(canonical),
            &admitted,
        ),
    ))
}

/// A minimal line-oriented shim builder. The api-projector renders into it with
/// CodeTransform-style inserts (append-only line writes) — there is no
/// post-hoc string rewrite of already-built content. Lines carrying a mapped
/// fragment record their prop-name anchors as the line is appended, so the
/// generated offsets are construction-exact (never a re-scan of the built
/// text).
#[derive(Default)]
struct ShimBuilder {
    lines: Vec<String>,
    /// Running byte offset of the NEXT line's start in the finished code
    /// (each pushed line contributes its length plus the joining `\n`).
    offset: usize,
    /// Prop-name anchors, rebased to finished-code byte offsets.
    mappings: Vec<RebasedNameMapping>,
}

impl ShimBuilder {
    fn line(&mut self, text: &str) {
        self.offset += text.len() + 1;
        self.lines.push(text.to_string());
    }
    fn blank(&mut self) {
        self.line("");
    }
    /// Push one line carrying a mapped props fragment at EACH of
    /// `fragment_bases_in_line` (the generics call-signature renders the props
    /// object twice — `Parameters<…>` and `ReturnType<…>` — and both
    /// occurrences map to the same authored member), rebasing the fragment's
    /// prop-name anchors to finished-code byte offsets.
    fn line_mapped(
        &mut self,
        text: &str,
        fragment_bases_in_line: &[usize],
        mappings: &[PropNameMapping],
    ) {
        let line_base = self.offset;
        for &fragment_base in fragment_bases_in_line {
            self.mappings
                .extend(mappings.iter().map(|mapping| RebasedNameMapping {
                    generated_offset: line_base + fragment_base + mapping.offset_in_fragment,
                    source_start: mapping.source_start,
                    name: mapping.name.clone(),
                }));
        }
        self.line(text);
    }
    fn finish(self) -> (String, Vec<RebasedNameMapping>) {
        let mut s = self.lines.join("\n");
        s.push('\n');
        (s, self.mappings)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_component_props_preserve_dispatcher_payloads_as_callbacks() {
        let props = "{ onselect: (id: number) => void; label: string }";
        let rendered = render_native_component_props(props, Some("{ save: string }"));
        assert!(rendered.contains("on${K}"));
        assert!(rendered.contains("CustomEvent<({ save: string })[K]>"));
        assert!(rendered.contains("onselect: (id: number) => void"));
        assert!(!rendered.contains("any"));
        assert!(!rendered.contains("__Verter"));
    }
}
