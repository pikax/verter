//! SFC tokenization, block hashing, and `ParseSnapshot` construction.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::Arc;

use oxc_allocator::Allocator;
use oxc_ast::ast::Program;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::{GetSpan, SourceType};

use verter_compiler::diagnostics::DiagnosticSeverity;
use verter_compiler::parser::types::ParsedSfc;
use verter_compiler::types::NodeProp;

use crate::hash::{hash_16, semantic_hash};
use crate::id::resolve_external;
use crate::types::{
    DescriptorMin, DiagnosticsSnapshot, ExternalBlockKind, ExternalSourceRequest, FileMeta,
    HostDiagnostic, HostSeverity, ParseSnapshot, PreprocessorBlockType, PreprocessorRequest,
    SliceHashes, SrcBlockInfo,
};

/// Closed failure while assigning carrier statements to typed script regions.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ScriptOwnerIndexError {
    #[error("carrier script regions overlap at [{left_start}, {left_end}) and [{right_start}, {right_end})")]
    OverlappingRegions {
        left_start: u32,
        left_end: u32,
        right_start: u32,
        right_end: u32,
    },
    #[error("top-level statement {statement_index} at [{start}, {end}) is outside every carrier script region")]
    UnownedStatement {
        statement_index: usize,
        start: u32,
        end: u32,
    },
    #[error("top-level statement {statement_index} at [{start}, {end}) overlaps multiple carrier script regions")]
    AmbiguousStatement {
        statement_index: usize,
        start: u32,
        end: u32,
    },
    #[error(transparent)]
    InvalidTable(#[from] verter_semantic::analysis::TopLevelOwnerTableError),
    #[error(transparent)]
    InvalidRegions(#[from] verter_semantic::analysis::TopLevelOwnerRegionError),
    #[error("parser owner mapping length mismatch: program has {statement_count} statements, mapping has {owner_count}")]
    ParserTable {
        statement_count: usize,
        owner_count: usize,
    },
}

fn script_owner_index_diagnostic(error: &ScriptOwnerIndexError) -> HostDiagnostic {
    HostDiagnostic {
        severity: HostSeverity::Error,
        code: "script-owner-index".to_string(),
        message: error.to_string(),
        span: Some(verter_span::Span::new(0, 0)),
    }
}

pub(crate) fn top_level_owner_table(
    program: &Program<'_>,
    framework_parse: Option<&verter_language::FrameworkParseArtifact>,
) -> Result<verter_semantic::analysis::TopLevelOwnerTable, ScriptOwnerIndexError> {
    let Some(artifact) = framework_parse else {
        return Ok(
            verter_semantic::analysis::TopLevelOwnerTable::ordinary_file(program.body.len()),
        );
    };
    top_level_owner_table_from_regions(program, &artifact.common.script_regions)
}

fn top_level_owner_table_from_regions(
    program: &Program<'_>,
    regions: &[verter_language::ScriptRegion],
) -> Result<verter_semantic::analysis::TopLevelOwnerTable, ScriptOwnerIndexError> {
    let regions = regions
        .iter()
        .map(|region| (region.span, region.kind))
        .collect::<Vec<_>>();
    top_level_owner_table_from_region_spans(program, &regions)
}

fn vue_top_level_owner_table(
    program: &Program<'_>,
    parsed: &ParsedSfc,
) -> Result<verter_semantic::analysis::TopLevelOwnerTable, ScriptOwnerIndexError> {
    let mut regions = Vec::new();
    if let Some(span) = parsed.script().and_then(|script| script.content) {
        regions.push((
            verter_span::Span::new(span.start, span.end),
            verter_language::ScriptRegionKind::Module,
        ));
    }
    if let Some(span) = parsed.script_setup().and_then(|script| script.content) {
        regions.push((
            verter_span::Span::new(span.start, span.end),
            verter_language::ScriptRegionKind::Instance,
        ));
    }
    top_level_owner_table_from_region_spans(program, &regions)
}

fn top_level_owner_table_from_region_spans(
    program: &Program<'_>,
    regions: &[(verter_span::Span, verter_language::ScriptRegionKind)],
) -> Result<verter_semantic::analysis::TopLevelOwnerTable, ScriptOwnerIndexError> {
    let mut regions = regions.to_vec();
    // A script block with no inline content — an external `<script src=...>`
    // block, or a genuinely empty `<script></script>` — contributes NO
    // top-level statements to the parsed program (an external source is
    // merged in later, at compile time). The carrier emits an EMPTY content
    // span for such a block (the `tag_open.end` fallback, `start == end`).
    // Such a region owns nothing at this stage, so drop it before building
    // the owner mapping: keeping it would consume an owner ordinal and make
    // `try_with_regions` reject the empty span as `EmptyRegion`, so a
    // `<script src=...>` beside a `<script setup>` (or any empty script
    // block) would fail to index.
    regions.retain(|(span, _)| span.start < span.end);
    regions.sort_by_key(|(span, _)| (span.start, span.end));
    for pair in regions.windows(2) {
        if pair[1].0.start < pair[0].0.end {
            return Err(ScriptOwnerIndexError::OverlappingRegions {
                left_start: pair[0].0.start,
                left_end: pair[0].0.end,
                right_start: pair[1].0.start,
                right_end: pair[1].0.end,
            });
        }
    }

    let mut module_ordinal = 0_u32;
    let mut instance_ordinal = 0_u32;
    let mut frontmatter_ordinal = 0_u32;
    let regions = regions
        .into_iter()
        .map(|(span, kind)| {
            let owner = match kind {
                verter_language::ScriptRegionKind::Module => {
                    let owner = verter_type_expr::TopLevelOwnerId::module(module_ordinal);
                    module_ordinal = module_ordinal.saturating_add(1);
                    owner
                }
                verter_language::ScriptRegionKind::Instance => {
                    let owner = verter_type_expr::TopLevelOwnerId::instance(instance_ordinal);
                    instance_ordinal = instance_ordinal.saturating_add(1);
                    owner
                }
                verter_language::ScriptRegionKind::Frontmatter => {
                    let owner = verter_type_expr::TopLevelOwnerId::frontmatter(frontmatter_ordinal);
                    frontmatter_ordinal = frontmatter_ordinal.saturating_add(1);
                    owner
                }
            };
            (span, owner)
        })
        .collect::<Vec<_>>();

    let mut owners = Vec::with_capacity(program.body.len());
    for (statement_index, statement) in program.body.iter().enumerate() {
        let span = statement.span();
        let mut matches = regions
            .iter()
            .filter(|(region, _)| span.start >= region.start && span.end <= region.end);
        let Some((_, owner)) = matches.next() else {
            return Err(ScriptOwnerIndexError::UnownedStatement {
                statement_index,
                start: span.start,
                end: span.end,
            });
        };
        if matches.next().is_some() {
            return Err(ScriptOwnerIndexError::AmbiguousStatement {
                statement_index,
                start: span.start,
                end: span.end,
            });
        }
        owners.push(*owner);
    }
    let table = verter_semantic::analysis::TopLevelOwnerTable::try_from_statement_owners(
        program.body.len(),
        owners,
    )?;
    Ok(table.try_with_regions(
        regions
            .into_iter()
            .map(|(span, owner)| verter_semantic::analysis::TopLevelOwnerRegion { owner, span }),
    )?)
}

/// Zero-copy attribute extraction: returns slices borrowed from `source`.
pub(crate) fn extract_attrs<'a>(props: &[NodeProp], source: &'a str) -> Vec<(&'a str, &'a str)> {
    props
        .iter()
        .map(|p| {
            let name = &source[p.start as usize..p.name_end as usize];
            let value = match (p.value_start, p.value_end) {
                (Some(s), Some(e)) => &source[s as usize..e as usize],
                _ => "",
            };
            (name, value)
        })
        .collect()
}

pub(crate) fn normalize_attr_map(attrs: &[(&str, &str)], include: &[&str]) -> String {
    let mut map = BTreeMap::<&str, &str>::new();
    for &(k, v) in attrs {
        if let Some(&key) = include.iter().find(|&&s| k.eq_ignore_ascii_case(s)) {
            let value = if v.is_empty() { "true" } else { v };
            map.insert(key, value);
        }
    }
    let mut out = String::new();
    for (k, v) in map {
        let _ = writeln!(&mut out, "{}={}", k, v);
    }
    out
}

pub(crate) fn find_attr(attrs: &[(&str, &str)], name: &str) -> Option<String> {
    attrs
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| {
            if v.is_empty() {
                "true".to_string()
            } else {
                v.to_string()
            }
        })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn try_resolve_src_block(
    canonical_id: &str,
    attrs: &[(&str, &str)],
    tag_name: &str,
    block_kind: ExternalBlockKind,
    index: usize,
    tag_open_start: u32,
    tag_open_end: u32,
    tag_close: Option<u32>,
    external_requests: &mut Vec<ExternalSourceRequest>,
    src_blocks: &mut Vec<SrcBlockInfo>,
) {
    if let Some(src) = find_attr(attrs, "src") {
        let resolved = resolve_external(canonical_id, &src);
        external_requests.push(ExternalSourceRequest {
            owner_canonical_id: canonical_id.to_string(),
            block_kind,
            index,
            specifier: src,
            resolved_canonical_id: resolved.clone(),
        });
        src_blocks.push(SrcBlockInfo {
            tag_name: tag_name.to_string(),
            resolved_canonical_id: resolved,
            tag_open_start,
            tag_open_end,
            tag_close_start: tag_close,
        });
    }
}

/// The single counted carrier-parse chokepoint for `verter_session`.
///
/// EVERY framework carrier parse the host materializes — Vue, Svelte,
/// and every later vertical — routes through here. It bumps the
/// framework-neutral `MetaProvenance::carrier_parses` rail exactly once
/// per `CarrierCompiler::parse`, and the Vue compatibility rail
/// `sfc_parses` when (and only when) the dispatched carrier is Vue.
/// Counting lives in the HOST, not the carrier: the compiler is the
/// parser/producer only — it owns no provenance, lease, or lifecycle
/// state. A direct `CarrierCompiler::parse` / registry `.parse()` call
/// anywhere else in the crate is an uncounted parse the dedup suite
/// cannot see (guard:
/// `carrier_parse_routes_through_the_counted_chokepoint`).
pub(crate) fn parse_carrier_counted(
    provenance: &crate::types::MetaProvenance,
    compiler: &dyn verter_compiler::framework_common::CarrierCompiler,
    source: &str,
    opts: &verter_compiler::framework_common::ParseOptions,
) -> Arc<verter_language::FrameworkParseArtifact> {
    use std::sync::atomic::Ordering::Relaxed;
    provenance.carrier_parses.fetch_add(1, Relaxed);
    if compiler.adapter_id().is_vue() {
        // Vue compatibility rail: every Vue carrier parse stays visible
        // on the historical `sfc_parses` counter the dedup suite pins.
        provenance.sfc_parses.fetch_add(1, Relaxed);
    }
    compiler.parse(source, opts)
}

/// The process-wide compiler-side carrier-compiler registry.
///
/// The carrier parse dispatch (`execute_source` → [`carrier_parse_snapshot`])
/// looks the file's adapter compiler up here. The registry is stateless
/// (it owns the per-framework [`CarrierCompiler`](verter_compiler::framework_common::CarrierCompiler)
/// implementations), so one process-wide instance serves every host.
pub(crate) fn carrier_compiler_registry(
) -> &'static verter_compiler::framework_common::CarrierCompilerRegistry {
    use std::sync::OnceLock;
    static REGISTRY: OnceLock<verter_compiler::framework_common::CarrierCompilerRegistry> =
        OnceLock::new();
    REGISTRY.get_or_init(verter_compiler::framework_common::CarrierCompilerRegistry::built_in)
}

/// Produce a carrier file's `ParseSnapshot` + framework-neutral artifact by
/// dispatching the parse through the compiler-side carrier registry.
///
/// The SINGLE carrier parse dispatch the host executor reaches: it interns the
/// file's adapter id, looks up its [`CarrierCompiler`](verter_compiler::framework_common::CarrierCompiler)
/// (Vue via the bridge), and produces the framework-neutral artifact through
/// the counted carrier chokepoint ([`parse_carrier_counted`]). The host then
/// reaches the artifact's typed carrier back out (the blessed `vue_parse`
/// accessor) to build the Vue-shaped `ParseSnapshot`. Routing the parse through
/// the registry keeps Vue's compile output byte-identical (the bridge calls
/// `parse_sfc(source, None, None)` and stamps the same parser version) while a
/// later carrier vertical's compiler drops in without a second dispatch branch.
///
/// This is the SCHEDULER Source-stage dispatch: it has no flight-shared eval
/// program, so the Vue snapshot's script walk parses once here
/// ([`VueScriptProgram::ParseHere`]). The cold-materialise flight reaches its
/// own reuse decision (sharing the retained eval program) in
/// `ensure_indexed_ready_serve`.
pub(crate) fn carrier_parse_snapshot(
    canonical_id: &str,
    source: &str,
    analysis_scope: verter_semantic::analysis::AnalysisScope,
    file_language: &verter_language::FileLanguage,
    provenance: &crate::types::MetaProvenance,
) -> Option<(ParseSnapshot, Arc<verter_language::FrameworkParseArtifact>)> {
    // The row must be a true CARRIER (it carries a carrier language id) AND
    // the registry must serve THAT carrier language — a same-adapter
    // non-carrier row (an external template) is NOT dispatched by adapter
    // id alone.
    let adapter_id = file_language.adapter_id()?;
    let carrier_language_id = file_language.carrier_language_id()?;
    let compiler = carrier_compiler_registry()
        .compiler_for_carrier_language(adapter_id, carrier_language_id)?;
    let artifact = parse_carrier_counted(
        provenance,
        compiler.as_ref(),
        source,
        &verter_compiler::framework_common::ParseOptions::default(),
    );
    // Vue builds the Vue-shaped snapshot through the blessed `vue_parse`
    // accessor; Svelte builds its snapshot from the neutral artifact's script
    // regions (the script analysis runs over the position-preserving
    // eval-source). The carrier-row dispatch chose the compiler, so the artifact
    // is that carrier's — open it through the matching accessor.
    if let Some(parsed) = crate::typeinfo::adapters::vue::vue_parse(&artifact) {
        let snapshot = build_vue_snapshot_from_parsed(
            canonical_id,
            source,
            analysis_scope,
            parsed,
            provenance,
            VueScriptProgram::ParseHere,
            None,
        );
        return Some((snapshot, artifact));
    }
    if crate::typeinfo::adapters::svelte::svelte_parse(&artifact).is_some() {
        let eval_source = compiler.eval_source(source, &artifact);
        let snapshot = build_svelte_snapshot_from_eval_source(
            canonical_id,
            source,
            eval_source.as_ref(),
            &artifact,
            provenance,
            FrameworkScriptProgram::ParseHere,
            None,
        );
        return Some((snapshot, artifact));
    }
    None
}

/// Capture a file's active framework script-fact candidates from its
/// eval-source (the synth-injection parse-domain inputs).
///
/// Returns an empty set when no provider is active or no eval-source is
/// available. The OXC parse lives HERE (the scheduler-bound parse module) rather
/// than in the host body — the synth injection threads the resolved active
/// provider set in. Syntax-only: no resolver, no capability bits.
/// `canonical_id` is the PRODUCING canonical the captured payload-ref anchors
/// absolutize to (producer-side, through each envelope's owning provider)
/// before the set feeds the synthesis.
pub(crate) fn capture_synth_script_candidates(
    active_providers: &[std::sync::Arc<
        dyn verter_semantic::analysis::framework_facts::ScriptFactProvider,
    >],
    canonical_id: &str,
    eval_source: Option<&str>,
    module_script_region: Option<(u32, u32)>,
    framework_mode_hint: Option<
        verter_semantic::analysis::framework_facts::FrameworkScriptModeHint,
    >,
    source_type: SourceType,
    owner_table: &verter_semantic::analysis::TopLevelOwnerTable,
) -> verter_semantic::analysis::framework_facts::FrameworkScriptCandidateSet {
    use verter_semantic::analysis::framework_facts::FrameworkScriptCandidateSet;
    if active_providers.is_empty() {
        return FrameworkScriptCandidateSet::default();
    }
    let Some(source) = eval_source else {
        return FrameworkScriptCandidateSet::default();
    };
    // Parse the eval-source ONCE under the carrier's resolved script dialect (so
    // a `lang="tsx"` `.svelte` parses as TSX) and capture across the active
    // providers. The eval-source carries both script blocks at raw offsets.
    let alloc = Allocator::new();
    let program = Parser::new(&alloc, source, source_type)
        .with_options(ParseOptions {
            parse_regular_expression: false,
            ..ParseOptions::default()
        })
        .parse()
        .program;
    let mut set =
        verter_semantic::analysis::framework_facts::capture_script_candidates_with_context(
            active_providers,
            source,
            &program,
            module_script_region,
            framework_mode_hint,
            owner_table,
        );
    // Producer-side locator absolutization: route each envelope through its
    // OWNING provider (typed downcast + coherent `stable_hash` rebuild) so the
    // synthesis consumes absolute payload-ref anchors, matching the candidate
    // store's fill at `framework::script_facts`.
    set.per_provider = set
        .per_provider
        .into_iter()
        .map(|candidates| {
            match active_providers
                .iter()
                .find(|provider| provider.adapter_id() == candidates.adapter_id)
            {
                Some(provider) => provider.absolutize_candidates(candidates, canonical_id),
                None => candidates,
            }
        })
        .collect();
    set
}

