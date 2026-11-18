//! What "the same output" means between the two routes.

use std::collections::HashMap;
use std::fmt;

use verter_compiler::assembly::map_input::{validate_and_decode, DecodedFragmentMap};
use verter_compiler::compile::VerterCompileResult;

/// One place where the routes disagree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Difference {
    /// Which part of the output differs.
    pub aspect: &'static str,
    /// The first differing byte, for text aspects.
    pub offset: Option<usize>,
    pub detail: String,
}

impl fmt::Display for Difference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.offset {
            Some(offset) => write!(
                f,
                "{} differs at byte {offset}: {}",
                self.aspect, self.detail
            ),
            None => write!(f, "{} differs: {}", self.aspect, self.detail),
        }
    }
}

/// A diagnostic reduced to what the comparison reads.
pub(super) type DiagnosticKey = (String, Option<(u32, u32)>);

/// The parts of one compile the comparison reads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct CompiledView {
    /// Complete staged module and its terminal source map, when compared.
    pub module: Option<(String, String)>,
    pub render: String,
    pub render_map: String,
    pub script: String,
    pub script_map: String,
    /// Runtime helper imports, in emission order; server-renderer helpers
    /// follow the runtime ones and carry a `server:` prefix.
    pub helper_imports: Vec<String>,
    pub styles: Vec<String>,
    pub diagnostics: Vec<DiagnosticKey>,
}

/// Reduce a compile result to the parts the comparison reads.
pub(super) fn view_of(result: &VerterCompileResult) -> CompiledView {
    let mut view = CompiledView::default();
    if let Some(template) = &result.template {
        view.render = template.code.clone();
        view.render_map = template.source_map.clone();
        view.helper_imports
            .extend(template.imports.iter().map(|name| (*name).to_string()));
        view.helper_imports.extend(
            template
                .ssr_imports
                .iter()
                .map(|name| format!("server:{name}")),
        );
    }
    if let Some(script) = &result.script {
        view.script = script.code.clone();
        view.script_map = script.source_map.clone();
    }
    view.styles = result
        .styles
        .iter()
        .map(|style| style.code().to_string())
        .collect();
    view.diagnostics = result
        .errors
        .iter()
        .map(|error| {
            (
                error.code.clone(),
                error.span.map(|span| (span.start, span.end)),
            )
        })
        .collect();
    view
}

/// Compare every aspect of two compiles. The legacy view comes first.
pub(super) fn compare_views(legacy: &CompiledView, new: &CompiledView) -> Vec<Difference> {
    let mut differences = Vec::new();
    if let (Some(legacy), Some(new)) = (&legacy.module, &new.module) {
        differences.extend(compare_text("module code", &legacy.0, &new.0));
        differences.extend(compare_maps(
            "module map",
            (&legacy.1, &legacy.0),
            (&new.1, &new.0),
        ));
    } else if legacy.module.is_some() != new.module.is_some() {
        differences.push(Difference {
            aspect: "module",
            offset: None,
            detail: "only one route published a module".into(),
        });
    }
    differences.extend(compare_text("render code", &legacy.render, &new.render));
    differences.extend(compare_text("script code", &legacy.script, &new.script));
    differences.extend(compare_ordered_lists(
        "helper imports",
        &legacy.helper_imports,
        &new.helper_imports,
    ));
    differences.extend(compare_blocks("css", &legacy.styles, &new.styles));
    differences.extend(compare_maps(
        "render map",
        (&legacy.render_map, &legacy.render),
        (&new.render_map, &new.render),
    ));
    differences.extend(compare_maps(
        "script map",
        (&legacy.script_map, &legacy.script),
        (&new.script_map, &new.script),
    ));
    differences.extend(compare_diagnostics(&legacy.diagnostics, &new.diagnostics));
    differences
}

