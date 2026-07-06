//! Top-level static `import`-declaration classification for the default-deny client
//! surface.
//!
//! The SOLE authority for which static imports are admitted and what each imported
//! local's binding kind is. EVERY static import FORM is admitted — default (component
//! and plain value), named (with aliases and string-literal export names), namespace,
//! side-effect, and mixed — in BOTH the instance `<script>` and the import-only
//! `<script module>` slots, with `with { … }` import attributes preserved. The
//! fail-closed residual is exactly the non-static-import surface: the deprecated
//! `assert { … }` attribute keyword (official parse-REJECTS it), an import PHASE
//! (`import defer` / source-phase), and a TypeScript type-only import in a plain
//! script (official parse-rejects TS syntax outside `lang="ts"`).
//!
//! The classifier runs ONCE per component ([`classify_script_imports`] at IR
//! construction) and retains the per-slot outcome on the [`ClassifiedScriptImports`]
//! carrier. BOTH consumers read that same carrier — the binding-table insertion
//! (lowering, [`prepare_import_bindings`](super::state_scan::prepare_import_bindings))
//! declares the admitted carriers' locals, and the surface classification
//! (`classify_script_items`) propagates a retained refusal — so the bindings template
//! reads resolve against are exactly the imports the module prelude emits, never a
//! divergent second admit rule and never a re-classification.

use oxc_allocator::Allocator;

use super::client::UnsupportedSvelteRuntimeSurface;
use super::client_imports::{
    ImportAttributeKey, ImportName, UserImport, UserImportAttribute, UserImportSlot,
    UserImportSpecifier,
};
use super::expr::BindingRuntimeKind;
use verter_span::Span;

/// The per-slot static-import classification of one component, computed ONCE at IR
/// construction ([`classify_script_imports`]) — the single import authority BOTH
/// consumers read: the binding preparation
/// ([`prepare_import_bindings`](super::state_scan::prepare_import_bindings)) declares
/// the [`admitted`](Self::admitted) carriers' locals, and the surface classifier
/// (`classify_script_items`) propagates a retained [`slot`](Self::slot) refusal — so a
/// refused slot fails the component closed before any binding is consumed. Neither
/// consumer re-runs the classifier.
#[derive(Debug)]
pub(super) struct ClassifiedScriptImports {
    /// The `<script module>` slot outcome.
    module: Result<Vec<UserImport>, UnsupportedSvelteRuntimeSurface>,
    /// The instance `<script>` slot outcome.
    instance: Result<Vec<UserImport>, UnsupportedSvelteRuntimeSurface>,
}

impl ClassifiedScriptImports {
    /// One slot's retained classification outcome — the admitted carriers in source
    /// order, or the classifier's typed refusal (the surface classifier propagates
    /// it; the component fails closed).
    pub(super) fn slot(
        &self,
        slot: UserImportSlot,
    ) -> Result<&[UserImport], &UnsupportedSvelteRuntimeSurface> {
        match slot {
            UserImportSlot::Module => self.module.as_deref(),
            UserImportSlot::Instance => self.instance.as_deref(),
        }
    }

    /// One slot's ADMITTED carriers — EMPTY when the slot's classification was
    /// refused. A refused slot declares no bindings: the surface classifier
    /// propagates the retained refusal, so the component never reaches emission and
    /// no binding is consumed.
    pub(super) fn admitted(&self, slot: UserImportSlot) -> &[UserImport] {
        self.slot(slot).unwrap_or(&[])
    }
}

/// Classify BOTH script slots' top-level static imports ONCE, retaining the per-slot
/// outcome on the shared [`ClassifiedScriptImports`] carrier (an absent script yields
/// the empty admitted set). The sole classification entry — every consumer reads the
/// returned carrier.
pub(super) fn classify_script_imports(
    module_source: Option<&str>,
    instance_source: Option<&str>,
) -> ClassifiedScriptImports {
    ClassifiedScriptImports {
        module: module_source.map_or_else(
            || Ok(Vec::new()),
            |text| classify_static_imports(text, UserImportSlot::Module),
        ),
        instance: instance_source.map_or_else(
            || Ok(Vec::new()),
            |text| classify_static_imports(text, UserImportSlot::Instance),
        ),
    }
}

