/// Extract a debug snippet around `offset` in `content`, returning
/// `(before_cursor, after_cursor)`.
///
/// Returns `None` if the offset is out of bounds.
pub(in crate::server) fn debug_snippet(content: &str, offset: usize) -> Option<(String, String)> {
    if offset > content.len() {
        return None;
    }

    // Snap to char boundaries so we never slice inside a multi-byte UTF-8
    // sequence.
    let snippet_start = content.floor_char_boundary(offset.saturating_sub(20));
    let snippet_end = content.ceil_char_boundary((offset + 30).min(content.len()));
    let cursor = content.floor_char_boundary(offset);
    if snippet_end <= snippet_start || cursor < snippet_start || cursor > snippet_end {
        return None;
    }

    let before = &content[snippet_start..cursor];
    let after = &content[cursor..snippet_end];
    Some((before.to_string(), after.to_string()))
}
