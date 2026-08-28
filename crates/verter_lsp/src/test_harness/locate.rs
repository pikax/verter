//! Locating a document position for a test assertion.
//!
//! Two locators live here, and the difference between them is the whole point.
//!
//! * [`RealProviderTestSession::find_position`] and
//!   [`RealProviderTestSession::find_nth_position`] search for a CONTIGUOUS byte
//!   string. That is all a byte search can do: it has no notion of tokens, so it
//!   cannot tell a formatting separator from a space inside an attribute value —
//!   and it must not try. Every whitespace, quote, or escape rule such a search
//!   could grow is a guess about a language it does not parse, and each guess
//!   buys a new class of confidently wrong offsets. A needle whose text the
//!   document lays out differently is therefore simply NOT FOUND, and the
//!   locator panics naming the needle.
//! * [`RealProviderTestSession::find_template_tag_position`] names a template
//!   CONSTRUCT — the element's tag name plus the attribute that says which
//!   element — and resolves it through the real Vue parse. `:count` on
//!   `<GlobalCountComp>` has a definite span no matter how the tag wraps across
//!   lines, so whitespace tolerance, quote masks, and escape rules never arise
//!   here: the parser already decided, exactly, where every token is.
//!
//! A test whose needle would span a reflowed (multi-line) tag belongs on the
//! structural locator. Reflowing the fixture to suit the byte search instead
//! would delete the multi-line coverage that fixture exists to provide.
//!
//! # What the structural locator covers
//!
//! [`RealProviderTestSession::find_template_tag_position`] answers exactly one
//! question: a position inside a TAG NAME, disambiguated by one VALUED
//! `(attribute, value)` pair. Every other template position is outside it —
//! attribute and directive NAME positions, positions into an attribute value or
//! expression, positions inside an interpolation, Svelte `{expr}` and `bind:`
//! forms, slot names, `v-for` binding parts, selectors for VALUE-LESS
//! attributes, structural nth-disambiguation, and text nodes. The parse it
//! drives is Vue-SFC only, so there is no Svelte structural path.
//!
//! The byte-string locators therefore carry most of this harness, including many
//! positions that sit inside a construct the parser already knows exactly. Do
//! not read the existence of the structural locator as a claim that template
//! positions here are located structurally — check the one covered question
//! above before assuming it can address a given position. The call-site
//! inventory and the API surface a wider structural reach would need are
//! recorded in
//! the language-service contract.

use tower_lsp_server::ls_types::*;
use verter_compiler::ast::types::{AstNodeKind, ElementNode, TemplateAst};
use verter_compiler::diagnostics::{
    CompilerErrorCode, DiagnosticSeverity, SyntaxPluginContext, SyntaxPluginOptions,
};
use verter_compiler::parser::Syntax;
use verter_compiler::types::NodeProp;

use super::RealProviderTestSession;

impl RealProviderTestSession {
    /// Find a position within an open document by searching for `needle` and adding `delta`.
    ///
    /// CONTIGUOUS ONLY. `needle` must appear in the document byte for byte; a
    /// needle whose whitespace the document lays out differently is not present
    /// and this panics. To address a template element whose tag may wrap across
    /// lines, use [`Self::find_template_tag_position`].
    pub(crate) fn find_position(&self, uri: &Uri, needle: &str, delta: usize) -> Position {
        let doc = self
            .server()
            .test_documents()
            .get(uri)
            .expect("document should be open");
        let offset = doc
            .source
            .find(needle)
            .unwrap_or_else(|| panic!("needle `{needle}` should exist in document"))
            + delta;
        doc.line_index
            .offset_to_position(offset as u32)
            .expect("valid position")
    }

