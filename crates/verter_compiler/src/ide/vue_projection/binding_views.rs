//! Live template read/write views and actual usage accounting.
//!
//! Three named products:
//!
//! - [`TemplateReadView`]: the template-visible read shape of each setup or
//!   Options binding. Top-level `ref` (and `toRefs`) locals read unwrapped
//!   in the template while the script side still sees the `Ref` (`.value`)
//!   wrapper; nested refs (a `ref` inside an object literal) stay wrapped.
//!   `reactive` arrays/collections read directly; `readonly` objects and
//!   `defineProps` members (including destructured reactive props) read
//!   directly and are never writable through the write product.
//! - [`TemplateWriteTarget`]: the write shape, deliberately distinct from
//!   the read shape. Mutable refs, writable-computed setters (typed by the
//!   declared setter domain, not the read type), reactive members and
//!   `defineModel` refs are writable; getter-only computed values and
//!   readonly props are refused by [`TemplateWriteTarget::write_target`].
//!   There is no universal mutable alias: every write resolves through that
//!   one checked entry point.
//! - [`BindingUsageSet`]: actual usage only. A binding counts as used when
//!   an authored template, script or style reference names it; nothing
//!   seeds synthetic `void` reads, so `noUnusedLocals` stays meaningful and
//!   a binding unused in every region is reported by
//!   [`BindingUsageSet::unused`] instead of being hidden.
//!
//! Classification is syntax-only over one parse of the setup block under
//! its own grammar (plus the [`CombinedScriptProjection`] facts for Options
//! members, owned by [`project_options_pair`](super::options_api::project_options_pair)).
//! Vue factory calls (`ref`, `computed`, `reactive`, `readonly`,
//! `defineModel`, `toRefs`) resolve by binding, not by bare spelling: the
//! local name must be a runtime import of the matching export from `'vue'`,
//! or a free reference. A setup-local value binding of the same name makes
//! the call ordinary, so a local `function ref()` never unwraps anything.
//! `defineProps` follows the same rule (free reference or runtime `'vue'`
//! import). TypeScript stays the type-answer owner: the setter domain is
//! the authored annotation text slice, never an evaluated type.
//!
//! Views are live: [`TemplateReadView::snapshot_kind`] is always
//! [`ViewSnapshotKind::Live`]. No immutable snapshot type is generated, so
//! an authored mutation cannot be masked by a narrowing-preserving copy.
//!
//! Dormant relative to the live IDE route: Vue IDE routing stays on
//! [`super::super::script`] until STP58 atomic activation. Qualification
//! harnesses reach this through
//! [`VueProjectionBackend::binding_views`](crate::framework_common::vue_projection_backend::VueProjectionBackend::binding_views).

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Argument, BindingPattern, Expression, ImportDeclarationSpecifier, ObjectPropertyKind, Program,
    PropertyKey, Statement, TSSignature, TSType,
};
use oxc_ast_visit::{walk, Visit};
use oxc_span::{GetSpan, SourceType};
use rustc_hash::{FxHashMap, FxHashSet};
use verter_parser::oxc_parse::Parser;

use crate::cursor::ScriptLanguage;

use super::options_api::{project_options_pair, CombinedScriptProjection, OptionsMemberKind};
use super::script_setup::{value_bindings, ScriptBlockInput, SetupProjectionRefusal, SourceRange};

/// Syntax classification of one template-visible binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindingKind {
    /// Top-level `ref(...)` (or one `toRefs(...)` member): the template
    /// reads the value unwrapped; the script side keeps the wrapper.
    Ref,
    /// `computed(...)`: `setter` is true for the `{ get, set }` form.
    Computed {
        /// Whether a setter was declared.
        setter: bool,
    },
    /// `reactive(...)` object/array/collection.
    Reactive,
    /// `readonly(...)` wrap or a `defineProps` member (destructured or
    /// whole-object): direct reads, never writable.
    Readonly,
    /// `defineModel(...)`: a two-way model ref, writable with its value.
    Model,
    /// Direct lexical read: plain locals, functions, imports, object
    /// literals holding nested refs, reactive destructuring copies.
    Plain,
}

/// One template-visible read binding with its source link.
///
/// The `range` is the authored declarator/property name in carrier
/// coordinates, so hover and rename keep pointing at the source binding
/// even though TypeScript checks the generated view property.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateReadBinding {
    /// Binding name.
    pub name: String,
    /// Syntax classification.
    pub kind: BindingKind,
    /// True when the template reads this binding unwrapped while the
    /// script side keeps the wrapper (top-level refs only).
    pub unwrapped: bool,
    /// True when the script-side declaration keeps the `Ref` wrapper
    /// (`.value` access in `<script setup>`).
    pub script_wraps_ref: bool,
    /// Authored name span in the carrier.
    pub range: SourceRange,
}

/// The template-visible read shape: one live row per binding.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TemplateReadView {
    /// Read rows in authored order.
    pub bindings: Vec<TemplateReadBinding>,
}

/// What kind of generated view the read rows describe. Only one variant
/// exists: views are live bindings, never immutable snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewSnapshotKind {
    /// The view names the live binding; authored mutations stay visible.
    Live,
}

impl TemplateReadView {
    /// Look up one read binding by name.
    #[must_use]
    pub fn lookup(&self, name: &str) -> Option<&TemplateReadBinding> {
        self.bindings.iter().find(|binding| binding.name == name)
    }

    /// Always [`ViewSnapshotKind::Live`]: this product generates no
    /// immutable snapshot that could preserve narrowing across an authored
    /// mutation.
    #[must_use]
    pub fn snapshot_kind(&self) -> ViewSnapshotKind {
        ViewSnapshotKind::Live
    }
}

/// The declared write domain of one writable binding. Read types and write
/// domains are separate facts: a writable computed accepts its setter
/// domain, not merely its read type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteDomain {
    /// Mutable `ref` / `toRefs` member: written with the wrapped value.
    RefValue,
    /// Writable computed: written with the declared setter parameter type
    /// (authored annotation text, e.g. `"string"`; empty when the setter
    /// is unannotated, never copied from the read type).
    SetterParam(String),
    /// Reactive member.
    ReactiveMember,
    /// `defineModel` ref: two-way model value.
    ModelValue,
    /// Plain lexical assignment target.
    PlainAssign,
}

/// One writable binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WritableBinding {
    /// Binding name.
    pub name: String,
    /// Declared write domain.
    pub domain: WriteDomain,
}

/// Refusal for a template assignment that must not typecheck.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteRejection {
    /// Assignment to a getter-only `computed(...)`.
    GetterOnlyComputed,
    /// Assignment to a readonly prop or `readonly(...)` object.
    ReadonlyProp,
    /// Assignment to an immutable lexical (`const`) binding.
    ConstBinding,
    /// The name is not a known binding at all.
    UnknownBinding,
}

/// The template-visible write shape. The only way to resolve a write is
/// [`TemplateWriteTarget::write_target`]; there is no universal mutable
/// alias that would let a getter-only computed or readonly prop through.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TemplateWriteTarget {
    /// Writable rows in authored order.
    pub writable: Vec<WritableBinding>,
    /// Names refused as getter-only computed writes.
    pub getter_only: Vec<String>,
    /// Names refused as readonly writes.
    pub readonly: Vec<String>,
    /// Names refused as `const` reassignment writes.
    pub immutable: Vec<String>,
}

impl TemplateWriteTarget {
    /// Resolve a template assignment target: `Ok` for writable bindings,
    /// `Err` for getter-only computed values, readonly props/objects,
    /// immutable `const` bindings, and unknown names.
    pub fn write_target(&self, name: &str) -> Result<&WritableBinding, WriteRejection> {
        if let Some(binding) = self.writable.iter().find(|binding| binding.name == name) {
            return Ok(binding);
        }
        if self.getter_only.iter().any(|known| known == name) {
            return Err(WriteRejection::GetterOnlyComputed);
        }
        if self.readonly.iter().any(|known| known == name) {
            return Err(WriteRejection::ReadonlyProp);
        }
        if self.immutable.iter().any(|known| known == name) {
            return Err(WriteRejection::ConstBinding);
        }
        Err(WriteRejection::UnknownBinding)
    }
}

/// Which authored regions reference a binding.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UsageRegions {
    /// Named by an authored template reference.
    pub template: bool,
    /// Named by an authored script reference.
    pub script: bool,
    /// Named by an authored style reference (`v-bind`).
    pub style: bool,
}

/// One actually-used binding and where it was referenced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsedBinding {
    /// Binding name.
    pub name: String,
    /// Regions with an authored reference.
    pub regions: UsageRegions,
}

