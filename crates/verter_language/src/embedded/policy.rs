//! Profile-declared codec policy. Everything here is data a profile supplies;
//! nothing names a language.

use std::sync::Arc;

use verter_identity::encoding::{CanonicalDigest, CanonicalEncoder};

/// One escape whose spelling after the introducer is a fixed character.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimpleEscape {
    pub marker: char,
    pub cooked: char,
    /// Characters that make this escape invalid when they follow it.
    pub invalid_before: Box<[char]>,
}

/// An escape spelling a numeric code point. A high-surrogate result pairs with
/// an immediately following low-surrogate escape into one scalar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumericEscape {
    Fixed {
        marker: char,
        radix: u32,
        digits: u8,
    },
    Braced {
        marker: char,
        open: char,
        close: char,
        radix: u32,
    },
}

impl NumericEscape {
    pub(super) fn marker(&self) -> char {
        match self {
            NumericEscape::Fixed { marker, .. } | NumericEscape::Braced { marker, .. } => *marker,
        }
    }
}

/// What an escape with an otherwise undeclared marker means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityEscapes {
    /// It cooks to the marker itself.
    AnyOther,
    /// It leaves the literal without a cooked value.
    Invalid,
}

/// The escape vocabulary of a cooked selection. An empty table with
/// [`IdentityEscapes::Invalid`] makes every escape invalid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EscapePolicy {
    pub simple: Box<[SimpleEscape]>,
    pub numeric: Box<[NumericEscape]>,
    /// Markers that are explicitly invalid.
    pub invalid_markers: Box<[char]>,
    pub identity: IdentityEscapes,
    /// Terminator spellings removed from the value when escaped. The longest
    /// match wins, whatever the declared order.
    pub continuations: Box<[Box<str>]>,
}

/// What an unescaped line terminator does in a cooked value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineTerminatorPolicy {
    Preserve,
    /// CRLF and lone CR cook to LF; each records its contraction.
    NormalizeToLf,
    /// The literal syntax has no unescaped line terminators.
    Forbidden,
}

/// Indentation removal. Nothing is removed unless declared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndentationPolicy {
    Preserve,
    /// Remove the longest leading run of the declared whitespace common to
    /// every non-blank line, and at most that much from blank lines.
    StripCommonLeading {
        whitespace: Box<[char]>,
        include_first_line: bool,
    },
}

/// Whether a profile declares how cooked text becomes authored spelling.
/// Without it, no cooked range can be an edit target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReverseEncoding {
    Undeclared,
    /// Replacement text is spelled through the declared escape table.
    ThroughEscapes,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CookedPolicy {
    pub escapes: EscapePolicy,
    pub line_terminators: LineTerminatorPolicy,
    pub indentation: IndentationPolicy,
    pub reverse: ReverseEncoding,
}

/// Raw selection keeps the exact authored spelling, so it takes no
/// normalization policy at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextSelection {
    Raw,
    Cooked(CookedPolicy),
}

/// The literal's delimiters. `escape_introducer` applies under both selections
/// because an escaped closer does not close the literal even in raw text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DelimiterPolicy {
    pub open: Box<str>,
    pub close: Box<str>,
    pub escape_introducer: Option<char>,
}

/// An opaque, profile-owned label for a hole. The codec only carries it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProfileHoleClass(pub Arc<str>);

/// Synthetic text standing in for a hole, spelled `prefix` + ordinal + `suffix`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceholderSpelling {
    pub prefix: Box<str>,
    pub suffix: Box<str>,
}

/// How a profile treats interpolation holes reported by the host parser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HolePolicy {
    /// The syntax has no holes; any reported hole is refused.
    Absent,
    /// The syntax has holes and this profile refuses them, naming why.
    Refuse {
        opener: Box<str>,
        disposition: ProfileHoleClass,
    },
    /// The profile admits holes under its own classification.
    Admit {
        opener: Box<str>,
        class: ProfileHoleClass,
        placeholder: Option<PlaceholderSpelling>,
    },
}

impl HolePolicy {
    pub(super) fn opener(&self) -> Option<&str> {
        match self {
            HolePolicy::Absent => None,
            HolePolicy::Refuse { opener, .. } | HolePolicy::Admit { opener, .. } => Some(opener),
        }
    }
}

/// Replacement text the policy cannot spell in authored form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnencodableReplacement {
    /// The profile declares no reverse encoding.
    NoReverseEncoding,
    /// This character has no authored spelling under the policy.
    Character(char),
}

/// A profile's complete, immutable text policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedTextCodec {
    selection: TextSelection,
    delimiters: DelimiterPolicy,
    holes: HolePolicy,
    digest: CanonicalDigest,
}

impl EmbeddedTextCodec {
    const DOMAIN_TAG: &'static str = "verter.language.embedded.codec_policy.v1";

    pub fn new(selection: TextSelection, delimiters: DelimiterPolicy, holes: HolePolicy) -> Self {
        let digest = policy_digest(Self::DOMAIN_TAG, &selection, &delimiters, &holes);
        Self {
            selection,
            delimiters,
            holes,
            digest,
        }
    }