    /// Find the Nth (0-indexed) occurrence of `needle` and add `delta`.
    ///
    /// CONTIGUOUS ONLY, exactly as [`Self::find_position`].
    pub(crate) fn find_nth_position(
        &self,
        uri: &Uri,
        needle: &str,
        n: usize,
        delta: usize,
    ) -> Position {
        let doc = self
            .server()
            .test_documents()
            .get(uri)
            .expect("document should be open");
        let mut start = 0;
        let mut count = 0;
        loop {
            match doc.source[start..].find(needle) {
                Some(pos) => {
                    let abs_pos = start + pos;
                    if count == n {
                        return doc
                            .line_index
                            .offset_to_position((abs_pos + delta) as u32)
                            .expect("valid position");
                    }
                    count += 1;
                    start = abs_pos + 1;
                }
                None => {
                    panic!("needle `{needle}` occurrence {n} not found (only {count} occurrences)")
                }
            }
        }
    }

    /// Find the position `name_offset` bytes into the TAG NAME of the template
    /// element written as `<{tag} … {attribute.0}="{attribute.1}" …>`.
    ///
    /// The element is resolved through the SFC parse, so how the authored tag
    /// wraps its attributes across lines is irrelevant — the parser reports the
    /// tag's span either way. `attribute` picks one element out of the
    /// same-named ones by attribute/directive name and value AS AUTHORED
    /// (`(":count", "42")`, `("@click.stop", "go")`, `("v-if", "ready")`,
    /// `("class", "row")`), compared byte for byte against the parsed spans and
    /// never normalised. The name runs to the end of the LAST modifier, so
    /// `@click` and `@click.stop` are different selectors and never match the
    /// same element. The whole authored inventory is searched, including the
    /// directives the parser lifts out of `props` into its own fields
    /// (`v-if`/`v-else-if`/`v-else`, `v-for`, `v-slot`, `v-once`, `ref`).
    ///
    /// Panics — loudly, naming the reason — when no element matches, when more
    /// than one does, when `name_offset` does not address a character inside the
    /// authored tag name, when the source does not parse, or when the parse
    /// DROPPED an authored directive it cannot store (a second `v-for` on one
    /// element), because the inventory is then incomplete and some other element
    /// can look like the unique answer. It never falls back to a byte search: a
    /// wrong position would silently point every downstream assertion at a token
    /// the author never named.
    pub(crate) fn find_template_tag_position(
        &self,
        uri: &Uri,
        tag: &str,
        attribute: (&str, &str),
        name_offset: usize,
    ) -> Position {
        let doc = self
            .server()
            .test_documents()
            .get(uri)
            .expect("document should be open");
        let offset = template_tag_name_offset(&doc.source, tag, attribute, name_offset)
            .unwrap_or_else(|error| {
                panic!(
                    "cannot locate <{tag}> carrying `{}=\"{}\"`: {error}",
                    attribute.0, attribute.1
                )
            });
        doc.line_index
            .offset_to_position(offset as u32)
            .expect("valid position")
    }
}

/// Why a structural tag lookup refused to answer.
///
/// Every variant is a REFUSAL, never a fallback. The alternative to failing here
/// is returning a position for some other token, which turns every downstream
/// assertion into a statement about the wrong part of the document.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum TagLocateError {
    /// The source reported parse errors, so its spans are not trustworthy.
    SourceDidNotParse,
    /// The source has no `<template>` block to search.
    NoTemplate,
    /// The parse reported that it THREW AWAY an authored directive, so the
    /// element inventory it produced is not the inventory the author wrote.
    AuthoredDirectiveDropped { dropped: usize },
    /// No element carries that tag name AND that attribute.
    NoMatch { tag_occurrences: usize },
    /// Several elements match — the attribute did not identify one.
    Ambiguous { matches: usize },
    /// `name_offset` does not address a byte INSIDE the authored tag name.
    /// `name_offset == name_len` is already outside the tag token — the
    /// character there belongs to whatever follows the name.
    OffsetPastTagName { name_len: usize, name_offset: usize },
    /// `name_offset` falls in the middle of a multi-byte character of the
    /// authored tag name, so it addresses no character at all.
    OffsetSplitsCharacter { name_len: usize, name_offset: usize },
}

