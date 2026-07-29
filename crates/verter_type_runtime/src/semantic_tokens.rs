//! The single owner of semantic-token legend mapping for every
//! provider-backed lane.
//!
//! Verter's LSP advertises ONE published [`SemanticTokensLegend`]-shaped
//! vocabulary ([`VERTER_TOKEN_TYPES`] / [`VERTER_TOKEN_MODIFIERS`]); every
//! [`crate::protocol::SemanticToken`] that crosses the `TypeProvider` boundary
//! carries indices into THAT legend, never into a provider's own. Each backend
//! speaks a different token vocabulary:
//!
//! - **tsserver-family** (`encodedSemanticClassifications-full` with
//!   `"format": "2020"`): each span's classification packs
//!   `((tokenTypeIdx + 1) << 8) | modifierSet` over TypeScript's fixed
//!   classifier-2020 legend ([`TS_CLASSIFICATION_2020_TOKEN_TYPES`] /
//!   [`TS_CLASSIFICATION_2020_TOKEN_MODIFIERS`], `typescript`'s
//!   `classifier2020.ts` `TokenType` / `TokenModifier` enums — the `member`
//!   entry is spelled `method` here, matching the legend name VS Code's own
//!   TypeScript extension publishes for it).
//! - **tsgo** (LSP-native `textDocument/semanticTokens/full`): indices into the
//!   legend the SERVER advertises in its `initialize` result. That legend's
//!   order is server-owned and version-dependent (observed live on the pinned
//!   7.0.2: 22 types with `interface = 3`, where Verter publishes
//!   `interface = 4`), so it must be RETAINED at initialize time and remapped
//!   per token — never assumed to match anyone else's order.
//!
//! Both lanes remap by NAME through [`SemanticTokenLegendMap`]: token types are
//! an index lookup, modifiers are a per-bit lookup (a bitset does NOT remap as
//! a unit). Anything unmappable fails closed — the token is dropped rather
//! than emitted with a guessed kind (Carrier IDE TS Surface Principle: absent
//! beats wrong).

use crate::protocol::SemanticToken;

/// Verter's published semantic-token TYPES, in published-index order.
///
/// This array is the single source of truth for the `SemanticTokensLegend` the
/// LSP advertises (`verter_lsp::capabilities` builds the wire legend from it)
/// and for the target space of every [`SemanticTokenLegendMap`].
pub const VERTER_TOKEN_TYPES: [&str; 23] = [
    "namespace",
    "type",
    "class",
    "enum",
    "interface",
    "struct",
    "typeParameter",
    "parameter",
    "variable",
    "property",
    "enumMember",
    "event",
    "function",
    "method",
    "macro",
    "keyword",
    "modifier",
    "comment",
    "string",
    "number",
    "regexp",
    "operator",
    "decorator",
];

/// Verter's published semantic-token MODIFIERS, in published-bit order.
///
/// The first ten are the LSP-standard modifiers; `local` (bit 10) is the
/// TypeScript-family extension modifier both tsserver's classifier-2020 legend
/// and tsgo's advertised legend emit (VS Code's TypeScript extension publishes
/// it too). Without it every function-scoped binding's token would have to be
/// dropped by the fail-closed rule.
pub const VERTER_TOKEN_MODIFIERS: [&str; 11] = [
    "declaration",
    "definition",
    "readonly",
    "static",
    "deprecated",
    "abstract",
    "async",
    "modification",
    "documentation",
    "defaultLibrary",
    "local",
];

/// TypeScript's classifier-2020 token-type legend, by encoded index.
///
/// Order is fixed by `typescript`'s `classifier2020.ts` `TokenType` enum:
/// `class, enum, interface, namespace, typeParameter, type, parameter,
/// variable, enumMember, property, function, member`. The final entry is
/// spelled `method` — the published legend name VS Code's TypeScript extension
/// uses for `TokenType.member`.
pub const TS_CLASSIFICATION_2020_TOKEN_TYPES: [&str; 12] = [
    "class",
    "enum",
    "interface",
    "namespace",
    "typeParameter",
    "type",
    "parameter",
    "variable",
    "enumMember",
    "property",
    "function",
    "method",
];

