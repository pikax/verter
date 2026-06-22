//! Hermetic unit tests for [`translate_completion_import_edits`]: the completion-resolve
//! auto-import edit translator.
//!
//! These tests build synthetic [`ProviderPositionMapper`] + [`LineIndex`] fixtures (no external
//! corpus, no provider process) that reproduce the dangerous strict-map geometry: a zero-width
//! auto-import insertion in the synthetic helper-import preamble whose strict map SUCCEEDS to the
//! carrier file top `(0,0)`. They pin the classify-before-strict-accept behavior — such an insertion
//! is NEVER strict-accepted at `(0,0)`; it is re-anchored at the `<script setup>` anchor or fails
//! closed — while proving a genuine mapped-user-source edit (the `AddToExisting` shape) STILL takes
//! the strict verbatim route.

use tower_lsp_server::ls_types::{Position, Range};

use super::auto_import::{
    translate_completion_import_edits, AutoImportEditMappingError, ProviderImportEdit,
    ScriptImportInsertionAnchor,
};
use crate::documents::line_index::LineIndex;
use crate::documents::position_map::PositionMapper;
use crate::documents::provider_projection::ProviderPositionMapper;

/// A carrier-IDE mapper whose synthetic helper-import preamble offset (TSX offset 0, the add-import
/// insertion point) DOES strict-map to the carrier — to position `(0,0)`, the file top ABOVE
/// `<script setup>`. Mirrors `merge::tests::make_strict_mapped_preamble_fixture`: a source-map token
/// anchors generated-TSX `(0,0)` → carrier `(0,0)`, so a strict map of the offset-0 preamble
/// insertion SUCCEEDS to `(0,0)` instead of returning `None`; the typed `x_verter_helper_preamble_end`
/// boundary (generated line 1 col 0) still classifies the offset-0 insertion as a preamble import.
///
/// Returns the carrier source (a real `<script setup>` SFC), its UTF-16 line index, the TSX UTF-16
/// line index, and the mapper.
fn strict_mapped_preamble_fixture() -> (String, LineIndex, LineIndex, ProviderPositionMapper) {
    let carrier_source = "<script setup lang=\"ts\">\n\nconst base = 1;\n</script>\n<template>\n  <div>{{ base }}</div>\n</template>\n".to_string();
    let tsx_source =
        "import { defineComponent } from 'vue';\nconst base = 1;\nexport default {};\n";

    let mut builder = oxc_sourcemap::SourceMapBuilder::default();
    let source_id = builder.set_source_and_content("App.vue", &carrier_source);
    // The DISCRIMINATING token: TSX line 0 col 0 → carrier line 0 col 0. With this run present the
    // strict mapper maps the offset-0 add-import insertion to carrier `(0,0)`. Also map the user line.
    builder.add_token(0, 0, 0, 0, Some(source_id), None);
    builder.add_token(1, 0, 2, 0, Some(source_id), None);
    builder.add_token(1, 6, 2, 6, Some(source_id), None);
    let base_json = builder.into_sourcemap().to_json_string();

    // Publish the `x_verter_helper_preamble_end` boundary: the generated position immediately after
    // the last helper import (start of the user line, line 1 col 0).
    let mut value: serde_json::Value = serde_json::from_str(&base_json).unwrap();
    value["x_verter_helper_preamble_end"] = serde_json::json!({ "line": 1, "character": 0 });
    let json = serde_json::to_string(&value).unwrap();

    let mapper = ProviderPositionMapper::source_map(PositionMapper::from_json(&json).unwrap());
    let carrier_li = LineIndex::new_utf16(&carrier_source);
    let tsx_li = LineIndex::new_utf16(tsx_source);
    (carrier_source, carrier_li, tsx_li, mapper)
}

