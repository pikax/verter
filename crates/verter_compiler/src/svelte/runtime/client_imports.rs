//! The typed USER-import carrier vocabulary — the module-scope static-import prelude
//! the client module hoists above the component function.
//!
//! `UserImport` is the SHARED carrier for every admitted top-level static `import`
//! declaration: default (component and plain value), named (with aliases and
//! string-literal module-export names), namespace, side-effect, and mixed forms, in
//! BOTH the instance `<script>` and `<script module>` slots, with
//! `with { … }` import attributes preserved. Classification (the OXC walk building
//! these carriers) lives in [`super::client_surface_imports`]; the two-slot module
//! emission lives in [`super::client_module_frame`]. Extracted from
//! `client_plan_types.rs` to keep that file under the file-size guard.

use verter_span::Span;

use super::client_codegen_helpers::js_single_quoted;

/// A typed top-level static USER import the client module hoists to module scope —
/// the general prelude/import carrier (`ClientModulePlan.user_imports`).
///
/// One carrier per source `import` DECLARATION (duplicates from the same source stay
/// separate carriers — official does not merge them), in source order within its
/// slot. The emission slots mirror official `svelte@5.56.3`: `Module` imports emit
/// BEFORE the runtime namespace (`import * as $`), `Instance` imports AFTER it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct UserImport {
    /// The owning script slot (`<script module>` vs the instance `<script>`) — the
    /// two-slot emission discriminant.
    pub(super) slot: UserImportSlot,
    /// The module specifier string (`'./m.js'`), emitted through the JS single-quote
    /// serializer.
    pub(super) source: String,
    /// The typed specifiers, in source order. EMPTY means a side-effect import
    /// (`import './setup.js'`).
    pub(super) specifiers: Vec<UserImportSpecifier>,
    /// The `with { … }` import attributes, in source order (preserved on emission).
    /// The deprecated `assert { … }` keyword is fail-closed at the classifier and
    /// never reaches this carrier.
    pub(super) attributes: Vec<UserImportAttribute>,
    /// The import declaration's source span (script-relative).
    pub(super) span: Span,
}

/// The script slot an admitted user import was declared in — drives the two-slot
/// module emission (module imports before `import * as $`, instance imports after).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum UserImportSlot {
    /// A `<script module>` top-level import.
    Module,
    /// An instance `<script>` top-level import.
    Instance,
}

/// One typed import specifier of a [`UserImport`] declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum UserImportSpecifier {
    /// `import local from '…'` — the default binding.
    Default {
        /// The imported local binding name.
        local: String,
    },
    /// `import { imported as local } from '…'` (shorthand when `imported == local`),
    /// including a string-literal module-export name (`import { "a-b" as local }`).
    Named {
        /// The module-export NAME being imported (an identifier or a string literal).
        imported: ImportName,
        /// The local binding name.
        local: String,
    },
    /// `import * as local from '…'` — the namespace object binding.
    Namespace {
        /// The local namespace binding name.
        local: String,
    },
}

/// A named-import module-export NAME — an identifier (`{ x }` / `{ a as b }`) or a
/// string literal (`{ "a-b" as c }`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ImportName {
    /// An identifier export name.
    Ident(String),
    /// A string-literal export name (always aliased to an identifier local).
    StringLiteral(String),
}

/// One `with { key: 'value' }` import attribute (preserved verbatim on emission).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct UserImportAttribute {
    /// The attribute key (an identifier or a string literal).
    pub(super) key: ImportAttributeKey,
    /// The attribute's string value.
    pub(super) value: String,
}

/// A `with { … }` import-attribute KEY — an identifier (`type`) or a string literal
/// (`"type"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ImportAttributeKey {
    /// An identifier key.
    Ident(String),
    /// A string-literal key.
    StringLiteral(String),
}

impl UserImport {
    /// Whether this import declares NO bindings (`import './setup.js'` — evaluated
    /// for its side effects only).
    pub(super) fn is_side_effect(&self) -> bool {
        self.specifiers.is_empty()
    }

    /// Render the emitted `import …;` statement text (no trailing newline). Every
    /// string payload (the specifier, string-literal export names, attribute keys /
    /// values) routes through the JS single-quote serializer so a quote / backslash
    /// stays one quote-safe, parseable string literal.
    pub(super) fn render_statement(&self) -> String {
        let mut out = String::from("import ");
        if self.is_side_effect() {
            out.push_str(&js_single_quoted(&self.source));
        } else {
            // The default specifier leads; a namespace / named group follows after a
            // comma (`import D, { n } from …` / `import D, * as NS from …`) — valid
            // source order guarantees at most one default, and a namespace never
            // co-occurs with a named group.
            let mut parts: Vec<String> = Vec::new();
            let mut named: Vec<String> = Vec::new();
            for spec in &self.specifiers {
                match spec {
                    UserImportSpecifier::Default { local } => parts.push(local.clone()),
                    UserImportSpecifier::Namespace { local } => {
                        parts.push(format!("* as {local}"));
                    }
                    UserImportSpecifier::Named { imported, local } => {
                        named.push(match imported {
                            ImportName::Ident(name) if name == local => local.clone(),
                            ImportName::Ident(name) => format!("{name} as {local}"),
                            ImportName::StringLiteral(name) => {
                                format!("{} as {local}", js_single_quoted(name))
                            }
                        });
                    }
                }
            }
            if !named.is_empty() {
                parts.push(format!("{{ {} }}", named.join(", ")));
            }
            out.push_str(&parts.join(", "));
            out.push_str(" from ");
            out.push_str(&js_single_quoted(&self.source));
        }
        if !self.attributes.is_empty() {
            let entries = self
                .attributes
                .iter()
                .map(|attr| {
                    let key = match &attr.key {
                        ImportAttributeKey::Ident(name) => name.clone(),
                        ImportAttributeKey::StringLiteral(name) => js_single_quoted(name),
                    };
                    format!("{key}: {}", js_single_quoted(&attr.value))
                })
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!(" with {{ {entries} }}"));
        }
        out.push(';');
        out
    }
}
