//! Instance-script `import`-declaration classification for the default-deny client
//! surface.
//!
//! The SOLE authority for which `.svelte` imports are admitted: a default import of a
//! `.svelte` component module is the supported component-callee subset (admitted as a
//! typed [`UserImport::ComponentDefault`] prelude carrier); every other import FORM —
//! named / namespace / side-effect / mixed / default-non-`.svelte` — is the broad
//! static-import prelude (not yet supported) and fails closed. The admit predicate is the
//! SINGLE rail the prelude carrier (classification) AND the
//! [`BindingRuntimeKind::ComponentImport`](super::expr::BindingRuntimeKind) binding-table
//! insertion (lowering) both consult.

use oxc_allocator::Allocator;

use super::client::UnsupportedSvelteRuntimeSurface;
use super::client_plan_types::UserImport;
use verter_span::Span;

/// Classify every instance-script `import` declaration: ADMIT a default import of a
/// `.svelte` component module (the component-callee subset) as a typed
/// [`UserImport::ComponentDefault`], and REFUSE every other import FORM — named,
/// namespace, side-effect, default-non-`.svelte`, or a mixed default+named — via the
/// [`ScriptImport`](UnsupportedSvelteRuntimeSurface::ScriptImport) deferral (the broad
/// static-import prelude, not yet supported).
///
/// The admit predicate is STRUCTURAL over the OXC AST + a module-specifier extension
/// test (`.svelte`): `import Child from './Child.svelte'` is exactly ONE
/// `ImportDefaultSpecifier` whose source path ends with `.svelte`. The specifier
/// string is a real import-source PATH (not type text), so the extension check is
/// outside the typed-IR text-ban — query/hash suffixes are stripped before the test.
pub(super) fn classify_instance_imports(
    instance_source: &str,
) -> Result<Vec<UserImport>, UnsupportedSvelteRuntimeSurface> {
    let alloc = Allocator::default();
    let Some(program) = super::expr::reparse_module(&alloc, instance_source) else {
        return Ok(Vec::new());
    };
    let mut imports = Vec::new();
    for stmt in &program.body {
        let oxc_ast::ast::Statement::ImportDeclaration(import) = stmt else {
            continue;
        };
        let span = Span::new(import.span.start, import.span.end);
        match admitted_svelte_component_import_local(import) {
            Some(local) => imports.push(UserImport::ComponentDefault {
                local: local.to_string(),
                source: import.source.value.as_str().to_string(),
                span,
            }),
            // Every other import form is the broad static-import-prelude deferral.
            None => {
                return Err(UnsupportedSvelteRuntimeSurface::ScriptImport {
                    construct: "import",
                    span,
                })
            }
        }
    }
    Ok(imports)
}

/// The local name of an admitted default `.svelte`-COMPONENT import declaration
/// (`import Local from './X.svelte'`) — EXACTLY one `ImportDefaultSpecifier` whose source
/// resolves to a `.svelte` component-carrier module — or `None` for every other import
/// FORM (named / namespace / side-effect / mixed default+named / default-non-`.svelte`). A
/// mixed `import D, { n } from …` carries TWO specifiers; a side-effect `import '…'`
/// carries `None`.
///
/// This is the SINGLE admit predicate that the [`UserImport::ComponentDefault`] prelude
/// carrier (classification) AND the [`BindingRuntimeKind::ComponentImport`](super::expr::BindingRuntimeKind)
/// binding-table insertion (lowering, [`prepare_component_import_bindings`](super::state_scan::prepare_component_import_bindings))
/// both consult — so the non-reactive binding a `<Local/>` static callee RESOLVES against
/// is exactly the import the module prelude emits, never a divergent second admit rule.
pub(super) fn admitted_svelte_component_import_local<'a>(
    import: &oxc_ast::ast::ImportDeclaration<'a>,
) -> Option<&'a str> {
    let default_local = import
        .specifiers
        .as_ref()
        .and_then(|specs| match specs.as_slice() {
            [oxc_ast::ast::ImportDeclarationSpecifier::ImportDefaultSpecifier(spec)] => {
                Some(spec.local.name.as_str())
            }
            _ => None,
        })?;
    specifier_is_svelte_module(import.source.value.as_str()).then_some(default_local)
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
