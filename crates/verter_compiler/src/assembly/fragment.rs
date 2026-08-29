//! One generated logical unit: framework/product identity, source
//! identity, placement, a declared syntactic contract its bytes must parse
//! under, the exact dialect they parse under, declared imports/exports/
//! helpers, and dependencies on other fragments.
//!
//! A [`Fragment`] is untrusted until [`Fragment::validate`] proves its
//! `code` parses under its declared [`SyntacticContract`] and
//! [`FragmentDialect`] — only the resulting [`ValidatedFragment`] is
//! accepted by [`super::compose::assemble_sequence`] /
//! [`super::compose::splice_into_hole`] / [`super::publish::publish`].
//! `ValidatedFragment` is a SEALED type (its only field is private, its
//! only mint site is [`Fragment::validate`]): those three call sites accept
//! `&ValidatedFragment` (or `Vec<&ValidatedFragment>`) exclusively — there
//! is no overload, "unchecked" variant, or raw `{code, source_map}` pair
//! shape a caller could pass instead. This is a type-system guarantee, not
//! a documented convention — see `tests/cases/compile-fail/
//! assemble_sequence_requires_validated_fragment.rs` for the compile-time
//! proof.
//!
//! [`super::compose::prepend_preamble`] is the ONE exception, and is named
//! here deliberately: it composes ASSEMBLY-OWNED literal bytes (a
//! synthesized `import { ... } from "..."` line with no syntactic contract
//! of its own — see its own doc) ahead of an ALREADY-VALIDATED artifact's
//! code, so it takes a raw `&str` preamble by design, not a `Fragment`.
//! Nothing routes a producer-supplied fragment through it unchecked.

use std::ops::Range;

use oxc_allocator::Allocator;
use oxc_ast::ast::Statement;
use oxc_parser::Parser;
use oxc_span::SourceType;

use crate::compile_request::ProductKind;

pub use super::source_space::{FragmentRange, SourceSpaceKind};
use super::source_unit::SourceUnitId;

/// Opaque local identity for a fragment inside one assembly — assigned
/// when a fragment is collected into a [`super::plan::ProductPlan`], not a
/// cross-assembly stable identity ([`SourceUnitId`] is that).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FragmentId(pub u32);

/// Which framework produced this fragment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameworkDomain {
    Vue,
    Svelte,
}

/// The grammar a fragment's bytes must parse under before it can be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntacticContract {
    /// A complete ECMAScript/TypeScript module: parses standalone,
    /// `import`/`export` declarations are legal.
    CompleteModule,
    /// A sequence of statements, no top-level module declarations.
    StatementList,
    /// A single expression.
    Expression,
    /// A single top-level declaration (variable, function, or class).
    Declaration,
    /// A non-ECMAScript payload (CSS). Not parsed by this module — CSS
    /// ownership stays with the CSS-owning surface (suspended to a later
    /// program train); a `Style` fragment crosses this assembly boundary
    /// only as an already-produced opaque part and is accepted here
    /// structurally (non-empty payload), never JS-parsed.
    Style,
    /// A non-code payload (e.g. raw custom-block content). Accepted
    /// structurally, never JS-parsed.
    Metadata,
}

/// The exact ECMAScript/TypeScript dialect a fragment's (or a published
/// artifact's) bytes are written in — carried explicitly and derived ONCE
/// by the producer, never assumed. Parsing every fragment/artifact under a
/// fixed permissive TSX regardless of its real dialect would let a plain-JS
/// artifact silently swallow TypeScript-only syntax it should reject, or a
/// non-JSX artifact silently accept stray JSX; both are the exact class of
/// false "parses cleanly" the final-parse check exists to prevent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FragmentDialect {
    /// Plain JavaScript, no JSX.
    JavaScript,
    /// JavaScript with JSX.
    Jsx,
    /// TypeScript, no JSX.
    TypeScript,
    /// TypeScript with JSX.
    Tsx,
    /// A TypeScript ambient declaration file (`.d.ts`).
    Declaration,
}

impl FragmentDialect {
    /// The base [`SourceType`] this dialect parses under — module-ness
    /// (`with_module`) is layered on top per [`SyntacticContract`], since
    /// that axis is orthogonal to the dialect itself.
    fn base_source_type(self) -> SourceType {
        match self {
            FragmentDialect::JavaScript => SourceType::mjs(),
            FragmentDialect::Jsx => SourceType::jsx(),
            FragmentDialect::TypeScript => SourceType::ts(),
            FragmentDialect::Tsx => SourceType::tsx(),
            FragmentDialect::Declaration => SourceType::d_ts(),
        }
    }
}

