#![deny(missing_docs)]
//! The framework-neutral macro-surface result vocabulary.
//!
//! [`MacroSurfaceDtos`] is the framework-neutral macro-payload result the
//! executor resolves and the adapter normalizes. It covers EVERY framework
//! surface the wire taxonomy defines (props / emits / slots / options / expose /
//! model) — including the prop/emit index signatures the Vue macro surface
//! carries and the options/expose/model object surfaces that today flow through
//! separate projection paths. A narrower prop/emit/slot-only vocabulary would
//! silently emit supported-empty OPTIONS/EXPOSE entries for components with real
//! `defineOptions<T>()` / `defineExpose<T>()` — a status-fidelity violation.
//!
//! [`ResolvedOutcome<T>`] carries the per-surface status basis DIRECTLY:
//! `Resolved` / `Partial` / `Unsupported` / `Missing` map onto the wire
//! `FrameworkSurfaceKindSupport` (SUPPORTED / PARTIAL / UNSUPPORTED) without
//! inferring status from a bare `Option` — a supported-but-empty surface is
//! `Resolved(empty)`, distinct from `Missing` (the selector has no such surface)
//! and from `Unsupported` (the adapter does not produce the kind).

use std::sync::Arc;

use verter_semantic::analysis::type_expand::ExpandedIndexSignature;
use verter_semantic::analysis::types::{AnalyzedEmitField, AnalyzedPropField, AnalyzedSlotField};
use verter_type_expr::TypeExpr;

/// The resolved `defineProps` surface: named prop fields plus index signatures.
///
/// A props member is `properties + index signatures` per the props-surface
/// rule, so the resolved props surface carries both the named
/// [`AnalyzedPropField`] vector and the surface's [`ExpandedIndexSignature`]
/// rows (`defineProps<{ [k: string]: string }>()`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PropsSurface {
    /// Named prop fields.
    pub fields: Vec<AnalyzedPropField>,
    /// Index signatures on the props type-argument surface.
    pub index_signatures: Vec<ExpandedIndexSignature>,
}

/// The resolved `defineEmits` surface: named emit fields plus index signatures.
///
/// The emits object is `properties (events) + index signatures`; an
/// index-signature-only emits surface has no named events but still carries its
/// index signature (`defineEmits<{ [event: string]: [v: number] }>()`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EmitsSurface {
    /// Named emit fields.
    pub fields: Vec<AnalyzedEmitField>,
    /// Index signatures on the emits type-argument surface.
    pub index_signatures: Vec<ExpandedIndexSignature>,
}

/// The resolved `defineOptions<T>()` object surface.
///
/// The pass-through object surface the options projection produces — the named
/// members of the options type argument, each carrying its resolved
/// [`TypeExpr`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OptionsSurface {
    /// Named members of the options object surface.
    pub members: Vec<NamedTypeMember>,
}

/// The resolved `defineExpose<T>()` object surface.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExposeSurface {
    /// Named members of the expose object surface.
    pub members: Vec<NamedTypeMember>,
}

/// The resolved `defineModel<T>()` binding(s).
///
/// Each `defineModel` macro contributes one model binding (the model name plus
/// the synthesized prop field for the binding's value type).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModelSurface {
    /// One entry per `defineModel` binding.
    pub bindings: Vec<ModelBinding>,
}

/// One `defineModel` binding.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelBinding {
    /// The model name (`defineModel("title")` → `"title"`; the default model is
    /// `"modelValue"`).
    pub name: String,
    /// The synthesized prop field carrying the binding's value type.
    pub prop: AnalyzedPropField,
}

/// A named member of a resolved object surface (options / expose).
#[derive(Debug, Clone, PartialEq)]
pub struct NamedTypeMember {
    /// The member name.
    pub name: String,
    /// Whether the member is optional.
    pub is_optional: bool,
    /// The member's resolved type, when one is available.
    pub type_expr: Option<TypeExpr>,
}

/// The framework-neutral macro-payload result covering all six surfaces.
///
/// A surface is `Some` when the adapter resolved it (possibly empty); `None`
/// when the selector carries no such macro. Every framework adapter produces
/// the same six-surface shapes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MacroSurfaceDtos {
    /// The resolved props surface.
    pub props: Option<PropsSurface>,
    /// The resolved emits surface.
    pub emits: Option<EmitsSurface>,
    /// The resolved slots surface.
    pub slots: Option<Vec<AnalyzedSlotField>>,
    /// The resolved options object surface.
    pub options: Option<OptionsSurface>,
    /// The resolved expose object surface.
    pub expose: Option<ExposeSurface>,
    /// The resolved model binding(s).
    pub model: Option<ModelSurface>,
}

