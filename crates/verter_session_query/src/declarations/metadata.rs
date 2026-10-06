//! Owned declaration-resolution answers shared by the semantic engine and
//! its host: where a named type declaration resolved to, and the exact
//! identity of a top-level runtime value declaration. The algorithms that
//! produce them belong to the host.

use crate::declarations::DeclarationId;

/// The final target an export resolved to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedExportTarget {
    pub source_canonical_id: Option<String>,
    pub source_owner: verter_type_expr::TopLevelOwnerId,
    pub source_name: String,
}

/// The syntactic kind of a resolved type declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedDeclarationKind {
    Interface,
    TypeAlias,
    Class,
    Unknown,
}

/// Kind and span of one local type symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedLocalTypeSymbolMetadata {
    pub kind: ResolvedDeclarationKind,
    pub span: verter_span::Span,
}

/// Where a requested type name resolved to: the declaring file, owner,
/// resolved name, span and kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTypeDeclaration {
    pub requested_name: String,
    pub declaration_id: Option<DeclarationId>,
    pub resolved_name: String,
    pub canonical_source: String,
    pub owner: verter_type_expr::TopLevelOwnerId,
    pub span: verter_span::Span,
    pub kind: ResolvedDeclarationKind,
    pub text: Option<String>,
}

/// Exact identity of a top-level runtime value declaration.
///
/// `owner` is part of the identity for carrier files: module and instance
/// declarations with the same name are distinct values.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ValueDeclIdentity {
    pub canonical_id: String,
    pub owner: verter_type_expr::TopLevelOwnerId,
    pub name: String,
}

/// An imported type-registry symbol resolved to its defining declaration: the
/// owning file and owner, the exported name, the narrowed body facts and the
/// canonical files it depends on.
#[derive(Debug, Clone)]
pub struct ResolvedImportedRegistrySymbol {
    pub canonical_id: String,
    pub owner: verter_type_expr::TopLevelOwnerId,
    pub exported_name: String,
    /// The resolved declaration's narrowed body FACTS: classification plus the
    /// content-free authored body-slot locator (never an embedded body).
    /// Consumers lower the slot through the one shared dispatch on demand and
    /// classify node-domain; the carrier itself stays `Send + Sync`
    /// cache-safe (`ImportedRegistryDb` stores it cross-request).
    pub body: verter_type_expr::facts::PreparedTypeBodyFacts,
    pub canonical_dependencies: std::collections::BTreeSet<String>,
}
