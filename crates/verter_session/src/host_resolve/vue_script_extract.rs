//! Free helpers for SFC script extraction and template-converter input
//! shaping — the session's Vue-bridge extraction module.
//!
//! Owns:
//! - `template_converter_inputs` — projects analysed imports and value bindings
//!   into runtime component-linkage pairs. Semantic class domains are
//!   owned by the project semantic dispatcher.
//! - `extract_vue_script_content` — public Vue-SFC `<script>` /
//!   `<script setup>` position-preserving source builder used by the
//!   eval-program pipeline. It copies each script block's content to its
//!   RAW SFC byte range and whitespace-blanks every non-script byte, so
//!   every OXC-produced span is SFC-absolute by construction. Carrier
//!   parser-owned spans are the sole geometry source.
//! - The `<script setup generic="…">` type-parameter reader
//!   (`sfc_script_setup_type_params`) and the component-meta
//!   `populate_ordered_sfc_structure` — Vue-semantic leaves that open the
//!   neutral parse artifact through the blessed `vue_parse()` accessor,
//!   keeping `host_manage/**` free of Vue parse types. Generic parameters
//!   BIND through the prepared-decl bundle's script-setup type bindings
//!   (the dispatch `DeclarationScopePayload` rail), never through an eval
//!   env.

pub(crate) fn template_converter_inputs(
    imports: &[verter_semantic::analysis::AnalyzedImport],
    bindings: &[verter_semantic::analysis::AnalyzedBinding],
) -> Vec<(String, String)> {
    // Carrier-linkage map for template components. A `import type { X }` (or a
    // per-specifier `import { type X }`) is a TYPE-ONLY binding — it has no
    // runtime value, so a tag `<X/>` must NEVER be carrier-linked to it as a
    // value component. Mirror `compile_fact_emission::binding_is_value`
    // (`!import.is_type_only && !binding.is_type_only`) so only runtime value
    // bindings enter the linkage map.
    let mut all_imports: Vec<(String, String)> = imports
        .iter()
        .filter(|imp| !imp.is_type_only)
        .flat_map(|imp| {
            imp.bindings
                .iter()
                .filter(|binding| !binding.is_type_only)
                .map(|binding| (binding.name.clone(), imp.source.clone()))
        })
        .collect();

    // A `const X = defineAsyncComponent(() => import('./X.vue'))` declares a
    // component whose carrier is the dynamically-imported `.vue`. The analyzer
    // captures the static loader specifier on the binding's initializer; surface
    // it in the linkage map so a `<X>` tag links to its `.vue` carrier exactly
    // like a static default import.
    for binding in bindings {
        if let Some(verter_semantic::analysis::BindingInitializer::FunctionCall {
            async_component_source: Some(source),
            ..
        }) = &binding.initializer
        {
            all_imports.push((binding.name.clone(), source.clone()));
        }
    }

    all_imports
}

/// Read the `<script setup generic="…">` type-parameter clause from the
/// neutral parse artifact (opened through the blessed `vue_parse()`
/// accessor). Empty for non-Vue artifacts, plain scripts, and Vue files
/// without `<script setup>` generics.
pub(crate) fn sfc_script_setup_type_params(
    source: &str,
    framework_parse: Option<&verter_language::FrameworkParseArtifact>,
) -> Vec<verter_type_expr::TypeParam> {
    let parsed = framework_parse.and_then(crate::typeinfo::adapters::vue::vue_parse);
    let Some(setup) = parsed.and_then(|parsed| parsed.script_setup()) else {
        return Vec::new();
    };
    let Some(generic_span) = setup.generic else {
        return Vec::new();
    };
    let clause = source[generic_span.start as usize..generic_span.end as usize].trim();
    if clause.is_empty() {
        return Vec::new();
    }
    verter_semantic::analysis::type_eval_build::parse_type_parameter_clause(clause)
}

/// The ONE artifact-local transient producer of the full
/// `<script setup generic="…">` parameter clause over a PINNED
/// [`crate::project_type_store::IndexedReady`]: raw source + framework
/// parse → OWNED transient `Vec<TypeParam>` (typed IR — a lease-time
/// re-borrow product, never stored). Every query-time clause re-borrow
/// routes through this one ingress — the dispatch's script-setup
/// `TypeParam` node construction (which selects + validates one parameter
/// by the stored binding's `(ordinal, name)`) and the macro hot-mirror
/// seed-frame builder — so the two can never diverge.
pub(crate) fn indexed_script_setup_type_params(
    indexed: &crate::project_type_store::IndexedReady,
) -> Vec<verter_type_expr::TypeParam> {
    sfc_script_setup_type_params(
        indexed.raw_source.as_ref(),
        indexed.framework_parse.as_deref(),
    )
}

