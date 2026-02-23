//! Canonical ID normalization, virtual ID rendering, and relative import resolution.

use std::borrow::Cow;

use crate::types::{FileMeta, ParsedRawId, VirtualNodeKind};

pub(crate) fn canonicalize_id(input: &str) -> Cow<'_, str> {
    let trimmed = input.trim();
    // Fast path: no transformations needed — avoid allocations
    if trimmed.len() == input.len()
        && !trimmed.contains('\\')
        && !trimmed.contains('?')
        && !trimmed.contains("._VERTER_.")
    {
        return Cow::Borrowed(trimmed);
    }
    let mut s = trimmed.replace('\\', "/");
    if let Some((base, _)) = s.split_once('?') {
        s = base.to_string();
    }
    if let Some((base, _)) = s.split_once("._VERTER_.") {
        s = base.to_string();
    }
    Cow::Owned(s)
}

pub(crate) fn parse_raw_id(raw: &str) -> Option<ParsedRawId> {
    let trimmed = raw.trim();
    let normalized_raw: Cow<'_, str> = if trimmed.contains('\\') {
        Cow::Owned(trimmed.replace('\\', "/"))
    } else {
        Cow::Borrowed(trimmed)
    };

    if let Some((canonical, suffix)) = normalized_raw.split_once("._VERTER_.") {
        let canonical = canonicalize_id(canonical);
        let node_kind = if suffix.starts_with("bundle.ts") {
            VirtualNodeKind::Main
        } else if suffix.starts_with("options.ts") {
            VirtualNodeKind::Script
        } else if suffix.starts_with("render.tsx") {
            VirtualNodeKind::Template
        } else if let Some(rest) = suffix.strip_prefix("style.") {
            let index = rest
                .split('.')
                .next()
                .and_then(|p| p.parse::<usize>().ok())
                .unwrap_or(0);
            VirtualNodeKind::Style { index }
        } else if let Some(rest) = suffix.strip_prefix("custom.") {
            let index = rest
                .split('.')
                .next()
                .and_then(|p| p.parse::<usize>().ok())
                .unwrap_or(0);
            VirtualNodeKind::Custom { index }
        } else {
            return None;
        };

        return Some(ParsedRawId {
            canonical_id: canonical.into_owned(),
            node_kind,
            was_lsp_like: true,
        });
    }

    let (base, query) = if let Some((b, q)) = normalized_raw.split_once('?') {
        (b, Some(q))
    } else {
        (normalized_raw.as_ref(), None)
    };

    let canonical = canonicalize_id(base);
    let mut ty: Option<String> = None;
    let mut index: Option<usize> = None;

    if let Some(q) = query {
        for chunk in q.split('&') {
            if chunk.is_empty() {
                continue;
            }
            if chunk.eq_ignore_ascii_case("vue") || chunk.eq_ignore_ascii_case("verter") {
                continue;
            }
            if chunk.starts_with("lang.") {
                continue;
            }
            let (k, v_opt) = if let Some((k, v)) = chunk.split_once('=') {
                (k.to_ascii_lowercase(), Some(v))
            } else {
                (chunk.to_ascii_lowercase(), None)
            };
            match k.as_str() {
                "type" => {
                    ty = v_opt.map(|v| v.to_ascii_lowercase());
                }
                "index" => {
                    if let Some(v) = v_opt {
                        index = v.parse::<usize>().ok();
                    }
                }
                _ => {}
            }
        }
    }

    let node_kind = match ty.as_deref() {
        Some("script") => VirtualNodeKind::Script,
        Some("template") => VirtualNodeKind::Template,
        Some("style") => VirtualNodeKind::Style {
            index: index.unwrap_or(0),
        },
        Some("custom") => VirtualNodeKind::Custom {
            index: index.unwrap_or(0),
        },
        Some(_) => {
            if index.is_some() {
                VirtualNodeKind::Custom {
                    index: index.unwrap_or(0),
                }
            } else {
                VirtualNodeKind::Main
            }
        }
        None => VirtualNodeKind::Main,
    };

    Some(ParsedRawId {
        canonical_id: canonical.into_owned(),
        node_kind,
        was_lsp_like: false,
    })
}