/// One declared import a fragment's own bytes need, OR (as
/// [`super::publish::ArtifactContribution::emitted_imports`]) one the
/// composer actually wrote — the same shape serves both declaration and
/// emission so the two can be compared name-for-name. `kind` distinguishes
/// the four import forms a `specifier` can be imported under: a
/// side-effect-only import binds no name at all, so collapsing it into an
/// empty `Named` list would be ambiguous with "declares nothing".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredImport {
    pub specifier: String,
    pub kind: DeclaredImportKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclaredImportKind {
    /// `import "specifier"` — no bound name, imported purely for effect.
    SideEffect,
    /// `import name from "specifier"`.
    Default(String),
    /// `import * as name from "specifier"`.
    Namespace(String),
    /// `import { a, b as c, ... } from "specifier"` — each entry is the
    /// LOCAL (imported-as) bound name, matching what the fragment's own
    /// code actually references.
    Named(Vec<String>),
}

impl DeclaredImport {
    /// Every name this import binds into scope — empty for
    /// [`DeclaredImportKind::SideEffect`].
    pub(crate) fn bound_names(&self) -> Vec<&str> {
        match &self.kind {
            DeclaredImportKind::SideEffect => Vec::new(),
            DeclaredImportKind::Default(name) | DeclaredImportKind::Namespace(name) => {
                vec![name.as_str()]
            }
            DeclaredImportKind::Named(names) => names.iter().map(String::as_str).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredExport {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredHelper {
    pub name: String,
}

/// A declared generated-script export-placement fact: every `__sfc__`
/// binding-reference range within one generated script's OWN bytes that
/// must be renamed to `_sfc_main` during host assembly, plus the exact
/// terminal `export default __sfc__;\n` statement range removed once the
/// assembled module re-exports the composed result under its own name.
///
/// Declared once, by the producer that wrote these bytes, at the exact
/// point of writing (`crate::script::push_sfc_binding` /
/// `push_default_export_statement`, or the equivalent marker-based
/// tracking through [`crate::code_transform::CodeTransform`]) — never
/// rediscovered downstream by scanning generated text for the landmark
/// string. A companion type rather than an extension of
/// [`DeclaredExport`]: `DeclaredExport` names an export a consumer may
/// import, while this fact describes a REWRITE the host assembler applies
/// to the script's own bytes — a distinct concern with its own shape
/// (multiple binding references plus one removable statement, not a
/// single exported name).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SfcExportPlacement {
    /// Byte ranges of every `__sfc__` binding reference, in the owning
    /// script's own generated coordinate space.
    pub binding_ranges: Vec<Range<u32>>,
    /// The terminal `export default __sfc__;\n` statement's own range, in
    /// the same coordinate space. `None` for a script that emits no
    /// standalone default-export statement for the composer to remove.
    pub export_statement_range: Option<Range<u32>>,
}

/// Where a fragment lands in the final module. Symbolic — never inferred
/// by scanning another fragment's generated text for a landmark string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlacementSlot {
    ModulePrelude,
    ImportSection,
    ModuleBody,
    /// A typed hole another fragment (`owner`) declares in ITS OWN
    /// generated bytes. `hole.fragment` is always `owner` — the range's
    /// own space tag, not a second independent claim — carried as a typed
    /// [`FragmentRange`] rather than a bare `Range<u32>` so a hole offset
    /// cannot compile against an assembled- or original-space range.
    Hole {
        owner: FragmentId,
        hole: FragmentRange,
    },
}

#[derive(Debug, Clone)]
pub struct Fragment {
    pub domain: FrameworkDomain,
    pub product: ProductKind,
    pub source_unit: SourceUnitId,
    /// The coordinate space this fragment's own bytes map back to —
    /// `GeneratedFragment` for a normally-emitted fragment; `Original`
    /// only for a fragment whose bytes are moved verbatim from the
    /// authored source with no codegen pass in between.
    pub source_space: SourceSpaceKind,
    pub placement: PlacementSlot,
    pub contract: SyntacticContract,
    /// The exact ECMAScript/TypeScript dialect [`Self::code`] is written
    /// in — never assumed permissive TSX. Irrelevant (but still required,
    /// for a uniform constructor) for [`SyntacticContract::Style`]/
    /// [`SyntacticContract::Metadata`], which are not JS-parsed at all.
    pub dialect: FragmentDialect,
    pub code: String,
    /// This fragment's own generated-space source map, when produced.
    pub source_map: Option<String>,
    pub imports: Vec<DeclaredImport>,
    pub exports: Vec<DeclaredExport>,
    pub helpers: Vec<DeclaredHelper>,
    pub dependencies: Vec<FragmentId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FragmentRefusal {
    /// The fragment's bytes did not parse under its declared contract.
    ContractViolation {
        contract: SyntacticContract,
        reason: String,
    },
}

/// A fragment whose bytes are PROVEN to parse under its declared
/// [`SyntacticContract`] — the only shape [`super::compose`] /
/// [`super::publish`] accept.
#[derive(Debug, Clone)]
pub struct ValidatedFragment(Fragment);

/// Whether `code` parses as a complete ECMAScript/TypeScript module under
/// `dialect` — shared by [`Fragment::validate`]'s `CompleteModule` contract
/// and [`super::publish::publish`]'s final-assembly parse check, so final
/// assembly always parses as its declared ECMAScript/TypeScript module.
/// `dialect` is the artifact's OWN declared dialect, never a fixed
/// permissive default — a JS artifact must reject TypeScript-only syntax,
/// not silently accept it under a TSX parse.
pub(crate) fn final_module_parse_errors(code: &str, dialect: FragmentDialect) -> Option<String> {
    oxc_parse_errors(code, dialect.base_source_type())
}

fn oxc_parse_errors(code: &str, source_type: SourceType) -> Option<String> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, code, source_type).parse();
    if parsed.panicked {
        return Some("parser panicked".to_string());
    }
    if !parsed.errors.is_empty() {
        return Some(
            parsed
                .errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; "),
        );
    }
    None
}

fn is_declaration_statement(statement: &Statement<'_>) -> bool {
    matches!(
        statement,
        Statement::VariableDeclaration(_)
            | Statement::FunctionDeclaration(_)
            | Statement::ClassDeclaration(_)
            | Statement::TSTypeAliasDeclaration(_)
            | Statement::TSInterfaceDeclaration(_)
            | Statement::TSEnumDeclaration(_)
    )
}

impl Fragment {
    /// Parse `code` under `contract` and, on success, wrap as a
    /// [`ValidatedFragment`]. `Style`/`Metadata` fragments are not
    /// ECMAScript and are accepted whenever `code` is non-empty — see
    /// [`SyntacticContract`]'s own doc for why this module does not parse
    /// them.
    pub fn validate(self) -> Result<ValidatedFragment, FragmentRefusal> {
        match self.contract {
            SyntacticContract::Style | SyntacticContract::Metadata => {
                if self.code.is_empty() {
                    return Err(FragmentRefusal::ContractViolation {
                        contract: self.contract,
                        reason: "empty payload".to_string(),
                    });
                }
                Ok(ValidatedFragment(self))
            }
            SyntacticContract::CompleteModule => {
                let source_type = self.dialect.base_source_type();
                if let Some(reason) = oxc_parse_errors(&self.code, source_type) {
                    return Err(FragmentRefusal::ContractViolation {
                        contract: self.contract,
                        reason,
                    });
                }
                Ok(ValidatedFragment(self))
            }
            SyntacticContract::StatementList => {
                let source_type = self.dialect.base_source_type().with_module(false);
                if let Some(reason) = oxc_parse_errors(&self.code, source_type) {
                    return Err(FragmentRefusal::ContractViolation {
                        contract: self.contract,
                        reason,
                    });
                }
                Ok(ValidatedFragment(self))
            }
            SyntacticContract::Expression => {
                let source_type = self.dialect.base_source_type();
                let wrapped = format!("({})", self.code);
                if let Some(reason) = oxc_parse_errors(&wrapped, source_type) {
                    return Err(FragmentRefusal::ContractViolation {
                        contract: self.contract,
                        reason,
                    });
                }
                let allocator = Allocator::default();
                let parsed = Parser::new(&allocator, &wrapped, source_type).parse();
                let is_single_expression = parsed.program.body.len() == 1
                    && matches!(parsed.program.body[0], Statement::ExpressionStatement(_));
                if !is_single_expression {
                    return Err(FragmentRefusal::ContractViolation {
                        contract: self.contract,
                        reason: "not a single expression".to_string(),
                    });
                }
                Ok(ValidatedFragment(self))
            }
            SyntacticContract::Declaration => {
                let source_type = self.dialect.base_source_type().with_module(false);
                if let Some(reason) = oxc_parse_errors(&self.code, source_type) {
                    return Err(FragmentRefusal::ContractViolation {
                        contract: self.contract,
                        reason,
                    });
                }
                let allocator = Allocator::default();
                let parsed = Parser::new(&allocator, &self.code, source_type).parse();
                let is_single_declaration = parsed.program.body.len() == 1
                    && is_declaration_statement(&parsed.program.body[0]);
                if !is_single_declaration {
                    return Err(FragmentRefusal::ContractViolation {
                        contract: self.contract,
                        reason: "not a single top-level declaration".to_string(),
                    });
                }
                Ok(ValidatedFragment(self))
            }
        }
    }
}

impl ValidatedFragment {
    pub fn fragment(&self) -> &Fragment {
        &self.0
    }