/// A completion-resolve auto-import insertion that STRICT-MAPS to carrier `(0,0)` (the file top,
/// ABOVE `<script setup>`) must NOT be accepted at `(0,0)`: with no usable anchor the whole resolve
/// fails closed, and the returned edit set contains NO `TextEdit` whose range is the `(0,0)..(0,0)`
/// file top carrying import-shaped text.
///
/// Pre-fix, the per-edit loop calls the strict mapper FIRST and accepts the `Some((0,0))` arm,
/// pushing `TextEdit { range: (0,0)..(0,0), new_text: "import ..." }` — the import lands above
/// `<script setup>`, an invalid location. Classify-before-strict-accept diverts the preamble
/// insertion to the shared re-anchor BEFORE the strict `(0,0)` can be taken.
#[test]
fn translate_completion_import_edits_preamble_insertion_strict_mapping_to_origin_is_not_accepted() {
    let (_carrier_source, carrier_li, tsx_li, mapper) = strict_mapped_preamble_fixture();

    // DISCRIMINATING precondition: the offset-0 preamble insertion STRICT-MAPS to carrier `(0,0)` —
    // the exact geometry that makes the pre-fix `Some((0,0))` accept land an import at the file top.
    assert_eq!(
        crate::type_provider::merge::tsx_range_to_carrier_range(
            0,
            0,
            &tsx_li,
            &mapper,
            &carrier_li
        ),
        Some(Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: 0,
            },
        }),
        "fixture precondition: the preamble offset 0 must STRICT-MAP to carrier (0,0)"
    );

    let edits = vec![ProviderImportEdit {
        start: 0,
        end: 0,
        new_text: "import { computed } from \"vue\";\n".to_string(),
    }];

    // No usable anchor: the preamble insertion cannot be placed, so the whole resolve fails closed.
    let result = translate_completion_import_edits(&edits, None, &tsx_li, &mapper, &carrier_li);

    // KEY assertion: regardless of Ok/Err, NO returned edit is the `(0,0)..(0,0)` file top with
    // import-shaped text. (Pre-fix this is exactly what the `Some((0,0))` arm produced.)
    if let Ok(edits) = &result {
        for e in edits {
            let is_origin = e.range.start
                == (Position {
                    line: 0,
                    character: 0,
                })
                && e.range.end
                    == (Position {
                        line: 0,
                        character: 0,
                    });
            assert!(
                !(is_origin && e.new_text.contains("import")),
                "a preamble import insertion must never be accepted at the carrier (0,0) file top: \
                 {e:?}"
            );
        }
    }

    // With no anchor, the classify-before-strict path routes the insertion to the shared re-anchor,
    // which finds it IS a preamble insertion (boundary present) but has no anchor to place it ⇒
    // the whole resolve is rejected with NoInsertionAnchor (fail-closed, all-or-nothing).
    assert_eq!(
        result,
        Err(AutoImportEditMappingError::NoInsertionAnchor),
        "a preamble insertion with no usable anchor must fail closed (NoInsertionAnchor), not splice \
         at (0,0): {result:?}"
    );
}

/// With a usable `ExistingScriptSetup` anchor, the same strict-`(0,0)` preamble insertion is
/// RE-ANCHORED at the anchor position (NOT `(0,0)`), and the resolve succeeds.
#[test]
fn translate_completion_import_edits_preamble_insertion_strict_mapping_to_origin_reanchors_with_anchor(
) {
    let (_carrier_source, carrier_li, tsx_li, mapper) = strict_mapped_preamble_fixture();

    let edits = vec![ProviderImportEdit {
        start: 0,
        end: 0,
        new_text: "import { computed } from \"vue\";\n".to_string(),
    }];

    // The carrier `<script setup>` content starts on carrier line 1 (past the one leading break) —
    // an existing-script-setup anchor at that offset (carrier byte offset of line 1 col 0).
    let anchor_offset = carrier_li
        .position_to_offset(&Position {
            line: 1,
            character: 0,
        })
        .expect("a valid carrier offset at the <script setup> content start");
    let anchor = ScriptImportInsertionAnchor::ExistingScriptSetup {
        offset: anchor_offset,
    };

    let result =
        translate_completion_import_edits(&edits, Some(&anchor), &tsx_li, &mapper, &carrier_li);

    let edits = result.expect("a usable anchor must let the resolve succeed");
    assert_eq!(
        edits.len(),
        1,
        "exactly one re-anchored import edit: {edits:?}"
    );
    assert_eq!(
        edits[0].new_text, "import { computed } from \"vue\";\n",
        "the re-anchored edit carries the provider import text verbatim"
    );
    // It must land at the anchor (carrier line 1), NEVER the strict-mapped `(0,0)` file top.
    assert_ne!(
        edits[0].range,
        Range::default(),
        "the re-anchored import must never land at the strict-mapped (0,0) file top"
    );
    assert_eq!(
        edits[0].range.start,
        Position {
            line: 1,
            character: 0,
        },
        "the import must be re-anchored at the <script setup> content start (line 1), not (0,0): {:?}",
        edits[0].range.start
    );
}

