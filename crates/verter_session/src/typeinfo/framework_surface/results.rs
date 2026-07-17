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
/// [`ResolvedPropField`] rows (each pairing its [`AnalyzedPropField`] analysis
/// with the session-resolved member-value SOURCE) and the surface's
/// [`ExpandedIndexSignature`] rows (`defineProps<{ [k: string]: string }>()`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PropsSurface {
    /// Named prop rows, each paired with its session-resolved value source.
    pub fields: Vec<ResolvedPropField>,
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
    /// Named emit fields, each paired with its session-resolved payload
    /// source.
    pub fields: Vec<ResolvedEmitField>,
    /// Index signatures on the emits type-argument surface.
    pub index_signatures: Vec<ExpandedIndexSignature>,
}

/// One session-resolved emit row: the emit analysis field plus the payload's
/// published SOURCE POSITION.
///
/// `payload_source` carries the content-free source a consumer re-raises
/// through the one shared dispatch — and it is the emit payload AUTHORITY
/// (`define_emits_shape` publishes it directly; the flat evaluated fields
/// contribute metadata only). A LOCAL authored property event carries its
/// exact authored macro-payload position (`Authored(MacroPayload(..))`, the
/// analyzer-stamped locator); an INHERITED / substituted property event
/// carries the graph-native closed/use-site source projected from its value
/// node (a complete closed leaf / leaf-union / tuple fact, the projected
/// member-path route, or the arg-preserving authored use-site body slot);
/// a realized call-signature event carries the closed payload tuple built
/// from the post-event-name parameters in the node domain — label /
/// optionality / rest / order preserved, with leaf and leaf-union element
/// facts (`Closed(Tuple(..))`) — when every parameter is closed-expressible,
/// and the projected CALLABLE-PARAMS replay route
/// ([`ProjectedTypeFact::CallableParams`](verter_type_expr::facts::ProjectedTypeFact))
/// when any parameter is richer (a named reference / composite / nested
/// object / array / callback / instantiated generic — the demand side
/// replays the signature's raw parameters through the one shared dispatch).
/// A realized emit's payload position is REQUIRED: with no stamped macro
/// type-argument base to replay off the row carries the typed
/// `Failed(UnrepresentableRequiredPayload)` position — output
/// materialization fails it instead of rendering a fabricated `unknown`
/// success.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedEmitField {
    /// The emit analysis row (name, display payload, JSDoc, authored payload
    /// locator).
    pub analysis: AnalyzedEmitField,
    /// The payload's published source position.
    pub payload_source: verter_type_expr::facts::SourcePosition,
}

/// One session-resolved prop row: the prop analysis field plus the member
/// VALUE's published SOURCE POSITION — the prop-type AUTHORITY
/// (`define_props_shape` publishes it directly; the flat evaluated fields
/// contribute metadata only). A PROVEN local authored member carries its
/// exact authored macro-payload position (`Authored(MacroPayload(..))`, the
/// analyzer-stamped locator — proven by the shared raised-shape equality);
/// a member value that decides a complete closed leaf / leaf-union / tuple
/// carries the closed fact; a resolvable reference carries its shallow
/// symbol-reference carrier; every remaining KNOWN structural value carries
/// the projected MEMBER-PATH replay route off the macro's stamped
/// type-argument base
/// ([`ProjectedTypeFact::MemberPath`](verter_type_expr::facts::ProjectedTypeFact)
/// — replayed through the one shared dispatch on demand). A type-based
/// macro member's value-type position is REQUIRED: a genuine miss carries
/// the typed `Failed(UnrepresentableRequiredMemberValue)` position — output
/// materialization fails it instead of rendering a fabricated `unknown`
/// success. A `defineModel` synthesized prop row carries its authored
/// type-argument position, or the PROVEN unannotated absence for an untyped
/// `defineModel()`.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedPropField {
    /// The prop analysis row (name, optionality, display annotation, JSDoc,
    /// authored payload locator).
    pub analysis: AnalyzedPropField,
    /// The member value's published source position.
    pub type_source: verter_type_expr::facts::SourcePosition,
}

