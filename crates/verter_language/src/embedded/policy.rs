//! Profile-declared codec policy. Everything here is data a profile supplies;
//! nothing names a language.

use std::sync::Arc;

use verter_identity::encoding::{CanonicalDigest, CanonicalEncoder};

use super::decode::{scan_escape, EscapeScan};

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
    /// The spelling and the authored bytes around it would form a closer, a
    /// hole opener or a contracted terminator neither side spells alone.
    FormsSyntaxWithNeighbours,
    /// An escape at an edge of the edit, authored or spelled, would read
    /// differently in its new context: the character now after it is one the
    /// escape forbids, or the text across the seam extends it into a longer
    /// escape.
    ChangesAdjacentEscape,
}

/// The authored bytes around an edit. Slices of the source reaching to its ends
/// are always enough, and the delimiters count: a replacement ending in the
/// start of a closer means something else beside it. An escape's meaning can
/// depend on what follows it, so `before` must reach back past the escape
/// introducers immediately preceding the edit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AuthoredNeighbours<'a> {
    pub before: &'a str,
    pub after: &'a str,
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

    /// Spell `cooked` so that decoding it between `neighbours` under this policy
    /// yields exactly `cooked` again there, or refuse.
    pub fn encode_replacement(
        &self,
        cooked: &str,
        neighbours: AuthoredNeighbours<'_>,
    ) -> Result<String, UnencodableReplacement> {
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
        // Where each spelled escape's marker starts, and what it must cook to.
        let mut escapes = Vec::new();
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
            escapes.push((spelled.len(), ch));
            spelled.push(escape_marker(&policy.escapes, ch)?);
        }
        let contracts_terminators = cooked_policy
            .is_some_and(|policy| policy.line_terminators == LineTerminatorPolicy::NormalizeToLf);
        let formed = [
            Some(&*self.delimiters.close),
            self.holes.opener(),
            contracts_terminators.then_some("\r\n"),
        ]
        .into_iter()
        .flatten()
        .any(|syntax| spans_a_seam(neighbours, &spelled, syntax));
        if formed {
            return Err(UnencodableReplacement::FormsSyntaxWithNeighbours);
        }
        if let (Some(policy), Some(introducer)) = (cooked_policy, self.delimiters.escape_introducer)
        {
            let seams = EscapeSeams {
                escapes: &policy.escapes,
                introducer,
                lookahead: escape_lookahead(&policy.escapes),
            };
            if !seams.authored_escape_reads_alike(neighbours, &spelled) {
                return Err(UnencodableReplacement::ChangesAdjacentEscape);
            }
            seams.check_spelled_escapes(&spelled, &escapes, neighbours.after)?;
        }
        Ok(spelled)
    }
}

/// The bytes after an introducer that can decide what its escape means: a
/// marker and the character after it, or the longest declared continuation.
/// A numeric escape ends where its own digits or closer do, whatever follows.
fn escape_lookahead(escapes: &EscapePolicy) -> usize {
    const SCALAR: usize = 4;
    let continuation = escapes
        .continuations
        .iter()
        .map(|spelling| spelling.len())
        .max()
        .unwrap_or(0);
    continuation.max(SCALAR) + SCALAR
}

/// Re-reads the escapes on either side of an edit with the decoder's own
/// scanner, so the profile's escape table stays the only authority on what an
/// escape means beside other text.
struct EscapeSeams<'a> {
    escapes: &'a EscapePolicy,
    introducer: char,
    lookahead: usize,
}

impl EscapeSeams<'_> {
    /// Whether every escape `before` ends with still reads as it did alone once
    /// the spelling and `after` follow it. Only an escape starting within the
    /// lookahead of the seam can reach it.
    fn authored_escape_reads_alike(
        &self,
        neighbours: AuthoredNeighbours<'_>,
        spelled: &str,
    ) -> bool {
        let before = neighbours.before;
        let reach = self.lookahead + self.introducer.len_utf8();
        let window = floor_boundary(before, before.len().saturating_sub(reach));
        let head = prefix(spelled, self.lookahead);
        let head = if head.len() == spelled.len() {
            [
                head,
                prefix(neighbours.after, self.lookahead.saturating_sub(head.len())),
            ]
            .concat()
        } else {
            head.to_owned()
        };
        before[window..]
            .char_indices()
            .map(|(offset, ch)| (window + offset, ch))
            .filter(|&(at, ch)| ch == self.introducer && self.starts_escape(before, at))
            .all(|(at, _)| {
                let body = &before[at + self.introducer.len_utf8()..];
                scan_escape(self.escapes, body)
                    == scan_escape(self.escapes, &[body, &head].concat())
            })
    }

    /// Whether each escape the spelling wrote reads as the scalar it spells:
    /// first among the spelled text after it, then, near the end of the
    /// spelling, with the authored text after the edit too.
    fn check_spelled_escapes(
        &self,
        spelled: &str,
        escapes: &[(usize, char)],
        after: &str,
    ) -> Result<(), UnencodableReplacement> {
        for &(marker_at, cooked) in escapes {
            let rest = &spelled[marker_at..];
            let len = rest.chars().next().map_or(0, char::len_utf8);
            let expected = EscapeScan::Scalar { cooked, len };
            if scan_escape(self.escapes, rest) != expected {
                return Err(UnencodableReplacement::Character(cooked));
            }
            if rest.len() <= self.lookahead
                && scan_escape(
                    self.escapes,
                    &[rest, prefix(after, self.lookahead)].concat(),
                ) != expected
            {
                return Err(UnencodableReplacement::ChangesAdjacentEscape);
            }
        }
        Ok(())
    }

    /// Whether the introducer at `at` starts an escape. An introducer is an
    /// escape's marker only right after another one, so a run of them pairs
    /// from its start: an even number before `at` leaves it opening an escape.
    fn starts_escape(&self, text: &str, at: usize) -> bool {
        text[..at]
            .chars()
            .rev()
            .take_while(|&ch| ch == self.introducer)
            .count()
            % 2
            == 0
    }
}