impl MacroSurfaceDtos {
    /// The resolved prop fields, or an empty slice when no props surface was
    /// resolved.
    ///
    /// The flat-vector accessor the meta-shape consumers
    /// (`meta_resolve::projectors::define_shapes`,
    /// `resolver_core::component_meta`) read: the props surface NESTS its named
    /// fields inside [`PropsSurface`], and these accessors hand back the inner
    /// vectors so a consumer reads `prop_fields()` / `prop_index_signatures()`
    /// without unwrapping the surface.
    #[must_use]
    pub fn prop_fields(&self) -> &[AnalyzedPropField] {
        self.props
            .as_ref()
            .map_or(&[], |surface| surface.fields.as_slice())
    }

    /// The resolved props index signatures, or an empty slice when no props
    /// surface was resolved (`defineProps<{ [k: string]: string }>()`).
    #[must_use]
    pub fn prop_index_signatures(&self) -> &[ExpandedIndexSignature] {
        self.props
            .as_ref()
            .map_or(&[], |surface| surface.index_signatures.as_slice())
    }

    /// The resolved emit fields, or an empty slice when no emits surface was
    /// resolved.
    #[must_use]
    pub fn emit_fields(&self) -> &[AnalyzedEmitField] {
        self.emits
            .as_ref()
            .map_or(&[], |surface| surface.fields.as_slice())
    }

    /// The resolved emits index signatures, or an empty slice when no emits
    /// surface was resolved (`defineEmits<{ [event: string]: [v: number] }>()`).
    #[must_use]
    pub fn emit_index_signatures(&self) -> &[ExpandedIndexSignature] {
        self.emits
            .as_ref()
            .map_or(&[], |surface| surface.index_signatures.as_slice())
    }

    /// The resolved slot fields, or an empty slice when no slots surface was
    /// resolved.
    #[must_use]
    pub fn slot_fields(&self) -> &[AnalyzedSlotField] {
        self.slots.as_deref().unwrap_or(&[])
    }
}

impl crate::framework::surface_store::FrameworkSurfaceDtoBundle for MacroSurfaceDtos {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// The per-surface resolution outcome the executor produces.
///
/// Maps DIRECTLY onto the wire `FrameworkSurfaceKindSupport` — `None` can NOT
/// stand in for the supported-empty / miss / partial / unsupported distinction:
/// - [`ResolvedOutcome::Resolved`] → SUPPORTED (a supported-empty surface is
///   `Resolved(empty)`, never `Missing`);
/// - [`ResolvedOutcome::Partial`] → PARTIAL (e.g. a budget-exceeded usable
///   subset);
/// - [`ResolvedOutcome::Unsupported`] → UNSUPPORTED;
/// - [`ResolvedOutcome::Missing`] → the selector has no such surface (e.g. no
///   `defineOptions`) — distinct from supported-empty; a `Missing` outcome for a
///   kind the adapter DOES support resolves to SUPPORTED-empty downstream.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedOutcome<T> {
    /// A fully-resolved surface (SUPPORTED).
    Resolved(T),
    /// A partially-resolved surface plus its diagnostic MESSAGES (PARTIAL).
    ///
    /// Diagnostics carry their message TEXT (not a pre-baked wire
    /// `GraphDiagnostic` with a fabricated `message_name_id`); the wire encoder
    /// (`graph_export`) is the SOLE interner — it puts each message into the
    /// graph string table and stamps the diagnostic's `message_name_id` so it
    /// always indexes a real entry.
    Partial {
        /// The usable resolved subset.
        value: T,
        /// Diagnostic messages describing the partiality.
        diagnostics: Vec<String>,
    },
    /// The surface is unsupported plus its diagnostic MESSAGES (UNSUPPORTED).
    Unsupported {
        /// Diagnostic messages describing why the surface is unsupported.
        diagnostics: Vec<String>,
    },
    /// The selector carries no such surface (distinct from supported-empty).
    Missing,
}

impl<T> ResolvedOutcome<T> {
    /// The resolved value, if the outcome carries one (`Resolved` or `Partial`).
    #[must_use]
    pub fn value(&self) -> Option<&T> {
        match self {
            ResolvedOutcome::Resolved(v) | ResolvedOutcome::Partial { value: v, .. } => Some(v),
            ResolvedOutcome::Unsupported { .. } | ResolvedOutcome::Missing => None,
        }
    }
}