/// Root identifier references in binding-bearing template positions:
/// `{{ ... }}` interpolation expressions and bound attribute values
/// (`:prop`, `@event`, `#slot`, `v-...`). Expression positions parse
/// as parenthesized expressions (object literals included); only
/// `v-on` handlers parse as freestanding programs, since those can
/// contain statements. Only resolved root references are collected:
/// static markup text never counts, member properties (`user.foo`
/// contributes `user`, never `foo`), and static attribute strings
/// (`class="accent"`) are skipped outright. HTML comments are removed
/// first. References shadowed by an enclosing `v-for`/`v-slot` alias
/// scope never count as root uses. Undeclared words are ignored
/// downstream, never invented into the population.
fn template_mentions(template: &str) -> FxHashSet<String> {
    let mut visible = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find("<!--") {
        visible.push_str(&rest[..open]);
        rest = match rest[open + "<!--".len()..].find("-->") {
            Some(close) => &rest[open + "<!--".len() + close + "-->".len()..],
            None => break,
        };
    }
    visible.push_str(rest);
    let allocator = Allocator::default();
    let mut scan = ScriptReferenceScan {
        names: FxHashSet::default(),
    };
    // One document-order pass over interpolations and tags so `v-for`
    // and `v-slot` aliases scope their whole element subtree: a child
    // use of a locally bound alias never marks a top-level binding as
    // used. Interpolation expressions use brace-depth matching so object
    // literals (`{{ { a: 1 }.a }}`) do not truncate the snippet. The
    // boundary scan tracks JavaScript lexical state, so a `}` inside a
    // string, template literal, regular expression or comment
    // (`{{ ok ? '}' : count }}`) never closes the interpolation early.
    let mut scopes: Vec<ElementScope> = Vec::new();
    let bytes = visible.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' && bytes.get(i + 1) == Some(&b'{') {
            let Some(j) = interpolation_end(bytes, i + 2) else {
                break;
            };
            let mut local = ScriptReferenceScan {
                names: FxHashSet::default(),
            };
            collect_expression_references(&allocator, &visible[i + 2..j - 2], &mut local);
            insert_unaliased(&mut scan, local.names, &scopes);
            i = j;
            continue;
        }
        if bytes[i] == b'<' {
            let Some(close) = find_tag_end(&visible[i..]) else {
                break;
            };
            let tag = &visible[i..i + close];
            if let Some(name) = close_tag_name(tag) {
                pop_element_scope(&mut scopes, name);
            } else if let Some(name) = open_tag_name(tag) {
                // A `v-for` source sits outside its own alias scope
                // (`item in item` reads the outer `item`): resolve it
                // against the enclosing scopes before this tag's aliases
                // are pushed.
                let mut outer = ScriptReferenceScan {
                    names: FxHashSet::default(),
                };
                collect_v_for_sources(&allocator, tag, &mut outer);
                insert_unaliased(&mut scan, outer.names, &scopes);
                // The tag's own aliases scope its remaining bound
                // attributes (`v-for="todo in items" :key="todo.id"`):
                // push before collecting so the alias never counts as a
                // root use.
                scopes.push(ElementScope {
                    name: name.to_string(),
                    aliases: tag_aliases(&allocator, tag),
                });
                let mut local = ScriptReferenceScan {
                    names: FxHashSet::default(),
                };
                collect_bound_attribute_references(&allocator, tag, &mut local);
                insert_unaliased(&mut scan, local.names, &scopes);
                if tag.trim_end().ends_with("/>") || is_void_element(name) {
                    scopes.pop();
                }
            }
            i += close;
            continue;
        }
        i += 1;
    }
    scan.names
}

/// One open element's template-local alias scope (`v-for` aliases,
/// `v-slot` slot-prop aliases): names bound here, never root uses.
struct ElementScope {
    name: String,
    aliases: FxHashSet<String>,
}

/// Move collected references into the shared scan, dropping any name
/// shadowed by an active element alias scope.
fn insert_unaliased(
    scan: &mut ScriptReferenceScan,
    names: FxHashSet<String>,
    scopes: &[ElementScope],
) {
    for name in names {
        if scopes
            .iter()
            .all(|scope| !scope.aliases.contains(name.as_str()))
        {
            scan.names.insert(name);
        }
    }
}

/// Pop the innermost open element called `name` plus anything opened
/// after it; an unmatched close tag changes nothing.
fn pop_element_scope(scopes: &mut Vec<ElementScope>, name: &str) {
    if let Some(position) = scopes.iter().rposition(|scope| scope.name == name) {
        scopes.truncate(position);
    }
}