/// One session-resolved expose row: the expose analysis field plus the
/// member VALUE's published SOURCE POSITION — the exposed-type AUTHORITY
/// for `defineExpose<T>()` type-argument surface members (the extraction
/// layer publishes it directly; the flat evaluated lane contributes
/// metadata only). Same source vocabulary as [`ResolvedPropField`]; expose
/// analyzer fields never stamp an authored payload, so the sources are the
/// closed/ref upgrades, the projected member-path replay route, or the
/// typed failure for a genuine miss.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedExposeField {
    /// The expose analysis row (name, JSDoc).
    pub analysis: AnalyzedExposeField,
    /// The member value's published source position.
    pub type_source: verter_type_expr::facts::SourcePosition,
}

/// The resolved `defineOptions<T>()` object surface.
///
/// The pass-through object surface the options projection produces — the named
/// members of the options type argument, each carrying its sealed shallow
/// [`NamedTypeMemberOutput`] value.
#[derive(Debug, Clone, Default, PartialEq, verter_no_typeexpr::NoTypeExpr)]
pub struct OptionsSurface {
    /// Named members of the options object surface.
    pub members: Vec<NamedTypeMember>,
}

/// The resolved `defineExpose<T>()` object surface.
#[derive(Debug, Clone, Default, PartialEq, verter_no_typeexpr::NoTypeExpr)]
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

/// The sealed shallow OUTPUT value of a named object-surface member — the
/// CLOSED vocabulary the zero-dispatch wire encoder publishes for an
/// options / expose member value. One arm per shallow-encodable shape:
/// a primitive / literal leaf, a bare named reference (type arguments are
/// NOT carried — expanding them would be an eager second walk), the empty
/// object surface, and the [`Self::Opaque`] degradation for every value
/// outside the shallow vocabulary (never a fabricated ref, never a raw
/// `TypeExpr`). `NoTypeExpr` by derive; there is no public unwrap back to
/// a `TypeExpr`.
#[derive(Debug, Clone, PartialEq, verter_no_typeexpr::NoTypeExpr)]
pub enum NamedTypeMemberOutput {
    /// A primitive leaf (`string` / `number` / …).
    Primitive(verter_type_expr::PrimitiveName),
    /// A literal leaf (`"solid"` / `42` / `true` / `1n`).
    Literal(verter_type_expr::LiteralValue),
    /// A bare named reference — the shallow-by-default escape (the consumer
    /// re-resolves the name on demand).
    Ref {
        /// The referenced type name.
        name: Arc<str>,
    },
    /// The empty object surface (`{}`) — the one structural shape the shallow
    /// vocabulary encodes directly.
    EmptyObject,
    /// A resolved value outside the shallow output vocabulary — degraded at
    /// CONSTRUCTION time (the producer classifies and discards its transient
    /// raised form), encoded as a structurally-unencodable opaque on the wire.
    Opaque,
}

impl NamedTypeMemberOutput {
    /// Classify a producer-transient raised [`TypeExpr`] into the closed
    /// shallow output vocabulary. The `TypeExpr` is read ONCE at the
    /// publication boundary and discarded — it never enters the DTO.
    ///
    /// Mirrors the zero-dispatch wire encoder's shallow member-value rules
    /// exactly (wire parity): primitive / literal leaves map to their arms, a
    /// named `Ref` keeps ONLY its name (arguments are not expanded), an EMPTY
    /// object literal maps to [`Self::EmptyObject`], and every other shape
    /// degrades to [`Self::Opaque`].
    pub(crate) fn classify_shallow(raised: &TypeExpr) -> Self {
        match raised {
            TypeExpr::Primitive(name) => Self::Primitive(*name),
            TypeExpr::Literal(lit) => Self::Literal(lit.clone()),
            TypeExpr::Ref { name, .. } => Self::Ref {
                name: Arc::clone(name),
            },
            TypeExpr::Object(obj) if obj.properties.is_empty() => Self::EmptyObject,
            _ => Self::Opaque,
        }
    }
}

