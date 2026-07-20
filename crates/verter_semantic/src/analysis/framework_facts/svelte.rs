//! The Svelte script-fact provider — syntax-capture half.
//!
//! Owned by `verter_semantic` (the crate that owns the OXC pass). The
//! [`SvelteScriptProvider`] captures a `.svelte` component's runes inventory
//! from the live OXC program in the ONE shallow pass into the parse-domain
//! [`SvelteScriptCandidates`] payload:
//!
//! * `SveltePropsCandidate` — the `$props()` type, captured as an
//!   [`AuthoredTypePayloadRef`] (a content-free `MacroPayload` locator plus a
//!   parse-stable structural payload hash) from either the generic argument
//!   (`$props<T>()`), the destructuring annotation (`let {…}: T =
//!   $props()`), or the JavaScript/JSDoc equivalent (`/** @type {T} */ let
//!   {…} = $props()`); the `$bindable()` member names; and the members
//!   annotated with a binding IMPORTED-AS-`Snippet`-CANDIDATE — recorded as
//!   `(local_binding, raw_import_source)` pairs, NOT validated here;
//! * the top-level export inventory (instance-script exports the synth folds
//!   onto the component instance);
//! * the legacy `export let` props + `createEventDispatcher<E>()` type
//!   argument.
//!
//! Capture is SYNTAX-ONLY (the `script_fact_capture_is_syntax_only` guard): it
//! may touch the OXC AST + `lower_ts_type`, but MUST NOT resolve imports or read
//! capability bits. The snippet-typed classification becomes REAL only in the
//! session-side resolved-validation half ([`SvelteScriptProvider::validate`]),
//! where the candidate's import source must resolve through the resolver to the
//! `svelte` package — a `Snippet` from a userland module is rejected
//! STRUCTURALLY (never a name-string match).

use std::any::Any;
use std::sync::Arc;

use oxc_ast::ast::{
    BindingPattern, CallExpression, Declaration, Expression, ImportDeclarationSpecifier, Program,
    PropertyKey, Statement, TSSignature, TSType, TSTypeName, VariableDeclarator,
};
use oxc_span::GetSpan;

use verter_language::{FrameworkAdapterId, LanguageId};
use verter_no_typeexpr::NoTypeExpr;
use verter_span::Span;
use verter_type_expr::facts::{
    SvelteLegacyPropFact, SvelteModuleExportFact, SvelteScriptFactsFact,
};

#[path = "svelte_payload.rs"]
mod payload;
use payload::*;
#[path = "svelte_ordinal.rs"]
mod ordinal;
use ordinal::{capture_ordinal_macro_call, MacroOrdinalWalk, OrdinalMacroCall};
use verter_type_expr::locators::{
    AuthoredAnchor, AuthoredBodyLocator, AuthoredTypePayloadRef, LocatorSymbolSpace,
    MacroPayloadLocator, MacroPayloadPosition,
};
use verter_type_expr::TypeExpr;
use verter_type_expr_oxc::lower_ts_type;

use crate::analysis::jsdoc::extract_jsdoc_type_at_offset;
use crate::analysis::types::AnalyzedDefaultValue;
use crate::facts::hashing::{compute_semantic_hash, UnresolvedLens};
use crate::facts::registry::SymbolSpace;

use super::{
    FrameworkScriptCandidates, FrameworkScriptFactPayload, ResolvedValidationCx, ScriptCandidateCx,
    ScriptFactProvider, ScriptFactSyntaxGate,
};

/// The carrier language id Svelte components classify under.
pub const SVELTE_CARRIER_LANGUAGE: &str = "svelte";

/// The package the structural `Snippet` import must resolve to for a candidate
/// member to be classified as snippet-typed by the resolved-validation half. A `Snippet` imported
/// from any other module is a userland look-alike and is REJECTED.
pub const SVELTE_PACKAGE: &str = "svelte";

/// One captured `$props()` candidate.
#[derive(Debug, Clone, Default, NoTypeExpr)]
pub struct SveltePropsCandidate {
    /// The `$props()` call span.
    pub call_span: Span,
    /// The props-type authored PAYLOAD REF — `Some` whenever the component
    /// authored a props type (`$props<T>()` generic argument or
    /// `let {…}: T = $props()` annotation): the content-free `MacroPayload`
    /// locator (anchored to the component's `default` value symbol under the
    /// analyzer's local-file convention) plus a parse-stable structural hash of
    /// the authored type. A bare named reference, an inline object literal,
    /// and an instantiation carrying type arguments ALL carry a payload ref —
    /// never a raw `TypeExpr`, never fail-closed. `None` only when the
    /// component declares no props type.
    pub props_type: Option<AuthoredTypePayloadRef>,
    /// Exact syntax display of the authored props type. Captured from the
    /// OXC type node at the same time as [`Self::props_type`]; display-only,
    /// never a resolution input. This lets public declaration projection
    /// preserve function, snippet, conditional, and imported types without
    /// reparsing or source scanning at render time.
    pub props_type_display: Option<String>,
    /// Syntactic type-reference names in [`Self::props_type_display`], produced
    /// by the shared OXC type-reference visitor. The declaration projector
    /// uses these only to retain required `import type` bindings.
    pub props_type_references: Vec<String>,
    /// Whether the props type came from a `$props<T>()` generic argument
    /// (`true`) vs a `let {…}: T = $props()` annotation (`false`).
    pub from_generic_argument: bool,
    /// The `$bindable()` member names declared in the destructuring (the prop
    /// keys whose default is `$bindable(...)`).
    pub bindable_members: Vec<String>,
    /// Prop DEFAULT values captured SYNTAX-ONLY from the `$props()`
    /// destructuring: a destructuring default (`let { size = 'md' }`) records
    /// the RHS source text, and a `$bindable(<arg0>)` fallback
    /// (`value = $bindable(false)`) records the first-argument source text. A
    /// `$bindable()` with NO argument is bindable but contributes NO default.
    /// Mirrors Vue's `withDefaults` defaults — source-text + span, NOT a
    /// `TypeExpr` (defaults are runtime expressions).
    pub prop_defaults: Vec<AnalyzedDefaultValue>,
    /// Depth-closed LEAF display members for an authored props type that is an
    /// INLINE OBJECT LITERAL whose every member value is a closed leaf
    /// (primitive / literal / bare named reference) — captured SYNTAX-ONLY by
    /// lowering the authored payload once at the producer boundary.
    /// `Some(members)` lets the dispatch-free api projector render the props
    /// shape shallowly (member refs preserved un-inlined) without any
    /// render-time resolution; `None` (a named / generic / non-leaf-able
    /// payload) keeps the honest locator carrier. The payload REF above stays
    /// the semantic authority — this is a display/prelude inventory, never a
    /// resolution input.
    pub props_leaf_members: Option<Vec<verter_type_expr::facts::SynthesizedLeafMember>>,
}