/// Name of an open tag (`<li ...>`, `<template ...>`); `None` for close
/// tags, comments, doctypes and processing instructions.
fn open_tag_name(tag: &str) -> Option<&str> {
    let rest = tag.strip_prefix('<')?;
    if rest.starts_with(['/', '!', '?']) {
        return None;
    }
    let end = rest
        .find(|c: char| c.is_ascii_whitespace() || c == '/' || c == '>')
        .unwrap_or(rest.len());
    let name = &rest[..end];
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// Name of a close tag (`</li>`); `None` for anything else.
fn close_tag_name(tag: &str) -> Option<&str> {
    let rest = tag.strip_prefix("</")?;
    let end = rest
        .find(|c: char| c.is_ascii_whitespace() || c == '/' || c == '>')
        .unwrap_or(rest.len());
    let name = &rest[..end];
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// True for HTML void elements, which never have a close tag or a
/// subtree; their aliases (none in practice) pop immediately.
fn is_void_element(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

/// Template-local aliases declared by one open tag: the `v-for` alias
/// side plus `v-slot`/`#` values (slot-prop declarations, never root
/// uses). A malformed `v-for` without a top-level `in`/`of` separator
/// declares nothing.
fn tag_aliases(allocator: &Allocator, tag: &str) -> FxHashSet<String> {
    let mut aliases = FxHashSet::default();
    for (name, value) in tag_attributes(tag) {
        match directive_kind(name) {
            DirectiveKind::For => {
                if let Some((alias_side, _)) = value.and_then(split_v_for) {
                    aliases.extend(v_for_alias_names(allocator, alias_side));
                }
            }
            DirectiveKind::Slot => {
                if let Some(value) = value {
                    aliases.extend(pattern_alias_names(allocator, value));
                }
            }
            DirectiveKind::Bind
            | DirectiveKind::On
            | DirectiveKind::Other
            | DirectiveKind::Static => {}
        }
    }
    aliases
}

/// End of an interpolation opened at `bytes[start - 2..start] == "{{"`:
/// the byte offset past the matching closing `}}`, or `None` when the
/// template ends first. Nested `{`/`}` pairs balance; bytes inside
/// single- or double-quoted strings, template literals (including nested
/// `${ ... }` expressions), line/block comments and regular expression
/// literals never open, close or balance anything.
fn interpolation_end(bytes: &[u8], start: usize) -> Option<usize> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Mode {
        Normal,
        Single,
        Double,
        Template,
        LineComment,
        BlockComment,
        Regex,
        RegexClass,
    }
    let mut mode = Mode::Normal;
    // Suspended modes to resume when a string, template literal,
    // comment or regex literal closes.
    let mut suspended: Vec<Mode> = Vec::new();
    // One entry per `{` consumed in expression position: `true` when the
    // brace opened a template-literal `${ ... }` (whose `}` resumes
    // template text), `false` for a plain object/block brace.
    let mut braces: Vec<bool> = Vec::new();
    let resume = |mode: &mut Mode, suspended: &mut Vec<Mode>, fallback: Mode| {
        *mode = suspended.pop().unwrap_or(fallback);
    };
    let mut j = start;
    // Byte before `index`, skipping ASCII whitespace, if any.
    let prev_significant = |index: usize| -> Option<u8> {
        let mut k = index;
        while k > 0 {
            k -= 1;
            if !bytes[k].is_ascii_whitespace() {
                return Some(bytes[k]);
            }
        }
        None
    };
    while j < bytes.len() {
        let byte = bytes[j];
        let next = bytes.get(j + 1).copied();
        match mode {
            Mode::Single | Mode::Double => {
                let quote = if mode == Mode::Single { b'\'' } else { b'"' };
                if byte == b'\\' {
                    j += 1;
                } else if byte == quote {
                    resume(&mut mode, &mut suspended, Mode::Normal);
                }
            }
            Mode::Template => {
                if byte == b'\\' {
                    j += 1;
                } else if byte == b'`' {
                    resume(&mut mode, &mut suspended, Mode::Normal);
                } else if byte == b'$' && next == Some(b'{') {
                    // A nested `${ ... }` parses as expression text; its
                    // closing brace resumes this template literal.
                    suspended.push(mode);
                    mode = Mode::Normal;
                    braces.push(true);
                    j += 1;
                }
                // Every other byte (including bare `{` and `}`) is literal
                // template text and never balances the interpolation.
            }
            Mode::LineComment => {
                if byte == b'\n' {
                    resume(&mut mode, &mut suspended, Mode::Normal);
                }
            }
            Mode::BlockComment => {
                if byte == b'*' && next == Some(b'/') {
                    resume(&mut mode, &mut suspended, Mode::Normal);
                    j += 1;
                }
            }
            Mode::Regex => {
                if byte == b'\\' {
                    j += 1;
                } else if byte == b'[' {
                    suspended.push(mode);
                    mode = Mode::RegexClass;
                } else if byte == b'/' || byte == b'\n' {
                    resume(&mut mode, &mut suspended, Mode::Normal);
                }
            }
            Mode::RegexClass => {
                if byte == b'\\' {
                    j += 1;
                } else if byte == b']' {
                    resume(&mut mode, &mut suspended, Mode::Regex);
                }
            }
            Mode::Normal => {
                if byte == b'\'' || byte == b'"' || byte == b'`' {
                    suspended.push(mode);
                    mode = if byte == b'\'' {
                        Mode::Single
                    } else if byte == b'"' {
                        Mode::Double
                    } else {
                        Mode::Template
                    };
                } else if byte == b'/' && next == Some(b'/') {
                    suspended.push(mode);
                    mode = Mode::LineComment;
                    j += 1;
                } else if byte == b'/' && next == Some(b'*') {
                    suspended.push(mode);
                    mode = Mode::BlockComment;
                    j += 1;
                } else if byte == b'/' && next != Some(b'/') && next != Some(b'*') {
                    // A `/` opens a regex literal only in operand position:
                    // at the expression start or after an operator or opener.
                    // `//`, `/*` and `/=` (division-assign) never do.
                    let regex_open = next != Some(b'=')
                        && !next.is_some_and(|b| b.is_ascii_whitespace())
                        && match prev_significant(j) {
                            None => true,
                            Some(prev) => matches!(
                                prev,
                                b'(' | b','
                                    | b':'
                                    | b'='
                                    | b'!'
                                    | b'?'
                                    | b'&'
                                    | b'|'
                                    | b';'
                                    | b'{'
                                    | b'}'
                                    | b'['
                                    | b'+'
                                    | b'-'
                                    | b'*'
                                    | b'%'
                                    | b'<'
                                    | b'>'
                                    | b'^'
                                    | b'~'
                            ),
                        };
                    if regex_open {
                        suspended.push(mode);
                        mode = Mode::Regex;
                    }
                } else if byte == b'{' {
                    braces.push(false);
                } else if byte == b'}' {
                    // Only a `}}` pair seen with no brace open closes the
                    // interpolation; otherwise this `}` closes the innermost
                    // open brace and its neighbour is examined on its own
                    // (`{{ { title: label}}}` keeps the object's brace).
                    if braces.is_empty() {
                        if next == Some(b'}') {
                            return Some(j + 2);
                        }
                    } else if braces.pop() == Some(true) {
                        mode = suspended.pop().unwrap_or(Mode::Normal);
                    }
                }
            }
        }
        j += 1;
    }
    None
}

/// End of the tag starting at `text[0] == '<'`: the first `>` outside a
/// quoted attribute value, as a byte offset past `>`.
fn find_tag_end(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut quote = None;
    for (index, &byte) in bytes.iter().enumerate() {
        if let Some(q) = quote {
            if byte == q {
                quote = None;
            }
        } else if byte == b'"' || byte == b'\'' {
            quote = Some(byte);
        } else if byte == b'>' {
            return Some(index + 1);
        }
    }
    None
}

/// Identifier references inside bound attribute positions
/// (`:prop="..."`, `@event="..."`, `#slot="..."`, `v-...="..."`); static
/// attributes contribute nothing. `v-on` values parse as freestanding
/// programs so statement handlers (`bump(); count++`) contribute; every
/// other value position parses as an expression. Vue directive syntax is
/// honoured, not just `name="value"` pairs: same-name shorthand
/// (`:count`, kebab-case `:text-content` binding `textContent`),
/// dynamic arguments (`:[key]="value"`), `v-for` aliases (locally
/// bound, never root uses) and `v-slot` bindings (slot-prop
/// declarations, never root uses).
fn collect_bound_attribute_references(
    allocator: &Allocator,
    tag: &str,
    scan: &mut ScriptReferenceScan,
) {
    for (name, value) in tag_attributes(tag) {
        let Some(value) = value else {
            // Valueless attribute: only bound same-name shorthand
            // (`<div :count>`, `<div v-bind:count>`) names a binding.
            collect_shorthand_reference(name, scan);
            continue;
        };
        if value.is_empty() {
            continue;
        }
        collect_directive_references(allocator, name, value, scan);
    }
}

/// One tag's attributes as `(name, value)` pairs in authored order; a
/// valueless attribute yields `None`. The name scan absorbs the
/// tag-closing `>` (and `/`), so those are stripped from valueless
/// names before classification.
fn tag_attributes(tag: &str) -> Vec<(&str, Option<&str>)> {
    let bytes = tag.as_bytes();
    let mut attributes = Vec::new();
    let mut i = 1;
    while i < bytes.len() {
        while i < bytes.len() && (bytes[i].is_ascii_whitespace() || bytes[i] == b'/') {
            i += 1;
        }
        let name_start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'=' {
            i += 1;
        }
        if i == name_start {
            i += 1;
            continue;
        }
        let name = &tag[name_start..i];
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'=' {
            attributes.push((name.trim_end_matches(['>', '/']), None));
            continue;
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let value = if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'') {
            let quote = bytes[i];
            i += 1;
            let value_start = i;
            while i < bytes.len() && bytes[i] != quote {
                i += 1;
            }
            let value = &tag[value_start..i];
            i += 1;
            value
        } else {
            let value_start = i;
            while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'>' {
                i += 1;
            }
            &tag[value_start..i]
        };
        attributes.push((name, Some(value)));
    }
    attributes
}

/// Vue directive class of one attribute name: which positions name root
/// bindings and which declare template-local aliases.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DirectiveKind {
    /// `:arg`, `v-bind:arg`: the value is a root expression.
    Bind,
    /// `@arg`, `v-on:arg`: the value is a root handler expression.
    On,
    /// `v-for`: the value is `alias in source`; only the source names
    /// root bindings.
    For,
    /// `v-slot...`, `#...`: the value declares slot-prop aliases; only a
    /// dynamic argument in the name (`#[name]`) names a root binding.
    Slot,
    /// Any other `v-...` directive (`v-model`, `v-if`, `v-show`, ...):
    /// the value is a root expression.
    Other,
    /// A static attribute: contributes nothing.
    Static,
}

/// Classify one attribute name into its Vue directive class.
fn directive_kind(name: &str) -> DirectiveKind {
    if name.starts_with(':') {
        DirectiveKind::Bind
    } else if let Some(rest) = name.strip_prefix("v-bind") {
        if rest.is_empty() || rest.starts_with([':', '.', '[']) {
            DirectiveKind::Bind
        } else {
            DirectiveKind::Static
        }
    } else if name.starts_with('@')
        || name
            .strip_prefix("v-on")
            .is_some_and(|rest| rest.is_empty() || rest.starts_with([':', '.', '@', '[']))
    {
        DirectiveKind::On
    } else if name == "v-for" {
        DirectiveKind::For
    } else if name.starts_with('#')
        || name == "v-slot"
        || name.starts_with("v-slot:")
        || name.starts_with("v-slot[")
        || name.starts_with("v-slot.")
    {
        DirectiveKind::Slot
    } else if name.starts_with("v-") {
        DirectiveKind::Other
    } else {
        DirectiveKind::Static
    }
}

/// Collect references for one bound `name="value"` attribute: the dynamic
/// argument in the name (if any) plus the value positions that name root
/// bindings for the directive class.
fn collect_directive_references(
    allocator: &Allocator,
    name: &str,
    value: &str,
    scan: &mut ScriptReferenceScan,
) {
    let kind = directive_kind(name);
    if kind == DirectiveKind::Static {
        return;
    }
    // A dynamic argument (`:[key]`, `@[event]`, `#[name]`, `v-slot:[name]`)
    // is itself a root expression, whatever the directive class.
    if let Some(argument) = dynamic_argument(name) {
        collect_expression_references(allocator, argument, scan);
    }
    match kind {
        // `v-on` handlers can contain statements; every other value
        // position is an expression (object literals included).
        DirectiveKind::On => {
            collect_snippet_references(allocator, value, scan);
        }
        DirectiveKind::Bind | DirectiveKind::Other => {
            collect_expression_references(allocator, value, scan);
        }
        // A `v-for` source resolves against the enclosing scope, so the
        // caller collects it before this tag's aliases apply.
        DirectiveKind::For => {}
        // A slot value (`v-slot="props"`, `#default="{ item }"`) declares
        // template-local aliases, never root uses.
        DirectiveKind::Slot | DirectiveKind::Static => {}
    }
}

/// A valueless bound same-name shorthand (`:count`, `v-bind:count`)
/// references the named binding directly; a dynamic-argument shorthand
/// (`:[key]`) collects the argument expression. Anything else (static
/// attributes, valueless `@`/`#`/`v-` names) contributes nothing.
fn collect_shorthand_reference(name: &str, scan: &mut ScriptReferenceScan) {
    let argument = if let Some(arg) = name.strip_prefix(':') {
        arg
    } else if let Some(rest) = name.strip_prefix("v-bind") {
        rest.strip_prefix(':').unwrap_or(rest)
    } else {
        return;
    };
    if argument.is_empty() {
        return;
    }
    if argument.starts_with('[') {
        if let Some(inner) = dynamic_argument(name) {
            let allocator = Allocator::default();
            collect_expression_references(&allocator, inner, scan);
        }
        return;
    }
    let plain = argument.split(['.', '[']).next().unwrap_or("");
    // A valueless `:text-content` binds the camelCase local.
    let camelized = camelize_shorthand(plain);
    if is_plain_identifier(&camelized) {
        scan.names.insert(camelized);
    }
}

/// Vue `camelize` for same-name shorthand: `text-content` binds
/// `textContent`. A name without `-` round-trips unchanged.
fn camelize_shorthand(name: &str) -> String {
    if !name.contains('-') {
        return name.to_string();
    }
    let mut camelized = String::with_capacity(name.len());
    let mut upper = false;
    for c in name.chars() {
        if c == '-' {
            upper = true;
        } else if upper {
            camelized.push(c.to_ascii_uppercase());
            upper = false;
        } else {
            camelized.push(c);
        }
    }
    camelized
}

/// The `[expression]` dynamic argument inside a directive name, if any.
fn dynamic_argument(name: &str) -> Option<&str> {
    let open = name.find('[')?;
    let mut depth = 0usize;
    for (offset, byte) in name.as_bytes()[open..].iter().enumerate() {
        if *byte == b'[' {
            depth += 1;
        } else if *byte == b']' {
            depth -= 1;
            if depth == 0 {
                return Some(&name[open + 1..open + offset]);
            }
        }
    }
    None
}

/// True for a bare identifier reference (`count`, `$props`); dotted,
/// called or empty spellings are never shorthand references.
fn is_plain_identifier(candidate: &str) -> bool {
    let mut chars = candidate.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' || first == '$' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// Root references of one tag's `v-for="alias in source"` values: only
/// the source side, parsed as an expression. The source lies outside the
/// alias's iteration scope, so the caller resolves these names against
/// the enclosing scopes; the alias side only declares template-local
/// names. A value without a top-level `in`/`of` separator is malformed;
/// it contributes nothing rather than a false alias use.
fn collect_v_for_sources(allocator: &Allocator, tag: &str, scan: &mut ScriptReferenceScan) {
    for (name, value) in tag_attributes(tag) {
        if directive_kind(name) != DirectiveKind::For {
            continue;
        }
        if let Some((_, source)) = value.and_then(split_v_for) {
            collect_expression_references(allocator, source, scan);
        }
    }
}

/// Split a `v-for` value into its alias side and its source side at a
/// top-level `in`/`of` separator (outside any nesting, string or template
/// literal). Returns `None` when there is no such separator.
fn split_v_for(value: &str) -> Option<(&str, &str)> {
    let bytes = value.as_bytes();
    let mut depth = 0usize;
    let mut quote = None::<u8>;
    let mut template = false;
    let mut i = 0;
    while i < bytes.len() {
        let byte = bytes[i];
        if template {
            if byte == b'\\' {
                i += 2;
                continue;
            }
            if byte == b'`' {
                template = false;
            }
            i += 1;
            continue;
        }
        if let Some(q) = quote {
            if byte == b'\\' {
                i += 2;
                continue;
            }
            if byte == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match byte {
            b'\'' | b'"' => {
                quote = Some(byte);
                i += 1;
            }
            b'`' => {
                template = true;
                i += 1;
            }
            b'(' | b'[' | b'{' => {
                depth += 1;
                i += 1;
            }
            b')' | b']' | b'}' => {
                depth = depth.saturating_sub(1);
                i += 1;
            }
            _ => {
                if depth == 0 {
                    // Compare bytes, never slice: `i` may sit inside a
                    // multi-byte character (`café in items`). A match is
                    // ASCII, so `i` is then a character boundary.
                    for separator in [" in ", " of "] {
                        if bytes[i..].starts_with(separator.as_bytes()) {
                            // `for...in`/`for...of` keywords only separate at
                            // an identifier boundary on the left.
                            let left_ok = i > 0
                                && (bytes[i - 1].is_ascii_alphanumeric()
                                    || bytes[i - 1] == b'_'
                                    || bytes[i - 1] == b'$'
                                    || bytes[i - 1] == b')'
                                    || bytes[i - 1] == b']'
                                    || bytes[i - 1] == b'}');
                            if left_ok {
                                return Some((
                                    value[..i].trim(),
                                    value[i + separator.len()..].trim(),
                                ));
                            }
                        }
                    }
                    // Fall back to tab/newline separators (`item\tin\tlist`).
                    for keyword in ["in", "of"] {
                        if bytes[i..].starts_with(keyword.as_bytes()) {
                            let before = i.checked_sub(1).map(|k| bytes[k]);
                            let after = bytes.get(i + keyword.len()).copied();
                            let boundary =
                                |b: Option<u8>| b.is_none_or(|c| c.is_ascii_whitespace());
                            if boundary(before) && after.is_some_and(|c| c.is_ascii_whitespace()) {
                                return Some((
                                    value[..i].trim(),
                                    value[i + keyword.len()..].trim(),
                                ));
                            }
                        }
                    }
                }
                i += 1;
            }
        }
    }
    None
}

/// Names declared by a `v-for` alias side (`item`, `(item, index)`,
/// `{ id, label }`): parsed as an array-destructuring probe so nested
/// patterns resolve through the real grammar instead of word-splitting.
/// Falls back to no aliases (collect the source whole) when the probe
/// does not parse.
fn v_for_alias_names(allocator: &Allocator, aliases: &str) -> FxHashSet<String> {
    pattern_alias_names(allocator, aliases)
}

/// Names declared by one destructuring pattern (`props`, `{ item }`,
/// `(row, index)`): parsed as an array-destructuring probe so nested
/// patterns resolve through the real grammar instead of word-splitting.
/// Used for `v-for` alias sides and `v-slot` values alike. Falls back
/// to no aliases when the probe does not parse.
fn pattern_alias_names(allocator: &Allocator, pattern: &str) -> FxHashSet<String> {
    let trimmed = pattern.trim();
    let inner = trimmed
        .strip_prefix('(')
        .and_then(|rest| rest.strip_suffix(')'))
        .unwrap_or(trimmed);
    let probe = format!("let [{inner}] = [];");
    let Some(program) = parse_reference_snippet(allocator, &probe) else {
        return FxHashSet::default();
    };
    let mut names = FxHashSet::default();
    for statement in &program.body {
        let Statement::VariableDeclaration(declaration) = statement else {
            continue;
        };
        for declarator in &declaration.declarations {
            for (name, _) in pattern_names(&declarator.id, 0) {
                names.insert(name);
            }
        }
    }
    names
}

/// Identifier references (never declarations) in one script or template
/// expression snippet.
struct ScriptReferenceScan {
    names: FxHashSet<String>,
}

impl<'a> Visit<'a> for ScriptReferenceScan {
    fn visit_identifier_reference(&mut self, it: &oxc_ast::ast::IdentifierReference<'a>) {
        self.names.insert(it.name.to_string());
        walk::walk_identifier_reference(self, it);
    }
}

/// Parse one reference snippet under the plain TS grammar, falling back to
/// TSX so admitted TSX blocks (for example `const node = <div />`) still
/// contribute references. Returns `None` when the snippet does not parse
/// under either grammar.
fn parse_reference_snippet<'a>(allocator: &'a Allocator, snippet: &'a str) -> Option<Program<'a>> {
    for source_type in [SourceType::ts(), SourceType::tsx()] {
        let parsed = Parser::new(allocator, snippet, source_type.with_module(true)).parse();
        if !parsed.panicked && parsed.errors.is_empty() {
            return Some(parsed.program);
        }
    }
    None
}

/// Collect root identifier references from one template expression
/// snippet. Member properties resolve to their root object (`user.foo`
/// contributes `user`, never `foo`); unparseable snippets contribute
/// nothing rather than synthetic references.
fn collect_snippet_references(
    allocator: &Allocator,
    snippet: &str,
    scan: &mut ScriptReferenceScan,
) {
    let Some(program) = parse_reference_snippet(allocator, snippet) else {
        return;
    };
    scan.visit_program(&program);
}

/// Collect references from one expression-position snippet
/// (interpolations, bound values, dynamic arguments, `v-for` sources).
/// Object literals (`{ active: isActive, ... }`) parse as statement
/// blocks under program grammar, so the snippet is wrapped in
/// parentheses to force expression parsing. Statement positions
/// (`v-on` handlers) keep program parsing via
/// [`collect_snippet_references`].
fn collect_expression_references(
    allocator: &Allocator,
    expression: &str,
    scan: &mut ScriptReferenceScan,
) {
    let wrapped = format!("({expression});");
    collect_snippet_references(allocator, &wrapped, scan);
}

/// Script references resolved to the projected top-level declarations.
/// References resolving through the semantic scope tree to a declared
/// top-level symbol count; a shadowed identifier inside a nested scope
/// resolves to its own symbol and never marks the top-level name as used.
/// A declared name with no top-level symbol in this script (Options
/// members, cross-block references) falls back to the name-based
/// reference set so reachable uses are not lost. An unparseable script
/// contributes no references rather than synthetic ones.
fn script_reference_names(script: &str, declared: &[String]) -> FxHashSet<String> {
    let allocator = Allocator::default();
    let Some(program) = parse_reference_snippet(&allocator, script) else {
        return FxHashSet::default();
    };
    let mut scan = ScriptReferenceScan {
        names: FxHashSet::default(),
    };
    scan.visit_program(&program);
    let semantic = oxc_semantic::SemanticBuilder::new()
        .build(&program)
        .semantic;
    let scoping = semantic.scoping();
    let root = scoping.root_scope_id();
    let mut names = FxHashSet::default();
    for name in declared {
        match scoping.get_binding(root, name.as_str().into()) {
            Some(symbol) if !scoping.get_resolved_reference_ids(symbol).is_empty() => {
                names.insert(name.clone());
            }
            None if scan.names.contains(name.as_str()) => {
                names.insert(name.clone());
            }
            Some(_) | None => {}
        }
    }
    names
}

/// Style `v-bind()` names: the only way authored style references script
/// bindings. Bare `v-bind(name)` names one identifier; quoted
/// `v-bind('theme.color')` holds a JavaScript expression whose root
/// references count (`theme`), with any `)` inside the quotes kept.
fn style_vbind_names(style: &str) -> FxHashSet<String> {
    let allocator = Allocator::default();
    let mut scan = ScriptReferenceScan {
        names: FxHashSet::default(),
    };
    let mut rest = style;
    while let Some(open) = rest.find("v-bind(") {
        rest = &rest[open + "v-bind(".len()..];
        let inner = rest.trim_start();
        if let Some(quote) = inner.chars().next().filter(|c| *c == '\'' || *c == '"') {
            let body = &inner[1..];
            let Some(end) = body.find(quote) else {
                break;
            };
            collect_expression_references(&allocator, &body[..end], &mut scan);
            rest = &body[end + 1..];
            continue;
        }
        let Some(close) = rest.find(')') else {
            break;
        };
        let candidate = rest[..close].trim();
        if is_plain_identifier(candidate) {
            scan.names.insert(candidate.to_string());
        }
        rest = &rest[close + 1..];
    }
    scan.names
}

/// Actual usage accounting over one declared name set.
///
/// Built only from authored references; the constructor never seeds
/// synthetic `void` reads, so a binding unused in every region stays
/// visible in [`BindingUsageSet::unused`] and `noUnusedLocals` /
/// `noUnusedParameters` keep firing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BindingUsageSet {
    /// Declared names in authored order.
    pub declared: Vec<String>,
    /// Actually referenced names in declared order.
    pub used: Vec<UsedBinding>,
}

impl BindingUsageSet {
    /// Account usage from authored references only. Reference names outside
    /// `declared` are ignored, never invented into the population.
    #[must_use]
    pub fn from_authored_references(
        declared: &[String],
        template: &[&str],
        script: &[&str],
        style: &[&str],
    ) -> Self {
        let in_region = |name: &str, region: &[&str]| region.contains(&name);
        let mut used = Vec::new();
        for name in declared {
            let regions = UsageRegions {
                template: in_region(name, template),
                script: in_region(name, script),
                style: in_region(name, style),
            };
            if regions.template || regions.script || regions.style {
                used.push(UsedBinding {
                    name: name.clone(),
                    regions,
                });
            }
        }
        Self {
            declared: declared.to_vec(),
            used,
        }
    }

    /// True exactly when `name` has an authored reference in any region.
    #[must_use]
    pub fn is_used(&self, name: &str) -> bool {
        self.used.iter().any(|binding| binding.name == name)
    }

    /// Account usage from authored region text instead of caller-supplied
    /// slices: template root references, resolved script references, and
    /// style `v-bind()` names. Template collection parses `{{ }}`
    /// interpolations and bound attribute values as expressions over
    /// authored bytes with HTML comments removed, so static markup and
    /// member properties never count as uses. Script collection resolves
    /// references through the semantic scope tree to the projected
    /// top-level declarations, so declarations and shadowed identifiers
    /// never count as uses; each block parses under TS with a TSX
    /// fallback matching the admitted setup dialects. An unparseable
    /// script contributes no script references rather than synthetic
    /// ones.
    #[must_use]
    pub fn from_region_text(
        declared: &[String],
        template: &str,
        script: &str,
        style: &str,
    ) -> Self {
        let template_owned = template_mentions(template);
        let script_owned = script_reference_names(script, declared);
        let style_owned = style_vbind_names(style);
        let template_refs: Vec<&str> = template_owned.iter().map(String::as_str).collect();
        let script_refs: Vec<&str> = script_owned.iter().map(String::as_str).collect();
        let style_refs: Vec<&str> = style_owned.iter().map(String::as_str).collect();
        Self::from_authored_references(declared, &template_refs, &script_refs, &style_refs)
    }

    /// Declared names with no authored reference in any region, in
    /// declared order.
    #[must_use]
    pub fn unused(&self) -> Vec<&str> {
        let used: FxHashSet<&str> = self
            .used
            .iter()
            .map(|binding| binding.name.as_str())
            .collect();
        self.declared
            .iter()
            .filter(|name| !used.contains(name.as_str()))
            .map(String::as_str)
            .collect()
    }
}

/// The owned STP15 product pair: live read rows plus checked write rows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BindingViewsProjection {
    /// Template-visible read shape.
    pub read: TemplateReadView,
    /// Template-visible write shape.
    pub write: TemplateWriteTarget,
}

/// Vue reactivity factories recognised at setup scope by binding (not by
/// bare spelling). The membership is Vue's public Composition API surface
/// (`ref`, `computed`, `reactive`, `readonly`, `toRefs`,
/// <https://vuejs.org/api/reactivity-core.html>) plus the `<script setup>`
/// compile-time macros (`defineProps`, `defineModel`, `withDefaults`,
/// <https://vuejs.org/api/sfc-script-setup.html>); the pinned engine
/// `vue@3.6.0-rc.5` (`tests/sfc-projection/STP1/products/engine-matrix.json`)
/// exports the same runtime surface. Each entry is checked against an
/// actual `from 'vue'` import or a free reference by [`factory_export`],
/// so the table is verified against the parsed imports rather than
/// trusted on spelling; a name outside the table (for example `watch`)
/// resolves to no factory and classifies as an ordinary binding.
const FACTORY_EXPORTS: [&str; 8] = [
    "ref",
    "computed",
    "reactive",
    "readonly",
    "defineModel",
    "toRefs",
    "defineProps",
    "withDefaults",
];

/// Runtime (non-type-only) `'vue'` imports as local name to exported symbol.
fn vue_runtime_imports<'a>(program: &'a Program<'a>) -> FxHashMap<&'a str, &'a str> {
    let mut names = FxHashMap::default();
    for statement in &program.body {
        let Statement::ImportDeclaration(import) = statement else {
            continue;
        };
        if import.import_kind.is_type() || import.source.value != "vue" {
            continue;
        }
        let Some(specifiers) = &import.specifiers else {
            continue;
        };
        for specifier in specifiers {
            if let ImportDeclarationSpecifier::ImportSpecifier(spec) = specifier {
                if spec.import_kind.is_type() {
                    continue;
                }
                names.insert(spec.local.name.as_str(), imported_name(spec));
            }
        }
    }
    names
}