    pub fn selection(&self) -> &TextSelection {
        &self.selection
    }

    pub fn delimiters(&self) -> &DelimiterPolicy {
        &self.delimiters
    }

    pub fn holes(&self) -> &HolePolicy {
        &self.holes
    }

    /// Identity of this exact policy, for syntax keys. Requester encoding and
    /// process identity never enter it.
    pub fn policy_digest(&self) -> CanonicalDigest {
        self.digest
    }

    /// Spell `cooked` so that decoding it under this policy yields exactly
    /// `cooked` again, or refuse.
    pub fn encode_replacement(&self, cooked: &str) -> Result<String, UnencodableReplacement> {
        let cooked_policy = match &self.selection {
            TextSelection::Raw => None,
            TextSelection::Cooked(policy) => {
                if policy.reverse == ReverseEncoding::Undeclared {
                    return Err(UnencodableReplacement::NoReverseEncoding);
                }
                Some(policy)
            }
        };
        let mut spelled = String::with_capacity(cooked.len());
        for (index, ch) in cooked.char_indices() {
            let rest = &cooked[index..];
            let spells_syntax = (!self.delimiters.close.is_empty()
                && rest.starts_with(&*self.delimiters.close))
                || self.holes.opener().is_some_and(|o| rest.starts_with(o))
                || self.delimiters.escape_introducer == Some(ch);
            let needs_escape = spells_syntax
                || cooked_policy.is_some_and(|policy| line_terminator_needs_escape(policy, ch));
            if !needs_escape {
                spelled.push(ch);
                continue;
            }
            let (Some(policy), Some(introducer)) =
                (cooked_policy, self.delimiters.escape_introducer)
            else {
                return Err(UnencodableReplacement::Character(ch));
            };
            spelled.push(introducer);
            spelled.push(escape_marker(&policy.escapes, ch)?);
        }
        Ok(spelled)
    }
}

fn line_terminator_needs_escape(policy: &CookedPolicy, ch: char) -> bool {
    if !matches!(ch, '\n' | '\r') {
        return false;
    }
    // A new line under indentation removal would change what is stripped.
    let reindents = policy.indentation != IndentationPolicy::Preserve;
    match policy.line_terminators {
        LineTerminatorPolicy::Preserve => reindents,
        LineTerminatorPolicy::NormalizeToLf => ch == '\r' || reindents,
        LineTerminatorPolicy::Forbidden => true,
    }
}

fn escape_marker(escapes: &EscapePolicy, ch: char) -> Result<char, UnencodableReplacement> {
    // A marker with a following-character restriction could change meaning
    // next to the text after it, so it is not a safe spelling.
    if let Some(simple) = escapes
        .simple
        .iter()
        .find(|simple| simple.cooked == ch && simple.invalid_before.is_empty())
    {
        return Ok(simple.marker);
    }
    let declared = escapes.simple.iter().any(|simple| simple.marker == ch)
        || escapes.invalid_markers.contains(&ch)
        || escapes.numeric.iter().any(|numeric| numeric.marker() == ch);
    if escapes.identity == IdentityEscapes::AnyOther && !declared {
        Ok(ch)
    } else {
        Err(UnencodableReplacement::Character(ch))
    }
}

fn policy_digest(
    tag: &'static str,
    selection: &TextSelection,
    delimiters: &DelimiterPolicy,
    holes: &HolePolicy,
) -> CanonicalDigest {
    let mut encoder = CanonicalEncoder::new(tag);
    encoder.field_str(1, &delimiters.open);
    encoder.field_str(2, &delimiters.close);
    let introducer = delimiters
        .escape_introducer
        .map(u32::from)
        .unwrap_or(u32::MAX);
    encoder.field_u32(3, introducer);
    match holes {
        HolePolicy::Absent => {
            encoder.field_enum_discriminant(4, 0);
        }
        HolePolicy::Refuse {
            opener,
            disposition,
        } => {
            encoder.field_enum_discriminant(4, 1);
            encoder.field_str(5, opener);
            encoder.field_str(6, &disposition.0);
        }
        HolePolicy::Admit {
            opener,
            class,
            placeholder,
        } => {
            encoder.field_enum_discriminant(4, 2);
            encoder.field_str(5, opener);
            encoder.field_str(6, &class.0);
            if let Some(placeholder) = placeholder {
                encoder.field_str(7, &placeholder.prefix);
                encoder.field_str(8, &placeholder.suffix);
            }
        }
    }
    if let TextSelection::Cooked(policy) = selection {
        encode_cooked_policy(&mut encoder, policy);
    }
    encoder.digest()
}

fn encode_cooked_policy(encoder: &mut CanonicalEncoder, policy: &CookedPolicy) {
    // The `Debug` spelling of plain tables is deterministic and these bytes
    // only feed a digest; nothing parses them back.
    encoder.field_str(9, &format!("{:?}", policy.escapes));
    encoder.field_enum_discriminant(10, policy.line_terminators as u32);
    encoder.field_enum_discriminant(11, policy.reverse as u32);
    encoder.field_str(12, &format!("{:?}", policy.indentation));
}