/// One member annotated with a type IMPORTED-AS-`Snippet`-CANDIDATE.
///
/// Recorded as `(local_binding, raw_import_source)` — the local type name and
/// the module specifier it was imported from. NOT validated here: the
/// resolved-validation half rejects a candidate whose `import_source` does not
/// resolve to the `svelte` package.
#[derive(Debug, Clone, NoTypeExpr)]
pub struct SvelteSnippetImportCandidate {
    /// The local type binding the member is annotated with (e.g. `Snippet`).
    pub local_binding: String,
    /// The raw module specifier the binding was imported from.
    pub import_source: String,
    /// The annotated member name on the props destructuring.
    pub member_name: String,
}

/// One captured legacy `export let` prop.
#[derive(Debug, Clone, NoTypeExpr)]
pub struct SvelteLegacyProp {
    /// The exported binding name.
    pub name: String,
    /// Whether the declaration carries an initializer (an optional prop).
    pub has_default: bool,
}

/// One public instance-script export.
///
/// `export { local as public }` has two distinct identities: `public` is the
/// component API member while `local` is the value binding whose `typeof` the
/// shared resolver must demand. Keeping both prevents alias exports from being
/// resolved against a fabricated same-name local.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr)]
pub struct SvelteInstanceExport {
    /// The user-visible name on Svelte's `Component` exports surface.
    pub exported_name: String,
    /// The local value binding used to resolve the export's type.
    pub local_name: String,
    /// Neutral lexical owner of the export statement.
    pub owner: verter_type_expr::TopLevelOwnerId,
    /// Exact owner-qualified local value binding identity.
    pub binding_key: verter_type_expr::DeclBindingKey,
    /// The authored export-name token span in the owning Svelte file.
    pub source_span: Span,
}

/// One public module-script export with its exact owner-qualified binding.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr)]
pub struct SvelteModuleExport {
    pub exported_name: String,
    pub local_name: String,
    pub owner: verter_type_expr::TopLevelOwnerId,
    pub binding_key: verter_type_expr::DeclBindingKey,
    pub source_span: Span,
}

/// The parse-domain Svelte script candidates for one component.
#[derive(Debug, Clone, Default, NoTypeExpr)]
pub struct SvelteScriptCandidates {
    /// The `$props()` candidate, when the component uses runes-mode props.
    pub props: Option<SveltePropsCandidate>,
    /// Members annotated with a `Snippet`-candidate import (validated by the resolved-validation half).
    pub snippet_candidates: Vec<SvelteSnippetImportCandidate>,
    /// INSTANCE-script export names — the synth folds these onto the component
    /// instance (`<script module>` exports do NOT appear here).
    pub instance_exports: Vec<SvelteInstanceExport>,
    /// MODULE-script (`<script module>` / legacy `context="module"`) export
    /// names — top-level named declarations of the module, NOT instance members.
    /// The api-projector surfaces these as top-level declarations.
    pub module_exports: Vec<SvelteModuleExport>,
    /// Legacy `export let` props (legacy-mode components).
    pub legacy_props: Vec<SvelteLegacyProp>,
    /// The `createEventDispatcher<E>()` type-argument authored PAYLOAD REF
    /// (legacy emits) — `Some` whenever a type argument is authored: a bare
    /// named reference, an inline event-map literal, and an instantiation all
    /// carry a payload ref (locator + parse-stable structural hash) — never a
    /// raw `TypeExpr`, never fail-closed. `None` when no dispatcher (or no
    /// type argument) is declared.
    pub dispatcher_events: Option<AuthoredTypePayloadRef>,
    /// Exact syntax display of the dispatcher event-map type argument.
    pub dispatcher_events_display: Option<String>,
    /// Syntactic type-reference names in the dispatcher event-map display.
    pub dispatcher_event_references: Vec<String>,
    /// The module specifier `createEventDispatcher` was imported from (raw,
    /// un-validated). The resolved-validation half emits `dispatcher_events`
    /// ONLY when this source resolves to the `svelte` package — a userland
    /// `createEventDispatcher` look-alike does NOT contribute EMITS. `None` when
    /// the component declares no dispatcher (or imports it without a recordable
    /// specifier).
    pub dispatcher_import_source: Option<String>,
}

impl SvelteScriptCandidates {
    /// Whether the component carries any captured candidate.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.props.is_none()
            && self.snippet_candidates.is_empty()
            && self.instance_exports.is_empty()
            && self.module_exports.is_empty()
            && self.legacy_props.is_empty()
            && self.dispatcher_events.is_none()
            && self.dispatcher_import_source.is_none()
    }
}

impl FrameworkScriptFactPayload for SvelteScriptCandidates {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_arc(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
        self
    }
}

/// The Svelte resolved facts (resolved-validation output) — the full per-source
/// inventory the typeinfo surface adapter reads, with package-provenance applied
/// where it matters.
///
/// The parse-domain inventory (`props_type`, `bindable_members`, `legacy_props`,
/// `instance_exports`) passes through verbatim — those carry no package
/// provenance. The provenance-gated members are structurally validated against
/// the typed [`ResolvedPackage`](super::ResolvedPackage) identity (NEVER a
/// path/name substring): `validated_snippet_members` keep only the members whose
/// `Snippet` import resolved to the `svelte` package, and `dispatcher_events` is
/// `Some` only when `createEventDispatcher` resolved to the `svelte` package. A
/// userland look-alike never contributes to either.
///
/// The shape MIRRORS the shared persisted fact
/// [`verter_type_expr::facts::SvelteScriptFactsFact`] (authored-type payload
/// refs + `Arc<[…]>` lists, `NoTypeExpr`); it stays a distinct in-crate
/// struct only because it additionally carries `prop_defaults` (source-text
/// runtime defaults with spans — not part of the shared fact schema).
#[derive(Debug, Clone, Default, NoTypeExpr)]
pub struct SvelteScriptFacts {
    /// The runes `$props()` authored-type payload ref (shallow-by-default —
    /// the authored payload is carried by content-free locator + structural
    /// payload hash; the session re-resolves it on demand). `None` for a
    /// legacy or props-less component.
    pub props_type: Option<AuthoredTypePayloadRef>,
    /// Exact syntax display paired with [`Self::props_type`].
    pub props_type_display: Option<String>,
    /// Syntactic reference inventory paired with the props display.
    pub props_type_references: Arc<[String]>,
    /// The `$bindable()` member names (the MODEL bindings).
    pub bindable_members: Arc<[String]>,
    /// Prop DEFAULT values (source-text + span), passed through verbatim from
    /// the parse-domain capture — a destructuring default or a
    /// `$bindable(<arg0>)` fallback. No package provenance applies.
    pub prop_defaults: Arc<[AnalyzedDefaultValue]>,
    /// The member names whose `Snippet`-candidate import RESOLVED to the
    /// `svelte` package — the snippet-typed props (structurally validated). A
    /// userland look-alike never appears here.
    pub validated_snippet_members: Arc<[String]>,
    /// The legacy `export let` props (legacy-mode PROPS).
    pub legacy_props: Arc<[SvelteLegacyProp]>,
    /// The `createEventDispatcher<E>()` event-map authored-type payload ref —
    /// PRESENT only when the `createEventDispatcher` import resolved to the
    /// `svelte` package (provenance-validated; a userland look-alike
    /// contributes `None`).
    pub dispatcher_events: Option<AuthoredTypePayloadRef>,
    /// Exact syntax display of the provenance-validated dispatcher map.
    pub dispatcher_events_display: Option<String>,
    /// Syntactic reference inventory of the validated dispatcher map.
    pub dispatcher_event_references: Arc<[String]>,
    /// The exported instance-script members (the EXPOSE surface).
    pub instance_exports: Arc<[SvelteInstanceExport]>,
    /// Module-script exports with exact owner-qualified local bindings.
    pub module_exports: Arc<[SvelteModuleExportFact]>,
}

