//! The CROSS-SCRIPT duplicate-declaration scans — the component-level authority the
//! official-reject gate consults for a binding declared in BOTH the `<script module>`
//! slot and the instance `<script>` slot. The per-body parse probe
//! (`script_body_parse.rs`) owns SAME-body duplicates (Acorn raises them at parse,
//! `js_parse_error`); this module owns the cross-body families the per-body probe
//! cannot see.
//!
//! Official `svelte@5.56.3` merges both slots into ONE emitted module with the
//! instance scope a CHILD of the module scope (`phases/2-analyze/index.js`). Two
//! collision families follow (both oracle-probed against the pinned compiler):
//! - **`declaration_duplicate`** — an INSTANCE import local colliding with a non-`var`
//!   MODULE-slot binding: the binder (`phases/scope.js` `declare`) HOISTS an `import`
//!   declaration to the parent scope, so the instance import declares INTO the module
//!   scope, where the scope-creation duplicate check fires ("`x` has already been
//!   declared"). A prior `var` is exempt (the binder's check requires both sides
//!   non-`var`), so a module `var x` + an instance `import { x }` is ACCEPTED.
//! - **`declaration_duplicate_module_import`** — an INSTANCE top-level variable
//!   declarator re-declaring a MODULE-slot IMPORT local: official's
//!   `ensure_no_module_import_conflict` (fired per `VariableDeclarator` in the analyze
//!   walk; export-wrapped declarators included). Only variable declarators
//!   participate — an instance `function` / `class` over a module import SHADOWS and
//!   is ACCEPTED, as is any instance value declaration over a module lexical binding.
//!
//! Both scans are JS-reparse-driven (the same OXC module reparse as the gate's other
//! analyze scans): a TS-only body (`import type …`, TS syntax) skips them, keeping
//! type-namespace names out of the value-collision domain — a `lang="ts"` component
//! stays owned by the TypeScript-script refusal downstream.

use oxc_ast::ast::{
    Declaration, ExportDefaultDeclarationKind, ImportDeclarationSpecifier, ImportOrExportKind,
    Program, Statement, VariableDeclaration,
};

use super::expr::collect_pattern_names;
use super::official_rule::{CoreOfficialValidationRule, OfficialRejection};

/// How a module-slot top-level name binds — the discriminant the two collision
/// families consult (an instance import collides with any non-[`Var`] binding; an
/// instance variable declarator collides with an [`Import`] binding only).
///
/// [`Var`]: ModuleBindingKind::Var
/// [`Import`]: ModuleBindingKind::Import
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModuleBindingKind {
    /// A VALUE import specifier local (default / named-`as` / namespace).
    Import,
    /// A lexical module-scope binding: `let` / `const`, a bodied `function`, a
    /// non-ambient `class` — including the `export`-wrapped and named
    /// `export default` forms (the name binds in the module scope either way).
    Lexical,
    /// A `var` declarator — exempt from the binder's duplicate check on the prior
    /// side (`var x` + a later instance import of `x` is official-ACCEPTED).
    Var,
}

/// The retained MODULE-slot top-level binding inventory — collected once per gate run
/// from the module script's JS reparse and consulted by both cross-script scans. Empty
/// when there is no module script or its body is not plain-JS-parseable (a TS-only
/// body — the cross-script families are value-domain only).
#[derive(Default)]
pub(super) struct ModuleSlotBindings {
    /// `(name, kind)` per top-level module-slot binding, in source order.
    names: Vec<(String, ModuleBindingKind)>,
}

