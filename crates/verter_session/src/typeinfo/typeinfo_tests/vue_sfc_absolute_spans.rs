//! Discriminating raw-slice tests for **SFC-absolute** `.vue` typeinfo spans.
//!
//! A `.vue` file's eval source is position-preserving (script content sits at
//! its raw SFC byte offsets, non-script bytes whitespace-blanked), so every
//! member / signature / index span the OXC lowering stamps is SFC-absolute —
//! i.e. an offset into `IndexedReady.raw_source`, the original `.vue` file.
//!
//! Each test SLICES the reported span out of `raw_source` and compares it to
//! the expected source token. These tests FAIL against the pre-fix tree (which
//! produced spans relative to the COMPACT concatenation of script-block
//! contents — `script() + "\n" + script_setup()` — so the slice landed on the
//! wrong raw bytes, badly for a `<template>`-before-`<script>` layout and
//! provably wrong for the second of two script blocks) and PASS against the
//! position-preserving tree.

use std::sync::Arc;

use verter_semantic::analysis::types::AnalyzedMacroKind;

use crate::typeinfo::adapters::vue::slots_from_typeinfo_surface;
use crate::typeinfo::types::{TypeInfoQueryLevel, VueMacroSurfaceRequest};
use crate::typeinfo::{CanonicalSpan, TypeInfoSurface, TypeInfoSurfaceMember};
use crate::types::{FileKind, HostConfig, UpsertRequest};
use crate::VerterHost;

fn make_host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig::default()))
}

fn upsert(host: &VerterHost, canonical_id: &str, source: &str) {
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(canonical_id.to_string()),
        input_id: canonical_id.to_string(),
        source: Arc::from(source),
        file_kind: FileKind::from_path(canonical_id),
        aliases: Vec::new(),
    });
}

/// The cache-owned raw `.vue` source for `canonical_id` (the SFC-absolute
/// coordinate system every typeinfo span indexes into).
fn raw_source(host: &VerterHost, canonical_id: &str) -> Arc<str> {
    Arc::clone(
        &host
            .ensure_indexed_ready(canonical_id)
            .expect("indexed ready")
            .raw_source,
    )
}

fn whole_hash(host: &VerterHost, canonical_id: &str) -> verter_semantic::analysis::types::Hash16 {
    host.ensure_indexed_ready(canonical_id)
        .expect("indexed ready")
        .whole_hash
}

fn macro_index_of(host: &VerterHost, canonical_id: &str, kind: AnalyzedMacroKind) -> usize {
    let indexed = host.ensure_indexed_ready(canonical_id).expect("indexed");
    indexed
        .snapshot
        .macros
        .iter()
        .position(|m| m.kind == kind)
        .unwrap_or_else(|| panic!("no {kind:?} macro in {canonical_id}"))
}

fn macro_request(
    host: &VerterHost,
    canonical_id: &str,
    kind: AnalyzedMacroKind,
) -> VueMacroSurfaceRequest {
    VueMacroSurfaceRequest {
        owner_canonical: Arc::from(canonical_id),
        macro_index: macro_index_of(host, canonical_id, kind),
        macro_kind: kind,
        root_identity: whole_hash(host, canonical_id),
        level: TypeInfoQueryLevel::FullMetadata,
    }
}

fn member<'a>(surface: &'a TypeInfoSurface, name: &str) -> &'a TypeInfoSurfaceMember {
    surface
        .members
        .iter()
        .find(|m| m.name.as_ref() == name)
        .unwrap_or_else(|| {
            panic!(
                "member `{name}` must be on the surface; got {:?}",
                surface
                    .members
                    .iter()
                    .map(|m| m.name.as_ref())
                    .collect::<Vec<_>>()
            )
        })
}

/// Slice a [`CanonicalSpan`] out of `raw` and assert the span references
/// `expected_file` (so a cross-block / cross-file member can't silently slice
/// the wrong source).
fn slice<'a>(raw: &'a str, span: &CanonicalSpan, expected_file: &str) -> &'a str {
    assert_eq!(
        span.file.as_ref(),
        expected_file,
        "span must reference file {expected_file}, got {}",
        span.file
    );
    let start = span.span.start as usize;
    let end = span.span.end as usize;
    raw.get(start..end).unwrap_or_else(|| {
        panic!(
            "span [{start}, {end}) out of bounds for raw source of len {}",
            raw.len()
        )
    })
}

