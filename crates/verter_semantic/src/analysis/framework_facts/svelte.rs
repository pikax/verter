//! The Svelte script-fact provider — syntax-capture half.
//!
//! Owned by `verter_semantic` (the crate that owns the OXC pass). The
//! [`SvelteScriptProvider`] captures a `.svelte` component's runes inventory
//! from the live OXC program in the ONE shallow pass into the parse-domain
//! [`SvelteScriptCandidates`] payload:
//!
//! * `SveltePropsCandidate` — the `$props()` type, lowered ONCE via
//!   [`lower_ts_type`](verter_type_expr_oxc::lower_ts_type), from either the
//!   generic argument (`$props<T>()`) or the destructuring annotation
//!   (`let {…}: T = $props()`); the `$bindable()` member names; and the members
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
    BindingPattern, Declaration, Expression, ImportDeclarationSpecifier, Program, PropertyKey,
    Statement, TSSignature, TSType, TSTypeName, VariableDeclarator,
};
use oxc_span::GetSpan;

use verter_language::{FrameworkAdapterId, LanguageId};
use verter_span::Span;
use verter_type_expr::TypeExpr;
use verter_type_expr_oxc::lower_ts_type;

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
#[derive(Debug, Clone, Default)]
pub struct SveltePropsCandidate {
    /// The `$props()` call span.
    pub call_span: Span,
    /// The props TYPE, lowered ONCE via `lower_ts_type` from the generic
    /// argument or the destructuring annotation. `None` when the component
    /// declares no props type (an un-annotated `$props()`).
    pub props_type: Option<TypeExpr>,
    /// Whether the props type came from a `$props<T>()` generic argument
    /// (`true`) vs a `let {…}: T = $props()` annotation (`false`).
    pub from_generic_argument: bool,
    /// The `$bindable()` member names declared in the destructuring (the prop
    /// keys whose default is `$bindable(...)`).
    pub bindable_members: Vec<String>,
}

/// One member annotated with a type IMPORTED-AS-`Snippet`-CANDIDATE.
///
/// Recorded as `(local_binding, raw_import_source)` — the local type name and
/// the module specifier it was imported from. NOT validated here: the
/// resolved-validation half rejects a candidate whose `import_source` does not
/// resolve to the `svelte` package.
#[derive(Debug, Clone)]
pub struct SvelteSnippetImportCandidate {
    /// The local type binding the member is annotated with (e.g. `Snippet`).
    pub local_binding: String,
    /// The raw module specifier the binding was imported from.
    pub import_source: String,
    /// The annotated member name on the props destructuring.
    pub member_name: String,
}

/// One captured legacy `export let` prop.
#[derive(Debug, Clone)]
pub struct SvelteLegacyProp {
    /// The exported binding name.
    pub name: String,
    /// Whether the declaration carries an initializer (an optional prop).
    pub has_default: bool,
}

/// The parse-domain Svelte script candidates for one component.
#[derive(Debug, Clone, Default)]
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
    /// The `createEventDispatcher<E>()` type argument, lowered once (legacy
    /// emits). `None` when no dispatcher is declared.
    pub dispatcher_events: Option<TypeExpr>,
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
#[derive(Debug, Clone, Default)]
pub struct SvelteScriptFacts {
    /// The runes `$props()` type, lowered once (shallow-by-default — a bare
    /// `Ref` is preserved). `None` for a legacy or props-less component.
    pub props_type: Option<TypeExpr>,
    /// The `$bindable()` member names (the MODEL bindings).
    pub bindable_members: Vec<String>,
    /// The member names whose `Snippet`-candidate import RESOLVED to the
    /// `svelte` package — the snippet-typed props (structurally validated). A
    /// userland look-alike never appears here.
    pub validated_snippet_members: Vec<String>,
    /// The legacy `export let` props (legacy-mode PROPS).
    pub legacy_props: Vec<SvelteLegacyProp>,
    /// The `createEventDispatcher<E>()` event-map type — PRESENT only when the
    /// `createEventDispatcher` import resolved to the `svelte` package
    /// (provenance-validated; a userland look-alike contributes `None`).
    pub dispatcher_events: Option<TypeExpr>,
    /// The exported instance-script members (the EXPOSE surface).
    pub instance_exports: Vec<String>,
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
    pub const VERSION: u32 = 1;
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
        let dispatcher_events = candidates
            .dispatcher_import_source
            .as_deref()
            .filter(|src| specifier_resolves_to_svelte(&cx, src))
            .and(candidates.dispatcher_events.clone());