impl std::fmt::Display for TagLocateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SourceDidNotParse => write!(
                f,
                "the source reported parse errors, so its spans are not trustworthy"
            ),
            Self::NoTemplate => write!(f, "the source has no <template> block"),
            Self::AuthoredDirectiveDropped { dropped } => write!(
                f,
                "the parse dropped {dropped} authored directive(s) it cannot \
                 represent, so its element inventory is incomplete"
            ),
            Self::NoMatch { tag_occurrences } => write!(
                f,
                "no element with that tag name carries that attribute value \
                 ({tag_occurrences} element(s) carry the tag name)"
            ),
            Self::Ambiguous { matches } => write!(
                f,
                "{matches} elements match — the attribute must identify exactly one"
            ),
            Self::OffsetPastTagName {
                name_len,
                name_offset,
            } => write!(
                f,
                "name offset {name_offset} is outside the {name_len}-byte tag name \
                 (the last addressable offset is {})",
                name_len.saturating_sub(1)
            ),
            Self::OffsetSplitsCharacter {
                name_len,
                name_offset,
            } => write!(
                f,
                "name offset {name_offset} splits a multi-byte character of the \
                 {name_len}-byte tag name"
            ),
        }
    }
}

/// Resolve the source byte offset `name_offset` bytes into the tag name of the
/// single template element named `tag` that carries `attribute`.
///
/// This is the whole policy: parse, match the AUTHORED tag name, match the
/// AUTHORED attribute name and value, require EXACTLY ONE element, and measure
/// the offset from the first byte of the tag name. No byte scanning, no
/// whitespace relaxation, no quote or escape model — the spans come from the
/// same parser the compiler itself runs.
fn template_tag_name_offset(
    source: &str,
    tag: &str,
    attribute: (&str, &str),
    name_offset: usize,
) -> Result<usize, TagLocateError> {
    let ast = parse_template(source)?;

    let mut tag_occurrences = 0usize;
    let mut matches: Vec<usize> = Vec::new();
    for node in &ast.nodes {
        let AstNodeKind::Element(element) = &node.kind else {
            continue;
        };
        let name_start = element.tag_open.start as usize + 1;
        let name_end = element.tag_open.name_end as usize;
        if source.get(name_start..name_end) != Some(tag) {
            continue;
        }
        tag_occurrences += 1;
        if authored_props(element).any(|prop| authored_attribute(source, prop) == Some(attribute)) {
            matches.push(name_start);
        }
    }

    let name_start = match matches.len() {
        0 => return Err(TagLocateError::NoMatch { tag_occurrences }),
        1 => matches[0],
        n => return Err(TagLocateError::Ambiguous { matches: n }),
    };

    // The offset must address a CHARACTER of the tag name. `tag.len()` is the
    // boundary just past the final byte — in a reflowed tag that is the newline
    // the formatter inserted, which is outside the tag token the caller named.
    if name_offset >= tag.len() {
        return Err(TagLocateError::OffsetPastTagName {
            name_len: tag.len(),
            name_offset,
        });
    }
    if !tag.is_char_boundary(name_offset) {
        return Err(TagLocateError::OffsetSplitsCharacter {
            name_len: tag.len(),
            name_offset,
        });
    }
    Ok(name_start + name_offset)
}

/// Every attribute and directive the author wrote on `element`.
///
/// `ElementNode::props` is NOT that inventory. The parser LIFTS the directives
/// codegen branches on — `v-if`/`v-else-if`/`v-else`, `v-for`, `v-slot`,
/// `v-once`, and static `ref` — out of `props` into dedicated fields, so a
/// locator reading only `props` cannot see them and answers `NoMatch` for a
/// selector naming one. Chaining the cached fields back in makes the search
/// cover what the source actually contains.
fn authored_props(element: &ElementNode) -> impl Iterator<Item = &NodeProp> {
    element
        .props
        .iter()
        .chain(element.v_condition.as_ref().map(|cond| &cond.prop))
        .chain(element.v_for.as_ref())
        .chain(element.v_slot.as_ref())
        .chain(element.v_once.as_ref())
        .chain(element.v_ref.as_ref())
}

