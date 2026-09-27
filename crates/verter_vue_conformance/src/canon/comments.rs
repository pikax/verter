//! Semantic-comment anchoring (PURE, license, JSDoc, bundler-significant) —
//! extracted from `canon.rs` (see `mod.rs`).

use std::collections::HashMap;

use oxc_ast::CommentContent;
use oxc_semantic::{NodeId, Semantic};
use oxc_span::GetSpan;

use super::Canon;

// ---------------------------------------------------------------------------
// Semantic comments (PURE, license, JSDoc, bundler-significant), anchored to
// the smallest AST node whose span contains them. Ordinary comments
// (`CommentContent::None`, e.g. Vue's `/* TEXT */` patch-flag annotations)
// are cosmetic and dropped.
// ---------------------------------------------------------------------------

pub(crate) fn anchor_comments(semantic: &Semantic, source: &str) -> HashMap<NodeId, Vec<Canon>> {
    let mut out: HashMap<NodeId, Vec<Canon>> = HashMap::new();
    for comment in semantic.comments() {
        // A `#__KEY__` comment had no class before oxc 0.151.
        if matches!(
            comment.content,
            CommentContent::None | CommentContent::PropertyKey
        ) {
            continue;
        }
        let class = match comment.content {
            CommentContent::Legal => "legal",
            CommentContent::Jsdoc => "jsdoc",
            CommentContent::JsdocLegal => "jsdoc-legal",
            CommentContent::Pure => "pure",
            CommentContent::PureNotApplied => "pure-not-applied",
            // Before oxc 0.151 an unapplied `#__NO_SIDE_EFFECTS__` was `NoSideEffects`.
            CommentContent::NoSideEffects | CommentContent::NoSideEffectsNotApplied => {
                "no-side-effects"
            }
            CommentContent::Webpack => "webpack",
            CommentContent::Vite => "vite",
            CommentContent::CoverageIgnore | CommentContent::CoverageIgnoreFile => {
                "coverage-ignore"
            }
            CommentContent::Turbopack => "turbopack",
            CommentContent::None | CommentContent::PropertyKey => unreachable!(),
        };
        let raw = &source[comment.content_span()];
        let text: String = raw.split_whitespace().collect::<Vec<_>>().join(" ");
        // Smallest enclosing AST node = the occurrence anchor.
        let mut best: Option<(u32, NodeId)> = None;
        for node in semantic.nodes().iter() {
            let span = node.kind().span();
            if span.start <= comment.span.start && comment.span.end <= span.end {
                let len = span.end - span.start;
                if best.is_none_or(|(best_len, _)| len < best_len) {
                    best = Some((len, node.id()));
                }
            }
        }
        if let Some((_, node_id)) = best {
            out.entry(node_id).or_default().push(Canon::node(
                "comment",
                vec![Canon::leaf("class", class), Canon::leaf("text", text)],
            ));
        }
    }
    // Deterministic order within one anchor.
    for comments in out.values_mut() {
        comments.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
    }
    out
}