// ---------------------------------------------------------------------------
// (1) Single-block `.vue`, `<template>` BEFORE `<script setup>`. The member
//     name span slices the RAW source to `label`.
//
//     PRE-FIX: spans were relative to the compact script content (which did
//     NOT include the leading `<template>...</template>\n<script setup>` bytes),
//     so the offset slices the WRONG raw text (somewhere inside the template).
//     POST-FIX: the span is SFC-absolute → slices `label`.
// ---------------------------------------------------------------------------

const TEMPLATE_BEFORE_SCRIPT: &str = r#"<template>
  <div>{{ label }}</div>
</template>
<script setup lang="ts">
interface Props {
  count: number;
  label?: string;
  readonly id: string;
}
defineProps<Props>();
</script>
"#;

#[test]
fn template_before_script_member_name_span_slices_raw_to_label() {
    const FILE: &str = "/w/TemplateBefore.vue";
    let host = make_host();
    upsert(&host, FILE, TEMPLATE_BEFORE_SCRIPT);
    let raw = raw_source(&host, FILE);

    let request = macro_request(&host, FILE, AnalyzedMacroKind::DefineProps);
    let macro_surface = host
        .resolve_vue_macro_surface(&request)
        .expect("defineProps<Props>() must resolve a macro surface");
    let surface = &macro_surface.surface;

    let label = member(surface, "label");
    let name_span = label
        .name_span
        .as_ref()
        .expect("`label` must carry a NAME span");
    assert_eq!(
        slice(&raw, name_span, FILE),
        "label",
        "member name span must be SFC-absolute and slice the raw `.vue` to `label`"
    );

    // DISCRIMINATING: the byte offset where `label` lives in the SFC is AFTER
    // the whole template block. A compact-relative span (the pre-fix bug) would
    // be far smaller than this offset.
    let sfc_label_offset = raw.find("label?").expect("`label?` present in raw") as u32;
    assert_eq!(
        name_span.span.start, sfc_label_offset,
        "name span start must equal the SFC-absolute offset of `label`, not a compact offset"
    );
    let template_end = raw.find("</template>").expect("template present") as u32;
    assert!(
        name_span.span.start > template_end,
        "the `label` span must land AFTER the template block (offset {} > template end {}); a \
         span that landed before it proves a compact-relative offset",
        name_span.span.start,
        template_end
    );

    // NEGATIVE: the name span must not slice an unrelated token.
    assert_ne!(slice(&raw, name_span, FILE), "count");
    assert_ne!(slice(&raw, name_span, FILE), "string");
}

// ---------------------------------------------------------------------------
// (2) DUAL-block `<script>` + `<script setup>`. Members originate from BOTH
//     blocks; raw-slice each, ESPECIALLY a member from the SECOND block.
//
//     This is exactly where single-scalar (compact) conversion fails: block2's
//     eval→raw delta differs from block1's, so a compact offset for a second
//     block member lands on the wrong raw bytes. With position-preserving eval
//     source each block sits at its own raw range → no scalar delta.
// ---------------------------------------------------------------------------

const DUAL_BLOCK: &str = r#"<template>
  <div>{{ id }}</div>
</template>
<script lang="ts">
export interface Base {
  baseField: number;
}
</script>
<script setup lang="ts">
interface Props extends Base {
  ownField: string;
}
defineProps<Props>();
</script>
"#;

#[test]
fn dual_block_member_spans_slice_raw_from_both_blocks() {
    const FILE: &str = "/w/DualBlock.vue";
    let host = make_host();
    upsert(&host, FILE, DUAL_BLOCK);
    let raw = raw_source(&host, FILE);

    let request = macro_request(&host, FILE, AnalyzedMacroKind::DefineProps);
    let macro_surface = host
        .resolve_vue_macro_surface(&request)
        .expect("defineProps<Props>() must resolve a macro surface across the heritage edge");
    let surface = &macro_surface.surface;

    // `ownField` is declared in the SECOND block (`<script setup>`).
    let own = member(surface, "ownField");
    let own_name = own
        .name_span
        .as_ref()
        .expect("`ownField` must carry a name span");
    assert_eq!(
        slice(&raw, own_name, FILE),
        "ownField",
        "second-block member name span must slice the raw `.vue` to `ownField`"
    );

    // `baseField` is INHERITED from `Base`, declared in the FIRST block
    // (`<script>`). Its span must slice the FIRST block's raw bytes.
    let base = member(surface, "baseField");
    let base_name = base
        .name_span
        .as_ref()
        .expect("`baseField` must carry a name span (inherited from the first block)");
    assert_eq!(
        slice(&raw, base_name, FILE),
        "baseField",
        "first-block (inherited) member name span must slice the raw `.vue` to `baseField`"
    );

    // DISCRIMINATING: the two members live in DIFFERENT script blocks, so their
    // SFC-absolute offsets straddle the inter-block markup. `baseField` is in
    // the first `<script>`; `ownField` is in the second `<script setup>` —
    // `ownField` MUST come later in the file. A compact concatenation collapses
    // the inter-block markup, so the relative ordering/offsets would not match
    // these raw `.find` positions.
    let raw_base_off = raw.find("baseField").expect("baseField in raw") as u32;
    let raw_own_off = raw.find("ownField").expect("ownField in raw") as u32;
    assert_eq!(
        base_name.span.start, raw_base_off,
        "baseField span must equal its SFC-absolute offset in the first block"
    );
    assert_eq!(
        own_name.span.start, raw_own_off,
        "ownField span must equal its SFC-absolute offset in the second block"
    );
    assert!(
        own_name.span.start > base_name.span.start,
        "the second-block member must have a larger SFC offset than the first-block member \
         ({} > {})",
        own_name.span.start,
        base_name.span.start
    );
}

