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
use verter_semantic::analysis::types::{
    AnalyzedDefaultValue, AnalyzedEmitField, AnalyzedExposeField, AnalyzedPropField,
    AnalyzedSlotField,
};
use verter_type_expr::TypeExpr;

use crate::resolver_core::ResolvedTypeDeclaration;

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
    /// Prop DEFAULT values — a framework-neutral SIDECAR populated only by an
    /// adapter that captures runtime defaults (Svelte runes `$props()`
    /// destructuring defaults + `$bindable(<default>)` fallbacks). The Vue
    /// analyzer pipeline carries defaults through its own `withDefaults`
    /// merge path and leaves this empty. Source-text + span, keyed by prop
    /// name — NOT a `TypeExpr` (defaults are runtime expressions).
    pub prop_defaults: Vec<AnalyzedDefaultValue>,
    /// Prop ORIGIN entries — a framework-neutral SIDECAR carrying the
    /// resolver-known declaration provenance for each prop whose type resolved
    /// to a declaration. Populated only from routes the shared resolver already
    /// traversed; an inline/local prop carries a [`OriginHop::Local`], an
    /// unresolved prop carries NO entry (never source-text-guessed).
    pub prop_origins: Vec<PropOriginEntry>,
}

/// One prop's resolver-known declaration origin (a framework-neutral SIDECAR
/// entry on [`PropsSurface`]).
#[derive(Debug, Clone, PartialEq)]
pub struct PropOriginEntry {
    /// The prop name this origin describes.
    pub prop_name: String,
    /// The resolved origin.
    pub origin: PropOrigin,
}

/// The declaration origin of a prop type: the final resolved declaration plus
/// the ordered hop chain the shared resolver traversed to reach it.
///
/// Built ENTIRELY from resolver-known routes (`ResolvedTypeDeclaration` +
/// requested-vs-resolved name / canonical comparison) — never a new traversal,
/// never a source-text guess.
#[derive(Debug, Clone, PartialEq)]
pub struct PropOrigin {
    /// The final resolved declaration (canonical source, resolved name, span,
    /// kind), as produced by the shared resolver's `resolve_type_declaration`.
    pub declaration: ResolvedTypeDeclaration,
    /// The ordered hop chain from the requesting file to the declaration.
    pub chain: Vec<OriginHop>,
}

/// One hop in a prop's origin chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OriginHop {
    /// The declaration lives in the requesting file (no cross-file hop).
    Local,
    /// The declaration was reached by an import from another module.
    Import {
        /// The module the symbol was imported from (the resolved canonical).
        from: String,
        /// The raw import specifier, when recorded.
        specifier: Option<String>,
        /// The imported name (the name in the SOURCE module).
        imported_name: String,
    },
    /// The declaration was reached by a re-export chain.
    Reexport {
        /// The re-exporting module.
        from: String,
        /// The module the symbol re-exports TO (the next hop's canonical).
        to: String,
        /// The name the symbol is re-exported under.
        exported_name: String,
        /// The original name before the re-export rename.
        original_name: String,
    },
    /// The declaration was reached by a same-name-changing alias
    /// (`export { Foo as Bar }` / `type Bar = Foo`) within a file.
    Alias {
        /// The alias target name.
        name: String,
    },
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
    /// The resolved expose object surface (the framework-neutral
    /// [`NamedTypeMember`] pass-through, consumed by the wire `graph_export`).
    pub expose: Option<ExposeSurface>,
    /// The resolved expose fields in the component-meta `AnalyzedExposeField`
    /// shape — the per-member normalize that carries the `type_expr_scope` +
    /// JSDoc the [`ExposeSurface`] pass-through drops. Empty when no
    /// `defineExpose` surface was resolved. The component-meta extract layer
    /// reads these (the SFC object-literal fields union with them downstream).
    pub exposed_fields: Vec<AnalyzedExposeField>,
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

    /// The resolved expose fields in the component-meta [`AnalyzedExposeField`]
    /// shape, or an empty slice when no `defineExpose` surface was resolved.
    /// Mirrors [`Self::prop_fields`] / [`Self::emit_fields`] / [`Self::slot_fields`]
    /// for the expose surface.
    #[must_use]
    pub fn expose_fields(&self) -> &[AnalyzedExposeField] {
        self.exposed_fields.as_slice()
    }

    /// The resolved prop DEFAULT values (the framework-neutral SIDECAR), or an
    /// empty slice when no props surface was resolved or the adapter captured
    /// no defaults.
    #[must_use]
    pub fn prop_defaults(&self) -> &[AnalyzedDefaultValue] {
        self.props
            .as_ref()
            .map_or(&[], |surface| surface.prop_defaults.as_slice())
    }

    /// The resolved prop ORIGIN entries (the framework-neutral SIDECAR), or an
    /// empty slice when no props surface was resolved or no prop origin was
    /// resolver-known.
    #[must_use]
    pub fn prop_origins(&self) -> &[PropOriginEntry] {
        self.props
            .as_ref()
            .map_or(&[], |surface| surface.prop_origins.as_slice())
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
            exposed_fields,
            model,
        } = &dtos;
        assert!(props.is_none());
        assert!(emits.is_none());
        assert!(slots.is_none());
        assert!(options.is_none());
        assert!(expose.is_none());
        assert!(exposed_fields.is_empty());
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
            prop_defaults,
            prop_origins,
        } = &surface;
        assert!(fields.is_empty());
        assert!(index_signatures.is_empty());
        assert!(prop_defaults.is_empty());
        assert!(prop_origins.is_empty());
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
