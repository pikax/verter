//! The checker's template-literal relation (`templateLiteralRelatedTo`):
//! a string literal or a template literal type below a template literal
//! pattern is matched by inferring each hole's slice
//! (`inferFromLiteralPartsToTemplateLiteral`) and checking it against the
//! hole (`isValidTypeForTemplateLiteralPlaceholder`).

use super::build::TemplatePiece;
use super::ProjectSemanticDispatch;
use crate::semantic_query::{LiteralValue, PrimitiveKind, SemanticNodeData, SemanticNodeId};

/// One side of a template relation: its texts (one more than its holes)
/// and its holes.
struct TemplateParts {
    texts: Vec<String>,
    holes: Vec<SemanticNodeId>,
}

impl ProjectSemanticDispatch<'_> {
    /// The verdict of `source` below the template literal pattern
    /// `target` — `Some(true)` / `Some(false)` when the checker's rule
    /// decides it, `None` when a slice's relation to its hole is undecided
    /// or the target is not a settled pattern (a hole the template reducer
    /// would still distribute or that is open).
    pub(super) fn template_pattern_accepts(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> Option<bool> {
        let target = self.template_parts(target)?;
        let source_parts = match self.graph().node_data(source).as_deref() {
            Some(SemanticNodeData::Literal(LiteralValue::String(text))) => TemplateParts {
                texts: vec![text.clone()],
                holes: Vec::new(),
            },
            Some(SemanticNodeData::TemplateLiteral { .. }) => self.template_parts(source)?,
            // `string` is not below any pattern other than `${string}`,
            // which the template reducer already reads as `string`.
            Some(SemanticNodeData::Primitive(PrimitiveKind::String)) => return Some(false),
            _ => return None,
        };
        let slices = if source_parts.texts == target.texts {
            source_parts.holes.clone()
        } else {
            match self.infer_template_slices(&source_parts, &target) {
                Some(slices) => slices,
                None => return Some(false),
            }
        };
        let mut undecided = false;
        for (slice, hole) in slices.iter().zip(&target.holes) {
            match self.valid_for_template_placeholder(*slice, *hole) {
                Some(true) => {}
                Some(false) => return Some(false),
                None => undecided = true,
            }
        }
        (!undecided).then_some(true)
    }

    /// Whether `node` is a settled template literal type (see
    /// [`Self::template_parts`]).
    pub(super) fn template_is_settled(&self, node: SemanticNodeId) -> bool {
        self.template_parts(node).is_some()
    }

    /// The verdict a string-mapping application decides with a string
    /// literal or `string` beside it: a literal is below `Uppercase<T>`
    /// when the mapping leaves it unchanged and it fits `T`
    /// (`isMemberOfStringMapping`), and an application over `string` or
    /// `any` is below `string` / `any`. `None` for every other pair.
    pub(super) fn string_mapping_relation(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> Option<bool> {
        let graph = self.graph();
        if self.string_mapping_of(target).is_some()
            && matches!(
                graph.node_data(source).as_deref(),
                Some(SemanticNodeData::Literal(LiteralValue::String(_)))
            )
        {
            return self.valid_for_template_placeholder(source, target);
        }
        if self.string_mapping_of(source).is_some()
            && matches!(
                graph.node_data(target).as_deref(),
                Some(SemanticNodeData::Primitive(
                    PrimitiveKind::String | PrimitiveKind::Any | PrimitiveKind::Unknown
                ))
            )
        {
            return Some(true);
        }
        None
    }

    /// A settled template literal type's parts: every hole a placeholder
    /// the reducer keeps (`string`, `number`, `bigint`, `any`, a string
    /// mapping over one). `None` for any other node, or a template whose
    /// quasis are not comparable raw text.
    fn template_parts(&self, node: SemanticNodeId) -> Option<TemplateParts> {
        match self.graph().node_data(node).as_deref() {
            Some(SemanticNodeData::TemplateLiteral {
                quasis,
                expressions,
            }) if quasis.len() == expressions.len() + 1
                && quasis.iter().all(|quasi| !quasi.contains('\\'))
                && expressions.iter().all(|hole| self.is_template_hole(*hole)) =>
            {
                Some(TemplateParts {
                    texts: quasis.iter().map(|quasi| quasi.to_string()).collect(),
                    holes: expressions.to_vec(),
                })
            }
            _ => None,
        }
    }

    /// `inferFromLiteralPartsToTemplateLiteral`: the slice of `source` each
    /// hole of `target` covers — each target text matched leftmost — as a
    /// string literal, the source's own hole, or the template literal type
    /// of the parts it spans. `None` when the texts do not fit.
    fn infer_template_slices(
        &self,
        source: &TemplateParts,
        target: &TemplateParts,
    ) -> Option<Vec<SemanticNodeId>> {
        let last_source = source.texts.len() - 1;
        let last_target = target.texts.len() - 1;
        let target_start = target.texts[0].as_str();
        let target_end = target.texts[last_target].as_str();
        let source_start = source.texts[0].as_str();
        let source_end = source.texts[last_source].as_str();
        if (last_source == 0 && source_start.len() < target_start.len() + target_end.len())
            || !source_start.starts_with(target_start)
            || !source_end.ends_with(target_end)
        {
            return None;
        }
        let remaining_end = &source_end[..source_end.len() - target_end.len()];
        let text_at = |index: usize| -> &str {
            if index < last_source {
                source.texts[index].as_str()
            } else {
                remaining_end
            }
        };
        let mut matches = Vec::with_capacity(target.holes.len());
        let mut seg = 0usize;
        let mut pos = target_start.len();
        let mut add_match = |s: usize, p: usize, seg: &mut usize, pos: &mut usize| {
            let node = if s == *seg {
                self.graph()
                    .intern_node(SemanticNodeData::Literal(LiteralValue::String(
                        text_at(s)[*pos..p].to_owned(),
                    )))
            } else {
                let mut pieces = vec![TemplatePiece::Text(source.texts[*seg][*pos..].to_owned())];
                for index in *seg..s {
                    pieces.push(TemplatePiece::Hole(source.holes[index]));
                    let text = if index + 1 == s {
                        text_at(s)[..p].to_owned()
                    } else {
                        source.texts[index + 1].clone()
                    };
                    pieces.push(TemplatePiece::Text(text));
                }
                self.template_type_from_pieces(pieces)
            };
            matches.push(node);
            *seg = s;
            *pos = p;
        };
        for delimiter in &target.texts[1..last_target] {
            if !delimiter.is_empty() {
                let mut s = seg;
                let mut from = pos;
                let found = loop {
                    if let Some(offset) = text_at(s)
                        .get(from..)
                        .and_then(|text| text.find(delimiter.as_str()))
                    {
                        break from + offset;
                    }
                    s += 1;
                    if s == source.texts.len() {
                        return None;
                    }
                    from = 0;
                };
                add_match(s, found, &mut seg, &mut pos);
                pos += delimiter.len();
            } else if pos < text_at(seg).len() {
                let next = pos + text_at(seg)[pos..].chars().next().map_or(1, char::len_utf8);
                add_match(seg, next, &mut seg, &mut pos);
            } else if seg < last_source {
                add_match(seg + 1, 0, &mut seg, &mut pos);
            } else {
                return None;
            }
        }
        add_match(last_source, text_at(last_source).len(), &mut seg, &mut pos);
        Some(matches)
    }

    /// `isValidTypeForTemplateLiteralPlaceholder`: whether a slice fits a
    /// hole. A string literal fits `number` / `bigint` when it is a valid
    /// numeric / bigint spelling, and a string mapping when the mapping
    /// leaves it unchanged; a lone-hole template fits by its hole; every
    /// other slice by assignability.
    fn valid_for_template_placeholder(
        &self,
        slice: SemanticNodeId,
        hole: SemanticNodeId,
    ) -> Option<bool> {
        let graph = self.graph();
        if slice == hole
            || matches!(
                graph.node_data(hole).as_deref(),
                Some(SemanticNodeData::Primitive(
                    PrimitiveKind::String | PrimitiveKind::Any
                ))
            )
        {
            return Some(true);
        }
        let slice_data = graph.node_data(slice);
        match slice_data.as_deref() {
            Some(SemanticNodeData::Literal(LiteralValue::String(value))) => {
                if let Some((intrinsic, operand)) = self.string_mapping_of(hole) {
                    if super::build::transform_string_intrinsic(&intrinsic, value) != *value {
                        return Some(false);
                    }
                    return self.valid_for_template_placeholder(slice, operand);
                }
                Some(match graph.node_data(hole).as_deref() {
                    Some(SemanticNodeData::Primitive(PrimitiveKind::Number)) => {
                        valid_number_string(value)
                    }
                    Some(SemanticNodeData::Primitive(PrimitiveKind::BigInt)) => {
                        valid_bigint_string(value)
                    }
                    Some(SemanticNodeData::TemplateLiteral { .. }) => {
                        return self.template_pattern_accepts(slice, hole);
                    }
                    _ => false,
                })
            }
            Some(SemanticNodeData::TemplateLiteral {
                quasis,
                expressions,
            }) => {
                let lone = (quasis.len() == 2
                    && quasis.iter().all(|quasi| quasi.is_empty())
                    && expressions.len() == 1)
                    .then(|| expressions[0]);
                drop(slice_data);
                self.relate_decided(lone.unwrap_or(slice), hole)
            }
            _ => {
                drop(slice_data);
                self.relate_decided(slice, hole)
            }
        }
    }

    fn relate_decided(&self, source: SemanticNodeId, target: SemanticNodeId) -> Option<bool> {
        match self.execute_relate_pair(source, target) {
            super::dispatch_txn::RelationStep::Assignable { .. } => Some(true),
            super::dispatch_txn::RelationStep::NotAssignable => Some(false),
            _ => None,
        }
    }
}

/// The checker's `isValidNumberString(s, false)`: `s` is not empty and
/// JavaScript's `+s` is finite.
fn valid_number_string(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    let trimmed = text.trim_matches(is_js_whitespace);
    if trimmed.is_empty() {
        return true;
    }
    for (prefix, radix) in [
        ("0x", 16),
        ("0X", 16),
        ("0o", 8),
        ("0O", 8),
        ("0b", 2),
        ("0B", 2),
    ] {
        if let Some(digits) = trimmed.strip_prefix(prefix) {
            return !digits.is_empty() && digits.chars().all(|c| c.is_digit(radix));
        }
    }
    let unsigned = trimmed
        .strip_prefix('+')
        .or_else(|| trimmed.strip_prefix('-'))
        .unwrap_or(trimmed);
    decimal_literal(unsigned)
}

/// A JavaScript `StrUnsignedDecimalLiteral` other than `Infinity` (whose
/// value is not finite).
fn decimal_literal(text: &str) -> bool {
    let (mantissa, exponent) = match text.find(['e', 'E']) {
        Some(index) => (&text[..index], Some(&text[index + 1..])),
        None => (text, None),
    };
    let (integer, fraction) = match mantissa.split_once('.') {
        Some((integer, fraction)) => (integer, Some(fraction)),
        None => (mantissa, None),
    };
    let digits = |part: &str| part.chars().all(|c| c.is_ascii_digit());
    let mantissa_ok = digits(integer)
        && fraction.is_none_or(digits)
        && !(integer.is_empty() && fraction.is_none_or(str::is_empty));
    let exponent_ok = exponent.is_none_or(|exponent| {
        let exponent = exponent
            .strip_prefix('+')
            .or_else(|| exponent.strip_prefix('-'))
            .unwrap_or(exponent);
        !exponent.is_empty() && digits(exponent)
    });
    mantissa_ok && exponent_ok
}

/// JavaScript's `StrWhiteSpaceChar`.
fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\u{9}' | '\u{a}' | '\u{b}' | '\u{c}' | '\u{d}' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

/// The checker's `isValidBigIntString(s, false)`: `s` followed by `n`
/// scans as one bigint literal token, an optional leading `-` aside — a
/// decimal without a leading zero, or a hex / octal / binary literal, with
/// no separator.
fn valid_bigint_string(text: &str) -> bool {
    let unsigned = text.strip_prefix('-').unwrap_or(text);
    for (prefix, radix) in [
        ("0x", 16),
        ("0X", 16),
        ("0o", 8),
        ("0O", 8),
        ("0b", 2),
        ("0B", 2),
    ] {
        if let Some(digits) = unsigned.strip_prefix(prefix) {
            return !digits.is_empty() && digits.chars().all(|c| c.is_digit(radix));
        }
    }
    unsigned == "0"
        || (unsigned.starts_with(|c: char| ('1'..='9').contains(&c))
            && unsigned.chars().all(|c| c.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::{valid_bigint_string, valid_number_string};

    /// Measured on TypeScript 7.0.2 through ``S extends `${number}` ``:
    /// `" 1"`, `"1 "`, `" "`, `"0x1F"`, `"0b1"`, `"0o7"`, `"-1.5e-3"`,
    /// `".5"`, `"5."`, `"+1"` and `"\t2\n"` are numbers; `""`, `"-0x1"`,
    /// `"Infinity"`, `"1_000"`, `"NaN"` and `"1e"` are not.
    #[test]
    fn a_number_placeholder_accepts_what_javascript_reads_as_finite() {
        for text in [
            " 1", "1 ", " ", "0x1F", "0b1", "0o7", "-1.5e-3", ".5", "5.", "+1", "\t2\n",
        ] {
            assert!(valid_number_string(text), "{text:?} is a number");
        }
        for text in ["", "-0x1", "Infinity", "1_000", "NaN", "1e"] {
            assert!(!valid_number_string(text), "{text:?} is not a number");
        }
    }

    /// Measured on TypeScript 7.0.2 through ``S extends `${bigint}` ``:
    /// `"10"`, `"-10"`, `"0x10"` and `"-0x10"` are bigints; `" 10"`,
    /// `"1_0"`, `"10.5"`, `"+10"`, `""` and `"010"` are not.
    #[test]
    fn a_bigint_placeholder_accepts_one_bigint_token() {
        for text in ["10", "-10", "0x10", "-0x10"] {
            assert!(valid_bigint_string(text), "{text:?} is a bigint");
        }
        for text in [" 10", "1_0", "10.5", "+10", "", "010"] {
            assert!(!valid_bigint_string(text), "{text:?} is not a bigint");
        }
    }
}
