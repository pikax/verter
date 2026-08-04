use verter_language::parse_artifact::carrier_inventory::{
    CarrierBlock, MarkupNodeKind, SectionRole,
};
use verter_session::carrier_publication_store::RegisteredFileStructure;

const VOID_TAGS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];
const CURRENT_TOKEN_LIMIT: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CarrierKind {
    Vue,
    Svelte,
}

pub(crate) fn carrier_kind_for_language(
    language: &verter_session::FileLanguage,
) -> Option<CarrierKind> {
    use verter_protocol::typeinfo::graph::FrameworkTag;
    let adapter_id = language.adapter_id()?;
    let carrier_language = language.carrier_language_id()?;
    let tag = verter_session::framework::descriptor::built_in_descriptors()
        .into_iter()
        .find(|descriptor| {
            &descriptor.id == adapter_id
                && descriptor.carrier_language.as_ref() == Some(carrier_language)
        })
        .map(|descriptor| descriptor.tag)?;
    match tag {
        FrameworkTag::Vue => Some(CarrierKind::Vue),
        FrameworkTag::Svelte => Some(CarrierKind::Svelte),
        _ => None,
    }
}

/// Resolve an on-type close from registered syntax plus at most 256 bytes of
/// cursor-local recovery lexing. No root or block delimiter scan is performed.
pub fn auto_close_tag_in_structure(
    source: &str,
    offset: usize,
    carrier: CarrierKind,
    structure: &RegisteredFileStructure,
) -> Option<String> {
    if offset == 0 || offset > source.len() || source.as_bytes().get(offset - 1) != Some(&b'>') {
        return None;
    }
    let gt = (offset - 1) as u32;
    let (window_start, window_end) = markup_window(structure, gt, carrier, source.len() as u32)?;

    let inventory = structure.inventory();
    for node in inventory.markup().nodes() {
        match node.kind() {
            MarkupNodeKind::Element(element) => {
                if element.attributes.iter().any(|attribute| {
                    span_contains(attribute.full_span().start, attribute.full_span().end, gt)
                }) {
                    return None;
                }
            }
            other if span_contains(other.full_span().start, other.full_span().end, gt) => {
                return None;
            }
            _ => {}
        }
    }

    let parsed = inventory.markup().nodes().iter().find_map(|node| {
        let MarkupNodeKind::Element(element) = node.kind() else {
            return None;
        };
        (element.opening_span.end == offset as u32
            && element.opening_span.start >= window_start
            && element.opening_span.end <= window_end)
            .then_some(element)
    });

    if let Some(element) = parsed {
        if element.self_closing || element.void_element || element.closing_span.is_some() {
            return None;
        }
        let name = inventory.slice(element.authored_name).ok()?;
        if VOID_TAGS.contains(&name.to_ascii_lowercase().as_str())
            || has_immediate_close(source, offset, name)
        {
            return None;
        }
        return Some(format!("$0</{name}>"));
    }

    let name = bounded_current_open_tag(source, offset, window_start as usize)?;
    if VOID_TAGS.contains(&name.to_ascii_lowercase().as_str())
        || has_immediate_close(source, offset, name)
    {
        return None;
    }
    Some(format!("$0</{name}>"))
}

fn markup_window(
    structure: &RegisteredFileStructure,
    offset: u32,
    carrier: CarrierKind,
    source_len: u32,
) -> Option<(u32, u32)> {
    let inventory = structure.inventory();
    match carrier {
        CarrierKind::Vue => inventory
            .blocks()
            .iter()
            .enumerate()
            .find_map(|(index, block)| match block {
                CarrierBlock::Section {
                    role: SectionRole::TemplateHost,
                    syntax,
                    ..
                } => {
                    let end = syntax.closing_span.map_or_else(
                        || {
                            inventory.blocks()[index + 1..]
                                .iter()
                                .find_map(|next| inventory.block_start(next).ok())
                                .unwrap_or(source_len)
                        },
                        |closing| closing.start,
                    );
                    (offset >= syntax.opening_span.end && offset < end)
                        .then_some((syntax.opening_span.end, end))
                }
                _ => None,
            }),
        CarrierKind::Svelte => {
            let inside_non_markup = inventory.blocks().iter().any(|block| match block {
                CarrierBlock::Section {
                    role: SectionRole::Script { .. } | SectionRole::Style { .. },
                    syntax,
                    ..
                } => offset >= syntax.full_span.start && offset < syntax.full_span.end,
                _ => false,
            });
            (!inside_non_markup).then_some((0, source_len))
        }
    }
}