/// Byte-equal comparison; reports the first differing byte.
pub(super) fn compare_text(aspect: &'static str, legacy: &str, new: &str) -> Vec<Difference> {
    let offset = legacy
        .bytes()
        .zip(new.bytes())
        .position(|(a, b)| a != b)
        .or_else(|| (legacy.len() != new.len()).then_some(legacy.len().min(new.len())));
    match offset {
        None => Vec::new(),
        Some(offset) => vec![Difference {
            aspect,
            offset: Some(offset),
            detail: format!(
                "legacy {:?} against new {:?}",
                excerpt(legacy, offset),
                excerpt(new, offset)
            ),
        }],
    }
}

fn excerpt(text: &str, offset: usize) -> String {
    let bytes = text.as_bytes();
    let start = offset.saturating_sub(16).min(bytes.len());
    let end = (offset + 16).min(bytes.len());
    String::from_utf8_lossy(&bytes[start..end]).into_owned()
}

fn compare_ordered_lists(
    aspect: &'static str,
    legacy: &[String],
    new: &[String],
) -> Vec<Difference> {
    if legacy == new {
        return Vec::new();
    }
    vec![Difference {
        aspect,
        offset: None,
        detail: format!("legacy {legacy:?} against new {new:?}"),
    }]
}

fn compare_blocks(aspect: &'static str, legacy: &[String], new: &[String]) -> Vec<Difference> {
    if legacy.len() != new.len() {
        return vec![Difference {
            aspect,
            offset: None,
            detail: format!("legacy has {} blocks, new has {}", legacy.len(), new.len()),
        }];
    }
    legacy
        .iter()
        .zip(new)
        .flat_map(|(legacy, new)| compare_text(aspect, legacy, new))
        .collect()
}

/// Equal `(code, span)` multisets.
pub(super) fn compare_diagnostics(
    legacy: &[DiagnosticKey],
    new: &[DiagnosticKey],
) -> Vec<Difference> {
    let mut legacy_sorted = legacy.to_vec();
    let mut new_sorted = new.to_vec();
    legacy_sorted.sort();
    new_sorted.sort();
    if legacy_sorted == new_sorted {
        return Vec::new();
    }
    vec![Difference {
        aspect: "diagnostics",
        offset: None,
        detail: format!("legacy {legacy_sorted:?} against new {new_sorted:?}"),
    }]
}

/// Every token of the legacy map must resolve in the new map to the same
/// source position at the same generated position. The new map may hold more
/// tokens. Each side is a map with the generated code it describes.
pub(super) fn compare_maps(
    aspect: &'static str,
    legacy: (&str, &str),
    new: (&str, &str),
) -> Vec<Difference> {
    let (legacy_map, legacy_code) = legacy;
    if legacy_map.is_empty() {
        return Vec::new();
    }
    let legacy_decoded = match validate_and_decode(legacy_map, legacy_code) {
        Ok(decoded) => decoded,
        Err(code) => {
            return vec![Difference {
                aspect,
                offset: None,
                detail: format!("the legacy map does not decode: {code:?}"),
            }];
        }
    };
    let new_decoded = match validate_and_decode(new.0, new.1) {
        Ok(decoded) => decoded,
        Err(code) => {
            return vec![Difference {
                aspect,
                offset: None,
                detail: format!("the new map is missing or does not decode: {code:?}"),
            }];
        }
    };
    let new_tokens = token_index(&new_decoded);
    let mut missing = Vec::new();
    for (generated, sources) in &token_index(&legacy_decoded) {
        let resolved = new_tokens.get(generated);
        for source in sources {
            if !resolved.is_some_and(|candidates| candidates.contains(source)) {
                missing.push((*generated, source.clone()));
            }
        }
    }
    if missing.is_empty() {
        return Vec::new();
    }
    missing.sort();
    let first = &missing[0];
    vec![Difference {
        aspect,
        offset: None,
        detail: format!(
            "{} legacy tokens do not resolve in the new map; first at generated {:?} to {:?}",
            missing.len(),
            first.0,
            first.1
        ),
    }]
}

type GeneratedPosition = (u32, u32);
type SourcePosition = (Option<String>, u32, u32);