/// TypeScript's classifier-2020 token-modifier legend, by encoded bit.
///
/// Order is fixed by `classifier2020.ts`'s `TokenModifier` enum.
pub const TS_CLASSIFICATION_2020_TOKEN_MODIFIERS: [&str; 6] = [
    "declaration",
    "static",
    "async",
    "readonly",
    "defaultLibrary",
    "local",
];

/// Decode a `"format": "2020"` classification into
/// `(ts_token_type_index, ts_modifier_set)` — both still in TS classifier-2020
/// space.
///
/// The packing is `((tokenTypeIdx + 1) << 8) | modifierSet`
/// (`typescript`'s `classifier2020.ts`, `TokenEncodingConsts.typeOffset = 8`).
/// A classification below `1 << 8` carries no `+1`-offset type field and is
/// undecodable — `None`, never a guessed `(0, bits)`.
pub fn decode_classification_2020(classification: u32) -> Option<(u32, u32)> {
    let type_plus_one = classification >> 8;
    if type_plus_one == 0 {
        return None;
    }
    Some((type_plus_one - 1, classification & 0xFF))
}

/// A name-built remap from ONE provider legend into Verter's published legend.
///
/// Built once per provider session (or once statically for the fixed
/// tsserver-2020 legend) and applied per token. Provider entries whose name
/// Verter does not publish map to `None` and fail closed at
/// [`Self::map_token`].
#[derive(Debug)]
pub struct SemanticTokenLegendMap {
    /// Provider type index → Verter type index.
    type_map: Vec<Option<u32>>,
    /// Provider modifier BIT → Verter modifier bit.
    modifier_map: Vec<Option<u32>>,
}

impl SemanticTokenLegendMap {
    /// Build a map from a provider legend given as name slices.
    pub fn from_names<T: AsRef<str>, M: AsRef<str>>(
        token_types: &[T],
        token_modifiers: &[M],
    ) -> Self {
        let type_map = token_types
            .iter()
            .map(|name| {
                VERTER_TOKEN_TYPES
                    .iter()
                    .position(|v| *v == name.as_ref())
                    .map(|i| i as u32)
            })
            .collect();
        let modifier_map = token_modifiers
            .iter()
            .map(|name| {
                VERTER_TOKEN_MODIFIERS
                    .iter()
                    .position(|v| *v == name.as_ref())
                    .map(|i| i as u32)
            })
            .collect();
        Self {
            type_map,
            modifier_map,
        }
    }

    /// Build a map from a JSON `SemanticTokensLegend`
    /// (`{ "tokenTypes": [...], "tokenModifiers": [...] }`).
    ///
    /// Returns `None` when the value is not legend-shaped or advertises an
    /// EMPTY type list — an empty legend cannot map any token, and retaining it
    /// would silently drop everything while claiming a legend was negotiated.
    pub fn from_legend_json(legend: &serde_json::Value) -> Option<Self> {
        let token_types: Vec<&str> = legend
            .get("tokenTypes")?
            .as_array()?
            .iter()
            .map(|v| v.as_str())
            .collect::<Option<Vec<_>>>()?;
        if token_types.is_empty() {
            return None;
        }
        let token_modifiers: Vec<&str> = legend
            .get("tokenModifiers")?
            .as_array()?
            .iter()
            .map(|v| v.as_str())
            .collect::<Option<Vec<_>>>()?;
        Some(Self::from_names(&token_types, &token_modifiers))
    }

    /// Extract and build the legend map from an LSP `initialize` RESULT
    /// (`capabilities.semanticTokensProvider.legend`).
    pub fn from_initialize_result(init_result: &serde_json::Value) -> Option<Self> {
        let legend = init_result
            .get("capabilities")?
            .get("semanticTokensProvider")?
            .get("legend")?;
        Self::from_legend_json(legend)
    }

    /// The fixed map for tsserver-family `"format": "2020"` classifications.
    pub fn ts_classification_2020() -> &'static SemanticTokenLegendMap {
        static MAP: std::sync::OnceLock<SemanticTokenLegendMap> = std::sync::OnceLock::new();
        MAP.get_or_init(|| {
            SemanticTokenLegendMap::from_names(
                &TS_CLASSIFICATION_2020_TOKEN_TYPES,
                &TS_CLASSIFICATION_2020_TOKEN_MODIFIERS,
            )
        })
    }

    /// Remap one provider-space token into Verter's published legend space.
    ///
    /// Fails closed: an out-of-range or unpublished token type, an out-of-range
    /// modifier bit, or a modifier bit whose name Verter does not publish all
    /// return `None` — the caller drops the token rather than emit a wrong
    /// kind.
    pub fn map_token(&self, token_type: u32, token_modifiers: u32) -> Option<(u32, u32)> {
        let mapped_type = (*self.type_map.get(token_type as usize)?)?;
        let mut mapped_modifiers = 0u32;
        let mut remaining = token_modifiers;
        while remaining != 0 {
            let bit = remaining.trailing_zeros();
            remaining &= remaining - 1;
            let mapped_bit = (*self.modifier_map.get(bit as usize)?)?;
            mapped_modifiers |= 1 << mapped_bit;
        }
        Some((mapped_type, mapped_modifiers))
    }
}