/// The largest scalar boundary of `text` at or before `at`.
fn floor_boundary(text: &str, mut at: usize) -> usize {
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// At least `bytes` of `text`, or all of it, ending on a scalar boundary.
fn prefix(text: &str, bytes: usize) -> &str {
    let mut end = bytes.min(text.len());
    while !text.is_char_boundary(end) {
        end += 1;
    }
    &text[..end]
}

/// Whether `syntax` occurs in `before + spelled + after` across either seam.
fn spans_a_seam(neighbours: AuthoredNeighbours<'_>, spelled: &str, syntax: &str) -> bool {
    let Some(reach) = syntax.len().checked_sub(1).filter(|reach| *reach > 0) else {
        return false;
    };
    let mut tail_start = neighbours.before.len().saturating_sub(reach);
    while !neighbours.before.is_char_boundary(tail_start) {
        tail_start += 1;
    }
    let mut head_end = reach.min(neighbours.after.len());
    while !neighbours.after.is_char_boundary(head_end) {
        head_end -= 1;
    }
    let tail = &neighbours.before[tail_start..];
    let head = &neighbours.after[..head_end];
    let window = [tail, spelled, head].concat();
    let seams = [tail.len(), tail.len() + spelled.len()];
    window.char_indices().any(|(start, _)| {
        let end = start + syntax.len();
        window[start..].starts_with(syntax) && seams.iter().any(|&seam| start < seam && seam < end)
    })
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
    encoder.field_bytes(9, &escape_table_bytes(&policy.escapes));
    encoder.field_enum_discriminant(10, policy.line_terminators as u32);
    encoder.field_enum_discriminant(11, policy.reverse as u32);
    encoder.field_bytes(12, &indentation_bytes(&policy.indentation));
}

/// A table's canonical bytes: little-endian `u32` values, lists length-prefixed.
#[derive(Default)]
struct TableBytes(Vec<u8>);

impl TableBytes {
    fn value(&mut self, value: u32) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    fn scalar(&mut self, scalar: char) {
        self.value(u32::from(scalar));
    }

    fn scalars(&mut self, scalars: &[char]) {
        self.value(scalars.len() as u32);
        for &scalar in scalars {
            self.scalar(scalar);
        }
    }
}

fn escape_table_bytes(escapes: &EscapePolicy) -> Vec<u8> {
    let mut bytes = TableBytes::default();
    bytes.value(escapes.simple.len() as u32);
    for simple in escapes.simple.iter() {
        bytes.scalar(simple.marker);
        bytes.scalar(simple.cooked);
        bytes.scalars(&simple.invalid_before);
    }
    bytes.value(escapes.numeric.len() as u32);
    for numeric in escapes.numeric.iter() {
        match *numeric {
            NumericEscape::Fixed {
                marker,
                radix,
                digits,
            } => {
                bytes.value(0);
                bytes.scalar(marker);
                bytes.value(radix);
                bytes.value(u32::from(digits));
            }
            NumericEscape::Braced {
                marker,
                open,
                close,
                radix,
            } => {
                bytes.value(1);
                bytes.scalar(marker);
                bytes.scalar(open);
                bytes.scalar(close);
                bytes.value(radix);
            }
        }
    }
    bytes.scalars(&escapes.invalid_markers);
    bytes.value(escapes.identity as u32);
    bytes.value(escapes.continuations.len() as u32);
    for continuation in escapes.continuations.iter() {
        bytes.value(continuation.len() as u32);
        bytes.0.extend_from_slice(continuation.as_bytes());
    }
    bytes.0
}

fn indentation_bytes(indentation: &IndentationPolicy) -> Vec<u8> {
    let mut bytes = TableBytes::default();
    match indentation {
        IndentationPolicy::Preserve => bytes.value(0),
        IndentationPolicy::StripCommonLeading {
            whitespace,
            include_first_line,
        } => {
            bytes.value(1);
            bytes.value(u32::from(*include_first_line));
            bytes.scalars(whitespace);
        }
    }
    bytes.0
}