/// Extract a **position-preserving** script-only source from a Vue SFC string.
///
/// The result is byte-for-byte the SAME LENGTH as `source`: every `<script>` /
/// `<script setup>` block's content is copied to its RAW SFC byte range, and
/// every non-script byte is whitespace-blanked (original `\r`/`\n` preserved so
/// line/column geometry is unchanged; all other bytes replaced with a single
/// space). Because the script text sits at its raw offsets, every span the OXC
/// parser produces from this source is SFC-ABSOLUTE by construction — there is
/// no compact-concatenation coordinate system to translate back from.
///
pub(crate) fn extract_vue_script_content(
    source: &str,
    parsed_sfc: &verter_compiler::parser::types::ParsedSfc,
) -> Option<String> {
    let spans = script_content_spans_from_parsed(parsed_sfc)?;
    // A whole-source-length duplicate of the carrier per extraction: the
    // position-preserving projection is the single largest source-text copy
    // in the pipeline, so it is what this rail is measuring.
    verter_audit::attribute_n!(SourceTextCopy, source.len());
    Some(build_position_preserving_script_source(source, &spans))
}

/// Collect the sorted, de-duplicated `<script>` content byte ranges from a
/// parsed SFC. Ranges are RAW SFC byte offsets `[start, end)`.
fn script_content_spans_from_parsed(
    parsed: &verter_compiler::parser::types::ParsedSfc,
) -> Option<Vec<(u32, u32)>> {
    let mut spans: Vec<(u32, u32)> = [parsed.script(), parsed.script_setup()]
        .into_iter()
        .flatten()
        .filter_map(|script| script.content.map(|span| (span.start, span.end)))
        .filter(|(start, end)| end > start)
        .collect();
    spans.sort_by_key(|(start, _)| *start);
    (!spans.is_empty()).then_some(spans)
}

/// Build a same-length, position-preserving script-only source.
///
/// `spans` are RAW SFC content byte ranges `[start, end)`, pre-sorted and
/// non-overlapping. Every byte of the output is one of:
/// - a script-content byte copied verbatim from `source` at its raw offset;
/// - an original line terminator (`\r` / `\n`) preserved so line geometry is
///   identical to the SFC;
/// - a single ASCII space for every other (markup) byte.
///
/// In addition, every inter-script gap that contains NO line terminator gets a
/// single injected `\n` at its first byte, matching the `"\n"` separator the
/// previous compact extractor inserted between adjacent blocks (so two blocks
/// whose last/first statements lack a trailing `;` or end in a `//` line
/// comment still parse as two statements, never one fused line).
///
/// UTF-8 safety: every replaced byte is a single-byte ASCII value and every
/// preserved range is copied wholesale from valid UTF-8, so the result is valid
/// UTF-8 and exactly `source.len()` bytes long.
pub(crate) fn build_position_preserving_script_source(
    source: &str,
    spans: &[(u32, u32)],
) -> String {
    let src = source.as_bytes();
    // Blank every byte first (line terminators preserved), then stamp script
    // content over its raw range.
    let mut out: Vec<u8> = src
        .iter()
        .map(|&b| if b == b'\n' || b == b'\r' { b } else { b' ' })
        .collect();

    for &(start, end) in spans {
        let (start, end) = (start as usize, end as usize);
        if end <= src.len() && start <= end {
            out[start..end].copy_from_slice(&src[start..end]);
        }
    }

    // Inject a newline into any inter-script gap that lacks a terminator so
    // adjacent blocks stay on separate logical lines for the TS parser.
    for window in spans.windows(2) {
        let prev_end = window[0].1 as usize;
        let next_start = window[1].0 as usize;
        if prev_end >= next_start || next_start > out.len() {
            continue;
        }
        let gap = &out[prev_end..next_start];
        if !gap.iter().any(|&b| b == b'\n' || b == b'\r') {
            // Gap is all spaces (already blanked); the first gap byte is safe
            // to overwrite with a newline without disturbing any script byte.
            out[prev_end] = b'\n';
        }
    }

    debug_assert_eq!(
        out.len(),
        src.len(),
        "position-preserving script source must equal the SFC source length"
    );
    // SAFETY-equivalent: constructed entirely from ASCII replacements + verbatim
    // copies of valid-UTF-8 script ranges, so `out` is valid UTF-8.
    String::from_utf8(out).unwrap_or_else(|err| {
        // Defensive: a malformed span boundary could (in principle) split a
        // multi-byte sequence. Fall back to a lossy decode rather than panic;
        // this preserves length only when the input was ASCII, but a torn
        // boundary is already a corrupt-span bug surfaced elsewhere.
        String::from_utf8_lossy(err.as_bytes()).into_owned()
    })
}

