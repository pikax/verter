//! Rendered comments append their authored HTML to the current template.
use crate::ast::types::CommentNode;

pub(super) fn process_comment(
    comment: &CommentNode,
    source: &str,
    comments: bool,
    html: &mut String,
) {
    if comments {
        html.push_str(&source[comment.start as usize..comment.end as usize]);
    }
}