/// NEGATIVE companion: a genuine `AddToExisting` edit that strict-maps to a real non-`(0,0)` carrier
/// range is STILL accepted verbatim at that mapped range — classify-before-strict must not divert a
/// valid mapped edit. The `AddToExisting` shape is a zero-width insertion inside the user's own
/// import statement: a mapped run PAST the helper-preamble-end boundary, so it is NOT classified as a
/// preamble insertion and keeps the strict verbatim route.
#[test]
fn translate_completion_import_edits_add_to_existing_mapped_edit_is_accepted_verbatim() {
    let (_carrier_source, carrier_li, tsx_li, mapper) = strict_mapped_preamble_fixture();

    // A zero-width insertion at TSX line 1 col 6 — a MAPPED user position (the source map carries a
    // run there → carrier line 2 col 6) PAST the preamble-end boundary (line 1 col 0). This is the
    // `AddToExisting` geometry: extending the user's own import run, strict-mapping to real source.
    let at = tsx_li
        .position_to_offset(&Position {
            line: 1,
            character: 6,
        })
        .expect("a valid mapped TSX offset past the preamble boundary");

    // Discriminating preconditions: this position STRICT-MAPS to a real non-(0,0) carrier range, and
    // it is NOT a preamble insertion (it is past the boundary), so classify-before-strict leaves it
    // on the verbatim route.
    let mapped = crate::type_provider::merge::tsx_range_to_carrier_range(
        at,
        at,
        &tsx_li,
        &mapper,
        &carrier_li,
    )
    .expect("the AddToExisting position must strict-map to real carrier source");
    assert_ne!(
        mapped.start,
        Position {
            line: 0,
            character: 0,
        },
        "fixture precondition: the AddToExisting target maps to a real non-(0,0) carrier position"
    );
    assert!(
        !super::auto_import::is_preamble_import_insertion(at, at, &tsx_li, &mapper),
        "fixture precondition: the AddToExisting position is PAST the preamble boundary (not a \
         preamble insertion)"
    );

    let edits = vec![ProviderImportEdit {
        start: at,
        end: at,
        new_text: ", computed".to_string(),
    }];

    // No anchor supplied: a genuine mapped edit must still succeed verbatim (it never needs an anchor).
    let result = translate_completion_import_edits(&edits, None, &tsx_li, &mapper, &carrier_li);
    let out = result.expect("a genuine mapped AddToExisting edit must be accepted, not rejected");
    assert_eq!(out.len(), 1, "exactly the one verbatim edit: {out:?}");
    assert_eq!(
        out[0].range, mapped,
        "the AddToExisting edit must be applied verbatim at its strict-mapped carrier range"
    );
    assert_eq!(
        out[0].new_text, ", computed",
        "the AddToExisting edit text is carried verbatim"
    );
}

