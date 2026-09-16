//! Svelte main-module admission owned by the Svelte runtime compiler.
//!
//! Svelte emits a self-contained ESM body. This module converts that already-
//! produced module into a [`CompileArtifactSet`] without reconstructing
//! topology, scanning bytes, or composing fragments. Callers transport the
//! complete body; they do not infer Svelte imports, exports, or style
//! relations from block fields.

#[cfg(any(test, feature = "test-support"))]
use std::cell::Cell;
use std::sync::Arc;

use oxc_sourcemap::SourceMap;

use super::fragment::{
    ArtifactContent, ArtifactProvenance, ArtifactRelation, ArtifactRelationKind, CompileArtifact,
    FragmentDialect,
};
use super::publish::{CompileArtifactSet, StagedCompileArtifacts};
use super::source_space::{ArtifactMapFamily, ArtifactMapSegment, QualifiedArtifactMap};
use super::source_unit::{ArtifactSourceUnit, ContentId, SourceId, SourceRevision, SourceUnit};
use crate::compile_request::ProductKind;
use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};
use verter_identity::identity::{InputBasisId, ResultContractId};

/// Already-produced Svelte runtime module facts for compile-artifact admission.
pub struct SvelteMainCompileRequest<'a> {
    /// Host/canonical identity the source unit is minted against.
    pub canonical_id: &'a str,
    /// Authored `.svelte` bytes. Identity and qualified-map geometry use this
    /// space; the converter does not re-parse or re-emit them.
    pub source: &'a str,
    /// Compiler-owned ESM body. Shared with the typed main artifact.
    pub code: Arc<str>,
    /// Runtime source-map JSON, when maps were demanded and produced.
    pub source_map: Option<&'a str>,
    /// Client vs SSR product of this body.
    pub kind: ProductKind,
    /// Whether the request demanded a runtime source map.
    pub want_maps: bool,
    /// Production compile axis.
    pub is_production: bool,
    /// SSR compile axis (runtime mode).
    pub ssr: bool,
    /// Explicit `runes` compile-option override, when present.
    pub runes: Option<bool>,
    /// Resolved `cssHash` override, when present.
    pub css_hash_override: Option<&'a str>,
    /// `customElement` compile-option selector.
    pub custom_element: bool,
    /// External scoped-css bytes, when the module produced that companion.
    pub css_code: Option<&'a str>,
}

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    static SVELTE_MAIN_ASSEMBLY_COUNT: Cell<usize> = const { Cell::new(0) };
}

/// Assemblies observed on this thread. An unrequested runtime compile must
/// stay at zero.
#[cfg(any(test, feature = "test-support"))]
#[must_use]
pub fn svelte_main_assembly_count() -> usize {
    SVELTE_MAIN_ASSEMBLY_COUNT.with(Cell::get)
}

/// Reset this thread's assembly counter.
#[cfg(any(test, feature = "test-support"))]
pub fn reset_svelte_main_assembly_count() {
    SVELTE_MAIN_ASSEMBLY_COUNT.with(|count| count.set(0));
}

fn record_svelte_main_assembly() {
    #[cfg(any(test, feature = "test-support"))]
    SVELTE_MAIN_ASSEMBLY_COUNT.with(|count| count.set(count.get() + 1));
}

struct SvelteAssemblyTag<'a>(&'a str);
impl CanonicalEncode for SvelteAssemblyTag<'_> {
    const DOMAIN_TAG: &'static str = "verter.compiler.svelte.main.assembly.v1";
    fn encode_fields(&self, e: &mut CanonicalEncoder) {
        e.field_str(1, self.0);
    }
}

fn content_digest(bytes: &[u8]) -> [u8; 32] {
    *ContentId::from_content_bytes(bytes).digest().as_bytes()
}

fn runes_tag(runes: Option<bool>) -> &'static str {
    match runes {
        Some(true) => "true",
        Some(false) => "false",
        None => "",
    }
}

struct SvelteMainInputBasis<'a> {
    canonical_id: &'a str,
    kind: &'a str,
    is_production: bool,
    ssr: bool,
    want_maps: bool,
    runes: &'a str,
    css_hash: &'a str,
    custom_element: bool,
    source_digest: [u8; 32],
    main_digest: [u8; 32],
    map_digest: [u8; 32],
    style_digest: [u8; 32],
}
impl CanonicalEncode for SvelteMainInputBasis<'_> {
    const DOMAIN_TAG: &'static str = "verter.compiler.svelte.main.input_basis.v1";
    fn encode_fields(&self, e: &mut CanonicalEncoder) {
        e.field_str(1, self.canonical_id);
        e.field_str(2, self.kind);
        e.field_bool(3, self.is_production);
        e.field_bool(4, self.ssr);
        e.field_bool(5, self.want_maps);
        e.field_str(6, self.runes);
        e.field_str(7, self.css_hash);
        e.field_bool(8, self.custom_element);
        e.field_bytes(9, &self.source_digest);
        e.field_bytes(10, &self.main_digest);
        e.field_bytes(11, &self.map_digest);
        e.field_bytes(12, &self.style_digest);
    }
}

