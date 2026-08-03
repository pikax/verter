use std::collections::HashMap;
use std::sync::Arc;

use verter_language::carrier_grammar::{AcceptedRegisteredCarrierSource, CarrierGrammarConfig};
use verter_language::parse_artifact::carrier_inventory::*;
use verter_language::{
    compute_carrier_structure_hash, CarrierParse, CarrierStructureHash, FrameworkAdapterId,
    LanguageId,
};
use verter_span::Span;

use super::carrier_compiler::{CarrierCompiler, ParseOptions};
use super::registered_projector_seal::RegisteredProjectorSeal;
use super::vue_bridge::VueCarrierCompiler;
use crate::svelte::SvelteCarrierCompiler;

/// Opaque in-process carrier retained by the registered projector.
///
/// Only immutable producer metadata is observable. The erased carrier has no
/// accessor or downcast surface, and this type has no serialization, equality,
/// or hashing implementation that could turn it into publication identity.
#[derive(Clone)]
pub struct RegisteredCarrierPayload {
    inner: Arc<RegisteredCarrierPayloadInner>,
}

struct RegisteredCarrierPayloadInner {
    carrier: Arc<dyn CarrierParse>,
    adapter_id: FrameworkAdapterId,
    language_id: LanguageId,
    parser_version: u32,
}

impl RegisteredCarrierPayload {
    fn new(
        carrier: Arc<dyn CarrierParse>,
        adapter_id: FrameworkAdapterId,
        language_id: LanguageId,
        parser_version: u32,
    ) -> Self {
        Self {
            inner: Arc::new(RegisteredCarrierPayloadInner {
                carrier,
                adapter_id,
                language_id,
                parser_version,
            }),
        }
    }

    /// Adapter that owns the retained parse.
    #[must_use]
    pub fn adapter_id(&self) -> &FrameworkAdapterId {
        &self.inner.adapter_id
    }

    /// Carrier language parsed by the owning adapter.
    #[must_use]
    pub fn language_id(&self) -> &LanguageId {
        &self.inner.language_id
    }

    /// Parser version that produced the retained parse.
    #[must_use]
    pub fn parser_version(&self) -> u32 {
        self.inner.parser_version
    }

    #[cfg(test)]
    fn points_to(&self, carrier: &Arc<dyn CarrierParse>) -> bool {
        Arc::ptr_eq(&self.inner.carrier, carrier)
    }
}

#[doc(hidden)]
pub struct RegisteredCarrierProjection {
    carrier: RegisteredCarrierPayload,
    inventory: Arc<CarrierBlockInventory>,
    carrier_structure_hash: CarrierStructureHash,
}

impl RegisteredCarrierProjection {
    #[cfg(test)]
    fn carrier(&self) -> &RegisteredCarrierPayload {
        &self.carrier
    }

    #[cfg(test)]
    fn inventory(&self) -> &Arc<CarrierBlockInventory> {
        &self.inventory
    }

    /// Consume the exact projector result into the registered artifact.
    #[doc(hidden)]
    pub fn into_framework_parse_artifact(self) -> verter_language::FrameworkParseArtifact {
        let Self {
            carrier,
            inventory,
            carrier_structure_hash,
        } = self;
        verter_language::FrameworkParseArtifact::__from_registered_projection(
            carrier.inner.adapter_id.clone(),
            carrier.inner.language_id.clone(),
            carrier.inner.parser_version,
            inventory,
            carrier_structure_hash,
            Arc::clone(&carrier.inner.carrier),
        )
    }
}

/// Capability-sealed registered projection entry. Architecture guards restrict
/// its sole non-test caller to the elected session store leader.
#[doc(hidden)]
pub fn __project_registered_carrier_for_store_leader(
    compiler: &dyn CarrierCompiler,
    accepted: &AcceptedRegisteredCarrierSource,
) -> RegisteredCarrierProjection {
    let seal = super::registered_projector_seal::mint_registered_projector_seal_for_store_leader();
    project_registered_carrier(compiler, accepted, &seal)
}

fn project_registered_carrier(
    compiler: &dyn CarrierCompiler,
    accepted: &AcceptedRegisteredCarrierSource,
    _seal: &RegisteredProjectorSeal,
) -> RegisteredCarrierProjection {
    project_registered_carrier_with_witness(compiler, accepted, _seal).0
}

fn project_registered_carrier_with_witness(
    compiler: &dyn CarrierCompiler,
    accepted: &AcceptedRegisteredCarrierSource,
    _seal: &RegisteredProjectorSeal,
) -> (RegisteredCarrierProjection, Arc<dyn CarrierParse>) {
    let language = accepted.source().resolved_file_language();
    assert_eq!(
        compiler.adapter_id(),
        *language.adapter_id().expect("accepted carrier adapter")
    );
    assert_eq!(
        compiler.carrier_language_id(),
        *language
            .carrier_language_id()
            .expect("accepted carrier language")
    );
    let options = match accepted.grammar().canonical_config() {
        CarrierGrammarConfig::Vue {
            delimiters,
            custom_elements,
        } => ParseOptions {
            delimiters: Some((
                delimiters.open().to_string(),
                delimiters.close().to_string(),
            )),
            custom_elements: Some(
                custom_elements
                    .iter()
                    .map(|name| name.as_str().to_string())
                    .collect(),
            ),
        },
        CarrierGrammarConfig::Svelte => ParseOptions::default(),
    };
    let artifact = compiler.parse(accepted.source().bytes(), &options);
    assert_eq!(artifact.adapter_id, compiler.adapter_id());
    assert_eq!(artifact.language_id, compiler.carrier_language_id());
    let (inventory, parsed_carrier): (CarrierBlockInventory, Arc<dyn CarrierParse>) =
        if let Some(vue) = compiler
            .__verter_as_any()
            .downcast_ref::<VueCarrierCompiler>()
        {
            (
                project_vue(vue, accepted, &artifact),
                vue.carrier_arc(&artifact).expect("Vue carrier payload"),
            )
        } else if let Some(svelte) = compiler
            .__verter_as_any()
            .downcast_ref::<SvelteCarrierCompiler>()
        {
            (
                project_svelte(svelte, accepted, &artifact),
                svelte
                    .carrier_arc(&artifact)
                    .expect("Svelte carrier payload"),
            )
        } else {
            panic!("registered compiler lacks a closed carrier inventory projector")
        };
    let inventory = Arc::new(inventory);
    let carrier_structure_hash = compute_carrier_structure_hash(&inventory);
    let carrier = RegisteredCarrierPayload::new(
        Arc::clone(&parsed_carrier),
        artifact.adapter_id.clone(),
        artifact.language_id.clone(),
        artifact.parser_version,
    );
    (
        RegisteredCarrierProjection {
            carrier,
            inventory,
            carrier_structure_hash,
        },
        parsed_carrier,
    )
}

#[cfg(test)]
pub(super) fn project_registered_carrier_for_tests(
    compiler: &dyn CarrierCompiler,
    accepted: &AcceptedRegisteredCarrierSource,
    seal: &RegisteredProjectorSeal,
) -> (
    RegisteredCarrierPayload,
    Arc<CarrierBlockInventory>,
    CarrierStructureHash,
    bool,
) {
    let (projection, parsed_carrier) =
        project_registered_carrier_with_witness(compiler, accepted, seal);
    let same_carrier_arc = projection.carrier().points_to(&parsed_carrier);
    let RegisteredCarrierProjection {
        carrier,
        inventory,
        carrier_structure_hash,
    } = projection;
    (carrier, inventory, carrier_structure_hash, same_carrier_arc)
}