/// Walk a tsserver-family `encodedSemanticClassifications-full` (`"2020"`)
/// `spans` array — `[start, length, classification, ...]` triplets — decoding
/// each classification and remapping it into Verter's published legend space.
///
/// This is the ONE implementation both the managed-tsserver provider and the
/// extension-hosted provider consume. Unmappable classifications drop their
/// span (fail closed); trailing partial triplets are ignored.
///
/// The engine's span `start`/`length` are UTF-16 CODE-UNIT offsets
/// (`ts.LanguageService.getEncodedSemanticClassifications` spans). The
/// [`SemanticToken`] contract is BYTE offsets into the queried file, so when
/// `content` is available each span converts through it; a span that does not
/// land on the text (out of range) is dropped rather than emitted at a wrong
/// position. Without `content` the raw offsets pass through — correct only
/// for pure-ASCII text, which callers avoid by always passing their cached
/// content.
pub fn map_classified_spans_2020(
    spans: &[serde_json::Value],
    content: Option<&str>,
) -> Vec<SemanticToken> {
    let map = SemanticTokenLegendMap::ts_classification_2020();
    let mut cursor = content.map(Utf16ToByteCursor::new);
    let mut tokens = Vec::with_capacity(spans.len() / 3);
    for triplet in spans.chunks_exact(3) {
        let Some(start) = triplet[0]
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
        else {
            continue;
        };
        let Some(length) = triplet[1]
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
        else {
            continue;
        };
        let Some(classification) = triplet[2]
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
        else {
            continue;
        };
        let Some((ts_type, ts_modifiers)) = decode_classification_2020(classification) else {
            continue;
        };
        let Some((token_type, token_modifiers)) = map.map_token(ts_type, ts_modifiers) else {
            continue;
        };
        let (start, length) = match cursor.as_mut() {
            Some(cursor) => {
                let Some(byte_start) = cursor.byte_offset(start) else {
                    continue;
                };
                let Some(utf16_end) = start.checked_add(length) else {
                    continue;
                };
                let Some(byte_end) = cursor.byte_offset(utf16_end) else {
                    continue;
                };
                (byte_start, byte_end - byte_start)
            }
            None => (start, length),
        };
        tokens.push(SemanticToken {
            start,
            length,
            token_type,
            token_modifiers,
        });
    }
    tokens
}

/// A resumable UTF-16 code-unit → byte offset converter.
///
/// Classification spans arrive in ascending order, so conversion is a single
/// forward walk; a backwards target restarts from the beginning (correctness
/// over speed for the defensive case).
struct Utf16ToByteCursor<'a> {
    content: &'a str,
    utf16_pos: u32,
    byte_pos: u32,
}

impl<'a> Utf16ToByteCursor<'a> {
    fn new(content: &'a str) -> Self {
        Self {
            content,
            utf16_pos: 0,
            byte_pos: 0,
        }
    }

    /// The byte offset for a UTF-16 code-unit offset, or `None` when the
    /// target is past the end of the text or lands inside a surrogate pair.
    fn byte_offset(&mut self, utf16_target: u32) -> Option<u32> {
        if utf16_target < self.utf16_pos {
            self.utf16_pos = 0;
            self.byte_pos = 0;
        }
        for c in self.content[self.byte_pos as usize..].chars() {
            if self.utf16_pos >= utf16_target {
                break;
            }
            self.utf16_pos += c.len_utf16() as u32;
            self.byte_pos += c.len_utf8() as u32;
        }
        (self.utf16_pos == utf16_target).then_some(self.byte_pos)
    }
}

#[cfg(test)]
#[path = "semantic_tokens_tests.rs"]
mod tests;
