//! OXC canonicalizer for the Vue conformance comparator.
//!
//! Parses a JS module with OXC (the same parser on both sides — Verter output
//! and the vendored official golden), runs `SemanticBuilder` fail-closed, and
//! produces a canonical tree (`Canon`) in which the WAIVED dimensions are
//! erased and the in-contract dimensions are retained exactly:
//!
//! - Waived: trivia whitespace/formatting, redundant parentheses (transparent
//!   `ParenthesizedExpression`), empty no-op statements, quote delimiters,
//!   ordinary comments, and the SPELLING of private/compiler-generated local
//!   bindings (scope-aware alpha equivalence).
//! - Retained: statement/expression/property order, operators, arguments,
//!   string/template/numeric/bigint/regex payloads, static HTML, patch flags,
//!   helper family identity, import sources + imported names + side-effect
//!   sequence, export names, source-authored and public/exported identifiers,
//!   member/property keys, labels, and semantic comments (PURE, license,
//!   JSDoc, bundler-significant) anchored to their AST occurrence node.
//!
//! Every OXC JS AST variant is represented; TS/JSX/V8-intrinsic variants are
//! EXPLICITLY REFUSED (panic with the variant name) — the inputs are plain JS
//! modules, and a silent catch-all would hide a missed variant.
//!
//! ## Scope-aware alpha equivalence
//!
//! A binding is alpha-eligible (its spelling waived, represented by a
//! `BindingKey`) only when ALL hold:
//!
//! - not source-authored (its name does not appear in the SFC source — the
//!   official RC maps ship empty `names`, so provenance is the SFC identifier
//!   set; conservatively exact when in doubt),
//! - not exported/public,
//! - not used in a name-bearing position (object-shorthand property,
//!   destructuring-assignment shorthand, `.name` inferred-name escape),
//! - the module contains no direct `eval` or `with` (ultra-conservative:
//!   disables alpha for the whole module),
//! - all its references resolve (unresolved references stay exact).
//!
//! `BindingKey = (structural scope identity, declaration ordinal, binding
//! pattern slot, binding kind)` — references carry the same key as their
//! declaration, preserving shadowing and closure-capture topology.

// Module split (file-size guard): `types + driver` here, `classify` for the
// Exact-vs-Alpha identifier classification, `comments` for semantic-comment
// anchoring, and `canonize` for the exhaustive canonicalizer. Behavior is
// identical to the single-file `canon.rs` — pure extraction.

mod canonize;
mod classify;
mod comments;

use std::collections::BTreeSet;

use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;

use canonize::Canonizer;
use classify::Classifier;

/// A canonical AST node: `kind` + positional children.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Canon {
    Node(&'static str, Vec<Canon>),
    /// Exact-scrutiny leaf: `"ident"` (contract identifier), `"str"`, `"num"`,
    /// `"op"`, `"flag"`, `"tpl"`, `"text"` (comment), `"none"` (absent child)…
    Leaf(&'static str, String),
    /// Alpha-renamable private binding/reference, identified structurally.
    Alpha(BindingKey),
    /// An import binding reference/declaration: the alias SPELLING is waived,
    /// but the family identity — `(module source, imported name)` — is
    /// contract. (Import aliases must not key on declaration order: two
    /// compilers order specifiers differently for the same helper set.)
    ImportBinding {
        source: String,
        imported: String,
    },
}

impl Canon {
    fn node(kind: &'static str, children: Vec<Canon>) -> Canon {
        Canon::Node(kind, children)
    }
    fn leaf(kind: &'static str, value: impl Into<String>) -> Canon {
        Canon::Leaf(kind, value.into())
    }
    fn none() -> Canon {
        Canon::Leaf("none", String::new())
    }
}

/// `(structural scope identity, declaration ordinal, pattern slot, kind)`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BindingKey {
    /// Pre-order `NodeId` index of the scope-creating node (stable across
    /// cosmetically-reformatted isomorphic modules).
    pub scope_ordinal: u32,
    /// Rank among the scope's bindings by declaration position.
    pub declaration_ordinal: u32,
    /// Child-index path within the root binding pattern (`[]` for plain
    /// identifiers, `255` = rest, `254` = default-value).
    pub pattern_slot: Vec<u8>,
    pub kind: BindingKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BindingKind {
    Var,
    Let,
    Const,
    Param,
    Function,
    Class,
    Catch,
    Import,
}

/// One import declaration in canonical form: source exact, named specifiers
/// sorted by imported name, aliases classified (helper aliases may
/// alpha-rename; the imported/family name may not).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportEntry {
    pub source: String,
    pub side_effect: bool,
    pub default: Option<Canon>,
    pub namespace: Option<Canon>,
    /// `(imported name, alias)` sorted by imported name.
    pub named: Vec<(String, Canon)>,
    /// `(key, value)` import attributes, sorted.
    pub attributes: Vec<(String, String)>,
}

/// A canonicalized module: the AST tree plus the import topology (imports are
/// extracted from the tree and compared via their own dimension; a
/// `Leaf("import", source)` marker preserves each declaration's position in
/// the module-item sequence).
#[derive(Debug, Clone)]
pub struct CanonModule {
    pub tree: Canon,
    pub imports: Vec<ImportEntry>,
}

/// Parse + canonicalize a JS module. Fails hard on parse/semantic errors —
/// both sides of the comparison must be valid ESM.
pub fn canonicalize_module(code: &str, authored: &BTreeSet<String>) -> Result<CanonModule, String> {
    let allocator = Allocator::new();
    let parse = Parser::new(&allocator, code, SourceType::mjs()).parse();
    if parse.panicked || !parse.errors.is_empty() {
        let messages = parse
            .errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("; ");
        return Err(format!("module does not parse as ESM JS: {messages}"));
    }
    let program = parse.program;
    let built = SemanticBuilder::new().build(&program);
    if !built.errors.is_empty() {
        let messages = built
            .errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("; ");
        return Err(format!("module fails semantic analysis: {messages}"));
    }
    let semantic = built.semantic;
    let scoping = semantic.scoping();
    let nodes = semantic.nodes();

    let classifier = Classifier::build(&semantic, authored);
    let comments = comments::anchor_comments(&semantic, code);

    let canonizer = Canonizer {
        scoping,
        nodes,
        classifier: &classifier,
        comments: std::cell::RefCell::new(comments),
        specifier_import_sources_seen: std::cell::RefCell::new(BTreeSet::new()),
    };
    let tree = canonizer.canon_program(&program);
    let imports = canonizer.extract_imports(&program);
    Ok(CanonModule { tree, imports })
}