/// Exported name a runtime (non-type-only) `'vue'` import specifier binds.
fn imported_name<'a>(spec: &'a oxc_ast::ast::ImportSpecifier<'a>) -> &'a str {
    match &spec.imported {
        oxc_ast::ast::ModuleExportName::IdentifierName(name) => name.name.as_str(),
        oxc_ast::ast::ModuleExportName::IdentifierReference(name) => name.name.as_str(),
        oxc_ast::ast::ModuleExportName::StringLiteral(literal) => literal.value.as_str(),
    }
}

/// Resolve a call callee to the Vue factory/macro export it names, or
/// `None` when the callee is not a recognised export or is shadowed by a
/// setup-local value binding (which makes the call ordinary).
fn factory_export<'a>(
    callee: &'a Expression<'a>,
    values: &FxHashSet<String>,
    vue_imports: &FxHashMap<&'a str, &'a str>,
) -> Option<&'a str> {
    let Expression::Identifier(identifier) = callee else {
        return None;
    };
    let local = identifier.name.as_str();
    match vue_imports.get(local) {
        Some(exported) if FACTORY_EXPORTS.contains(exported) => Some(exported),
        Some(_) => None,
        None if !values.contains(local) && FACTORY_EXPORTS.contains(&local) => Some(local),
        None => None,
    }
}

