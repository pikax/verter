#![deny(missing_docs)]
//! The Svelte plan/normalize adapter.
//!
//! [`SvelteFrameworkAdapter`] is the registry's Svelte
//! [`FrameworkSurfaceAdapter`](crate::typeinfo::framework_surface::FrameworkSurfaceAdapter):
//! it PLANS one [`PlannedDemand::SvelteSurface`] per requested wire kind's source
//! family (SLOTS plans TWO — snippet props + the legacy `<slot>` inventory) and
//! NORMALIZES the executor-resolved single-source bundles into per-kind DTO
//! bundles, merging the two SLOTS sources at normalise time (first-writer-wins on
//! a duplicate slot name). Planning is selector + requested-kind data work;
//! normalization is a pure resolved-data → DTO transform. The executor owns
//! resolution — the adapter never resolves types, indexes a file, or calls
//! `ProjectSemanticDispatch`.
//!
//! Surface mapping (§9): PROPS = `$props()` members (incl. snippet-typed) or
//! legacy `export let`; MODEL = `$bindable()`; SLOTS = snippet-typed props +
//! legacy `<slot>`; EMITS = legacy `createEventDispatcher<E>` (provenance-
//! validated, runes callbacks STAY props); EXPOSE = exported instance members;
//! OPTIONS is unsupported (the descriptor omits it from `supported_surfaces`, so
//! the executor fills OPTIONS structurally UNSUPPORTED — the adapter never plans
//! an OPTIONS demand).

use std::sync::Arc;

use verter_protocol::typeinfo::graph::FrameworkSurfaceKind;

use crate::framework::ctx::FrameworkAdapterCtx;
use crate::framework::descriptor::{svelte_descriptor, FrameworkAdapterDescriptor};
use crate::typeinfo::framework_surface::results::MacroSurfaceDtos;
use crate::typeinfo::framework_surface::{
    FrameworkSurfaceAdapter, FrameworkSurfacePlan, NormalizedSurface, NormalizedSurfaces,
    PlannedDemand, PlannedResolve, ResolvedComponentSelector, ResolvedDemand, ResolvedOutcome,
    ResolvedSurfaces, SvelteSurfaceSource,
};

/// The Svelte plan/normalize adapter.
#[derive(Debug)]
pub struct SvelteFrameworkAdapter {
    descriptor: FrameworkAdapterDescriptor,
}

impl Default for SvelteFrameworkAdapter {
    fn default() -> Self {
        Self {
            descriptor: svelte_descriptor(),
        }
    }
}

impl SvelteFrameworkAdapter {
    /// The Svelte source families that contribute to a requested wire kind.
    ///
    /// PROPS draws from BOTH `RunesProps` and `LegacyExportLet` (a component is
    /// one or the other, never both, so the two never collide). SLOTS draws from
    /// TWO families (`SnippetProps` + `LegacySlotInventory`) merged at normalise
    /// time. OPTIONS has NO source family — it is unsupported and the executor
    /// fills it structurally UNSUPPORTED (the descriptor omits it from
    /// `supported_surfaces`).
    fn sources_for(kind: FrameworkSurfaceKind) -> &'static [SvelteSurfaceSource] {
        match kind {
            FrameworkSurfaceKind::Props => &[
                SvelteSurfaceSource::RunesProps,
                SvelteSurfaceSource::LegacyExportLet,
            ],
            FrameworkSurfaceKind::Emits => &[
                SvelteSurfaceSource::LegacyDispatcher,
                SvelteSurfaceSource::CallbackPropEvents,
            ],
            FrameworkSurfaceKind::Slots => &[
                SvelteSurfaceSource::SnippetProps,
                SvelteSurfaceSource::LegacySlotInventory,
            ],
            FrameworkSurfaceKind::Model => &[SvelteSurfaceSource::Bindable],
            FrameworkSurfaceKind::Expose => &[SvelteSurfaceSource::InstanceExports],
            // OPTIONS is unsupported for Svelte — no source family, no demand.
            FrameworkSurfaceKind::Options => &[],
        }
    }
}

impl FrameworkSurfaceAdapter for SvelteFrameworkAdapter {
    fn descriptor(&self) -> &FrameworkAdapterDescriptor {
        &self.descriptor
    }