/// A `.svelte`-carrier mapper that publishes the `x_verter_helper_preamble_end` boundary — the
/// geometry the Svelte IDE projector now produces (its `@jsxImportSource`-led prelude registers as
/// the helper preamble, so the map carries the boundary just like Vue). The completion-resolve
/// translator is carrier-AGNOSTIC, so a Svelte carrier with the boundary present must behave exactly
/// like the Vue fixture: an `AddToExisting` zero-width insertion PAST the boundary maps verbatim, and
/// a preamble insertion is re-anchored / fails closed — never over-dropped by the absent-boundary fuse.
///
/// Returns the carrier `.svelte` source, its UTF-16 line index, the TSX UTF-16 line index, and the
/// mapper. The boundary is at generated line 1 col 0 (immediately after the synthetic prelude on line
/// 0); the user script line (generated line 1) maps to the carrier `<script>` body.
fn svelte_boundary_present_fixture() -> (String, LineIndex, LineIndex, ProviderPositionMapper) {
    let carrier_source =
        "<script lang=\"ts\">\nimport { onMount } from 'svelte';\n</script>\n<div>hi</div>\n"
            .to_string();
    // Generated TSX: line 0 the synthetic prelude (unmapped), line 1 the user import line (mapped).
    let tsx_source =
        "/** @jsxImportSource @verter/svelte-jsx */\nimport { onMount } from 'svelte';\n";

    let mut builder = oxc_sourcemap::SourceMapBuilder::default();
    let source_id = builder.set_source_and_content("C.svelte", &carrier_source);
    // The user import line: TSX line 1 → carrier line 1 (the `<script>` body). Map col 0 and col 6
    // so a zero-width insertion at col 6 strict-maps to a real carrier position PAST the boundary.
    builder.add_token(1, 0, 1, 0, Some(source_id), None);
    builder.add_token(1, 6, 1, 6, Some(source_id), None);
    let base_json = builder.into_sourcemap().to_json_string();

    // Publish the boundary the Svelte producer fix now emits: generated line 1 col 0 (right after the
    // single-line prelude on line 0).
    let mut value: serde_json::Value = serde_json::from_str(&base_json).unwrap();
    value["x_verter_helper_preamble_end"] = serde_json::json!({ "line": 1, "character": 0 });
    let json = serde_json::to_string(&value).unwrap();

    let mapper = ProviderPositionMapper::source_map(PositionMapper::from_json(&json).unwrap());
    let carrier_li = LineIndex::new_utf16(&carrier_source);
    let tsx_li = LineIndex::new_utf16(tsx_source);
    (carrier_source, carrier_li, tsx_li, mapper)
}

/// WI-2 (Svelte regression coverage): with the boundary now PRESENT on a `.svelte` carrier map, a
/// genuine `AddToExisting` zero-width insertion that strict-maps to a REAL carrier position PAST the
/// boundary is APPLIED VERBATIM — NOT re-anchored, NOT dropped. This is the case the absent-boundary
/// fuse over-dropped before the producer fix.
///
/// RED is shown structurally by the boundary discriminator: this position is PAST the published
/// boundary, so the classify-before-strict guard leaves it on the verbatim route. With the boundary
/// FLIPPED to `None` (the pre-producer-fix Svelte map), the absent-boundary zero-width fuse would
/// instead divert this exact edit and — absent an anchor — reject the whole resolve. The contrasting
/// assertion below pins that flip.
#[test]
fn translate_completion_import_edits_svelte_add_to_existing_past_boundary_is_accepted_verbatim() {
    let (_carrier_source, carrier_li, tsx_li, mapper) = svelte_boundary_present_fixture();

    // A zero-width insertion at TSX line 1 col 6 — a MAPPED carrier position PAST the boundary
    // (line 1 col 0). The `AddToExisting` geometry: extending the user's own import run.
    let at = tsx_li
        .position_to_offset(&Position {
            line: 1,
            character: 6,
        })
        .expect("a valid mapped TSX offset past the Svelte preamble boundary");

    // DISCRIMINATING preconditions: the position strict-maps to real carrier source, AND it is NOT a
    // preamble insertion (it is past the boundary) — so classify-before-strict keeps the verbatim route.
    let mapped = crate::type_provider::merge::tsx_range_to_carrier_range(
        at,
        at,
        &tsx_li,
        &mapper,
        &carrier_li,
    )
    .expect("the AddToExisting position must strict-map to real Svelte carrier source");
    assert!(
        !super::auto_import::is_preamble_import_insertion(at, at, &tsx_li, &mapper),
        "fixture precondition: the AddToExisting position is PAST the Svelte preamble boundary"
    );

    let edits = vec![ProviderImportEdit {
        start: at,
        end: at,
        new_text: ", onDestroy".to_string(),
    }];

    // BOUNDARY PRESENT (post-producer-fix Svelte map): a genuine mapped edit succeeds verbatim (it
    // never needs an anchor) — NOT diverted, NOT dropped.
    let result = translate_completion_import_edits(&edits, None, &tsx_li, &mapper, &carrier_li);
    let out =
        result.expect("a genuine mapped Svelte AddToExisting edit must be accepted, not rejected");
    assert_eq!(out.len(), 1, "exactly the one verbatim edit: {out:?}");
    assert_eq!(
        out[0].range, mapped,
        "the Svelte AddToExisting edit must be applied verbatim at its strict-mapped carrier range"
    );
    assert_eq!(
        out[0].new_text, ", onDestroy",
        "the Svelte AddToExisting edit text is carried verbatim"
    );

    // CONTRASTING RED WITNESS (the over-drop the producer fix removes): the SAME edit through a
    // boundary-LESS mapper (the pre-producer-fix Svelte map) is DIVERTED by the absent-boundary
    // zero-width fuse and — with no anchor — REJECTED. This proves the boundary's presence is exactly
    // what flips this edit from over-dropped to accepted, i.e. the producer fix is load-bearing here.
    let no_boundary_mapper = {
        let mut builder = oxc_sourcemap::SourceMapBuilder::default();
        // Same carrier + same tokens as the present-boundary fixture, but NO boundary member.
        let sid = builder.set_source_and_content(
            "C.svelte",
            "<script lang=\"ts\">\nimport { onMount } from 'svelte';\n</script>\n<div>hi</div>\n",
        );
        builder.add_token(1, 0, 1, 0, Some(sid), None);
        builder.add_token(1, 6, 1, 6, Some(sid), None);
        let no_boundary_json = builder.into_sourcemap().to_json_string();
        ProviderPositionMapper::source_map(PositionMapper::from_json(&no_boundary_json).unwrap())
    };
    assert!(
        no_boundary_mapper.helper_preamble_end().is_none(),
        "the boundary-less map (pre-producer-fix Svelte) publishes no boundary"
    );
    let red_result =
        translate_completion_import_edits(&edits, None, &tsx_li, &no_boundary_mapper, &carrier_li);
    assert_eq!(
        red_result,
        Err(AutoImportEditMappingError::UnmappableEdit { start: at, end: at }),
        "WITHOUT the boundary the SAME edit is over-dropped (UnmappableEdit) — the regression the \
         producer fix removes; got {red_result:?}"
    );
}