fn first_argument<'a>(call: &'a oxc_ast::ast::CallExpression<'a>) -> Option<&'a Expression<'a>> {
    call.arguments.first().and_then(|argument| {
        if matches!(argument, Argument::SpreadElement(_)) {
            None
        } else {
            argument.as_expression()
        }
    })
}

/// Property value for `key` in an object expression, when the property is
/// a plain (non-computed) entry.
fn object_property<'a>(
    object: &'a oxc_ast::ast::ObjectExpression<'a>,
    key: &str,
) -> Option<&'a Expression<'a>> {
    for property in &object.properties {
        let ObjectPropertyKind::ObjectProperty(property) = property else {
            continue;
        };
        let matches = match &property.key {
            PropertyKey::StaticIdentifier(identifier) => identifier.name.as_str() == key,
            PropertyKey::StringLiteral(literal) => literal.value.as_str() == key,
            _ => false,
        };
        if matches && !property.computed {
            return Some(&property.value);
        }
    }
    None
}

/// Slice the setter parameter annotation of a writable-computed options
/// object: `computed({ get, set(v: Domain) {} })` yields `Some("Domain")`.
/// Unannotated setters yield `None`; the domain is then recorded empty,
/// never defaulted to the read type.
fn setter_domain(content: &str, object: &oxc_ast::ast::ObjectExpression<'_>) -> Option<String> {
    let set = object_property(object, "set")?;
    let params = match set {
        Expression::ArrowFunctionExpression(arrow) => Some(arrow.params.as_ref()),
        Expression::FunctionExpression(function) => Some(function.params.as_ref()),
        _ => return None,
    };
    let first = params?.items.first()?;
    let annotation = first.type_annotation.as_ref()?;
    let span = annotation.type_annotation.span();
    let (start, end) = (span.start as usize, span.end as usize);
    // The span comes from this same parse of `content`, so an
    // out-of-bounds slice is an internal invariant break, never a
    // plausible empty domain: assert instead of clamping to `""`.
    assert!(
        start <= end && end <= content.len(),
        "setter annotation span {start}..{end} escapes the parsed setup block"
    );
    Some(content[start..end].to_string())
}