/// Absolutize the analyzer-emitted macro-payload locator anchors to the
/// PRODUCING canonical, before the snapshot enters any host-owned storage.
///
/// The analyzer (`verter_semantic`) is path-agnostic and stamps every
/// macro-payload locator with the local-file EMPTY-sentinel anchor
/// (`canonical_id == ""`); the SESSION alone knows the artifact identity
/// backing the `DeclBodyMemo` a deref will serve through, so it fills the
/// producing canonical here — a PRODUCER-side fill, never a consumer
/// tolerance (the deref boundary keeps rejecting a canonical mismatch).
///
/// Fills ONLY empty anchors: a non-empty anchor may be a cross-file
/// resolver's canonical (the locator contract) and is never rewritten —
/// which also makes the pass idempotent. An empty `canonical_id` (no
/// producing identity to absolutize to) leaves the sentinel in place.
pub(crate) fn absolutize_macro_payload_anchors(
    macros: &mut [verter_semantic::analysis::types::AnalyzedMacro],
    canonical_id: &str,
) {
    use verter_type_expr::locators::MacroPayloadLocator;
    if canonical_id.is_empty() {
        return;
    }
    let canonical: std::sync::Arc<str> = std::sync::Arc::from(canonical_id);
    let fill = |locator: &mut MacroPayloadLocator| {
        if locator.anchor.canonical_id.is_empty() {
            locator.anchor.canonical_id = std::sync::Arc::clone(&canonical);
        }
    };
    for mac in macros.iter_mut() {
        if let Some(locator) = mac.parsed_type_argument.as_mut() {
            fill(locator);
        }
        for field in &mut mac.prop_fields {
            if let Some(locator) = field.payload.as_mut() {
                fill(locator);
            }
        }
        for field in &mut mac.emit_fields {
            if let Some(locator) = field.payload.as_mut() {
                fill(locator);
            }
        }
        for field in &mut mac.slot_fields {
            if let Some(locator) = field.payload.as_mut() {
                fill(locator);
            }
            // Slot BINDING payloads are analyzer-`None` today (the flat field
            // vocabulary cannot address a nested position); walked under the
            // same fill-only-empty rule so a future producer-emitted binding
            // payload absolutizes identically.
            for binding in &mut field.bindings {
                if let Some(locator) = binding.payload.as_mut() {
                    fill(locator);
                }
            }
        }
        for field in &mut mac.expose_fields {
            if let Some(locator) = field.payload.as_mut() {
                fill(locator);
            }
        }
    }
}

/// The OXC [`SourceType`] for a carrier artifact's combined eval-source. Joins
/// every script region because module and instance blocks may use different
/// dialects; TS/JSX promote to the grammar capable of parsing the whole
/// extracted program. Defaults to TS when the carrier has no script region.
pub(crate) fn carrier_eval_source_type(
    framework_parse: Option<&verter_language::FrameworkParseArtifact>,
) -> SourceType {
    framework_parse
        .and_then(|artifact| combined_framework_script_source_type(&artifact.common.script_regions))
        .map(oxc_source_type_from_neutral)
        .unwrap_or_else(SourceType::ts)
}

/// The MODULE-script byte region (`<script module>` / legacy
/// `context="module"`) of a framework carrier artifact, when it records one — so
/// a script-fact provider can classify a declaration's owning block. Reads the
/// neutral `FrameworkParseCommon.script_regions` (no per-carrier downcast).
pub(crate) fn module_script_region(
    artifact: &verter_language::FrameworkParseArtifact,
) -> Option<(u32, u32)> {
    artifact
        .common
        .script_regions
        .iter()
        .find(|region| region.kind == verter_language::ScriptRegionKind::Module)
        .map(|region| (region.span.start, region.span.end))
}

/// The parser-owned non-script mode inputs carried by a neutral parse artifact.
///
/// Svelte captures `<svelte:options runes={...}>` during its single carrier
/// parse. Script-fact capture receives that typed fact rather than rescanning
/// markup and combines it with the already-parsed scripts through the shared
/// reactivity-mode classifier.
pub(crate) fn framework_script_mode_hint(
    artifact: &verter_language::FrameworkParseArtifact,
) -> Option<verter_semantic::analysis::framework_facts::FrameworkScriptModeHint> {
    let parsed = crate::typeinfo::adapters::svelte::svelte_parse(artifact)?;
    Some(
        verter_semantic::analysis::framework_facts::FrameworkScriptModeHint::Svelte {
            forced_runes: parsed.forced_runes,
            // This capture produces SCRIPT candidates only. A template-only
            // `$host` cannot create a script declaration or public script
            // surface, so it is deliberately outside this producer's domain;
            // the IDE projector derives that fact from its typed expression
            // walk when selecting the template prelude.
            template_uses_host_rune: false,
        },
    )
}

/// Classify a retained Svelte carrier's combined eval program under the shared
/// scope-aware reactivity authority. The caller already owns both the neutral
/// parse artifact and the retained OXC program, so this performs no text scan
/// and no reparse.
pub(crate) fn svelte_component_runes_mode(
    artifact: &verter_language::FrameworkParseArtifact,
    program: &oxc_ast::ast::Program<'_>,
) -> bool {
    let Some(parsed) = crate::typeinfo::adapters::svelte::svelte_parse(artifact) else {
        return false;
    };
    verter_parser::svelte_reactivity::infer_combined_program_mode(
        program,
        module_script_region(artifact),
        parsed.forced_runes,
        // DeclBodyMemo indexes only the position-preserving script program. A
        // template-only `$host` has no declaration-body lookup to affect; the
        // IDE expression path owns its ambient template typing.
        false,
    )
    .is_runes()
}