/// Staged compile-artifact handoff for one self-contained Svelte main
/// artifact. Construction performs no parse, compile, or map encode.
pub fn svelte_main_compile_artifacts(
    request: SvelteMainCompileRequest<'_>,
) -> Result<StagedCompileArtifacts, super::publish::ArtifactSchemaError> {
    use std::collections::BTreeSet;

    record_svelte_main_assembly();

    let dialect = FragmentDialect::JavaScript;
    let map_json = request.source_map.unwrap_or("");
    let css_bytes = request.css_code.map(str::as_bytes).unwrap_or(&[]);
    let basis = SvelteMainInputBasis {
        canonical_id: request.canonical_id,
        kind: request.kind.wire_tag(),
        is_production: request.is_production,
        ssr: request.ssr,
        want_maps: request.want_maps,
        runes: runes_tag(request.runes),
        css_hash: request.css_hash_override.unwrap_or(""),
        custom_element: request.custom_element,
        source_digest: content_digest(request.source.as_bytes()),
        main_digest: content_digest(request.code.as_bytes()),
        map_digest: content_digest(map_json.as_bytes()),
        style_digest: content_digest(css_bytes),
    };
    let source_id = SourceId::from_canonical(&SvelteAssemblyTag(request.canonical_id));
    let revision = SourceRevision::from_canonical(&basis);
    let mut source_units = Vec::new();
    let mut inputs = BTreeSet::new();

    let mut push_unit = |role: &str, bytes: &[u8], span: verter_span::Span| {
        let unit = SourceUnit::mint(
            source_id.clone(),
            revision.clone(),
            role,
            ContentId::from_content_bytes(bytes),
        );
        inputs.insert(unit.id().clone());
        source_units.push(ArtifactSourceUnit {
            source_span: span,
            unit,
        });
    };
    push_unit(
        "main",
        request.code.as_bytes(),
        verter_span::Span::new(0, request.code.len() as u32),
    );
    push_unit(
        "source",
        request.source.as_bytes(),
        verter_span::Span::new(0, request.source.len() as u32),
    );
    if let Some(css) = request.css_code {
        push_unit(
            "style",
            css.as_bytes(),
            verter_span::Span::new(0, css.len() as u32),
        );
    }

    let producer = ResultContractId::from_canonical(&SvelteAssemblyTag("svelte-runtime-main"));
    let input_basis = InputBasisId::from_canonical(&basis);
    let mut artifacts = Vec::new();
    let mut main = CompileArtifact::new(
        source_units[0].unit.id().clone(),
        request.kind,
        dialect.schema_language(),
        "main",
        ArtifactProvenance {
            input_basis: input_basis.clone(),
            producer,
            inputs: inputs.clone(),
        },
        ArtifactContent::Available(Arc::clone(&request.code)),
    );

    if let Some(style_unit) = source_units
        .iter()
        .find(|unit| unit.unit.logical_role() == "style")
    {
        let style = CompileArtifact::new(
            style_unit.unit.id().clone(),
            request.kind,
            verter_language::LanguageId::new("css"),
            "style",
            ArtifactProvenance {
                input_basis: input_basis.clone(),
                producer: ResultContractId::from_canonical(&SvelteAssemblyTag(
                    "svelte-runtime-style",
                )),
                inputs: BTreeSet::from([style_unit.unit.id().clone()]),
            },
            ArtifactContent::Unavailable(super::fragment::ArtifactUnavailableReason::NotProduced),
        );
        main.relations.insert(ArtifactRelation {
            kind: ArtifactRelationKind::CompanionOf,
            target: style.id().clone(),
        });
        artifacts.push(style);
    }

    if request.want_maps {
        if let Some(map_json) = request.source_map.filter(|json| !json.is_empty()) {
            let source_unit = source_units
                .iter()
                .find(|unit| unit.unit.logical_role() == "source")
                .expect("source unit is always minted");
            let source_id = source_unit.unit.id().clone();
            main.maps.push(QualifiedArtifactMap {
                family: ArtifactMapFamily::RuntimeSourceMap,
                generated: main.id().clone(),
                generated_content: ContentId::from_content_bytes(request.code.as_bytes()),
                input_basis,
                sources: BTreeSet::from([source_id.clone()]),
                segments: runtime_map_segments(
                    map_json,
                    request.code.as_ref(),
                    request.source,
                    &source_id,
                    source_unit.source_span,
                ),
            });
        }
    }

    let root = main.id().clone();
    artifacts.insert(0, main);
    let set = CompileArtifactSet::new(source_units, artifacts)?;
    StagedCompileArtifacts::stage(set, root, dialect, request.source_map.map(str::to_string))
}