/// The resolved macro-payload bundle as the executor hands it to `normalize`.
pub type ResolvedMacroPayload = ResolvedOutcome<Arc<MacroSurfaceDtos>>;

/// One normalized framework surface: the wire kind plus its per-kind DTO
/// outcome.
///
/// The output of [`FrameworkSurfaceAdapter::normalize`](crate::typeinfo::framework_surface::FrameworkSurfaceAdapter::normalize):
/// the adapter folds the executor-resolved [`ResolvedSurfaces`](crate::typeinfo::framework_surface::ResolvedSurfaces)
/// into one [`MacroSurfaceDtos`] outcome per surface kind. The wire encoder
/// (`graph_export`) consumes these per-kind outcomes as DATA — it never
/// re-resolves. The outcome's status maps DIRECTLY onto the wire support enum;
/// a supported-but-empty surface is `Resolved` with an empty bundle, distinct
/// from `Missing`.
#[derive(Debug, Clone, PartialEq)]
pub struct NormalizedSurface {
    /// The wire surface kind.
    pub kind: verter_protocol::typeinfo::graph::FrameworkSurfaceKind,
    /// The per-kind normalized DTO bundle outcome.
    pub outcome: ResolvedOutcome<MacroSurfaceDtos>,
}

/// The full set of normalized surfaces an adapter's `normalize` produces.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NormalizedSurfaces {
    /// One normalized surface per kind the adapter produced.
    pub surfaces: Vec<NormalizedSurface>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::surface_store::FrameworkSurfaceDtoBundle;

    #[test]
    fn macro_surface_dtos_covers_all_six_surfaces() {
        // Whole-struct destructure pin: every wire surface kind has a slot, so a
        // future surface addition forces this test to acknowledge it.
        let dtos = MacroSurfaceDtos::default();
        let MacroSurfaceDtos {
            props,
            emits,
            slots,
            options,
            expose,
            model,
        } = &dtos;
        assert!(props.is_none());
        assert!(emits.is_none());
        assert!(slots.is_none());
        assert!(options.is_none());
        assert!(expose.is_none());
        assert!(model.is_none());
    }

    #[test]
    fn props_surface_carries_fields_and_index_signatures() {
        // Whole-struct destructure pin: the props surface is `fields + index
        // signatures` — dropping the index-signature slot must fail this.
        let surface = PropsSurface::default();
        let PropsSurface {
            fields,
            index_signatures,
        } = &surface;
        assert!(fields.is_empty());
        assert!(index_signatures.is_empty());
    }

    #[test]
    fn emits_surface_carries_fields_and_index_signatures() {
        // Whole-struct destructure pin: the emits surface is `fields + index
        // signatures` — an index-signature-only emits surface still carries its
        // index signature, so the slot must stay.
        let surface = EmitsSurface::default();
        let EmitsSurface {
            fields,
            index_signatures,
        } = &surface;
        assert!(fields.is_empty());
        assert!(index_signatures.is_empty());
    }

    #[test]
    fn resolved_outcome_distinguishes_empty_from_missing() {
        // Supported-empty is Resolved(empty) — distinct from Missing.
        let empty: ResolvedOutcome<Vec<u32>> = ResolvedOutcome::Resolved(Vec::new());
        let missing: ResolvedOutcome<Vec<u32>> = ResolvedOutcome::Missing;
        assert_ne!(empty, missing);
        assert!(empty.value().is_some());
        assert!(missing.value().is_none());
    }

    #[test]
    fn resolved_outcome_value_reaches_partial_subset() {
        let partial: ResolvedOutcome<u32> = ResolvedOutcome::Partial {
            value: 7,
            diagnostics: Vec::new(),
        };
        assert_eq!(partial.value(), Some(&7));
        let unsupported: ResolvedOutcome<u32> = ResolvedOutcome::Unsupported {
            diagnostics: Vec::new(),
        };
        assert!(unsupported.value().is_none());
    }

    #[test]
    fn macro_surface_dtos_is_a_dto_bundle() {
        let dtos = MacroSurfaceDtos::default();
        // The bundle's Any bridge downcasts back to the concrete type.
        let erased: &dyn FrameworkSurfaceDtoBundle = &dtos;
        assert!(erased.as_any().downcast_ref::<MacroSurfaceDtos>().is_some());
    }
}