/// Names bound by a binding pattern, with each name's span relative to
/// `base` (the carrier offset of the block content).
fn pattern_names(pattern: &BindingPattern<'_>, base: u32) -> Vec<(String, SourceRange)> {
    let mut names = Vec::new();
    match pattern {
        BindingPattern::BindingIdentifier(identifier) => names.push((
            identifier.name.to_string(),
            SourceRange {
                start: base + identifier.span.start,
                end: base + identifier.span.end,
            },
        )),
        BindingPattern::ObjectPattern(object) => {
            for property in &object.properties {
                names.extend(pattern_names(&property.value, base));
            }
            if let Some(rest) = &object.rest {
                names.extend(pattern_names(&rest.argument, base));
            }
        }
        BindingPattern::ArrayPattern(array) => {
            for element in array.elements.iter().flatten() {
                names.extend(pattern_names(element, base));
            }
            if let Some(rest) = &array.rest {
                names.extend(pattern_names(&rest.argument, base));
            }
        }
        BindingPattern::AssignmentPattern(assignment) => {
            names.extend(pattern_names(&assignment.left, base));
        }
    }
    names
}

/// Push one read row plus its write shape. `computed_domain` carries the
/// declared setter domain for writable computed values; other kinds ignore
/// it. Writable computed rows always record the declared domain (empty
/// when unannotated), never the read type. `mutable` is false for `const`
/// declarators: an immutable lexical plain (object literals holding nested
/// refs, reactive destructuring copies, other non-call initializers)
/// keeps its direct read row but is refused as a write target, so template
/// reassignment of a `const` name cannot typecheck. A name that already
/// has a read row is left untouched: the earliest row wins, so a later
/// partially bound destructuring pattern cannot duplicate or override an
/// earlier readonly classification with a conflicting writable row.
fn push_binding(
    projection: &mut BindingViewsProjection,
    name: String,
    kind: BindingKind,
    range: SourceRange,
    computed_domain: Option<String>,
    mutable: bool,
) {
    if projection.read.lookup(&name).is_some() {
        return;
    }
    let (unwrapped, script_wraps_ref) = match &kind {
        BindingKind::Ref | BindingKind::Model => (true, true),
        BindingKind::Computed { .. }
        | BindingKind::Reactive
        | BindingKind::Readonly
        | BindingKind::Plain => (false, false),
    };
    projection.read.bindings.push(TemplateReadBinding {
        name: name.clone(),
        kind: kind.clone(),
        unwrapped,
        script_wraps_ref,
        range,
    });
    match kind {
        BindingKind::Ref => projection.write.writable.push(WritableBinding {
            name,
            domain: WriteDomain::RefValue,
        }),
        BindingKind::Computed { setter: true } => projection.write.writable.push(WritableBinding {
            name,
            domain: WriteDomain::SetterParam(computed_domain.unwrap_or_default()),
        }),
        BindingKind::Computed { setter: false } => projection.write.getter_only.push(name),
        BindingKind::Reactive => projection.write.writable.push(WritableBinding {
            name,
            domain: WriteDomain::ReactiveMember,
        }),
        BindingKind::Model => projection.write.writable.push(WritableBinding {
            name,
            domain: WriteDomain::ModelValue,
        }),
        BindingKind::Readonly => projection.write.readonly.push(name),
        BindingKind::Plain if mutable => projection.write.writable.push(WritableBinding {
            name,
            domain: WriteDomain::PlainAssign,
        }),
        // Immutable lexical plains keep the direct read row above but are
        // refused here: reassigning a `const` name must not typecheck.
        BindingKind::Plain => projection.write.immutable.push(name),
    }
}

/// Push one read row with no write shape. Function declarations and ES
/// module imports are template-visible reads but immutable in scope
/// (TS2588/TS2630): they resolve through [`TemplateWriteTarget`] as
/// [`WriteRejection::UnknownBinding`], never as a writable assignment
/// target.
fn push_read_only(projection: &mut BindingViewsProjection, name: String, range: SourceRange) {
    if projection.read.lookup(&name).is_some() {
        return;
    }
    projection.read.bindings.push(TemplateReadBinding {
        name,
        kind: BindingKind::Plain,
        unwrapped: false,
        script_wraps_ref: false,
        range,
    });
}

/// Resolve a `defineProps` call, unwrapping the `withDefaults(...)`
/// outer call to the inner `defineProps<Props>()` call that carries the
/// type arguments. Returns `None` for any other initializer.
fn resolve_props_call<'a>(
    call: &'a oxc_ast::ast::CallExpression<'a>,
    values: &FxHashSet<String>,
    vue_imports: &FxHashMap<&str, &str>,
) -> Option<&'a oxc_ast::ast::CallExpression<'a>> {
    if factory_export(&call.callee, values, vue_imports) == Some("withDefaults") {
        // `withDefaults(defineProps<Props>(), ...)` binds props: return
        // the inner call so its type arguments stay visible.
        first_argument(call).and_then(|first| match first {
            Expression::CallExpression(inner)
                if factory_export(&inner.callee, values, vue_imports) == Some("defineProps") =>
            {
                Some(inner.as_ref())
            }
            _ => None,
        })
    } else if factory_export(&call.callee, values, vue_imports) == Some("defineProps") {
        Some(call)
    } else {
        None
    }
}

/// Shared context for [`classify_declarator`]: the parsed block text,
/// its carrier base offset, the setup value bindings, the runtime `'vue'`
/// imports, and whether the declarator is mutable (`let`/`var`, not `const`).
struct DeclaratorCtx<'a> {
    content: &'a str,
    base: u32,
    values: &'a FxHashSet<String>,
    vue_imports: &'a FxHashMap<&'a str, &'a str>,
    mutable: bool,
}