/// A named member of a resolved object surface (options / expose).
#[derive(Debug, Clone, PartialEq, verter_no_typeexpr::NoTypeExpr)]
pub struct NamedTypeMember {
    /// The member name.
    pub name: String,
    /// Whether the member is optional.
    pub is_optional: bool,
    /// The member's sealed shallow output value: `None` when no resolved type
    /// was available at all; `Some(NamedTypeMemberOutput::Opaque)` when a
    /// resolved value exists but lies outside the shallow output vocabulary.
    /// The two states stay distinct on the wire (different opaque
    /// diagnostics).
    pub value: Option<NamedTypeMemberOutput>,
    /// Exact TypeScript display of the resolved member value when the producing
    /// output sink materialized the full value type. This is a display-only
    /// publication sidecar: semantic decisions continue to use the graph node
    /// and [`Self::value`] remains the closed shallow wire vocabulary.
    pub type_annotation: Option<String>,
    /// Named type references preserved by [`Self::type_annotation`]. Public
    /// declaration projectors use this inventory to retain only the imports
    /// required by the emitted annotation; it is never a resolution input.
    pub type_references: Vec<String>,
    /// The most relevant authored member-name span in the owning carrier, when
    /// the framework capture can provide one. Generated declaration source maps
    /// use this anchor for definition/navigation fidelity.
    pub source_span: Option<verter_span::Span>,
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
    /// The resolved expose rows in the component-meta shape — the per-member
    /// normalize that carries the JSDoc the [`ExposeSurface`] pass-through
    /// drops, each paired with its session-resolved member-value SOURCE.
    /// Empty when no `defineExpose` surface was resolved. The component-meta
    /// extract layer reads these (the SFC object-literal fields union with
    /// them downstream).
    pub exposed_fields: Vec<ResolvedExposeField>,
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
    pub fn prop_fields(&self) -> &[ResolvedPropField] {
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

    /// The resolved emit rows (analysis field + payload source), or an empty
    /// slice when no emits surface was resolved.
    #[must_use]
    pub fn emit_fields(&self) -> &[ResolvedEmitField] {
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
    pub fn expose_fields(&self) -> &[ResolvedExposeField] {
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

/// A resolved macro DTO bundle PLUS the per-result completeness the cold
/// compute observed.
///
/// `vue_macro_dtos_with_ctx` hands this back instead of a bare
/// `Arc<MacroSurfaceDtos>` so every consumer can fold the surface's
/// partiality into its own request-result completeness. A `Partial` bundle
/// (a budget exhaustion / fatal `QueryError` tripped the surface
/// resolution mid-materialisation) is RETURNED to the caller but is NEVER
/// admitted into the host's `vue_surface_store` — a partial surface in the
/// store would launder a warm complete replay on the next request (the
/// no-poison invariant). Consumers fold `completeness` via
/// [`crate::request_context::mark_request_result_partial`]
/// (see [`Self::observe_partial`]) so the enclosing component-meta result's
/// warm promotion is refused too.
#[derive(Debug, Clone)]
pub struct MacroDtosRead {
    /// The resolved (possibly partial) DTO bundle.
    pub dtos: std::sync::Arc<MacroSurfaceDtos>,
    /// The completeness of the cold compute that produced `dtos`. `Complete`
    /// when the bundle was served from a warm store hit (only `Complete`
    /// bundles ever enter the store) or resolved without tripping a fuse.
    pub completeness: crate::semantic_query::ResultCompleteness,
}

impl MacroDtosRead {
    /// Whether the resolved surface is a genuine partial.
    #[must_use]
    pub fn is_partial(&self) -> bool {
        self.completeness.is_partial()
    }

    /// Fold a partial surface into the active request-result completeness so
    /// the enclosing component-meta result's warm promotion is refused. A
    /// no-op when the surface is `Complete`.
    pub fn observe_partial(&self) {
        if self.completeness.is_partial() {
            crate::request_context::mark_request_result_partial();
        }
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
