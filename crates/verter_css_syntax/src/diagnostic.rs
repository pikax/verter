use std::fmt;

use verter_span::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CssSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryKind {
    None,
    AdvanceOneToken,
    AdvanceToBoundary,
    CloseAtEndOfInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CssDiagnosticKind {
    UnexpectedClosingDelimiter,
    MismatchedDelimiter,
    UnterminatedBlock,
    ExpectedAtRuleTerminator,
    ExpectedRuleBlock,
    ExpectedDeclarationColon,
    UnterminatedComment,
    UnterminatedString,
    BadString,
    UnterminatedUrl,
    BadUrl,
    InconsistentIndentation,
    UnexpectedIndentation,
    AmbiguousStatement,
    UnterminatedInterpolation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CssDiagnostic {
    pub kind: CssDiagnosticKind,
    pub severity: CssSeverity,
    pub span: Span,
    pub recovery: RecoveryKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CssSourceTooLarge {
    pub origin: u32,
    pub source_len: u64,
}

impl fmt::Display for CssSourceTooLarge {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "CSS source at origin {} with {} bytes exceeds the u32 span domain",
            self.origin, self.source_len
        )
    }
}

impl std::error::Error for CssSourceTooLarge {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructureOverflowKind {
    TokenIndex,
    NodeIndex,
    ElementIndex,
    ChildRange,
    NestingDepth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CssStructureTooLarge {
    pub kind: StructureOverflowKind,
    pub attempted: u64,
}

impl fmt::Display for CssStructureTooLarge {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "CSS {:?} exceeds the compact structure domain at {}",
            self.kind, self.attempted
        )
    }
}

impl std::error::Error for CssStructureTooLarge {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CssParseFailure {
    Diagnostic(CssDiagnostic),
    Structure(CssStructureTooLarge),
}

impl fmt::Display for CssParseFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Diagnostic(diagnostic) => {
                write!(formatter, "CSS parse diagnostic: {:?}", diagnostic.kind)
            }
            Self::Structure(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for CssParseFailure {}

impl From<CssStructureTooLarge> for CssParseFailure {
    fn from(value: CssStructureTooLarge) -> Self {
        Self::Structure(value)
    }
}
