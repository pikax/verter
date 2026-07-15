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
//!   (`$props<T>()`) or the destructuring annotation (`let {…}: T =
//!   $props()`); the `$bindable()` member names; and the members
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
use verter_type_expr::locators::{
    AuthoredAnchor, AuthoredBodyLocator, AuthoredTypePayloadRef, LocatorSymbolSpace,
    MacroPayloadLocator, MacroPayloadPosition,
};
use verter_type_expr::TypeExpr;
use verter_type_expr_oxc::lower_ts_type;

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

/// The parse-domain Svelte script candidates for one component.
#[derive(Debug, Clone, Default, NoTypeExpr)]
pub struct SvelteScriptCandidates {
    /// The `$props()` candidate, when the component uses runes-mode props.
    pub props: Option<SveltePropsCandidate>,
    /// Members annotated with a `Snippet`-candidate import (validated by the resolved-validation half).
    pub snippet_candidates: Vec<SvelteSnippetImportCandidate>,
    /// INSTANCE-script export names — the synth folds these onto the component
    /// instance (`<script module>` exports do NOT appear here).
    pub instance_exports: Vec<String>,
    /// MODULE-script (`<script module>` / legacy `context="module"`) export
    /// names — top-level named declarations of the module, NOT instance members.
    /// The api-projector surfaces these as top-level declarations.
    pub module_exports: Vec<String>,
    /// Legacy `export let` props (legacy-mode components).
    pub legacy_props: Vec<SvelteLegacyProp>,
    /// The `createEventDispatcher<E>()` type-argument authored PAYLOAD REF
    /// (legacy emits) — `Some` whenever a type argument is authored: a bare
    /// named reference, an inline event-map literal, and an instantiation all
    /// carry a payload ref (locator + parse-stable structural hash) — never a
    /// raw `TypeExpr`, never fail-closed. `None` when no dispatcher (or no
    /// type argument) is declared.
    pub dispatcher_events: Option<AuthoredTypePayloadRef>,
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
    /// The exported instance-script members (the EXPOSE surface).
    pub instance_exports: Arc<[String]>,
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
    pub const VERSION: u32 = 4;
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
        let candidates = capture_svelte_candidates(cx.source, cx.program, cx.module_script_region);
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
            instance_exports: candidates.instance_exports.clone().into(),
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
    module_region: Option<(u32, u32)>,
) -> SvelteScriptCandidates {
    let mut out = SvelteScriptCandidates::default();
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
    let prop_kind_locals = collect_prop_kind_local_names(program, module_region);
    // ONE shared macro-ordinal walk ([`MacroOrdinalWalk`] — the addressing
    // engine the deref-side accessor replays) assigns each captured macro CALL
    // its source-order ordinal; the candidate builders below only CONSUME the
    // yielded `(ordinal, call)` pairs.
    let mut walk = MacroOrdinalWalk::new();

    for stmt in &program.body {
        // Ordinal-bearing macro calls of THIS statement (`$props()` declarators
        // and tracked dispatcher calls) — yielded by the shared walk, built
        // into candidates here. Runs before the arms below; the touched
        // candidate fields are disjoint from the export/legacy inventory.
        walk.visit_statement(stmt, module_region, &mut |macro_index, call| {
            capture_ordinal_macro_call(call, macro_index, source, &snippet_imports, &mut out);
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
                // An export's owning script block: MODULE when its statement
                // start falls inside the module-script byte region, else
                // INSTANCE. With no module region (the trait `capture` entry,
                // conservative) every export is an instance export.
                let in_module_block = statement_in_module(export.span.start, module_region);
                let exports = if in_module_block {
                    &mut out.module_exports
                } else {
                    &mut out.instance_exports
                };
                if let Some(decl) = &export.declaration {
                    // In the INSTANCE block a legacy `export let` / `export var`
                    // is a PROP, NOT an instance-script EXPOSE member, so it must
                    // not enter `instance_exports` (it is captured separately as a
                    // legacy prop below). In the MODULE block such a binding is a
                    // plain module binding and IS an export. `export const` /
                    // `export function` / `export class` are instance EXPOSE
                    // members in both blocks.
                    let skip_legacy_prop_vars = !in_module_block;
                    collect_declaration_exports(decl, exports, skip_legacy_prop_vars);
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
                    exports.push(spec.exported.name().to_string());
                }
                // Legacy-`export let` capture is INSTANCE-ONLY semantics: a
                // `<script module>` `export let` is a module binding, NOT a
                // component prop. So legacy capture runs ONLY for an
                // instance-block export (or when no module region splits
                // them). An exported `$props()` declarator is already yielded
                // by the shared ordinal walk above (same instance-only gate).
                let in_module = statement_in_module(export.span.start, module_region);
                if !in_module {
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
    exports: &mut Vec<String>,
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
                    exports.push(name);
                }
            }
        }
        Declaration::FunctionDeclaration(func) => {
            if let Some(id) = &func.id {
                exports.push(id.name.as_str().to_string());
            }
        }
        Declaration::ClassDeclaration(class) => {
            if let Some(id) = &class.id {
                exports.push(id.name.as_str().to_string());
            }
        }
        // Runtime-emit follows the TS stripper (`strip_types::typescript`): a
        // non-ambient `enum` is the ONE TS-syntax declaration the stripper LOWERS
        // to a runtime JS object (`convert_enum`), so `export enum E` IS an
        // instance EXPOSE member. An ambient `declare enum` has no runtime emit.
        Declaration::TSEnumDeclaration(en) if !en.declare => {
            exports.push(en.id.name.as_str().to_string());
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

/// One ordinal-bearing macro CALL yielded by the shared [`MacroOrdinalWalk`].
enum OrdinalMacroCall<'a, 'b> {
    /// A `$props()` declarator call (`let {…}: T = $props()` /
    /// `let x = $props<T>()`), yielded with its declarator + call init.
    Props {
        declarator: &'b VariableDeclarator<'a>,
        init: &'b Expression<'a>,
    },
    /// A TRACKED `createEventDispatcher` call — the callee's local binding was
    /// imported as `createEventDispatcher`; `import_source` is the module
    /// specifier it was imported from (owned — the walk's tracking list is
    /// walk-internal).
    Dispatcher {
        call: &'b CallExpression<'a>,
        import_source: String,
    },
}

/// The ONE source-order macro-ordinal walk — the shared addressing engine for
/// svelte macro CALLS. Both the candidate CAPTURE (which stamps each payload
/// locator's `macro_index`) and the deref-side position accessor
/// ([`lower_props_annotation_at`]) drive this same walk, so the mint-side and
/// deref-side ordinal conventions cannot drift.
///
/// Convention (each yielded call consumes one ordinal from ONE shared
/// counter — `$props()` runes and tracked dispatcher calls never alias):
///
/// - statements are walked in source order;
/// - a plain variable declaration yields its `$props()` declarators FIRST,
///   then its tracked dispatcher declarators (the deterministic capture
///   order within one statement);
/// - an exported variable declaration yields only its `$props()` declarators,
///   and ONLY in the instance block (a `<script module>` export is a module
///   binding, not a component macro); a whole-statement type-only export
///   yields nothing;
/// - dispatcher-import tracking is source-order INCREMENTAL (a call lexically
///   before its import statement is untracked and consumes no ordinal),
///   matching the single-pass capture semantics.
struct MacroOrdinalWalk {
    /// The local binding `createEventDispatcher` was imported under, mapped to
    /// its import source — accumulated as import statements are visited.
    dispatcher_imports: Vec<(String, String)>,
    /// The shared source-order ordinal counter.
    ordinal: u32,
}

impl MacroOrdinalWalk {
    fn new() -> Self {
        Self {
            dispatcher_imports: Vec::new(),
            ordinal: 0,
        }
    }

    /// Visit one top-level statement: track its dispatcher imports and yield
    /// its ordinal-bearing macro calls to `visit`.
    fn visit_statement<'a, 'b>(
        &mut self,
        stmt: &'b Statement<'a>,
        module_region: Option<(u32, u32)>,
        visit: &mut dyn FnMut(u32, OrdinalMacroCall<'a, 'b>),
    ) {
        match stmt {
            Statement::ImportDeclaration(import) => {
                collect_dispatcher_imports(import, &mut self.dispatcher_imports);
            }
            Statement::VariableDeclaration(decl) => {
                self.visit_props_declarators(&decl.declarations, visit);
                self.visit_dispatcher_declarators(&decl.declarations, visit);
            }
            Statement::ExportNamedDeclaration(export) => {
                // A whole-statement type-only export carries no runtime macro
                // call; a module-block export is a module binding, not a
                // component macro (the same instance-only gate the legacy-prop
                // capture applies).
                if export.export_kind.is_type() {
                    return;
                }
                if statement_in_module(export.span.start, module_region) {
                    return;
                }
                if let Some(Declaration::VariableDeclaration(var)) = &export.declaration {
                    self.visit_props_declarators(&var.declarations, visit);
                }
            }
            _ => {}
        }
    }

    /// Yield each `$props()` declarator, assigning one ordinal per CALL —
    /// whether or not it authors a type payload, so the ordinal stays a pure
    /// call address.
    fn visit_props_declarators<'a, 'b>(
        &mut self,
        declarators: &'b [VariableDeclarator<'a>],
        visit: &mut dyn FnMut(u32, OrdinalMacroCall<'a, 'b>),
    ) {
        for d in declarators {
            let Some(init) = &d.init else { continue };
            if !is_rune_call(init, "$props") {
                continue;
            }
            let ordinal = self.ordinal;
            self.ordinal += 1;
            visit(
                ordinal,
                OrdinalMacroCall::Props {
                    declarator: d,
                    init,
                },
            );
        }
    }

    /// Yield each TRACKED dispatcher declarator (the callee's local binding
    /// was imported as `createEventDispatcher` — an untracked global /
    /// re-export is not provenance-checkable and consumes no ordinal),
    /// assigning one ordinal per tracked CALL whether or not it authors a
    /// type argument.
    fn visit_dispatcher_declarators<'a, 'b>(
        &mut self,
        declarators: &'b [VariableDeclarator<'a>],
        visit: &mut dyn FnMut(u32, OrdinalMacroCall<'a, 'b>),
    ) {
        for d in declarators {
            let Some(init) = &d.init else { continue };
            let Expression::CallExpression(call) = init else {
                continue;
            };
            let Expression::Identifier(ident) = &call.callee else {
                continue;
            };
            // Match the LOCAL binding the dispatcher factory was imported
            // under (handles `import { createEventDispatcher as mk }`).
            let local = ident.name.as_str();
            let Some(import_source) = self
                .dispatcher_imports
                .iter()
                .find(|(binding, _)| binding == local)
                .map(|(_, src)| src.clone())
            else {
                continue;
            };
            let ordinal = self.ordinal;
            self.ordinal += 1;
            visit(
                ordinal,
                OrdinalMacroCall::Dispatcher {
                    call,
                    import_source,
                },
            );
        }
    }
}

/// Depth-closed LEAF extraction over a lowered authored props payload: an
/// inline object literal whose EVERY member value is a closed leaf
/// (primitive / literal / bare argument-less reference) maps to the
/// synthesized leaf-member vocabulary; any deeper shape (nested object,
/// function, union, generic application, index signature) yields `None` —
/// all-or-nothing, so a partial display shape is never fabricated.
fn leaf_members_from_lowered(
    expr: &TypeExpr,
) -> Option<Vec<verter_type_expr::facts::SynthesizedLeafMember>> {
    use verter_type_expr::facts::{LeafTypeFact, SynthesizedLeafMember};
    use verter_type_expr::{LiteralValue, ObjectMember};

    let TypeExpr::Object(obj) = expr else {
        return None;
    };
    let mut members = Vec::with_capacity(obj.properties.len());
    for member in obj.properties.iter() {
        let ObjectMember::Property(property) = member else {
            return None;
        };
        let leaf = match &property.ty {
            TypeExpr::Ref {
                name,
                type_arguments,
            } if type_arguments.is_empty() => LeafTypeFact::Ref(name.as_ref().to_string()),
            TypeExpr::Primitive(name) => LeafTypeFact::Primitive(*name),
            TypeExpr::Literal(LiteralValue::String(text)) => {
                LeafTypeFact::StringLiteral(text.clone())
            }
            TypeExpr::Literal(LiteralValue::Number(value)) => {
                LeafTypeFact::NumberLiteral(value.to_string())
            }
            TypeExpr::Literal(LiteralValue::Boolean(flag)) => LeafTypeFact::BooleanLiteral(*flag),
            _ => return None,
        };
        members.push(SynthesizedLeafMember {
            name: property.name.clone(),
            optional: property.optional,
            ty: leaf,
        });
    }
    Some(members)
}

/// Build the candidate inventory for one walk-yielded macro call.
fn capture_ordinal_macro_call(
    call: OrdinalMacroCall<'_, '_>,
    macro_index: u32,
    source: &str,
    snippet_imports: &[(String, String)],
    out: &mut SvelteScriptCandidates,
) {
    match call {
        OrdinalMacroCall::Props {
            declarator: d,
            init,
        } => {
            let mut candidate = SveltePropsCandidate {
                call_span: oxc_span_to_verter(init.span()),
                ..Default::default()
            };
            // 1. `$props<T>()` generic argument (wins over the annotation when
            //    authored, matching the pre-payload-ref capture precedence).
            if let Some(generic_ty) = props_generic_argument_ts_type(init) {
                candidate.props_type = Some(authored_type_payload_ref(
                    generic_ty,
                    source,
                    macro_index,
                    MacroPayloadPosition::TypeArgument,
                ));
                candidate.from_generic_argument = true;
                candidate.props_leaf_members =
                    leaf_members_from_lowered(&lower_ts_type(generic_ty, source));
            }
            // 2. destructuring annotation `let {…}: T = $props()` — the annotation
            //    rides on the DECLARATOR (not the pattern) in this OXC version.
            //    Consulted only when NO generic argument was authored.
            else if let Some(annotation) = &d.type_annotation {
                candidate.props_type = Some(authored_type_payload_ref(
                    &annotation.type_annotation,
                    source,
                    macro_index,
                    MacroPayloadPosition::TypeAnnotation,
                ));
                candidate.props_leaf_members =
                    leaf_members_from_lowered(&lower_ts_type(&annotation.type_annotation, source));
            }
            // 3. `$bindable()` members + prop DEFAULT values from the destructuring
            //    pattern (both are syntax-only reads over the destructuring +
            //    source slice).
            collect_bindable_and_defaults(&d.id, source, &mut candidate);
            // 4. snippet-candidate members from the props type — BOTH the
            //    destructuring annotation (`let {…}: { row: Snippet } = $props()`)
            //    AND the generic argument (`$props<{ row: Snippet }>()`). A member
            //    typed as a `Snippet`-candidate import is recorded (validated later).
            if let Some(annotation) = &d.type_annotation {
                collect_snippet_candidate_members(
                    &annotation.type_annotation,
                    snippet_imports,
                    out,
                );
            }
            if let Some(generic_ty) = props_generic_argument_ts_type(init) {
                collect_snippet_candidate_members(generic_ty, snippet_imports, out);
            }
            out.props = Some(candidate);
        }
        OrdinalMacroCall::Dispatcher {
            call,
            import_source,
        } => {
            if let Some(args) = &call.type_arguments {
                if let Some(first) = args.params.first() {
                    out.dispatcher_events = Some(authored_type_payload_ref(
                        first,
                        source,
                        macro_index,
                        MacroPayloadPosition::TypeArgument,
                    ));
                    // The import SOURCE is capture inventory: recorded
                    // whenever a type argument is authored
                    // (resolved-validation gates the payload ref on it).
                    out.dispatcher_import_source = Some(import_source);
                }
            }
        }
    }
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
    let mut walk = MacroOrdinalWalk::new();
    let mut found = PropsAnnotationLowering::NoPropsCall;
    for stmt in &program.body {
        walk.visit_statement(stmt, module_region, &mut |ordinal, call| {
            if ordinal != macro_index {
                return;
            }
            if let OrdinalMacroCall::Props { declarator, .. } = call {
                found = match &declarator.type_annotation {
                    Some(annotation) => PropsAnnotationLowering::Annotation(lower_ts_type(
                        &annotation.type_annotation,
                        source,
                    )),
                    None => PropsAnnotationLowering::Unannotated,
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
fn collect_snippet_candidate_members(
    ty: &TSType<'_>,
    snippet_imports: &[(String, String)],
    out: &mut SvelteScriptCandidates,
) {
    let TSType::TSTypeLiteral(literal) = ty else {
        return;
    };
    for member in &literal.members {
        let TSSignature::TSPropertySignature(sig) = member else {
            continue;
        };
        let Some(member_name) = property_key_name(&sig.key) else {
            continue;
        };
        let Some(annotation) = &sig.type_annotation else {
            continue;
        };
        if let TSType::TSTypeReference(reference) = &annotation.type_annotation {
            if let TSTypeName::IdentifierReference(local) = &reference.type_name {
                let type_name = local.name.as_str();
                if let Some((_, source)) =
                    snippet_imports.iter().find(|(name, _)| name == type_name)
                {
                    out.snippet_candidates.push(SvelteSnippetImportCandidate {
                        local_binding: type_name.to_string(),
                        import_source: source.clone(),
                        member_name: member_name.to_string(),
                    });
                }
            }
        }
    }
}

/// The content-free anchor of a svelte macro payload: the component's
/// `default` value symbol under the analyzer's local-file convention (the
/// empty producing canonical = the component's own file). Mirrors the Vue
/// analyzer's macro-payload anchor — the owning declaration is the
/// component's synthesized default-export value symbol.
fn local_default_anchor() -> AuthoredAnchor {
    AuthoredAnchor {
        canonical_id: Arc::from(""),
        symbol: Arc::from("default"),
        space: LocatorSymbolSpace::Value,
    }
}

/// Whether any captured authored-type payload ref still carries the
/// local-file EMPTY-sentinel anchor (`canonical_id == ""`) — the
/// [`ScriptFactProvider::absolutize_candidates`] no-op fast path predicate.
fn carries_empty_payload_anchor(candidates: &SvelteScriptCandidates) -> bool {
    let is_empty = |payload_ref: &AuthoredTypePayloadRef| {
        payload_ref_anchor(&payload_ref.locator)
            .canonical_id
            .is_empty()
    };
    candidates
        .props
        .as_ref()
        .and_then(|p| p.props_type.as_ref())
        .is_some_and(is_empty)
        || candidates.dispatcher_events.as_ref().is_some_and(is_empty)
}

/// The anchor of a payload-ref locator — every locator kind carries exactly
/// one authored anchor.
fn payload_ref_anchor(locator: &AuthoredBodyLocator) -> &AuthoredAnchor {
    match locator {
        AuthoredBodyLocator::DeclBody(slot) => &slot.anchor,
        AuthoredBodyLocator::AugmentationBody(aug) => &aug.anchor,
        AuthoredBodyLocator::JsdocTypedefBody(typedef) => &typedef.anchor,
        AuthoredBodyLocator::MacroPayload(payload) => &payload.anchor,
    }
}

/// Fill an EMPTY payload-ref anchor with the producing canonical. A
/// non-empty anchor may be a cross-file resolver's canonical and is never
/// rewritten (the locator contract), which also makes the fill idempotent.
/// The `payload_hash` axis is untouched — it fingerprints the authored TYPE,
/// not the anchor.
fn fill_empty_payload_ref_anchor(payload_ref: &mut AuthoredTypePayloadRef, canonical: &str) {
    let anchor = match &mut payload_ref.locator {
        AuthoredBodyLocator::DeclBody(slot) => &mut slot.anchor,
        AuthoredBodyLocator::AugmentationBody(aug) => &mut aug.anchor,
        AuthoredBodyLocator::JsdocTypedefBody(typedef) => &mut typedef.anchor,
        AuthoredBodyLocator::MacroPayload(payload) => &mut payload.anchor,
    };
    if anchor.canonical_id.is_empty() {
        anchor.canonical_id = Arc::from(canonical);
    }
}

/// The authored-type payload REFERENCE of a props / dispatcher type position:
/// a content-free [`MacroPayloadLocator`] (the re-resolution address) plus a
/// parse-stable STRUCTURAL `payload_hash` of the authored type (the cache
/// discriminator).
///
/// NEVER fail-closed: a bare named reference (`Props`), an inline object
/// literal (`{ a: string }`), and an instantiation carrying type arguments
/// (`Props<T>`) all yield a payload ref — the authored payload is carried by
/// POSITION, so no authored shape is lost, and two captures whose authored
/// content differs always discriminate through the hash.
///
/// The authored `TSType` lowers ONCE via `lower_ts_type` into transient
/// producer-local typed IR, is fingerprinted through the shared
/// alpha-normalised semantic hasher ([`compute_semantic_hash`] — span-free,
/// so the content-addressed candidate slot stays stable across
/// formatting-only edits), and is dropped — the lowered `TypeExpr` never
/// leaves this function. Capture is SYNTAX-ONLY, so every named reference
/// hashes as an unresolved reference-shape edge (name + space) via
/// [`UnresolvedLens`] — exactly the content discrimination the candidate slot
/// needs; resolved reference identity stays the fact rail's job. A
/// depth-budget-exceeded walk still yields a deterministic hash (the
/// budget-fold arm of the shared hasher).
fn authored_type_payload_ref(
    ty: &TSType<'_>,
    source: &str,
    macro_index: u32,
    payload: MacroPayloadPosition,
) -> AuthoredTypePayloadRef {
    let lowered: TypeExpr = lower_ts_type(ty, source);
    let outcome = compute_semantic_hash(&lowered, SymbolSpace::Type, &UnresolvedLens);
    AuthoredTypePayloadRef {
        locator: AuthoredBodyLocator::MacroPayload(MacroPayloadLocator {
            anchor: local_default_anchor(),
            macro_index,
            payload,
        }),
        payload_hash: outcome.hash,
    }
}

/// The raw `$props<T>()` generic type-argument OXC `TSType`, for snippet-member
/// scanning over the un-lowered annotation.
fn props_generic_argument_ts_type<'a>(init: &'a Expression<'a>) -> Option<&'a TSType<'a>> {
    let Expression::CallExpression(call) = init else {
        return None;
    };
    call.type_arguments.as_ref()?.params.first()
}

/// Whether `expr` is a call to the named rune (`$props` / `$bindable` / …).
fn is_rune_call(expr: &Expression<'_>, rune: &str) -> bool {
    if let Expression::CallExpression(call) = expr {
        if let Expression::Identifier(ident) = &call.callee {
            return ident.name == rune;
        }
    }
    false
}

/// The simple binding name of a pattern, when it is a plain identifier.
fn binding_name(pattern: &BindingPattern<'_>) -> Option<String> {
    match pattern {
        BindingPattern::BindingIdentifier(id) => Some(id.name.as_str().to_string()),
        _ => None,
    }
}

/// The static name of a property/binding key, when it is a plain identifier or
/// string literal.
fn property_key_name<'a>(key: &'a PropertyKey<'a>) -> Option<&'a str> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.as_str()),
        PropertyKey::StringLiteral(s) => Some(s.value.as_str()),
        _ => None,
    }
}

fn oxc_span_to_verter(span: oxc_span::Span) -> Span {
    Span::new(span.start, span.end)
}

/// A structural hash of the captured candidates, invariant under cosmetic edits
/// — lets the content-addressed candidate slot stay stable across formatting.
///
/// Every SEMANTIC capture field folds in DIRECTLY as typed data (the authored
/// payload refs hash their own `Hash` impls — locator + parse-stable
/// payload hash, never a `format!("{:?}", …)` debug rendering); spans
/// deliberately do NOT fold in (they shift under formatting-only edits). The
/// hash shape is versioned by [`SvelteScriptProvider::VERSION`].
fn stable_candidate_hash(candidates: &SvelteScriptCandidates) -> [u8; 16] {
    use std::hash::{Hash, Hasher};
    let mut hasher = rustc_hash::FxHasher::default();
    candidates.props.is_some().hash(&mut hasher);
    if let Some(p) = &candidates.props {
        p.from_generic_argument.hash(&mut hasher);
        p.bindable_members.hash(&mut hasher);
        // The payload REF hashes both axes: the locator (authored position)
        // and the parse-stable structural payload hash (authored content) —
        // `$props<{ a: string }>()` and `$props<{ a: number }>()` occupy
        // distinct candidate slots.
        p.props_type.hash(&mut hasher);
        // Defaults are part of the captured shape — an edited default value
        // changes the hash so the content-addressed candidate slot misses.
        for d in &p.prop_defaults {
            d.key.hash(&mut hasher);
            d.value.hash(&mut hasher);
        }
    }
    for c in &candidates.snippet_candidates {
        c.local_binding.hash(&mut hasher);
        c.import_source.hash(&mut hasher);
        c.member_name.hash(&mut hasher);
    }
    candidates.instance_exports.hash(&mut hasher);
    candidates.module_exports.hash(&mut hasher);
    for p in &candidates.legacy_props {
        p.name.hash(&mut hasher);
        p.has_default.hash(&mut hasher);
    }
    candidates.dispatcher_import_source.hash(&mut hasher);
    candidates.dispatcher_events.hash(&mut hasher);
    let h = hasher.finish();
    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&h.to_le_bytes());
    out[8..].copy_from_slice(&h.rotate_left(17).to_le_bytes());
    out
}

#[cfg(test)]
#[path = "svelte_tests.rs"]
mod tests;
