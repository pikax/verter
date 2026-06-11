//! The Vue carrier bridge.
//!
//! Owns [`VueParseCarrier`] — the concrete [`CarrierParse`] payload
//! wrapping a parsed Vue SFC — and [`build_vue_parse_artifact`], the
//! Vue producer that lifts a [`ParsedSfc`] into the framework-neutral
//! [`FrameworkParseArtifact`] at parse time (typed script/template/
//! style regions, external links, the resolved per-region
//! [`ScriptSourceType`]).
//!
//! Consumers outside the Vue adapter read the artifact's typed
//! [`FrameworkParseCommon`] surface; the typed carrier is reachable
//! ONLY through the blessed token-gated `carrier_for::<VueParseCarrier>`
//! wrappers (see the `carrier_downcast_confined_to_owning_adapter`
//! architecture guard).

use std::any::Any;
use std::sync::Arc;

use verter_language::{
    CarrierParse, ExternalLink, ExternalLinkKind, FrameworkAdapterId, FrameworkParseArtifact,
    FrameworkParseCommon, LanguageId, ScriptRegion, ScriptRegionKind, ScriptSourceType,
    StyleRegion, TemplateRegion,
};
use verter_span::Span;

use verter_parser::parser::types::ParsedSfc;
use verter_parser::types::NodeProp;

/// The concrete Vue carrier: the full parsed SFC behind the erasure
/// seam.
#[derive(Debug)]
pub struct VueParseCarrier {
    parsed: Arc<ParsedSfc>,
}

impl VueParseCarrier {
    /// Wrap a parsed SFC.
    pub fn new(parsed: Arc<ParsedSfc>) -> Self {
        Self { parsed }
    }

    /// The wrapped parse result.
    pub fn parsed(&self) -> &ParsedSfc {
        &self.parsed
    }

    /// The wrapped parse result, as the shared handle.
    pub fn parsed_arc(&self) -> &Arc<ParsedSfc> {
        &self.parsed
    }
}

impl CarrierParse for VueParseCarrier {
    fn __verter_as_any(&self) -> &dyn Any {
        self
    }
    fn __verter_as_any_arc(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
        self
    }
}

/// Attribute lookup mirroring the session's historical
/// `extract_attrs` + `find_attr` semantics byte-for-byte: names match
/// case-insensitively, a present-but-empty value reads as `"true"`.
/// Returns the value string plus the value span when one exists.
fn attr_value(props: &[NodeProp], source: &str, name: &str) -> Option<(String, Option<Span>)> {
    props.iter().find_map(|p| {
        let attr_name = &source[p.start as usize..p.name_end as usize];
        if !attr_name.eq_ignore_ascii_case(name) {
            return None;
        }
        match (p.value_start, p.value_end) {
            (Some(s), Some(e)) => {
                let value = &source[s as usize..e as usize];
                if value.is_empty() {
                    Some(("true".to_string(), Some(Span::new(s, e))))
                } else {
                    Some((value.to_string(), Some(Span::new(s, e))))
                }
            }
            _ => Some(("true".to_string(), None)),
        }
    })
}

/// Resolve the SFC's script source type from `<script lang>` data —
/// the first script block (plain `<script>` before `<script setup>`)
/// carrying an explicit `lang` attribute decides; `tsx`/`jsx`/`js`
/// (ASCII-case-insensitively) map to their dialects, anything else is
/// TypeScript.
pub fn vue_script_source_type(parsed: &ParsedSfc, source: &str) -> ScriptSourceType {
    let lang = [parsed.script(), parsed.script_setup()]
        .into_iter()
        .flatten()
        .find_map(|script| {
            attr_value(&script.attributes, source, "lang")
                .map(|(value, _)| value)
                .filter(|v| v != "true")
        });

    match lang.as_deref().map(|value| value.to_ascii_lowercase()) {
        Some(lang) if lang == "tsx" => ScriptSourceType::Tsx,
        Some(lang) if lang == "jsx" => ScriptSourceType::Jsx,
        Some(lang) if lang == "js" => ScriptSourceType::Js,
        _ => ScriptSourceType::Ts,
    }
}

