//! Sealed artifact-bound block identity: mint authority, owner validation,
//! and content-addressed (generation-free) artifact identity.

use std::sync::Arc;

use verter_language::parse_artifact::carrier_inventory::{
    BlockId, CarrierBlock, CarrierBlockInventory, InternedNameId, MarkupSyntaxArena,
    NormalizedNameTable, SectionRole, SourceSlice, SourceSpaceDescriptor, SourceSpaceId,
    SourceSpan, StyleDialect, StyleModule, SyntaxTermination, TaggedSyntax,
};
use verter_language::registered_source_authority::{
    CanonicalFileId, FileIncarnation, RegisteredSourceAuthority, SourceGeneration,
};
use verter_language::FileLanguage;

/// Build a two-style-block inventory over `"<style>.X{}</style><style>.Y{}</style>"`
/// registered at the given generation. Geometry is fixed; only the class
/// letters (content bytes) and the registration generation vary.
fn style_inventory(class_a: char, class_b: char, generation: u64) -> CarrierBlockInventory {
    let source_text = format!("<style>.{class_a}{{}}</style><style>.{class_b}{{}}</style>");
    let authority = RegisteredSourceAuthority::new().expect("source authority");
    let snapshot = authority
        .register_source(
            CanonicalFileId::new("file:///workspace/App.vue"),
            FileIncarnation::new(1),
            SourceGeneration::new(generation),
            FileLanguage::vue(),
            Arc::from(source_text.as_str()),
        )
        .expect("registered source");

    let space = SourceSpaceId(0);
    let span = |start: u32, end: u32| SourceSpan::new(space, start, end);
    let style_section = |id: u32, base: u32| CarrierBlock::Section {
        id: BlockId(id),
        role: SectionRole::Style {
            dialect: StyleDialect::Css,
            scoped: false,
            module: StyleModule::None,
        },
        syntax: TaggedSyntax {
            authored_name: SourceSlice::new(span(base + 1, base + 6)),
            normalized_name: InternedNameId(0),
            opening_span: span(base, base + 7),
            opening_name_span: span(base + 1, base + 6),
            attribute_insertion_anchor: span(base + 6, base + 6),
            content_span: span(base + 7, base + 11),
            closing_span: Some(span(base + 11, base + 19)),
            closing_name_span: Some(span(base + 13, base + 18)),
            full_span: span(base, base + 19),
            termination: SyntaxTermination::Closed,
            attributes: Arc::from([]),
        },
    };

    CarrierBlockInventory::new(
        Arc::from([SourceSpaceDescriptor::registered(space, &snapshot)]),
        Arc::new(NormalizedNameTable {
            values: Arc::from([Arc::from("style")]),
        }),
        Arc::from([style_section(0, 0), style_section(1, 19)]),
        Arc::new(MarkupSyntaxArena::default()),
    )
    .expect("valid fixture inventory")
}

#[test]
fn sealed_block_ref_mints_only_existing_blocks_and_validates_owner() {
    let inventory = style_inventory('a', 'b', 2);

    let first = inventory.block_ref(BlockId(0)).expect("existing block");
    let second = inventory.block_ref(BlockId(1)).expect("existing block");
    assert!(
        inventory.block_ref(BlockId(5)).is_none(),
        "mint authority refuses out-of-range blocks"
    );

    assert_eq!(first.block_id(), BlockId(0));
    assert_eq!(first.artifact_identity(), second.artifact_identity());
    assert_ne!(first, second, "block id is part of the sealed identity");
    assert!(first.validate(&inventory));
    assert!(second.validate(&inventory));
}

#[test]
fn foreign_artifact_ref_fails_owner_validation() {
    let inventory = style_inventory('a', 'b', 2);
    // Same block geometry, different content bytes: a different artifact.
    let foreign = style_inventory('b', 'a', 2);

    let stale = foreign.block_ref(BlockId(0)).expect("existing block");
    assert_ne!(
        stale.artifact_identity(),
        inventory.block_ref(BlockId(0)).unwrap().artifact_identity(),
        "content change must change the artifact identity"
    );
    assert!(
        !stale.validate(&inventory),
        "a ref minted by a different artifact must fail closed"
    );
    assert_ne!(
        stale,
        inventory.block_ref(BlockId(0)).unwrap(),
        "full-identity equality must reject the same local id on a \
         different artifact"
    );
}

#[test]
fn artifact_identity_is_content_addressed_not_generation_bound() {
    let inventory = style_inventory('a', 'b', 2);
    // Identical bytes and geometry re-registered at a different generation.
    let reregistered = style_inventory('a', 'b', 9);

    assert_eq!(
        inventory.artifact_identity_token(),
        reregistered.artifact_identity_token(),
        "identity is content-addressed: same bytes + same geometry \
         => same identity regardless of registration generation"
    );
    let ref_before = inventory.block_ref(BlockId(1)).unwrap();
    assert!(
        ref_before.validate(&reregistered),
        "content-identical re-registration still owns the ref"
    );
    assert_eq!(
        ref_before,
        reregistered.block_ref(BlockId(1)).unwrap(),
        "sealed refs joined across content-identical registrations"
    );
}