impl SvelteScriptFacts {
    /// Convert the resolved payload into its shared, span-free persisted fact.
    #[must_use]
    pub fn to_persisted_fact(&self) -> SvelteScriptFactsFact {
        SvelteScriptFactsFact {
            props_type: self.props_type.clone(),
            bindable_members: Arc::clone(&self.bindable_members),
            validated_snippet_members: Arc::clone(&self.validated_snippet_members),
            legacy_props: self
                .legacy_props
                .iter()
                .map(|prop| SvelteLegacyPropFact {
                    name: prop.name.clone(),
                    has_default: prop.has_default,
                })
                .collect(),
            dispatcher_events: self.dispatcher_events.clone(),
            instance_exports: self
                .instance_exports
                .iter()
                .map(|export| export.exported_name.clone())
                .collect(),
            module_exports: Arc::clone(&self.module_exports),
        }
    }
}

impl FrameworkScriptFactPayload for SvelteScriptFacts {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_arc(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
        self
    }
}

/// The Svelte syntax-capture script-fact provider.
#[derive(Debug, Default)]
pub struct SvelteScriptProvider;

impl SvelteScriptProvider {
    /// The provider's monotonically-versioned identity. Bump to invalidate the
    /// content-addressed candidate cache after a capture-shape change.
    ///
    /// `4` — `props_type` / `dispatcher_events` widened from bare-`Ref`-only
    /// symbol-body locators (which FAIL-CLOSED on inline / instantiation
    /// payloads and collided the candidate hash on their content) to
    /// [`AuthoredTypePayloadRef`]s: a content-free `MacroPayload` locator plus
    /// a parse-stable structural `payload_hash` of the authored type, both
    /// folded into the stable candidate hash. The redundant `*_scope`
    /// pairings are removed (the locator anchor carries the local-file
    /// convention). Old candidate keys intentionally miss.
    ///
    /// `5` — a JavaScript `$props()` destructuring binding's leading JSDoc
    /// `@type {T}` now occupies the same `TypeAnnotation` authored-payload
    /// position as the TypeScript spelling. Old candidate keys intentionally
    /// miss so a previously untyped cached JavaScript surface cannot stay warm.
    ///
    /// `6` — the OXC capture now retains an exact display string and typed
    /// reference-name inventory for props and dispatcher payloads. This is
    /// display/prelude data only; authored payload locators remain semantic
    /// authority. Old candidate keys intentionally miss.
    ///
    /// `7` — Svelte mode is now classified structurally (including store
    /// accessor and explicit-runes distinctions), and instance exports retain
    /// both exported and local binding identities. Old candidate keys
    /// intentionally miss so legacy-prop and aliased-export surfaces cannot
    /// remain warm under the corrected semantics.
    ///
    /// `8` — instance and module exports carry their exact neutral owner and
    /// owner-qualified local binding key. Old candidate keys intentionally miss
    /// so same-name bindings in module and instance scripts cannot alias.
    ///
    /// `9` — resolved and persisted Svelte facts retain module-script exports
    /// as exact, span-free owner-qualified facts. Old resolved-fact keys miss.
    pub const VERSION: u32 = 9;
}

impl ScriptFactProvider for SvelteScriptProvider {
    fn adapter_id(&self) -> FrameworkAdapterId {
        FrameworkAdapterId::svelte()
    }

    fn provider_version(&self) -> u32 {
        Self::VERSION
    }

    fn syntax_gate(&self) -> ScriptFactSyntaxGate {
        ScriptFactSyntaxGate::CarrierLanguage(LanguageId::new(SVELTE_CARRIER_LANGUAGE))
    }

    fn capture(&self, cx: ScriptCandidateCx<'_>) -> Option<FrameworkScriptCandidates> {
        let (forced_runes, template_uses_host_rune) = match cx.framework_mode_hint {
            Some(super::FrameworkScriptModeHint::Svelte {
                forced_runes,
                template_uses_host_rune,
            }) => (forced_runes, template_uses_host_rune),
            None => (None, false),
        };
        let candidates = capture_svelte_candidates(
            cx.source,
            cx.program,
            cx.top_level_owners,
            cx.module_script_region,
            forced_runes,
            template_uses_host_rune,
        );
        if candidates.is_empty() {
            return None;
        }
        let payload = Arc::new(candidates);
        Some(FrameworkScriptCandidates {
            adapter_id: self.adapter_id(),
            provider_version: self.provider_version(),
            stable_hash: stable_candidate_hash(&payload),
            payload,
        })
    }