impl ModuleSlotBindings {
    /// Collect the module script's top-level binding inventory. `None` (no module
    /// script) or an unparseable body yields the EMPTY inventory — the scans then
    /// degrade to their single-script behavior.
    pub(super) fn collect(module_source: Option<&str>) -> Self {
        let Some(src) = module_source else {
            return Self::default();
        };
        let alloc = oxc_allocator::Allocator::default();
        let Some(program) = super::expr::reparse_module(&alloc, src) else {
            return Self::default();
        };
        let mut names = Vec::new();
        for stmt in &program.body {
            match stmt {
                Statement::ImportDeclaration(import) => {
                    collect_import_locals(import, &mut names);
                }
                Statement::VariableDeclaration(decl) => {
                    collect_variable_names(decl, &mut names);
                }
                Statement::FunctionDeclaration(func) => {
                    // A bodiless function is TS overload/ambient surface — no binding.
                    if func.body.is_none() {
                        continue;
                    }
                    if let Some(id) = &func.id {
                        names.push((id.name.to_string(), ModuleBindingKind::Lexical));
                    }
                }
                Statement::ClassDeclaration(class) => {
                    // A `declare class` is TS ambient surface — no binding.
                    if class.declare {
                        continue;
                    }
                    if let Some(id) = &class.id {
                        names.push((id.name.to_string(), ModuleBindingKind::Lexical));
                    }
                }
                // `export`-wrapped declarations still bind their names in the module
                // scope (oracle-probed: `export const z` + an instance `import { z }`
                // is `declaration_duplicate`; `export var v` stays `var`-exempt).
                Statement::ExportNamedDeclaration(export) => {
                    if !matches!(export.export_kind, ImportOrExportKind::Value) {
                        continue;
                    }
                    match &export.declaration {
                        Some(Declaration::VariableDeclaration(decl)) => {
                            collect_variable_names(decl, &mut names);
                        }
                        Some(Declaration::FunctionDeclaration(func)) => {
                            if func.body.is_none() {
                                continue;
                            }
                            if let Some(id) = &func.id {
                                names.push((id.name.to_string(), ModuleBindingKind::Lexical));
                            }
                        }
                        Some(Declaration::ClassDeclaration(class)) => {
                            if class.declare {
                                continue;
                            }
                            if let Some(id) = &class.id {
                                names.push((id.name.to_string(), ModuleBindingKind::Lexical));
                            }
                        }
                        // A specifier-only `export { a }` / `export … from` re-export
                        // binds no NEW local; TS-only inner declarations bind no value.
                        _ => {}
                    }
                }
                // A NAMED `export default function x() {}` / `class x {}` still binds
                // `x` in the module scope BEFORE official's illegal-default-export
                // analyze check — the scope-creation collision fires first
                // (oracle-probed: + an instance `import { x }` is
                // `declaration_duplicate`).
                Statement::ExportDefaultDeclaration(export) => match &export.declaration {
                    ExportDefaultDeclarationKind::FunctionDeclaration(func) => {
                        if func.body.is_none() {
                            continue;
                        }
                        if let Some(id) = &func.id {
                            names.push((id.name.to_string(), ModuleBindingKind::Lexical));
                        }
                    }
                    ExportDefaultDeclarationKind::ClassDeclaration(class) => {
                        if class.declare {
                            continue;
                        }
                        if let Some(id) = &class.id {
                            names.push((id.name.to_string(), ModuleBindingKind::Lexical));
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        Self { names }
    }

    /// Whether an INSTANCE import of `name` collides — any module-slot binding of the
    /// same name whose kind is non-`var` (the binder's duplicate check exempts a prior
    /// `var`; the import side is always non-`var`).
    fn collides_with_instance_import(&self, name: &str) -> bool {
        self.names
            .iter()
            .any(|(n, kind)| n == name && *kind != ModuleBindingKind::Var)
    }

    /// Whether the module slot binds `name` as a VALUE import local — the
    /// `declaration_duplicate_module_import` family's subject.
    fn has_import_local(&self, name: &str) -> bool {
        self.names
            .iter()
            .any(|(n, kind)| n == name && *kind == ModuleBindingKind::Import)
    }
}

/// Push a `var`/`let`/`const` declaration's pattern names with their kind. A TS
/// `declare` statement binds no runtime value.
fn collect_variable_names(
    decl: &VariableDeclaration,
    names: &mut Vec<(String, ModuleBindingKind)>,
) {
    use oxc_ast::ast::VariableDeclarationKind;
    if decl.declare {
        return;
    }
    let kind = if matches!(decl.kind, VariableDeclarationKind::Var) {
        ModuleBindingKind::Var
    } else {
        ModuleBindingKind::Lexical
    };
    for d in &decl.declarations {
        let mut pattern_names = Vec::new();
        collect_pattern_names(&d.id, &mut pattern_names);
        for name in pattern_names {
            names.push((name, kind));
        }
    }
}

/// Push a VALUE import declaration's specifier locals. Type-only imports (decl-level
/// or per-specifier) bind no runtime value.
fn collect_import_locals(
    import: &oxc_ast::ast::ImportDeclaration,
    names: &mut Vec<(String, ModuleBindingKind)>,
) {
    if !matches!(import.import_kind, ImportOrExportKind::Value) {
        return;
    }
    let Some(specifiers) = &import.specifiers else {
        return;
    };
    for spec in specifiers {
        let local = match spec {
            ImportDeclarationSpecifier::ImportSpecifier(s) => {
                if matches!(s.import_kind, ImportOrExportKind::Type) {
                    continue;
                }
                &s.local
            }
            ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => &s.local,
            ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => &s.local,
        };
        names.push((local.name.to_string(), ModuleBindingKind::Import));
    }
}

/// The SCOPE-CREATION scan for one script — the source-order interleave of the
/// binder's duplicate check and the `$`-prefix name validation, mirroring official
/// `create_scopes`: each `declare()` runs its duplicate check BEFORE
/// `validate_identifier_name`, statement by statement (and import specifier by
/// specifier) in source order.
///
/// For the MODULE script the collision inventory is EMPTY (its same-body duplicates
/// are the parse-phase body probe's; a module script cannot cross-collide with
/// itself), so the pass reduces to the `$`-prefix validation. For the INSTANCE script
/// each VALUE import local first checks the module-slot inventory (the import-hoist
/// `declaration_duplicate`), then its `$` prefix — official's binder order,
/// oracle-probed both ways.
pub(super) fn scan_script_scope_creation(
    script_source: &str,
    module_bindings: &ModuleSlotBindings,
) -> Option<OfficialRejection> {
    let alloc = oxc_allocator::Allocator::default();
    let program = super::expr::reparse_module(&alloc, script_source)?;

    for stmt in &program.body {
        match stmt {
            Statement::VariableDeclaration(decl) => {
                for d in &decl.declarations {
                    let mut names = Vec::new();
                    collect_pattern_names(&d.id, &mut names);
                    if names.iter().any(|n| n.starts_with('$')) {
                        return Some(OfficialRejection::of(
                            CoreOfficialValidationRule::DollarPrefixInvalid,
                        ));
                    }
                }
            }
            Statement::ImportDeclaration(import) => {
                if !matches!(import.import_kind, ImportOrExportKind::Value) {
                    continue;
                }
                let Some(specifiers) = &import.specifiers else {
                    continue;
                };
                for spec in specifiers {
                    let local = match spec {
                        ImportDeclarationSpecifier::ImportSpecifier(s) => {
                            if matches!(s.import_kind, ImportOrExportKind::Type) {
                                continue; // `import { type Foo as $x }` — type-only
                            }
                            &s.local
                        }
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => &s.local,
                        ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => &s.local,
                    };
                    // The binder's duplicate check runs FIRST: the import hoists into
                    // the module scope, where a same-name non-`var` binding is the
                    // scope-creation `declaration_duplicate`.
                    if module_bindings.collides_with_instance_import(local.name.as_str()) {
                        return Some(OfficialRejection::with_code(
                            CoreOfficialValidationRule::DeclarationDuplicate,
                            "declaration_duplicate",
                        ));
                    }
                    if local.name.starts_with('$') {
                        return Some(OfficialRejection::of(
                            CoreOfficialValidationRule::DollarPrefixInvalid,
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// The earliest INSTANCE top-level variable declarator whose pattern re-declares a
/// MODULE-slot IMPORT local — official's analyze-walk
/// `ensure_no_module_import_conflict` (`declaration_duplicate_module_import`), fired
/// per `VariableDeclarator` in walk order. Export-wrapped declarators count
/// (`export let x` is still an instance-scope declarator, oracle-probed); an instance
/// `function` / `class` does NOT (it shadows). Returns the conflicting declarator's
/// span START for walk-order arbitration against the gate's other walk-phase scan
/// (a misplaced `$inspect.trace`).
pub(super) fn first_instance_module_import_conflict(
    instance_source: &str,
    module_bindings: &ModuleSlotBindings,
) -> Option<u32> {
    let alloc = oxc_allocator::Allocator::default();
    let program = super::expr::reparse_module(&alloc, instance_source)?;
    first_conflicting_declarator(&program, module_bindings)
}

/// Walk the parsed instance program's top-level (and export-wrapped) variable
/// declarators in source order; return the first one re-declaring a module IMPORT
/// local.
fn first_conflicting_declarator(
    program: &Program,
    module_bindings: &ModuleSlotBindings,
) -> Option<u32> {
    for stmt in &program.body {
        let decl = match stmt {
            Statement::VariableDeclaration(decl) => decl,
            Statement::ExportNamedDeclaration(export) => match &export.declaration {
                Some(Declaration::VariableDeclaration(decl)) => decl,
                _ => continue,
            },
            _ => continue,
        };
        if decl.declare {
            continue; // a TS ambient declaration binds no runtime value
        }
        for d in &decl.declarations {
            let mut names = Vec::new();
            collect_pattern_names(&d.id, &mut names);
            if names
                .iter()
                .any(|name| module_bindings.has_import_local(name))
            {
                return Some(d.span.start);
            }
        }
    }
    None
}