/// Classify one setup declarator initializer into read/write rows.
/// `ctx.mutable` is false for `const` declarators (only the
/// [`BindingKind::Plain`] write shape consults it; refs, computed values,
/// reactive objects, models and readonly rows carry their own mutability).
fn classify_declarator(
    projection: &mut BindingViewsProjection,
    ctx: &DeclaratorCtx<'_>,
    id: &BindingPattern<'_>,
    init: Option<&Expression<'_>>,
) {
    // Names already carrying a read row keep it: a partially bound
    // destructuring pattern only adds its unbound members.
    let names: Vec<(String, SourceRange)> = pattern_names(id, ctx.base)
        .into_iter()
        .filter(|(name, _)| projection.read.lookup(name).is_none())
        .collect();
    if names.is_empty() {
        return;
    }
    let Some(Expression::CallExpression(call)) = init else {
        // Object literals (possibly holding nested refs), arrays and every
        // other initializer read directly: only top-level `ref` unwraps.
        for (name, range) in names {
            push_binding(
                projection,
                name,
                BindingKind::Plain,
                range,
                None,
                ctx.mutable,
            );
        }
        return;
    };
    // `withDefaults(defineProps<...>(), ...)` binds props, not a plain.
    if let Some(props_call) = resolve_props_call(call, ctx.values, ctx.vue_imports) {
        collect_props_call(projection, ctx.base, props_call, &names, id);
        return;
    }
    match factory_export(&call.callee, ctx.values, ctx.vue_imports) {
        Some("ref") => {
            // Top-level `ref` stays a ref: the template unwraps the member.
            // A destructured `ref(...)` result holds the extracted value,
            // not the ref object, so only a plain identifier keeps
            // `BindingKind::Ref`; destructured names are plain bindings
            // with the declaration mutability (`const` still refuses
            // reassignment through `push_binding`).
            let whole = matches!(id, BindingPattern::BindingIdentifier(_));
            for (name, range) in names {
                let kind = if whole {
                    BindingKind::Ref
                } else {
                    BindingKind::Plain
                };
                push_binding(projection, name, kind, range, None, ctx.mutable);
            }
        }
        Some("toRefs") => {
            // `toRefs(state)` returns an object whose members are refs: only
            // destructured members (`const { a } = toRefs(state)`) are
            // template-unwrapped refs. The whole returned object
            // (`const refs = toRefs(state)`) reads directly like any other
            // object holding nested refs.
            let whole = matches!(id, BindingPattern::BindingIdentifier(_));
            for (name, range) in names {
                let kind = if whole {
                    BindingKind::Plain
                } else {
                    BindingKind::Ref
                };
                push_binding(projection, name, kind, range, None, ctx.mutable);
            }
        }
        // A destructured `computed(...)` result holds the extracted value,
        // not the computed ref: those names are plain bindings with the
        // declaration mutability, exactly like a destructured `ref(...)`.
        Some("computed") if !matches!(id, BindingPattern::BindingIdentifier(_)) => {
            for (name, range) in names {
                push_binding(
                    projection,
                    name,
                    BindingKind::Plain,
                    range,
                    None,
                    ctx.mutable,
                );
            }
        }
        Some("computed") => match first_argument(call) {
            Some(Expression::ObjectExpression(object)) => {
                let has_set = object_property(object, "set").is_some();
                let domain = setter_domain(ctx.content, object);
                for (name, range) in names {
                    push_binding(
                        projection,
                        name,
                        BindingKind::Computed { setter: has_set },
                        range,
                        domain.clone(),
                        ctx.mutable,
                    );
                }
            }
            // Any `computed(...)` without an options object is a
            // getter-only `ComputedRef`: non-literal getters (identifier
            // references, call results) are never plain writable rows.
            _ => {
                for (name, range) in names {
                    push_binding(
                        projection,
                        name,
                        BindingKind::Computed { setter: false },
                        range,
                        None,
                        ctx.mutable,
                    );
                }
            }
        },
        Some("reactive") => {
            for (name, range) in names {
                // Plain destructuring out of `reactive(...)` copies values;
                // only the whole object keeps reactive member writes.
                let kind = match id {
                    BindingPattern::BindingIdentifier(_) => BindingKind::Reactive,
                    _ => BindingKind::Plain,
                };
                push_binding(projection, name, kind, range, None, ctx.mutable);
            }
        }
        Some("readonly") => {
            for (name, range) in names {
                push_binding(
                    projection,
                    name,
                    BindingKind::Readonly,
                    range,
                    None,
                    ctx.mutable,
                );
            }
        }
        Some("defineModel") => {
            for (name, range) in names {
                push_binding(
                    projection,
                    name,
                    BindingKind::Model,
                    range,
                    None,
                    ctx.mutable,
                );
            }
        }
        _ => {
            for (name, range) in names {
                push_binding(
                    projection,
                    name,
                    BindingKind::Plain,
                    range,
                    None,
                    ctx.mutable,
                );
            }
        }
    }
}

/// Declared prop names with their source ranges: runtime object entries,
/// array-form string literals, plus type-literal members. Empty for an
/// aliased props type (`defineProps<Props>()`), whose members TypeScript
/// owns.
fn extract_props(base: u32, call: &oxc_ast::ast::CallExpression<'_>) -> Vec<(String, SourceRange)> {
    let mut props: Vec<(String, SourceRange)> = Vec::new();
    // Runtime object syntax (`defineProps({ title: String, ... })`):
    // each entry key is a declared prop, mirroring the Options
    // `props: {...}` object entries.
    if let Some(Expression::ObjectExpression(object)) = first_argument(call) {
        for property in &object.properties {
            let ObjectPropertyKind::ObjectProperty(property) = property else {
                continue;
            };
            if property.computed {
                continue;
            }
            let name = match &property.key {
                PropertyKey::StaticIdentifier(identifier) => Some(identifier.name.to_string()),
                PropertyKey::StringLiteral(literal) => Some(literal.value.to_string()),
                _ => None,
            };
            if let Some(name) = name {
                let span = property.key.span();
                props.push((
                    name,
                    SourceRange {
                        start: base + span.start,
                        end: base + span.end,
                    },
                ));
            }
        }
    }
    if let Some(Expression::ArrayExpression(array)) = first_argument(call) {
        for element in &array.elements {
            let Some(element) = element.as_expression() else {
                continue;
            };
            if let Expression::StringLiteral(literal) = element {
                props.push((
                    literal.value.to_string(),
                    SourceRange {
                        start: base + literal.span.start + 1,
                        end: base + literal.span.end - 1,
                    },
                ));
            }
        }
    }
    if let Some(type_arguments) = &call.type_arguments {
        for argument in &type_arguments.params {
            if let TSType::TSTypeLiteral(literal) = argument {
                for member in &literal.members {
                    let TSSignature::TSPropertySignature(signature) = member else {
                        continue;
                    };
                    let name = match &signature.key {
                        PropertyKey::StaticIdentifier(identifier) => {
                            Some(identifier.name.to_string())
                        }
                        PropertyKey::StringLiteral(literal) => Some(literal.value.to_string()),
                        _ => None,
                    };
                    if let Some(name) = name {
                        let span = signature.key.span();
                        props.push((
                            name,
                            SourceRange {
                                start: base + span.start,
                                end: base + span.end,
                            },
                        ));
                    }
                }
            }
        }
    }
    props
}

/// Declared prop key to bound local name: `const { title: heading }`
/// maps `title` to the local `heading`; shorthand members map to
/// themselves. Non-object patterns contribute identity pairs.
fn props_aliases(id: &BindingPattern<'_>, base: u32) -> Vec<(String, String, SourceRange)> {
    let BindingPattern::ObjectPattern(object) = id else {
        return pattern_names(id, base)
            .into_iter()
            .map(|(name, range)| (name.clone(), name, range))
            .collect();
    };
    let mut aliases = Vec::new();
    for property in &object.properties {
        if property.computed {
            continue;
        }
        let key = match &property.key {
            PropertyKey::StaticIdentifier(identifier) => Some(identifier.name.to_string()),
            PropertyKey::StringLiteral(literal) => Some(literal.value.to_string()),
            _ => None,
        };
        let Some(key) = key else {
            continue;
        };
        // Renamed members with defaults (`{ title: heading = "x" }`) nest
        // the local inside an assignment pattern: unwrap to the target so
        // the declared prop key still maps to the bound local.
        let mut target = &property.value;
        while let BindingPattern::AssignmentPattern(assignment) = target {
            target = &assignment.left;
        }
        if let BindingPattern::BindingIdentifier(local) = target {
            aliases.push((
                key,
                local.name.to_string(),
                SourceRange {
                    start: base + local.span.start,
                    end: base + local.span.end,
                },
            ));
        } else {
            for (name, range) in pattern_names(&property.value, base) {
                aliases.push((name.clone(), name, range));
            }
        }
    }
    if let Some(rest) = &object.rest {
        for (name, range) in pattern_names(&rest.argument, base) {
            aliases.push((name.clone(), name, range));
        }
    }
    aliases
}