/// Mint byte-qualified segments for one JSON map over a single authored
/// source unit, dropping tokens that fall outside the unit's span — the
/// conversion for a produced map whose every token addresses the carrier
/// bytes it was compiled from.
pub(crate) fn runtime_map_segments(
    map_json: &str,
    generated: &str,
    source: &str,
    source_unit: &super::source_unit::SourceUnitId,
    source_span: verter_span::Span,
) -> Vec<ArtifactMapSegment> {
    let Ok(map) = SourceMap::from_json_string(map_json) else {
        return Vec::new();
    };
    let gen_starts = generated_line_starts(generated);
    let src_starts = generated_line_starts(source);
    let gen_len = generated.len() as u32;
    let mut segments: Vec<ArtifactMapSegment> = map
        .get_tokens()
        .filter_map(|token| {
            let source_start = utf16_offset_to_bytes(
                source,
                &src_starts,
                token.get_src_line(),
                token.get_src_col(),
            )?;
            let source_end = next_char_end(source, source_start);
            if source_start < source_span.start || source_end > source_span.end {
                return None;
            }
            let start = utf16_offset_to_bytes(
                generated,
                &gen_starts,
                token.get_dst_line(),
                token.get_dst_col(),
            )?;
            if start >= gen_len {
                return None;
            }
            let end = next_char_end(generated, start).min(gen_len);
            generated.get(start as usize..end as usize)?;
            Some(ArtifactMapSegment {
                generated: start..end,
                source_unit: source_unit.clone(),
                source_span: verter_span::Span::new(source_start, source_end),
            })
        })
        .collect();
    segments.sort_by_key(|segment| (segment.generated.start, segment.generated.end));
    segments.dedup_by(|a, b| a.generated.start == b.generated.start);
    let mut kept = Vec::with_capacity(segments.len());
    for segment in segments {
        if kept
            .last()
            .is_none_or(|prev: &ArtifactMapSegment| prev.generated.end <= segment.generated.start)
        {
            kept.push(segment);
        }
    }
    kept
}

pub(crate) fn generated_line_starts(code: &str) -> Vec<u32> {
    let mut starts = vec![0u32];
    for (index, byte) in code.bytes().enumerate() {
        if byte == b'\n' {
            starts.push((index + 1) as u32);
        }
    }
    starts
}

pub(crate) fn utf16_offset_to_bytes(
    code: &str,
    line_starts: &[u32],
    line: u32,
    column: u32,
) -> Option<u32> {
    let start = *line_starts.get(line as usize)? as usize;
    let end = line_starts
        .get(line as usize + 1)
        .map(|next| *next as usize)
        .unwrap_or(code.len());
    let line_bytes = code.get(start..end)?;
    let mut utf16 = 0u32;
    for (byte_off, ch) in line_bytes.char_indices() {
        if utf16 == column {
            return Some((start + byte_off) as u32);
        }
        utf16 = utf16.saturating_add(ch.len_utf16() as u32);
        if utf16 > column {
            return None;
        }
    }
    if utf16 == column {
        Some((start + line_bytes.len()) as u32)
    } else {
        None
    }
}