#[cfg(test)]
pub(super) fn materialize_registered_carrier_for_tests(
    compiler: &dyn CarrierCompiler,
    accepted: &AcceptedRegisteredCarrierSource,
    seal: &RegisteredProjectorSeal,
) -> (bool, bool) {
    let (projection, parsed_carrier) =
        project_registered_carrier_with_witness(compiler, accepted, seal);
    let inventory = Arc::clone(projection.inventory());
    let artifact = projection.into_framework_parse_artifact();
    let inventory_is_exact = Arc::ptr_eq(&inventory, &artifact.common.inventory);
    let materialized_carrier: Arc<dyn CarrierParse> = if let Some(vue) = compiler
        .__verter_as_any()
        .downcast_ref::<VueCarrierCompiler>(
    ) {
        vue.carrier_arc(&artifact)
            .expect("materialized Vue carrier")
    } else if let Some(svelte) = compiler
        .__verter_as_any()
        .downcast_ref::<SvelteCarrierCompiler>()
    {
        svelte
            .carrier_arc(&artifact)
            .expect("materialized Svelte carrier")
    } else {
        panic!("test compiler lacks carrier witness")
    };
    (
        Arc::ptr_eq(&parsed_carrier, &materialized_carrier),
        inventory_is_exact,
    )
}

struct Builder<'a> {
    source: &'a str,
    names: Vec<Arc<str>>,
    name_ids: HashMap<Arc<str>, InternedNameId>,
    attributes: u32,
    nodes: Vec<MarkupSyntaxNode>,
    child_ids: Vec<MarkupNodeId>,
    roots: Vec<MarkupNodeId>,
}

impl<'a> Builder<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            names: Vec::new(),
            name_ids: HashMap::new(),
            attributes: 0,
            nodes: Vec::new(),
            child_ids: Vec::new(),
            roots: Vec::new(),
        }
    }
    fn span(&self, span: Span) -> SourceSpan {
        SourceSpan::new(SourceSpaceId(0), span.start, span.end)
    }
    fn raw_span(&self, start: u32, end: u32) -> SourceSpan {
        SourceSpan::new(SourceSpaceId(0), start, end)
    }
    fn slice(&self, span: Span) -> SourceSlice {
        SourceSlice::new(self.span(span))
    }
    fn intern(&mut self, value: &str) -> InternedNameId {
        if let Some(id) = self.name_ids.get(value) {
            return *id;
        }
        let value: Arc<str> = Arc::from(value);
        let id = InternedNameId(self.names.len() as u32);
        self.names.push(Arc::clone(&value));
        self.name_ids.insert(value, id);
        id
    }
    fn attribute_id(&mut self) -> AttributeId {
        let id = AttributeId(self.attributes);
        self.attributes += 1;
        id
    }
    fn finish(
        self,
        accepted: &AcceptedRegisteredCarrierSource,
        blocks: Vec<CarrierBlock>,
    ) -> CarrierBlockInventory {
        CarrierBlockInventory::new(
            Arc::from([SourceSpaceDescriptor::registered(
                SourceSpaceId(0),
                accepted.source(),
            )]),
            Arc::new(NormalizedNameTable {
                values: Arc::from(self.names),
            }),
            Arc::from(blocks),
            Arc::new(MarkupSyntaxArena {
                roots: Arc::from(self.roots),
                nodes: Arc::from(self.nodes),
                child_ids: Arc::from(self.child_ids),
            }),
        )
        .expect("compiler projector must produce a valid inventory")
    }
    fn add_node(
        &mut self,
        root_block: BlockId,
        parent: Option<MarkupNodeId>,
        kind: MarkupNodeKind,
        children: Vec<MarkupNodeId>,
    ) -> MarkupNodeId {
        let start = self.child_ids.len() as u32;
        self.child_ids.extend(children);
        let end = self.child_ids.len() as u32;
        let id = MarkupNodeId(self.nodes.len() as u32);
        self.nodes.push(MarkupSyntaxNode {
            id,
            root_block,
            parent,
            children: start..end,
            kind,
        });
        id
    }
}