    fn absolutize_candidates(
        &self,
        candidates: FrameworkScriptCandidates,
        canonical: &str,
    ) -> FrameworkScriptCandidates {
        if canonical.is_empty() {
            // No producing identity to absolutize to — keep the sentinel
            // rather than stamp another empty anchor.
            return candidates;
        }
        let Some(typed) = candidates.payload.downcast_ref::<SvelteScriptCandidates>() else {
            // A foreign payload envelope carries nothing this provider can
            // re-anchor; pass it through untouched.
            return candidates;
        };
        if !carries_empty_payload_anchor(typed) {
            // Nothing to fill — the envelope (payload AND `stable_hash`) is
            // already coherent; skip the clone + rehash.
            return candidates;
        }
        let mut filled = typed.clone();
        if let Some(payload_ref) = filled.props.as_mut().and_then(|p| p.props_type.as_mut()) {
            fill_empty_payload_ref_anchor(payload_ref, canonical);
        }
        if let Some(payload_ref) = filled.dispatcher_events.as_mut() {
            fill_empty_payload_ref_anchor(payload_ref, canonical);
        }
        let payload = Arc::new(filled);
        FrameworkScriptCandidates {
            adapter_id: candidates.adapter_id,
            provider_version: candidates.provider_version,
            // REBUILT hash: the candidate hash folds the payload refs
            // (locator anchors included), so the re-anchored payload
            // re-hashes — the envelope never carries a hash that disagrees
            // with its payload.
            stable_hash: stable_candidate_hash(&payload),
            payload,
        }
    }

    fn validate(
        &self,
        cx: ResolvedValidationCx<'_>,
    ) -> Option<Arc<dyn FrameworkScriptFactPayload>> {
        // Recover the typed candidates from the neutral envelope.
        let candidates = cx
            .candidates
            .payload
            .downcast_ref::<SvelteScriptCandidates>()?;
        if candidates.is_empty() {
            // Nothing captured ⇒ no resolved facts.
            return None;
        }

        // A snippet-candidate member is REAL only when its import source resolved
        // to the `svelte` PACKAGE — tested via the session-computed TYPED
        // [`ResolvedPackage`](super::ResolvedPackage) identity, NEVER a
        // name/path substring: a `Snippet` from `./fake-svelte` is rejected even
        // though its local binding name is `Snippet`.
        let mut validated_snippet_members = Vec::new();
        for candidate in &candidates.snippet_candidates {
            if specifier_resolves_to_svelte(&cx, &candidate.import_source) {
                validated_snippet_members.push(candidate.member_name.clone());
            }
        }

        // The legacy dispatcher contributes EMITS only when its
        // `createEventDispatcher` import resolved to the `svelte` package (the
        // SAME typed-package provenance test) — a userland look-alike does not.
        let dispatcher_validated = candidates
            .dispatcher_import_source
            .as_deref()
            .is_some_and(|src| specifier_resolves_to_svelte(&cx, src));
        let dispatcher_events = dispatcher_validated
            .then(|| candidates.dispatcher_events.clone())
            .flatten();

        let facts = SvelteScriptFacts {
            props_type: candidates.props.as_ref().and_then(|p| p.props_type.clone()),
            props_type_display: candidates
                .props
                .as_ref()
                .and_then(|p| p.props_type_display.clone()),
            props_type_references: candidates
                .props
                .as_ref()
                .map(|p| p.props_type_references.clone())
                .unwrap_or_default()
                .into(),
            bindable_members: candidates
                .props
                .as_ref()
                .map(|p| p.bindable_members.clone())
                .unwrap_or_default()
                .into(),
            prop_defaults: candidates
                .props
                .as_ref()
                .map(|p| p.prop_defaults.clone())
                .unwrap_or_default()
                .into(),
            validated_snippet_members: validated_snippet_members.into(),
            legacy_props: candidates.legacy_props.clone().into(),
            dispatcher_events,
            dispatcher_events_display: dispatcher_validated
                .then(|| candidates.dispatcher_events_display.clone())
                .flatten(),
            dispatcher_event_references: if dispatcher_validated {
                candidates.dispatcher_event_references.clone().into()
            } else {
                Arc::from([])
            },
            instance_exports: candidates.instance_exports.clone().into(),
            module_exports: candidates
                .module_exports
                .iter()
                .map(|export| SvelteModuleExportFact {
                    exported_name: export.exported_name.clone(),
                    local_name: export.local_name.clone(),
                    owner: export.owner,
                    binding_key: export.binding_key.clone(),
                })
                .collect(),
        };

        // The honest answer: emit facts whenever the component carries ANY
        // resolved-surface-relevant inventory. A pure-markup component with no
        // props / exports / dispatcher / snippets produces no facts (the synth
        // still synthesises its empty `$props`).
        if facts.props_type.is_none()
            && facts.bindable_members.is_empty()
            && facts.validated_snippet_members.is_empty()
            && facts.legacy_props.is_empty()
            && facts.dispatcher_events.is_none()
            && facts.instance_exports.is_empty()
            && facts.module_exports.is_empty()
        {
            return None;
        }
        Some(Arc::new(facts))
    }
}

/// Whether `specifier` resolved to the installed `svelte` PACKAGE, tested via the
/// session-computed typed [`ResolvedPackage`](super::ResolvedPackage) identity —
/// NEVER a path / name substring. A specifier whose resolved target is
/// workspace-owned (a userland `./fake-svelte`) carries no `svelte` package
/// identity and is rejected.
fn specifier_resolves_to_svelte(cx: &ResolvedValidationCx<'_>, specifier: &str) -> bool {
    cx.resolved_import_targets.iter().any(|t| {
        t.specifier == specifier && t.package.as_ref().is_some_and(|p| p.name == SVELTE_PACKAGE)
    })
}