pub(crate) fn next_char_end(code: &str, start: u32) -> u32 {
    let start = start as usize;
    let Some(rest) = code.get(start..) else {
        return start as u32;
    };
    match rest.chars().next() {
        Some(ch) => (start + ch.len_utf8()) as u32,
        None => start as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::too_many_arguments)]
    fn request<'a>(
        canonical_id: &'a str,
        source: &'a str,
        code: impl Into<Arc<str>>,
        source_map: Option<&'a str>,
        kind: ProductKind,
        want_maps: bool,
        is_production: bool,
        css_code: Option<&'a str>,
    ) -> SvelteMainCompileRequest<'a> {
        SvelteMainCompileRequest {
            canonical_id,
            source,
            code: code.into(),
            source_map,
            kind,
            want_maps,
            is_production,
            ssr: kind == ProductKind::RuntimeServer,
            runes: None,
            css_hash_override: None,
            custom_element: false,
            css_code,
        }
    }

    #[test]
    fn admits_self_contained_main_without_reconstructing_fragments() {
        let source = "<script>let n = $state(1);</script>\n<p>{n}</p>\n";
        let code = "import * as $ from 'svelte/internal/client';\nexport default function App($$anchor) {}\n";
        let artifacts = svelte_main_compile_artifacts(request(
            "App.svelte",
            source,
            code,
            None,
            ProductKind::RuntimeClient,
            false,
            false,
            None,
        ))
        .expect("schema accepts");
        let names: Vec<_> = artifacts
            .set()
            .artifacts()
            .iter()
            .map(|a| a.name())
            .collect();
        assert!(names.contains(&"main"));
        assert!(!names.contains(&"script"));
        assert!(!names.contains(&"template"));
        let main = artifacts
            .set()
            .artifacts()
            .iter()
            .find(|a| a.name() == "main")
            .expect("main");
        let ArtifactContent::Available(body) = &main.content else {
            panic!("main must stay available");
        };
        assert_eq!(body.as_ref(), code);
        assert!(main.maps.is_empty());
    }

    #[test]
    fn external_style_is_a_companion_relation() {
        let source = "<style>.x{color:red}</style>\n<p class=\"x\"></p>\n";
        let code = "export default function App($$anchor) {}\n";
        let css = ".x.svelte-abc{color:red}";
        let artifacts = svelte_main_compile_artifacts(request(
            "Styled.svelte",
            source,
            code,
            None,
            ProductKind::RuntimeClient,
            false,
            false,
            Some(css),
        ))
        .expect("schema accepts");
        let main = artifacts
            .set()
            .artifacts()
            .iter()
            .find(|a| a.name() == "main")
            .expect("main");
        let style = artifacts
            .set()
            .artifacts()
            .iter()
            .find(|a| a.name() == "style")
            .expect("style companion");
        assert!(main.relations.iter().any(|relation| {
            relation.kind == ArtifactRelationKind::CompanionOf && relation.target == *style.id()
        }));
        assert!(matches!(
            style.content,
            ArtifactContent::Unavailable(crate::assembly::ArtifactUnavailableReason::NotProduced)
        ));
    }

    #[test]
    fn identity_binds_revision_options_runtime_mode_and_restores_on_revert() {
        let source = "<p/>\n";
        let code = "export default function App($$anchor) {}\n";
        let first = svelte_main_compile_artifacts(request(
            "App.svelte",
            source,
            code,
            None,
            ProductKind::RuntimeClient,
            false,
            false,
            None,
        ))
        .expect("schema accepts");
        let option_changed = svelte_main_compile_artifacts(SvelteMainCompileRequest {
            is_production: true,
            ..request(
                "App.svelte",
                source,
                code,
                None,
                ProductKind::RuntimeClient,
                false,
                false,
                None,
            )
        })
        .expect("schema accepts");
        let ssr = svelte_main_compile_artifacts(request(
            "App.svelte",
            source,
            code,
            None,
            ProductKind::RuntimeServer,
            false,
            false,
            None,
        ))
        .expect("schema accepts");
        let edited = format!("{code}\n");
        let after_edit = svelte_main_compile_artifacts(request(
            "App.svelte",
            source,
            edited.as_str(),
            None,
            ProductKind::RuntimeClient,
            false,
            false,
            None,
        ))
        .expect("schema accepts");
        let reverted = svelte_main_compile_artifacts(request(
            "App.svelte",
            source,
            code,
            None,
            ProductKind::RuntimeClient,
            false,
            false,
            None,
        ))
        .expect("schema accepts");
        let first_main = first
            .set()
            .artifacts()
            .iter()
            .find(|a| a.name() == "main")
            .expect("main");
        let option_main = option_changed
            .set()
            .artifacts()
            .iter()
            .find(|a| a.name() == "main")
            .expect("main");
        let ssr_main = ssr
            .set()
            .artifacts()
            .iter()
            .find(|a| a.name() == "main")
            .expect("main");
        let reverted_main = reverted
            .set()
            .artifacts()
            .iter()
            .find(|a| a.name() == "main")
            .expect("main");
        assert_ne!(
            first_main.provenance.input_basis, option_main.provenance.input_basis,
            "option change must change input basis"
        );
        assert_ne!(
            first_main.provenance.input_basis, ssr_main.provenance.input_basis,
            "runtime mode must change input basis"
        );
        let first_rev = first
            .set()
            .source_units()
            .find(|u| u.unit.logical_role() == "main")
            .expect("main unit")
            .unit
            .revision()
            .clone();
        let edited_rev = after_edit
            .set()
            .source_units()
            .find(|u| u.unit.logical_role() == "main")
            .expect("main unit")
            .unit
            .revision()
            .clone();
        let reverted_rev = reverted
            .set()
            .source_units()
            .find(|u| u.unit.logical_role() == "main")
            .expect("main unit")
            .unit
            .revision()
            .clone();
        assert_ne!(first_rev, edited_rev, "edit must change source revision");
        assert_eq!(
            first_main.provenance.input_basis, reverted_main.provenance.input_basis,
            "revert must restore input basis"
        );
        assert_eq!(first_rev, reverted_rev, "revert must restore revision");
    }

    #[test]
    fn qualified_runtime_map_binds_authored_source_not_generated_main() {
        let source = "<p>hi</p>\n";
        let generated = "export default function App($$anchor) {}\n";
        let map_json = r#"{"version":3,"file":"App.svelte","sources":["App.svelte"],"sourcesContent":["<p>hi</p>\n"],"names":[],"mappings":"AAAA"}"#;
        let artifacts = svelte_main_compile_artifacts(request(
            "App.svelte",
            source,
            generated,
            Some(map_json),
            ProductKind::RuntimeClient,
            true,
            false,
            None,
        ))
        .expect("schema accepts authored map geometry");
        let main = artifacts
            .set()
            .artifacts()
            .iter()
            .find(|artifact| artifact.name() == "main")
            .expect("main");
        let map = main
            .maps
            .iter()
            .find(|map| map.family == ArtifactMapFamily::RuntimeSourceMap)
            .expect("runtime map");
        let main_unit = artifacts
            .set()
            .source_units()
            .find(|unit| unit.unit.logical_role() == "main")
            .expect("main unit")
            .unit
            .id()
            .clone();
        let source_unit = artifacts
            .set()
            .source_units()
            .find(|unit| unit.unit.logical_role() == "source")
            .expect("source unit");
        assert!(
            map.segments
                .iter()
                .all(|segment| segment.source_unit != main_unit),
            "segments must not use the generated main unit as authored source"
        );
        assert!(
            map.segments
                .iter()
                .all(|segment| segment.source_unit == *source_unit.unit.id()),
            "segments must bind the authored svelte source unit"
        );
    }

    #[test]
    fn demanded_main_shares_one_payload_allocation() {
        let source = "<p/>\n";
        let code: Arc<str> = Arc::from("export default function App($$anchor) {}\n");
        let artifacts = svelte_main_compile_artifacts(request(
            "App.svelte",
            source,
            Arc::clone(&code),
            None,
            ProductKind::RuntimeClient,
            false,
            false,
            None,
        ))
        .expect("schema accepts");
        let main = artifacts
            .set()
            .artifacts()
            .iter()
            .find(|artifact| artifact.name() == "main")
            .expect("main");
        let ArtifactContent::Available(content) = &main.content else {
            panic!("produced Main must keep available content");
        };
        assert!(
            Arc::ptr_eq(&code, content),
            "body payload and typed artifact must share one allocation"
        );
    }

    #[test]
    fn input_basis_retains_content_digests_not_raw_bytes() {
        let source = "<p/>\n";
        let huge = format!(
            "export default function App($$anchor) {{ const n = '{}'; }}\n",
            "a".repeat(32 * 1024)
        );
        let artifacts = svelte_main_compile_artifacts(request(
            "App.svelte",
            source,
            huge.as_str(),
            None,
            ProductKind::RuntimeClient,
            false,
            false,
            None,
        ))
        .expect("schema accepts");
        let revision = artifacts
            .set()
            .source_units()
            .find(|unit| unit.unit.logical_role() == "main")
            .expect("main unit")
            .unit
            .revision()
            .clone();
        let main = artifacts
            .set()
            .artifacts()
            .iter()
            .find(|artifact| artifact.name() == "main")
            .expect("main");
        assert!(
            revision.canonical_bytes().len() < huge.len() / 4,
            "source revision must not retain the assembled payload"
        );
        assert!(
            main.provenance.input_basis.canonical_bytes().len() < huge.len() / 4,
            "input basis must not retain the assembled payload"
        );
    }
}