/// The attribute's authored name and value, sliced straight out of the source at
/// the spans the parser recorded.
///
/// The name runs to the end of the LAST MODIFIER when there is one, else to the
/// end of the directive ARGUMENT, else to the end of the name — so what comes
/// back is the whole thing as written (`@click.stop`, `v-model.trim`, `:count`,
/// `class`). Dropping the modifiers would make `@click` and `@click.stop` the
/// same selector, and a caller naming the shorter one would get a confident
/// offset for an attribute the document does not contain.
///
/// The value is the text between the quotes, verbatim: a newline inside it stays
/// a newline, because nothing is interpreted here — the parser already decided
/// where the value begins and ends.
///
/// A value-less attribute (`v-once`, the same-name shorthand `:count`) has no
/// value span and so answers no selector; the caller's lookup refuses rather
/// than matching on the name alone.
fn authored_attribute<'a>(source: &'a str, prop: &NodeProp) -> Option<(&'a str, &'a str)> {
    let name_end = prop
        .modifiers
        .last()
        .map(|modifier| modifier.end)
        .or(prop.arg_end)
        .unwrap_or(prop.name_end);
    let name = source.get(prop.start as usize..name_end as usize)?;
    let value = source.get(prop.value_start? as usize..prop.value_end? as usize)?;
    Some((name, value))
}

/// Parse `source` as an SFC and take its template AST.
///
/// This is the production Vue parse — the same `Syntax` plugin driven by the
/// same `tokenize_sfc` the compiler runs — so the spans a test resolves against
/// are the spans the compiler itself sees.
///
/// Two classes of diagnostic stop the lookup before any span is measured:
///
/// * an ERROR — the parser does not stand behind the spans it produced;
/// * a dropped duplicate directive — the parser's cached-directive fields
///   (`v_if`/`v_for`/`v_slot`/`v_once`/`v_ref`) hold ONE prop each, so a second
///   `v-for` on an element is discarded and only warned about. The AST is then a
///   LOSSY record of the source: the discarded directive is gone, and an element
///   that authored the caller's selector can be missing from the inventory
///   entirely — leaving some other element looking like the unique answer.
///
/// The refusal belongs HERE, not in the parser. First-occurrence-wins is Vue's
/// own semantics for these directives and is what both codegen paths compile
/// (`ElementNode::v_for` is one `Option<NodeProp>`, read as the element's single
/// loop); widening the AST to retain the discards would change production
/// parse output for every consumer to serve a test locator. The locator instead
/// refuses to answer from an inventory it knows is incomplete.
fn parse_template(source: &str) -> Result<TemplateAst, TagLocateError> {
    let bytes = source.as_bytes();
    let mut syntax = Syntax::new(false);
    verter_compiler::tokenizer::byte::tokenize_sfc(bytes, |event| {
        syntax.handle(
            &event,
            &SyntaxPluginContext {
                input: source,
                bytes,
                options: &SyntaxPluginOptions::default(),
                diagnostics: Vec::new(),
            },
        )
    });
    let diagnostics = syntax.take_diagnostics();
    if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
    {
        return Err(TagLocateError::SourceDidNotParse);
    }
    // This refusal is deliberately GLOBAL: a dropped duplicate ANYWHERE in the
    // source refuses the whole lookup, even when it landed on an element the
    // caller did not name. Scoping it to the requested tag by span containment
    // would look tighter and is the wrong trade here. This is test-only code, so
    // the two failure modes are not symmetric: a false refusal is LOUD — the
    // panic names the reason and the author fixes the fixture — while a false
    // answer is SILENT, and every downstream assertion then describes a token
    // the author never named. Over-refusing is the correct bias; do not
    // "optimise" this into span-scoping.
    let dropped = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == CompilerErrorCode::XDuplicateDirective)
        .count();
    if dropped > 0 {
        return Err(TagLocateError::AuthoredDirectiveDropped { dropped });
    }
    syntax.take_template_ast().ok_or(TagLocateError::NoTemplate)
}

#[cfg(test)]
#[path = "locate_tests.rs"]
mod locate_tests;