/// WI-2 (Svelte regression coverage, companion): a `.svelte`-carrier preamble insertion (zero-width,
/// classified by the PRESENT boundary) is RE-ANCHORED at the supplied anchor — proving the boundary
/// classifier fires on a Svelte map exactly as on Vue. (Svelte SFCs do not use `<script setup>`; the
/// anchor here is an arbitrary carrier offset standing in for whatever the caller resolved — the
/// translator is anchor-agnostic, it only places the import at the anchor it is handed.)
#[test]
fn translate_completion_import_edits_svelte_preamble_insertion_reanchors_with_anchor() {
    let (carrier_source, carrier_li, tsx_li, mapper) = svelte_boundary_present_fixture();

    // The offset-0 preamble insertion is at/before the boundary (line 1 col 0) ⇒ a preamble insertion.
    assert!(
        super::auto_import::is_preamble_import_insertion(0, 0, &tsx_li, &mapper),
        "fixture precondition: the offset-0 insertion classifies as a Svelte preamble import"
    );

    // An anchor at the carrier `<script>` body start (carrier line 1 col 0).
    let _ = &carrier_source;
    let anchor_offset = carrier_li
        .position_to_offset(&Position {
            line: 1,
            character: 0,
        })
        .expect("a valid carrier offset at the <script> body start");
    let anchor = ScriptImportInsertionAnchor::ExistingScriptSetup {
        offset: anchor_offset,
    };

    let edits = vec![ProviderImportEdit {
        start: 0,
        end: 0,
        new_text: "import { tick } from 'svelte';\n".to_string(),
    }];

    let result =
        translate_completion_import_edits(&edits, Some(&anchor), &tsx_li, &mapper, &carrier_li);
    let out = result.expect("a usable anchor must let the Svelte preamble resolve succeed");
    assert_eq!(out.len(), 1, "exactly one re-anchored import edit: {out:?}");
    assert_eq!(
        out[0].new_text, "import { tick } from 'svelte';\n",
        "the re-anchored Svelte edit carries the provider import text verbatim"
    );
    assert_eq!(
        out[0].range.start,
        Position {
            line: 1,
            character: 0,
        },
        "the Svelte import re-anchors at the supplied anchor (carrier line 1), never (0,0): {:?}",
        out[0].range.start
    );
}