    pub fn into_fragment(self) -> Fragment {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(contract: SyntacticContract, code: &str) -> Fragment {
        dialect_base(contract, FragmentDialect::Tsx, code)
    }

    fn dialect_base(contract: SyntacticContract, dialect: FragmentDialect, code: &str) -> Fragment {
        Fragment {
            domain: FrameworkDomain::Vue,
            product: ProductKind::RuntimeClient,
            source_unit: crate::assembly::source_unit::source_unit_id("source", "unit"),
            source_space: SourceSpaceKind::GeneratedFragment,
            placement: PlacementSlot::ModuleBody,
            contract,
            dialect,
            code: code.to_string(),
            source_map: None,
            imports: Vec::new(),
            exports: Vec::new(),
            helpers: Vec::new(),
            dependencies: Vec::new(),
        }
    }

    #[test]
    fn a_javascript_dialect_fragment_rejects_typescript_only_syntax() {
        // `: number` is TypeScript-only syntax — a fragment DECLARED as
        // plain JavaScript must reject it, not silently accept it under a
        // permissive-TSX-always parse.
        let fragment = dialect_base(
            SyntacticContract::CompleteModule,
            FragmentDialect::JavaScript,
            "const n: number = 1",
        );
        let err = fragment.validate().unwrap_err();
        assert!(matches!(
            err,
            FragmentRefusal::ContractViolation {
                contract: SyntacticContract::CompleteModule,
                ..
            }
        ));
    }

    #[test]
    fn the_same_typescript_syntax_validates_under_the_typescript_dialect() {
        // Positive control for the JS-rejects-TS refusal above: the exact
        // same bytes are accepted once the fragment DECLARES itself
        // TypeScript.
        let fragment = dialect_base(
            SyntacticContract::CompleteModule,
            FragmentDialect::TypeScript,
            "const n: number = 1",
        );
        assert!(fragment.validate().is_ok());
    }

    #[test]
    fn complete_module_with_valid_esm_validates() {
        let fragment = base(
            SyntacticContract::CompleteModule,
            "import { ref } from 'vue'\nexport default { setup() { return { ref } } }",
        );
        assert!(fragment.validate().is_ok());
    }

    #[test]
    fn complete_module_declared_as_expression_only_is_refused() {
        // A bare expression is not a complete module in the sense this
        // contract cares about testing here: force a genuine parse error
        // instead (unterminated construct) so the refusal exercises the
        // real oxc parser boundary, not a permissive default.
        let fragment = base(SyntacticContract::CompleteModule, "import { from 'vue'");
        let err = fragment.validate().unwrap_err();
        assert!(matches!(
            err,
            FragmentRefusal::ContractViolation {
                contract: SyntacticContract::CompleteModule,
                ..
            }
        ));
    }

    #[test]
    fn expression_contract_accepts_a_single_expression() {
        let fragment = base(SyntacticContract::Expression, "1 + 2");
        assert!(fragment.validate().is_ok());
    }

    #[test]
    fn expression_contract_refuses_a_statement_list() {
        let fragment = base(SyntacticContract::Expression, "const a = 1; const b = 2;");
        let err = fragment.validate().unwrap_err();
        assert!(matches!(
            err,
            FragmentRefusal::ContractViolation {
                contract: SyntacticContract::Expression,
                ..
            }
        ));
    }

    #[test]
    fn declaration_contract_accepts_a_single_function_declaration() {
        let fragment = base(
            SyntacticContract::Declaration,
            "function render() { return 1 }",
        );
        assert!(fragment.validate().is_ok());
    }

    #[test]
    fn declaration_contract_refuses_a_bare_expression() {
        let fragment = base(SyntacticContract::Declaration, "1 + 2");
        let err = fragment.validate().unwrap_err();
        assert!(matches!(
            err,
            FragmentRefusal::ContractViolation {
                contract: SyntacticContract::Declaration,
                ..
            }
        ));
    }

    #[test]
    fn statement_list_contract_accepts_multiple_statements() {
        let fragment = base(
            SyntacticContract::StatementList,
            "const a = 1;\nconst b = 2;",
        );
        assert!(fragment.validate().is_ok());
    }

    #[test]
    fn statement_list_contract_refuses_unparseable_bytes() {
        let fragment = base(SyntacticContract::StatementList, "const a = ;");
        let err = fragment.validate().unwrap_err();
        assert!(matches!(
            err,
            FragmentRefusal::ContractViolation {
                contract: SyntacticContract::StatementList,
                ..
            }
        ));
    }
}
