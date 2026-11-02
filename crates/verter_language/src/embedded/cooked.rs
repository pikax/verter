//! Scalar validation of cooked values.

use verter_span::SourceByteRange;

/// Which half of a surrogate pair stood alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SurrogateKind {
    High,
    Low,
}

/// A cooked value holding a code unit that is no Unicode scalar: a lone high
/// or low surrogate.
///
/// It carries no embedded offset, because no UTF-8 position exists for such a
/// unit. `authored` names the escape that produced it when the value came from
/// authored text; a host-supplied value has none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NonUnicodeCookedLiteral {
    pub surrogate: SurrogateKind,
    pub authored: Option<SourceByteRange>,
}

/// Cooked selection over a literal with no cooked value, such as a tagged
/// template chunk with an invalid escape. There is no fallback to raw text.
/// `authored` names the invalid escape when the codec found it itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MissingCookedValue {
    pub authored: Option<SourceByteRange>,
}

/// Why a cooked value produced no [`CookedText`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CookedPartiality {
    NonUnicode(NonUnicodeCookedLiteral),
    Missing(MissingCookedValue),
}

/// A complete cooked value, validated as Unicode scalars and encoded as UTF-8.
/// Only validated input constructs one, so holding it proves every byte offset
/// on one of its scalar boundaries is a true position.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CookedText(String);

impl CookedText {
    /// Validate a host literal's code units in full. A paired surrogate becomes
    /// one scalar; a lone one is refused, never replaced by U+FFFD.
    pub fn from_code_units(units: &[u16]) -> Result<Self, NonUnicodeCookedLiteral> {
        let mut text = String::with_capacity(units.len());
        for decoded in char::decode_utf16(units.iter().copied()) {
            match decoded {
                Ok(scalar) => text.push(scalar),
                Err(lone) => {
                    let surrogate = if lone.unpaired_surrogate() < 0xDC00 {
                        SurrogateKind::High
                    } else {
                        SurrogateKind::Low
                    };
                    return Err(NonUnicodeCookedLiteral {
                        surrogate,
                        authored: None,
                    });
                }
            }
        }
        Ok(Self(text))
    }

    /// Validate a host parser's optional cooked value: `None` is the host
    /// saying the literal has no cooked value at all.
    pub fn from_host_value(units: Option<&[u16]>) -> Result<Self, CookedPartiality> {
        let units = units.ok_or(CookedPartiality::Missing(MissingCookedValue {
            authored: None,
        }))?;
        Self::from_code_units(units).map_err(CookedPartiality::NonUnicode)
    }

    /// Text the decoder assembled from authored UTF-8 and validated scalars.
    pub(super) fn from_validated(text: String) -> Self {
        Self(text)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}