/// Classify every top-level static `import` declaration of one script into its typed
/// [`UserImport`] carrier, in source order.
///
/// STRUCTURAL over the OXC AST (never a text scan). Admits every static import form;
/// REFUSES (fail-closed, via the [`ScriptImport`](UnsupportedSvelteRuntimeSurface::ScriptImport)
/// surface) the residual forms official svelte does not accept in a plain script:
///
/// - `assert { … }` import attributes (the deprecated keyword — official
///   parse-rejects it; only `with { … }` is preserved);
/// - an import PHASE (`import defer * as ns` / source-phase imports);
/// - a TypeScript type-only import (`import type …` / `import { type T }`) — TS
///   syntax in a plain script is an official parse error, and the shared script
///   reparse is TS-lenient, so the type-only form is refused here structurally.
///
/// Non-import statements are IGNORED here (the script-item gates own them). An
/// unparseable script yields no imports (the upstream script-parse diagnostic owns
/// that refusal). Module-PRIVATE: [`classify_script_imports`] is the sole caller —
/// consumers read the shared carrier, never a fresh classification.
fn classify_static_imports(
    source: &str,
    slot: UserImportSlot,
) -> Result<Vec<UserImport>, UnsupportedSvelteRuntimeSurface> {
    let alloc = Allocator::default();
    let Some(program) = super::expr::reparse_module(&alloc, source) else {
        return Ok(Vec::new());
    };
    let mut imports = Vec::new();
    for stmt in &program.body {
        let oxc_ast::ast::Statement::ImportDeclaration(import) = stmt else {
            continue;
        };
        imports.push(classify_import_declaration(import, slot)?);
    }
    Ok(imports)
}

/// Classify ONE `ImportDeclaration` into its typed [`UserImport`], or refuse the
/// residual non-static-import forms (type-only / phase / `assert`).
fn classify_import_declaration(
    import: &oxc_ast::ast::ImportDeclaration<'_>,
    slot: UserImportSlot,
) -> Result<UserImport, UnsupportedSvelteRuntimeSurface> {
    let span = Span::new(import.span.start, import.span.end);
    let refuse =
        |construct: &'static str| UnsupportedSvelteRuntimeSurface::ScriptImport { construct, span };
    // A TypeScript TYPE-ONLY import (`import type { T } from …`) binds no runtime
    // value and is a PARSE error in an official plain script — fail closed (the
    // TS-script surface owns the elision when it lands).
    if import.import_kind != oxc_ast::ast::ImportOrExportKind::Value {
        return Err(refuse("type-only import"));
    }
    // An import PHASE (`import defer * as ns from …` / source-phase) is not the
    // static-import prelude surface — fail closed.
    if import.phase.is_some() {
        return Err(refuse("import phase"));
    }
    // Import attributes: ONLY the `with { … }` keyword is official-accepted (and
    // preserved verbatim); the deprecated `assert { … }` keyword is an official
    // parse-REJECT, and the TS-lenient shared reparse accepts it — refuse it
    // structurally here.
    let mut attributes = Vec::new();
    if let Some(with_clause) = &import.with_clause {
        if with_clause.keyword != oxc_ast::ast::WithClauseKeyword::With {
            return Err(refuse("import assertion"));
        }
        for entry in &with_clause.with_entries {
            let key = match &entry.key {
                oxc_ast::ast::ImportAttributeKey::Identifier(name) => {
                    ImportAttributeKey::Ident(name.name.as_str().to_string())
                }
                oxc_ast::ast::ImportAttributeKey::StringLiteral(lit) => {
                    ImportAttributeKey::StringLiteral(lit.value.as_str().to_string())
                }
            };
            attributes.push(UserImportAttribute {
                key,
                value: entry.value.value.as_str().to_string(),
            });
        }
    }
    // The typed specifiers, in source order. `None` (a side-effect import) yields
    // the EMPTY specifier list; `Some([])` (`import {} from …`) is likewise empty.
    let mut specifiers = Vec::new();
    if let Some(specs) = &import.specifiers {
        for spec in specs {
            match spec {
                oxc_ast::ast::ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                    specifiers.push(UserImportSpecifier::Default {
                        local: s.local.name.as_str().to_string(),
                    });
                }
                oxc_ast::ast::ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                    specifiers.push(UserImportSpecifier::Namespace {
                        local: s.local.name.as_str().to_string(),
                    });
                }
                oxc_ast::ast::ImportDeclarationSpecifier::ImportSpecifier(s) => {
                    // A per-specifier type-only member (`import { type T }`) is TS
                    // syntax in a plain script — fail closed like the decl-level form.
                    if s.import_kind != oxc_ast::ast::ImportOrExportKind::Value {
                        return Err(refuse("type-only import"));
                    }
                    let imported = match &s.imported {
                        oxc_ast::ast::ModuleExportName::IdentifierName(name) => {
                            ImportName::Ident(name.name.as_str().to_string())
                        }
                        oxc_ast::ast::ModuleExportName::IdentifierReference(name) => {
                            ImportName::Ident(name.name.as_str().to_string())
                        }
                        oxc_ast::ast::ModuleExportName::StringLiteral(lit) => {
                            ImportName::StringLiteral(lit.value.as_str().to_string())
                        }
                    };
                    specifiers.push(UserImportSpecifier::Named {
                        imported,
                        local: s.local.name.as_str().to_string(),
                    });
                }
            }
        }
    }
    Ok(UserImport {
        slot,
        source: import.source.value.as_str().to_string(),
        specifiers,
        attributes,
        span,
    })
}