    fn plan_surfaces(
        &self,
        _ctx: &FrameworkAdapterCtx<'_>,
        selector: &ResolvedComponentSelector,
        requested: &[FrameworkSurfaceKind],
    ) -> FrameworkSurfacePlan {
        let owner = Arc::clone(&selector.canonical);
        let mut items = Vec::new();
        for &kind in requested {
            for &source in Self::sources_for(kind) {
                items.push(PlannedResolve {
                    kind,
                    demand: PlannedDemand::SvelteSurface {
                        owner: Arc::clone(&owner),
                        source,
                    },
                });
            }
        }
        FrameworkSurfacePlan { items }
    }

    fn normalize(
        &self,
        _ctx: &FrameworkAdapterCtx<'_>,
        resolved: ResolvedSurfaces,
    ) -> NormalizedSurfaces {
        // Fold every resolved single-source bundle into one per-kind aggregate.
        // Multiple sources for one kind (PROPS over runes|legacy, SLOTS over
        // snippet|legacy-slot) merge here. A kind is PRESENT once ANY of its
        // sources resolved (supported-empty when present-but-empty); MISSING for
        // every source ⇒ the kind stays absent (the executor fills it
        // supported-empty for a supported kind).
        let mut by_kind: Vec<(FrameworkSurfaceKind, MacroSurfaceDtos, bool)> = Vec::new();
        for item in resolved.items {
            let ResolvedDemand::SvelteSurface(payload) = item.result else {
                // The Svelte adapter only plans SvelteSurface demands; any other
                // resolved demand is not its concern.
                continue;
            };
            let entry = match by_kind.iter_mut().find(|(k, _, _)| *k == item.kind) {
                Some(e) => e,
                None => {
                    by_kind.push((item.kind, MacroSurfaceDtos::default(), false));
                    by_kind.last_mut().unwrap()
                }
            };
            if let ResolvedOutcome::Resolved(dtos) | ResolvedOutcome::Partial { value: dtos, .. } =
                &payload
            {
                entry.2 = true;
                merge_source_into(&mut entry.1, item.kind, dtos);
            }
        }

        let surfaces = by_kind
            .into_iter()
            .map(|(kind, dtos, present)| NormalizedSurface {
                kind,
                outcome: if present {
                    ResolvedOutcome::Resolved(dtos)
                } else {
                    ResolvedOutcome::Missing
                },
            })
            .collect();
        NormalizedSurfaces { surfaces }
    }
}