// ---------------------------------------------------------------------------
// (3) Slot-return span: a `defineSlots` slot whose function returns `VNode[]`
//     has its return-type span slice the RAW source to `VNode[]`.
//
//     `slots_from_typeinfo_surface` slices the slot member's return-type span
//     through `slice_canonical_span` (now `raw_source`). With a `<template>`
//     before the `<script setup>`, an eval-relative span would slice the wrong
//     raw bytes; the SFC-absolute span slices `VNode[]`.
// ---------------------------------------------------------------------------

const SLOTS_VUE: &str = r#"<template>
  <div><slot :item="0" /></div>
</template>
<script setup lang="ts">
defineSlots<{
  default(props: { item: number }): VNode[];
}>();
</script>
"#;

#[test]
fn defineslots_return_span_slices_raw_to_vnode_array() {
    const FILE: &str = "/w/SlotsComp.vue";
    let host = make_host();
    upsert(&host, FILE, SLOTS_VUE);
    let raw = raw_source(&host, FILE);

    let request = macro_request(&host, FILE, AnalyzedMacroKind::DefineSlots);
    let macro_surface = host
        .resolve_vue_macro_surface(&request)
        .expect("defineSlots<...>() must resolve a macro surface");
    let slots = slots_from_typeinfo_surface(&host, &macro_surface);

    let default = slots
        .iter()
        .find(|s| s.name == "default")
        .expect("the `default` slot must surface");
    // The display `return_type` is sliced from the return-type annotation span
    // in the raw source. `VNode` is unresolved (not imported), so this value can
    // ONLY come from a correct raw-source slice — `render_type_expr_display`
    // renders the unresolved type as `None`.
    assert_eq!(
        default.return_type.as_deref(),
        Some("VNode[]"),
        "slot return type must be sliced from the SFC-absolute return span in raw source"
    );

    // DISCRIMINATING: prove the `VNode[]` text lives AFTER the template in the
    // SFC — a compact-relative span could not produce this exact slice.
    let template_end = raw.find("</template>").expect("template present");
    let vnode_off = raw.find("VNode[]").expect("VNode[] present in raw");
    assert!(
        vnode_off > template_end,
        "`VNode[]` is after the template block; a slice that produced it from raw proves the \
         span was SFC-absolute"
    );
}

// ---------------------------------------------------------------------------
// (4) JSDoc in a `.vue`: a member's leading `/** */` description span attaches
//     and slices from RAW source.
//
//     `with_member_jsdoc_spans` anchors the JSDoc search on the member's
//     SFC-absolute name offset and scans the source the host passes
//     (`raw_source`). With a `<template>` before the script, an eval-relative
//     anchor + a compact source would either miss the block or slice the wrong
//     bytes; SFC-absolute anchor + raw source slices the exact doc text.
// ---------------------------------------------------------------------------

const JSDOC_VUE: &str = r#"<template>
  <div>{{ count }}</div>
</template>
<script setup lang="ts">
interface Props {
  /** the documented count */
  count: number;
  plain: string;
}
defineProps<Props>();
</script>
"#;