/// Lift a parsed Vue SFC into the framework-neutral parse artifact.
///
/// The ONE Vue carrier producer: every host slot that stores a Vue
/// parse stores the artifact this function builds. The typed common
/// surface carries:
///
/// * one [`ScriptRegion`] per script block (plain `<script>` →
///   [`ScriptRegionKind::Module`], `<script setup>` →
///   [`ScriptRegionKind::Instance`]), each stamped with the SFC's
///   resolved [`ScriptSourceType`] (Vue requires every script block of
///   one SFC to share a dialect, so the SFC-level resolution is the
///   per-region truth);
/// * template/style content regions in source order;
/// * external `src` links for script/template/style blocks;
/// * NO duplicated diagnostics — Vue's parse diagnostics flow through
///   the host's existing `ParseSnapshot` channel; mirroring them here
///   would create a second source for the same data.
pub fn build_vue_parse_artifact(
    source: &str,
    parsed: Arc<ParsedSfc>,
    parser_version: u32,
) -> Arc<FrameworkParseArtifact> {
    let source_type = vue_script_source_type(&parsed, source);

    let mut script_regions = Vec::new();
    let mut external_links = Vec::new();

    for script in [parsed.script(), parsed.script_setup()]
        .into_iter()
        .flatten()
    {
        let span = script
            .content
            .unwrap_or_else(|| Span::new(script.tag_open.end, script.tag_open.end));
        script_regions.push(ScriptRegion {
            span,
            source_type,
            kind: if script.is_setup {
                ScriptRegionKind::Instance
            } else {
                ScriptRegionKind::Module
            },
        });
        if let Some(src_span) = script.src {
            external_links.push(ExternalLink {
                kind: ExternalLinkKind::Script,
                specifier: source[src_span.start as usize..src_span.end as usize].to_string(),
                span: Some(src_span),
            });
        }
    }
    // `FrameworkParseCommon.script_regions` is SOURCE-ordered: a Vue file
    // may place `<script setup>` before the plain `<script>`, while the
    // parser accessors expose them in fixed plain-then-setup order.
    script_regions.sort_by_key(|region| region.span.start);

    let mut template_regions = Vec::new();
    if let Some(template) = parsed.template_ast() {
        let span = template
            .root
            .content
            .as_ref()
            .map(|content| Span::new(content.start, content.end))
            .unwrap_or_else(|| Span::new(template.root.tag_open.end, template.root.tag_open.end));
        template_regions.push(TemplateRegion { span });
        if let Some((specifier, span)) = attr_value(&template.root.attributes, source, "src") {
            external_links.push(ExternalLink {
                kind: ExternalLinkKind::Template,
                specifier,
                span,
            });
        }
    }

    let mut style_regions = Vec::new();
    for style in parsed.style_nodes() {
        let span = style
            .content
            .unwrap_or_else(|| Span::new(style.tag_open.end, style.tag_open.end));
        style_regions.push(StyleRegion { span });
        if let Some((specifier, span)) = attr_value(&style.attributes, source, "src") {
            external_links.push(ExternalLink {
                kind: ExternalLinkKind::Style,
                specifier,
                span,
            });
        }
    }

    Arc::new(FrameworkParseArtifact::new(
        FrameworkAdapterId::vue(),
        LanguageId::new("vue"),
        parser_version,
        FrameworkParseCommon {
            script_regions,
            template_regions,
            style_regions,
            external_links,
            diagnostics: Vec::new(),
        },
        Arc::new(VueParseCarrier::new(parsed)),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::parse_sfc;

    fn artifact_for(source: &str) -> Arc<FrameworkParseArtifact> {
        let parsed = Arc::new(parse_sfc(source, None, None));
        build_vue_parse_artifact(source, parsed, 1)
    }

    #[test]
    fn script_regions_carry_kind_and_resolved_source_type() {
        let source =
            "<script>export default {}</script>\n<script setup lang=\"tsx\">const a = 1</script>";
        let artifact = artifact_for(source);
        let regions = &artifact.common.script_regions;
        assert_eq!(regions.len(), 2);
        assert_eq!(regions[0].kind, ScriptRegionKind::Module);
        assert_eq!(regions[1].kind, ScriptRegionKind::Instance);
        // The first block WITH a lang attribute decides for the SFC —
        // both regions are stamped with the resolved dialect.
        assert_eq!(regions[0].source_type, ScriptSourceType::Tsx);
        assert_eq!(regions[1].source_type, ScriptSourceType::Tsx);
        // Content spans slice to the script text.
        assert_eq!(regions[0].span.slice(source).trim(), "export default {}");
        assert_eq!(regions[1].span.slice(source).trim(), "const a = 1");
    }

    #[test]
    fn lang_resolution_matches_the_historical_attr_scan() {
        // (source, expected) — the historical `sfc_script_source_type`
        // semantics: lowercase compare, unknown/absent → Ts.
        let cases = [
            ("<script lang=\"ts\">a</script>", ScriptSourceType::Ts),
            ("<script lang=\"tsx\">a</script>", ScriptSourceType::Tsx),
            ("<script lang=\"TSX\">a</script>", ScriptSourceType::Tsx),
            ("<script lang=\"jsx\">a</script>", ScriptSourceType::Jsx),
            ("<script lang=\"js\">a</script>", ScriptSourceType::Js),
            ("<script lang=\"coffee\">a</script>", ScriptSourceType::Ts),
            ("<script lang>a</script>", ScriptSourceType::Ts),
            ("<script>a</script>", ScriptSourceType::Ts),
            ("<template><div /></template>", ScriptSourceType::Ts),
        ];
        for (source, expected) in cases {
            let parsed = Arc::new(parse_sfc(source, None, None));
            assert_eq!(
                vue_script_source_type(&parsed, source),
                expected,
                "lang resolution drifted for {source:?}"
            );
        }
    }

    #[test]
    fn template_style_regions_and_external_links_are_populated() {
        let source = "<template src=\"./tpl.html\"></template>\n<style src=\"./a.css\"></style>\n<style>.x{}</style>\n<script src=\"./impl.ts\"></script>";
        let artifact = artifact_for(source);
        assert_eq!(artifact.common.template_regions.len(), 1);
        assert_eq!(artifact.common.style_regions.len(), 2);
        let links: Vec<(&ExternalLinkKind, &str)> = artifact
            .common
            .external_links
            .iter()
            .map(|l| (&l.kind, l.specifier.as_str()))
            .collect();
        assert!(links.contains(&(&ExternalLinkKind::Script, "./impl.ts")));
        assert!(links.contains(&(&ExternalLinkKind::Template, "./tpl.html")));
        assert!(links.contains(&(&ExternalLinkKind::Style, "./a.css")));
        // The second style block has no src.
        assert_eq!(
            links
                .iter()
                .filter(|(k, _)| matches!(k, ExternalLinkKind::Style))
                .count(),
            1
        );
    }

    #[test]
    fn script_regions_are_source_ordered_when_setup_precedes_plain_script() {
        // The parser exposes plain-script-then-setup accessor order; the
        // artifact must re-order by source position.
        let source = "<script setup lang=\"ts\">const a = 1</script>\n<script lang=\"ts\">export default {}</script>";
        let artifact = artifact_for(source);
        let regions = &artifact.common.script_regions;
        assert_eq!(regions.len(), 2);
        assert_eq!(
            regions[0].kind,
            ScriptRegionKind::Instance,
            "the <script setup> block appears first in source"
        );
        assert_eq!(regions[1].kind, ScriptRegionKind::Module);
        assert!(
            regions[0].span.start < regions[1].span.start,
            "regions must be ordered by source position"
        );
    }

    #[test]
    fn artifact_identity_names_the_vue_adapter() {
        let artifact = artifact_for("<script>a</script>");
        assert!(artifact.adapter_id.is_vue());
        assert_eq!(artifact.language_id.as_str(), "vue");
        assert_eq!(artifact.parser_version, 1);
        assert!(artifact.common.diagnostics.is_empty());
    }
}