pub(crate) fn resolve_external(owner: &str, specifier: &str) -> String {
    if specifier.starts_with('/') {
        return canonicalize_id(specifier).into_owned();
    }
    if specifier.starts_with(".") {
        let mut parts: Vec<&str> = owner.split('/').collect();
        parts.pop(); // remove filename
                     // Track whether the owner had a root prefix (leading empty segment from "/...")
        let had_root = parts.first() == Some(&"");
        for segment in specifier.split('/') {
            match segment {
                "." | "" => {}
                ".." => {
                    // Guard: don't pop past the root segment (empty string from leading /)
                    if parts.len() > 1 || (parts.len() == 1 && !had_root) {
                        let _ = parts.pop();
                    }
                }
                other => parts.push(other),
            }
        }
        return parts.join("/");
    }
    canonicalize_id(specifier).into_owned()
}

pub(crate) fn render_ids(
    canonical_id: &str,
    node: &VirtualNodeKind,
    meta: &FileMeta,
) -> (String, String) {
    match node {
        VirtualNodeKind::Main => (
            canonical_id.to_string(),
            format!("{}._VERTER_.bundle.ts", canonical_id),
        ),
        VirtualNodeKind::Script => (
            format!("{}?vue&type=script", canonical_id),
            format!("{}._VERTER_.options.ts", canonical_id),
        ),
        VirtualNodeKind::Template => (
            format!("{}?vue&type=template", canonical_id),
            format!("{}._VERTER_.render.tsx", canonical_id),
        ),
        VirtualNodeKind::Style { index } => {
            let lang = meta
                .style_langs
                .get(*index)
                .cloned()
                .flatten()
                .unwrap_or_else(|| "css".to_string());
            (
                format!(
                    "{}?vue&type=style&index={}&lang.{}",
                    canonical_id, index, lang
                ),
                format!("{}._VERTER_.style.{}.{}", canonical_id, index, lang),
            )
        }
        VirtualNodeKind::Custom { index } => {
            let block_type = meta
                .custom_types
                .get(*index)
                .cloned()
                .unwrap_or_else(|| "custom".to_string());
            (
                format!(
                    "{}?vue&type=custom&index={}&blockType={}",
                    canonical_id, index, block_type
                ),
                format!("{}._VERTER_.custom.{}.{}", canonical_id, index, block_type),
            )
        }
    }
}