/// The authored segments of a decoded map, by generated position.
fn token_index(map: &DecodedFragmentMap) -> HashMap<GeneratedPosition, Vec<SourcePosition>> {
    let mut index: HashMap<GeneratedPosition, Vec<SourcePosition>> = HashMap::new();
    for segment in map.segments() {
        let Some(payload) = segment.payload else {
            continue;
        };
        index
            .entry((segment.generated_line, segment.generated_column))
            .or_default()
            .push((
                map.sources().get(payload.source_index as usize).cloned(),
                payload.source_line,
                payload.source_column,
            ));
    }
    index
}

/// A short line diff around the first differing line, for reports.
pub(super) fn render_diff(legacy: &str, new: &str) -> String {
    let legacy_lines: Vec<&str> = legacy.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    let Some(first) = (0..legacy_lines.len().max(new_lines.len()))
        .find(|&i| legacy_lines.get(i) != new_lines.get(i))
    else {
        return String::new();
    };
    let mut out = String::new();
    for line in &legacy_lines[first.saturating_sub(2)..first] {
        out.push_str(&format!("  {line}\n"));
    }
    for line in legacy_lines.iter().skip(first).take(6) {
        out.push_str(&format!("- {line}\n"));
    }
    for line in new_lines.iter().skip(first).take(6) {
        out.push_str(&format!("+ {line}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_compiler::oxc_sourcemap::SourceMapBuilder;

    const GENERATED: &str = "const render = () => {};";

    fn map_with_tokens(tokens: &[(u32, u32, u32, u32)]) -> String {
        let mut builder = SourceMapBuilder::default();
        let source = builder.add_source_and_content("App.vue", "<template></template>");
        for &(dst_line, dst_col, src_line, src_col) in tokens {
            builder.add_token(dst_line, dst_col, src_line, src_col, Some(source), None);
        }
        builder.into_sourcemap().to_json_string()
    }

    #[test]
    fn the_harness_reports_a_planted_difference() {
        let differences = compare_text("render code", "const a = 1;", "const a = 2;");
        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].offset, Some(10));
        assert!(compare_text("render code", "same", "same").is_empty());
    }

    #[test]
    fn text_of_different_length_differs_at_the_end_of_the_shorter() {
        let differences = compare_text("render code", "abc", "abcd");
        assert_eq!(differences[0].offset, Some(3));
    }

    #[test]
    fn a_map_with_extra_tokens_is_accepted() {
        let legacy = map_with_tokens(&[(0, 0, 0, 0), (0, 4, 0, 2)]);
        let new = map_with_tokens(&[(0, 0, 0, 0), (0, 2, 0, 1), (0, 4, 0, 2)]);
        assert!(compare_maps("render map", (&legacy, GENERATED), (&new, GENERATED)).is_empty());
    }

    #[test]
    fn a_map_missing_a_legacy_token_is_rejected() {
        let legacy = map_with_tokens(&[(0, 0, 0, 0), (0, 4, 0, 2)]);
        let new = map_with_tokens(&[(0, 0, 0, 0)]);
        let differences = compare_maps("render map", (&legacy, GENERATED), (&new, GENERATED));
        assert_eq!(differences.len(), 1);
    }

    #[test]
    fn a_token_resolving_to_another_source_position_is_rejected() {
        let legacy = map_with_tokens(&[(0, 4, 0, 2)]);
        let new = map_with_tokens(&[(0, 4, 0, 3)]);
        let differences = compare_maps("render map", (&legacy, GENERATED), (&new, GENERATED));
        assert_eq!(differences.len(), 1);
    }

    #[test]
    fn diagnostics_compare_as_multisets() {
        let a = ("X_MISSING_END_TAG".to_string(), Some((1, 2)));
        let b = ("X_INVALID_END_TAG".to_string(), None);
        assert!(compare_diagnostics(&[a.clone(), b.clone()], &[b.clone(), a.clone()]).is_empty());
        assert_eq!(compare_diagnostics(&[a.clone(), a], &[b]).len(), 1);
    }

    #[test]
    fn the_diff_shows_the_first_differing_line() {
        let diff = render_diff("a\nb\nc", "a\nx\nc");
        assert!(diff.contains("- b"));
        assert!(diff.contains("+ x"));
    }
}