fn project_vue(
    vue: &VueCarrierCompiler,
    accepted: &AcceptedRegisteredCarrierSource,
    artifact: &verter_language::FrameworkParseArtifact,
) -> CarrierBlockInventory {
    use verter_parser::parser::types::RootNodeKind;
    let parsed = vue.parsed_sfc(artifact).expect("Vue artifact");
    enum Root<'a> {
        Script(&'a verter_parser::parser::types::RootNodeScript),
        Template(&'a verter_parser::ast::types::TemplateAst),
        Style(&'a verter_parser::parser::types::RootNodeStyle),
        Custom(&'a verter_parser::parser::types::RootNodeUnknown),
    }
    impl Root<'_> {
        fn start(&self) -> u32 {
            match self {
                Self::Script(v) => v.tag_open.start,
                Self::Template(v) => v.root.tag_open.start,
                Self::Style(v) => v.tag_open.start,
                Self::Custom(v) => v.tag_open.start,
            }
        }
    }
    let mut roots = Vec::new();
    roots.extend(parsed.script().map(Root::Script));
    roots.extend(parsed.script_setup().map(Root::Script));
    roots.extend(parsed.template_ast().map(Root::Template));
    roots.extend(parsed.style_nodes().iter().map(Root::Style));
    roots.extend(parsed.unknown_nodes().iter().map(Root::Custom));
    roots.sort_by_key(Root::start);
    let mut builder = Builder::new(accepted.source().bytes());
    let mut blocks = Vec::new();
    for root in roots {
        let id = BlockId(blocks.len() as u32);
        match root {
            Root::Script(v) => {
                let dialect = ScriptSourceType::from(super::vue_bridge::vue_script_source_type(
                    parsed,
                    accepted.source().bytes(),
                ));
                let role = if v.is_setup {
                    ScriptRole::Setup
                } else {
                    ScriptRole::Module
                };
                let content = v.content.map(|span| builder.span(span));
                let closing = v
                    .tag_close
                    .as_ref()
                    .map(|tag| builder.raw_span(tag.start, tag.end));
                let closing_name = v
                    .tag_close
                    .as_ref()
                    .map(|tag| builder.raw_span(tag.start + 2, tag.name_end));
                let syntax = vue_tagged(
                    &mut builder,
                    "script",
                    VueTaggedSpans {
                        start: v.tag_open.start,
                        name_end: v.tag_open.name_end,
                        open_end: v.tag_open.end,
                        content,
                        close: closing,
                        close_name: closing_name,
                    },
                    &v.attributes,
                );
                blocks.push(CarrierBlock::Section {
                    id,
                    role: SectionRole::Script { role, dialect },
                    syntax,
                });
            }
            Root::Template(ast) => {
                let v = &ast.root;
                let content = v.content.as_ref().map(|c| builder.raw_span(c.start, c.end));
                let closing = v
                    .tag_close
                    .as_ref()
                    .map(|tag| builder.raw_span(tag.start, tag.end));
                let closing_name = v
                    .tag_close
                    .as_ref()
                    .map(|tag| builder.raw_span(tag.start + 2, tag.name_end));
                let syntax = vue_tagged(
                    &mut builder,
                    "template",
                    VueTaggedSpans {
                        start: v.tag_open.start,
                        name_end: v.tag_open.name_end,
                        open_end: v.tag_open.end,
                        content,
                        close: closing,
                        close_name: closing_name,
                    },
                    &v.attributes,
                );
                blocks.push(CarrierBlock::Section {
                    id,
                    role: SectionRole::TemplateHost,
                    syntax,
                });
                if let Some(content) = &v.content {
                    for child in &content.children {
                        let node = project_vue_node(
                            &mut builder,
                            ast,
                            *child,
                            id,
                            None,
                            MarkupNamespace::Html,
                        );
                        builder.roots.push(node);
                    }
                }
            }
            Root::Style(v) => {
                let dialect = match v.lang {
                    Some(verter_parser::parser::types::StyleLang::Css) => StyleDialect::Css,
                    Some(verter_parser::parser::types::StyleLang::Scss) => StyleDialect::Scss,
                    Some(verter_parser::parser::types::StyleLang::Sass) => StyleDialect::Sass,
                    Some(verter_parser::parser::types::StyleLang::Less) => StyleDialect::Less,
                    Some(verter_parser::parser::types::StyleLang::Stylus) => StyleDialect::Stylus,
                    Some(verter_parser::parser::types::StyleLang::Unknown) => StyleDialect::Missing,
                    None => StyleDialect::Css,
                };
                let content = v.content.map(|span| builder.span(span));
                let closing = v
                    .tag_close
                    .as_ref()
                    .map(|tag| builder.raw_span(tag.start, tag.end));
                let closing_name = v
                    .tag_close
                    .as_ref()
                    .map(|tag| builder.raw_span(tag.start + 2, tag.name_end));
                let syntax = vue_tagged(
                    &mut builder,
                    "style",
                    VueTaggedSpans {
                        start: v.tag_open.start,
                        name_end: v.tag_open.name_end,
                        open_end: v.tag_open.end,
                        content,
                        close: closing,
                        close_name: closing_name,
                    },
                    &v.attributes,
                );
                blocks.push(CarrierBlock::Section {
                    id,
                    role: SectionRole::Style {
                        dialect,
                        scoped: v.scoped,
                        module: if v.module {
                            StyleModule::Default
                        } else {
                            StyleModule::None
                        },
                    },
                    syntax,
                });
            }
            Root::Custom(v) => {
                let name =
                    &builder.source[v.tag_open.start as usize + 1..v.tag_open.name_end as usize];
                let normalized = name.to_ascii_lowercase();
                let content = v.content.map(|span| builder.span(span));
                let closing = v
                    .tag_close
                    .as_ref()
                    .map(|tag| builder.raw_span(tag.start, tag.end));
                let closing_name = v
                    .tag_close
                    .as_ref()
                    .map(|tag| builder.raw_span(tag.start + 2, tag.name_end));
                let syntax = vue_tagged(
                    &mut builder,
                    name,
                    VueTaggedSpans {
                        start: v.tag_open.start,
                        name_end: v.tag_open.name_end,
                        open_end: v.tag_open.end,
                        content,
                        close: closing,
                        close_name: closing_name,
                    },
                    &v.attributes,
                );
                blocks.push(CarrierBlock::Section {
                    id,
                    role: SectionRole::Custom {
                        normalized_name: Arc::from(normalized),
                    },
                    syntax,
                });
            }
        }
    }
    let _ = RootNodeKind::Unknown;
    builder.finish(accepted, blocks)
}

struct VueTaggedSpans {
    start: u32,
    name_end: u32,
    open_end: u32,
    content: Option<SourceSpan>,
    close: Option<SourceSpan>,
    close_name: Option<SourceSpan>,
}

fn vue_tagged(
    builder: &mut Builder<'_>,
    name: &str,
    spans: VueTaggedSpans,
    props: &[verter_parser::types::NodeProp],
) -> TaggedSyntax {
    let VueTaggedSpans {
        start,
        name_end,
        open_end,
        content,
        close,
        close_name,
    } = spans;
    let name_span = builder.raw_span(start + 1, name_end);
    let normalized = builder.intern(&name.to_ascii_lowercase());
    let attributes = vue_attributes(builder, props);
    let content = content.unwrap_or(builder.raw_span(open_end, open_end));
    let full_end = close.map(|s| s.end).unwrap_or(content.end);
    TaggedSyntax {
        authored_name: SourceSlice::new(name_span),
        normalized_name: normalized,
        opening_span: builder.raw_span(start, open_end),
        opening_name_span: name_span,
        attribute_insertion_anchor: builder
            .raw_span(open_end.saturating_sub(1), open_end.saturating_sub(1)),
        content_span: content,
        closing_span: close,
        closing_name_span: close_name,
        full_span: builder.raw_span(start, full_end),
        termination: if close.is_some() {
            SyntaxTermination::Closed
        } else {
            SyntaxTermination::UnclosedEof
        },
        attributes: Arc::from(attributes),
    }
}

fn project_vue_node(
    builder: &mut Builder<'_>,
    ast: &verter_parser::ast::types::TemplateAst,
    id: verter_parser::types::NodeId,
    root_block: BlockId,
    parent: Option<MarkupNodeId>,
    parent_namespace: MarkupNamespace,
) -> MarkupNodeId {
    use verter_parser::ast::types::AstNodeKind;
    let placeholder = MarkupNodeId(builder.nodes.len() as u32);
    builder.nodes.push(MarkupSyntaxNode {
        id: placeholder,
        root_block,
        parent,
        children: 0..0,
        kind: MarkupNodeKind::Text {
            content_span: builder.raw_span(0, 0),
        },
    });
    let node = &ast.nodes[id.0];
    let (kind, children) = match &node.kind {
        AstNodeKind::Text(v) => (
            MarkupNodeKind::Text {
                content_span: builder.raw_span(v.start, v.end),
            },
            vec![],
        ),
        AstNodeKind::Comment(v) => (
            MarkupNodeKind::Comment {
                opening_span: builder.raw_span(v.start, v.content_start),
                content_span: builder.raw_span(v.content_start, v.content_end),
                closing_span: (v.content_end < v.end)
                    .then(|| builder.raw_span(v.content_end, v.end)),
                full_span: builder.raw_span(v.start, v.end),
                termination: if v.content_end < v.end {
                    SyntaxTermination::Closed
                } else {
                    SyntaxTermination::UnclosedEof
                },
            },
            vec![],
        ),
        AstNodeKind::Interpolation(v) => (
            MarkupNodeKind::Interpolation {
                family: MarkupInterpolationFamily::VueInterpolation,
                opening_span: builder.raw_span(v.start, v.inner_start),
                expression_span: builder.raw_span(v.inner_start, v.inner_end),
                closing_span: (v.inner_end < v.end).then(|| builder.raw_span(v.inner_end, v.end)),
                full_span: builder.raw_span(v.start, v.end),
                termination: if v.inner_end < v.end {
                    SyntaxTermination::Closed
                } else {
                    SyntaxTermination::UnclosedEof
                },
            },
            vec![],
        ),
        AstNodeKind::Element(v) => {
            let name_span = builder.raw_span(v.tag_open.start + 1, v.tag_open.name_end);
            let name = builder.source[name_span.start as usize..name_span.end as usize].to_string();
            let lower_name = name.to_ascii_lowercase();
            let namespace = match lower_name.as_str() {
                "svg" => MarkupNamespace::Svg,
                "math" => MarkupNamespace::MathMl,
                _ => parent_namespace,
            };
            let parser_known_native =
                verter_parser::utils::vue::tag::is_html_tag(lower_name.as_bytes())
                    || verter_parser::utils::vue::tag::is_svg_tag(lower_name.as_bytes())
                    || verter_parser::utils::vue::tag::is_mathml_tag(lower_name.as_bytes());
            let normalized = if v.tag_type.is_component() && !parser_known_native {
                builder.intern(&name)
            } else {
                builder.intern(&lower_name)
            };
            let content = v
                .content
                .as_ref()
                .map(|c| builder.raw_span(c.start, c.end))
                .unwrap_or(builder.raw_span(v.tag_open.end, v.tag_open.end));
            let close = v
                .tag_close
                .as_ref()
                .map(|t| builder.raw_span(t.start, t.end));
            let close_name = v
                .tag_close
                .as_ref()
                .map(|t| builder.raw_span(t.start + 2, t.name_end));
            let full_end = close.map(|s| s.end).unwrap_or(content.end);
            let mut props: Vec<&verter_parser::types::NodeProp> = v.props.iter().collect();
            if let Some(c) = &v.v_condition {
                props.push(&c.prop)
            }
            if let Some(p) = &v.v_for {
                props.push(p)
            }
            if let Some(p) = &v.v_slot {
                props.push(p)
            }
            if let Some(p) = &v.v_once {
                props.push(p)
            }
            if let Some(p) = &v.v_ref {
                props.push(p)
            }
            props.sort_by_key(|p| p.start);
            let attributes = vue_attributes_refs(builder, &props);
            let child_parser_ids = v
                .content
                .as_ref()
                .map(|c| c.children.as_slice())
                .unwrap_or(&[]);
            let child_namespace =
                if namespace == MarkupNamespace::Svg && lower_name == "foreignobject" {
                    MarkupNamespace::Html
                } else {
                    namespace
                };
            let children = child_parser_ids
                .iter()
                .map(|child| {
                    project_vue_node(
                        builder,
                        ast,
                        *child,
                        root_block,
                        Some(placeholder),
                        child_namespace,
                    )
                })
                .collect();
            let void_element = namespace == MarkupNamespace::Html && is_void_html(&lower_name);
            (
                MarkupNodeKind::Element(MarkupElementSyntax {
                    authored_name: SourceSlice::new(name_span),
                    normalized_name: normalized,
                    namespace,
                    kind: if lower_name == "component" {
                        MarkupElementKind::DynamicComponent
                    } else if v.tag_type.is_component() && !parser_known_native {
                        MarkupElementKind::Component
                    } else {
                        MarkupElementKind::Native
                    },
                    opening_span: builder.raw_span(v.tag_open.start, v.tag_open.end),
                    opening_name_span: name_span,
                    attribute_insertion_anchor: builder.raw_span(
                        v.tag_open
                            .end
                            .saturating_sub(if v.is_self_closing { 2 } else { 1 }),
                        v.tag_open
                            .end
                            .saturating_sub(if v.is_self_closing { 2 } else { 1 }),
                    ),
                    content_span: content,
                    closing_span: close,
                    closing_name_span: close_name,
                    full_span: builder.raw_span(v.tag_open.start, full_end),
                    self_closing: v.is_self_closing,
                    void_element,
                    raw_text: matches!(
                        name.to_ascii_lowercase().as_str(),
                        "script" | "style" | "textarea"
                    ),
                    termination: if void_element {
                        SyntaxTermination::Void
                    } else if v.is_self_closing {
                        SyntaxTermination::SelfClosing
                    } else if close.is_some() {
                        SyntaxTermination::Closed
                    } else {
                        SyntaxTermination::UnclosedEof
                    },
                    attributes: Arc::from(attributes),
                }),
                children,
            )
        }
    };
    let start = builder.child_ids.len() as u32;
    builder.child_ids.extend(children);
    let end = builder.child_ids.len() as u32;
    builder.nodes[placeholder.0 as usize] = MarkupSyntaxNode {
        id: placeholder,
        root_block,
        parent,
        children: start..end,
        kind,
    };
    placeholder
}

fn is_void_html(name: &str) -> bool {
    matches!(
        name,
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

fn vue_attributes(
    builder: &mut Builder<'_>,
    props: &[verter_parser::types::NodeProp],
) -> Vec<CarrierAttribute> {
    let refs = props.iter().collect::<Vec<_>>();
    vue_attributes_refs(builder, &refs)
}
fn vue_attributes_refs(
    builder: &mut Builder<'_>,
    props: &[&verter_parser::types::NodeProp],
) -> Vec<CarrierAttribute> {
    let mut duplicates: HashMap<String, AttributeId> = HashMap::new();
    props
        .iter()
        .map(|p| vue_attribute(builder, p, &mut duplicates))
        .collect()
}
fn vue_attribute(
    builder: &mut Builder<'_>,
    p: &verter_parser::types::NodeProp,
    duplicates: &mut HashMap<String, AttributeId>,
) -> CarrierAttribute {
    let id = builder.attribute_id();
    let name_text = builder.source[p.start as usize..p.name_end as usize].to_string();
    let full_end = attribute_full_end(builder.source, p);
    let full = builder.raw_span(p.start, full_end);
    if p.is_directive {
        let (family, prefix_len) = vue_directive(&name_text);
        let prefix = builder.raw_span(p.start, p.start + prefix_len as u32);
        let argument = match (p.is_dynamic.unwrap_or(false), p.arg_start, p.arg_end) {
            (true, Some(start), Some(end)) => {
                // The tokenizer's EOF recovery still emits a dynamic `DirArg`
                // for an UNCLOSED `[` (X_MISSING_DYNAMIC_DIRECTIVE_ARGUMENT_END),
                // so the bracket geometry is fact-derived: a missing close
                // bracket projects a TYPED recovery argument (`close_span:
                // None`, `UnclosedEof`) — never a panic, never a fabricated
                // closed bracket.
                let authored = &builder.source[start as usize..end as usize];
                let open = authored.find('[');
                let close = open.and_then(|open| authored.rfind(']').filter(|close| *close > open));
                match (open, close) {
                    (Some(open), Some(close)) => {
                        let inner = &authored[open + 1..close];
                        let expression_start = inner.len() - inner.trim_start().len();
                        let expression_end = inner.trim_end().len();
                        let open = start + open as u32;
                        let close = start + close as u32;
                        DirectiveArgument::Dynamic {
                            full_span: builder.raw_span(open, close + 1),
                            open_span: builder.raw_span(open, open + 1),
                            expression_span: builder.raw_span(
                                open + 1 + expression_start as u32,
                                open + 1 + expression_end as u32,
                            ),
                            close_span: Some(builder.raw_span(close, close + 1)),
                            termination: SyntaxTermination::Closed,
                        }
                    }
                    (Some(open), None) => {
                        let inner = &authored[open + 1..];
                        let expression_start = inner.len() - inner.trim_start().len();
                        let expression_end = inner.trim_end().len();
                        let open = start + open as u32;
                        DirectiveArgument::Dynamic {
                            full_span: builder.raw_span(open, end),
                            open_span: builder.raw_span(open, open + 1),
                            expression_span: builder.raw_span(
                                open + 1 + expression_start as u32,
                                open + 1 + expression_end as u32,
                            ),
                            close_span: None,
                            termination: SyntaxTermination::UnclosedEof,
                        }
                    }
                    (None, _) => {
                        // A dynamic-flagged argument with no `[` at all has no
                        // live tokenizer producer; keep it recoverable-typed
                        // rather than panicking on a producer drift.
                        let expression_start = authored.len() - authored.trim_start().len();
                        let expression_end = authored.trim_end().len();
                        DirectiveArgument::Dynamic {
                            full_span: builder.raw_span(start, end),
                            open_span: builder.raw_span(start, start),
                            expression_span: builder.raw_span(
                                start + expression_start as u32,
                                start + expression_end as u32,
                            ),
                            close_span: None,
                            termination: SyntaxTermination::Recovered {
                                reason: verter_language::BlockRecoveryReason::ParserRejectedSyntax,
                                recovery_span: None,
                            },
                        }
                    }
                }
            }
            (false, Some(start), Some(end)) => {
                let authored = builder.raw_span(start, end);
                let normalized = builder.intern(&builder.source[start as usize..end as usize]);
                DirectiveArgument::Static {
                    name: AttributeName {
                        authored: SourceSlice::new(authored),
                        normalized,
                        name_span: authored,
                    },
                }
            }
            _ => DirectiveArgument::None,
        };
        let modifiers = p
            .modifiers
            .iter()
            .map(|m| {
                let text = builder.source[m.start as usize..m.end as usize].to_string();
                let normalized = builder.intern(&text.to_ascii_lowercase());
                DirectiveModifier {
                    authored: SourceSlice::new(builder.span(*m)),
                    normalized,
                    separator_span: builder.raw_span(m.start.saturating_sub(1), m.start),
                    name_span: builder.span(*m),
                    full_span: builder.raw_span(m.start.saturating_sub(1), m.end),
                }
            })
            .collect::<Vec<_>>();
        let value = vue_value(builder, p, true);
        let key = format!("d:{name_text}");
        let duplicate_of = duplicates.insert(key, id);
        CarrierAttribute::Directive {
            id,
            family: DirectiveFamily::Vue(family),
            prefix_span: prefix,
            local_name: None,
            argument,
            modifiers: Arc::from(modifiers),
            value,
            full_span: full,
            duplicate_of,
        }
    } else {
        let name_span = builder.raw_span(p.start, p.name_end);
        let normalized_text = name_text.to_ascii_lowercase();
        let normalized = builder.intern(&normalized_text);
        let duplicate_of = duplicates.insert(normalized_text, id);
        CarrierAttribute::Named {
            id,
            name: AttributeName {
                authored: SourceSlice::new(name_span),
                normalized,
                name_span,
            },
            syntax: NamedAttributeSyntax::Explicit,
            value: vue_value(builder, p, false),
            full_span: full,
            duplicate_of,
        }
    }
}
fn vue_directive(name: &str) -> (VueDirectiveKind, usize) {
    if name.starts_with(':') {
        return (VueDirectiveKind::Bind, 1);
    }
    if name.starts_with('@') {
        return (VueDirectiveKind::On, 1);
    }
    if name.starts_with('#') {
        return (VueDirectiveKind::Slot, 1);
    }
    let family = name.split([':', '.']).next().unwrap_or(name);
    let kind = match family {
        "v-bind" => VueDirectiveKind::Bind,
        "v-on" => VueDirectiveKind::On,
        "v-model" => VueDirectiveKind::Model,
        "v-show" => VueDirectiveKind::Show,
        "v-if" => VueDirectiveKind::If,
        "v-else-if" => VueDirectiveKind::ElseIf,
        "v-else" => VueDirectiveKind::Else,
        "v-for" => VueDirectiveKind::For,
        "v-slot" => VueDirectiveKind::Slot,
        "v-pre" => VueDirectiveKind::Pre,
        "v-cloak" => VueDirectiveKind::Cloak,
        "v-once" => VueDirectiveKind::Once,
        "v-memo" => VueDirectiveKind::Memo,
        "v-html" => VueDirectiveKind::Html,
        "v-text" => VueDirectiveKind::Text,
        _ => VueDirectiveKind::Custom,
    };
    (kind, family.len())
}
fn vue_value(
    builder: &mut Builder<'_>,
    p: &verter_parser::types::NodeProp,
    dynamic: bool,
) -> AttributeValue {
    match p.value_start.zip(p.value_end) {
        None => AttributeValue::Missing,
        Some((s, e)) if dynamic => AttributeValue::Expression {
            syntax: AttributeDynamicSyntax::VueBracedExpression,
            full_span: match quote_at(builder.source, s) {
                AttributeQuote::Unquoted => builder.raw_span(s, e),
                _ => builder.raw_span(s - 1, e + 1),
            },
            open_span: match quote_at(builder.source, s) {
                AttributeQuote::Unquoted => None,
                _ => Some(builder.raw_span(s - 1, s)),
            },
            expression_span: builder.raw_span(s, e),
            close_span: match quote_at(builder.source, s) {
                AttributeQuote::Unquoted => None,
                _ => Some(builder.raw_span(e, e + 1)),
            },
            termination: SyntaxTermination::Closed,
        },
        Some((s, e)) => {
            let quote = quote_at(builder.source, s);
            let raw = SourceSlice::new(builder.raw_span(s, e));
            let decoded = if builder.source[s as usize..e as usize].contains('&') {
                LazyDecodedText::EntityDecode {
                    key: DecodedValueKey {
                        raw,
                        recipe: EntityDecodeRecipe::Html5Attribute { quote },
                    },
                }
            } else {
                LazyDecodedText::SameAsSource
            };
            let value_span = match quote {
                AttributeQuote::Unquoted => builder.raw_span(s, e),
                _ => builder.raw_span(s - 1, e + 1),
            };
            AttributeValue::Static {
                raw,
                decoded,
                quote,
                value_span,
                inner_span: builder.raw_span(s, e),
            }
        }
    }
}
fn quote_at(source: &str, start: u32) -> AttributeQuote {
    match source.as_bytes().get(start.saturating_sub(1) as usize) {
        Some(b'\'') => AttributeQuote::Single,
        Some(b'\"') => AttributeQuote::Double,
        _ => AttributeQuote::Unquoted,
    }
}
fn attribute_full_end(source: &str, p: &verter_parser::types::NodeProp) -> u32 {
    p.value_end
        .map(|e| match quote_at(source, p.value_start.unwrap_or(e)) {
            AttributeQuote::Unquoted => e,
            _ => e + 1,
        })
        .unwrap_or(p.name_end)
}

fn project_svelte(
    svelte: &SvelteCarrierCompiler,
    accepted: &AcceptedRegisteredCarrierSource,
    artifact: &verter_language::FrameworkParseArtifact,
) -> CarrierBlockInventory {
    let parsed = svelte.parsed_svelte(artifact).expect("Svelte artifact");
    enum Root<'a> {
        Script(&'a crate::svelte::parser::SvelteScript),
        Style(&'a crate::svelte::parser::SvelteStyle),
        Markup(&'a crate::svelte::parser::SvelteNode),
    }
    impl Root<'_> {
        fn start(&self) -> u32 {
            match self {
                Self::Script(v) => v.tag_open.start,
                Self::Style(v) => v.tag_open.start,
                Self::Markup(v) => svelte_node_span(v).start,
            }
        }
    }
    let mut roots = Vec::new();
    roots.extend(parsed.instance_script.iter().map(Root::Script));
    roots.extend(parsed.module_script.iter().map(Root::Script));
    roots.extend(parsed.styles.iter().map(Root::Style));
    roots.extend(parsed.template.iter().map(Root::Markup));
    roots.sort_by_key(Root::start);
    let mut builder = Builder::new(accepted.source().bytes());
    let mut blocks = Vec::new();
    for root in roots {
        let id = BlockId(blocks.len() as u32);
        match root {
            Root::Script(v) => {
                let syntax = svelte_tagged(
                    &mut builder,
                    "script",
                    v.tag_open,
                    v.content,
                    v.tag_close,
                    &v.attributes,
                );
                let dialect = ScriptSourceType::from(
                    crate::svelte::carrier::svelte_script_source_type(Some(v)),
                );
                blocks.push(CarrierBlock::Section {
                    id,
                    role: SectionRole::Script {
                        role: if v.is_module {
                            ScriptRole::Module
                        } else {
                            ScriptRole::Instance
                        },
                        dialect,
                    },
                    syntax,
                });
            }
            Root::Style(v) => {
                let syntax = svelte_tagged(
                    &mut builder,
                    "style",
                    v.tag_open,
                    v.content,
                    v.tag_close,
                    &v.attributes,
                );
                blocks.push(CarrierBlock::Section {
                    id,
                    role: SectionRole::Style {
                        dialect: StyleDialect::Css,
                        scoped: false,
                        module: StyleModule::None,
                    },
                    syntax,
                });
            }
            Root::Markup(v) => {
                let node = project_svelte_node(&mut builder, v, id, None, MarkupNamespace::Html);
                builder.roots.push(node);
                blocks.push(CarrierBlock::MarkupRoot { id, node });
            }
        }
    }
    builder.finish(accepted, blocks)
}

fn svelte_tagged(
    builder: &mut Builder<'_>,
    name: &str,
    open: Span,
    content: Option<Span>,
    close: Option<Span>,
    attrs: &[crate::svelte::parser::SvelteAttribute],
) -> TaggedSyntax {
    let name_start = open.start + 1;
    let name_span = builder.raw_span(name_start, name_start + name.len() as u32);
    let normalized = builder.intern(name);
    let content = content
        .map(|s| builder.span(s))
        .unwrap_or(builder.raw_span(open.end, open.end));
    let attributes = svelte_attributes(builder, attrs);
    TaggedSyntax {
        authored_name: SourceSlice::new(name_span),
        normalized_name: normalized,
        opening_span: builder.span(open),
        opening_name_span: name_span,
        attribute_insertion_anchor: builder
            .raw_span(open.end.saturating_sub(1), open.end.saturating_sub(1)),
        content_span: content,
        closing_span: close.map(|s| builder.span(s)),
        closing_name_span: close
            .map(|s| builder.raw_span(s.start + 2, s.start + 2 + name.len() as u32)),
        full_span: builder.raw_span(open.start, close.map(|s| s.end).unwrap_or(content.end)),
        termination: if close.is_some() {
            SyntaxTermination::Closed
        } else {
            SyntaxTermination::UnclosedEof
        },
        attributes: Arc::from(attributes),
    }
}

fn project_svelte_node(
    builder: &mut Builder<'_>,
    node: &crate::svelte::parser::SvelteNode,
    root_block: BlockId,
    parent: Option<MarkupNodeId>,
    parent_namespace: MarkupNamespace,
) -> MarkupNodeId {
    use crate::svelte::parser::{
        SvelteBlockKind, SvelteClauseKind, SvelteElementKind, SvelteNode, SvelteSpecialKind,
        SvelteTagKind,
    };
    let placeholder = MarkupNodeId(builder.nodes.len() as u32);
    builder.nodes.push(MarkupSyntaxNode {
        id: placeholder,
        root_block,
        parent,
        children: 0..0,
        kind: MarkupNodeKind::Text {
            content_span: builder.raw_span(0, 0),
        },
    });
    let (kind, children) = match node {
        SvelteNode::Text(span) => (
            MarkupNodeKind::Text {
                content_span: builder.span(*span),
            },
            vec![],
        ),
        SvelteNode::Comment(span) => {
            let closed = span.end >= span.start + 7;
            (
                MarkupNodeKind::Comment {
                    opening_span: builder.raw_span(span.start, (span.start + 4).min(span.end)),
                    content_span: builder.raw_span(
                        (span.start + 4).min(span.end),
                        span.end.saturating_sub(if closed { 3 } else { 0 }),
                    ),
                    closing_span: closed.then(|| builder.raw_span(span.end - 3, span.end)),
                    full_span: builder.span(*span),
                    termination: if closed {
                        SyntaxTermination::Closed
                    } else {
                        SyntaxTermination::UnclosedEof
                    },
                },
                vec![],
            )
        }
        SvelteNode::Interpolation(span) => (
            MarkupNodeKind::Interpolation {
                family: MarkupInterpolationFamily::SvelteInterpolation,
                opening_span: builder.raw_span(span.start.saturating_sub(1), span.start),
                expression_span: builder.span(*span),
                closing_span: Some(builder.raw_span(span.end, span.end + 1)),
                full_span: builder.raw_span(span.start.saturating_sub(1), span.end + 1),
                termination: SyntaxTermination::Closed,
            },
            vec![],
        ),
        SvelteNode::Element(v) => {
            let normalized_text = match v.kind {
                SvelteElementKind::Intrinsic | SvelteElementKind::NestedStyle => {
                    v.name.to_ascii_lowercase()
                }
                _ => v.name.clone(),
            };
            let normalized = builder.intern(&normalized_text);
            let lower_name = v.name.to_ascii_lowercase();
            let namespace = match lower_name.as_str() {
                "svg" => MarkupNamespace::Svg,
                "math" => MarkupNamespace::MathMl,
                _ => parent_namespace,
            };
            let attributes = svelte_attributes(builder, &v.attributes);
            let child_namespace =
                if namespace == MarkupNamespace::Svg && lower_name == "foreignobject" {
                    MarkupNamespace::Html
                } else {
                    namespace
                };
            let child_ids = v
                .children
                .iter()
                .map(|child| {
                    project_svelte_node(
                        builder,
                        child,
                        root_block,
                        Some(placeholder),
                        child_namespace,
                    )
                })
                .collect();
            let content = if let Some(first) = v.children.first() {
                let first = svelte_node_span(first).start;
                let end = v.close_span.map(|s| s.start).unwrap_or_else(|| {
                    v.children
                        .last()
                        .map(svelte_node_span)
                        .map(|s| s.end)
                        .unwrap_or(v.open_span.end)
                });
                builder.raw_span(first, end)
            } else {
                builder.raw_span(
                    v.open_span.end,
                    v.close_span.map(|s| s.start).unwrap_or(v.open_span.end),
                )
            };
            let kind = match v.kind {
                SvelteElementKind::Intrinsic => MarkupElementKind::Native,
                SvelteElementKind::Component => MarkupElementKind::Component,
                SvelteElementKind::NestedStyle => MarkupElementKind::SvelteNestedStyle,
                SvelteElementKind::Special(value) => {
                    MarkupElementKind::SvelteSpecial(match value {
                        SvelteSpecialKind::Head => SvelteSpecialElementKind::Head,
                        SvelteSpecialKind::Window => SvelteSpecialElementKind::Window,
                        SvelteSpecialKind::Document => SvelteSpecialElementKind::Document,
                        SvelteSpecialKind::Body => SvelteSpecialElementKind::Body,
                        SvelteSpecialKind::Element => SvelteSpecialElementKind::Element,
                        SvelteSpecialKind::Boundary => SvelteSpecialElementKind::Boundary,
                        SvelteSpecialKind::Options => SvelteSpecialElementKind::Options,
                        SvelteSpecialKind::Component => SvelteSpecialElementKind::Component,
                        SvelteSpecialKind::SelfRef => SvelteSpecialElementKind::SelfRef,
                        SvelteSpecialKind::Fragment => SvelteSpecialElementKind::Fragment,
                        SvelteSpecialKind::Unknown => SvelteSpecialElementKind::Unknown {
                            authored_local: SourceSlice::new(builder.raw_span(
                                v.name_span.start + "svelte:".len() as u32,
                                v.name_span.end,
                            )),
                        },
                    })
                }
            };
            let full_end = v.close_span.map(|s| s.end).unwrap_or_else(|| {
                if v.self_closing {
                    v.open_span.end
                } else {
                    content.end
                }
            });
            let void_element = namespace == MarkupNamespace::Html && is_void_html(&lower_name);
            (
                MarkupNodeKind::Element(MarkupElementSyntax {
                    authored_name: builder.slice(v.name_span),
                    normalized_name: normalized,
                    namespace,
                    kind,
                    opening_span: builder.span(v.open_span),
                    opening_name_span: builder.span(v.name_span),
                    attribute_insertion_anchor: builder.raw_span(
                        v.open_span
                            .end
                            .saturating_sub(if v.self_closing { 2 } else { 1 }),
                        v.open_span
                            .end
                            .saturating_sub(if v.self_closing { 2 } else { 1 }),
                    ),
                    content_span: content,
                    closing_span: v.close_span.map(|s| builder.span(s)),
                    closing_name_span: v
                        .close_span
                        .map(|s| builder.raw_span(s.start + 2, s.start + 2 + v.name.len() as u32)),
                    full_span: builder.raw_span(v.open_span.start, full_end),
                    self_closing: v.self_closing,
                    void_element,
                    raw_text: matches!(v.kind, SvelteElementKind::NestedStyle),
                    termination: if void_element {
                        SyntaxTermination::Void
                    } else if v.self_closing {
                        SyntaxTermination::SelfClosing
                    } else if v.close_span.is_some() {
                        SyntaxTermination::Closed
                    } else {
                        SyntaxTermination::UnclosedEof
                    },
                    attributes: Arc::from(attributes),
                }),
                child_ids,
            )
        }
        SvelteNode::Block(v) => {
            let mut child_ids = v
                .children
                .iter()
                .map(|child| {
                    project_svelte_node(
                        builder,
                        child,
                        root_block,
                        Some(placeholder),
                        parent_namespace,
                    )
                })
                .collect::<Vec<_>>();
            for clause in &v.clauses {
                let clause_children = clause
                    .children
                    .iter()
                    .map(|child| {
                        project_svelte_node(builder, child, root_block, None, parent_namespace)
                    })
                    .collect::<Vec<_>>();
                let head = match clause.kind {
                    SvelteClauseKind::Else => SvelteClauseHead::Else,
                    SvelteClauseKind::ElseIf => SvelteClauseHead::ElseIf {
                        condition: builder.span(clause.expr.expect("else-if condition")),
                    },
                    SvelteClauseKind::Then => SvelteClauseHead::Then {
                        binding: clause.expr.map(|s| builder.span(s)),
                    },
                    SvelteClauseKind::Catch => SvelteClauseHead::Catch {
                        binding: clause.expr.map(|s| builder.span(s)),
                    },
                };
                let clause_id = builder.add_node(
                    root_block,
                    Some(placeholder),
                    MarkupNodeKind::SvelteClause(SvelteClauseSyntax {
                        head,
                        marker_span: builder.span(clause.tag_span),
                        full_span: builder.span(clause.tag_span),
                        termination: SyntaxTermination::Closed,
                    }),
                    clause_children,
                );
                for child in builder.nodes.iter_mut().filter(|n| {
                    n.parent.is_none() && n.root_block == root_block && n.id != placeholder
                }) {
                    child.parent = Some(clause_id);
                }
                child_ids.push(clause_id);
            }
            let head = match &v.kind {
                SvelteBlockKind::If => SvelteControlBlockHead::If {
                    condition: builder.span(v.head_expr.expect("if head")),
                },
                SvelteBlockKind::Each { item, index, key } => SvelteControlBlockHead::Each {
                    iterable: builder.span(v.head_expr.expect("each head")),
                    item: item.map(|s| builder.span(s)),
                    index: index.map(|s| builder.span(s)),
                    key: key.map(|s| builder.span(s)),
                },
                SvelteBlockKind::Await { inline_branch, .. } => SvelteControlBlockHead::Await {
                    promise: builder.span(v.head_expr.expect("await head")),
                    inline_branch: match inline_branch {
                        crate::svelte::parser::SvelteAwaitInline::None => {
                            SvelteAwaitInlineBranch::None
                        }
                        crate::svelte::parser::SvelteAwaitInline::Then {
                            marker_span,
                            head_span,
                            binding,
                        } => SvelteAwaitInlineBranch::Then {
                            marker_span: builder.span(*marker_span),
                            head_span: builder.span(*head_span),
                            binding: binding.map(|span| builder.span(span)),
                        },
                        crate::svelte::parser::SvelteAwaitInline::Catch {
                            marker_span,
                            head_span,
                            binding,
                        } => SvelteAwaitInlineBranch::Catch {
                            marker_span: builder.span(*marker_span),
                            head_span: builder.span(*head_span),
                            binding: binding.map(|span| builder.span(span)),
                        },
                    },
                },
                SvelteBlockKind::Key => SvelteControlBlockHead::Key {
                    expression: builder.span(v.head_expr.expect("key head")),
                },
                SvelteBlockKind::Snippet {
                    name,
                    name_text: _,
                    params,
                } => SvelteControlBlockHead::Snippet {
                    authored_name: SourceSlice::new(builder.span(*name)),
                    name_span: builder.span(*name),
                    params_span: params.map(|s| builder.span(s)),
                },
            };
            (
                MarkupNodeKind::SvelteControlBlock(SvelteControlBlockSyntax {
                    head,
                    opening_span: builder.span(v.head_span),
                    closing_span: None,
                    full_span: builder.span(v.span),
                    termination: SyntaxTermination::Closed,
                }),
                child_ids,
            )
        }
        SvelteNode::Tag(v) => {
            let family = match v.kind {
                SvelteTagKind::Render => SvelteStandaloneTagFamily::Render,
                SvelteTagKind::Html => SvelteStandaloneTagFamily::Html,
                SvelteTagKind::LegacyConst => SvelteStandaloneTagFamily::LegacyConst,
                SvelteTagKind::Const => SvelteStandaloneTagFamily::Const,
                SvelteTagKind::Let => SvelteStandaloneTagFamily::Let,
                SvelteTagKind::Debug => SvelteStandaloneTagFamily::Debug,
                SvelteTagKind::Attach => SvelteStandaloneTagFamily::Attach,
                SvelteTagKind::Unknown => SvelteStandaloneTagFamily::Unknown {
                    authored_name: SourceSlice::new(builder.span(v.inner)),
                    reason: UnknownMarkupReason::ParserUnknownVariant,
                },
            };
            (
                MarkupNodeKind::SvelteStandaloneTag(SvelteStandaloneTagSyntax {
                    family,
                    opening_span: builder.raw_span(v.span.start, v.inner.start),
                    expression_span: Some(builder.span(v.inner)),
                    closing_span: Some(builder.raw_span(v.inner.end, v.span.end)),
                    full_span: builder.span(v.span),
                    termination: SyntaxTermination::Closed,
                }),
                vec![],
            )
        }
    };
    let start = builder.child_ids.len() as u32;
    builder.child_ids.extend(children);
    let end = builder.child_ids.len() as u32;
    builder.nodes[placeholder.0 as usize] = MarkupSyntaxNode {
        id: placeholder,
        root_block,
        parent,
        children: start..end,
        kind,
    };
    placeholder
}

fn svelte_attributes(
    builder: &mut Builder<'_>,
    attrs: &[crate::svelte::parser::SvelteAttribute],
) -> Vec<CarrierAttribute> {
    use crate::svelte::parser::{SvelteAttributeKind, SvelteDirectiveKind as K};
    let mut duplicates = HashMap::new();
    attrs
        .iter()
        .map(|attr| {
            let id = builder.attribute_id();
            match &attr.kind {
                SvelteAttributeKind::Plain {
                    name,
                    name_span,
                    value,
                } => {
                    let shorthand = !name.is_empty()
                        && matches!(value, Some(crate::svelte::parser::SvelteAttributeValue::Expression(expression)) if attr.span.start + 1 == name_span.start && expression.end + 1 == attr.span.end);
                    let normalized_text = name.to_ascii_lowercase();
                    let normalized = builder.intern(&normalized_text);
                    let duplicate_of = duplicates.insert(normalized_text, id);
                    CarrierAttribute::Named {
                        id,
                        name: AttributeName {
                            authored: builder.slice(*name_span),
                            normalized,
                            name_span: builder.span(*name_span),
                        },
                        syntax: if shorthand {
                            NamedAttributeSyntax::SvelteShorthand
                        } else {
                            NamedAttributeSyntax::Explicit
                        },
                        value: svelte_value(
                            builder,
                            value.as_ref(),
                            &attr.mixed_parts,
                            shorthand,
                        ),
                        full_span: builder.span(attr.span),
                        duplicate_of,
                    }
                }
                SvelteAttributeKind::Spread(expr) => CarrierAttribute::Spread {
                    id,
                    full_span: builder.span(attr.span),
                    open_span: builder.raw_span(attr.span.start, expr.start),
                    expression_span: builder.span(*expr),
                    close_span: (expr.end < attr.span.end)
                        .then(|| builder.raw_span(expr.end, attr.span.end)),
                    termination: SyntaxTermination::Closed,
                },
                SvelteAttributeKind::Directive(v) => {
                    let prefix_text = v.prefix.as_str();
                    let family = match v.kind {
                        K::Bind => SvelteDirectiveKind::Bind,
                        K::Class => SvelteDirectiveKind::Class,
                        K::Style => SvelteDirectiveKind::Style,
                        K::Use => SvelteDirectiveKind::Use,
                        K::Transition => SvelteDirectiveKind::Transition,
                        K::In => SvelteDirectiveKind::In,
                        K::Out => SvelteDirectiveKind::Out,
                        K::Animate => SvelteDirectiveKind::Animate,
                        K::On => SvelteDirectiveKind::On,
                        K::Let => SvelteDirectiveKind::Let,
                        K::Unknown => SvelteDirectiveKind::Unknown {
                            authored_family: SourceSlice::new(builder.raw_span(
                                attr.span.start,
                                attr.span.start + prefix_text.len() as u32,
                            )),
                            reason: UnknownDirectiveReason::ParserUnknownVariant,
                        },
                    };
                    let local_start = attr.span.start + prefix_text.len() as u32 + 1;
                    let local_span =
                        builder.raw_span(local_start, local_start + v.local.len() as u32);
                    let local_normalized = builder.intern(&v.local);
                    let modifiers = v
                        .modifiers
                        .iter()
                        .zip(&v.modifier_spans)
                        .map(|(m, span)| {
                            let normalized = builder.intern(m);
                            DirectiveModifier {
                                authored: builder.slice(*span),
                                normalized,
                                separator_span: builder
                                    .raw_span(span.start.saturating_sub(1), span.start),
                                name_span: builder.span(*span),
                                full_span: builder
                                    .raw_span(span.start.saturating_sub(1), span.end),
                            }
                        })
                        .collect::<Vec<_>>();
                    CarrierAttribute::Directive {
                        id,
                        family: DirectiveFamily::Svelte(family),
                        prefix_span: builder
                            .raw_span(attr.span.start, attr.span.start + prefix_text.len() as u32),
                        local_name: Some(AttributeName {
                            authored: SourceSlice::new(local_span),
                            normalized: local_normalized,
                            name_span: local_span,
                        }),
                        argument: DirectiveArgument::None,
                        modifiers: Arc::from(modifiers),
                        value: v
                            .value
                            .as_ref()
                            .map(|v| svelte_value(builder, Some(v), &attr.mixed_parts, false))
                            .unwrap_or(AttributeValue::Missing),
                        full_span: builder.span(attr.span),
                        duplicate_of: None,
                    }
                }
                SvelteAttributeKind::Attach { expr_span } => CarrierAttribute::Attach {
                    id,
                    full_span: builder.span(attr.span),
                    keyword_span: builder
                        .raw_span(attr.span.start, (attr.span.start + 8).min(attr.span.end)),
                    expression_span: builder.span(*expr_span),
                    close_span: (expr_span.end < attr.span.end)
                        .then(|| builder.raw_span(expr_span.end, attr.span.end)),
                    termination: SyntaxTermination::Closed,
                },
            }
        })
        .collect()
}
fn svelte_value(
    builder: &mut Builder<'_>,
    value: Option<&crate::svelte::parser::SvelteAttributeValue>,
    mixed_parts: &[crate::svelte::parser::SvelteMixedAttributePart],
    shorthand: bool,
) -> AttributeValue {
    use crate::svelte::parser::SvelteAttributeValue;
    match value {
        None => AttributeValue::Missing,
        Some(SvelteAttributeValue::Text(span)) => {
            let quote = quote_at(builder.source, span.start);
            let raw = builder.slice(*span);
            AttributeValue::Static {
                raw,
                decoded: if builder.source[span.start as usize..span.end as usize].contains('&') {
                    LazyDecodedText::EntityDecode {
                        key: DecodedValueKey {
                            raw,
                            recipe: EntityDecodeRecipe::SvelteAttribute { quote },
                        },
                    }
                } else {
                    LazyDecodedText::SameAsSource
                },
                quote,
                value_span: match quote {
                    AttributeQuote::Unquoted => builder.span(*span),
                    _ => builder.raw_span(span.start - 1, span.end + 1),
                },
                inner_span: builder.span(*span),
            }
        }
        Some(SvelteAttributeValue::Expression(span)) => AttributeValue::Expression {
            syntax: if shorthand {
                AttributeDynamicSyntax::SvelteShorthand
            } else {
                AttributeDynamicSyntax::SvelteMustacheExpression
            },
            full_span: builder.raw_span(span.start.saturating_sub(1), span.end + 1),
            open_span: Some(builder.raw_span(span.start.saturating_sub(1), span.start)),
            expression_span: builder.span(*span),
            close_span: Some(builder.raw_span(span.end, span.end + 1)),
            termination: SyntaxTermination::Closed,
        },
        Some(SvelteAttributeValue::Mixed(span)) => {
            let quote = quote_at(builder.source, span.start);
            let parts = mixed_parts
                .iter()
                .map(|part| match part {
                    crate::svelte::parser::SvelteMixedAttributePart::Text(part_span) => {
                        let raw = builder.slice(*part_span);
                        let decoded = if builder.source
                            [part_span.start as usize..part_span.end as usize]
                            .contains('&')
                        {
                            LazyDecodedText::EntityDecode {
                                key: DecodedValueKey {
                                    raw,
                                    recipe: EntityDecodeRecipe::SvelteAttribute { quote },
                                },
                            }
                        } else {
                            LazyDecodedText::SameAsSource
                        };
                        AttributeValuePart::Static { raw, decoded }
                    }
                    crate::svelte::parser::SvelteMixedAttributePart::Expression(expression) => {
                        AttributeValuePart::Expression {
                            syntax: AttributeDynamicSyntax::SvelteMustacheExpression,
                            full_span: builder
                                .raw_span(expression.start.saturating_sub(1), expression.end + 1),
                            open_span: Some(
                                builder
                                    .raw_span(expression.start.saturating_sub(1), expression.start),
                            ),
                            expression_span: builder.span(*expression),
                            close_span: Some(builder.raw_span(expression.end, expression.end + 1)),
                            termination: SyntaxTermination::Closed,
                        }
                    }
                })
                .collect::<Vec<_>>();
            AttributeValue::Mixed {
                full_span: match quote {
                    AttributeQuote::Unquoted => builder.span(*span),
                    _ => builder.raw_span(span.start - 1, span.end + 1),
                },
                parts: Arc::from(parts),
            }
        }
    }
}
fn svelte_node_span(node: &crate::svelte::parser::SvelteNode) -> Span {
    use crate::svelte::parser::SvelteNode;
    match node {
        SvelteNode::Text(s) | SvelteNode::Comment(s) => *s,
        SvelteNode::Interpolation(s) => Span::new(s.start.saturating_sub(1), s.end + 1),
        SvelteNode::Element(v) => Span::new(
            v.open_span.start,
            v.close_span.map(|s| s.end).unwrap_or(v.open_span.end),
        ),
        SvelteNode::Block(v) => v.span,
        SvelteNode::Tag(v) => v.span,
    }
}