#[test]
fn vue_member_jsdoc_description_span_slices_raw_to_doc_text() {
    const FILE: &str = "/w/JsdocComp.vue";
    let host = make_host();
    upsert(&host, FILE, JSDOC_VUE);
    let raw = raw_source(&host, FILE);

    let request = macro_request(&host, FILE, AnalyzedMacroKind::DefineProps);
    let macro_surface = host
        .resolve_vue_macro_surface(&request)
        .expect("defineProps<Props>() must resolve a macro surface");
    let surface = &macro_surface.surface;

    let count = member(surface, "count");
    let desc_span = count
        .jsdoc_description_span
        .as_ref()
        .expect("`count` must carry a JSDoc description span (it has a leading `/** */` block)");
    assert_eq!(
        slice(&raw, desc_span, FILE),
        "the documented count",
        "JSDoc description span must slice the exact doc text from the raw `.vue` source"
    );

    // DISCRIMINATING: the doc text lives AFTER the template block in the SFC; a
    // compact-relative description span (or one scanned against compact source)
    // could not slice this exact raw text.
    let template_end = raw.find("</template>").expect("template present") as u32;
    assert!(
        desc_span.span.start > template_end,
        "the JSDoc description span must land after the template block; a span before it proves \
         a compact/eval-relative offset"
    );

    // NEGATIVE: the plain member carries no description span.
    let plain = member(surface, "plain");
    assert!(
        plain.jsdoc_description_span.is_none(),
        "a member with no leading JSDoc must carry NO description span"
    );
}

// ---------------------------------------------------------------------------
// (5) Eval-source SHAPE invariant: the `.vue` eval source is position-
//     preserving — `eval_source.len() == raw_source.len()`, every script byte
//     equals the raw byte at its offset, and every non-script (markup) byte is
//     whitespace (space) or a preserved line terminator (`\r`/`\n`).
//
//     This is the structural guarantee that makes every OXC-produced span
//     SFC-absolute: the TS parser sees script text at its raw offsets and
//     blanks (never markup tokens) everywhere else.
// ---------------------------------------------------------------------------

#[test]
fn vue_eval_source_is_position_preserving_same_length_and_blanked_markup() {
    const FILE: &str = "/w/ShapeComp.vue";
    let host = make_host();
    upsert(&host, FILE, DUAL_BLOCK);

    let indexed = host.ensure_indexed_ready(FILE).expect("indexed ready");
    let raw = indexed.raw_source.as_ref();
    let eval = indexed.eval_source.as_ref();

    // Same length — the core position-preserving property.
    assert_eq!(
        eval.len(),
        raw.len(),
        "eval_source must be exactly the same byte length as raw_source"
    );

    let raw_b = raw.as_bytes();
    let eval_b = eval.as_bytes();

    // Script-content ranges: every byte of each known script block's content
    // must be IDENTICAL in eval and raw (script preserved at raw offsets).
    for needle in [
        "export interface Base {\n  baseField: number;\n}",
        "interface Props extends Base {\n  ownField: string;\n}\ndefineProps<Props>();",
    ] {
        let start = raw.find(needle).unwrap_or_else(|| {
            panic!("script content {needle:?} must be present in raw source")
        });
        let end = start + needle.len();
        assert_eq!(
            &eval_b[start..end],
            &raw_b[start..end],
            "script content range [{start}, {end}) must be byte-identical in eval and raw"
        );
    }

    // Markup region: the leading `<template>...</template>` block has NO script
    // bytes, so in eval source every byte there must be a space or a preserved
    // line terminator — NEVER an original markup byte like `<` or `t`.
    let template_start = raw.find("<template>").expect("template present");
    let template_end = raw.find("</template>").expect("template close present") + "</template>".len();
    for i in template_start..template_end {
        let eb = eval_b[i];
        assert!(
            eb == b' ' || eb == b'\n' || eb == b'\r',
            "markup byte at offset {i} must be blanked to space or a preserved line terminator in \
             eval source; got {:?} (raw byte {:?})",
            eb as char,
            raw_b[i] as char
        );
    }
    // The literal `<template>` open tag must NOT survive into eval source.
    assert!(
        !eval.contains("<template>"),
        "eval source must not contain the `<template>` tag — the TS parser must never see markup"
    );
    assert!(
        !eval.contains("</script>"),
        "eval source must not contain a `</script>` close tag"
    );

    // The inter-block gap (`</script>\n<script setup ...>`) between the first
    // and second script blocks must carry at least one line terminator so the
    // two blocks parse as separate statements (the injected/preserved newline
    // policy).
    let block1_close = raw.find("</script>").expect("first </script> present");
    let block2_open = raw[block1_close..]
        .find("<script setup")
        .map(|rel| block1_close + rel)
        .expect("second <script setup> present");
    let gap = &eval_b[block1_close..block2_open];
    assert!(
        gap.iter().any(|&b| b == b'\n' || b == b'\r'),
        "the inter-script gap must contain a line terminator so adjacent blocks stay on separate \
         logical lines for the TS parser"
    );
}
