use std::fmt::Write as _;

use verter_audit::observe::ObserveExec;
use verter_span::Span;

use crate::ast::types::{AstNodeKind, ElementNode, PropFlags};
use crate::framework::vue::semantic::VueSemanticSnapshot;
use crate::framework_common::structure::{
    Arena, FlatArena, FlatRange, Id, Semantic, StructureError, TextId, TextInterner,
};
use crate::template::topology::{
    RegionCompleteness, VueBranchRole, VueTemplateNodeId, VueTemplateNodeKind, VueTemplateTopology,
};
use crate::types::NodeProp;
use crate::utils::vue::tag::{is_mathml_tag, is_raw_text_tag, is_svg_tag, is_void_tag};
use crate::work_ledger::{CompilerWorkLedger, WorkKind};

/// Collection marker of compile nodes.
pub enum VueCompileNodeKey {}
/// Dense identity of a node admitted to compilation. Its index equals the
/// index of the topology node it describes.
pub type VueCompileNodeId = Id<VueCompileNodeKey>;

pub enum VueAttrKey {}
pub enum VueChildKey {}

/// Hot per-node classification bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VueNodeFlags(pub u16);

impl VueNodeFlags {
    pub const FULLY_STATIC: u16 = 1;
    pub const HAS_KEY: u16 = 1 << 1;
    pub const HAS_REF: u16 = 1 << 2;
    pub const HAS_SPREAD: u16 = 1 << 3;
    pub const HAS_CUSTOM_DIRECTIVE: u16 = 1 << 4;
    pub const HAS_V_ONCE: u16 = 1 << 5;
    pub const HAS_V_MEMO: u16 = 1 << 6;
    pub const VOID_ELEMENT: u16 = 1 << 7;
    pub const RAW_TEXT_CHILDREN: u16 = 1 << 8;
    pub const SVG_NAMESPACE: u16 = 1 << 9;
    pub const MATHML_NAMESPACE: u16 = 1 << 10;
    pub const WHITESPACE_ONLY_TEXT: u16 = 1 << 11;
    /// Text in this subtree keeps authored whitespace.
    pub const PRESERVE_WHITESPACE: u16 = 1 << 12;

    pub const fn has(self, bit: u16) -> bool {
        self.0 & bit != 0
    }
    pub const fn with(self, bit: u16) -> Self {
        Self(self.0 | bit)
    }
}

/// Per-node record. `Copy`; variable-size data lives in the flat tables.
#[derive(Clone, Copy)]
pub struct VueNodeFacts {
    pub flags: VueNodeFlags,
    /// Interned tag name; `None` for non-element nodes.
    pub tag: Option<TextId>,
    pub attrs: FlatRange<VueAttrKey>,
    pub children: FlatRange<VueChildKey>,
}

/// The built-in directives, as classified by the parser's one directive
/// classifier.
pub type VueBuiltinDirective = verter_parser::types::BuiltinDirective;

/// What an attribute or directive stands for, as classified by the parser's
/// one directive classifier.
///
/// - `Static`: plain attribute.
/// - `Bind`: `v-bind:name` / `:name`; `v-bind="obj"` when the name is absent.
/// - `On`: `v-on:name` / `@name`; `v-on="obj"` when the name is absent.
/// - `Builtin`: a directive with a fixed meaning.
/// - `Custom`: a user directive.
pub type VueAttrKind = verter_parser::types::DirectiveKind;

/// One attribute or directive of an element, in source order.
#[derive(Debug, Clone, Copy)]
pub struct VueAttr {
    pub kind: VueAttrKind,
    /// Index into the parser element's `props`, or [`VueAttr::CACHED_PROP`]
    /// for a directive the parser keeps on the element instead (`v-if`,
    /// `v-else-if`, `v-else`, `v-for`, `v-slot`, `v-once`, `ref`).
    pub prop_index: u16,
    /// Interned static name or directive argument; `None` when dynamic or
    /// absent. A custom directive carries its name without the `v-`.
    pub name: Option<TextId>,
    pub name_span: Span,
    pub arg_span: Option<Span>,
    pub value_span: Option<Span>,
    pub dynamic_arg: bool,
    pub modifier_count: u8,
}