/// Bind schema-8 metadata to the registered content-free structure.
pub(crate) fn populate_ordered_sfc_structure(
    host: &crate::VerterHost,
    canonical_id: &str,
    meta: &mut verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
) {
    let Some((structure, _)) = host.registered_file_structure_snapshot(canonical_id) else {
        return;
    };
    meta.ordered_sfc_structure = Some(ordered_sfc_structure_analysis(&structure));
}

/// Project the registered carrier inventory into the shared schema-8 envelope.
/// This is deliberately content-free and never reparses source bytes.
pub(crate) fn ordered_sfc_structure_analysis(
    structure: &crate::carrier_publication_store::RegisteredFileStructure,
) -> verter_semantic::analysis::component_meta::OrderedSfcStructureAnalysis {
    let inventory = structure.inventory();

    let source_space_tokens = inventory
        .source_spaces()
        .iter()
        .map(|space| {
            structure
                .public_source_space_token(space.id)
                .expect("validated source space")
                .as_str()
                .to_owned()
        })
        .collect::<Vec<_>>();
    let block_tokens = inventory
        .blocks()
        .iter()
        .map(|block| {
            let block_ref = structure.block_ref(block.id()).expect("validated block");
            structure
                .public_block_token(&block_ref)
                .expect("same artifact")
                .as_str()
                .to_owned()
        })
        .collect::<Vec<_>>();
    let markup_node_tokens = inventory
        .markup()
        .nodes()
        .iter()
        .map(|node| {
            let node_ref = structure.node_ref(node.id).expect("validated markup node");
            structure
                .public_node_token(&node_ref)
                .expect("same artifact")
                .as_str()
                .to_owned()
        })
        .collect::<Vec<_>>();
    let mut attributes = inventory
        .blocks()
        .iter()
        .filter_map(|block| match block {
            verter_language::parse_artifact::carrier_inventory::CarrierBlock::Section {
                syntax,
                ..
            } => Some(syntax.attributes.as_ref()),
            verter_language::parse_artifact::carrier_inventory::CarrierBlock::MarkupRoot {
                ..
            } => None,
        })
        .flatten()
        .chain(
            inventory
                .markup()
                .nodes()
                .iter()
                .flat_map(|node| node.kind().attributes()),
        )
        .map(|attribute| attribute.id())
        .collect::<Vec<_>>();
    attributes.sort_unstable();
    attributes.dedup();
    let attribute_tokens = attributes
        .into_iter()
        .map(|id| {
            let attribute_ref = structure.attribute_ref(id).expect("validated attribute");
            structure
                .public_attribute_token(&attribute_ref)
                .expect("same artifact")
                .as_str()
                .to_owned()
        })
        .collect::<Vec<_>>();

    verter_semantic::analysis::component_meta::OrderedSfcStructureAnalysis {
        schema_version: 1,
        artifact_token: structure.public_artifact_token().as_str().to_owned(),
        inventory: std::sync::Arc::clone(inventory),
        source_space_tokens: source_space_tokens.into(),
        block_tokens: block_tokens.into(),
        markup_node_tokens: markup_node_tokens.into(),
        attribute_tokens: attribute_tokens.into(),
    }
}

#[cfg(test)]
mod tests {
    /// Macro expression statements likewise contribute no runtime linkage.
    #[test]
    fn template_converter_inputs_is_linkage_only_for_macro_statements() {
        let alloc = oxc_allocator::Allocator::new();
        let snapshot = verter_semantic::analysis::build_script_analysis(
            "withDefaults(defineProps<{ variant: string }>(), { variant: 'x' });",
            oxc_span::SourceType::ts(),
            &alloc,
        );
        let imports = super::template_converter_inputs(&snapshot.imports, &snapshot.bindings);
        assert!(imports.is_empty());

        // Bound macros remain linkage-neutral too.
        let alloc = oxc_allocator::Allocator::new();
        let bound = verter_semantic::analysis::build_script_analysis(
            "const props = withDefaults(defineProps<{ variant: string }>(), { variant: 'x' });",
            oxc_span::SourceType::ts(),
            &alloc,
        );
        let imports = super::template_converter_inputs(&bound.imports, &bound.bindings);
        assert!(imports.is_empty());
    }
}