/// The [`BindingRuntimeKind`] an imported LOCAL registers as — the single
/// kind-mapping authority the binding preparation consults.
///
/// A DEFAULT import of a `.svelte` component-carrier module is the component-callee
/// binding ([`ComponentImport`](BindingRuntimeKind::ComponentImport) — a `<Local/>`
/// static callee / dynamic component value resolves to it, read as the bare name).
/// EVERY other imported local — named / aliased / namespace / non-`.svelte` default —
/// is a plain imported VALUE ([`ImportedValue`](BindingRuntimeKind::ImportedValue)):
/// a NON-writable, non-signal binding whose reads are LIVE (they join the region's
/// `$.template_effect`, read plain — never `$.get`).
pub(super) fn import_specifier_binding_kind(
    specifier: &UserImportSpecifier,
    source: &str,
) -> BindingRuntimeKind {
    match specifier {
        UserImportSpecifier::Default { .. } if specifier_is_svelte_module(source) => {
            BindingRuntimeKind::ComponentImport
        }
        _ => BindingRuntimeKind::ImportedValue,
    }
}

/// The local binding names + kinds a [`UserImport`] declares, in source order — the
/// shared iteration the binding preparation and any import-local sweep consume.
pub(super) fn import_binding_entries(
    import: &UserImport,
) -> impl Iterator<Item = (&str, BindingRuntimeKind)> + '_ {
    import.specifiers.iter().map(|spec| {
        let local = match spec {
            UserImportSpecifier::Default { local }
            | UserImportSpecifier::Named { local, .. }
            | UserImportSpecifier::Namespace { local } => local.as_str(),
        };
        (local, import_specifier_binding_kind(spec, &import.source))
    })
}

/// Whether a module-specifier string resolves to a `.svelte` component-CARRIER module —
/// routed through the SINGLE language-classification authority
/// ([`LanguageRegistry::classify_static`](verter_language::LanguageRegistry::classify_static)),
/// NEVER a hand-matched extension literal (the `single_language_classifier` guard). The
/// query `?...` and hash `#...` suffixes are stripped first, so `'./Child.svelte?raw'` still
/// classifies. A `.svelte.ts` / `.svelte.js` rune MODULE is NOT a carrier
/// (`is_framework_carrier()` excludes it), and a `.vue` carrier is rejected by the
/// `is_svelte()` adapter check.
fn specifier_is_svelte_module(source: &str) -> bool {
    let path = source
        .split_once(['?', '#'])
        .map_or(source, |(head, _)| head);
    match verter_language::LanguageRegistry::global().classify_static(path) {
        verter_language::StaticClassification::Resolved(language) => {
            language.is_framework_carrier()
                && language.adapter_id().is_some_and(|id| id.is_svelte())
        }
        _ => false,
    }
}
