#![deny(missing_docs)]
//! The Vue plan/normalize adapter.
//!
//! [`VueFrameworkAdapter`] is the registry's Vue
//! [`FrameworkSurfaceAdapter`](crate::typeinfo::framework_surface::FrameworkSurfaceAdapter):
//! it PLANS the Vue component's typed surface demands and NORMALIZES the
//! executor-resolved surfaces into per-kind DTO bundles. Planning is selector +
//! requested-kind data work; normalization is a pure resolved-data → DTO
//! transform. The executor owns resolution — the adapter never resolves types,
//! indexes a file, or calls `ProjectSemanticDispatch`.

use std::sync::Arc;

use verter_protocol::typeinfo::graph::FrameworkSurfaceKind;
use verter_semantic::analysis::types::AnalyzedMacroKind;

use crate::framework::ctx::FrameworkAdapterCtx;
use crate::framework::descriptor::{vue_descriptor, FrameworkAdapterDescriptor};
use crate::typeinfo::framework_surface::{
    FrameworkSurfaceAdapter, FrameworkSurfacePlan, MacroPayloadSelector, MacroSurfaceDtos,
    NormalizedSurface, NormalizedSurfaces, PlannedDemand, PlannedResolve,
    ResolvedComponentSelector, ResolvedDemand, ResolvedOutcome, ResolvedSurfaces,
};

/// The Vue plan/normalize adapter.
///
/// Holds the Vue descriptor row so [`Self::descriptor`] hands back a stable
/// reference (the descriptor is the registry row's immutable identity half).
#[derive(Debug)]
pub struct VueFrameworkAdapter {
    descriptor: FrameworkAdapterDescriptor,
}

impl Default for VueFrameworkAdapter {
    fn default() -> Self {
        Self {
            descriptor: vue_descriptor(),
        }
    }
}

impl VueFrameworkAdapter {
    /// The [`AnalyzedMacroKind`] each macro surface kind targets.
    ///
    /// Every Vue surface kind is a macro-payload surface, so the mapping is
    /// total (the wire enum has no non-surface zero variant).
    fn macro_kind_for(kind: FrameworkSurfaceKind) -> AnalyzedMacroKind {
        match kind {
            FrameworkSurfaceKind::Props => AnalyzedMacroKind::DefineProps,
            FrameworkSurfaceKind::Emits => AnalyzedMacroKind::DefineEmits,
            FrameworkSurfaceKind::Slots => AnalyzedMacroKind::DefineSlots,
            FrameworkSurfaceKind::Options => AnalyzedMacroKind::DefineOptions,
            FrameworkSurfaceKind::Expose => AnalyzedMacroKind::DefineExpose,
            FrameworkSurfaceKind::Model => AnalyzedMacroKind::DefineModel,
        }
    }

    /// Project one resolved macro payload onto a single-kind
    /// [`MacroSurfaceDtos`] bundle carrying only the requested surface — the
    /// pure normalization transform.
    fn project_kind(kind: FrameworkSurfaceKind, dtos: &MacroSurfaceDtos) -> MacroSurfaceDtos {
        let mut out = MacroSurfaceDtos::default();
        match kind {
            FrameworkSurfaceKind::Props => out.props = dtos.props.clone(),
            FrameworkSurfaceKind::Emits => out.emits = dtos.emits.clone(),
            FrameworkSurfaceKind::Slots => out.slots = dtos.slots.clone(),
            FrameworkSurfaceKind::Options => out.options = dtos.options.clone(),
            FrameworkSurfaceKind::Expose => out.expose = dtos.expose.clone(),
            FrameworkSurfaceKind::Model => out.model = dtos.model.clone(),
        }
        out
    }
}

