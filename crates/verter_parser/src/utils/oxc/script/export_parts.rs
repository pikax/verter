//! One view of a named export statement.
//!
//! oxc 0.151 splits what was one `ExportNamedDeclaration` into three
//! statements: `export <declaration>` ([`ExportDeclaration`]),
//! `export { … }` ([`ExportNamedDeclaration`]) and `export { … } from "m"`
//! ([`ExportFromDeclaration`]). [`NamedExportParts`] reads any of the three
//! as the parts they share, for a consumer that treats them as one form.

use oxc_ast::ast::{
    Declaration, ExportDeclaration, ExportFromDeclaration, ExportNamedDeclaration, ExportSpecifier,
    ImportOrExportKind, Statement, StringLiteral,
};
use oxc_span::Span;

/// The parts of a named export statement: at most one of a declaration and
/// specifiers, and a source only for a re-export.
#[derive(Clone, Copy)]
pub struct NamedExportParts<'b, 'a> {
    /// The whole statement's span.
    pub span: Span,
    /// `export <declaration>`'s declaration.
    pub declaration: Option<&'b Declaration<'a>>,
    /// `export { … }`'s specifiers (empty for a declaration export).
    pub specifiers: &'b [ExportSpecifier<'a>],
    /// `export { … } from "m"`'s source.
    pub source: Option<&'b StringLiteral<'a>>,
    /// `export type`: written for specifiers, derived from the declaration
    /// (a type or `declare` declaration) for a declaration export.
    pub export_kind: ImportOrExportKind,
}

impl<'b, 'a> NamedExportParts<'b, 'a> {
    /// The parts of `statement`, when it is a named export.
    #[must_use]
    pub fn of_statement(statement: &'b Statement<'a>) -> Option<Self> {
        match statement {
            Statement::ExportDeclaration(export) => Some(Self::of_declaration(export)),
            Statement::ExportNamedDeclaration(export) => Some(Self::of_named(export)),
            Statement::ExportFromDeclaration(export) => Some(Self::of_from(export)),
            _ => None,
        }
    }

    /// The parts of `export <declaration>`.
    #[must_use]
    pub fn of_declaration(export: &'b ExportDeclaration<'a>) -> Self {
        Self {
            span: export.span,
            declaration: Some(&export.declaration),
            specifiers: &[],
            source: None,
            export_kind: export.export_kind(),
        }
    }

    /// The parts of `export { … }`.
    #[must_use]
    pub fn of_named(export: &'b ExportNamedDeclaration<'a>) -> Self {
        Self {
            span: export.span,
            declaration: None,
            specifiers: &export.specifiers,
            source: None,
            export_kind: export.export_kind,
        }
    }

    /// The parts of `export { … } from "m"`.
    #[must_use]
    pub fn of_from(export: &'b ExportFromDeclaration<'a>) -> Self {
        Self {
            span: export.span,
            declaration: None,
            specifiers: &export.specifiers,
            source: Some(&export.source),
            export_kind: export.export_kind,
        }
    }
}