/// Merge one resolved single-source bundle's relevant slot into the per-kind
/// aggregate. SLOTS merges the two source rows first-writer-wins on a duplicate
/// slot name (a snippet prop and a legacy `<slot>` sharing a name keep the
/// first).
fn merge_source_into(
    aggregate: &mut MacroSurfaceDtos,
    kind: FrameworkSurfaceKind,
    source: &MacroSurfaceDtos,
) {
    match kind {
        FrameworkSurfaceKind::Props => {
            if let Some(props) = &source.props {
                let target = aggregate.props.get_or_insert_with(Default::default);
                target.fields.extend(props.fields.iter().cloned());
                target
                    .index_signatures
                    .extend(props.index_signatures.iter().cloned());
            } else {
                aggregate.props.get_or_insert_with(Default::default);
            }
        }
        FrameworkSurfaceKind::Emits => {
            if let Some(emits) = &source.emits {
                let target = aggregate.emits.get_or_insert_with(Default::default);
                target.fields.extend(emits.fields.iter().cloned());
                target
                    .index_signatures
                    .extend(emits.index_signatures.iter().cloned());
            } else {
                aggregate.emits.get_or_insert_with(Default::default);
            }
        }
        FrameworkSurfaceKind::Slots => {
            let target = aggregate.slots.get_or_insert_with(Vec::new);
            if let Some(slots) = &source.slots {
                for slot in slots {
                    // First-writer-wins on a duplicate slot name (a snippet prop
                    // and a legacy `<slot>` of the same name collapse to one).
                    if !target.iter().any(|s| s.name == slot.name) {
                        target.push(slot.clone());
                    }
                }
            }
        }
        FrameworkSurfaceKind::Model => {
            if let Some(model) = &source.model {
                aggregate
                    .model
                    .get_or_insert_with(Default::default)
                    .bindings
                    .extend(model.bindings.iter().cloned());
            } else {
                aggregate.model.get_or_insert_with(Default::default);
            }
        }
        FrameworkSurfaceKind::Expose => {
            if let Some(expose) = &source.expose {
                aggregate
                    .expose
                    .get_or_insert_with(Default::default)
                    .members
                    .extend(expose.members.iter().cloned());
            } else {
                aggregate.expose.get_or_insert_with(Default::default);
            }
        }
        FrameworkSurfaceKind::Options => {
            // Svelte has no OPTIONS source; never reached (no OPTIONS demand is
            // planned). Defensive no-op rather than a panic.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::descriptor::ALL_FRAMEWORK_SURFACE_KINDS;

    #[test]
    fn descriptor_is_the_svelte_row() {
        let adapter = SvelteFrameworkAdapter::default();
        assert!(adapter.descriptor().id.is_svelte());
    }

    #[test]
    fn options_has_no_source_family_every_other_kind_does() {
        // OPTIONS is unsupported — no Svelte source contributes to it.
        assert!(SvelteFrameworkAdapter::sources_for(FrameworkSurfaceKind::Options).is_empty());
        for kind in ALL_FRAMEWORK_SURFACE_KINDS {
            if *kind == FrameworkSurfaceKind::Options {
                continue;
            }
            assert!(
                !SvelteFrameworkAdapter::sources_for(*kind).is_empty(),
                "{kind:?} must have at least one Svelte source family"
            );
        }
    }

    #[test]
    fn slots_draws_from_two_source_families() {
        // SLOTS = snippet props + legacy <slot> inventory (two source rows merged
        // at normalise time).
        let sources = SvelteFrameworkAdapter::sources_for(FrameworkSurfaceKind::Slots);
        assert_eq!(sources.len(), 2);
        assert!(sources.contains(&SvelteSurfaceSource::SnippetProps));
        assert!(sources.contains(&SvelteSurfaceSource::LegacySlotInventory));
    }

    #[test]
    fn emits_draws_from_dispatcher_and_callback_props() {
        // EMITS draws from BOTH the authoritative legacy dispatcher AND the
        // DERIVED modern callback-prop event index (F13). The dispatcher source
        // is FIRST (authoritative); the callback-prop source is the derived
        // compatibility surface. A runes callback prop ALSO stays a PROP (the
        // callback-prop source does not move it out of PROPS — it is a derived
        // index, not the authority).
        let sources = SvelteFrameworkAdapter::sources_for(FrameworkSurfaceKind::Emits);
        assert_eq!(
            sources,
            &[
                SvelteSurfaceSource::LegacyDispatcher,
                SvelteSurfaceSource::CallbackPropEvents,
            ]
        );
    }

    #[test]
    fn plan_emits_no_options_demand() {
        // Planning never emits an OPTIONS demand (OPTIONS is unsupported).
        let adapter = SvelteFrameworkAdapter::default();
        let selector = ResolvedComponentSelector {
            canonical: Arc::from("/App.svelte"),
            export: crate::typeinfo::framework_surface::ComponentExport::Default,
        };
        let host = crate::VerterHost::new_standalone(crate::HostConfig::default());
        let registry = crate::framework::FrameworkAdapterRegistry::built_in(
            crate::typeinfo::adapters::vue::vue_carrier_token_clone(),
            crate::typeinfo::adapters::svelte::svelte_carrier_token_clone(),
        );
        let registration = registry
            .get(&crate::framework::FrameworkAdapterId::svelte())
            .expect("svelte registered");
        let ctx = FrameworkAdapterCtx::new(registration, &host);
        let plan = adapter.plan_surfaces(&ctx, &selector, ALL_FRAMEWORK_SURFACE_KINDS);
        // No planned demand targets OPTIONS.
        assert!(
            plan.items
                .iter()
                .all(|i| i.kind != FrameworkSurfaceKind::Options),
            "Svelte planning must not emit an OPTIONS demand"
        );
        // SLOTS plans exactly two demands (the two source families).
        let slot_demands = plan
            .items
            .iter()
            .filter(|i| i.kind == FrameworkSurfaceKind::Slots)
            .count();
        assert_eq!(slot_demands, 2, "SLOTS plans two source-family demands");
    }

    /// One slot field with the given name (an otherwise-empty fixture).
    fn slot_field(name: &str) -> verter_semantic::analysis::types::AnalyzedSlotField {
        verter_semantic::analysis::types::AnalyzedSlotField {
            name: name.to_string(),
            is_required: false,
            span: verter_span::Span::default(),
            bindings: Vec::new(),
            return_type: None,
            return_expr: None,
            return_expr_scope: None,
            description: None,
            tags: Vec::new(),
        }
    }

    #[test]
    fn slots_merge_is_first_writer_wins_on_duplicate_name() {
        // The two SLOTS source rows (snippet props + legacy <slot>) merge at
        // normalise time; a slot name present in BOTH collapses to ONE entry —
        // the FIRST writer wins (no duplicate, no overwrite). DISCRIMINATING:
        // a naive append would yield two `header` entries.
        let mut aggregate = MacroSurfaceDtos::default();
        // First source (snippet props): `header` + `body`.
        let first = MacroSurfaceDtos {
            slots: Some(vec![slot_field("header"), slot_field("body")]),
            ..Default::default()
        };
        merge_source_into(&mut aggregate, FrameworkSurfaceKind::Slots, &first);
        // Second source (legacy <slot>): a duplicate `header` + a new `footer`.
        let second = MacroSurfaceDtos {
            slots: Some(vec![slot_field("header"), slot_field("footer")]),
            ..Default::default()
        };
        merge_source_into(&mut aggregate, FrameworkSurfaceKind::Slots, &second);

        let names: Vec<&str> = aggregate
            .slots
            .as_ref()
            .unwrap()
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(
            names,
            vec!["header", "body", "footer"],
            "duplicate `header` collapses to one (first-writer-wins), order preserved"
        );
    }

    #[test]
    fn normalize_props_merges_runes_and_legacy_sources() {
        // PROPS draws from two source families; both fold into one aggregate, and
        // a kind with ANY resolved source is PRESENT (Resolved), not Missing.
        let adapter = SvelteFrameworkAdapter::default();
        let host = crate::VerterHost::new_standalone(crate::HostConfig::default());
        let registry = crate::framework::FrameworkAdapterRegistry::built_in(
            crate::typeinfo::adapters::vue::vue_carrier_token_clone(),
            crate::typeinfo::adapters::svelte::svelte_carrier_token_clone(),
        );
        let registration = registry
            .get(&crate::framework::FrameworkAdapterId::svelte())
            .expect("svelte registered");
        let ctx = FrameworkAdapterCtx::new(registration, &host);

        let prop = |name: &str| verter_semantic::analysis::types::AnalyzedPropField {
            name: name.to_string(),
            is_optional: false,
            span: verter_span::Span::default(),
            type_annotation: None,
            type_expr: None,
            type_expr_scope: None,
            description: None,
            tags: Vec::new(),
            resolution_source: verter_semantic::analysis::types::TypeResolutionSource::Rust,
            resolution_error: None,
            declared_in_macro_type_arg: false,
        };
        let runes = ResolvedOutcome::Resolved(Arc::new(MacroSurfaceDtos {
            props: Some(crate::typeinfo::framework_surface::results::PropsSurface {
                fields: vec![prop("a")],
                index_signatures: Vec::new(),
            }),
            ..Default::default()
        }));
        // The legacy source is Missing (the component is runes-mode) — it must
        // NOT flip the kind to Missing once the runes source resolved.
        let legacy = ResolvedOutcome::Missing;
        let resolved = ResolvedSurfaces {
            items: vec![
                crate::typeinfo::framework_surface::ResolvedItem {
                    kind: FrameworkSurfaceKind::Props,
                    result: ResolvedDemand::SvelteSurface(runes),
                },
                crate::typeinfo::framework_surface::ResolvedItem {
                    kind: FrameworkSurfaceKind::Props,
                    result: ResolvedDemand::SvelteSurface(legacy),
                },
            ],
        };
        let normalized = adapter.normalize(&ctx, resolved);
        let props = normalized
            .surfaces
            .iter()
            .find(|s| s.kind == FrameworkSurfaceKind::Props)
            .expect("PROPS normalized");
        let dtos = match &props.outcome {
            ResolvedOutcome::Resolved(d) => d,
            other => panic!("PROPS must be Resolved once a source resolved, got {other:?}"),
        };
        let names: Vec<&str> = dtos.prop_fields().iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["a"], "the runes source's prop folds in");
    }
}