impl FrameworkSurfaceAdapter for VueFrameworkAdapter {
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
        let mut items = Vec::with_capacity(requested.len() + 1);
        // The public instance type the synthesized `default` projects through.
        items.push(PlannedResolve {
            kind: FrameworkSurfaceKind::Props,
            demand: PlannedDemand::PublicTypeInstance {
                canonical: Arc::clone(&owner),
            },
        });
        // One macro-payload demand per requested macro surface kind. Planning is
        // KIND-PRIMARY: it has no snapshot access, so it never fabricates a
        // macro index — `macro_index: None` tells the executor to select the
        // macro of this kind from the owner's authoritative shallow snapshot
        // (an SFC whose `defineEmits` is not macro 0 still resolves correctly).
        for &kind in requested {
            items.push(PlannedResolve {
                kind,
                demand: PlannedDemand::MacroPayload {
                    owner: Arc::clone(&owner),
                    selector: MacroPayloadSelector {
                        macro_index: None,
                        macro_kind: Self::macro_kind_for(kind),
                    },
                },
            });
        }
        FrameworkSurfacePlan { items }
    }

    fn normalize(
        &self,
        _ctx: &FrameworkAdapterCtx<'_>,
        resolved: ResolvedSurfaces,
    ) -> NormalizedSurfaces {
        let mut surfaces = Vec::new();
        for item in resolved.items {
            // Only macro-payload resolutions carry a per-kind DTO bundle; the
            // public-instance resolution feeds the synthesized-default surface,
            // not a wire surface kind, so it is not normalized here.
            let ResolvedDemand::MacroPayload(payload) = item.result else {
                continue;
            };
            let outcome = match payload {
                ResolvedOutcome::Resolved(dtos) => {
                    ResolvedOutcome::Resolved(Self::project_kind(item.kind, &dtos))
                }
                ResolvedOutcome::Partial { value, diagnostics } => ResolvedOutcome::Partial {
                    value: Self::project_kind(item.kind, &value),
                    diagnostics,
                },
                ResolvedOutcome::Unsupported { diagnostics } => {
                    ResolvedOutcome::Unsupported { diagnostics }
                }
                ResolvedOutcome::Missing => ResolvedOutcome::Missing,
            };
            surfaces.push(NormalizedSurface {
                kind: item.kind,
                outcome,
            });
        }
        NormalizedSurfaces { surfaces }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_is_the_vue_row() {
        let adapter = VueFrameworkAdapter::default();
        assert!(adapter.descriptor().id.is_vue());
    }

    #[test]
    fn macro_kind_maps_every_surface_kind_distinctly() {
        // Every surface kind maps to a DISTINCT macro kind (no two kinds alias).
        let mut seen = std::collections::HashSet::new();
        for &kind in crate::framework::descriptor::ALL_FRAMEWORK_SURFACE_KINDS {
            let mk = VueFrameworkAdapter::macro_kind_for(kind);
            assert!(
                seen.insert(mk),
                "{kind:?} aliases a macro kind already seen"
            );
        }
        assert_eq!(seen.len(), 6, "six surface kinds map to six macro kinds");
    }

    #[test]
    fn plan_surfaces_never_fabricates_a_macro_index() {
        // Planning has no snapshot access, so it must NOT hard-code a macro
        // index — every macro-payload demand carries `macro_index: None` (the
        // executor selects the macro of the kind from the authoritative
        // snapshot). An SFC whose `defineEmits` is not macro 0 would otherwise
        // plan the wrong macro. Built without a host: planning is pure data
        // work, so it does not touch the ctx.
        let adapter = VueFrameworkAdapter::default();
        let selector = ResolvedComponentSelector {
            canonical: Arc::from("/App.vue"),
            export: crate::typeinfo::framework_surface::ComponentExport::Default,
        };
        // Build a minimal ctx purely to satisfy the signature; planning ignores
        // it.
        let host = crate::VerterHost::new_standalone(crate::HostConfig::default());
        let registry = crate::framework::FrameworkAdapterRegistry::built_in(
            crate::typeinfo::adapters::vue::vue_carrier_token_clone(),
            crate::typeinfo::adapters::svelte::svelte_carrier_token_clone(),
        );
        let registration = registry
            .get(&crate::framework::FrameworkAdapterId::vue())
            .expect("vue registered");
        let ctx = FrameworkAdapterCtx::new(registration, &host);
        let plan = adapter.plan_surfaces(
            &ctx,
            &selector,
            crate::framework::descriptor::ALL_FRAMEWORK_SURFACE_KINDS,
        );
        for item in &plan.items {
            if let PlannedDemand::MacroPayload { selector, .. } = &item.demand {
                assert_eq!(
                    selector.macro_index, None,
                    "planning must not fabricate a macro index for {:?}",
                    selector.macro_kind
                );
            }
        }
        // Every macro surface kind is planned (six macro-payload demands).
        let macro_payloads = plan
            .items
            .iter()
            .filter(|i| matches!(i.demand, PlannedDemand::MacroPayload { .. }))
            .count();
        assert_eq!(macro_payloads, 6, "all six macro surfaces are planned");
    }

    #[test]
    fn project_kind_isolates_the_requested_surface() {
        // A full DTO bundle projected at Props yields ONLY the props slot.
        let dtos = MacroSurfaceDtos {
            props: Some(crate::typeinfo::framework_surface::PropsSurface::default()),
            emits: Some(crate::typeinfo::framework_surface::EmitsSurface::default()),
            ..Default::default()
        };
        let props_only = VueFrameworkAdapter::project_kind(FrameworkSurfaceKind::Props, &dtos);
        assert!(props_only.props.is_some());
        assert!(props_only.emits.is_none(), "props projection drops emits");
        let emits_only = VueFrameworkAdapter::project_kind(FrameworkSurfaceKind::Emits, &dtos);
        assert!(emits_only.emits.is_some());
        assert!(emits_only.props.is_none(), "emits projection drops props");
    }
}