/// Capture the Svelte candidates from a parsed (combined eval-source) program.
fn capture_svelte_candidates(
    source: &str,
    program: &Program<'_>,
    top_level_owners: &crate::analysis::top_level_owners::TopLevelOwnerTable,
    module_region: Option<(u32, u32)>,
    forced_runes: Option<bool>,
    template_uses_host_rune: bool,
) -> SvelteScriptCandidates {
    let mut out = SvelteScriptCandidates::default();
    let runes_mode = verter_parser::svelte_reactivity::infer_combined_program_mode(
        program,
        module_region,
        forced_runes,
        template_uses_host_rune,
    )
    .is_runes();
    // The local type names imported as the structural `Snippet` candidate,
    // mapped to their import source. Built first so the props destructuring can
    // pair annotated members to a candidate import. (The dispatcher-import
    // tracking lives on the shared [`MacroOrdinalWalk`] — the yielded
    // dispatcher arm carries its import source.)
    let mut snippet_imports: Vec<(String, String)> = Vec::new();
    // INSTANCE-region top-level `let`/`var` binding names — the PROP-kind locals.
    // A re-export specifier (`export { x as y }`) of one of these is a re-exported
    // PROP, NOT an instance EXPOSE member; built first so the specifier loop can
    // classify. Scoped to the instance region (a module-script `let` is not a
    // prop), and a same-name `const`/function/class wins (it is an EXPOSE member).
    let prop_kind_locals = if runes_mode {
        std::collections::HashSet::new()
    } else {
        collect_prop_kind_local_names(program, module_region)
    };
    // ONE shared macro-ordinal walk ([`MacroOrdinalWalk`] — the addressing
    // engine the deref-side accessor replays) assigns each captured macro CALL
    // its source-order ordinal; the candidate builders below only CONSUME the
    // yielded `(ordinal, call)` pairs.
    let mut walk = MacroOrdinalWalk::new();

    for (statement_index, stmt) in program.body.iter().enumerate() {
        let owner = top_level_owners.statement(statement_index).owner;
        // Ordinal-bearing macro calls of THIS statement (`$props()` declarators
        // and tracked dispatcher calls) — yielded by the shared walk, built
        // into candidates here. Runs before the arms below; the touched
        // candidate fields are disjoint from the export/legacy inventory.
        walk.visit_statement(stmt, module_region, &mut |macro_index, call| {
            capture_ordinal_macro_call(
                call,
                owner,
                macro_index,
                source,
                &snippet_imports,
                &mut out,
            );
        });
        match stmt {
            Statement::ImportDeclaration(import) => {
                collect_snippet_imports(import, &mut snippet_imports);
            }
            Statement::ExportNamedDeclaration(export) => {
                // A whole-statement type-only export (`export type { Foo }` /
                // `export type Foo = ...`) is NOT a runtime instance member — it
                // carries no value binding, so it must never surface as an EXPOSE
                // member. Skip the entire statement.
                if export.export_kind.is_type() {
                    continue;
                }
                let in_module_block =
                    matches!(owner.kind(), verter_type_expr::TopLevelOwnerKind::Module);
                let in_instance_block =
                    matches!(owner.kind(), verter_type_expr::TopLevelOwnerKind::Instance);
                if !in_module_block && !in_instance_block {
                    continue;
                }
                if let Some(decl) = &export.declaration {
                    // In the INSTANCE block a legacy `export let` / `export var`
                    // is a PROP, NOT an instance-script EXPOSE member, so it must
                    // not enter `instance_exports` (it is captured separately as a
                    // legacy prop below). In the MODULE block such a binding is a
                    // plain module binding and IS an export. `export const` /
                    // `export function` / `export class` are instance EXPOSE
                    // members in both blocks.
                    let skip_legacy_prop_vars = !in_module_block && !runes_mode;
                    let mut declaration_names = Vec::new();
                    collect_declaration_exports(
                        decl,
                        &mut declaration_names,
                        skip_legacy_prop_vars,
                    );
                    if in_module_block {
                        for (name, source_span) in declaration_names {
                            push_module_export(
                                &mut out.module_exports,
                                name.clone(),
                                name,
                                owner,
                                source_span,
                            );
                        }
                    } else {
                        for (name, source_span) in declaration_names {
                            push_instance_export(
                                &mut out.instance_exports,
                                name.clone(),
                                name,
                                owner,
                                source_span,
                            );
                        }
                    }
                }
                for spec in &export.specifiers {
                    // An inline `type` specifier (`export { type Bar, baz }`) is a
                    // type-only re-export — not a runtime instance member. Drop it
                    // (the sibling value specifiers in the same statement stay).
                    if spec.export_kind.is_type() {
                        continue;
                    }
                    // A re-export of a top-level `let`/`var` local in the INSTANCE
                    // block is a re-exported PROP, not an EXPOSE member — skip it
                    // (and record it as a legacy prop below). `const` / function /
                    // class re-exports ARE instance EXPOSE members.
                    let local_name = spec.local.name();
                    if !in_module_block && prop_kind_locals.contains(local_name.as_ref()) {
                        if !out
                            .legacy_props
                            .iter()
                            .any(|p| p.name == spec.exported.name().as_ref())
                        {
                            out.legacy_props.push(SvelteLegacyProp {
                                name: spec.exported.name().to_string(),
                                // A re-export carries no initializer of its own;
                                // optionality follows the underlying binding,
                                // which this layer does not resolve — default to
                                // required (a conservative, non-optional prop).
                                has_default: false,
                            });
                        }
                        continue;
                    }
                    let exported_name = spec.exported.name().to_string();
                    let value_root = if export.source.is_some() {
                        exported_name.clone()
                    } else {
                        local_name.to_string()
                    };
                    if in_module_block {
                        push_module_export(
                            &mut out.module_exports,
                            exported_name,
                            value_root,
                            owner,
                            oxc_span_to_verter(spec.exported.span()),
                        );
                    } else {
                        // A source re-export has no same-file local binding. Its
                        // exported name is the shallow export root the shared
                        // `typeof` resolver follows through the re-export edge.
                        push_instance_export(
                            &mut out.instance_exports,
                            exported_name,
                            value_root,
                            owner,
                            oxc_span_to_verter(spec.exported.span()),
                        );
                    }
                }
                // Legacy-`export let` capture is INSTANCE-ONLY semantics: a
                // `<script module>` `export let` is a module binding, NOT a
                // component prop. So legacy capture runs ONLY for an
                // instance-block export (or when no module region splits
                // them). An exported `$props()` declarator is already yielded
                // by the shared ordinal walk above (same instance-only gate).
                if in_instance_block && !runes_mode {
                    if let Some(decl) = &export.declaration {
                        capture_legacy_export_let(decl, &mut out, true);
                    }
                }
            }
            // `$props()` / tracked dispatcher declarators are yielded by the
            // shared ordinal walk above.
            Statement::VariableDeclaration(_) => {}
            Statement::FunctionDeclaration(_) | Statement::ClassDeclaration(_) => {}
            _ => {}
        }
    }
    out
}

/// Whether a statement starting at `start` lies inside the module-script byte
/// region. `None` region ⇒ always `false` (no module split — every export is an
/// instance export, the conservative trait-`capture` default).
fn statement_in_module(start: u32, module_region: Option<(u32, u32)>) -> bool {
    matches!(module_region, Some((s, e)) if start >= s && start < e)
}

/// Insert one instance export while preserving first-seen source order.
/// Duplicate export names are invalid JavaScript, but error-recovery programs
/// can still contain them; keeping the first prevents a malformed file from
/// publishing duplicate declaration fields.
fn push_instance_export(
    exports: &mut Vec<SvelteInstanceExport>,
    exported_name: String,
    local_name: String,
    owner: verter_type_expr::TopLevelOwnerId,
    source_span: Span,
) {
    if exports
        .iter()
        .any(|existing| existing.exported_name == exported_name)
    {
        return;
    }
    exports.push(SvelteInstanceExport {
        exported_name,
        binding_key: verter_type_expr::DeclBindingKey::new(owner, local_name.as_str()),
        local_name,
        owner,
        source_span,
    });
}

