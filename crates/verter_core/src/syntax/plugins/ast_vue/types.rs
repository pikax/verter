#![allow(dead_code)]

use oxc_allocator::Vec;
use oxc_ast::ast::Program;
use serde::Serialize;
// contains AST types for vue usage
use serde_repr::Serialize_repr;

use crate::common::SourceLocation;

/// Namespaces for elements
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize_repr)]
#[allow(non_camel_case_types)]
#[repr(u8)]
pub enum Namespaces {
    #[default]
    HTML,
    SVG,
    MATH_ML,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize)]
pub struct Namespace(i8);

impl Namespace {
    pub const HTML: Namespace = Namespace(0);
    pub const SVG: Namespace = Namespace(1);
    pub const MATH_ML: Namespace = Namespace(2);
}

/// Node type discriminator for Vue-compatible AST
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize_repr)]
#[allow(non_camel_case_types)]
#[repr(u8)]
pub enum NodeTypes {
    ROOT = 0,
    ELEMENT,
    TEXT,
    COMMENT,
    SIMPLE_EXPRESSION,
    INTERPOLATION,
    ATTRIBUTE,
    DIRECTIVE,
}

/// Element type discriminator
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize_repr)]
#[allow(non_camel_case_types)]
#[repr(u8)]
pub enum ElementTypes {
    ELEMENT,
    COMPONENT,
    SLOT,
    TEMPLATE,
}

/// Constant type flags for optimization hints
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize_repr)]
#[repr(u8)]
pub enum ConstantTypes {
    #[default]
    NotConstant,
    CanSkipPatch,
    CanHoist,
    CanStringify,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum Node<'a> {
    Root(RootNode<'a>),
    Element(ElementNode<'a>),
    Text(TextNode<'a>),
    Comment(CommentNode<'a>),
    Interpolation(InterpolationNode<'a>),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RootNode<'a> {
    pub source: &'a str,

    #[serde(serialize_with = "serialize_oxc_vec")]
    pub children: Vec<'a, Node<'a>>,

    // #[serde(skip)]
    // pub language: Option<&'a str>,
    pub loc: SourceLocation<'a>,
    // /NOTE These are not needed so far, just to do a match with Vue AST
}
impl<'a> RootNode<'a> {
    fn get_type(&self) -> NodeTypes {
        NodeTypes::ROOT
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ElementNode<'a> {
    pub tag: &'a str,
    pub ns: Namespace,
    pub tag_type: ElementTypes,
    #[serde(serialize_with = "serialize_oxc_vec")]
    pub props: Vec<'a, PropNode<'a>>,
    #[serde(serialize_with = "serialize_oxc_vec")]
    pub children: Vec<'a, Node<'a>>,
    pub loc: Option<SourceLocation<'a>>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub is_self_closing: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inner_loc: Option<SourceLocation<'a>>,

    // this is just to keep track of where the open tag ends for void elements
    #[serde(skip)]
    pub open_tag_start: u32,
    #[serde(skip)]
    pub open_tag_end: u32,
}

fn serialize_oxc_vec<'a, T, S>(vec: &Vec<'a, T>, serializer: S) -> Result<S::Ok, S::Error>
where
    T: Serialize,
    S: serde::Serializer,
{
    use serde::ser::SerializeSeq;
    let mut seq = serializer.serialize_seq(Some(vec.len()))?;
    for item in vec.iter() {
        seq.serialize_element(item)?;
    }
    seq.end()
}

fn serialize_optional_oxc_vec<'a, T, S>(
    vec_opt: &Option<Vec<'a, T>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    T: Serialize,
    S: serde::Serializer,
{
    use serde::ser::SerializeSeq;
    match vec_opt {
        Some(vec) => {
            let mut seq = serializer.serialize_seq(Some(vec.len()))?;
            for item in vec.iter() {
                seq.serialize_element(item)?;
            }
            seq.end()
        }
        None => serializer.serialize_none(),
    }
}

impl<'a> ElementNode<'a> {
    fn get_type(&self) -> NodeTypes {
        NodeTypes::ELEMENT
    }
}
#[derive(Debug, Serialize)]
pub struct TextNode<'a> {
    pub content: &'a str,
    pub loc: SourceLocation<'a>,
}
impl<'a> TextNode<'a> {
    fn get_type(&self) -> NodeTypes {
        NodeTypes::TEXT
    }
}
#[derive(Debug, Serialize)]
pub struct CommentNode<'a> {
    pub content: &'a str,
    pub loc: SourceLocation<'a>,
}
impl<'a> CommentNode<'a> {
    fn get_type(&self) -> NodeTypes {
        NodeTypes::COMMENT
    }
}

#[derive(Debug, Serialize)]
pub struct InterpolationNode<'a> {
    pub content: SimpleExpressionNode<'a>,
    pub loc: SourceLocation<'a>,
}
impl<'a> InterpolationNode<'a> {
    fn get_type(&self) -> NodeTypes {
        NodeTypes::INTERPOLATION
    }
}

#[derive(Debug, Serialize)]
#[allow(clippy::large_enum_variant)]
#[serde(untagged)]
pub enum PropNode<'a> {
    Attribute(AttributeNode<'a>),
    Directive(DirectiveNode<'a>),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttributeNode<'a> {
    pub name: &'a str,
    pub name_loc: SourceLocation<'a>,
    pub value: Option<TextNode<'a>>,
    pub loc: SourceLocation<'a>,
}
impl<'a> AttributeNode<'a> {
    fn get_type(&self) -> NodeTypes {
        NodeTypes::ATTRIBUTE
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectiveNode<'a> {
    pub name: &'a str,
    pub raw_name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exp: Option<SimpleExpressionNode<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arg: Option<SimpleExpressionNode<'a>>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_optional_oxc_vec"
    )]
    pub modifiers: Option<Vec<'a, SimpleExpressionNode<'a>>>,
    pub loc: SourceLocation<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub for_parse_result: Option<ForParseResult<'a>>,
}
impl<'a> DirectiveNode<'a> {
    fn get_type(&self) -> NodeTypes {
        NodeTypes::DIRECTIVE
    }
}

#[derive(Debug)]
pub struct ExpressionParseResult<'a> {
    pub program: Program<'a>,
    pub errors: Vec<'a, oxc_diagnostics::OxcDiagnostic>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SimpleExpressionNode<'a> {
    pub content: &'a str,
    pub loc: SourceLocation<'a>,
    pub is_static: bool,
    pub const_type: ConstantTypes,
    // #[serde(skip)]
    // pub parsed: Option<Result<Expression<'a>, Vec<OxcDiagnostic>>>,
}
impl<'a> SimpleExpressionNode<'a> {
    fn get_type(&self) -> NodeTypes {
        NodeTypes::SIMPLE_EXPRESSION
    }
}

#[derive(Debug, Serialize)]
pub struct ForParseResult<'a> {
    pub source: Option<SimpleExpressionNode<'a>>,
    pub value: Option<SimpleExpressionNode<'a>>,
    pub key: Option<SimpleExpressionNode<'a>>,
    pub index: Option<SimpleExpressionNode<'a>>,
    pub is_of: bool,
}