/// Record `defineProps` rows: array-form and type-literal members as
/// readonly prop rows under their bound local names (renamed members
/// like `title: heading` register `heading`); a whole-object `props`
/// binding as one readonly object row; an aliased props type
/// (`defineProps<Props>()`) as direct readonly reads of the destructured
/// names (TypeScript owns the members).
fn collect_props_call(
    projection: &mut BindingViewsProjection,
    base: u32,
    call: &oxc_ast::ast::CallExpression<'_>,
    bound: &[(String, SourceRange)],
    id: &BindingPattern<'_>,
) {
    let props = extract_props(base, call);
    if props.is_empty() {
        for (name, range) in bound {
            push_binding(
                projection,
                name.clone(),
                BindingKind::Readonly,
                *range,
                None,
                true,
            );
        }
        return;
    }
    if bound.len() == 1 && !props.iter().any(|(name, _)| name == &bound[0].0) {
        push_binding(
            projection,
            bound[0].0.clone(),
            BindingKind::Readonly,
            bound[0].1,
            None,
            true,
        );
        return;
    }
    let aliases = props_aliases(id, base);
    for (name, _) in &props {
        if let Some((_, local, range)) = aliases.iter().find(|(key, _, _)| key == name) {
            push_binding(
                projection,
                local.clone(),
                BindingKind::Readonly,
                *range,
                None,
                true,
            );
        }
    }
    // Rest-pattern locals (`...rest`) are real setup bindings holding
    // readonly prop copies, not declared prop keys: they never match the
    // loop above, so record them explicitly as readonly rows. Other
    // non-prop sibling names stay row-less by contract.
    if let BindingPattern::ObjectPattern(object) = id {
        if let Some(rest) = &object.rest {
            for (name, range) in pattern_names(&rest.argument, base) {
                push_binding(projection, name, BindingKind::Readonly, range, None, true);
            }
        }
    }
}

/// Record a standalone `defineProps<...>();` (or
/// `withDefaults(defineProps<...>(), ...);`) expression statement: with
/// no declarator the declared props themselves are the template rows.
fn collect_standalone_props_call(
    projection: &mut BindingViewsProjection,
    base: u32,
    call: &oxc_ast::ast::CallExpression<'_>,
) {
    for (name, range) in extract_props(base, call) {
        push_binding(projection, name, BindingKind::Readonly, range, None, true);
    }
}

/// Source type for one setup block; absent `lang` is Vue's JavaScript
/// default and is refused, matching the setup authority.
fn setup_source_type(lang: Option<ScriptLanguage>) -> Result<SourceType, SetupProjectionRefusal> {
    match lang {
        Some(ScriptLanguage::TypeScript) => Ok(SourceType::ts().with_module(true)),
        Some(ScriptLanguage::TSX) => Ok(SourceType::tsx().with_module(true)),
        None | Some(_) => Err(SetupProjectionRefusal::NotTypeScript),
    }
}

/// Project live read/write views for one SFC script pair. Options members
/// come from the single combined authority; setup initializer detail comes
/// from one parse of the setup block under its own grammar.
pub fn project_binding_views(
    normal: Option<ScriptBlockInput<'_>>,
    setup: Option<ScriptBlockInput<'_>>,
    generic: Option<&str>,
) -> Result<BindingViewsProjection, SetupProjectionRefusal> {
    if let (Some(n), Some(s)) = (&normal, &setup) {
        if n.lang != s.lang {
            return Err(SetupProjectionRefusal::ScriptLangConflict);
        }
    }
    let combined = project_options_pair(normal, setup, generic)?;
    let mut projection = BindingViewsProjection::default();
    // Setup rows win over Options rows for the same name (setup-first
    // order, earliest row kept): classify `<script setup>` before
    // recording Options members, which skip names already bound.
    if let Some(block) = setup {
        let source_type = setup_source_type(block.lang)?;
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, block.content, source_type).parse();
        if parsed.panicked || !parsed.errors.is_empty() {
            return Err(SetupProjectionRefusal::SyntaxErrors { setup: true });
        }
        let program = &parsed.program;
        let semantic = oxc_semantic::SemanticBuilder::new().build(program).semantic;
        let values = value_bindings(semantic.scoping());
        let vue_imports = vue_runtime_imports(program);
        classify_setup_body(
            &mut projection,
            block.content,
            block.content_start,
            program,
            &values,
            &vue_imports,
        );
    }
    project_options_members(&mut projection, &combined);
    Ok(projection)
}

/// Record Options members (props, computed, methods) as template rows.
/// Emits stay events, not template identifiers; `data()`/`setup()`/
/// mixins/extends members are runtime-known and stay opaque.
fn project_options_members(
    projection: &mut BindingViewsProjection,
    combined: &CombinedScriptProjection,
) {
    let Some(options) = &combined.options else {
        return;
    };
    for member in &options.members {
        if projection.read.lookup(&member.name).is_some() {
            continue;
        }
        let range = member.range;
        match member.kind {
            OptionsMemberKind::Prop => {
                push_binding(
                    projection,
                    member.name.clone(),
                    BindingKind::Readonly,
                    range,
                    None,
                    true,
                );
            }
            OptionsMemberKind::Computed { setter } => {
                push_binding(
                    projection,
                    member.name.clone(),
                    BindingKind::Computed { setter },
                    range,
                    member.setter_domain.clone(),
                    true,
                );
            }
            // Options methods are callable reads, never writable
            // assignment targets (mirrors setup function declarations).
            OptionsMemberKind::Method => {
                push_read_only(projection, member.name.clone(), range);
            }
            OptionsMemberKind::Emit
            | OptionsMemberKind::Component
            | OptionsMemberKind::Directive => {}
        }
    }
}

/// Walk top-level setup statements once, classifying declarators and
/// `defineProps` calls. Nested function bodies are never entered: only
/// top-level bindings are template-visible. Setup rows win over Options
/// rows for the same name (setup-first order, earliest row kept).
fn classify_setup_body(
    projection: &mut BindingViewsProjection,
    content: &str,
    base: u32,
    program: &Program<'_>,
    values: &FxHashSet<String>,
    vue_imports: &FxHashMap<&str, &str>,
) {
    for statement in &program.body {
        match statement {
            Statement::VariableDeclaration(declaration) => {
                // Only `let`/`var` declarators bind mutable plains; `const`
                // plains keep their read row but refuse writes.
                let ctx = DeclaratorCtx {
                    content,
                    base,
                    values,
                    vue_imports,
                    mutable: !matches!(
                        declaration.kind,
                        oxc_ast::ast::VariableDeclarationKind::Const
                    ),
                };
                for declarator in &declaration.declarations {
                    if declarator_bound(projection, &declarator.id) {
                        continue;
                    }
                    classify_declarator(projection, &ctx, &declarator.id, declarator.init.as_ref());
                }
            }
            Statement::FunctionDeclaration(function) => {
                if let Some(id) = &function.id {
                    push_read_only(
                        projection,
                        id.name.to_string(),
                        SourceRange {
                            start: base + id.span.start,
                            end: base + id.span.end,
                        },
                    );
                }
            }
            Statement::ImportDeclaration(import) => {
                // Type-only imports bind no runtime value: `import type`
                // names are never template-visible reads.
                if import.import_kind.is_type() {
                    continue;
                }
                let Some(specifiers) = &import.specifiers else {
                    continue;
                };
                for specifier in specifiers {
                    let (local, span) = match specifier {
                        ImportDeclarationSpecifier::ImportSpecifier(spec) => {
                            if spec.import_kind.is_type() {
                                continue;
                            }
                            (spec.local.name.as_str(), spec.local.span)
                        }
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(spec) => {
                            (spec.local.name.as_str(), spec.local.span)
                        }
                        ImportDeclarationSpecifier::ImportNamespaceSpecifier(spec) => {
                            (spec.local.name.as_str(), spec.local.span)
                        }
                    };
                    push_read_only(
                        projection,
                        local.to_string(),
                        SourceRange {
                            start: base + span.start,
                            end: base + span.end,
                        },
                    );
                }
            }
            Statement::ExpressionStatement(statement) => {
                // Idiomatic `<script setup>` declares props without a
                // script-side reference: a standalone
                // `defineProps<...>();` still binds template rows.
                if let Expression::CallExpression(call) = &statement.expression {
                    if let Some(props_call) = resolve_props_call(call, values, vue_imports) {
                        collect_standalone_props_call(projection, base, props_call);
                    }
                }
            }
            _ => {}
        }
    }
}

/// True when every name bound by `id` already has a read row.
fn declarator_bound(projection: &BindingViewsProjection, id: &BindingPattern<'_>) -> bool {
    let names = pattern_names(id, 0);
    !names.is_empty()
        && names
            .iter()
            .all(|(name, _)| projection.read.lookup(name).is_some())
}