fn push_module_export(
    exports: &mut Vec<SvelteModuleExport>,
    exported_name: String,
    local_name: String,
    owner: verter_type_expr::TopLevelOwnerId,
    source_span: Span,
) {
    if exports
        .iter()
        .any(|existing| existing.exported_name == exported_name)
    {
        return;
    }
    exports.push(SvelteModuleExport {
        exported_name,
        binding_key: verter_type_expr::DeclBindingKey::new(owner, local_name.as_str()),
        local_name,
        owner,
        source_span,
    });
}

/// Collect import specifiers that import a `Snippet`-candidate binding,
/// recording `(local_binding, import_source)`.
fn collect_snippet_imports(
    import: &oxc_ast::ast::ImportDeclaration<'_>,
    out: &mut Vec<(String, String)>,
) {
    let source = import.source.value.as_str().to_string();
    let Some(specifiers) = &import.specifiers else {
        return;
    };
    for spec in specifiers {
        if let ImportDeclarationSpecifier::ImportSpecifier(named) = spec {
            // We record EVERY imported type binding as a snippet candidate
            // (the local binding name + its source). The resolved-validation
            // decides whether the SOURCE is the `svelte` package — the
            // structural test. A bare name match ("Snippet") is NOT used as
            // the classifier; the candidate is keyed on the import SOURCE.
            let imported = named.imported.name();
            if imported == "Snippet" {
                out.push((named.local.name.as_str().to_string(), source.clone()));
            }
        }
    }
}

/// The names of every INSTANCE-region top-level `let`/`var` binding in the
/// program — the PROP-kind locals. A re-export specifier (`export { x as y }`) of
/// one of these is a re-exported prop, not an instance EXPOSE member. Covers both
/// a bare top-level `let x` and an `export let x`. Scoped to the instance region
/// (a `module_region` binding is a module-script binding, NOT a prop), and a
/// same-name `const`/function/class binding is SUBTRACTED (it is an EXPOSE member,
/// so a re-export of that name is exposed, not a prop).
fn collect_prop_kind_local_names(
    program: &Program<'_>,
    module_region: Option<(u32, u32)>,
) -> std::collections::HashSet<String> {
    use oxc_ast::ast::VariableDeclarationKind;
    let mut prop_names = std::collections::HashSet::new();
    let mut expose_names = std::collections::HashSet::new();

    // A top-level statement is INSTANCE-region when it does NOT start inside the
    // module-script byte region.
    let is_instance = |start: u32| !statement_in_module(start, module_region);

    fn record_var(
        var: &oxc_ast::ast::VariableDeclaration<'_>,
        instance: bool,
        prop_names: &mut std::collections::HashSet<String>,
        expose_names: &mut std::collections::HashSet<String>,
    ) {
        let prop_kind = matches!(
            var.kind,
            VariableDeclarationKind::Let | VariableDeclarationKind::Var
        );
        for d in &var.declarations {
            if let Some(name) = binding_name(&d.id) {
                if prop_kind && instance {
                    prop_names.insert(name);
                } else if !prop_kind && instance {
                    // An INSTANCE-region `const` binding is an EXPOSE member. A
                    // module-region const must NOT subtract an instance prop-local
                    // of the same name (region-scoped subtraction).
                    expose_names.insert(name);
                }
            }
        }
    }

    for stmt in &program.body {
        match stmt {
            Statement::VariableDeclaration(var) => {
                record_var(
                    var,
                    is_instance(var.span.start),
                    &mut prop_names,
                    &mut expose_names,
                );
            }
            Statement::ExportNamedDeclaration(export) => {
                let instance = is_instance(export.span.start);
                match &export.declaration {
                    Some(Declaration::VariableDeclaration(var)) => {
                        record_var(var, instance, &mut prop_names, &mut expose_names);
                    }
                    // An INSTANCE function / class declaration is an EXPOSE member
                    // — subtract its name from the prop set (region-scoped).
                    Some(Declaration::FunctionDeclaration(func)) if instance => {
                        if let Some(id) = &func.id {
                            expose_names.insert(id.name.as_str().to_string());
                        }
                    }
                    Some(Declaration::ClassDeclaration(class)) if instance => {
                        if let Some(id) = &class.id {
                            expose_names.insert(id.name.as_str().to_string());
                        }
                    }
                    _ => {}
                }
            }
            Statement::FunctionDeclaration(func) if is_instance(func.span.start) => {
                if let Some(id) = &func.id {
                    expose_names.insert(id.name.as_str().to_string());
                }
            }
            Statement::ClassDeclaration(class) if is_instance(class.span.start) => {
                if let Some(id) = &class.id {
                    expose_names.insert(id.name.as_str().to_string());
                }
            }
            _ => {}
        }
    }
    // A name that ALSO has an INSTANCE const/function/class declaration is an
    // EXPOSE member, not a prop — subtract it.
    prop_names.retain(|n| !expose_names.contains(n));
    prop_names
}

/// Collect top-level export names contributed by an exported declaration into
/// `exports` (the caller chose instance vs module by the export's owning block).
///
/// When `skip_legacy_prop_vars` is set (the instance block), a `let`/`var`-kind
/// variable declaration is a legacy PROP — captured separately — and is NOT
/// collected as an instance EXPOSE member, so a legacy prop never surfaces under
/// both PROPS and EXPOSE. `const`-kind variable declarations, functions, and
/// classes are instance members and are always collected.
fn collect_declaration_exports(
    decl: &Declaration<'_>,
    exports: &mut Vec<(String, Span)>,
    skip_legacy_prop_vars: bool,
) {
    use oxc_ast::ast::VariableDeclarationKind;
    match decl {
        Declaration::VariableDeclaration(var) => {
            // A `let`/`var`-kind export in the instance block is a legacy prop,
            // not an EXPOSE member (`const` stays an instance member).
            if skip_legacy_prop_vars
                && matches!(
                    var.kind,
                    VariableDeclarationKind::Let | VariableDeclarationKind::Var
                )
            {
                return;
            }
            for d in &var.declarations {
                if let Some(name) = binding_name(&d.id) {
                    exports.push((name, oxc_span_to_verter(d.id.span())));
                }
            }
        }
        Declaration::FunctionDeclaration(func) => {
            if let Some(id) = &func.id {
                exports.push((id.name.as_str().to_string(), oxc_span_to_verter(id.span)));
            }
        }
        Declaration::ClassDeclaration(class) => {
            if let Some(id) = &class.id {
                exports.push((id.name.as_str().to_string(), oxc_span_to_verter(id.span)));
            }
        }
        // Runtime-emit follows the TS stripper (`strip_types::typescript`): a
        // non-ambient `enum` is the ONE TS-syntax declaration the stripper LOWERS
        // to a runtime JS object (`convert_enum`), so `export enum E` IS an
        // instance EXPOSE member. An ambient `declare enum` has no runtime emit.
        Declaration::TSEnumDeclaration(en) if !en.declare => {
            exports.push((
                en.id.name.as_str().to_string(),
                oxc_span_to_verter(en.id.span),
            ));
        }
        // Everything else is NOT a runtime instance member and contributes no
        // export name: pure type-space declarations (`export type Foo = ...`,
        // `export interface Foo {}`, `declare enum`) carry no value binding, and
        // `namespace`/`module` declarations (`TSModuleDeclaration`) are FULLY
        // stripped by the stripper (no runtime emit — unlike `enum`). (Whole-
        // statement type-only exports also short-circuit at the
        // `export_kind.is_type()` gate above; this arm keeps the helper correct
        // in isolation.)
        _ => {}
    }
}