pub(crate) fn render_single_id(
    canonical_id: &str,
    node: &VirtualNodeKind,
    meta: &FileMeta,
    lsp: bool,
) -> String {
    let (bundler, lsp_id) = render_ids(canonical_id, node, meta);
    if lsp {
        lsp_id
    } else {
        bundler
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_raw_id_bare_path_returns_main() {
        let result = parse_raw_id("Comp.vue").unwrap();
        assert_eq!(result.canonical_id, "Comp.vue");
        assert_eq!(result.node_kind, VirtualNodeKind::Main);
        assert!(!result.was_lsp_like);
    }

    #[test]
    fn parse_raw_id_empty_query_returns_main() {
        let result = parse_raw_id("Comp.vue?").unwrap();
        assert_eq!(result.canonical_id, "Comp.vue");
        assert_eq!(result.node_kind, VirtualNodeKind::Main);
    }

    #[test]
    fn parse_raw_id_unknown_type_without_index_returns_main() {
        let result = parse_raw_id("Comp.vue?type=foo").unwrap();
        assert_eq!(result.node_kind, VirtualNodeKind::Main);
    }

    #[test]
    fn parse_raw_id_unknown_type_with_index_returns_custom() {
        let result = parse_raw_id("Comp.vue?type=foo&index=3").unwrap();
        assert_eq!(result.node_kind, VirtualNodeKind::Custom { index: 3 });
    }

    #[test]
    fn parse_raw_id_lsp_style_format() {
        let result = parse_raw_id("Comp.vue._VERTER_.bundle.ts").unwrap();
        assert_eq!(result.canonical_id, "Comp.vue");
        assert_eq!(result.node_kind, VirtualNodeKind::Main);
        assert!(result.was_lsp_like);
    }

    #[test]
    fn parse_raw_id_lsp_custom_format() {
        let result = parse_raw_id("Comp.vue._VERTER_.custom.2.docs").unwrap();
        assert_eq!(result.canonical_id, "Comp.vue");
        assert_eq!(result.node_kind, VirtualNodeKind::Custom { index: 2 });
        assert!(result.was_lsp_like);
    }

    #[test]
    fn parse_raw_id_lsp_unknown_suffix_returns_none() {
        assert!(parse_raw_id("Comp.vue._VERTER_.unknown.ts").is_none());
    }

    #[test]
    fn parse_raw_id_windows_backslashes_normalized() {
        let result = parse_raw_id("C:\\Users\\foo\\Comp.vue").unwrap();
        assert_eq!(result.canonical_id, "C:/Users/foo/Comp.vue");
    }

    #[test]
    fn parse_raw_id_style_default_index_zero() {
        let result = parse_raw_id("Comp.vue?vue&type=style").unwrap();
        assert_eq!(result.node_kind, VirtualNodeKind::Style { index: 0 });
    }

    #[test]
    fn parse_raw_id_case_insensitive_type_param() {
        let result = parse_raw_id("Comp.vue?TYPE=SCRIPT").unwrap();
        assert_eq!(result.node_kind, VirtualNodeKind::Script);
    }

    #[test]
    fn resolve_external_absolute() {
        assert_eq!(
            resolve_external("/src/Comp.vue", "/lib/helper.ts"),
            "/lib/helper.ts"
        );
    }

    #[test]
    fn resolve_external_relative_sibling() {
        assert_eq!(
            resolve_external("/src/Comp.vue", "./helper.ts"),
            "/src/helper.ts"
        );
    }

    #[test]
    fn resolve_external_relative_parent() {
        assert_eq!(
            resolve_external("/src/components/Comp.vue", "../utils.ts"),
            "/src/utils.ts"
        );
    }

    #[test]
    fn resolve_external_double_parent() {
        assert_eq!(
            resolve_external("/src/a/b/Comp.vue", "../../c.ts"),
            "/src/c.ts"
        );
    }

    #[test]
    fn resolve_external_bare_specifier() {
        assert_eq!(resolve_external("/src/Comp.vue", "lodash"), "lodash");
    }

    #[test]
    fn resolve_external_dot_slash() {
        assert_eq!(
            resolve_external("/src/Comp.vue", "./sub/file.ts"),
            "/src/sub/file.ts"
        );
    }

    // ═══════════════════════════════════════════════════════════
    // Phase 3: Additional id.rs tests
    // ═══════════════════════════════════════════════════════════

    /// @ai-generated - resolve_external with excessive ../ clamps at root
    #[test]
    fn resolve_external_excessive_dotdot() {
        // /a/Comp.vue → owner parts = ["", "a"], after pop filename = [""]
        // ../../../z.ts → ".." can't pop past root "" → stays [""], pushes "z.ts" → ["", "z.ts"]
        let result = resolve_external("/a/Comp.vue", "../../../z.ts");
        assert_eq!(result, "/z.ts");
    }

    /// @ai-generated - render_ids + parse_raw_id roundtrip for all node kinds
    #[test]
    fn render_ids_parse_raw_id_roundtrip() {
        let meta = FileMeta {
            has_script: true,
            has_template: true,
            script_lang: Some("ts".to_string()),
            style_langs: vec![Some("scss".to_string()), None],
            custom_types: vec!["i18n".to_string()],
            custom_langs: vec![None],
        };
        let cases: Vec<VirtualNodeKind> = vec![
            VirtualNodeKind::Main,
            VirtualNodeKind::Script,
            VirtualNodeKind::Template,
            VirtualNodeKind::Style { index: 0 },
            VirtualNodeKind::Style { index: 1 },
            VirtualNodeKind::Custom { index: 0 },
        ];

        for node in &cases {
            let (bundler_id, lsp_id) = render_ids("/src/Comp.vue", node, &meta);

            // Bundler ID should roundtrip
            let parsed_b = parse_raw_id(&bundler_id).unwrap();
            assert_eq!(
                parsed_b.canonical_id, "/src/Comp.vue",
                "bundler roundtrip failed for {:?}",
                node
            );
            assert_eq!(
                parsed_b.node_kind, *node,
                "bundler node kind failed for {:?}",
                node
            );

            // LSP ID should roundtrip
            let parsed_l = parse_raw_id(&lsp_id).unwrap();
            assert_eq!(
                parsed_l.canonical_id, "/src/Comp.vue",
                "lsp roundtrip failed for {:?}",
                node
            );
            assert_eq!(
                parsed_l.node_kind, *node,
                "lsp node kind failed for {:?}",
                node
            );
            assert!(
                parsed_l.was_lsp_like,
                "lsp format not detected for {:?}",
                node
            );
        }
    }

    /// @ai-generated - canonicalize_id: both query and ._VERTER_. — query takes precedence
    #[test]
    fn canonicalize_id_both_query_and_verter() {
        // If input has both ? and ._VERTER_., the ? split happens first
        let result = canonicalize_id("Comp.vue?vue&type=script._VERTER_.foo");
        assert_eq!(result, "Comp.vue");
    }

    /// @ai-generated - resolve_external with owner having no directory part
    #[test]
    fn resolve_external_no_directory() {
        let result = resolve_external("Comp.vue", "./helper.ts");
        assert_eq!(result, "helper.ts");
    }
}
