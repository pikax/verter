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

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CssDiagnosticKind {
    UnexpectedClosingDelimiter = 0,
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

impl CssDiagnosticKind {
    /// Discriminant walk, matching [`crate::SyntaxKind::from_raw`]. An unknown
    /// raw value returns variant 0 so `kind as u8 != raw` terminates the walk.
    pub const fn from_raw(raw: u8) -> Self {
        match raw {
            0 => Self::UnexpectedClosingDelimiter,
            1 => Self::MismatchedDelimiter,
            2 => Self::UnterminatedBlock,
            3 => Self::ExpectedAtRuleTerminator,
            4 => Self::ExpectedRuleBlock,
            5 => Self::ExpectedDeclarationColon,
            6 => Self::UnterminatedComment,
            7 => Self::UnterminatedString,
            8 => Self::BadString,
            9 => Self::UnterminatedUrl,
            10 => Self::BadUrl,
            11 => Self::InconsistentIndentation,
            12 => Self::UnexpectedIndentation,
            13 => Self::AmbiguousStatement,
            14 => Self::UnterminatedInterpolation,
            _ => Self::UnexpectedClosingDelimiter,
        }
    }
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