/// Exact source display for one OXC type node. The span is produced by OXC and
/// sliced directly; this is not a scan or a semantic input. Empty/out-of-range
/// spans fail closed to `None`.
fn type_syntax_display(ty: &TSType<'_>, source: &str) -> Option<String> {
    let span = ty.span();
    source
        .get(span.start as usize..span.end as usize)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

/// Outcome of [`lower_props_annotation_at`] — the deref-side re-derivation of
/// an authored `$props()` binding-annotation payload. Every non-body arm is a
/// typed absence, never a fabricated body.
#[derive(Debug, Clone)]
pub enum PropsAnnotationLowering {
    /// The addressed `$props()` declarator carries an authored binding
    /// annotation, lowered to owned typed IR.
    Annotation(TypeExpr),
    /// The addressed `$props()` declarator authors NO binding annotation —
    /// there is no authored TYPE body at that position.
    Unannotated,
    /// `macro_index` addresses no `$props()` declarator at all (an
    /// out-of-range / drifted ordinal, or the ordinal of a non-`$props`
    /// macro call).
    NoPropsCall,
    /// The ordinal exists, but belongs to a different exact lexical owner.
    OwnerMismatch,
}

/// Outcome of [`lower_svelte_type_argument_at`] — the retained-AST
/// re-derivation of a Svelte macro's first generic type argument.
#[derive(Debug, Clone)]
pub enum SvelteTypeArgumentLowering {
    /// The addressed `$props<T>()` or tracked
    /// `createEventDispatcher<T>()` call carries a first type argument.
    TypeArgument(TypeExpr),
    /// The addressed Svelte macro call exists but has no type argument.
    Unannotated,
    /// `macro_index` addresses no Svelte macro call.
    NoMacroCall,
    /// The ordinal exists, but belongs to a different exact lexical owner.
    OwnerMismatch,
}

/// Lower the first generic type argument of the Svelte macro at
/// `macro_index` from the retained script program.
///
/// Capture and dereference both use [`MacroOrdinalWalk`], so `$props()` and
/// tracked `createEventDispatcher()` calls share one stable source-order
/// address space. This is the Svelte provider for the framework-neutral
/// [`MacroPayloadPosition::TypeArgument`] locator; it never reparses source.
#[must_use]
pub fn lower_svelte_type_argument_at(
    program: &Program<'_>,
    source: &str,
    module_region: Option<(u32, u32)>,
    macro_index: u32,
) -> SvelteTypeArgumentLowering {
    let owners =
        crate::analysis::top_level_owners::TopLevelOwnerTable::ordinary_file(program.body.len());
    lower_svelte_type_argument_at_with_owners(
        program,
        source,
        module_region,
        &owners,
        verter_type_expr::TopLevelOwnerId::ordinary_file(),
        macro_index,
    )
}

/// Owner-qualified form of [`lower_svelte_type_argument_at`]. The macro
/// ordinal and its exact lexical owner form one address; a role-only owner
/// mutation is rejected even when the ordinal still exists.
#[must_use]
pub fn lower_svelte_type_argument_at_with_owners(
    program: &Program<'_>,
    source: &str,
    module_region: Option<(u32, u32)>,
    owners: &crate::analysis::top_level_owners::TopLevelOwnerTable,
    expected_owner: verter_type_expr::TopLevelOwnerId,
    macro_index: u32,
) -> SvelteTypeArgumentLowering {
    let mut walk = MacroOrdinalWalk::new();
    let mut found = SvelteTypeArgumentLowering::NoMacroCall;
    for (statement_index, stmt) in program.body.iter().enumerate() {
        let owner = owners.statement(statement_index).owner;
        walk.visit_statement(stmt, module_region, &mut |ordinal, call| {
            if ordinal != macro_index {
                return;
            }
            if owner != expected_owner {
                found = SvelteTypeArgumentLowering::OwnerMismatch;
                return;
            }
            let ty = match call {
                OrdinalMacroCall::Props { init, .. } => props_generic_argument_ts_type(init),
                OrdinalMacroCall::Dispatcher { call, .. } => call
                    .type_arguments
                    .as_ref()
                    .and_then(|args| args.params.first()),
            };
            found = ty
                .map(|ty| SvelteTypeArgumentLowering::TypeArgument(lower_ts_type(ty, source)))
                .unwrap_or(SvelteTypeArgumentLowering::Unannotated);
        });
    }
    found
}

/// The lowered authored `$props()` binding-annotation payload at macro
/// ordinal `macro_index` — the deref-side re-derivation of the position a
/// [`MacroPayloadPosition::TypeAnnotation`] payload locator addresses.
///
/// Replays the SAME shared [`MacroOrdinalWalk`] the capture stamped the
/// locator's `macro_index` with (one addressing engine — mint side and deref
/// side cannot drift) and lowers the authored annotation through the same
/// `lower_ts_type` producer lowering the capture fingerprinted. The
/// annotation is served by POSITION: a declarator authoring BOTH a generic
/// argument and a binding annotation still carries the annotation at this
/// position (the capture's generic-argument preference is a capture policy,
/// not a position-existence fact).
#[must_use]
pub fn lower_props_annotation_at(
    program: &Program<'_>,
    source: &str,
    module_region: Option<(u32, u32)>,
    macro_index: u32,
) -> PropsAnnotationLowering {
    let owners =
        crate::analysis::top_level_owners::TopLevelOwnerTable::ordinary_file(program.body.len());
    lower_props_annotation_at_with_owners(
        program,
        source,
        module_region,
        &owners,
        verter_type_expr::TopLevelOwnerId::ordinary_file(),
        macro_index,
    )
}

/// Owner-qualified form of [`lower_props_annotation_at`]. The macro ordinal
/// and exact lexical owner are inseparable locator identity.
#[must_use]
pub fn lower_props_annotation_at_with_owners(
    program: &Program<'_>,
    source: &str,
    module_region: Option<(u32, u32)>,
    owners: &crate::analysis::top_level_owners::TopLevelOwnerTable,
    expected_owner: verter_type_expr::TopLevelOwnerId,
    macro_index: u32,
) -> PropsAnnotationLowering {
    let mut walk = MacroOrdinalWalk::new();
    let mut found = PropsAnnotationLowering::NoPropsCall;
    for (statement_index, stmt) in program.body.iter().enumerate() {
        let owner = owners.statement(statement_index).owner;
        walk.visit_statement(stmt, module_region, &mut |ordinal, call| {
            if ordinal != macro_index {
                return;
            }
            if owner != expected_owner {
                found = PropsAnnotationLowering::OwnerMismatch;
                return;
            }
            if let OrdinalMacroCall::Props { declarator, .. } = call {
                found = match &declarator.type_annotation {
                    Some(annotation) => PropsAnnotationLowering::Annotation(lower_ts_type(
                        &annotation.type_annotation,
                        source,
                    )),
                    None => extract_jsdoc_type_at_offset(source, declarator.id.span().start)
                        .map(PropsAnnotationLowering::Annotation)
                        .unwrap_or(PropsAnnotationLowering::Unannotated),
                };
            }
        });
    }
    found
}

/// Collect the local binding `createEventDispatcher` was imported under, paired
/// with its import source (`(local_binding, import_source)`).
fn collect_dispatcher_imports(
    import: &oxc_ast::ast::ImportDeclaration<'_>,
    out: &mut Vec<(String, String)>,
) {
    let source = import.source.value.as_str().to_string();
    let Some(specifiers) = &import.specifiers else {
        return;
    };
    for spec in specifiers {
        if let ImportDeclarationSpecifier::ImportSpecifier(named) = spec {
            // Keyed on the IMPORTED name `createEventDispatcher`; the local
            // binding may be aliased. The resolved-validation decides whether the
            // SOURCE is the `svelte` package (the structural test).
            if named.imported.name() == "createEventDispatcher" {
                out.push((named.local.name.as_str().to_string(), source.clone()));
            }
        }
    }
}

/// Capture legacy `export let name = default;` / `export var name` props.
///
/// Both `let` and `var` instance exports are legacy props (Svelte treats them
/// identically); a `const` export is a read-only instance member (EXPOSE), not a
/// prop, so it is NOT captured here.
fn capture_legacy_export_let(
    decl: &Declaration<'_>,
    out: &mut SvelteScriptCandidates,
    is_export: bool,
) {
    use oxc_ast::ast::VariableDeclarationKind;
    if !is_export {
        return;
    }
    if let Declaration::VariableDeclaration(var) = decl {
        if !matches!(
            var.kind,
            VariableDeclarationKind::Let | VariableDeclarationKind::Var
        ) {
            return;
        }
        for d in &var.declarations {
            // Skip `export let x = $props()` etc. — only plain bindings.
            if d.init.as_ref().is_some_and(|i| is_rune_call(i, "$props")) {
                continue;
            }
            if let Some(name) = binding_name(&d.id) {
                out.legacy_props.push(SvelteLegacyProp {
                    name,
                    has_default: d.init.is_some(),
                });
            }
        }
    }
}

/// Collect `$bindable()` member names AND prop DEFAULT values from a `$props()`
/// destructuring pattern. SYNTAX-ONLY: walks the destructuring `ObjectPattern`
/// and slices default-value source text directly — touches OXC + the source
/// slice ONLY (no import resolution, no capability bits).
///
/// Two default-bearing shapes are captured, mirroring Vue's `withDefaults`
/// source-text defaults:
///
/// - a destructuring DEFAULT (`let { size = 'md' }`) records the RHS expression
///   source text (`'md'`);
/// - a `$bindable(<arg0>)` FALLBACK (`value = $bindable(false)`) records the
///   first-argument source text (`false`) — a bindable member's default.
///
/// A `$bindable()` with NO argument is bindable but contributes NO default.
fn collect_bindable_and_defaults(
    pattern: &BindingPattern<'_>,
    source: &str,
    candidate: &mut SveltePropsCandidate,
) {
    let BindingPattern::ObjectPattern(obj) = pattern else {
        return;
    };
    for prop in &obj.properties {
        let key = property_key_name(&prop.key).map(|n| n.to_string());
        if let BindingPattern::AssignmentPattern(assign) = &prop.value {
            if is_rune_call(&assign.right, "$bindable") {
                if let Some(name) = &key {
                    candidate.bindable_members.push(name.clone());
                }
                // `$bindable(<arg0>)` carries a default; `$bindable()` does not.
                if let (Some(name), Some(arg0_span)) =
                    (&key, bindable_first_arg_span(&assign.right))
                {
                    push_default(candidate, name, source, arg0_span);
                }
            } else if let Some(name) = &key {
                // A plain destructuring default (`size = 'md'`): the RHS
                // expression source IS the default value.
                push_default(candidate, name, source, assign.right.span());
            }
        }
    }
}

/// The first-argument span of a `$bindable(<arg0>)` call expression, or `None`
/// for a no-arg `$bindable()`.
fn bindable_first_arg_span(expr: &Expression<'_>) -> Option<oxc_span::Span> {
    let Expression::CallExpression(call) = expr else {
        return None;
    };
    call.arguments.first().map(|arg| arg.span())
}

/// Record one prop default (key + sliced source text + span) on `candidate`,
/// dropping it when the slice is empty/out-of-bounds.
fn push_default(
    candidate: &mut SveltePropsCandidate,
    key: &str,
    source: &str,
    span: oxc_span::Span,
) {
    let Some(text) = source.get(span.start as usize..span.end as usize) else {
        return;
    };
    let value = text.trim();
    if value.is_empty() {
        return;
    }
    candidate.prop_defaults.push(AnalyzedDefaultValue {
        key: key.to_string(),
        value: value.to_string(),
        span: oxc_span_to_verter(span),
    });
}

/// Collect snippet-candidate members from a props TYPE-ANNOTATION object literal
/// (`let {…}: { row: Snippet } = $props()`). A member whose value type is a
/// reference to a `Snippet`-candidate import binding is recorded as
/// `(local_binding, import_source, member_name)` — NOT validated here.
#[cfg(test)]
#[path = "svelte_tests.rs"]
mod tests;
