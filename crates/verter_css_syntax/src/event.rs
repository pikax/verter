use crate::diagnostic::{CssDiagnostic, CssStructureTooLarge};
use crate::token::SyntaxToken;

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SyntaxKind {
    Stylesheet = 0,
    SelectorList,
    Selector,
    CompoundSelector,
    Combinator,
    NamespaceSelector,
    AttributeSelector,
    NestingSelector,
    PseudoClass,
    PseudoElement,
    PseudoSelectorList,
    NthSelector,
    NthOfSelectorList,
    UnknownPseudoFunction,
    QualifiedRule,
    RuleBlock,
    Declaration,
    CustomPropertyDeclaration,
    ComponentValueList,
    ComponentValueBlock,
    Function,
    GroupAtRule,
    DescriptorAtRule,
    KeyframesAtRule,
    UnknownAtRule,
    AtRulePrelude,
    AtRuleBlock,
    Recovery,
    ClassSelector,
    IdSelector,
    TypeSelector,
    Interpolation,
    IndentedBlock,
    AmbiguousStatement,
    VariableDeclaration,
    MixinOrFunctionHeader,
    ControlDirective,
}

impl SyntaxKind {
    pub const fn from_raw(raw: u16) -> Self {
        match raw {
            0 => Self::Stylesheet,
            1 => Self::SelectorList,
            2 => Self::Selector,
            3 => Self::CompoundSelector,
            4 => Self::Combinator,
            5 => Self::NamespaceSelector,
            6 => Self::AttributeSelector,
            7 => Self::NestingSelector,
            8 => Self::PseudoClass,
            9 => Self::PseudoElement,
            10 => Self::PseudoSelectorList,
            11 => Self::NthSelector,
            12 => Self::NthOfSelectorList,
            13 => Self::UnknownPseudoFunction,
            14 => Self::QualifiedRule,
            15 => Self::RuleBlock,
            16 => Self::Declaration,
            17 => Self::CustomPropertyDeclaration,
            18 => Self::ComponentValueList,
            19 => Self::ComponentValueBlock,
            20 => Self::Function,
            21 => Self::GroupAtRule,
            22 => Self::DescriptorAtRule,
            23 => Self::KeyframesAtRule,
            24 => Self::UnknownAtRule,
            25 => Self::AtRulePrelude,
            26 => Self::AtRuleBlock,
            27 => Self::Recovery,
            28 => Self::ClassSelector,
            29 => Self::IdSelector,
            30 => Self::TypeSelector,
            31 => Self::Interpolation,
            32 => Self::IndentedBlock,
            33 => Self::AmbiguousStatement,
            34 => Self::VariableDeclaration,
            35 => Self::MixinOrFunctionHeader,
            36 => Self::ControlDirective,
            _ => Self::Recovery,
        }
    }
}

#[repr(transparent)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct NodeFlags(pub u16);

impl NodeFlags {
    pub const RECOVERED: Self = Self(1 << 0);
    pub const DIALECT_EXTENSION: Self = Self(1 << 1);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseEvent {
    StartNode {
        kind: SyntaxKind,
        flags: NodeFlags,
        start: u32,
    },
    Token(SyntaxToken),
    FinishNode {
        kind: SyntaxKind,
        end: u32,
    },
    Diagnostic(CssDiagnostic),
}

impl ParseEvent {
    pub fn fold_fingerprint(self, fingerprint: u64) -> u64 {
        let mut value = fingerprint ^ 0xcbf2_9ce4_8422_2325;
        let fields = match self {
            Self::StartNode { kind, flags, start } => {
                [1, kind as u64, u64::from(flags.0), u64::from(start), 0]
            }
            Self::Token(token) => [
                2,
                u64::from(token.kind),
                u64::from(token.flags),
                u64::from(token.start),
                u64::from(token.end),
            ],
            Self::FinishNode { kind, end } => [3, kind as u64, u64::from(end), 0, 0],
            Self::Diagnostic(diagnostic) => [
                4,
                diagnostic.kind as u64,
                u64::from(diagnostic.span.start),
                u64::from(diagnostic.span.end),
                diagnostic.recovery as u64,
            ],
        };
        for field in fields {
            value ^= field;
            value = value.wrapping_mul(0x0000_0100_0000_01b3);
        }
        value
    }
}

pub trait ParseEventSink {
    fn event(&mut self, event: ParseEvent) -> Result<(), CssStructureTooLarge>;

    /// Whether whitespace trivia must appear as [`ParseEvent::Token`] events.
    ///
    /// Lossless CST reconstruction and Sass indent keep every whitespace token.
    /// The stylesheet IR sink returns `false`: the parser still sees trivia via
    /// the lexer for combinators and layout, but does not emit whitespace tokens
    /// the IR ignores. Comment tokens stay on the stream so [`crate::StyleSyntaxIr`]
    /// can retain comment spans.
    fn retain_whitespace_trivia(&self) -> bool {
        true
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ParseSummary {
    pub events: u32,
    pub tokens: u32,
    pub nodes: u32,
    pub diagnostics: u32,
    pub recoveries: u32,
    pub fingerprint: u64,
}
