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

/// The Svelte resolved facts (resolved-validation output): the snippet-validated subset of
/// the captured candidates plus the validated props/exports inventory.
#[derive(Debug, Clone, Default)]
pub struct SvelteScriptFacts {
    /// The member names whose `Snippet`-candidate import RESOLVED to the
    /// `svelte` package — the snippet-typed props (structurally validated). A
    /// userland look-alike never appears here.
    pub validated_snippet_members: Vec<String>,
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
        if candidates.snippet_candidates.is_empty() {
            // No snippet-candidate members to validate — no resolved facts.
            return None;
        }
        // A snippet-candidate member is REAL only when its import source
        // resolved to the `svelte` PACKAGE. Structural — never a name-string
        // match: a `Snippet` imported from `./fake-svelte` is rejected even
        // though its local binding name is `Snippet`.
        let mut validated_snippet_members = Vec::new();
        for candidate in &candidates.snippet_candidates {
            let resolved_to_svelte = cx.resolved_import_targets.iter().any(|t| {
                t.specifier == candidate.import_source
                    && t.resolved_canonical
                        .as_deref()
                        .is_some_and(import_resolves_to_svelte_package)
            });
            if resolved_to_svelte {
                validated_snippet_members.push(candidate.member_name.clone());
            }
        }
        if validated_snippet_members.is_empty() {
            return None;
        }
        Some(Arc::new(SvelteScriptFacts {
            validated_snippet_members,
        }))
    }
}

/// Whether a resolved import canonical lands inside the installed `svelte`
/// package (the structural `svelte`-package membership test). The resolver
/// hands the canonical it resolved the specifier to; a `node_modules/svelte/…`
/// canonical is the package, a userland file is not.
fn import_resolves_to_svelte_package(resolved_canonical: &str) -> bool {
    // A `svelte` package import resolves into the installed package directory.
    resolved_canonical.contains("/node_modules/svelte/")
        || resolved_canonical.contains("\\node_modules\\svelte\\")
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

    for stmt in &program.body {
        match stmt {
            Statement::ImportDeclaration(import) => {
                collect_snippet_imports(import, &mut snippet_imports);
            }
            Statement::ExportNamedDeclaration(export) => {
                // An export's owning script block: MODULE when its statement
                // start falls inside the module-script byte region, else
                // INSTANCE. With no module region (the trait `capture` entry,
                // conservative) every export is an instance export.
                let exports = if statement_in_module(export.span.start, module_region) {
                    &mut out.module_exports
                } else {
                    &mut out.instance_exports
                };
                if let Some(decl) = &export.declaration {
                    collect_declaration_exports(decl, exports);
                }
                for spec in &export.specifiers {
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
                capture_dispatcher_from_var_decls(&decl.declarations, source, &mut out);
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

/// Collect top-level export names contributed by an exported declaration into
/// `exports` (the caller chose instance vs module by the export's owning block).
fn collect_declaration_exports(decl: &Declaration<'_>, exports: &mut Vec<String>) {
    match decl {
        Declaration::VariableDeclaration(var) => {
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
fn capture_dispatcher_from_var_decls(
    declarators: &[VariableDeclarator<'_>],
    source: &str,
    out: &mut SvelteScriptCandidates,
) {
    for d in declarators {
        let Some(init) = &d.init else { continue };
        if let Expression::CallExpression(call) = init {
            if let Expression::Identifier(ident) = &call.callee {
                if ident.name == "createEventDispatcher" {
                    if let Some(args) = &call.type_arguments {
                        if let Some(first) = args.params.first() {
                            out.dispatcher_events = Some(lower_ts_type(first, source));
                        }
                    }
                }
            }
        }
    }
}

/// Capture legacy `export let name = default;` props.
fn capture_legacy_export_let(
    decl: &Declaration<'_>,
    out: &mut SvelteScriptCandidates,
    is_export: bool,
) {
    if !is_export {
        return;
    }
    if let Declaration::VariableDeclaration(var) = decl {
        if !matches!(var.kind, oxc_ast::ast::VariableDeclarationKind::Let) {
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
    fn captures_dispatcher_event_type_argument() {
        let c = capture("const dispatch = createEventDispatcher<{ change: number }>();");
        assert!(c.dispatcher_events.is_some());
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
}