/// WI-3 (== prior codex-A nit 1): the ABSENT-boundary zero-width arm with NO insertion anchor fails
/// closed as `Err(AutoImportEditMappingError::UnmappableEdit)` — the all-or-nothing fail-close when a
/// diverted edit cannot be re-anchored.
///
/// Geometry: a map with NO `x_verter_helper_preamble_end` boundary, and a zero-width edit that
/// STRICT-MAPS to a real non-`(0,0)` carrier position. Under the absent-boundary zero-width fuse the
/// edit is diverted to the shared re-anchor; with no boundary the classifier cannot prove it IS a
/// preamble insertion, so it is reported as the first non-preamble miss ⇒ `UnmappableEdit`.
///
/// DISCRIMINATING: this would NOT be `UnmappableEdit` if the arm wrongly strict-ACCEPTED the
/// zero-width edit (it strict-maps fine), or if the absent-boundary fuse were missing (the edit would
/// take the verbatim route and succeed). The fixture pins both: the precondition asserts the edit
/// strict-maps, and the result asserts it is REJECTED, so a strict-accept regression flips this red.
#[test]
fn translate_completion_import_edits_absent_boundary_zero_width_no_anchor_is_unmappable() {
    // A boundary-LESS carrier map (the pre-producer-fix shape / a non-Verter artifact) whose user line
    // maps to a real carrier position.
    let carrier_source = "<script lang=\"ts\">\nconst base = 1;\n</script>\n".to_string();
    let tsx_source = "const base = 1;\n";

    let mut builder = oxc_sourcemap::SourceMapBuilder::default();
    let source_id = builder.set_source_and_content("C.svelte", &carrier_source);
    // TSX line 0 → carrier line 1 (a real mapped position, NOT the file top).
    builder.add_token(0, 0, 1, 0, Some(source_id), None);
    builder.add_token(0, 6, 1, 6, Some(source_id), None);
    // NOTE: deliberately NO `x_verter_helper_preamble_end` member — the absent-boundary geometry.
    let json = builder.into_sourcemap().to_json_string();
    let mapper = ProviderPositionMapper::source_map(PositionMapper::from_json(&json).unwrap());
    let carrier_li = LineIndex::new_utf16(&carrier_source);
    let tsx_li = LineIndex::new_utf16(tsx_source);

    // A zero-width insertion at TSX line 0 col 6 — STRICT-MAPS to a real carrier position.
    let at = tsx_li
        .position_to_offset(&Position {
            line: 0,
            character: 6,
        })
        .expect("a valid mapped TSX offset");

    // DISCRIMINATING preconditions: NO boundary, yet the edit STRICT-MAPS to real carrier source — so
    // only the absent-boundary zero-width fuse (NOT a strict miss, NOT the preamble classifier) routes
    // it to the all-or-nothing fail-close.
    assert!(
        mapper.helper_preamble_end().is_none(),
        "fixture precondition: the map publishes NO helper-preamble-end boundary"
    );
    assert!(
        !super::auto_import::is_preamble_import_insertion(at, at, &tsx_li, &mapper),
        "fixture precondition: with no boundary the preamble classifier returns false (cannot prove)"
    );
    assert!(
        crate::type_provider::merge::tsx_range_to_carrier_range(
            at,
            at,
            &tsx_li,
            &mapper,
            &carrier_li
        )
        .is_some(),
        "fixture precondition: the zero-width edit STRICT-MAPS to a real carrier position"
    );

    let edits = vec![ProviderImportEdit {
        start: at,
        end: at,
        new_text: "import { x } from 'm';\n".to_string(),
    }];

    // No anchor: the diverted edit cannot be re-anchored, and with no boundary it is a non-preamble
    // miss ⇒ the whole resolve fails closed with UnmappableEdit (all-or-nothing).
    let result = translate_completion_import_edits(&edits, None, &tsx_li, &mapper, &carrier_li);
    assert_eq!(
        result,
        Err(AutoImportEditMappingError::UnmappableEdit { start: at, end: at }),
        "an absent-boundary zero-width edit with no anchor must fail closed as UnmappableEdit \
         (never strict-accepted at the mapped position): {result:?}"
    );
}