/// Build a Svelte carrier file's `ParseSnapshot` from its position-preserving
/// eval-source.
///
/// The eval-source carries BOTH script blocks (instance + module) at their raw
/// carrier-absolute offsets and blanks everything else, so the script analysis
/// over it produces carrier-absolute spans for free (the same property the Vue
/// path relies on). The `whole_hash` hashes the ORIGINAL component source (the
/// file content identity), while the analysis runs over the eval-source. The
/// `script_hash` slice covers each script block's open tag and content so a
/// script-only edit invalidates distinctly from a template-only edit.
fn build_svelte_snapshot_from_eval_source(
    canonical_id: &str,
    source: &str,
    eval_source: &str,
    artifact: &verter_language::FrameworkParseArtifact,
    provenance: &crate::types::MetaProvenance,
    script_program: FrameworkScriptProgram<'_>,
    script_owners: Option<&verter_semantic::analysis::TopLevelOwnerTable>,
) -> ParseSnapshot {
    let whole_hash = hash_16(source.as_bytes());

    let parsed = crate::typeinfo::adapters::svelte::svelte_parse(artifact)
        .expect("a Svelte carrier artifact must retain its typed parse");

    // Hash the complete authored block prefix (open tag + content), not only
    // the script body. A `lang` / `module` attribute changes compilation just
    // as surely as a body edit and must invalidate the script slice.
    let mut script_hashes = Vec::new();
    let mut scripts = [
        parsed.instance_script.as_ref(),
        parsed.module_script.as_ref(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    scripts.sort_by_key(|script| script.tag_open.start);
    for script in &scripts {
        let start = script.tag_open.start as usize;
        let end = script
            .content
            .map_or(script.tag_open.end, |content| content.end) as usize;
        script_hashes.push(hash_16(
            source.as_bytes().get(start..end).unwrap_or_default(),
        ));
    }

    let script_hash = if script_hashes.is_empty() {
        None
    } else {
        let mut buf = Vec::with_capacity(script_hashes.len() * 16);
        for hash in &script_hashes {
            buf.extend_from_slice(hash);
        }
        Some(hash_16(&buf))
    };

    let mut style_hashes = Vec::with_capacity(parsed.styles.len());
    let mut style_attr_fingerprints = Vec::with_capacity(parsed.styles.len());
    let mut style_langs = Vec::with_capacity(parsed.styles.len());
    let mut style_content_spans = Vec::with_capacity(parsed.styles.len());
    for style in &parsed.styles {
        let start = style.tag_open.start as usize;
        let end = style
            .content
            .map_or(style.tag_open.end, |content| content.end) as usize;
        style_hashes.push(hash_16(
            source.as_bytes().get(start..end).unwrap_or_default(),
        ));
        style_attr_fingerprints.push(
            source
                .get(style.tag_open.start as usize..style.tag_open.end as usize)
                .unwrap_or_default()
                .to_string(),
        );
        style_langs.push(style.attributes.iter().find_map(|attr| {
            use verter_compiler::svelte::parser::{SvelteAttributeKind, SvelteAttributeValue};
            match &attr.kind {
                SvelteAttributeKind::Plain {
                    name,
                    value: Some(SvelteAttributeValue::Text(span)),
                    ..
                } if name.eq_ignore_ascii_case("lang") => source
                    .get(span.start as usize..span.end as usize)
                    .map(ToOwned::to_owned),
                _ => None,
            }
        }));
        style_content_spans.push(style.content.map(|span| (span.start, span.end)));
    }

    // Svelte's template is the top-level carrier body rather than an explicit
    // `<template>` block. Hash the exact bytes outside complete top-level
    // script/style ranges. Removing (rather than blanking) those ranges keeps
    // length-changing block edits in their own slice while preserving every
    // authored template byte. The parser-retained close spans are the boundary
    // authority; this layer never re-scans raw source for closing tags.
    let mut excluded_ranges = scripts
        .iter()
        .map(|script| {
            (
                script.tag_open.start as usize,
                script
                    .tag_close
                    .or(script.content)
                    .map_or(script.tag_open.end, |span| span.end) as usize,
            )
        })
        .chain(parsed.styles.iter().map(|style| {
            (
                style.tag_open.start as usize,
                style
                    .tag_close
                    .or(style.content)
                    .map_or(style.tag_open.end, |span| span.end) as usize,
            )
        }))
        .collect::<Vec<_>>();
    excluded_ranges.sort_unstable_by_key(|(start, _)| *start);
    let mut template_bytes = Vec::with_capacity(source.len());
    let mut cursor = 0;
    for (start, end) in excluded_ranges {
        if start >= cursor && end <= source.len() {
            template_bytes.extend_from_slice(&source.as_bytes()[cursor..start]);
            cursor = end;
        }
    }
    template_bytes.extend_from_slice(&source.as_bytes()[cursor..]);
    let template_hash = (!parsed.template.is_empty()).then(|| hash_16(&template_bytes));

    let descriptor = DescriptorMin {
        script_count: scripts.len(),
        template_count: usize::from(!parsed.template.is_empty()),
        style_count: parsed.styles.len(),
        custom_count: 0,
        script_attr_fingerprints: scripts
            .iter()
            .map(|script| {
                source
                    .get(script.tag_open.start as usize..script.tag_open.end as usize)
                    .unwrap_or_default()
                    .to_string()
            })
            .collect(),
        template_attr_fingerprints: Vec::new(),
        style_attr_fingerprints,
        custom_attr_fingerprints: Vec::new(),
        vapor: false,
    };

    // Runtime codegen can depend on any carrier byte (template, style hash,
    // options, or scripts), so the compile-cache semantic identity is the whole
    // component. The granular slices above remain available for precise public
    // change reporting; they are never used to excuse a stale warm hit.
    let semantic_hash = whole_hash;

    let preprocessor_requests = build_preprocessor_requests(
        &None,
        None,
        &None,
        None,
        &style_langs,
        &style_content_spans,
        &[],
        &[],
        &[],
        source,
    );

    // Run the shallow analysis over the eval-source under the carrier's RESOLVED
    // script dialect (the producer stamped `lang="ts"/"tsx"/"jsx"/"js"` onto the
    // script regions) so a `lang="tsx"` `.svelte` parses as TSX, not plain TS.
    // The eval-source's blanked geometry keeps every span carrier-absolute.
    let source_type = combined_framework_script_source_type(&artifact.common.script_regions)
        .map(oxc_source_type_from_neutral)
        .unwrap_or_else(SourceType::ts);

    let fatal_snapshot = |owner_error: Option<&ScriptOwnerIndexError>| ParseSnapshot {
        whole_hash,
        semantic_hash,
        slices: SliceHashes::default(),
        descriptor: DescriptorMin::default(),
        meta: FileMeta::default(),
        external_requests: Vec::new(),
        src_blocks: Vec::new(),
        parse_diagnostics: owner_error.map_or_else(DiagnosticsSnapshot::default, |error| {
            DiagnosticsSnapshot::from_vec(vec![script_owner_index_diagnostic(error)])
        }),
        script_analysis: Arc::new(verter_semantic::analysis::ScriptAnalysisSnapshot::default()),
        export_signatures: Vec::new(),
        style_analyses: Vec::new(),
        markup_class_tokens: Vec::new(),
        preprocessor_requests: Vec::new(),
    };

    // The eval-source IS the carrier's position-preserving extracted script — so
    // the cold-materialise flight's retained eval program (which parsed exactly
    // these bytes) IS this snapshot's program: walk it, parse nothing. A lane
    // with no flight-shared program (the scheduler Source stage) parses once
    // here. Either way the Svelte cold build pays exactly one parse of the
    // script bytes.
    let mut snapshot = match script_program {
        FrameworkScriptProgram::Shared(program) => {
            debug_assert_eq!(
                program.source_str(),
                eval_source,
                "a shared eval program must carry this carrier file's \
                 position-preserving eval source",
            );
            if let Some(owners) = script_owners {
                build_snapshot_from_program_with_owners(
                    canonical_id,
                    eval_source,
                    source_type,
                    program.borrow_dependent(),
                    owners,
                    program.had_errors(),
                )
            } else {
                match top_level_owner_table(program.borrow_dependent(), Some(artifact)) {
                    Ok(owners) => build_snapshot_from_program_with_owners(
                        canonical_id,
                        eval_source,
                        source_type,
                        program.borrow_dependent(),
                        &owners,
                        program.had_errors(),
                    ),
                    Err(error) => fatal_snapshot(Some(&error)),
                }
            }
        }
        FrameworkScriptProgram::SharedFatal => fatal_snapshot(None),
        FrameworkScriptProgram::ParseHere => {
            // No flight-shared program: parse the eval source once here. This is
            // the scheduler Source-stage snapshot lane — counted on the same
            // full-program rail as the non-SFC snapshot lane.
            provenance
                .non_sfc_snapshot_parses
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let alloc = Allocator::new();
            let parser = Parser::new(&alloc, eval_source, source_type).with_options(ParseOptions {
                parse_regular_expression: false,
                ..ParseOptions::default()
            });
            let result = parser.parse();
            if result.panicked {
                fatal_snapshot(None)
            } else {
                match top_level_owner_table(&result.program, Some(artifact)) {
                    Ok(owners) => build_snapshot_from_program_with_owners(
                        canonical_id,
                        eval_source,
                        source_type,
                        &result.program,
                        &owners,
                        !result.errors.is_empty(),
                    ),
                    Err(error) => fatal_snapshot(Some(&error)),
                }
            }
        }
    };
    // The component identity and structural inventory come from the ORIGINAL
    // carrier source, not the position-preserving eval-source. Svelte exposes
    // its runtime as Main + compiled Style nodes; the cached neutral script
    // analysis is not itself a bundler Script virtual file.
    snapshot.whole_hash = whole_hash;
    snapshot.semantic_hash = semantic_hash;
    snapshot.slices = SliceHashes {
        script: script_hash,
        template: template_hash,
        styles: style_hashes,
        custom: Vec::new(),
    };
    snapshot.descriptor = descriptor;
    // Svelte style + markup class facts — the carrier analog of the Vue
    // style-analysis / template-element class inventory. Svelte styles are
    // scoped by default; per-selector `:global(...)` opt-outs are recorded by
    // the scanner as special pseudos.
    snapshot.style_analyses = build_svelte_style_analyses(source, &parsed.styles, &style_langs);
    snapshot.markup_class_tokens = collect_svelte_markup_class_tokens(source, &parsed.template);
    snapshot.meta = FileMeta {
        has_script: false,
        has_template: false,
        main_depends_on_styles: true,
        has_scoped_style: false,
        script_lang: scripts.first().and_then(|script| script.lang.clone()),
        template_lang: None,
        style_langs,
        custom_types: Vec::new(),
        custom_langs: Vec::new(),
    };
    snapshot.preprocessor_requests = preprocessor_requests;
    snapshot
}

/// Build [`verter_semantic::analysis::StyleBlockAnalysis`] facts for a Svelte
/// component's `<style>` blocks: scoped-by-default, scanned through the shared
/// dialect-aware CSS scanner (css/scss/less), carrier-absolute spans.
pub(crate) fn build_svelte_style_analyses(
    source: &str,
    styles: &[verter_compiler::svelte::parser::SvelteStyle],
    style_langs: &[Option<String>],
) -> Vec<verter_semantic::analysis::StyleBlockAnalysis> {
    styles
        .iter()
        .enumerate()
        .filter_map(|(idx, style)| {
            let span = style.content?;
            let content = source.get(span.start as usize..span.end as usize)?;
            let lang = match style_langs.get(idx).and_then(|l| l.as_deref()) {
                None | Some("css") | Some("postcss") => {
                    verter_semantic::analysis::StyleAnalysisLang::Css
                }
                Some("scss") => verter_semantic::analysis::StyleAnalysisLang::Scss,
                Some("sass") => verter_semantic::analysis::StyleAnalysisLang::Sass,
                Some("less") => verter_semantic::analysis::StyleAnalysisLang::Less,
                Some("stylus") => verter_semantic::analysis::StyleAnalysisLang::Stylus,
                Some(_) => verter_semantic::analysis::StyleAnalysisLang::Unknown,
            };
            let analysis = verter_semantic::analysis::build_scanned_style_analysis(
                lang,
                content,
                verter_semantic::analysis::VueStyleInput::default(),
                // Svelte styles are component-scoped by default.
                true,
                false,
                None,
                span.start,
            );
            if let Some(css) = &analysis.css {
                css.debug_assert_valid_spans(source.len() as u32);
            }
            Some(analysis)
        })
        .collect()
}

/// Collect resolvable markup class tokens from a Svelte template AST:
/// whitespace-separated names in static `class="a b"` values and the local
/// name of every `class:x` directive. Dynamic (`class={expr}`) and mixed
/// values are skipped — fail closed, never a guessed token.
pub(crate) fn collect_svelte_markup_class_tokens(
    source: &str,
    nodes: &[verter_compiler::svelte::parser::SvelteNode],
) -> Vec<verter_semantic::analysis::MarkupClassToken> {
    use verter_compiler::svelte::parser::{
        SvelteAttributeKind, SvelteAttributeValue, SvelteDirectiveKind, SvelteNode,
    };

    fn walk(
        source: &str,
        nodes: &[SvelteNode],
        out: &mut Vec<verter_semantic::analysis::MarkupClassToken>,
    ) {
        for node in nodes {
            match node {
                SvelteNode::Element(el) => {
                    for attr in &el.attributes {
                        match &attr.kind {
                            SvelteAttributeKind::Plain {
                                name,
                                value: Some(SvelteAttributeValue::Text(value_span)),
                                ..
                            } if name == "class" => {
                                let Some(text) =
                                    source.get(value_span.start as usize..value_span.end as usize)
                                else {
                                    continue;
                                };
                                // Whitespace-split with exact positions.
                                let bytes = text.as_bytes();
                                let mut i = 0usize;
                                while i < bytes.len() {
                                    if bytes[i].is_ascii_whitespace() {
                                        i += 1;
                                        continue;
                                    }
                                    let start = i;
                                    while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                                        i += 1;
                                    }
                                    out.push(verter_semantic::analysis::MarkupClassToken {
                                        name: text[start..i].to_string(),
                                        span: verter_span::Span::new(
                                            value_span.start + start as u32,
                                            value_span.start + i as u32,
                                        ),
                                        from_directive: false,
                                    });
                                }
                            }
                            SvelteAttributeKind::Directive(dir)
                                if dir.kind == SvelteDirectiveKind::Class
                                    && !dir.local.is_empty() =>
                            {
                                // `class:x` — the local name starts right after
                                // the `class:` prefix of the attribute span.
                                let start = attr.span.start + "class:".len() as u32;
                                let end = start + dir.local.len() as u32;
                                // Verify against the source before trusting the
                                // arithmetic (fail closed on any drift).
                                if source.get(start as usize..end as usize)
                                    == Some(dir.local.as_str())
                                {
                                    out.push(verter_semantic::analysis::MarkupClassToken {
                                        name: dir.local.clone(),
                                        span: verter_span::Span::new(start, end),
                                        from_directive: true,
                                    });
                                }
                            }
                            _ => {}
                        }
                    }
                    walk(source, &el.children, out);
                }
                SvelteNode::Block(block) => {
                    walk(source, &block.children, out);
                    for clause in &block.clauses {
                        walk(source, &clause.children, out);
                    }
                }
                _ => {}
            }
        }
    }

    let mut out = Vec::new();
    walk(source, nodes, &mut out);
    out
}

/// Where a framework carrier snapshot build gets its script PROGRAM from — the
/// framework-neutral analog of [`VueScriptProgram`] for carriers whose snapshot
/// is built by walking the position-preserving eval program (Svelte today).
///
/// A cold-materialise flight already OXC-parses the position-preserving eval
/// source ONCE as its eval program; the snapshot build walks that SAME program
/// instead of paying a second parse over the same bytes. Lanes with no
/// flight-shared program (the scheduler Source stage) parse here.
pub(crate) enum FrameworkScriptProgram<'a> {
    /// No flight-shared program: parse the eval source once here (counted on
    /// the `non_sfc_snapshot_parses` full-program rail).
    ParseHere,
    /// The flight's eval program IS the snapshot's script program (the eval
    /// source was the position-preserving extracted script): walk it, parse
    /// nothing.
    Shared(&'a crate::ParsedEvalProgram),
    /// The flight's single eval-program parse was fatal (recovered panic). A
    /// re-parse over the same bytes under the same source type fails
    /// identically, so the snapshot defaults directly with zero additional
    /// parses — the carrier mirror of [`VueScriptProgram::SharedFatal`].
    SharedFatal,
}

/// Produce a Vue carrier file's `ParseSnapshot` + artifact.
///
/// A thin Vue-pinned entry over [`carrier_parse_snapshot`]: every Vue parse
/// routes through the carrier registry (the bridge) and the counted carrier
/// chokepoint — there is no second Vue direct-parse path. The dispatch is
/// infallible for Vue (the registry always registers the Vue bridge, the
/// produced artifact is always a Vue carrier), so an unexpected miss is a build
/// defect, surfaced loudly rather than silently re-parsed.
pub(crate) fn parse_vue_snapshot(
    canonical_id: &str,
    source: &str,
    analysis_scope: verter_semantic::analysis::AnalysisScope,
    provenance: &crate::types::MetaProvenance,
) -> (ParseSnapshot, Arc<verter_language::FrameworkParseArtifact>) {
    carrier_parse_snapshot(
        canonical_id,
        source,
        analysis_scope,
        &verter_language::FileLanguage::vue(),
        provenance,
    )
    .expect("the carrier registry registers the Vue bridge; Vue parse cannot miss")
}

/// Parse Vue SFC source straight into the framework-neutral parse
/// artifact (the route-owned cold-parse producer's entry — no
/// `ParseSnapshot` is needed there).
///
/// Routes through the carrier registry (the Vue bridge) — the SAME single
/// dispatch path as [`carrier_parse_snapshot`], so there is no second
/// session-side Vue direct-parse producer. The dispatch is infallible for
/// Vue (the registry always registers the Vue bridge serving the `vue`
/// carrier language).
pub(crate) fn build_vue_parse_artifact_from_source(
    source: &str,
    provenance: &crate::types::MetaProvenance,
) -> Arc<verter_language::FrameworkParseArtifact> {
    let vue = verter_language::FileLanguage::vue();
    let adapter_id = vue.adapter_id().expect("the Vue row carries an adapter id");
    let carrier_language_id = vue
        .carrier_language_id()
        .expect("the Vue row carries a carrier language id");
    let compiler = carrier_compiler_registry()
        .compiler_for_carrier_language(adapter_id, carrier_language_id)
        .expect("the carrier registry registers the Vue bridge serving the vue carrier language");
    parse_carrier_counted(
        provenance,
        compiler.as_ref(),
        source,
        &verter_compiler::framework_common::ParseOptions::default(),
    )
}

/// Build the framework-neutral parse artifact for ANY carrier file from its
/// source, dispatching through the carrier registry by the file's resolved
/// carrier row. Returns `None` for a non-carrier file (a plain script) or a
/// carrier row whose adapter has no registered compiler — the caller then uses
/// the plain-script path. This is the CARRIER-NEUTRAL cold-parse producer the
/// route-owned / overlay materialization paths use (so a `.svelte` cold parse
/// produces a Svelte artifact, not `None`).
pub(crate) fn build_carrier_parse_artifact_from_source(
    file_language: &verter_language::FileLanguage,
    source: &str,
    provenance: &crate::types::MetaProvenance,
) -> Option<Arc<verter_language::FrameworkParseArtifact>> {
    let adapter_id = file_language.adapter_id()?;
    let carrier_language_id = file_language.carrier_language_id()?;
    let compiler = carrier_compiler_registry()
        .compiler_for_carrier_language(adapter_id, carrier_language_id)?;
    Some(parse_carrier_counted(
        provenance,
        compiler.as_ref(),
        source,
        &verter_compiler::framework_common::ParseOptions::default(),
    ))
}

/// The OXC [`SourceType`] of a plain (non-carrier) script file,
/// derived from its classified [`FileLanguage`](verter_language::FileLanguage)
/// row — the language registry is the SOLE plain-script dialect
/// authority (`.d.ts`-family detection included: the registry `Dts`
/// rows own it; session parse code never re-sniffs path extensions).
///
/// Non-script rows (a framework carrier or template reaching a plain
/// parse path, an unclassifiable input) fall back to the registry's
/// own unknown-extension routing: TypeScript.
pub(crate) fn plain_script_source_type(
    file_language: &verter_language::FileLanguage,
) -> SourceType {
    file_language
        .script_source_type()
        .map(oxc_source_type_from_neutral)
        .unwrap_or_else(SourceType::ts)
}

/// Pure source-type computation for an imported eval target.
///
/// Single source of truth; the scheduler caches its result on
/// [`crate::host_executor::HostSourceData::source_type`] so cache-key callers
/// can read the authoritative value via
/// [`crate::VerterHost::authoritative_source_type_for`] instead of recomputing
/// from `(canonical_id, raw_source, framework_parse)` — a pair that is
/// unstable when `framework_parse` is dropped mid-resolution.
///
/// Dispatches on the file's resolved [`FileLanguage`](verter_language::FileLanguage)
/// row: framework carriers read the neutral
/// `FrameworkParseCommon.script_regions[].source_type` their producer
/// populated at parse time (UNIFORMLY — no per-carrier downcast); plain
/// scripts derive from the row's classified dialect
/// ([`plain_script_source_type`]).
pub(crate) fn imported_eval_source_type(
    file_language: &verter_language::FileLanguage,
    framework_parse: Option<&verter_language::FrameworkParseArtifact>,
) -> SourceType {
    if file_language.is_framework_carrier() {
        framework_parse
            .and_then(|artifact| {
                combined_framework_script_source_type(&artifact.common.script_regions)
            })
            .map(oxc_source_type_from_neutral)
            .unwrap_or_else(SourceType::ts)
    } else {
        plain_script_source_type(file_language)
    }
}

/// Join the dialects of every embedded script region into the one grammar used
/// for the carrier's combined analysis program.
///
/// A TypeScript grammar accepts ordinary JavaScript, so any TS region promotes
/// the combined program to TS. JSX similarly promotes the selected grammar to
/// its JSX-bearing form. This matters for Svelte, where module and instance
/// scripts may legitimately use different languages; choosing the first region
/// silently dropped later TS declarations. Single-region carriers preserve the
/// producer's exact dialect and JS module kind.
fn combined_framework_script_source_type(
    regions: &[verter_language::ScriptRegion],
) -> Option<verter_language::ScriptSourceType> {
    use verter_language::ScriptSourceType;

    let first = regions.first()?.source_type;
    let has_typescript = regions.iter().any(|region| {
        matches!(
            region.source_type,
            ScriptSourceType::Ts | ScriptSourceType::Dts
        )
    });
    let has_tsx = regions
        .iter()
        .any(|region| matches!(region.source_type, ScriptSourceType::Tsx));
    let jsx_kind = regions.iter().find_map(|region| match region.source_type {
        ScriptSourceType::Jsx(kind) => Some(kind),
        _ => None,
    });

    if has_tsx || (has_typescript && jsx_kind.is_some()) {
        Some(ScriptSourceType::Tsx)
    } else if has_typescript {
        Some(ScriptSourceType::Ts)
    } else if let Some(kind) = jsx_kind {
        Some(ScriptSourceType::Jsx(kind))
    } else {
        Some(first)
    }
}

/// Map the neutral [`verter_language::ScriptSourceType`] dialect onto
/// the OXC [`SourceType`] the parser pipeline consumes.
///
/// JavaScript dialects carry their [`verter_language::JsModuleKind`]
/// through to OXC's module kind: `import`/`export` are module-only
/// syntax, so the kind decides whether module `.js`/`.mjs` content
/// parses (Unambiguous/Module), CommonJS stays CommonJS (`.cjs`), and
/// the Vue carrier's classic-script `lang="js"` row keeps its
/// historical `SourceType::script()`.
pub(crate) fn oxc_source_type_from_neutral(
    source_type: verter_language::ScriptSourceType,
) -> SourceType {
    match source_type {
        verter_language::ScriptSourceType::Ts => SourceType::ts(),
        verter_language::ScriptSourceType::Tsx => SourceType::tsx(),
        verter_language::ScriptSourceType::Js(kind) => oxc_js_base(kind),
        verter_language::ScriptSourceType::Jsx(kind) => oxc_js_base(kind).with_jsx(true),
        verter_language::ScriptSourceType::Dts => SourceType::d_ts(),
    }
}

/// The JavaScript [`SourceType`] for a neutral module kind.
fn oxc_js_base(kind: verter_language::JsModuleKind) -> SourceType {
    match kind {
        verter_language::JsModuleKind::Unambiguous => SourceType::unambiguous(),
        verter_language::JsModuleKind::Module => SourceType::mjs(),
        verter_language::JsModuleKind::CommonJs => SourceType::cjs(),
        verter_language::JsModuleKind::Script => SourceType::script(),
    }
}

/// Collect external `src=` block info from an already-parsed SFC — the
/// pure SFC-structure walk (no OXC work, no parse counting) shared by
/// the snapshot build and the lazy template-analysis computation, so
/// src-block derivation has exactly one implementation.
pub(crate) fn collect_vue_src_blocks(
    canonical_id: &str,
    source: &str,
    parsed: &ParsedSfc,
) -> (Vec<SrcBlockInfo>, Vec<ExternalSourceRequest>) {
    let mut src_blocks = Vec::new();
    let mut external_requests = Vec::new();

    for (idx, script) in [parsed.script(), parsed.script_setup()]
        .into_iter()
        .flatten()
        .enumerate()
    {
        if let Some(src_span) = script.src {
            let specifier = source[src_span.start as usize..src_span.end as usize].to_string();
            let resolved = resolve_external(canonical_id, &specifier);
            src_blocks.push(SrcBlockInfo {
                tag_name: "script".to_string(),
                resolved_canonical_id: resolved.clone(),
                tag_open_start: script.tag_open.start,
                tag_open_end: script.tag_open.end,
                tag_close_start: script.tag_close.as_ref().map(|c| c.start),
            });
            external_requests.push(ExternalSourceRequest {
                owner_canonical_id: canonical_id.to_string(),
                block_kind: ExternalBlockKind::Script,
                index: idx,
                specifier,
                resolved_canonical_id: resolved,
            });
        }
    }

    if let Some(ast) = parsed.template_ast() {
        let attrs = extract_attrs(&ast.root.attributes, source);
        try_resolve_src_block(
            canonical_id,
            &attrs,
            "template",
            ExternalBlockKind::Template,
            0,
            ast.root.tag_open.start,
            ast.root.tag_open.end,
            ast.root.tag_close.as_ref().map(|c| c.start),
            &mut external_requests,
            &mut src_blocks,
        );
    }

    for (idx, style) in parsed.style_nodes().iter().enumerate() {
        let mut attrs = extract_attrs(&style.attributes, source);
        if style.scoped {
            attrs.push(("scoped", "true"));
        }
        if style.module {
            attrs.push(("module", "true"));
        }
        try_resolve_src_block(
            canonical_id,
            &attrs,
            "style",
            ExternalBlockKind::Style,
            idx,
            style.tag_open.start,
            style.tag_open.end,
            style.tag_close.as_ref().map(|c| c.start),
            &mut external_requests,
            &mut src_blocks,
        );
    }

    for (idx, custom) in parsed.unknown_nodes().iter().enumerate() {
        let block_type =
            &source[custom.tag_open.start as usize + 1..custom.tag_open.name_end as usize];
        let mut attrs = extract_attrs(&custom.attributes, source);
        attrs.push(("type", block_type));
        try_resolve_src_block(
            canonical_id,
            &attrs,
            block_type,
            ExternalBlockKind::Custom,
            idx,
            custom.tag_open.start,
            custom.tag_open.end,
            custom.tag_close.as_ref().map(|c| c.start),
            &mut external_requests,
            &mut src_blocks,
        );
    }

    (src_blocks, external_requests)
}

pub(crate) fn build_vue_snapshot_from_parsed(
    canonical_id: &str,
    source: &str,
    analysis_scope: verter_semantic::analysis::AnalysisScope,
    parsed: &ParsedSfc,
    provenance: &crate::types::MetaProvenance,
    script_program: VueScriptProgram<'_>,
    script_owners: Option<&verter_semantic::analysis::TopLevelOwnerTable>,
) -> ParseSnapshot {
    let whole_hash = hash_16(source.as_bytes());

    // External `src=` inventory via the single shared pure walk.
    let (src_blocks, external_requests) = collect_vue_src_blocks(canonical_id, source, parsed);

    let mut script_hashes = Vec::new();
    let mut script_attrs_fp = Vec::new();
    let mut script_count = 0;
    let mut has_script = false;
    let mut script_lang: Option<String> = None;
    // Content span for script block (used for preprocessor request content extraction)
    let mut script_content_span: Option<(u32, u32)> = None;

    for script in [parsed.script(), parsed.script_setup()]
        .into_iter()
        .flatten()
    {
        script_count += 1;
        has_script = true;
        let content = if let Some(span) = script.content {
            &source.as_bytes()[span.start as usize..span.end as usize]
        } else {
            b""
        };
        script_hashes.push(hash_16(content));

        let mut attrs = extract_attrs(&script.attributes, source);
        // Capture script lang from the first script block that has one
        if script_lang.is_none() {
            if let Some(lang) = find_attr(&attrs, "lang") {
                if lang != "true" {
                    script_lang = Some(lang);
                    // Capture content span for the script with non-native lang
                    if let Some(span) = script.content {
                        script_content_span = Some((span.start, span.end));
                    }
                }
            }
        }
        if script.is_setup {
            attrs.push(("setup", "true"));
        }
        script_attrs_fp.push(normalize_attr_map(
            &attrs,
            &["setup", "lang", "src", "generic", "attrs"],
        ));
    }

    let script_hash = if script_hashes.is_empty() {
        None
    } else {
        let mut buf = Vec::with_capacity(script_hashes.len() * 16);
        for h in &script_hashes {
            buf.extend_from_slice(h);
        }
        Some(hash_16(&buf))
    };

    let mut template_count = 0;
    let mut has_template = false;
    let mut template_hash = None;
    let mut template_attrs_fp = Vec::new();
    let mut template_lang: Option<String> = None;
    // Content span for template block (used for preprocessor request content extraction)
    let mut template_content_span: Option<(u32, u32)> = None;

    if let Some(ast) = parsed.template_ast() {
        template_count = 1;
        has_template = true;
        if let Some(content) = ast.root.content.as_ref() {
            template_hash = Some(hash_16(
                &source.as_bytes()[content.start as usize..content.end as usize],
            ));
            template_content_span = Some((content.start, content.end));
        } else {
            template_hash = Some(hash_16(&[]));
        }

        let attrs = extract_attrs(&ast.root.attributes, source);
        // Capture template lang from lang attribute
        if let Some(lang) = find_attr(&attrs, "lang") {
            if lang != "true" && !lang.eq_ignore_ascii_case("html") {
                template_lang = Some(lang);
            }
        }
        template_attrs_fp.push(normalize_attr_map(&attrs, &["lang", "src"]));
    }

    let mut style_hashes = Vec::new();
    let mut style_attrs_fp = Vec::new();
    let mut style_langs = Vec::new();
    let mut has_scoped_style = false;
    // Content spans for style blocks (used for preprocessor request content extraction)
    let mut style_content_spans: Vec<Option<(u32, u32)>> = Vec::new();

    for style in parsed.style_nodes().iter() {
        let content = if let Some(span) = style.content {
            &source.as_bytes()[span.start as usize..span.end as usize]
        } else {
            b""
        };
        style_hashes.push(hash_16(content));

        let mut attrs = extract_attrs(&style.attributes, source);
        if style.scoped {
            has_scoped_style = true;
            attrs.push(("scoped", "true"));
        }
        if style.module {
            attrs.push(("module", "true"));
        }

        style_attrs_fp.push(normalize_attr_map(
            &attrs,
            &["scoped", "module", "lang", "src"],
        ));

        style_langs.push(find_attr(&attrs, "lang"));
        style_content_spans.push(style.content.map(|span| (span.start, span.end)));
    }

    let mut custom_hashes = Vec::new();
    let mut custom_attrs_fp = Vec::new();
    let mut custom_types = Vec::new();
    let mut custom_langs = Vec::new();
    // Content spans for custom blocks (used for preprocessor request content extraction)
    let mut custom_content_spans: Vec<Option<(u32, u32)>> = Vec::new();

    for custom in parsed.unknown_nodes().iter() {
        let content = if let Some(span) = custom.content {
            &source.as_bytes()[span.start as usize..span.end as usize]
        } else {
            b""
        };
        custom_hashes.push(hash_16(content));

        let block_type =
            &source[custom.tag_open.start as usize + 1..custom.tag_open.name_end as usize];
        custom_types.push(block_type.to_string());

        let mut attrs = extract_attrs(&custom.attributes, source);
        attrs.push(("type", block_type));

        custom_langs.push(find_attr(&attrs, "lang"));
        custom_content_spans.push(custom.content.map(|span| (span.start, span.end)));

        custom_attrs_fp.push(normalize_attr_map(&attrs, &["type", "lang", "src"]));
    }

    let descriptor = DescriptorMin {
        script_count,
        template_count,
        style_count: style_hashes.len(),
        custom_count: custom_hashes.len(),
        script_attr_fingerprints: script_attrs_fp,
        template_attr_fingerprints: template_attrs_fp,
        style_attr_fingerprints: style_attrs_fp,
        custom_attr_fingerprints: custom_attrs_fp,
        vapor: parsed.is_vapor(),
    };

    let slices = SliceHashes {
        script: script_hash,
        template: template_hash,
        styles: style_hashes,
        custom: custom_hashes,
    };

    let semantic_hash = semantic_hash(&slices, &descriptor);

    let raw_diags = parsed.clone_diagnostics();
    let parse_diagnostics = DiagnosticsSnapshot::from_vec(
        raw_diags
            .into_iter()
            .map(|d| HostDiagnostic {
                severity: match d.severity {
                    DiagnosticSeverity::Error => HostSeverity::Error,
                    DiagnosticSeverity::Warning => HostSeverity::Warning,
                    DiagnosticSeverity::Info => HostSeverity::Info,
                },
                code: format!("{:?}", d.code),
                message: d.message,
                span: d.span,
            })
            .collect(),
    );

    // Build style analyses for each style block (when style analysis flags are set)
    let style_analyses: Vec<verter_semantic::analysis::StyleBlockAnalysis> =
        if analysis_scope.needs_style_analysis() {
            build_style_analyses_from_parsed(parsed, source, canonical_id)
        } else {
            Vec::new()
        };

    // Vue SFCs are still modules: we need named export signatures from the
    // script content even when full script analysis is disabled so barrel
    // re-export resolution can find `export type Foo = ...` in `.vue` files.
    // Export signatures and (when the scope requests it) script analysis
    // are walks over ONE shared OXC script-program parse — never
    // per-consumer re-parses of the same script bytes. A flight-shared
    // eval program (the cold materialise lanes) is walked directly so
    // the whole flight pays exactly one script-program parse.
    let script_outputs = match script_program {
        VueScriptProgram::ParseHere => build_vue_script_outputs(
            parsed,
            source,
            /* needs_exports */ true,
            analysis_scope.needs_script_analysis(),
            provenance,
        ),
        VueScriptProgram::Shared(program) => {
            debug_assert_eq!(
                Some(program.source_str()),
                crate::host_resolve::extract_vue_script_content(source, Some(parsed)).as_deref(),
                "a shared script program must carry this SFC's \
                 position-preserving extracted script",
            );
            match script_owners {
                Some(owners) => vue_script_walks_from_program(
                    program.source_str(),
                    program.source_type(),
                    program.borrow_dependent(),
                    owners,
                    /* needs_exports */ true,
                    analysis_scope.needs_script_analysis(),
                    program.had_errors(),
                ),
                None => vue_script_walks_for_sfc(
                    program.source_str(),
                    program.source_type(),
                    program.borrow_dependent(),
                    parsed,
                    /* needs_exports */ true,
                    analysis_scope.needs_script_analysis(),
                    program.had_errors(),
                ),
            }
        }
        VueScriptProgram::SharedFatal => VueScriptOutputs {
            export_signatures: Vec::new(),
            script_analysis: analysis_scope
                .needs_script_analysis()
                .then(verter_semantic::analysis::ScriptAnalysisSnapshot::default),
            panic_diags: Vec::new(),
        },
    };
    let export_signatures = script_outputs.export_signatures;
    let mut script_analysis = script_outputs.script_analysis.unwrap_or_default();
    // Producer-side locator absolutization: fill the analyzer's empty-sentinel
    // macro-payload anchors with THIS snapshot's producing canonical before
    // the snapshot enters host-owned storage.
    absolutize_macro_payload_anchors(&mut script_analysis.macros, canonical_id);

    // Cross-reference: mark script bindings that are referenced by CSS
    // v-bind() in style blocks. Runs even with zero script bindings — the
    // recorded `style_vbind_roots` also carry PROP liveness (style v-bind
    // resolves props by bare name through the render context).
    if !style_analyses.is_empty() {
        script_analysis.mark_bindings_used_in_style(&style_analyses);
    }

    // Merge any panic diagnostics into parse diagnostics
    let parse_diagnostics = if script_outputs.panic_diags.is_empty() {
        parse_diagnostics
    } else {
        parse_diagnostics.merge(DiagnosticsSnapshot::from_vec(script_outputs.panic_diags))
    };

    // Build preprocessor requests for non-native languages
    let preprocessor_requests = build_preprocessor_requests(
        &template_lang,
        template_content_span,
        &script_lang,
        script_content_span,
        &style_langs,
        &style_content_spans,
        &custom_types,
        &custom_langs,
        &custom_content_spans,
        source,
    );

    ParseSnapshot {
        whole_hash,
        semantic_hash,
        slices,
        descriptor,
        meta: FileMeta {
            has_script,
            has_template,
            main_depends_on_styles: false,
            has_scoped_style,
            script_lang,
            template_lang,
            style_langs,
            custom_types,
            custom_langs,
        },
        external_requests,
        src_blocks,
        parse_diagnostics,
        script_analysis: Arc::new(script_analysis),
        export_signatures,
        style_analyses,
        markup_class_tokens: Vec::new(),
        preprocessor_requests,
    }
}

/// Build preprocessor requests for blocks that use non-native languages.
///
/// A non-native language is any `lang` that the Rust compiler cannot handle natively:
/// - Template: anything other than HTML (or no `lang`)
/// - Script: anything not in `[ts, tsx, js, jsx]`
/// - Style: anything other than CSS (or no `lang`)
/// - Custom: any custom block with a `lang` attribute
#[allow(clippy::too_many_arguments)]
fn build_preprocessor_requests(
    template_lang: &Option<String>,
    template_content_span: Option<(u32, u32)>,
    script_lang: &Option<String>,
    script_content_span: Option<(u32, u32)>,
    style_langs: &[Option<String>],
    style_content_spans: &[Option<(u32, u32)>],
    custom_types: &[String],
    custom_langs: &[Option<String>],
    custom_content_spans: &[Option<(u32, u32)>],
    source: &str,
) -> Vec<PreprocessorRequest> {
    let mut requests = Vec::new();

    // Template: non-native if template_lang is Some (already filtered for "html")
    if let Some(lang) = template_lang {
        let content = template_content_span
            .map(|(s, e)| &source[s as usize..e as usize])
            .unwrap_or("");
        requests.push(PreprocessorRequest {
            block_type: PreprocessorBlockType::Template,
            index: 0,
            lang: lang.clone(),
            content: content.to_string(),
        });
    }

    // Script: non-native if not in [ts, tsx, js, jsx]
    if let Some(lang) = script_lang {
        let is_native = matches!(
            lang.as_str(),
            "ts" | "tsx" | "js" | "jsx" | "TS" | "TSX" | "JS" | "JSX"
        );
        if !is_native {
            let content = script_content_span
                .map(|(s, e)| &source[s as usize..e as usize])
                .unwrap_or("");
            requests.push(PreprocessorRequest {
                block_type: PreprocessorBlockType::Script,
                index: 0,
                lang: lang.clone(),
                content: content.to_string(),
            });
        }
    }

    // Style: non-native if lang is Some and not "css"
    for (idx, lang_opt) in style_langs.iter().enumerate() {
        if let Some(lang) = lang_opt {
            if !lang.eq_ignore_ascii_case("css") {
                let content = style_content_spans
                    .get(idx)
                    .and_then(|s| *s)
                    .map(|(s, e)| &source[s as usize..e as usize])
                    .unwrap_or("");
                requests.push(PreprocessorRequest {
                    block_type: PreprocessorBlockType::Style,
                    index: idx,
                    lang: lang.clone(),
                    content: content.to_string(),
                });
            }
        }
    }

    // Custom: any custom block with a lang attribute
    for (idx, lang_opt) in custom_langs.iter().enumerate() {
        if let Some(lang) = lang_opt {
            let content = custom_content_spans
                .get(idx)
                .and_then(|s| *s)
                .map(|(s, e)| &source[s as usize..e as usize])
                .unwrap_or("");
            requests.push(PreprocessorRequest {
                block_type: PreprocessorBlockType::Custom,
                index: idx,
                lang: lang.clone(),
                content: content.to_string(),
            });
            // Also store custom block type name in context for the caller
            let _ = custom_types.get(idx); // suppress unused warning
        }
    }

    requests
}

/// Build a single style analysis from a parsed style node and the SFC source.
/// Shared by `parse_vue_snapshot()` (eager) and `build_style_analyses_from_source()` (on-demand).
fn build_single_style_analysis(
    style: &verter_compiler::parser::types::RootNodeStyle,
    source: &str,
    canonical_id: &str,
) -> verter_semantic::analysis::StyleBlockAnalysis {
    let module_name =
        find_attr(&extract_attrs(&style.attributes, source), "module").filter(|v| v != "true");
    let content_offset = style.content.map(|span| span.start).unwrap_or(0);

    // Extract CSS content from the SFC source
    let css_content = style
        .content
        .map(|span| &source[span.start as usize..span.end as usize])
        .unwrap_or("");

    // Run CSS prepass to extract v-bind() expressions and their generated variable names
    let component_name = verter_compiler::compile::extract_component_name(canonical_id);
    let scope_id = verter_compiler::compile::get_hash(&component_name);
    let prepass_result = verter_compiler::css::prepass::prepass(css_content, &scope_id);

    // Build VueStyleInput from prepass results. Each v-bind carries its
    // authored expression span (SFC-absolute) and the SOUND OXC-derived free
    // identifier roots — the single owning usage fact consumed by liveness
    // marking and compile-input assembly.
    let vue_input = verter_semantic::analysis::VueStyleInput {
        v_binds: prepass_result
            .v_bind_vars
            .iter()
            .map(|vb| {
                let roots =
                    verter_compiler::compile::style_usage::expression_free_roots(&vb.expression);
                verter_semantic::analysis::VBindInput {
                    expression: vb.expression.clone(),
                    quoted: false,
                    start: content_offset + vb.expr_start,
                    end: content_offset + vb.expr_end,
                    generated_var_name: Some(vb.var_name.clone()),
                    roots_complete: roots.is_some(),
                    expr_roots: roots.unwrap_or_default(),
                }
            })
            .collect(),
        special_pseudos: vec![],
    };

    let sfc_source_len = source.len() as u32;

    let analysis_lang = match style.lang {
        Some(verter_compiler::parser::types::StyleLang::Css) | None => {
            verter_semantic::analysis::StyleAnalysisLang::Css
        }
        Some(verter_compiler::parser::types::StyleLang::Scss) => {
            verter_semantic::analysis::StyleAnalysisLang::Scss
        }
        Some(verter_compiler::parser::types::StyleLang::Sass) => {
            verter_semantic::analysis::StyleAnalysisLang::Sass
        }
        Some(verter_compiler::parser::types::StyleLang::Less) => {
            verter_semantic::analysis::StyleAnalysisLang::Less
        }
        Some(verter_compiler::parser::types::StyleLang::Stylus) => {
            verter_semantic::analysis::StyleAnalysisLang::Stylus
        }
        Some(verter_compiler::parser::types::StyleLang::Unknown) => {
            verter_semantic::analysis::StyleAnalysisLang::Unknown
        }
    };
    // CSS, SCSS and Less run the brace-based scanner (dialect-aware) so class
    // and selector facts exist for every brace-based style block; indented
    // languages (Sass, Stylus) keep the Vue-features-only analysis.
    let analysis = verter_semantic::analysis::build_scanned_style_analysis(
        analysis_lang,
        css_content,
        vue_input,
        style.scoped,
        style.module,
        module_name.as_deref(),
        content_offset,
    );
    if let Some(css) = &analysis.css {
        css.debug_assert_valid_spans(sfc_source_len);
    }
    analysis
}

/// Run a closure with panic safety, returning a warning diagnostic if it panics.
fn catch_analysis_panic<T: Default>(
    label: &str,
    f: impl FnOnce() -> T + std::panic::UnwindSafe,
) -> (T, Option<HostDiagnostic>) {
    match std::panic::catch_unwind(f) {
        Ok(value) => (value, None),
        Err(payload) => {
            let msg = if let Some(s) = payload.downcast_ref::<&str>() {
                (*s).to_string()
            } else if let Some(s) = payload.downcast_ref::<String>() {
                s.clone()
            } else {
                "unknown panic".to_string()
            };
            let diagnostic = HostDiagnostic {
                severity: HostSeverity::Warning,
                code: "HOST_ANALYSIS_PANIC".to_string(),
                message: format!("{label}: {msg}"),
                span: None,
            };
            (T::default(), Some(diagnostic))
        }
    }
}

/// The SFC's OXC source type, resolved from `<script lang>` through the
/// Vue carrier producer's resolver (the one `<script lang>` authority —
/// the same data the producer stamps onto `ScriptRegion.source_type`).
fn vue_oxc_source_type(parsed: &ParsedSfc, source: &str) -> SourceType {
    oxc_source_type_from_neutral(
        verter_compiler::framework_common::vue_bridge::vue_script_source_type(parsed, source),
    )
}

/// Build script analysis from an already-parsed SFC.
///
/// Runs OXC analysis over the **position-preserving** script source
/// ([`crate::host_resolve::extract_vue_script_content`]) — script content at
/// its raw SFC byte offsets, non-script bytes whitespace-blanked — so every
/// span the analyzer produces (including each `AnalyzedMacro.parsed_type_argument`
/// internal `TypeExpr` span) is SFC-absolute by construction. No post-analysis
/// offset translation is required; downstream consumers use the spans directly
/// with `LineIndex::offset_to_position()`.
///
/// Shared by `parse_vue_snapshot()` (eager) and `build_script_analysis_from_parsed()`.
/// Combined `.vue` script-program outputs from a SINGLE OXC parse.
struct VueScriptOutputs {
    export_signatures: Vec<verter_semantic::analysis::ExportSignature>,
    /// `None` when the caller did not request script analysis.
    script_analysis: Option<verter_semantic::analysis::ScriptAnalysisSnapshot>,
    /// Panic diagnostics in production order: parse, export walk,
    /// analysis walk.
    panic_diags: Vec<HostDiagnostic>,
}

/// Where the `.vue` snapshot build gets its script program from.
///
/// A cold materialise flight already OXC-parses the position-preserving
/// extracted script ONCE as its eval program; the snapshot build walks
/// that SAME program instead of paying a second parse over the same
/// bytes. Lanes with no flight-shared program (the eager scheduler
/// worker, the lazy `get_analysis` re-builds) parse here — the single
/// counted snapshot-lane parse.
pub(crate) enum VueScriptProgram<'a> {
    /// No flight-shared program: extract and parse once here (counted
    /// on the `vue_script_snapshot_parses` provenance rail).
    ParseHere,
    /// The flight's eval program IS the script program (the eval
    /// source was the position-preserving extracted script): walk it,
    /// parse nothing.
    Shared(&'a crate::ParsedEvalProgram),
    /// The flight's single parse attempt over the extracted script was
    /// fatal (recovered panic). A re-parse over the same bytes under
    /// the same source type fails identically, so every script output
    /// defaults directly with zero additional parses — the `.vue`
    /// mirror of the non-SFC fatal arm.
    SharedFatal,
}

/// The panic-contained walks over ONE already-parsed `.vue` script
/// program: export signatures and (when requested) script analysis are
/// derived from the SHARED program — never per-consumer re-parses.
/// Each walk is caught independently (an export-walk panic still
/// yields script analysis, and vice versa), preserving the
/// per-consumer granularity the split builders had.
fn vue_script_walks_from_program(
    script_source: &str,
    source_type: SourceType,
    program: &Program<'_>,
    owners: &verter_semantic::analysis::TopLevelOwnerTable,
    needs_exports: bool,
    needs_script_analysis: bool,
    parse_errors: bool,
) -> VueScriptOutputs {
    let mut outputs = VueScriptOutputs {
        export_signatures: Vec::new(),
        script_analysis: needs_script_analysis
            .then(verter_semantic::analysis::ScriptAnalysisSnapshot::default),
        panic_diags: Vec::new(),
    };

    if needs_exports {
        let (export_signatures, export_panic_diag) = catch_analysis_panic(
            "export signature analysis",
            std::panic::AssertUnwindSafe(|| {
                verter_semantic::analysis::build_export_signatures_from_program(
                    script_source,
                    program,
                )
            }),
        );
        outputs.export_signatures = export_signatures;
        if let Some(diag) = export_panic_diag {
            outputs.panic_diags.push(diag);
        }
    }

    if needs_script_analysis {
        let (script_analysis, script_panic_diag) = catch_analysis_panic(
            "script analysis",
            std::panic::AssertUnwindSafe(|| {
                verter_semantic::analysis::build_script_analysis_with_scope_from_program_with_owners(
                    script_source,
                    source_type,
                    program,
                    verter_semantic::analysis::AnalysisScope::all(),
                    owners,
                    parse_errors,
                )
            }),
        );
        outputs.script_analysis = Some(script_analysis);
        if let Some(diag) = script_panic_diag {
            outputs.panic_diags.push(diag);
        }
    }

    outputs
}

fn vue_script_walks_for_sfc(
    script_source: &str,
    source_type: SourceType,
    program: &Program<'_>,
    parsed: &ParsedSfc,
    needs_exports: bool,
    needs_script_analysis: bool,
    parse_errors: bool,
) -> VueScriptOutputs {
    match vue_top_level_owner_table(program, parsed) {
        Ok(owners) => vue_script_walks_from_program(
            script_source,
            source_type,
            program,
            &owners,
            needs_exports,
            needs_script_analysis,
            parse_errors,
        ),
        Err(error) => VueScriptOutputs {
            export_signatures: Vec::new(),
            script_analysis: needs_script_analysis
                .then(verter_semantic::analysis::ScriptAnalysisSnapshot::default),
            panic_diags: vec![script_owner_index_diagnostic(&error)],
        },
    }
}

/// The single `.vue` script-program parse for the snapshot path.
///
/// Extracts the **position-preserving** script source
/// ([`crate::host_resolve::extract_vue_script_content`] — script content
/// at its raw SFC byte offsets, non-script bytes whitespace-blanked, so
/// every span the analyzer produces is SFC-absolute by construction),
/// OXC-parses it EXACTLY ONCE (counted on the
/// `MetaProvenance::vue_script_snapshot_parses` rail, inside the worker
/// fn so every lane counts), and derives every requested consumer from
/// the SHARED program via the `_from_program` walkers — the same
/// threading [`build_non_sfc_snapshot_from_program`] uses for non-SFC
/// files. Export signatures and script analysis are walks over the one
/// program, never per-consumer re-parses of the same script bytes.
///
/// Panic containment mirrors the per-consumer granularity the split
/// helpers had: each walk is caught independently (an export-walk panic
/// still yields script analysis, and vice versa); a panic in the parse
/// itself defaults every output with a single `script parse`
/// diagnostic. A recovered-fatal parse (`panicked` without unwinding)
/// defaults every output silently, matching the underlying builders.
fn build_vue_script_outputs(
    parsed: &ParsedSfc,
    source: &str,
    needs_exports: bool,
    needs_script_analysis: bool,
    provenance: &crate::types::MetaProvenance,
) -> VueScriptOutputs {
    let mut outputs = VueScriptOutputs {
        export_signatures: Vec::new(),
        script_analysis: needs_script_analysis
            .then(verter_semantic::analysis::ScriptAnalysisSnapshot::default),
        panic_diags: Vec::new(),
    };
    if !needs_exports && !needs_script_analysis {
        return outputs;
    }
    let Some(script_source) = crate::host_resolve::extract_vue_script_content(source, Some(parsed))
    else {
        return outputs;
    };
    let source_type = vue_oxc_source_type(parsed, source);
    provenance
        .vue_script_snapshot_parses
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let alloc = Allocator::new();
    let (parse_result, parse_panic_diag) = catch_analysis_panic(
        "script parse",
        std::panic::AssertUnwindSafe(|| {
            let parser =
                Parser::new(&alloc, &script_source, source_type).with_options(ParseOptions {
                    parse_regular_expression: false,
                    ..ParseOptions::default()
                });
            Some(parser.parse())
        }),
    );
    if let Some(diag) = parse_panic_diag {
        outputs.panic_diags.push(diag);
    }
    let Some(parse_result) = parse_result else {
        return outputs;
    };
    if parse_result.panicked {
        return outputs;
    }

    let mut walked = vue_script_walks_for_sfc(
        &script_source,
        source_type,
        &parse_result.program,
        parsed,
        needs_exports,
        needs_script_analysis,
        !parse_result.errors.is_empty(),
    );
    // Keep production diagnostic order: parse first, then the walks.
    let mut panic_diags = outputs.panic_diags;
    panic_diags.append(&mut walked.panic_diags);
    walked.panic_diags = panic_diags;
    walked
}

/// Compute script analysis on demand from SFC source. Used by get_analysis()
/// when eager_analysis was false during upsert().
///
/// Counted INSIDE the worker (via the `parse_carrier_counted` chokepoint) so
/// every caller's carrier parse lights up the `carrier_parses` / `sfc_parses`
/// rails.
pub(crate) fn build_script_analysis_from_source(
    source: &str,
    provenance: &crate::types::MetaProvenance,
) -> verter_semantic::analysis::ScriptAnalysisSnapshot {
    // On-demand Vue re-parse routes through the Vue carrier producer (the
    // counted chokepoint) so the artifact stays the one post-parse
    // representation.
    let artifact = build_vue_parse_artifact_from_source(source, provenance);
    build_script_analysis_for_artifact(Some(&artifact), source, provenance)
}

pub(crate) fn build_script_analysis_from_parsed(
    parsed: &ParsedSfc,
    source: &str,
    provenance: &crate::types::MetaProvenance,
) -> verter_semantic::analysis::ScriptAnalysisSnapshot {
    build_vue_script_outputs(
        parsed, source, /* needs_exports */ false, /* needs_script_analysis */ true,
        provenance,
    )
    .script_analysis
    .unwrap_or_default()
}

/// Compute style analyses on demand from SFC source. Used by get_analysis()
/// when eager_analysis was false during upsert().
///
/// Counted INSIDE the worker (via the `parse_carrier_counted` chokepoint) so
/// every caller's carrier parse lights up the `carrier_parses` / `sfc_parses`
/// rails.
pub(crate) fn build_style_analyses_from_source(
    source: &str,
    canonical_id: &str,
    provenance: &crate::types::MetaProvenance,
) -> Vec<verter_semantic::analysis::StyleBlockAnalysis> {
    // On-demand Vue re-parse routes through the Vue carrier producer (the
    // counted chokepoint) so the artifact stays the one post-parse
    // representation.
    let artifact = build_vue_parse_artifact_from_source(source, provenance);
    build_style_analyses_for_artifact(Some(&artifact), source, canonical_id, provenance)
}

pub(crate) fn build_style_analyses_from_parsed(
    parsed: &ParsedSfc,
    source: &str,
    canonical_id: &str,
) -> Vec<verter_semantic::analysis::StyleBlockAnalysis> {
    parsed
        .style_nodes()
        .iter()
        .map(|style| build_single_style_analysis(style, source, canonical_id))
        .collect()
}

/// Artifact-facing script-analysis builder: reuse the carrier parse
/// when the neutral artifact opens through the blessed Vue accessor,
/// else re-parse from source.
pub(crate) fn build_script_analysis_for_artifact(
    framework_parse: Option<&verter_language::FrameworkParseArtifact>,
    source: &str,
    provenance: &crate::types::MetaProvenance,
) -> verter_semantic::analysis::ScriptAnalysisSnapshot {
    match framework_parse.and_then(crate::typeinfo::adapters::vue::vue_parse) {
        Some(parsed) => build_script_analysis_from_parsed(parsed, source, provenance),
        None => build_script_analysis_from_source(source, provenance),
    }
}

/// Artifact-facing style-analysis builder (see
/// [`build_script_analysis_for_artifact`]).
pub(crate) fn build_style_analyses_for_artifact(
    framework_parse: Option<&verter_language::FrameworkParseArtifact>,
    source: &str,
    canonical_id: &str,
    provenance: &crate::types::MetaProvenance,
) -> Vec<verter_semantic::analysis::StyleBlockAnalysis> {
    match framework_parse.and_then(crate::typeinfo::adapters::vue::vue_parse) {
        Some(parsed) => build_style_analyses_from_parsed(parsed, source, canonical_id),
        None => build_style_analyses_from_source(source, canonical_id, provenance),
    }
}

/// The cold-flight variant of [`build_carrier_snapshot_from_artifact`]: builds a
/// non-Vue carrier snapshot reusing the flight's retained eval program (so the
/// cold build pays exactly one parse of the script bytes). The eval source is
/// re-derived from the artifact's recorded script regions — byte-identical to
/// the flight's `IndexedReady.eval_source` — and the script PROGRAM is threaded
/// in (`Shared` on a live parse, `SharedFatal` on a fatal one). Returns `None`
/// for a non-carrier / Vue artifact (the cold path routes Vue through
/// `build_vue_snapshot_from_parsed` directly).
pub(crate) fn build_carrier_snapshot_from_artifact_with_program(
    canonical_id: &str,
    source: &str,
    _analysis_scope: verter_semantic::analysis::AnalysisScope,
    framework_parse: &verter_language::FrameworkParseArtifact,
    provenance: &crate::types::MetaProvenance,
    script_program: FrameworkScriptProgram<'_>,
    script_owners: Option<&verter_semantic::analysis::TopLevelOwnerTable>,
) -> ParseSnapshot {
    let mut spans: Vec<(u32, u32)> = framework_parse
        .common
        .script_regions
        .iter()
        .map(|region| (region.span.start, region.span.end))
        .filter(|(s, e)| e > s)
        .collect();
    spans.sort_by_key(|(s, _)| *s);
    let eval_source = crate::host_resolve::build_position_preserving_script_source(source, &spans);
    build_svelte_snapshot_from_eval_source(
        canonical_id,
        source,
        &eval_source,
        framework_parse,
        provenance,
        script_program,
        script_owners,
    )
}

/// Whether a file's resolved carrier row has a registered carrier compiler that
/// can extract template data — the REGISTRY-DISPATCHED ingestion gate that
/// replaces the hardcoded `.vue` / `is_vue()` check. A plain script (no carrier
/// row) or a carrier whose adapter has no registered compiler answers `false`.
#[must_use]
pub(crate) fn file_language_has_template_data_compiler(
    file_language: &verter_language::FileLanguage,
) -> bool {
    let Some(adapter_id) = file_language.adapter_id() else {
        return false;
    };
    let Some(carrier_language_id) = file_language.carrier_language_id() else {
        return false;
    };
    carrier_compiler_registry()
        .compiler_for_carrier_language(adapter_id, carrier_language_id)
        .is_some()
}

/// The carrier-NEUTRAL template-data extraction half shared by
/// `build_template_analysis` / `compute_template_analysis_if_missing`.
///
/// This is the SINGLE registry-dispatched template-data path: it interns the
/// file's resolved carrier row and dispatches the extraction through that
/// carrier's [`CarrierCompiler::template_data`](verter_compiler::framework_common::CarrierCompiler::template_data)
/// (Vue's bridge runs the META-target `compile_from_parsed` for
/// `referenced_bindings` / constness; Svelte's walks the typed template tree).
/// There is no Vue-only branch here.
///
/// When `reuse_carrier_parse` is set and `framework_parse` matches the file's
/// carrier, the cached artifact's parse is reused; otherwise a fresh artifact is
/// parsed from `compile_source` through the same registry (the external-src
/// merge case, where the compile source differs from the file content). Returns
/// `None` for a non-carrier file or a carrier row with no registered compiler.
pub(crate) fn compile_template_data(
    file_language: &verter_language::FileLanguage,
    compile_source: &str,
    framework_parse: Option<&verter_language::FrameworkParseArtifact>,
    reuse_carrier_parse: bool,
    provenance: &crate::types::MetaProvenance,
) -> Option<verter_compiler::compile::RawTemplateData> {
    let adapter_id = file_language.adapter_id()?;
    let carrier_language_id = file_language.carrier_language_id()?;
    let compiler = carrier_compiler_registry()
        .compiler_for_carrier_language(adapter_id, carrier_language_id)?;

    // Reuse the cached artifact only when the caller permits it AND the artifact
    // belongs to THIS carrier (adapter id + carrier language id match) — a
    // foreign / stale artifact forces a fresh parse rather than a misrouted
    // dispatch.
    let reuse = reuse_carrier_parse
        && framework_parse.is_some_and(|artifact| {
            artifact.adapter_id == *adapter_id && artifact.language_id == *carrier_language_id
        });

    let fresh_artifact = if reuse {
        None
    } else {
        Some(parse_carrier_counted(
            provenance,
            compiler.as_ref(),
            compile_source,
            &verter_compiler::framework_common::ParseOptions::default(),
        ))
    };
    let artifact = if reuse {
        framework_parse.expect("reuse implies a present artifact")
    } else {
        fresh_artifact
            .as_ref()
            .expect("a fresh artifact is built when the cached one is not reused")
            .as_ref()
    };

    Some(compiler.template_data(compile_source, artifact).data)
}

pub(crate) fn build_non_sfc_snapshot_from_program(
    canonical_id: &str,
    source: &str,
    source_type: SourceType,
    program: &Program<'_>,
    parse_errors: bool,
) -> ParseSnapshot {
    let owners = verter_semantic::analysis::TopLevelOwnerTable::ordinary_file(program.body.len());
    build_snapshot_from_program_with_owners(
        canonical_id,
        source,
        source_type,
        program,
        &owners,
        parse_errors,
    )
}

fn build_snapshot_from_program_with_owners(
    canonical_id: &str,
    source: &str,
    source_type: SourceType,
    program: &Program<'_>,
    owners: &verter_semantic::analysis::TopLevelOwnerTable,
    parse_errors: bool,
) -> ParseSnapshot {
    let whole_hash = hash_16(source.as_bytes());
    let slices = SliceHashes::default();
    let descriptor = DescriptorMin::default();
    let semantic_hash = whole_hash;

    let export_signatures =
        verter_semantic::analysis::build_export_signatures_from_program(source, program);
    let mut script_analysis =
        verter_semantic::analysis::build_script_analysis_with_scope_from_program_with_owners(
            source,
            source_type,
            program,
            verter_semantic::analysis::AnalysisScope::IMPORTS
                | verter_semantic::analysis::AnalysisScope::BINDINGS
                | verter_semantic::analysis::AnalysisScope::FUNC_RETURNS
                | verter_semantic::analysis::AnalysisScope::REACTIVITY
                | verter_semantic::analysis::AnalysisScope::MACROS
                | verter_semantic::analysis::AnalysisScope::MACRO_TYPE_DEPS
                | verter_semantic::analysis::AnalysisScope::VUE_API_USAGE
                | verter_semantic::analysis::AnalysisScope::EXPORT_SIGNATURES
                | verter_semantic::analysis::AnalysisScope::SCRIPT_USAGES,
            owners,
            parse_errors,
        );
    // Producer-side locator absolutization: fill the analyzer's empty-sentinel
    // macro-payload anchors with THIS snapshot's producing canonical before
    // the snapshot enters host-owned storage. Covers the non-SFC script lane
    // AND every carrier snapshot built through this walk (Svelte).
    absolutize_macro_payload_anchors(&mut script_analysis.macros, canonical_id);

    ParseSnapshot {
        whole_hash,
        semantic_hash,
        slices,
        descriptor,
        meta: FileMeta::default(),
        external_requests: Vec::new(),
        src_blocks: Vec::new(),
        parse_diagnostics: DiagnosticsSnapshot::default(),
        script_analysis: Arc::new(script_analysis),
        export_signatures,
        style_analyses: Vec::new(),
        markup_class_tokens: Vec::new(),
        preprocessor_requests: Vec::new(),
    }
}

pub(crate) fn parse_non_sfc_snapshot(
    canonical_id: &str,
    source: &str,
    file_language: &verter_language::FileLanguage,
    provenance: &crate::types::MetaProvenance,
) -> ParseSnapshot {
    // A full OXC program parse outside the `parse_eval_program` funnel —
    // counted on its own provenance rail (it is not a carrier parse and
    // not an eval-program parse) so every full-program snapshot lane
    // (scheduler Source stage, analysis read path) is dedup-suite-visible.
    // The dialect comes from the language registry (the SOLE plain-script
    // dialect authority), never a path-extension re-sniff.
    provenance
        .non_sfc_snapshot_parses
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let source_type = plain_script_source_type(file_language);
    let alloc = Allocator::new();
    let parser = Parser::new(&alloc, source, source_type).with_options(ParseOptions {
        parse_regular_expression: false,
        ..ParseOptions::default()
    });
    let result = parser.parse();
    if result.panicked {
        return ParseSnapshot {
            whole_hash: hash_16(source.as_bytes()),
            semantic_hash: hash_16(source.as_bytes()),
            slices: SliceHashes::default(),
            descriptor: DescriptorMin::default(),
            meta: FileMeta::default(),
            external_requests: Vec::new(),
            src_blocks: Vec::new(),
            parse_diagnostics: DiagnosticsSnapshot::default(),
            script_analysis: Arc::new(verter_semantic::analysis::ScriptAnalysisSnapshot::default()),
            export_signatures: Vec::new(),
            style_analyses: Vec::new(),
            markup_class_tokens: Vec::new(),
            preprocessor_requests: Vec::new(),
        };
    }

    build_non_sfc_snapshot_from_program(
        canonical_id,
        source,
        source_type,
        &result.program,
        !result.errors.is_empty(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The statically classified registry row for a path — tests
    /// thread it exactly like production call sites thread the
    /// host-resolved row.
    fn classified(id: &str) -> verter_language::FileLanguage {
        verter_language::LanguageRegistry::global()
            .classify_static(id)
            .static_resolution()
    }

    fn fragment_span(source: &str, fragment: &str) -> verter_span::Span {
        let start = source.find(fragment).expect("fixture fragment exists") as u32;
        verter_span::Span::new(start, start + fragment.len() as u32)
    }

    #[test]
    fn carrier_owner_table_preserves_kind_and_per_kind_region_ordinals() {
        use verter_language::ScriptRegionKind::{Frontmatter, Instance, Module};

        let source = "const moduleValue = 0;\nconst instanceZero = 0;\nconst frontmatter = 0;\nconst instanceOne = 1;";
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, source, SourceType::ts()).parse();
        assert!(!parsed.panicked);
        let table = top_level_owner_table_from_region_spans(
            &parsed.program,
            &[
                (fragment_span(source, "const moduleValue = 0;"), Module),
                (fragment_span(source, "const instanceZero = 0;"), Instance),
                (fragment_span(source, "const frontmatter = 0;"), Frontmatter),
                (fragment_span(source, "const instanceOne = 1;"), Instance),
            ],
        )
        .expect("exact carrier regions form a valid owner table");

        assert_eq!(
            table
                .statements()
                .iter()
                .map(|statement| statement.owner)
                .collect::<Vec<_>>(),
            vec![
                verter_type_expr::TopLevelOwnerId::module(0),
                verter_type_expr::TopLevelOwnerId::instance(0),
                verter_type_expr::TopLevelOwnerId::frontmatter(0),
                verter_type_expr::TopLevelOwnerId::instance(1),
            ]
        );
        assert_eq!(table.regions().len(), 4, "comment ownership needs regions");
    }

    #[test]
    fn carrier_owner_table_rejects_unowned_real_statement() {
        let source = "const owned = 0;\nconst escaped = 1;";
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, source, SourceType::ts()).parse();
        assert!(!parsed.panicked);

        assert!(matches!(
            top_level_owner_table_from_region_spans(
                &parsed.program,
                &[(
                    fragment_span(source, "const owned = 0;"),
                    verter_language::ScriptRegionKind::Module,
                )],
            ),
            Err(ScriptOwnerIndexError::UnownedStatement {
                statement_index: 1,
                ..
            })
        ));
    }

    #[test]
    fn carrier_owner_table_rejects_overlapping_regions_before_assignment() {
        let source = "const value = 0;";
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, source, SourceType::ts()).parse();
        assert!(!parsed.panicked);

        assert!(matches!(
            top_level_owner_table_from_region_spans(
                &parsed.program,
                &[
                    (
                        verter_span::Span::new(0, source.len() as u32),
                        verter_language::ScriptRegionKind::Module,
                    ),
                    (
                        verter_span::Span::new(1, source.len() as u32),
                        verter_language::ScriptRegionKind::Instance,
                    ),
                ],
            ),
            Err(ScriptOwnerIndexError::OverlappingRegions { .. })
        ));
    }

    /// Runtime half of the `plain_script_dialect_from_file_language`
    /// guard: the neutral-dialect → OXC `SourceType` mapping, pinned
    /// EXHAUSTIVELY over the `ScriptSourceType` vocabulary (every
    /// variant × every `JsModuleKind`).
    #[test]
    fn neutral_dialect_to_oxc_source_type_parity_matrix_is_exhaustive() {
        use verter_language::{JsModuleKind, ScriptSourceType};

        let matrix: &[(ScriptSourceType, SourceType)] = &[
            (ScriptSourceType::Ts, SourceType::ts()),
            (ScriptSourceType::Tsx, SourceType::tsx()),
            (ScriptSourceType::Dts, SourceType::d_ts()),
            (
                ScriptSourceType::Js(JsModuleKind::Unambiguous),
                SourceType::unambiguous(),
            ),
            (
                ScriptSourceType::Js(JsModuleKind::Module),
                SourceType::mjs(),
            ),
            (
                ScriptSourceType::Js(JsModuleKind::CommonJs),
                SourceType::cjs(),
            ),
            (
                ScriptSourceType::Js(JsModuleKind::Script),
                SourceType::script(),
            ),
            (
                ScriptSourceType::Jsx(JsModuleKind::Unambiguous),
                SourceType::unambiguous().with_jsx(true),
            ),
            (
                ScriptSourceType::Jsx(JsModuleKind::Module),
                SourceType::jsx(),
            ),
            (
                ScriptSourceType::Jsx(JsModuleKind::CommonJs),
                SourceType::cjs().with_jsx(true),
            ),
            (
                ScriptSourceType::Jsx(JsModuleKind::Script),
                SourceType::script().with_jsx(true),
            ),
        ];
        for (neutral, expected) in matrix {
            assert_eq!(
                oxc_source_type_from_neutral(*neutral),
                *expected,
                "dialect parity drifted for {neutral:?}"
            );
        }

        // Exhaustiveness pin: every `ScriptSourceType` discriminant and
        // every `JsModuleKind` appears in the matrix above. A new
        // variant fails this match (and the compiler walks the author
        // back here to extend the matrix).
        let covered = |st: &ScriptSourceType| match st {
            ScriptSourceType::Ts
            | ScriptSourceType::Tsx
            | ScriptSourceType::Dts
            | ScriptSourceType::Js(
                JsModuleKind::Unambiguous
                | JsModuleKind::Module
                | JsModuleKind::CommonJs
                | JsModuleKind::Script,
            )
            | ScriptSourceType::Jsx(
                JsModuleKind::Unambiguous
                | JsModuleKind::Module
                | JsModuleKind::CommonJs
                | JsModuleKind::Script,
            ) => true,
        };
        assert!(matrix.iter().all(|(neutral, _)| covered(neutral)));
        assert_eq!(matrix.len(), 11, "matrix must cover the full vocabulary");
    }

    /// Plain-script source types derive from the classified row — the
    /// registry rows map straight onto their parse dialects, and
    /// non-script rows (carriers) fall back to TypeScript.
    #[test]
    fn plain_script_source_type_derives_from_the_classified_row() {
        let cases: &[(&str, SourceType)] = &[
            ("/x/a.ts", SourceType::ts()),
            ("/x/a.mts", SourceType::ts()),
            ("/x/a.cts", SourceType::ts()),
            ("/x/a.tsx", SourceType::tsx()),
            ("/x/a.js", SourceType::unambiguous()),
            ("/x/a.mjs", SourceType::mjs()),
            ("/x/a.cjs", SourceType::cjs()),
            ("/x/a.jsx", SourceType::unambiguous().with_jsx(true)),
            ("/x/a.d.ts", SourceType::d_ts()),
            ("/x/a.d.mts", SourceType::d_ts()),
            ("/x/a.d.cts", SourceType::d_ts()),
            // Unknown extensions route as TypeScript scripts.
            ("/x/a.weird", SourceType::ts()),
        ];
        for (id, expected) in cases {
            assert_eq!(
                plain_script_source_type(&classified(id)),
                *expected,
                "plain-script dialect drifted for {id}"
            );
        }
        // A carrier row reaching the plain-script derivation falls
        // back to TypeScript (no script dialect on the row).
        assert_eq!(
            plain_script_source_type(&verter_language::FileLanguage::vue()),
            SourceType::ts()
        );
    }

    #[test]
    fn combined_framework_dialect_promotes_mixed_javascript_and_typescript_regions() {
        use verter_language::{JsModuleKind, ScriptRegion, ScriptRegionKind, ScriptSourceType};

        let region = |source_type, kind| ScriptRegion {
            span: verter_span::Span::new(0, 1),
            source_type,
            kind,
        };
        let mixed = [
            region(
                ScriptSourceType::Js(JsModuleKind::Module),
                ScriptRegionKind::Module,
            ),
            region(ScriptSourceType::Ts, ScriptRegionKind::Instance),
        ];
        assert_eq!(
            combined_framework_script_source_type(&mixed),
            Some(ScriptSourceType::Ts),
            "a later TypeScript instance script must promote the combined program"
        );

        let mixed_jsx = [
            region(ScriptSourceType::Ts, ScriptRegionKind::Module),
            region(
                ScriptSourceType::Jsx(JsModuleKind::Module),
                ScriptRegionKind::Instance,
            ),
        ];
        assert_eq!(
            combined_framework_script_source_type(&mixed_jsx),
            Some(ScriptSourceType::Tsx),
            "mixed TypeScript and JSX requires the TSX grammar"
        );

        let single_js = [region(
            ScriptSourceType::Js(JsModuleKind::Script),
            ScriptRegionKind::Instance,
        )];
        assert_eq!(
            combined_framework_script_source_type(&single_js),
            Some(ScriptSourceType::Js(JsModuleKind::Script)),
            "a single region preserves its producer-owned module kind"
        );
    }
    use smallvec::SmallVec;
    use verter_compiler::types::NodeProp;
    use verter_semantic::analysis::AnalysisScope;

    // Test-local wrappers fixing the worker fns' provenance param (the
    // workers count parses on the host's provenance rail; these tests
    // exercise parsing only, so a scratch counter suffices). Shadow the
    // glob-imported names so every call below stays unchanged.
    fn parse_vue_snapshot(
        canonical_id: &str,
        source: &str,
        analysis_scope: AnalysisScope,
    ) -> (ParseSnapshot, Arc<ParsedSfc>) {
        let (snapshot, artifact) = super::parse_vue_snapshot(
            canonical_id,
            source,
            analysis_scope,
            &crate::types::MetaProvenance::default(),
        );
        let parsed = crate::typeinfo::adapters::vue::vue_parse(&artifact)
            .expect("a Vue carrier artifact carries a ParsedSfc")
            .clone();
        (snapshot, parsed)
    }
    fn parse_non_sfc_snapshot(
        canonical_id: &str,
        source: &str,
        file_language: &verter_language::FileLanguage,
    ) -> ParseSnapshot {
        super::parse_non_sfc_snapshot(
            canonical_id,
            source,
            file_language,
            &crate::types::MetaProvenance::default(),
        )
    }

    // ── Helper: build a NodeProp pointing into a source string ──

    fn make_prop(
        start: u32,
        name_end: u32,
        value_start: Option<u32>,
        value_end: Option<u32>,
    ) -> NodeProp {
        NodeProp {
            start,
            name_end,
            is_directive: false,
            arg_start: None,
            arg_end: None,
            value_start,
            value_end,
            modifiers: SmallVec::new(),
            is_dynamic: None,
        }
    }

    // ═══════════════════════════════════════════════════════════
    // extract_attrs
    // ═══════════════════════════════════════════════════════════

    /// @ai-generated - Tests extract_attrs preserves original case (zero-copy)
    #[test]
    fn extract_attrs_preserves_case() {
        //              0123456789
        // Lang="ts"
        // 0=L 1=a 2=n 3=g 4== 5=" 6=t 7=s 8="
        let source = "Lang=\"ts\"";
        let props = vec![make_prop(0, 4, Some(6), Some(8))];
        let attrs = extract_attrs(&props, source);
        assert_eq!(attrs, vec![("Lang", "ts")]);
    }

    /// @ai-generated - Tests extract_attrs extracts attribute value correctly
    #[test]
    fn extract_attrs_extracts_value() {
        // src="./foo.html"
        // 0=s 1=r 2=c 3== 4=" 5=. 6=/ ... 14=l 15="
        let source = "src=\"./foo.html\"";
        let props = vec![make_prop(0, 3, Some(5), Some(15))];
        let attrs = extract_attrs(&props, source);
        assert_eq!(attrs[0].1, "./foo.html");
    }

    /// @ai-generated - Tests extract_attrs with no value (boolean attribute)
    #[test]
    fn extract_attrs_no_value_is_empty_string() {
        let source = "scoped";
        let props = vec![make_prop(0, 6, None, None)];
        let attrs = extract_attrs(&props, source);
        assert_eq!(attrs, vec![("scoped", "")]);
    }

    /// @ai-generated - Tests extract_attrs with multiple attributes
    #[test]
    fn extract_attrs_multiple_props() {
        // lang="ts" setup src
        // 0=l 1=a 2=n 3=g 4== 5=" 6=t 7=s 8=" 9=  10=s 11=e 12=t 13=u 14=p 15=  16=s 17=r 18=c
        let source = "lang=\"ts\" setup src";
        let props = vec![
            make_prop(0, 4, Some(6), Some(8)), // lang="ts"
            make_prop(10, 15, None, None),     // setup
            make_prop(16, 19, None, None),     // src
        ];
        let attrs = extract_attrs(&props, source);
        assert_eq!(attrs.len(), 3);
        assert_eq!(attrs[0], ("lang", "ts"));
        assert_eq!(attrs[1], ("setup", ""));
        assert_eq!(attrs[2], ("src", ""));
    }

    // ═══════════════════════════════════════════════════════════
    // normalize_attr_map
    // ═══════════════════════════════════════════════════════════

    /// @ai-generated - Tests normalize_attr_map filters to included keys only
    #[test]
    fn normalize_attr_map_include_filter() {
        let attrs = vec![("lang", "ts"), ("setup", ""), ("id", "foo")];
        let result = normalize_attr_map(&attrs, &["lang", "setup"]);
        assert!(result.contains("lang=ts"));
        assert!(result.contains("setup=true"));
        assert!(!result.contains("id"));
    }

    /// @ai-generated - Tests normalize_attr_map treats empty value as "true"
    #[test]
    fn normalize_attr_map_empty_value_becomes_true() {
        let attrs = vec![("scoped", "")];
        let result = normalize_attr_map(&attrs, &["scoped"]);
        assert!(result.contains("scoped=true"));
    }

    /// @ai-generated - Tests normalize_attr_map uses BTreeMap sort order
    #[test]
    fn normalize_attr_map_sorted_by_key() {
        let attrs = vec![("src", "x.ts"), ("lang", "ts")];
        let result = normalize_attr_map(&attrs, &["src", "lang"]);
        let lang_pos = result.find("lang").unwrap();
        let src_pos = result.find("src").unwrap();
        assert!(lang_pos < src_pos, "keys should be sorted alphabetically");
    }

    /// @ai-generated - Tests normalize_attr_map with no matching keys
    #[test]
    fn normalize_attr_map_no_matches_empty_string() {
        let attrs = vec![("id", "foo")];
        let result = normalize_attr_map(&attrs, &["lang", "setup"]);
        assert!(result.is_empty());
    }

    /// @ai-generated - normalize_attr_map uses newline separators, not literal \n
    #[test]
    fn normalize_attr_map_uses_newline_separator() {
        let attrs = vec![("lang", "ts"), ("scoped", "")];
        let result = normalize_attr_map(&attrs, &["lang", "scoped"]);
        // Each entry should be separated by a real newline character
        assert!(
            result.contains('\n'),
            "fingerprint should contain newline chars, got: {:?}",
            result
        );
        assert_eq!(result, "lang=ts\nscoped=true\n");
    }

    // ═══════════════════════════════════════════════════════════
    // find_attr
    // ═══════════════════════════════════════════════════════════

    /// @ai-generated - Tests find_attr is case-insensitive
    #[test]
    fn find_attr_case_insensitive() {
        let attrs = vec![("Lang", "ts")];
        assert_eq!(find_attr(&attrs, "LANG"), Some("ts".to_string()));
        assert_eq!(find_attr(&attrs, "lang"), Some("ts".to_string()));
    }

    /// @ai-generated - Tests find_attr returns None for missing attribute
    #[test]
    fn find_attr_missing_returns_none() {
        let attrs = vec![("lang", "ts")];
        assert_eq!(find_attr(&attrs, "src"), None);
    }

    /// @ai-generated - Tests find_attr empty value returns "true"
    #[test]
    fn find_attr_empty_value_returns_true() {
        let attrs = vec![("scoped", "")];
        assert_eq!(find_attr(&attrs, "scoped"), Some("true".to_string()));
    }

    /// @ai-generated - Tests find_attr returns first match
    #[test]
    fn find_attr_returns_first_match() {
        let attrs = vec![("lang", "ts"), ("lang", "jsx")];
        assert_eq!(find_attr(&attrs, "lang"), Some("ts".to_string()));
    }

    // ═══════════════════════════════════════════════════════════
    // parse_vue_snapshot
    // ═══════════════════════════════════════════════════════════

    /// @ai-generated - Script setup only: has_script=true, has_template=false
    #[test]
    fn parse_vue_snapshot_script_setup_only() {
        let (snap, _parsed) = parse_vue_snapshot(
            "Comp.vue",
            "<script setup>const n = 1</script>",
            AnalysisScope::LSP,
        );
        assert!(snap.meta.has_script);
        assert!(!snap.meta.has_template);
        assert!(snap.slices.script.is_some());
        assert!(snap.slices.template.is_none());
        assert_eq!(snap.descriptor.script_count, 1);
        assert_eq!(snap.descriptor.template_count, 0);
    }

    /// @ai-generated - Template only: has_template=true, has_script=false
    #[test]
    fn parse_vue_snapshot_template_only() {
        let (snap, _parsed) = parse_vue_snapshot(
            "Comp.vue",
            "<template><div>hello</div></template>",
            AnalysisScope::LSP,
        );
        assert!(snap.meta.has_template);
        assert!(!snap.meta.has_script);
        assert!(snap.slices.template.is_some());
        assert!(snap.slices.script.is_none());
        assert_eq!(snap.descriptor.template_count, 1);
        assert_eq!(snap.descriptor.script_count, 0);
    }

    /// @ai-generated - Full SFC: all blocks present
    #[test]
    fn parse_vue_snapshot_full_sfc() {
        let (snap, _parsed) = parse_vue_snapshot(
            "Comp.vue",
            "<script setup>const n = 1</script>\n<template><div>{{n}}</div></template>\n<style>.a{color:red}</style>",
            AnalysisScope::LSP,
        );
        assert!(snap.meta.has_script);
        assert!(snap.meta.has_template);
        assert!(snap.slices.script.is_some());
        assert!(snap.slices.template.is_some());
        assert_eq!(snap.slices.styles.len(), 1);
        assert_eq!(snap.descriptor.script_count, 1);
        assert_eq!(snap.descriptor.template_count, 1);
        assert_eq!(snap.descriptor.style_count, 1);
    }

    #[test]
    fn parse_vue_snapshot_collects_named_export_signatures_from_script() {
        let source = r#"<script lang="ts">
export interface Props {
  label: string
}

export type Keys = 'label'
</script>
<template><div /></template>"#;

        let (snap, _parsed) = parse_vue_snapshot("Comp.vue", source, AnalysisScope::NONE);
        let names: Vec<&str> = snap
            .export_signatures
            .iter()
            .map(|sig| sig.name.as_str())
            .collect();

        assert!(
            names.contains(&"Props"),
            "Vue SFC export signatures should include named type exports, got: {names:?}"
        );
        assert!(
            names.contains(&"Keys"),
            "Vue SFC export signatures should include named aliases, got: {names:?}"
        );

        let props_sig = snap
            .export_signatures
            .iter()
            .find(|sig| sig.name == "Props")
            .expect("Props export signature should exist");
        let expected_start = source
            .find("Props")
            .expect("Props identifier should exist in source") as u32;
        assert_eq!(
            props_sig.span.start, expected_start,
            "export signature span should be remapped to SFC-absolute offsets"
        );
    }

    #[test]
    fn parse_vue_snapshot_uses_script_lang_for_script_analysis() {
        let source = r#"<script setup lang="tsx">
const view = <div className="card">hello</div>
</script>
<template><div /></template>"#;

        let (snap, _parsed) = parse_vue_snapshot("Comp.vue", source, AnalysisScope::LSP);
        assert!(
            snap.script_analysis
                .bindings
                .iter()
                .any(|binding| binding.name == "view"),
            "TSX script analysis should respect the SFC script lang and retain bindings"
        );
    }

    /// @ai-generated - Multiple styles: correct count and langs
    #[test]
    fn parse_vue_snapshot_multiple_styles() {
        let (snap, _parsed) = parse_vue_snapshot(
            "Comp.vue",
            "<template><div/></template><style>.a{}</style><style lang=\"scss\">.b{}</style>",
            AnalysisScope::LSP,
        );
        assert_eq!(snap.slices.styles.len(), 2);
        assert_eq!(snap.descriptor.style_count, 2);
        assert_eq!(snap.meta.style_langs.len(), 2);
    }

    /// @ai-generated - Custom block detection
    #[test]
    fn parse_vue_snapshot_custom_block() {
        let (snap, _parsed) = parse_vue_snapshot(
            "Comp.vue",
            "<template><div/></template><i18n>{\"en\":{\"hi\":\"hello\"}}</i18n>",
            AnalysisScope::LSP,
        );
        assert_eq!(snap.descriptor.custom_count, 1);
        assert_eq!(snap.meta.custom_types, vec!["i18n"]);
        assert_eq!(snap.slices.custom.len(), 1);
    }

    /// @ai-generated - Empty string doesn't panic, all counts zero
    #[test]
    fn parse_vue_snapshot_empty_sfc() {
        let (snap, _parsed) = parse_vue_snapshot("Comp.vue", "", AnalysisScope::LSP);
        assert!(!snap.meta.has_script);
        assert!(!snap.meta.has_template);
        assert_eq!(snap.descriptor.script_count, 0);
        assert_eq!(snap.descriptor.template_count, 0);
        assert_eq!(snap.descriptor.style_count, 0);
        assert_eq!(snap.descriptor.custom_count, 0);
    }

    /// @ai-generated - Script with src produces external_requests
    #[test]
    fn parse_vue_snapshot_script_with_src() {
        let (snap, _parsed) = parse_vue_snapshot(
            "/src/Comp.vue",
            "<script setup src=\"./script.ts\"></script><template><div/></template>",
            AnalysisScope::LSP,
        );
        assert!(!snap.external_requests.is_empty());
        assert!(!snap.src_blocks.is_empty());
        assert_eq!(snap.src_blocks[0].tag_name, "script");
    }

    /// @ai-generated - Scoped style fingerprint contains scoped info
    #[test]
    fn parse_vue_snapshot_scoped_style_fingerprint() {
        let (snap, _parsed) = parse_vue_snapshot(
            "Comp.vue",
            "<template><div/></template><style scoped>.a{}</style>",
            AnalysisScope::LSP,
        );
        assert_eq!(snap.descriptor.style_count, 1);
        let fp = &snap.descriptor.style_attr_fingerprints[0];
        assert!(fp.contains("scoped=true"), "fingerprint: {}", fp);
    }

    /// @ai-generated - Style lang is detected
    #[test]
    fn parse_vue_snapshot_style_lang() {
        let (snap, _parsed) = parse_vue_snapshot(
            "Comp.vue",
            "<template><div/></template><style lang=\"scss\">.a{}</style>",
            AnalysisScope::LSP,
        );
        assert_eq!(snap.meta.style_langs[0], Some("scss".to_string()));
    }

    /// @ai-generated - script_lang is extracted from <script lang="ts">
    #[test]
    fn parse_vue_snapshot_script_lang_ts() {
        let (snap, _parsed) = parse_vue_snapshot(
            "Comp.vue",
            "<script setup lang=\"ts\">const n = 1</script><template><div/></template>",
            AnalysisScope::LSP,
        );
        assert_eq!(snap.meta.script_lang, Some("ts".to_string()));
    }

    /// @ai-generated - script_lang extracted from multiline SFC with export type
    #[test]
    fn parse_vue_snapshot_script_lang_ts_multiline() {
        let (snap, _parsed) = parse_vue_snapshot(
            "SideMenu.vue",
            r#"<script setup lang="ts">
import type { MenuItems } from './types.ts'
import { computed } from 'vue'

export type NavigatePayload =
  | { type: 'notification'; to: string }
  | { type: 'menu-item'; to: string }

interface SideMenuProps {
  visible?: boolean
  menuItems?: MenuItems[]
}

const props = defineProps<SideMenuProps>()
const isOpen = computed(() => props.visible)
</script>

<template><div>{{ isOpen }}</div></template>

<style lang="scss" scoped>
.menu { color: red; }
</style>"#,
            AnalysisScope::LSP,
        );
        assert_eq!(
            snap.meta.script_lang,
            Some("ts".to_string()),
            "script_lang should be 'ts' for multiline SFC with lang=\"ts\""
        );
    }

    /// @ai-generated - script_lang is None when no lang attribute
    #[test]
    fn parse_vue_snapshot_script_lang_none() {
        let (snap, _parsed) = parse_vue_snapshot(
            "Comp.vue",
            "<script setup>const n = 1</script><template><div/></template>",
            AnalysisScope::LSP,
        );
        assert_eq!(snap.meta.script_lang, None);
    }

    /// @ai-generated - Deterministic: same source → identical hashes
    #[test]
    fn parse_vue_snapshot_deterministic_hashes() {
        let src = "<script setup>const n = 1</script><template><div>{{n}}</div></template>";
        let (snap1, _) = parse_vue_snapshot("Comp.vue", src, AnalysisScope::LSP);
        let (snap2, _) = parse_vue_snapshot("Comp.vue", src, AnalysisScope::LSP);
        assert_eq!(snap1.whole_hash, snap2.whole_hash);
        assert_eq!(snap1.semantic_hash, snap2.semantic_hash);
        assert_eq!(snap1.slices.script, snap2.slices.script);
        assert_eq!(snap1.slices.template, snap2.slices.template);
    }

    // ═══════════════════════════════════════════════════════════
    // parse_non_sfc_snapshot
    // ═══════════════════════════════════════════════════════════

    /// @ai-generated - Non-SFC whole_hash differs per content
    #[test]
    fn parse_non_sfc_whole_hash_differs() {
        let a = parse_non_sfc_snapshot("a.ts", "export const x = 1", &classified("a.ts"));
        let b = parse_non_sfc_snapshot("b.ts", "export const y = 2", &classified("b.ts"));
        assert_ne!(a.whole_hash, b.whole_hash);
    }

    /// @ai-generated - Non-SFC semantic_hash is content-dependent so callers
    /// can detect when an imported .ts file changes.
    #[test]
    fn parse_non_sfc_semantic_hash_content_dependent() {
        let a = parse_non_sfc_snapshot("a.ts", "export const x = 1", &classified("a.ts"));
        let b = parse_non_sfc_snapshot("b.ts", "export const y = 2", &classified("b.ts"));
        assert_ne!(
            a.semantic_hash, b.semantic_hash,
            "different non-SFC content must produce different semantic hashes"
        );
    }

    /// @ai-generated - Non-SFC semantic_hash is deterministic
    #[test]
    fn parse_non_sfc_semantic_hash_deterministic() {
        let a = parse_non_sfc_snapshot("a.ts", "export const x = 1", &classified("a.ts"));
        let b = parse_non_sfc_snapshot("a.ts", "export const x = 1", &classified("a.ts"));
        assert_eq!(a.semantic_hash, b.semantic_hash);
    }

    /// @ai-generated - <template src="..."> produces ExternalSourceRequest
    /// with ExternalBlockKind::Template
    #[test]
    fn parse_vue_snapshot_template_src_external_request() {
        let (snap, _parsed) = parse_vue_snapshot(
            "/src/Comp.vue",
            "<template src=\"./t.html\"></template><script setup>const n=1</script>",
            AnalysisScope::LSP,
        );
        assert_eq!(snap.external_requests.len(), 1);
        assert_eq!(
            snap.external_requests[0].block_kind,
            ExternalBlockKind::Template
        );
        assert_eq!(snap.external_requests[0].specifier, "./t.html");
        assert_eq!(
            snap.external_requests[0].resolved_canonical_id,
            "/src/t.html"
        );
        assert_eq!(snap.src_blocks[0].tag_name, "template");
    }

    /// @ai-generated - <style src="..."> produces ExternalSourceRequest
    /// with ExternalBlockKind::Style
    #[test]
    fn parse_vue_snapshot_style_src_external_request() {
        let (snap, _parsed) = parse_vue_snapshot(
            "/src/Comp.vue",
            "<template><div/></template><style src=\"./s.css\"></style>",
            AnalysisScope::LSP,
        );
        assert_eq!(snap.external_requests.len(), 1);
        assert_eq!(
            snap.external_requests[0].block_kind,
            ExternalBlockKind::Style
        );
        assert_eq!(snap.external_requests[0].specifier, "./s.css");
        assert_eq!(
            snap.external_requests[0].resolved_canonical_id,
            "/src/s.css"
        );
        assert_eq!(snap.src_blocks[0].tag_name, "style");
    }

    /// @ai-generated - Vapor flag detection on <template vapor>
    #[test]
    fn parse_vue_snapshot_vapor_detection() {
        let (snap_normal, _) = parse_vue_snapshot(
            "Comp.vue",
            "<template><div>hello</div></template>",
            AnalysisScope::LSP,
        );
        assert!(
            !snap_normal.descriptor.vapor,
            "normal template should not be vapor"
        );

        let (snap_vapor, _) = parse_vue_snapshot(
            "Comp.vue",
            "<template vapor><div>hello</div></template>",
            AnalysisScope::LSP,
        );
        assert!(
            snap_vapor.descriptor.vapor,
            "template with vapor attribute should be detected"
        );
    }

    /// @ai-generated - Non-SFC has no blocks
    #[test]
    fn parse_non_sfc_no_blocks() {
        let snap = parse_non_sfc_snapshot("helper.ts", "const x = 1", &classified("helper.ts"));
        assert!(!snap.meta.has_script);
        assert!(!snap.meta.has_template);
        assert_eq!(snap.descriptor.script_count, 0);
        assert_eq!(snap.descriptor.template_count, 0);
        assert!(snap.external_requests.is_empty());
        assert!(snap.src_blocks.is_empty());
    }

    // ═══════════════════════════════════════════════════════════
    // Script span SFC-absolute adjustment
    // ═══════════════════════════════════════════════════════════

    /// @ai-generated - Script binding spans become SFC-absolute after parsing
    #[test]
    fn script_analysis_spans_are_sfc_absolute() {
        // Template block takes bytes 0..48, script starts after
        let source = r#"<template><div>{{ msg }}</div></template>
<script setup>
const msg = 'hello'
</script>"#;
        let (snap, _parsed) = parse_vue_snapshot("App.vue", source, AnalysisScope::LSP);

        // Find the "msg" binding
        let binding = snap
            .script_analysis
            .bindings
            .iter()
            .find(|b| b.name == "msg")
            .expect("should find 'msg' binding");

        // The span should point to "msg" in "const msg = 'hello'" within the SFC source
        let script_line = "const msg = 'hello'";
        let msg_in_script = source.find(script_line).unwrap() + "const ".len();
        assert_eq!(
            binding.span.start as usize, msg_in_script,
            "span.start should be SFC-absolute offset of 'msg' in script, got {} expected {}",
            binding.span.start, msg_in_script
        );
        assert_eq!(
            binding.span.end as usize,
            msg_in_script + "msg".len(),
            "span.end should be SFC-absolute end of 'msg' in script"
        );
    }

    /// @ai-generated - Import spans become SFC-absolute after parsing
    #[test]
    fn import_analysis_spans_are_sfc_absolute() {
        let source = r#"<template><div/></template>
<script setup>
import { ref } from 'vue'
const x = ref(0)
</script>"#;
        let (snap, _parsed) = parse_vue_snapshot("App.vue", source, AnalysisScope::LSP);

        let import = snap
            .script_analysis
            .imports
            .iter()
            .find(|i| i.source == "vue")
            .expect("should find vue import");

        // import statement should have SFC-absolute span
        let import_line = "import { ref } from 'vue'";
        let import_offset = source.find(import_line).unwrap();
        assert_eq!(
            import.span.start as usize, import_offset,
            "import span.start should be SFC-absolute"
        );

        // "ref" binding inside the import should also be SFC-absolute
        let ref_binding = import
            .bindings
            .iter()
            .find(|b| b.name == "ref")
            .expect("should find 'ref' binding");
        // "ref" appears after "import { "
        let ref_in_import = source.find("{ ref }").unwrap() + 2; // past "{ "
        assert_eq!(
            ref_binding.span.start as usize, ref_in_import,
            "import binding span.start should be SFC-absolute"
        );
    }

    // ═══════════════════════════════════════════════════════════
    // Preprocessor request tests
    // ═══════════════════════════════════════════════════════════

    #[test]
    fn vue_api_callback_param_spans_are_sfc_absolute() {
        let source = r#"<template><div/></template>
<script setup>
import { watch, ref } from 'vue'
const count = ref(0)
watch(count, (value, oldValue) => {
  console.log(value, oldValue)
})
</script>"#;
        let (snap, _parsed) = parse_vue_snapshot("App.vue", source, AnalysisScope::LSP);

        let watch_call = snap
            .script_analysis
            .vue_api_calls
            .iter()
            .find(|call| call.api == verter_semantic::analysis::VueApiClassification::Watch)
            .expect("should find watch() call");

        assert_eq!(watch_call.callback_params.len(), 2);

        let value_start = source.find("(value, oldValue)").unwrap() + 1;
        let old_value_start = source.find("oldValue").unwrap();
        assert_eq!(
            watch_call.callback_params[0].span.start as usize,
            value_start
        );
        assert_eq!(
            watch_call.callback_params[1].span.start as usize,
            old_value_start
        );
    }

    /// @ai-generated - template_lang captured for pug
    #[test]
    fn parse_captures_template_lang_pug() {
        let source =
            "<template lang=\"pug\">\ndiv hello\n</template>\n<script setup>\nconst x = 1\n</script>";
        let (snap, _parsed) = parse_vue_snapshot("test.vue", source, AnalysisScope::NONE);
        assert_eq!(
            snap.meta.template_lang,
            Some("pug".to_string()),
            "template_lang should be 'pug'"
        );
    }

    /// @ai-generated - no template_lang for plain HTML template
    #[test]
    fn no_template_lang_for_html() {
        let source =
            "<template><div>hello</div></template>\n<script setup>\nconst x = 1\n</script>";
        let (snap, _parsed) = parse_vue_snapshot("test.vue", source, AnalysisScope::NONE);
        assert!(
            snap.meta.template_lang.is_none(),
            "template_lang should be None for native HTML"
        );
    }

    /// @ai-generated - explicit lang="html" is treated as native (no preprocessor request)
    #[test]
    fn no_template_lang_for_explicit_html() {
        let source = "<template lang=\"html\"><div>hello</div></template>";
        let (snap, _parsed) = parse_vue_snapshot("test.vue", source, AnalysisScope::NONE);
        assert!(
            snap.meta.template_lang.is_none(),
            "template_lang should be None for explicit lang='html'"
        );
        assert!(
            snap.preprocessor_requests.is_empty(),
            "no preprocessor requests for native HTML"
        );
    }

    /// @ai-generated - preprocessor request for pug template
    #[test]
    fn preprocessor_request_for_pug_template() {
        let source = "<template lang=\"pug\">\ndiv hello\n</template>\n<script setup>\nconst x = 1\n</script>";
        let (snap, _parsed) = parse_vue_snapshot("test.vue", source, AnalysisScope::NONE);
        assert_eq!(snap.preprocessor_requests.len(), 1);
        let req = &snap.preprocessor_requests[0];
        assert_eq!(req.block_type, PreprocessorBlockType::Template);
        assert_eq!(req.index, 0);
        assert_eq!(req.lang, "pug");
        assert!(
            req.content.contains("div hello"),
            "content should contain 'div hello', got: {}",
            req.content
        );
    }

    /// @ai-generated - preprocessor request for coffee script
    #[test]
    fn preprocessor_request_for_coffee_script() {
        let source =
            "<template><div>hello</div></template>\n<script lang=\"coffee\">\nx = 1\n</script>";
        let (snap, _parsed) = parse_vue_snapshot("test.vue", source, AnalysisScope::NONE);
        assert_eq!(snap.preprocessor_requests.len(), 1);
        let req = &snap.preprocessor_requests[0];
        assert_eq!(req.block_type, PreprocessorBlockType::Script);
        assert_eq!(req.index, 0);
        assert_eq!(req.lang, "coffee");
        assert!(
            req.content.contains("x = 1"),
            "content should contain 'x = 1', got: {}",
            req.content
        );
    }

    /// @ai-generated - no preprocessor requests for native langs
    #[test]
    fn no_preprocessor_requests_for_native_langs() {
        let source =
            "<template><div>hello</div></template>\n<script lang=\"ts\" setup>\nconst x = 1\n</script>\n<style>.a { color: red }</style>";
        let (snap, _parsed) = parse_vue_snapshot("test.vue", source, AnalysisScope::NONE);
        assert!(
            snap.preprocessor_requests.is_empty(),
            "no preprocessor requests for html + ts + css"
        );
    }

    /// @ai-generated - preprocessor request for scss style
    #[test]
    fn preprocessor_request_for_scss_style() {
        let source = "<template><div>hello</div></template>\n<style lang=\"scss\">\n.a { .b { color: red } }\n</style>";
        let (snap, _parsed) = parse_vue_snapshot("test.vue", source, AnalysisScope::NONE);
        assert_eq!(snap.preprocessor_requests.len(), 1);
        let req = &snap.preprocessor_requests[0];
        assert_eq!(req.block_type, PreprocessorBlockType::Style);
        assert_eq!(req.index, 0);
        assert_eq!(req.lang, "scss");
        assert!(
            req.content.contains(".a { .b"),
            "content should contain '.a {{ .b', got: {}",
            req.content
        );
    }

    /// SCSS style blocks scan to full CSS facts (classes with exact
    /// SFC-absolute spans, rule body spans) — the class-intelligence
    /// features light up for `lang="scss"` blocks.
    #[test]
    fn scss_style_block_produces_scanned_css_facts() {
        let source = "<template><div class=\"card\">x</div></template>\n<style lang=\"scss\" scoped>\n.card {\n  .title { color: red; }\n}\n</style>";
        let (snap, _parsed) = parse_vue_snapshot("test.vue", source, AnalysisScope::LSP);
        assert_eq!(snap.style_analyses.len(), 1);
        let style = &snap.style_analyses[0];
        assert_eq!(
            style.lang,
            verter_semantic::analysis::StyleAnalysisLang::Scss
        );
        let css = style
            .css
            .as_ref()
            .expect("scss block must carry scanned CSS facts");
        let title = css
            .classes
            .iter()
            .find(|c| c.name == "title")
            .expect("nested scss class extracted");
        assert_eq!(
            &source[title.span.start as usize..title.span.end as usize],
            "title",
            "nested class span is SFC-absolute and exact"
        );
        assert!(
            css.selectors.iter().all(|s| s.rule_body_span.is_some()),
            "closed rules carry body spans"
        );
    }

    /// DISCRIMINATING pair for the sound v-bind usage fact: `v-bind(a + b)`
    /// marks BOTH `a` and `b` used-in-style (the retired `.split('.')` text
    /// probe produced the literal "a + b" and marked neither), while a truly
    /// unused binding stays unmarked.
    #[test]
    fn v_bind_complex_expression_marks_every_root_used_in_style() {
        let source = "<template><div>x</div></template>\n<script setup>\nconst a = 1\nconst b = 2\nconst unused = 3\n</script>\n<style>\n.x { width: v-bind(a + b); }\n</style>";
        let (snap, _parsed) = parse_vue_snapshot("test.vue", source, AnalysisScope::LSP);
        let binding = |name: &str| {
            snap.script_analysis
                .bindings
                .iter()
                .find(|b| b.name == name)
                .unwrap_or_else(|| panic!("binding {name}"))
        };
        assert!(
            binding("a").used_in_style,
            "root `a` of `a + b` is style-used"
        );
        assert!(
            binding("b").used_in_style,
            "root `b` of `a + b` is style-used"
        );
        assert!(
            !binding("unused").used_in_style,
            "a binding not referenced by any v-bind stays unmarked"
        );
        assert!(
            snap.script_analysis
                .style_vbind_roots
                .contains(&"a".to_string())
                && snap
                    .script_analysis
                    .style_vbind_roots
                    .contains(&"b".to_string()),
            "the B5 liveness feed carries both roots: {:?}",
            snap.script_analysis.style_vbind_roots
        );
    }

    /// The pair's positive leg: a binding used ONLY in style `v-bind()` is
    /// used_in_style (no unused diagnostic); member roots count.
    #[test]
    fn v_bind_only_style_usage_counts_as_used() {
        let source = "<template><div>x</div></template>\n<script setup>\nconst color = 'red'\nconst theme = { main: 'blue' }\n</script>\n<style>\n.x { color: v-bind(color); background: v-bind(theme.main); }\n</style>";
        let (snap, _parsed) = parse_vue_snapshot("test.vue", source, AnalysisScope::LSP);
        let binding = |name: &str| {
            snap.script_analysis
                .bindings
                .iter()
                .find(|b| b.name == name)
                .unwrap()
        };
        assert!(binding("color").used_in_style);
        assert!(
            binding("theme").used_in_style,
            "member-expression root counts"
        );
    }

    /// An unparseable v-bind expression fails OPEN: every binding is treated
    /// as style-used (no false unused diagnostic can fire).
    #[test]
    fn v_bind_unparseable_expression_fails_open() {
        let source = "<template><div>x</div></template>\n<script setup>\nconst a = 1\n</script>\n<style>\n.x { color: v-bind(@@@); }\n</style>";
        let (snap, _parsed) = parse_vue_snapshot("test.vue", source, AnalysisScope::LSP);
        let a = snap
            .script_analysis
            .bindings
            .iter()
            .find(|b| b.name == "a")
            .unwrap();
        assert!(
            a.used_in_style,
            "an unparseable v-bind marks every binding live (fail open)"
        );
    }

    /// Analyzed v-binds carry REAL SFC-absolute expression spans (the token
    /// the IDE hover/completion anchors on), not a degenerate content-offset
    /// pair.
    #[test]
    fn v_bind_carries_real_expression_span() {
        let source = "<template><div>x</div></template>\n<script setup>\nconst color = 'red'\n</script>\n<style>\n.x { color: v-bind(color); }\n</style>";
        let (snap, _parsed) = parse_vue_snapshot("test.vue", source, AnalysisScope::LSP);
        assert_eq!(snap.style_analyses.len(), 1);
        let vb = &snap.style_analyses[0].v_binds[0];
        assert_eq!(
            &source[vb.start as usize..vb.end as usize],
            "color",
            "v-bind span covers exactly the authored expression"
        );
        assert_eq!(vb.expr_roots, vec!["color".to_string()]);
        assert!(vb.roots_complete);
    }

    /// Svelte `<style>` blocks scan to scoped-by-default CSS facts with
    /// carrier-absolute spans, and `:global(...)` selectors are recorded as
    /// Global special pseudos.
    #[test]
    fn svelte_style_analyses_scoped_by_default_with_global_pseudo() {
        let source = "<script>let x = 1;</script>\n<div class=\"card\"></div>\n<style>\n.card { color: red; }\n:global(.reset) { margin: 0; }\n</style>\n";
        let parsed = verter_compiler::svelte::parser::parse_svelte(source);
        let styles = build_svelte_style_analyses(source, &parsed.styles, &[None]);
        assert_eq!(styles.len(), 1);
        let style = &styles[0];
        assert!(style.scoped, "svelte styles are scoped by default");
        let css = style.css.as_ref().expect("scanned css facts");
        let card = css.classes.iter().find(|c| c.name == "card").unwrap();
        assert_eq!(
            &source[card.span.start as usize..card.span.end as usize],
            "card",
            "carrier-absolute exact span"
        );
        let global = style
            .special_pseudos
            .iter()
            .find(|p| p.kind == verter_semantic::analysis::SpecialPseudoKind::Global)
            .expect(":global recorded");
        assert_eq!(
            &source[global.start as usize..global.end as usize],
            ":global(.reset)"
        );
        // The .reset class inside :global is still an addressable declaration.
        assert!(css.classes.iter().any(|c| c.name == "reset"));
    }

    /// Svelte markup class tokens: `class="a b"` entries split with exact
    /// spans; `class:x` directives carry the local name span; dynamic values
    /// are skipped (fail closed).
    #[test]
    fn svelte_markup_class_tokens_exact_spans() {
        let source = "<div class=\"card active\" class:open={cond}>\n  {#if cond}<span class=\"inner\"></span>{/if}\n  <b class={dynamic}></b>\n</div>\n";
        let parsed = verter_compiler::svelte::parser::parse_svelte(source);
        let tokens = collect_svelte_markup_class_tokens(source, &parsed.template);
        let by_name: Vec<(&str, bool)> = tokens
            .iter()
            .map(|t| (t.name.as_str(), t.from_directive))
            .collect();
        assert_eq!(
            by_name,
            vec![
                ("card", false),
                ("active", false),
                ("open", true),
                ("inner", false),
            ],
            "dynamic class={{expr}} yields NO token"
        );
        for t in &tokens {
            assert_eq!(
                &source[t.span.start as usize..t.span.end as usize],
                t.name,
                "token span must cover exactly the authored name"
            );
        }
    }

    /// End-to-end: the svelte snapshot carries style analyses + markup tokens.
    #[test]
    fn svelte_snapshot_carries_style_and_markup_class_facts() {
        // Route through the real carrier snapshot build.
        let source = "<script>let n = 1;</script>\n<div class=\"card\"></div>\n<style>.card { color: red; }</style>\n";
        let (snapshot, _artifact) = carrier_parse_snapshot(
            "test.svelte",
            source,
            AnalysisScope::LSP,
            &verter_language::FileLanguage::svelte(),
            &crate::types::MetaProvenance::default(),
        )
        .expect("svelte carrier dispatch yields a snapshot");
        assert_eq!(snapshot.style_analyses.len(), 1);
        assert!(snapshot.style_analyses[0].scoped);
        assert!(snapshot.style_analyses[0].css.is_some());
        assert_eq!(snapshot.markup_class_tokens.len(), 1);
        assert_eq!(snapshot.markup_class_tokens[0].name, "card");
    }

    /// Indented Sass stays fail-closed: Vue features only, no scanned CSS.
    #[test]
    fn sass_style_block_stays_unscanned() {
        let source =
            "<template><div>x</div></template>\n<style lang=\"sass\">\n.a\n  color: red\n</style>";
        let (snap, _parsed) = parse_vue_snapshot("test.vue", source, AnalysisScope::LSP);
        assert_eq!(snap.style_analyses.len(), 1);
        assert!(snap.style_analyses[0].css.is_none());
    }

    /// @ai-generated - preprocessor request for custom block with lang
    #[test]
    fn preprocessor_request_for_custom_block_with_lang() {
        let source = "<template><div>hello</div></template>\n<i18n lang=\"yaml\">\nen:\n  hello: world\n</i18n>";
        let (snap, _parsed) = parse_vue_snapshot("test.vue", source, AnalysisScope::NONE);
        assert_eq!(snap.preprocessor_requests.len(), 1);
        let req = &snap.preprocessor_requests[0];
        assert_eq!(req.block_type, PreprocessorBlockType::Custom);
        assert_eq!(req.index, 0);
        assert_eq!(req.lang, "yaml");
        assert!(
            req.content.contains("hello: world"),
            "content should contain 'hello: world', got: {}",
            req.content
        );
    }

    /// @ai-generated - multiple preprocessor requests for mixed non-native langs
    #[test]
    fn multiple_preprocessor_requests_for_mixed_langs() {
        let source = "<template lang=\"pug\">\ndiv hello\n</template>\n<script lang=\"coffee\">\nx = 1\n</script>\n<style lang=\"scss\">\n.a { .b { color: red } }\n</style>";
        let (snap, _parsed) = parse_vue_snapshot("test.vue", source, AnalysisScope::NONE);
        assert_eq!(
            snap.preprocessor_requests.len(),
            3,
            "should have 3 preprocessor requests: template, script, style"
        );

        // Verify each type is present
        let types: Vec<_> = snap
            .preprocessor_requests
            .iter()
            .map(|r| r.block_type)
            .collect();
        assert!(types.contains(&PreprocessorBlockType::Template));
        assert!(types.contains(&PreprocessorBlockType::Script));
        assert!(types.contains(&PreprocessorBlockType::Style));
    }

    #[test]
    fn parse_non_sfc_dts_does_not_panic() {
        // This .d.ts content with tuple types triggers an OXC panic when parsed
        // with SourceType::ts() instead of SourceType::d_ts().
        let dts_content = r#"
export type Slot<T extends any = any> = (...args: [T] | (T extends undefined ? [] : never)) => VNode[];
type InternalSlots = { [name: string]: Slot | undefined; };
export declare function defineComponent<T>(options: T): T;
export type VNodeRef = string | Ref | ((ref: Element | null, refs: Record<string, any>) => void);
"#;
        // Should not panic — previously crashed with unwrap() on None in oxc_ast::ts.rs
        let snapshot = parse_non_sfc_snapshot(
            "node_modules/@vue/runtime-core/dist/runtime-core.d.ts",
            dts_content,
            &classified("node_modules/@vue/runtime-core/dist/runtime-core.d.ts"),
        );
        // Verify no panic diagnostics were emitted
        assert!(
            snapshot.parse_diagnostics.diagnostics.is_empty(),
            "should not have parse diagnostics for valid .d.ts content"
        );
    }

    /// Non-SFC analysis scope includes VUE_API_USAGE, MACROS, MACRO_TYPE_DEPS,
    /// EXPORT_SIGNATURES, and SCRIPT_USAGES (all script-applicable flags).
    #[test]
    fn parse_non_sfc_expanded_analysis_scope() {
        // A .ts file that uses Vue APIs at top level — provide() call detected
        let source = r#"import { provide, ref, onMounted } from 'vue'
const count = ref(0)
provide('counter', count)
onMounted(() => { console.log('mounted') })
"#;
        let snap = parse_non_sfc_snapshot("composable.ts", source, &classified("composable.ts"));
        let analysis = &snap.script_analysis;
        // Positive: VUE_API_USAGE should detect provide() and onMounted() calls
        assert!(
            !analysis.vue_api_calls.is_empty(),
            "should detect vue API calls (provide, onMounted) in non-SFC .ts file"
        );
        assert!(
            analysis
                .vue_api_calls
                .iter()
                .any(|c| c.api == verter_semantic::analysis::VueApiClassification::Provide),
            "should detect provide() call"
        );
        assert!(
            analysis
                .vue_api_calls
                .iter()
                .any(|c| c.api == verter_semantic::analysis::VueApiClassification::OnMounted),
            "should detect onMounted() call"
        );
        // Positive: IMPORTS should be populated
        assert!(!analysis.imports.is_empty(), "should have imports from vue");
        // Positive: BINDINGS should be populated
        assert!(
            !analysis.bindings.is_empty(),
            "should have bindings (count)"
        );
        // Negative: should not have lifecycle hooks that aren't in the source
        assert!(
            analysis
                .vue_api_calls
                .iter()
                .all(|c| c.api != verter_semantic::analysis::VueApiClassification::OnUnmounted),
            "should not detect onUnmounted which isn't in the source"
        );
    }

    #[test]
    fn parse_non_sfc_dts_variants() {
        // All .d.ts extension variants should use the correct SourceType
        let content = "export declare const foo: string;";
        for id in &["types.d.ts", "index.d.mts", "utils.d.cts"] {
            let snapshot = parse_non_sfc_snapshot(id, content, &classified(id));
            assert!(
                snapshot.parse_diagnostics.diagnostics.is_empty(),
                "{id} should parse without diagnostics"
            );
        }
    }
}
