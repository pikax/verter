use crate::common::{ErrorCode, Position, SourceLocation, Span};

/// Character codes for fast comparison
pub mod char_codes {
    pub const TAB: u8 = 0x09;
    pub const NEWLINE: u8 = 0x0A;
    pub const FORM_FEED: u8 = 0x0C;
    pub const CARRIAGE_RETURN: u8 = 0x0D;
    pub const SPACE: u8 = 0x20;
    pub const EXCLAMATION_MARK: u8 = 0x21;
    pub const DOUBLE_QUOTE: u8 = 0x22;
    pub const NUMBER: u8 = 0x23;
    pub const AMP: u8 = 0x26;
    pub const SINGLE_QUOTE: u8 = 0x27;
    pub const DASH: u8 = 0x2D;
    pub const DOT: u8 = 0x2E;
    pub const SLASH: u8 = 0x2F;
    pub const BACKSLASH: u8 = 0x5C;
    pub const COLON: u8 = 0x3A;
    pub const LT: u8 = 0x3C;
    pub const EQ: u8 = 0x3D;
    pub const GT: u8 = 0x3E;
    pub const QUESTION_MARK: u8 = 0x3F;
    pub const AT: u8 = 0x40;
    pub const UPPER_A: u8 = 0x41;
    pub const UPPER_Z: u8 = 0x5A;
    pub const LEFT_SQUARE: u8 = 0x5B;
    pub const RIGHT_SQUARE: u8 = 0x5D;
    pub const LOWER_A: u8 = 0x61;
    pub const LOWER_V: u8 = 0x76;
    pub const LOWER_Z: u8 = 0x7A;
    pub const LEFT_BRACE: u8 = 0x7B;
    pub const RIGHT_BRACE: u8 = 0x7D;
}

pub mod sequences {
    pub const CDATA: &[u8] = b"CDATA[";
    pub const CDATA_END: &[u8] = b"]]>";
    pub const COMMENT_END: &[u8] = b"-->";
    pub const SCRIPT_END: &[u8] = b"</script";
    pub const STYLE_END: &[u8] = b"</style";
    pub const TITLE_END: &[u8] = b"</title";
    pub const TEXTAREA_END: &[u8] = b"</textarea";
}

/// Quote type for attribute values
#[derive(Debug, Clone, PartialEq, Eq)]
#[repr(u8)]
pub enum QuoteType {
    NoValue = 0,
    Unquoted = 1,
    Single = 2,
    Double = 3,
}

/// All events emitted by the tokenizer.
/// Spans are (start: u32, end: u32) indices into the input buffer.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event<'bump> {
    // Text and entities
    Text {
        start: u32,
        end: u32,
    },
    TextEntity {
        start: u32,
        end: u32,
    },

    // Interpolation
    Interpolation {
        start: u32,
        end: u32,
        delimiter_open_len: u8,
        delimiter_close_len: u8,
    },

    // Tags
    OpenTagName {
        start: u32,
        end: u32,
    },
    OpenTagEnd {
        end: u32,
    },
    SelfClosingTag {
        end: u32,
    },
    CloseTag {
        start: u32,
        end: u32,
        name_end: u32,
    },

    // Attributes
    AttribName {
        start: u32,
        end: u32,
    },
    AttribNameEnd {
        end: u32,
    },
    AttribData {
        start: u32,
        end: u32,
    },
    // AttribEntity { start: u32, end: u32 },
    AttribEnd {
        quote: QuoteType,
        end: u32,
    },

    // Directives
    DirName {
        start: u32,
        end: u32,
    },
    DirArg {
        is_dynamic: bool,
        start: u32,
        end: u32,
    },
    DirModifier {
        start: u32,
        end: u32,
    },

    // Comments and special content
    Comment {
        start: u32,
        end: u32,
    },
    // Cdata { start: u32, end: u32 },
    ProcessingInstruction {
        start: u32,
        end: u32,
    },

    // Errors
    Error {
        code: ErrorCode,
        index: u32,
    },

    // End-of-stream marker
    End,

    // Extended added by tokenizer
    // new in this tokenizer version
    DirVPre {
        start: u32,
        end: u32,
    },

    // added by plugins
    ElementOpenTag(ElementOpenTagEvent<'bump>),

    // either directive or attribute
    Prop(TokenizerPropNode<'bump>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementOpenTagEvent<'bump> {
    pub id: usize,

    pub start: u32,
    pub end: u32,
    pub name_end: u32,

    pub nested_level: usize,

    pub self_closing: bool,

    pub has_v_pre: bool,
    pub in_v_pre: bool,

    pub props: &'bump [TokenizerPropNode<'bump>],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenizerPropNode<'bump> {
    pub quote: QuoteType,
    pub start: u32,
    pub end: u32,

    pub name_end: u32,

    pub value_start: u32,
    pub value_end: u32,

    pub arg_start: u32,
    pub arg_end: u32,
    pub modifiers: &'bump [Span],

    pub is_directive: bool,
}

/// All events emitted by the tokenizer.
/// Spans are (start: u32, end: u32) indices into the input buffer.
#[non_exhaustive]
#[derive(Debug, PartialEq, Eq)]
pub enum EventSourceLocation<'a> {
    // Text and entities
    Text(SourceLocation<'a>),
    TextEntity(SourceLocation<'a>),

    // Interpolation
    Interpolation(SourceLocation<'a>),

    // Tags
    OpenTagName(SourceLocation<'a>),
    OpenTagEnd(Position),
    SelfClosingTag(Position),
    CloseTag(SourceLocation<'a>),

    // Attributes
    AttribName(SourceLocation<'a>),
    AttribNameEnd(Position),
    AttribData(SourceLocation<'a>),
    AttribEntity(SourceLocation<'a>),
    AttribEnd {
        quote: QuoteType,
        position: Position,
    },

    // Directives
    DirName(SourceLocation<'a>),
    DirArg(SourceLocation<'a>),
    DirModifier(SourceLocation<'a>),
    DirVPre(SourceLocation<'a>),

    // Comments and special content
    Comment(SourceLocation<'a>),
    Cdata(SourceLocation<'a>),
    ProcessingInstruction(SourceLocation<'a>),

    // Errors
    Error {
        code: ErrorCode,
        position: Position,
    },

    // End-of-stream marker
    End,
}