        let facts = SvelteScriptFacts {
            props_type: candidates.props.as_ref().and_then(|p| p.props_type.clone()),
            bindable_members: candidates
                .props
                .as_ref()
                .map(|p| p.bindable_members.clone())
                .unwrap_or_default(),
            validated_snippet_members,
            legacy_props: candidates.legacy_props.clone(),
            dispatcher_events,
            instance_exports: candidates.instance_exports.clone(),
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
    // pair annotated members to a candidate import.
    let mut snippet_imports: Vec<(String, String)> = Vec::new();
    // The local binding `createEventDispatcher` was imported under, mapped to its
    // import source — so the resolved-validation half can provenance-check the
    // dispatcher against the `svelte` package.
    let mut dispatcher_imports: Vec<(String, String)> = Vec::new();
    // INSTANCE-region top-level `let`/`var` binding names — the PROP-kind locals.
    // A re-export specifier (`export { x as y }`) of one of these is a re-exported
    // PROP, NOT an instance EXPOSE member; built first so the specifier loop can
    // classify. Scoped to the instance region (a module-script `let` is not a
    // prop), and a same-name `const`/function/class wins (it is an EXPOSE member).
    let prop_kind_locals = collect_prop_kind_local_names(program, module_region);

    for stmt in &program.body {
        match stmt {
            Statement::ImportDeclaration(import) => {
                collect_snippet_imports(import, &mut snippet_imports);
                collect_dispatcher_imports(import, &mut dispatcher_imports);
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
                // Props / legacy-`export let` capture is INSTANCE-ONLY semantics:
                // a `<script module>` `export let` is a module binding, NOT a
                // component prop. So props/legacy capture runs ONLY for an
                // instance-block export (or when no module region splits them).
                let in_module = statement_in_module(export.span.start, module_region);
                if !in_module {
                    if let Some(decl) = &export.declaration {
                        capture_props_from_declaration(decl, source, &snippet_imports, &mut out);
                        capture_legacy_export_let(decl, &mut out, true);
                    }
                }
            }
            Statement::VariableDeclaration(decl) => {
                capture_props_from_var_decls(
                    &decl.declarations,
                    source,
                    &snippet_imports,
                    &mut out,
                );
                capture_dispatcher_from_var_decls(
                    &decl.declarations,
                    source,
                    &dispatcher_imports,
                    &mut out,
                );
            }
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

/// Capture `$props()` from an exported declaration (rare, but legal).
fn capture_props_from_declaration(
    decl: &Declaration<'_>,
    source: &str,
    snippet_imports: &[(String, String)],
    out: &mut SvelteScriptCandidates,
) {
    if let Declaration::VariableDeclaration(var) = decl {
        capture_props_from_var_decls(&var.declarations, source, snippet_imports, out);
    }
}

/// Capture `$props()` from variable declarators.
fn capture_props_from_var_decls(
    declarators: &[VariableDeclarator<'_>],
    source: &str,
    snippet_imports: &[(String, String)],
    out: &mut SvelteScriptCandidates,
) {
    for d in declarators {
        let Some(init) = &d.init else { continue };
        if !is_rune_call(init, "$props") {
            continue;
        }
        let mut candidate = SveltePropsCandidate {
            call_span: oxc_span_to_verter(init.span()),
            ..Default::default()
        };
        // 1. `$props<T>()` generic argument.
        if let Some(type_expr) = props_generic_argument(init, source) {
            candidate.props_type = Some(type_expr);
            candidate.from_generic_argument = true;
        }
        // 2. destructuring annotation `let {…}: T = $props()` — the annotation
        //    rides on the DECLARATOR (not the pattern) in this OXC version.
        if candidate.props_type.is_none() {
            if let Some(annotation) = &d.type_annotation {
                candidate.props_type = Some(lower_ts_type(&annotation.type_annotation, source));
            }
        }
        // 3. `$bindable()` members from the destructuring pattern.
        collect_bindable_members(&d.id, &mut candidate);
        // 4. snippet-candidate members from the props type — BOTH the
        //    destructuring annotation (`let {…}: { row: Snippet } = $props()`)
        //    AND the generic argument (`$props<{ row: Snippet }>()`). A member
        //    typed as a `Snippet`-candidate import is recorded (validated later).
        if let Some(annotation) = &d.type_annotation {
            collect_snippet_candidate_members(&annotation.type_annotation, snippet_imports, out);
        }
        if let Some(generic_ty) = props_generic_argument_ts_type(init) {
            collect_snippet_candidate_members(generic_ty, snippet_imports, out);
        }
        out.props = Some(candidate);
    }
}

/// Capture `createEventDispatcher<E>()` type argument (legacy emits).
///
/// Records the dispatcher's import SOURCE (paired by the local binding the
/// dispatcher factory was imported under) so the resolved-validation half can
/// provenance-check it against the `svelte` package — a userland
/// `createEventDispatcher` look-alike must NOT contribute EMITS.
fn capture_dispatcher_from_var_decls(
    declarators: &[VariableDeclarator<'_>],
    source: &str,
    dispatcher_imports: &[(String, String)],
    out: &mut SvelteScriptCandidates,
) {
    for d in declarators {
        let Some(init) = &d.init else { continue };
        if let Expression::CallExpression(call) = init {
            if let Expression::Identifier(ident) = &call.callee {
                // Match the LOCAL binding the dispatcher factory was imported
                // under (handles `import { createEventDispatcher as mk }`).
                let local = ident.name.as_str();
                let import_source = dispatcher_imports
                    .iter()
                    .find(|(binding, _)| binding == local)
                    .map(|(_, src)| src.clone());
                if import_source.is_none() {
                    // Not a tracked `createEventDispatcher` import binding — skip
                    // (an untracked global / re-export is not provenance-checkable
                    // and therefore not a Svelte dispatcher for our purposes).
                    continue;
                }
                if let Some(args) = &call.type_arguments {
                    if let Some(first) = args.params.first() {
                        out.dispatcher_events = Some(lower_ts_type(first, source));
                        out.dispatcher_import_source = import_source;
                    }
                }
            }
        }
    }
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

/// Collect `$bindable()` member names from a `$props()` destructuring pattern
/// (`let { value = $bindable() } = $props()`).
fn collect_bindable_members(pattern: &BindingPattern<'_>, candidate: &mut SveltePropsCandidate) {
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
            }
        }
    }
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

/// The `$props<T>()` generic type argument, lowered once.
fn props_generic_argument(init: &Expression<'_>, source: &str) -> Option<TypeExpr> {
    props_generic_argument_ts_type(init).map(|ty| lower_ts_type(ty, source))
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
fn stable_candidate_hash(candidates: &SvelteScriptCandidates) -> [u8; 16] {
    use std::hash::{Hash, Hasher};
    let mut hasher = rustc_hash::FxHasher::default();
    candidates.props.is_some().hash(&mut hasher);
    if let Some(p) = &candidates.props {
        p.from_generic_argument.hash(&mut hasher);
        p.bindable_members.hash(&mut hasher);
        format!("{:?}", p.props_type).hash(&mut hasher);
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
    format!("{:?}", candidates.dispatcher_events).hash(&mut hasher);
    let h = hasher.finish();
    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&h.to_le_bytes());
    out[8..].copy_from_slice(&h.rotate_left(17).to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    fn capture(src: &str) -> SvelteScriptCandidates {
        capture_with_module_region(src, None)
    }

    fn capture_with_module_region(
        src: &str,
        module_region: Option<(u32, u32)>,
    ) -> SvelteScriptCandidates {
        let alloc = Allocator::default();
        let program = Parser::new(&alloc, src, SourceType::ts()).parse().program;
        capture_svelte_candidates(src, &program, module_region)
    }

    #[test]
    fn captures_props_generic_argument_type() {
        let c = capture("let { name } = $props<{ name: string }>();");
        let props = c.props.expect("props candidate");
        assert!(props.from_generic_argument);
        assert!(props.props_type.is_some());
    }

    #[test]
    fn captures_props_destructuring_annotation_type() {
        let c = capture("let { name }: { name: string } = $props();");
        let props = c.props.expect("props candidate");
        assert!(!props.from_generic_argument);
        assert!(props.props_type.is_some());
    }

    #[test]
    fn captures_props_from_eval_source_with_imports_and_exports() {
        // Mirror the synth-injection eval-source shape: leading blanks, an
        // import-type, the props destructuring, and an exported function.
        let src = "                  \n  import type { WidgetProps } from './props';\n  let { props }: { props: WidgetProps } = $props();\n  export function focus() {}\n";
        let c = capture(src);
        let props = c.props.expect("props candidate from eval-source");
        assert!(
            props.props_type.is_some(),
            "the destructuring annotation must lower to a props type, got None"
        );
        assert!(c.instance_exports.contains(&"focus".to_string()));
    }

    #[test]
    fn captures_bindable_members() {
        let c = capture("let { value = $bindable(), label } = $props();");
        let props = c.props.expect("props candidate");
        assert_eq!(props.bindable_members, vec!["value".to_string()]);
    }

    #[test]
    fn records_snippet_import_candidate_pairs_without_validating() {
        let src =
            "import type { Snippet } from 'svelte';\nlet { row }: { row: Snippet } = $props();";
        let c = capture(src);
        assert_eq!(c.snippet_candidates.len(), 1);
        let cand = &c.snippet_candidates[0];
        assert_eq!(cand.member_name, "row");
        assert_eq!(cand.import_source, "svelte");
        // No validation here — the pair is recorded raw.
    }

    #[test]
    fn records_snippet_candidate_from_generic_props_argument() {
        // `$props<{ row: Snippet }>()` records the snippet candidate from the
        // GENERIC argument (not just the destructuring annotation).
        // DISCRIMINATING: without the generic-arg scan this records 0 candidates.
        let src = "import type { Snippet } from 'svelte';\nlet p = $props<{ row: Snippet }>();";
        let c = capture(src);
        assert_eq!(
            c.snippet_candidates.len(),
            1,
            "generic-arg snippet candidate"
        );
        assert_eq!(c.snippet_candidates[0].member_name, "row");
        assert_eq!(c.snippet_candidates[0].import_source, "svelte");
    }

    #[test]
    fn records_userland_snippet_import_source_for_resolved_validation_rejection() {
        // A `Snippet` from a userland module is RECORDED with its source so the
        // resolved-validation can reject it (structural, never a name match).
        let src =
            "import type { Snippet } from './fake-svelte';\nlet { row }: { row: Snippet } = $props();";
        let c = capture(src);
        assert_eq!(c.snippet_candidates.len(), 1);
        assert_eq!(c.snippet_candidates[0].import_source, "./fake-svelte");
    }

    #[test]
    fn captures_instance_exports() {
        let c = capture("export const helper = 1;\nexport function go() {}\nlet local = 2;");
        assert!(c.instance_exports.contains(&"helper".to_string()));
        assert!(c.instance_exports.contains(&"go".to_string()));
        assert!(!c.instance_exports.contains(&"local".to_string()));
    }

    #[test]
    fn exported_runtime_enum_is_an_instance_export() {
        // A plain `export enum E { ... }` is a RUNTIME value binding (the TS
        // stripper lowers it to a runtime JS object), so it IS an instance EXPOSE
        // member. An ambient `export declare enum D` has no runtime emit and is
        // NOT a member; `export type Foo = ...` (type-space) is never a member.
        // DISCRIMINATING: a blanket "all leftover declarations are type-only"
        // wildcard would drop `E`.
        let src = "export enum E { A, B }\nexport declare enum D { X }\nexport type Foo = number;";
        let c = capture(src);
        assert!(
            c.instance_exports.contains(&"E".to_string()),
            "the runtime enum `E` must surface as an instance export, got {:?}",
            c.instance_exports
        );
        assert!(
            !c.instance_exports.contains(&"D".to_string()),
            "the ambient `declare enum D` has no runtime emit and must NOT be a member, got {:?}",
            c.instance_exports
        );
        assert!(
            !c.instance_exports.contains(&"Foo".to_string()),
            "the type alias `Foo` must NOT be a member, got {:?}",
            c.instance_exports
        );
    }

    #[test]
    fn exported_namespace_is_not_an_instance_export() {
        // `export namespace N { ... }` (a `TSModuleDeclaration`) is FULLY stripped
        // by the TS stripper (`strip_types::typescript` removes every
        // `TSModuleDeclaration`, unlike `enum` which it converts to runtime JS),
        // so it produces NO runtime binding and is NOT an instance EXPOSE member.
        // A sibling runtime `export const` in the same script stays. This pins the
        // stripper-aligned rule: enum → member, namespace/module → no member.
        let src = "export namespace N { export const x = 1; }\nexport const real = 2;";
        let c = capture(src);
        assert!(
            c.instance_exports.contains(&"real".to_string()),
            "the runtime `const real` must be an instance export, got {:?}",
            c.instance_exports
        );
        assert!(
            !c.instance_exports.contains(&"N".to_string()),
            "a stripped `namespace N` must NOT surface as a runtime member, got {:?}",
            c.instance_exports
        );
    }

    #[test]
    fn type_only_exports_are_not_instance_exports() {
        // Type-only exports are NOT runtime instance members and must not surface
        // as phantom EXPOSE members:
        //   - `export type { Foo }`   — the whole-statement type-only re-export.
        //   - `export { type Bar, baz }` — an inline `type` specifier (`Bar` is
        //     type-only and dropped; `baz` is a value re-export and stays).
        //   - `export const qux`      — a real value export (stays).
        // DISCRIMINATING: without the `export_kind.is_type()` filter, `Foo` and
        // `Bar` would wrongly enter `instance_exports`.
        let src = "type Foo = number;\nconst Bar = 1;\nconst baz = 2;\nexport type { Foo };\nexport { type Bar, baz };\nexport const qux = 3;";
        let c = capture(src);
        assert!(
            c.instance_exports.contains(&"baz".to_string()),
            "the value re-export `baz` must surface as an instance export, got {:?}",
            c.instance_exports
        );
        assert!(
            c.instance_exports.contains(&"qux".to_string()),
            "the value export `qux` must surface as an instance export, got {:?}",
            c.instance_exports
        );
        assert!(
            !c.instance_exports.contains(&"Foo".to_string()),
            "the type-only re-export `Foo` must NOT surface as an instance member, got {:?}",
            c.instance_exports
        );
        assert!(
            !c.instance_exports.contains(&"Bar".to_string()),
            "the inline `type Bar` specifier must NOT surface as an instance member, got {:?}",
            c.instance_exports
        );
    }

    #[test]
    fn module_exports_are_split_from_instance_exports_by_region() {
        // `export const meta` in the MODULE script region is a module export
        // (NOT an instance member); `export const ready` in the instance region
        // is an instance export. DISCRIMINATING: without the region split both
        // would land in instance_exports.
        let src = "export const meta = 1;\nexport const ready = true;\nlet local = 2;";
        // The module region covers the first export only (`export const meta`).
        let meta_end = src.find('\n').unwrap() as u32;
        let c = capture_with_module_region(src, Some((0, meta_end)));
        assert!(
            c.module_exports.contains(&"meta".to_string()),
            "`meta` in the module region is a module export, got module={:?} instance={:?}",
            c.module_exports,
            c.instance_exports
        );
        assert!(
            !c.instance_exports.contains(&"meta".to_string()),
            "`meta` (module export) must NOT be an instance member"
        );
        assert!(
            c.instance_exports.contains(&"ready".to_string()),
            "`ready` outside the module region is an instance export"
        );
    }

    #[test]
    fn captures_legacy_export_let_props() {
        let c = capture("export let name;\nexport let count = 0;");
        let names: Vec<&str> = c.legacy_props.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"name"));
        assert!(names.contains(&"count"));
        let count = c.legacy_props.iter().find(|p| p.name == "count").unwrap();
        assert!(count.has_default);
    }

    #[test]
    fn legacy_export_let_and_var_are_props_not_instance_exports() {
        // A legacy `export let` / `export var` is a PROP, NOT an instance-script
        // EXPOSE member — it must NOT enter `instance_exports` (it would otherwise
        // surface under both PROPS and EXPOSE). `export const` / `export function`
        // ARE instance members. DISCRIMINATING: a kind-blind capture put `name` /
        // `legacyVar` in BOTH.
        let c = capture(
            "export let name;\nexport var legacyVar;\nexport const ready = true;\nexport function focus() {}",
        );
        assert!(
            c.legacy_props.iter().any(|p| p.name == "name"),
            "`export let name` is a legacy prop"
        );
        assert!(
            c.legacy_props.iter().any(|p| p.name == "legacyVar"),
            "`export var legacyVar` is a legacy prop"
        );
        assert!(
            !c.instance_exports.contains(&"name".to_string()),
            "`export let name` must NOT be an instance EXPOSE member, got {:?}",
            c.instance_exports
        );
        assert!(
            !c.instance_exports.contains(&"legacyVar".to_string()),
            "`export var legacyVar` must NOT be an instance EXPOSE member, got {:?}",
            c.instance_exports
        );
        // `export const` / `export function` ARE instance members.
        assert!(c.instance_exports.contains(&"ready".to_string()));
        assert!(c.instance_exports.contains(&"focus".to_string()));
    }

    #[test]
    fn reexport_specifier_of_a_prop_local_is_a_prop_not_an_instance_export() {
        // `let local; export { local as leaked }` re-exports a PROP-kind local —
        // it is a re-exported prop, NOT an instance EXPOSE member. A `const`
        // re-export IS an instance member. DISCRIMINATING: an unconditional
        // specifier push put `leaked` into instance_exports.
        let c = capture(
            "let local = 1;\nconst stable = 2;\nexport { local as leaked, stable as exposed };",
        );
        assert!(
            c.legacy_props.iter().any(|p| p.name == "leaked"),
            "the re-exported prop-local `leaked` is a prop, got legacy_props={:?}",
            c.legacy_props
        );
        assert!(
            !c.instance_exports.contains(&"leaked".to_string()),
            "the re-exported prop-local `leaked` must NOT be an instance EXPOSE member, got {:?}",
            c.instance_exports
        );
        // A `const` re-export IS an instance member.
        assert!(
            c.instance_exports.contains(&"exposed".to_string()),
            "the re-exported const `exposed` IS an instance member, got {:?}",
            c.instance_exports
        );
        assert!(!c.legacy_props.iter().any(|p| p.name == "exposed"));
    }

    #[test]
    fn module_region_let_does_not_misclassify_an_instance_const_reexport() {
        // A MODULE-script `let conf` must NOT cause an INSTANCE-script
        // `const conf; export { conf as exposed }` to be mis-routed to props. The
        // prop-local scan is INSTANCE-region scoped AND subtracts const names.
        // DISCRIMINATING: an unscoped scan would route `exposed` to legacy_props.
        let src = "let conf = 1;\nconst conf2 = 2;\nexport { conf2 as exposed };";
        // The module region covers ONLY the first line (`let conf`).
        let module_end = src.find('\n').unwrap() as u32;
        let c = capture_with_module_region(src, Some((0, module_end)));
        assert!(
            c.instance_exports.contains(&"exposed".to_string()),
            "an instance `const` re-export is an EXPOSE member, got instance={:?} props={:?}",
            c.instance_exports,
            c.legacy_props
        );
        assert!(
            !c.legacy_props.iter().any(|p| p.name == "exposed"),
            "the instance `const` re-export must NOT be a prop"
        );
    }

    #[test]
    fn const_local_reexport_is_expose_even_with_same_name_let_absent() {
        // Subtraction rule: a `const x; export { x as y }` is EXPOSE (no `let x`
        // exists to mark it prop-kind).
        let c = capture("const x = 1;\nexport { x as y };");
        assert!(c.instance_exports.contains(&"y".to_string()));
        assert!(!c.legacy_props.iter().any(|p| p.name == "y"));
    }

    #[test]
    fn module_const_does_not_subtract_an_instance_prop_let_reexport() {
        // A MODULE-region `const value` must NOT subtract an INSTANCE-region
        // `let value; export { value as propValue }` from props (the subtraction
        // is INSTANCE-region scoped). DISCRIMINATING: an all-region subtraction
        // dropped `value` from prop-locals and routed `propValue` to EXPOSE.
        let src = "const value = 1;\nlet value2 = 2;\nexport { value2 as propValue };";
        let module_end = src.find('\n').unwrap() as u32;
        let c = capture_with_module_region(src, Some((0, module_end)));
        assert!(
            c.legacy_props.iter().any(|p| p.name == "propValue"),
            "an instance prop-let re-export is a PROP even with a module const of a different name, got props={:?} instance={:?}",
            c.legacy_props,
            c.instance_exports
        );
        assert!(
            !c.instance_exports.contains(&"propValue".to_string()),
            "the instance prop-let re-export must NOT be EXPOSE"
        );
    }

    #[test]
    fn captures_dispatcher_event_type_argument() {
        // The dispatcher factory MUST be imported (so its source is recordable
        // for provenance) — an untracked global `createEventDispatcher` is not a
        // capturable Svelte dispatcher.
        let c = capture(
            "import { createEventDispatcher } from 'svelte';\nconst dispatch = createEventDispatcher<{ change: number }>();",
        );
        assert!(c.dispatcher_events.is_some());
        assert_eq!(c.dispatcher_import_source.as_deref(), Some("svelte"));
    }

    #[test]
    fn records_dispatcher_import_source_for_userland_lookalike() {
        // A `createEventDispatcher` imported from a userland module records its
        // source so resolved-validation can reject it (provenance, not name).
        let c = capture(
            "import { createEventDispatcher } from './fake-svelte';\nconst dispatch = createEventDispatcher<{ change: number }>();",
        );
        assert!(c.dispatcher_events.is_some());
        assert_eq!(c.dispatcher_import_source.as_deref(), Some("./fake-svelte"));
    }

    #[test]
    fn untracked_global_dispatcher_is_not_captured() {
        // No import of `createEventDispatcher` ⇒ not a provenance-checkable Svelte
        // dispatcher ⇒ not captured (discriminating: the old name-only capture
        // would record it).
        let c = capture("const dispatch = createEventDispatcher<{ change: number }>();");
        assert!(c.dispatcher_events.is_none());
        assert!(c.dispatcher_import_source.is_none());
    }

    #[test]
    fn validate_rejects_userland_snippet_lookalike() {
        // Resolved-validation: a Snippet candidate whose import resolves to a
        // userland file (NOT the svelte package) is rejected — discriminating: a
        // name match would accept it.
        let provider = SvelteScriptProvider;
        let candidates = SvelteScriptCandidates {
            snippet_candidates: vec![SvelteSnippetImportCandidate {
                local_binding: "Snippet".to_string(),
                import_source: "./fake-svelte".to_string(),
                member_name: "row".to_string(),
            }],
            ..Default::default()
        };
        let envelope = FrameworkScriptCandidates {
            adapter_id: FrameworkAdapterId::svelte(),
            provider_version: SvelteScriptProvider::VERSION,
            stable_hash: [0u8; 16],
            payload: Arc::new(candidates),
        };
        let targets = vec![super::super::ResolvedImportTarget {
            specifier: "./fake-svelte".to_string(),
            resolved_canonical: Some("/src/fake-svelte.ts".to_string()),
            // A userland relative import is workspace-owned, not package-backed ⇒
            // no typed package identity (the structural rejection signal).
            package: None,
        }];
        let cx = ResolvedValidationCx {
            candidates: &envelope,
            resolved_import_targets: &targets,
            capability_on: &|_| true,
        };
        assert!(
            provider.validate(cx).is_none(),
            "a userland Snippet look-alike must NOT validate as snippet-typed"
        );
    }

    #[test]
    fn validate_accepts_real_svelte_snippet_import() {
        let provider = SvelteScriptProvider;
        let candidates = SvelteScriptCandidates {
            snippet_candidates: vec![SvelteSnippetImportCandidate {
                local_binding: "Snippet".to_string(),
                import_source: "svelte".to_string(),
                member_name: "row".to_string(),
            }],
            ..Default::default()
        };
        let envelope = FrameworkScriptCandidates {
            adapter_id: FrameworkAdapterId::svelte(),
            provider_version: SvelteScriptProvider::VERSION,
            stable_hash: [0u8; 16],
            payload: Arc::new(candidates),
        };
        let targets = vec![super::super::ResolvedImportTarget {
            specifier: "svelte".to_string(),
            resolved_canonical: Some("/project/node_modules/svelte/src/index.d.ts".to_string()),
            // The session classified the import as the `svelte` PACKAGE (the
            // typed identity the provider tests structurally).
            package: Some(super::super::ResolvedPackage::named("svelte")),
        }];
        let cx = ResolvedValidationCx {
            candidates: &envelope,
            resolved_import_targets: &targets,
            capability_on: &|_| true,
        };
        let facts = provider
            .validate(cx)
            .expect("real svelte snippet validates");
        let facts = facts
            .as_any()
            .downcast_ref::<SvelteScriptFacts>()
            .expect("svelte facts");
        assert_eq!(facts.validated_snippet_members, vec!["row".to_string()]);
    }

    #[test]
    fn validate_emits_dispatcher_only_when_resolved_to_svelte_package() {
        // A real `svelte`-resolved `createEventDispatcher` contributes
        // `dispatcher_events`; a userland look-alike does NOT (provenance, not a
        // name match). DISCRIMINATING: a name-only test would accept both.
        let provider = SvelteScriptProvider;
        let make_candidates = |src: &str| SvelteScriptCandidates {
            dispatcher_events: Some(TypeExpr::Object(std::sync::Arc::new(
                verter_type_expr::ObjectExpr {
                    properties: Vec::new(),
                },
            ))),
            dispatcher_import_source: Some(src.to_string()),
            ..Default::default()
        };
        let envelope = |c: SvelteScriptCandidates| FrameworkScriptCandidates {
            adapter_id: FrameworkAdapterId::svelte(),
            provider_version: SvelteScriptProvider::VERSION,
            stable_hash: [0u8; 16],
            payload: Arc::new(c),
        };

        // (1) Real svelte dispatcher ⇒ EMITS facts present.
        let real_env = envelope(make_candidates("svelte"));
        let real_targets = vec![super::super::ResolvedImportTarget {
            specifier: "svelte".to_string(),
            resolved_canonical: Some("/project/node_modules/svelte/index.d.ts".to_string()),
            package: Some(super::super::ResolvedPackage::named("svelte")),
        }];
        let real = provider
            .validate(ResolvedValidationCx {
                candidates: &real_env,
                resolved_import_targets: &real_targets,
                capability_on: &|_| true,
            })
            .expect("real svelte dispatcher validates");
        let real = real.as_any().downcast_ref::<SvelteScriptFacts>().unwrap();
        assert!(
            real.dispatcher_events.is_some(),
            "a svelte-resolved dispatcher contributes EMITS"
        );

        // (2) Userland look-alike ⇒ NO EMITS facts (and no other inventory ⇒ no
        // facts at all).
        let fake_env = envelope(make_candidates("./fake-svelte"));
        let fake_targets = vec![super::super::ResolvedImportTarget {
            specifier: "./fake-svelte".to_string(),
            resolved_canonical: Some("/src/fake-svelte.ts".to_string()),
            package: None,
        }];
        let fake = provider.validate(ResolvedValidationCx {
            candidates: &fake_env,
            resolved_import_targets: &fake_targets,
            capability_on: &|_| true,
        });
        assert!(
            fake.is_none(),
            "a userland createEventDispatcher look-alike must NOT contribute EMITS"
        );
    }

    #[test]
    fn validate_passes_through_parse_domain_inventory() {
        // props_type / bindable / legacy / instance exports pass through verbatim
        // (no package provenance needed for those).
        let provider = SvelteScriptProvider;
        let candidates = SvelteScriptCandidates {
            props: Some(SveltePropsCandidate {
                props_type: Some(TypeExpr::Ref {
                    name: Arc::from("Props"),
                    type_arguments: Arc::from(Vec::new().into_boxed_slice()),
                }),
                bindable_members: vec!["value".to_string()],
                ..Default::default()
            }),
            instance_exports: vec!["focus".to_string()],
            ..Default::default()
        };
        let envelope = FrameworkScriptCandidates {
            adapter_id: FrameworkAdapterId::svelte(),
            provider_version: SvelteScriptProvider::VERSION,
            stable_hash: [0u8; 16],
            payload: Arc::new(candidates),
        };
        let facts = provider
            .validate(ResolvedValidationCx {
                candidates: &envelope,
                resolved_import_targets: &[],
                capability_on: &|_| true,
            })
            .expect("props/exports inventory validates");
        let facts = facts.as_any().downcast_ref::<SvelteScriptFacts>().unwrap();
        assert!(
            matches!(&facts.props_type, Some(TypeExpr::Ref { name, .. }) if name.as_ref() == "Props")
        );
        assert_eq!(facts.bindable_members, vec!["value".to_string()]);
        assert_eq!(facts.instance_exports, vec!["focus".to_string()]);
    }
}