fn span_contains(start: u32, end: u32, point: u32) -> bool {
    point >= start && point < end
}

fn has_immediate_close(source: &str, offset: usize, name: &str) -> bool {
    let bytes = source.as_bytes();
    let limit = bytes.len().min(offset.saturating_add(CURRENT_TOKEN_LIMIT));
    let mut cursor = offset;
    while cursor < limit && bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    let Some(rest) = source.get(cursor..limit) else {
        return false;
    };
    let Some(candidate) = rest.get(2..2 + name.len()) else {
        return false;
    };
    rest.starts_with("</")
        && candidate.eq_ignore_ascii_case(name)
        && rest
            .as_bytes()
            .get(2 + name.len())
            .is_some_and(|byte| byte.is_ascii_whitespace() || matches!(*byte, b'>' | b'/'))
}

fn bounded_current_open_tag(source: &str, offset: usize, window_start: usize) -> Option<&str> {
    let bytes = source.as_bytes();
    let start = window_start.max(offset.saturating_sub(CURRENT_TOKEN_LIMIT));
    let region = &bytes[start..offset];
    let relative_lt = region.iter().rposition(|byte| *byte == b'<')?;
    let candidate = &region[relative_lt..];
    if candidate[..candidate.len().saturating_sub(1)].contains(&b'>') {
        return None;
    }
    if candidate
        .get(1)
        .is_none_or(|byte| matches!(*byte, b'/' | b'!' | b'?' | b'>'))
    {
        return None;
    }
    if candidate.get(candidate.len().saturating_sub(2)) == Some(&b'/') {
        return None;
    }

    let mut name_end = 1;
    while candidate.get(name_end).is_some_and(|byte| {
        byte.is_ascii_alphanumeric() || matches!(*byte, b'-' | b'_' | b':' | b'.')
    }) {
        name_end += 1;
    }
    if name_end == 1 {
        return None;
    }

    let mut quote = None;
    let mut braces = 0u16;
    for byte in &candidate[name_end..candidate.len().saturating_sub(1)] {
        match quote {
            Some(open) if *byte == open => quote = None,
            Some(_) => {}
            None if matches!(*byte, b'\'' | b'"') => quote = Some(*byte),
            None if *byte == b'{' => braces = braces.saturating_add(1),
            None if *byte == b'}' => braces = braces.saturating_sub(1),
            None => {}
        }
    }
    if quote.is_some() || braces != 0 {
        return None;
    }
    std::str::from_utf8(&candidate[1..name_end]).ok()
}

#[cfg(test)]
pub fn auto_close_tag_in_carrier(
    source: &str,
    offset: usize,
    carrier: CarrierKind,
) -> Option<String> {
    let structure =
        crate::documents::carrier_structure::test_structure(source, carrier == CarrierKind::Svelte);
    auto_close_tag_in_structure(source, offset, carrier, &structure)
}

#[cfg(test)]
pub fn auto_close_tag(source: &str, offset: usize) -> Option<String> {
    let name = bounded_current_open_tag(source, offset, 0)?;
    (!VOID_TAGS.contains(&name.to_ascii_lowercase().as_str())
        && !has_immediate_close(source, offset, name))
    .then(|| format!("$0</{name}>"))
}

#[cfg(test)]
mod bounded_tests {
    use super::*;

    #[test]
    fn recovery_lexer_is_strictly_cursor_local() {
        let prefix = "x".repeat(CURRENT_TOKEN_LIMIT + 1);
        let source = format!("<{prefix}<Panel>");
        assert_eq!(
            bounded_current_open_tag(&source, source.len(), 0),
            Some("Panel")
        );
        assert_eq!(source.len() - source.rfind('<').unwrap(), "<Panel>".len());
    }
}

#[cfg(test)]
mod tests {
    include!("auto_close_tag_tests.rs");
}