impl VueAttr {
    /// `prop_index` of a directive the parser caches on the element.
    pub const CACHED_PROP: u16 = u16::MAX;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VueChildKind {
    Text,
    Interpolation,
    Element,
    Comment,
    /// Whitespace that contains a line break.
    WhitespaceNewline,
    /// Whitespace on one line.
    WhitespaceSpace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VueChainRole {
    None,
    Start,
    Continuation,
}

/// One child of a node after whitespace resolution. Children dropped by
/// the whitespace policy do not appear.
#[derive(Debug, Clone, Copy)]
pub struct VueChildRecord {
    pub node: VueCompileNodeId,
    pub kind: VueChildKind,
    pub chain: VueChainRole,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VueStructureRefusal {
    /// The topology is recovered or incomplete; only complete templates
    /// are admitted to compilation.
    IncompleteTopology,
    Storage(StructureError),
}

impl From<StructureError> for VueStructureRefusal {
    fn from(error: StructureError) -> Self {
        Self::Storage(error)
    }
}

/// The compile structure of one template.
pub struct VueCompileStructure {
    pub(super) nodes: Arena<VueCompileNodeKey, VueNodeFacts, Semantic>,
    pub(super) attrs: FlatArena<VueAttrKey, VueAttr, Semantic>,
    pub(super) children: FlatArena<VueChildKey, VueChildRecord, Semantic>,
    pub(super) names: TextInterner<Semantic>,
    pub(super) condense_whitespace: bool,
    /// Compile identity by topology index. The storage layer mints
    /// identities only by insertion, so the index lookup is kept here.
    pub(super) compile_ids: Vec<VueCompileNodeId>,
    pub(super) template_ids: Vec<VueTemplateNodeId>,
}

impl VueCompileStructure {
    pub fn len(&self) -> usize {
        self.nodes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
    /// The compile identity of a topology node. O(1).
    pub fn compile_id(&self, node: VueTemplateNodeId) -> VueCompileNodeId {
        self.compile_ids[node.index()]
    }
    /// The topology node a compile identity describes. O(1).
    pub fn template_id(&self, node: VueCompileNodeId) -> VueTemplateNodeId {
        self.template_ids[node.index()]
    }
    pub fn facts(&self, node: VueCompileNodeId) -> VueNodeFacts {
        *self
            .nodes
            .get(node)
            .expect("node id belongs to this structure")
    }
    pub fn attrs(&self, node: VueCompileNodeId) -> &[VueAttr] {
        self.attrs.get(self.facts(node).attrs)
    }
    /// First attribute of a kind with a static name.
    pub fn find_attr(
        &self,
        node: VueCompileNodeId,
        kind: VueAttrKind,
        name: &str,
    ) -> Option<&VueAttr> {
        self.attrs(node)
            .iter()
            .find(|attr| attr.kind == kind && attr.name.is_some_and(|n| self.text(n) == name))
    }
    /// Children after whitespace resolution, in source order.
    pub fn children(&self, node: VueCompileNodeId) -> &[VueChildRecord] {
        self.children.get(self.facts(node).children)
    }
    pub fn tag_name(&self, node: VueCompileNodeId) -> Option<&str> {
        self.facts(node).tag.map(|tag| self.text(tag))
    }
    pub fn text(&self, id: TextId) -> &str {
        self.names
            .resolve(id)
            .expect("text id belongs to this structure")
    }
}

/// Build the structure in one pass over the topology. Refuses anything but
/// a complete topology. The semantic snapshot is not read: nothing in the
/// tables depends on a binding or an expression.
pub fn build_compile_structure<M: ObserveExec>(
    source: &str,
    topology: &VueTemplateTopology<'_>,
    _semantic: &VueSemanticSnapshot<'_>,
    condense_whitespace: bool,
    comments: bool,
    ledger: &mut CompilerWorkLedger<M>,
) -> Result<VueCompileStructure, VueStructureRefusal> {
    build_tables(source, topology, condense_whitespace, comments, ledger)
}

pub(super) fn build_tables<M: ObserveExec>(
    source: &str,
    topology: &VueTemplateTopology<'_>,
    condense_whitespace: bool,
    comments: bool,
    ledger: &mut CompilerWorkLedger<M>,
) -> Result<VueCompileStructure, VueStructureRefusal> {
    if topology.completeness() != RegionCompleteness::Complete {
        return Err(VueStructureRefusal::IncompleteTopology);
    }
    let mut builder = Builder::new(source, topology, condense_whitespace, comments)?;
    for node in topology.nodes() {
        builder.push_node(node)?;
    }
    let structure = builder.finish();
    ledger.record(WorkKind::StructureNodesMaterialized, structure.len() as u64);
    Ok(structure)
}

/// A MathML text integration point: its children read as HTML.
fn is_mathml_text_point(tag: &str) -> bool {
    matches!(tag, "mi" | "mo" | "mn" | "ms" | "mtext")
}

/// Namespace of a node's children.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Namespace {
    Html,
    Svg,
    MathMl,
}

struct Builder<'a, 'p> {
    source: &'a str,
    topology: &'a VueTemplateTopology<'p>,
    comments: bool,
    out: VueCompileStructure,
    /// Namespace of each node's children, by topology index.
    child_namespace: Vec<Namespace>,
    /// Whether each node's children keep their whitespace (inside `pre`,
    /// `v-pre` and raw-text elements), by topology index.
    preformatted: Vec<bool>,
    /// Compile identities of the nodes, minted ahead of the node records so
    /// that a record can name its children, which come after it.
    ids: Vec<VueCompileNodeId>,
    candidates: Vec<(VueTemplateNodeId, VueChildKind)>,
}

impl<'a, 'p> Builder<'a, 'p> {
    fn new(
        source: &'a str,
        topology: &'a VueTemplateTopology<'p>,
        condense_whitespace: bool,
        comments: bool,
    ) -> Result<Self, StructureError> {
        let mut minted: Arena<VueCompileNodeKey, (), Semantic> = Arena::new();
        let mut ids = Vec::with_capacity(topology.len());
        for _ in 0..topology.len() {
            ids.push(minted.push(())?);
        }
        Ok(Self {
            source,
            topology,
            comments,
            out: VueCompileStructure {
                nodes: Arena::new(),
                attrs: FlatArena::new(),
                children: FlatArena::new(),
                names: TextInterner::new(),
                condense_whitespace,
                compile_ids: ids.clone(),
                template_ids: topology.nodes().collect(),
            },
            child_namespace: Vec::with_capacity(topology.len()),
            preformatted: Vec::with_capacity(topology.len()),
            ids,
            candidates: Vec::new(),
        })
    }

    fn finish(self) -> VueCompileStructure {
        self.out
    }

    fn push_node(&mut self, node: VueTemplateNodeId) -> Result<(), VueStructureRefusal> {
        let topology = self.topology;
        let parent_namespace = topology.parent(node).map_or(Namespace::Html, |parent| {
            self.child_namespace[parent.index()]
        });
        let mut flags = VueNodeFlags::default();
        let mut tag = None;
        let mut attrs = self.out.attrs.append([])?;
        let mut namespace = Namespace::Html;

        if let Some(element) = topology.element(node) {
            let name = self.tag_bytes(element)?;
            tag = Some(self.out.names.intern(name)?);
            let bytes = name.as_bytes();
            // As Vue 3.6.0-rc.15 compiler-dom `getNamespace` decides it: an
            // element read as HTML starts the SVG or MathML namespace when its
            // tag is an SVG or MathML tag (`<path>` or `<g>` with no `<svg>`
            // in the file, `mglyph` inside a MathML text integration point),
            // and every descendant inherits it until an integration point.
            namespace = match parent_namespace {
                Namespace::Html if is_svg_tag(bytes) => Namespace::Svg,
                Namespace::Html if is_mathml_tag(bytes) => Namespace::MathMl,
                inherited => inherited,
            };
            match namespace {
                Namespace::Svg => flags = flags.with(VueNodeFlags::SVG_NAMESPACE),
                Namespace::MathMl => flags = flags.with(VueNodeFlags::MATHML_NAMESPACE),
                Namespace::Html => {}
            }
            if element.tag_type.is_element() {
                if is_void_tag(bytes) {
                    flags = flags.with(VueNodeFlags::VOID_ELEMENT);
                }
                if is_raw_text_tag(bytes) {
                    flags = flags.with(VueNodeFlags::RAW_TEXT_CHILDREN);
                }
            }
            flags = element_flags(element, flags);
            let (range, has_memo) = self.push_attrs(element)?;
            attrs = range;
            if has_memo {
                flags = flags.with(VueNodeFlags::HAS_V_MEMO);
            }
        } else if let Some(AstNodeKind::Text(text)) = self.syntax_kind(node) {
            if text.is_whitespace_only {
                flags = flags.with(VueNodeFlags::WHITESPACE_ONLY_TEXT);
            }
        }

        // Children of an integration point read as HTML again.
        let children_namespace = match (namespace, tag.map(|t| self.out.text(t))) {
            (Namespace::Svg, Some("foreignObject" | "desc" | "title")) => Namespace::Html,
            (Namespace::MathMl, Some("annotation-xml")) => Namespace::Html,
            (Namespace::MathMl, Some(tag)) if is_mathml_text_point(tag) => Namespace::Html,
            (namespace, _) => namespace,
        };
        self.child_namespace.push(children_namespace);
        // Only an HTML `<pre>` is preformatted; an svg or MathML element
        // named `pre` condenses like any other.
        let html_pre =
            namespace == Namespace::Html && tag.is_some_and(|t| self.out.text(t) == "pre");
        let preformatted = topology
            .parent(node)
            .is_some_and(|parent| self.preformatted[parent.index()])
            || flags.has(VueNodeFlags::RAW_TEXT_CHILDREN)
            || html_pre
            || self
                .out
                .attrs
                .get(attrs)
                .iter()
                .any(|attr| attr.kind == VueAttrKind::Builtin(VueBuiltinDirective::Pre));
        self.preformatted.push(preformatted);
        let preserve_whitespace = topology.parent(node).is_some_and(|parent| {
            self.out
                .facts(self.ids[parent.index()])
                .flags
                .has(VueNodeFlags::PRESERVE_WHITESPACE)
        }) || flags.has(VueNodeFlags::RAW_TEXT_CHILDREN)
            || html_pre;
        if preserve_whitespace {
            flags = flags.with(VueNodeFlags::PRESERVE_WHITESPACE);
        }

        let children = self.push_children(node, preserve_whitespace)?;
        let pushed = self.out.nodes.push(VueNodeFacts {
            flags,
            tag,
            attrs,
            children,
        })?;
        verter_debug_assert_eq!(pushed, self.ids[node.index()]);
        Ok(())
    }

    fn tag_bytes(&self, element: &ElementNode) -> Result<&'a str, StructureError> {
        self.source
            .get(element.tag_open.start as usize + 1..element.tag_open.name_end as usize)
            .ok_or(StructureError::InvalidSpan)
    }

    fn syntax_kind(&self, node: VueTemplateNodeId) -> Option<&'p AstNodeKind> {
        let syntax = self.topology.syntax(node)?;
        self.topology
            .ast()
            .nodes
            .get(syntax.0)
            .map(|parsed| &parsed.kind)
    }

    /// The element's attributes and directives in source order. The parser
    /// keeps some directives outside `props`; they are merged back by
    /// position.
    fn push_attrs(
        &mut self,
        element: &ElementNode,
    ) -> Result<(FlatRange<VueAttrKey>, bool), VueStructureRefusal> {
        let mut cached: [Option<&NodeProp>; 5] = [None; 5];
        let mut count = 0;
        for prop in [
            element.v_condition.as_ref().map(|c| &c.prop),
            element.v_for.as_ref(),
            element.v_slot.as_ref(),
            element.v_once.as_ref(),
            element.v_ref.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            cached[count] = Some(prop);
            count += 1;
        }
        let cached = &mut cached[..count];
        cached.sort_unstable_by_key(|prop| prop.map(|p| p.start));

        let mut attrs = Vec::with_capacity(element.props.len() + count);
        let mut has_memo = false;
        let mut cached = cached.iter().flatten().peekable();
        let mut props = element.props.iter().enumerate().peekable();
        loop {
            let take_cached = match (cached.peek(), props.peek()) {
                (None, None) => break,
                (Some(_), None) => true,
                (None, Some(_)) => false,
                (Some(c), Some((_, p))) => c.start < p.start,
            };
            let (prop, index) = if take_cached {
                (*cached.next().expect("peeked"), VueAttr::CACHED_PROP)
            } else {
                let (index, prop) = props.next().expect("peeked");
                (
                    prop,
                    u16::try_from(index).map_err(|_| StructureError::Capacity)?,
                )
            };
            let attr = self.attr(prop, index)?;
            has_memo |= attr.kind == VueAttrKind::Builtin(VueBuiltinDirective::Memo);
            attrs.push(attr);
        }
        Ok((self.out.attrs.append(attrs)?, has_memo))
    }

    fn attr(&mut self, prop: &NodeProp, prop_index: u16) -> Result<VueAttr, StructureError> {
        let kind = prop.kind(self.source.as_bytes());
        let dynamic_arg = prop.is_dynamic == Some(true);
        let arg_span = prop
            .arg_start
            .zip(prop.arg_end)
            .map(|(start, end)| Span::new(start, end));
        let slice = |span: Span| {
            self.source
                .get(span.start as usize..span.end as usize)
                .ok_or(StructureError::InvalidSpan)
        };
        let name_span = Span::new(prop.start, prop.name_end);
        let name = match kind {
            VueAttrKind::Static => Some(slice(name_span)?),
            VueAttrKind::Custom => {
                let full = slice(name_span)?;
                Some(full.strip_prefix("v-").unwrap_or(full))
            }
            _ if dynamic_arg => None,
            _ => arg_span.map(slice).transpose()?,
        };
        let name = name.map(|text| self.out.names.intern(text)).transpose()?;
        Ok(VueAttr {
            kind,
            prop_index,
            name,
            name_span,
            arg_span,
            value_span: prop
                .value_start
                .zip(prop.value_end)
                .map(|(start, end)| Span::new(start, end)),
            dynamic_arg,
            modifier_count: u8::try_from(prop.modifiers.len()).unwrap_or(u8::MAX),
        })
    }

    /// The node's children after the whitespace rules.
    fn push_children(
        &mut self,
        node: VueTemplateNodeId,
        preserve_whitespace: bool,
    ) -> Result<FlatRange<VueChildKey>, VueStructureRefusal> {
        let topology = self.topology;
        let mut candidates = std::mem::take(&mut self.candidates);
        candidates.clear();
        for child in topology.children(node) {
            let Some(kind) = self.child_kind(child, preserve_whitespace) else {
                continue;
            };
            if kind == VueChildKind::Comment && !self.comments {
                continue;
            }
            candidates.push((child, kind));
        }
        merge_whitespace_runs(&mut candidates);
        if !self.preformatted[node.index()] {
            resolve_whitespace(&mut candidates, self.out.condense_whitespace);
        }
        strip_between_branches(topology, &mut candidates);
        let records: Vec<VueChildRecord> = candidates
            .iter()
            .map(|&(child, kind)| VueChildRecord {
                node: self.ids[child.index()],
                kind,
                chain: chain_role(topology, child),
            })
            .collect();
        self.candidates = candidates;
        Ok(self.out.children.append(records)?)
    }

    fn child_kind(
        &self,
        node: VueTemplateNodeId,
        preserve_whitespace: bool,
    ) -> Option<VueChildKind> {
        Some(match self.topology.kind(node) {
            VueTemplateNodeKind::Root => return None,
            VueTemplateNodeKind::Element
            | VueTemplateNodeKind::Component
            | VueTemplateNodeKind::SlotOutlet
            | VueTemplateNodeKind::Template => VueChildKind::Element,
            VueTemplateNodeKind::Interpolation => VueChildKind::Interpolation,
            VueTemplateNodeKind::Comment => VueChildKind::Comment,
            VueTemplateNodeKind::Text => {
                let span = self.topology.span(node);
                if span.start == span.end {
                    return None;
                }
                let Some(AstNodeKind::Text(_)) = self.syntax_kind(node) else {
                    return None;
                };
                let content = &self.source[span.start as usize..span.end as usize];
                let mut decoded = String::new();
                let content =
                    if crate::template::code_gen::shared::helpers::has_html_entities(content) {
                        crate::template::code_gen::shared::helpers::decode_html_entities_into(
                            &mut decoded,
                            content,
                        );
                        decoded.as_str()
                    } else {
                        content
                    };
                if preserve_whitespace
                    || !content
                        .bytes()
                        .all(crate::template::code_gen::shared::helpers::is_condense_ws)
                {
                    VueChildKind::Text
                } else if content.bytes().any(|b| b == b'\n' || b == b'\r') {
                    VueChildKind::WhitespaceNewline
                } else {
                    VueChildKind::WhitespaceSpace
                }
            }
        })
    }
}

fn element_flags(element: &ElementNode, flags: VueNodeFlags) -> VueNodeFlags {
    let mut bits = flags.0;
    let mut set = |on: bool, bit: u16| {
        if on {
            bits |= bit;
        }
    };
    set(element.is_fully_static, VueNodeFlags::FULLY_STATIC);
    set(
        element.prop_flag.has(PropFlags::HasDynamicKey),
        VueNodeFlags::HAS_KEY,
    );
    set(
        element.v_ref.is_some() || element.prop_flag.has(PropFlags::HasRef),
        VueNodeFlags::HAS_REF,
    );
    set(element.has_spread(), VueNodeFlags::HAS_SPREAD);
    set(
        element.prop_flag.has(PropFlags::HasCustomDirective),
        VueNodeFlags::HAS_CUSTOM_DIRECTIVE,
    );
    set(element.v_once.is_some(), VueNodeFlags::HAS_V_ONCE);
    VueNodeFlags(bits)
}

fn is_whitespace(kind: VueChildKind) -> bool {
    matches!(
        kind,
        VueChildKind::WhitespaceNewline | VueChildKind::WhitespaceSpace
    )
}

fn is_text_run(kind: VueChildKind) -> bool {
    kind == VueChildKind::Text || is_whitespace(kind)
}

/// Adjacent whitespace is one run of text, as Vue's parser reads it, so
/// two neighbours left by a dropped comment count as one.
fn merge_whitespace_runs(candidates: &mut Vec<(VueTemplateNodeId, VueChildKind)>) {
    let mut merged: Vec<(VueTemplateNodeId, VueChildKind)> = Vec::with_capacity(candidates.len());
    for &(node, kind) in candidates.iter() {
        match merged.last_mut() {
            Some((_, previous)) if is_whitespace(*previous) && is_whitespace(kind) => {
                if kind == VueChildKind::WhitespaceNewline {
                    *previous = VueChildKind::WhitespaceNewline;
                }
            }
            _ => merged.push((node, kind)),
        }
    }
    *candidates = merged;
}

/// Vue's whitespace rules for the children of one node. Whitespace with no
/// previous or no next sibling is dropped, and so is whitespace between two
/// comments, between a comment and an element, and, when it holds a line
/// break, between two elements. Whitespace beside another text node is part
/// of that text and stays. Without condensing only the first rule applies.
fn resolve_whitespace(candidates: &mut Vec<(VueTemplateNodeId, VueChildKind)>, condense: bool) {
    let mut keep = Vec::with_capacity(candidates.len());
    for (i, &(_, kind)) in candidates.iter().enumerate() {
        let drop = is_whitespace(kind) && {
            let prev = i.checked_sub(1).map(|p| candidates[p].1);
            let next = candidates.get(i + 1).map(|n| n.1);
            match (prev, next) {
                (Some(prev), _) if is_text_run(prev) => false,
                (_, Some(next)) if is_text_run(next) => false,
                (None, _) | (_, None) => true,
                (Some(prev), Some(next)) => {
                    condense
                        && match (prev, next) {
                            (
                                VueChildKind::Comment,
                                VueChildKind::Comment | VueChildKind::Element,
                            ) => true,
                            (VueChildKind::Element, VueChildKind::Comment) => true,
                            (VueChildKind::Element, VueChildKind::Element) => {
                                kind == VueChildKind::WhitespaceNewline
                            }
                            _ => false,
                        }
                }
            }
        };
        keep.push(!drop);
    }
    let mut flags = keep.into_iter();
    candidates.retain(|_| flags.next().expect("one flag per candidate"));
}

/// Between the branches of one `v-if` chain, comments, whitespace and text
/// are not rendered.
fn strip_between_branches(
    topology: &VueTemplateTopology<'_>,
    candidates: &mut Vec<(VueTemplateNodeId, VueChildKind)>,
) {
    let mut at = candidates.len();
    while at > 0 {
        at -= 1;
        if chain_role(topology, candidates[at].0) != VueChainRole::Continuation {
            continue;
        }
        while at > 0 && candidates[at - 1].1 != VueChildKind::Element {
            candidates.remove(at - 1);
            at -= 1;
        }
    }
}

/// The part a node plays in its parent's `v-if` chain.
fn chain_role(topology: &VueTemplateTopology<'_>, node: VueTemplateNodeId) -> VueChainRole {
    let Some((chain, index)) = topology.chain_of(node) else {
        return VueChainRole::None;
    };
    match topology.branches(chain)[index as usize].role {
        VueBranchRole::If => VueChainRole::Start,
        VueBranchRole::ElseIf | VueBranchRole::Else => VueChainRole::Continuation,
    }
}

/// Check every table invariant; the first violation as text.
pub fn verify_structure(
    structure: &VueCompileStructure,
    topology: &VueTemplateTopology<'_>,
) -> Result<(), String> {
    if structure.len() != topology.len() {
        return Err(format!(
            "{} compile nodes for {} topology nodes",
            structure.len(),
            topology.len()
        ));
    }
    for node in topology.nodes() {
        let id = structure.compile_id(node);
        if id.index() != node.index() || structure.template_id(id) != node {
            return Err(format!(
                "node {} identity does not round-trip",
                node.index()
            ));
        }
        let span = topology.span(node);
        for attr in structure.attrs(id) {
            let spans = [Some(attr.name_span), attr.arg_span, attr.value_span];
            for inner in spans.into_iter().flatten() {
                if inner.start > inner.end || inner.start < span.start || inner.end > span.end {
                    return Err(format!(
                        "node {} has an attribute range {}..{} outside {}..{}",
                        node.index(),
                        inner.start,
                        inner.end,
                        span.start,
                        span.end
                    ));
                }
            }
        }
        let mut previous_element: Option<VueTemplateNodeId> = None;
        for record in structure.children(id) {
            let child = structure.template_id(record.node);
            if topology.parent(child) != Some(node) {
                return Err(format!(
                    "child record {} is not a child of node {}",
                    child.index(),
                    node.index()
                ));
            }
            let expected = chain_role(topology, child);
            if record.chain != expected {
                return Err(format!(
                    "child record {} has chain role {:?}, expected {:?}",
                    child.index(),
                    record.chain,
                    expected
                ));
            }
            if record.chain == VueChainRole::Continuation {
                let same_chain = previous_element
                    .and_then(|prev| topology.chain_of(prev))
                    .zip(topology.chain_of(child))
                    .is_some_and(|(prev, this)| prev.0 == this.0);
                if !same_chain {
                    return Err(format!(
                        "continuation {} does not follow its chain",
                        child.index()
                    ));
                }
            }
            if record.kind == VueChildKind::Element {
                previous_element = Some(child);
            }
        }
    }
    Ok(())
}

/// Deterministic text rendering for review and tests.
pub fn dump_structure(
    structure: &VueCompileStructure,
    topology: &VueTemplateTopology<'_>,
) -> String {
    let mut out = String::new();
    for node in topology.nodes() {
        let id = structure.compile_id(node);
        let facts = structure.facts(id);
        let _ = write!(out, "#{} {:?}", id.index(), topology.kind(node));
        if let Some(tag) = structure.tag_name(id) {
            let _ = write!(out, " <{tag}>");
        }
        let _ = write!(out, " flags={:#06x}", facts.flags.0);
        for attr in structure.attrs(id) {
            let _ = write!(out, " [{:?}", attr.kind);
            if let Some(name) = attr.name {
                let _ = write!(out, " {}", structure.text(name));
            }
            if attr.dynamic_arg {
                out.push_str(" dynamic");
            }
            if attr.modifier_count > 0 {
                let _ = write!(out, " mods={}", attr.modifier_count);
            }
            out.push(']');
        }
        if !structure.children(id).is_empty() {
            out.push_str(" children=");
            for (i, record) in structure.children(id).iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                let _ = write!(out, "{}:{:?}", record.node.index(), record.kind);
                match record.chain {
                    VueChainRole::None => {}
                    role => {
                        let _ = write!(out, "/{role:?}");
                    }
                }
            }
        }
        out.push('\n');
    }
    out
}

const _: () = {
    assert!(std::mem::size_of::<VueNodeFacts>() <= 24);
    assert!(std::mem::size_of::<VueChildRecord>() <= 8);
};
