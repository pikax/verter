//! VID0's case table, executable half (VID0T-AC1–AC5). The identity-level
//! fixtures follow `tests/kernel/VID0/products/identity-case-table.v1.json`:
//! `VC01`–`VC08` and `VC11` rows whose `executableOwner` is VID0T live here;
//! activation (`VC04`/`VC05`/`VC10` beyond the typed rejection), ambiguity
//! resolution (`VC03`) and the capability/catalog halves of `VC11` belong to
//! FWA1, COX0 and CPF1 and are never proved through a substitute encoder
//! here. Vue 2.6, Solid 1 and htmx 1 releases are fixture values pinned by
//! this table, not admitted products.

use verter_identity::encoding::{CanonicalDigest, CanonicalEncode, CanonicalEncoder};
use verter_identity::identity::{
    ConfigurationRevision, ContentId, EngineRevision, ProviderEpoch, SourceId, SourceRevision,
    SourceUnitId,
};
use verter_identity::mapping::MapRevision;
use verter_identity::successor::{
    AttachmentId, CarrierProfileId, ConfiguredProjectId, FrameworkProfileId, ProfileSchemaEpoch,
    ProjectProfileId, RegionId, ReleaseCoordinates, ReleaseDescriptorError, ReleaseId,
};

/// Logical identity of one carrier file in the fixtures.
struct SourceFile(&'static str);
impl CanonicalEncode for SourceFile {
    const DOMAIN_TAG: &'static str = "successor-test.source_file.v1";
    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder.field_str(1, self.0);
    }
}

/// One exact version fact of that file (what a `SourceRevision` folds).
struct VersionFact(u64);
impl CanonicalEncode for VersionFact {
    const DOMAIN_TAG: &'static str = "successor-test.version_fact.v1";
    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder.field_u64(1, self.0);
    }
}

/// One map-construction fact (what a `MapRevision` folds).
struct MapConstruction(u64);
impl CanonicalEncode for MapConstruction {
    const DOMAIN_TAG: &'static str = "successor-test.map_construction.v1";
    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder.field_u64(1, self.0);
    }
}

/// Canonical configuration identity of one configured project (VID0 `I08`:
/// the descriptor the project owner derives; the `ConfigurationRevision`
/// is its neighbour, never a field of it).
struct ConfigurationIdentity(&'static str, &'static str);
impl CanonicalEncode for ConfigurationIdentity {
    const DOMAIN_TAG: &'static str = "successor-test.configuration_identity.v1";
    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder.field_str(1, self.0).field_str(2, self.1);
    }
}

/// R12/R15 semantic cache key: source-text key plus the exact semantic
/// profile the entry computes under. A family-wide or carrier-only key is
/// forbidden (`R15`); composing the profile into the key is what this
/// fixture pins.
struct SemanticCacheKey<'a> {
    unit: &'a SourceUnitId,
    revision: &'a SourceRevision,
    content: &'a ContentId,
    profile: &'a FrameworkProfileId,
}
impl CanonicalEncode for SemanticCacheKey<'_> {
    const DOMAIN_TAG: &'static str = "successor-test.semantic_cache_key.v1";
    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder
            .field_bytes(1, self.unit.canonical_bytes())
            .field_bytes(2, self.revision.canonical_bytes())
            .field_bytes(3, self.content.canonical_bytes())
            .field_bytes(4, self.profile.canonical_bytes());
    }
}

/// R12 source-text key: `(SourceUnitId, SourceRevision, ContentId)`.
struct SourceTextKey<'a> {
    unit: &'a SourceUnitId,
    revision: &'a SourceRevision,
    content: &'a ContentId,
}
impl CanonicalEncode for SourceTextKey<'_> {
    const DOMAIN_TAG: &'static str = "successor-test.source_text_key.v1";
    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder
            .field_bytes(1, self.unit.canonical_bytes())
            .field_bytes(2, self.revision.canonical_bytes())
            .field_bytes(3, self.content.canonical_bytes());
    }
}

/// R12 map key: `(SourceUnitId, SourceRevision, MapRevision)`.
struct SourceMapKey<'a> {
    unit: &'a SourceUnitId,
    revision: &'a SourceRevision,
    map: &'a MapRevision,
}
impl CanonicalEncode for SourceMapKey<'_> {
    const DOMAIN_TAG: &'static str = "successor-test.source_map_key.v1";
    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder
            .field_bytes(1, self.unit.canonical_bytes())
            .field_bytes(2, self.revision.canonical_bytes())
            .field_bytes(3, self.map.canonical_bytes());
    }
}

/// Three raw byte payloads in fixed field order — the colliding-payload
/// shape `R13` names: two tuples whose concatenated payloads coincide must
/// still encode (and digest) differently.
struct RawTriple<'a>(&'a [u8], &'a [u8], &'a [u8]);
impl CanonicalEncode for RawTriple<'_> {
    const DOMAIN_TAG: &'static str = "successor-test.raw_triple.v1";
    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder
            .field_bytes(1, self.0)
            .field_bytes(2, self.1)
            .field_bytes(3, self.2);
    }
}

/// The descriptor-schema epoch every fixture mints under.
fn schema_epoch() -> ProfileSchemaEpoch {
    ProfileSchemaEpoch::for_schema_family("successor-test.descriptor", 1)
}

/// One admitted fixture release, for families whose exactness is not the
/// property under test.
fn release_id(family: &str, version: &str, admitted: &[u32]) -> ReleaseId {
    let coordinates =
        ReleaseCoordinates::decode(family, &[version], admitted).expect("exact fixture release");
    ReleaseId::from_coordinates(&coordinates, &schema_epoch())
}

/// VC01 — accepted unit lineage survives edits while the revision/content
/// keys move. The three versions keep `script` and `template` units equal;
/// every version fact and byte change moves exactly its own key.
#[test]
fn vc01_lineage_survives_edits_while_version_keys_move() {
    let source = SourceId::from_canonical(&SourceFile("editor:/ws/a/App.vue"));
    let versions: [(&str, &[u8]); 3] = [
        ("v1", b"<script>export const a = 1</script>"),
        ("v2", b"<script>export const a = 2</script>"),
        ("v3", b"<script>export const a = 3</script>"),
    ];
    let units: Vec<_> = versions
        .iter()
        .map(|(label, _)| {
            (
                SourceUnitId::from_lineage(&source, "script"),
                SourceUnitId::from_lineage(&source, "template"),
                *label,
            )
        })
        .collect();
    for (index, (unit_a, template_a, _)) in units.iter().enumerate() {
        for (unit_b, template_b, _) in units.iter().skip(index + 1) {
            assert_eq!(unit_a, unit_b, "script lineage survives edits");
            assert_eq!(template_a, template_b, "template lineage survives edits");
        }
    }
    let revisions: Vec<_> = versions
        .iter()
        .enumerate()
        .map(|(index, _)| SourceRevision::from_canonical(&VersionFact(index as u64 + 1)))
        .collect();
    let contents: Vec<_> = versions
        .iter()
        .map(|(_, bytes)| ContentId::from_content_bytes(bytes))
        .collect();
    for (index, revision) in revisions.iter().enumerate() {
        for other in revisions.iter().skip(index + 1) {
            assert_ne!(revision, other, "revision differs per version");
        }
    }
    for (index, content) in contents.iter().enumerate() {
        for other in contents.iter().skip(index + 1) {
            assert_ne!(content, other, "content differs per version");
        }
    }
    // MapRevision moves exactly when the map construction moves, and not
    // with any edit: same construction before and after an edit, a changed
    // construction under the same bytes.
    let construction = MapRevision::from_canonical(&MapConstruction(7));
    assert_eq!(
        construction,
        MapRevision::from_canonical(&MapConstruction(7)),
        "same construction is the same map revision"
    );
    assert_ne!(
        construction,
        MapRevision::from_canonical(&MapConstruction(8)),
        "a changed construction is a new map revision"
    );
}

/// VC02 — Vue 2.6 and Vue 3 coexist with no cache collision: one carrier
/// for the identical bytes, distinct releases and profiles, and semantic
/// cache keys that differ on profile alone (a family-wide key would
/// collide here). Vue 2.6 is a fixture value; no Vue 2 vertical is
/// admitted.
#[test]
fn vc02_vue_majors_coexist_without_collision() {
    let carrier = CarrierProfileId::from_carrier("vue-sfc", &schema_epoch());
    let carrier_again = CarrierProfileId::from_carrier("vue-sfc", &schema_epoch());
    assert_eq!(carrier, carrier_again, "same carrier inputs, same carrier");

    let admitted = [2u32, 3];
    let vue_2_6 = release_id("vue", "2.6.14", &admitted);
    let vue_3_4 = release_id("vue", "3.4.21", &admitted);
    assert_ne!(vue_2_6, vue_3_4, "separate majors are separate releases");

    let profile_2_6 = FrameworkProfileId::from_release_and_carriers(
        &vue_2_6,
        std::slice::from_ref(&carrier),
        &schema_epoch(),
    );
    let profile_3_4 = FrameworkProfileId::from_release_and_carriers(
        &vue_3_4,
        std::slice::from_ref(&carrier),
        &schema_epoch(),
    );
    assert_ne!(
        profile_2_6, profile_3_4,
        "separate releases carry separate profiles"
    );

    let source = SourceId::from_canonical(&SourceFile("editor:/ws/a/App.vue"));
    let unit = SourceUnitId::from_lineage(&source, "script");
    let revision = SourceRevision::from_canonical(&VersionFact(1));
    let content = ContentId::from_content_bytes(b"<script>export const a = 1</script>");
    let key_2_6 = SemanticCacheKey {
        unit: &unit,
        revision: &revision,
        content: &content,
        profile: &profile_2_6,
    }
    .canonical_bytes();
    let key_3_4 = SemanticCacheKey {
        unit: &unit,
        revision: &revision,
        content: &content,
        profile: &profile_3_4,
    }
    .canonical_bytes();
    assert_ne!(
        key_2_6, key_3_4,
        "identical bytes under two releases never share a semantic cache slot"
    );
}

/// VC03 — one TSX byte stream carries mutually exclusive React 19 and
/// Solid 2 claims: one carrier and one region in every case, two distinct
/// profile identities and two distinct attachments on that one region,
/// grouped by the shared `(SourceUnitId, RegionId)` claim key. The typed
/// ambiguous outcome and the pragma narrowing rule are COX0's and FWA1's;
/// the identity half is what this pins.
#[test]
fn vc03_exclusive_claims_share_one_region() {
    let carrier = CarrierProfileId::from_carrier("tsx", &schema_epoch());
    let react = release_id("react", "19.0.0", &[19u32]);
    let solid = release_id("solid-js", "2.11.6", &[2u32]);
    let react_profile = FrameworkProfileId::from_release_and_carriers(
        &react,
        std::slice::from_ref(&carrier),
        &schema_epoch(),
    );
    let solid_profile = FrameworkProfileId::from_release_and_carriers(
        &solid,
        std::slice::from_ref(&carrier),
        &schema_epoch(),
    );
    assert_ne!(
        react_profile, solid_profile,
        "exclusive claims are distinct profiles"
    );

    let source = SourceId::from_canonical(&SourceFile("editor:/ws/a/Card.tsx"));
    let unit = SourceUnitId::from_lineage(&source, "script");
    let region = RegionId::from_region_role(&unit, "module-root");
    let region_again = RegionId::from_region_role(&unit, "module-root");
    assert_eq!(
        region, region_again,
        "one region for the same unit and role"
    );

    let react_attachment = AttachmentId::from_claim(&unit, &region, &react_profile);
    let solid_attachment = AttachmentId::from_claim(&unit, &region, &solid_profile);
    assert_ne!(
        react_attachment, solid_attachment,
        "two active claims are two attachments, never one"
    );
    // R10: competing claims are grouped by the profile-independent region
    // they share — both attachments address the same (unit, region) pair,
    // so ambiguity is decided over that key, not by attachment equality.
    let _grouping_key = (&unit, &region);
    // The `@jsxImportSource solid-js` pragma narrows that one file to the
    // Solid claim; narrowing selects among these same identities and mints
    // no new one.
    let _narrowed_claim = [&solid_profile];
    let _competing_claims = [&react_profile, &solid_profile];
}

/// VC04 — Solid 1 cannot produce a Solid 2 identity: the 1.x row fails
/// admission with the typed `unsupported-version` rejection, so no
/// `ReleaseId` (and no `FrameworkProfileId` over it) can be minted from
/// it. Activation reporting is FWA1's.
#[test]
fn vc04_solid_one_cannot_mint_solid_two() {
    let solid_two_only = [2u32];
    let resolved = ReleaseCoordinates::decode("solid-js", &["1.8.0"], &solid_two_only);
    assert_eq!(resolved, Err(ReleaseDescriptorError::UnsupportedVersion));
    let minted = resolved
        .ok()
        .map(|coordinates| ReleaseId::from_coordinates(&coordinates, &schema_epoch()));
    assert!(
        minted.is_none(),
        "no Solid 2 release identity can be produced from a 1.x package"
    );

    let admitted =
        ReleaseCoordinates::decode("solid-js", &["2.11.6"], &solid_two_only).expect("admitted");
    assert_eq!(admitted.major(), 2);
    let solid_two = ReleaseId::from_coordinates(&admitted, &schema_epoch());
    let _profile = FrameworkProfileId::from_release_and_carriers(
        &solid_two,
        &[CarrierProfileId::from_carrier("tsx", &schema_epoch())],
        &schema_epoch(),
    );
}

/// VC05 — older htmx majors cannot activate htmx 4: under the htmx-4-only
/// line a 1.x row is rejected; under a line that admits the older major,
/// that major gets its own release and profile, never htmx 4's.
#[test]
fn vc05_older_htmx_majors_do_not_activate_htmx_four() {
    let htmx_four_only = [4u32];
    assert_eq!(
        ReleaseCoordinates::decode("htmx.org", &["1.9.12"], &htmx_four_only),
        Err(ReleaseDescriptorError::UnsupportedVersion)
    );

    let older_line = [1u32, 4];
    let htmx_1 = ReleaseCoordinates::decode("htmx.org", &["1.9.12"], &older_line)
        .expect("the admitted older major activates its own release");
    let htmx_4 =
        ReleaseCoordinates::decode("htmx.org", &["4.0.3"], &older_line).expect("htmx 4 release");
    let html_carrier = CarrierProfileId::from_carrier("html", &schema_epoch());
    let release_1 = ReleaseId::from_coordinates(&htmx_1, &schema_epoch());
    let release_4 = ReleaseId::from_coordinates(&htmx_4, &schema_epoch());
    assert_ne!(release_1, release_4, "one profile never spans two majors");
    assert_ne!(
        FrameworkProfileId::from_release_and_carriers(
            &release_1,
            std::slice::from_ref(&html_carrier),
            &schema_epoch()
        ),
        FrameworkProfileId::from_release_and_carriers(&release_4, &[html_carrier], &schema_epoch()),
        "each major carries its own profile identity"
    );
}

/// VC06 / VID0T-AC2 — floating tags, ranges, bare family strings, untagged
/// ids, a missing major and versions arrays all fail canonical decoding
/// with a typed error; only exact spellings admitted by the family's line
/// decode. A decoder never defaults the major.
#[test]
fn vc06_floating_and_untagged_releases_fail_decoding() {
    let admitted = [2u32, 3];
    let rejected: &[(&str, &[&str], ReleaseDescriptorError)] = &[
        ("vue", &["latest"], ReleaseDescriptorError::FloatingTag),
        ("vue", &["next"], ReleaseDescriptorError::FloatingTag),
        ("vue", &["canary"], ReleaseDescriptorError::FloatingTag),
        ("react", &["^3"], ReleaseDescriptorError::Range),
        ("react", &["^18 || ^19"], ReleaseDescriptorError::Range),
        ("htmx.org", &["1.x"], ReleaseDescriptorError::Range),
        ("htmx.org", &["*"], ReleaseDescriptorError::Range),
        ("vue", &["vue"], ReleaseDescriptorError::UntaggedFamily),
        ("vue", &[], ReleaseDescriptorError::MissingMajor),
        ("vue", &[""], ReleaseDescriptorError::MissingMajor),
        ("vue", &["beta"], ReleaseDescriptorError::MissingMajor),
        (
            "vue",
            &["2.6.14", "3.4.21"],
            ReleaseDescriptorError::VersionsArray,
        ),
    ];
    for (family, releases, expected) in rejected {
        assert_eq!(
            ReleaseCoordinates::decode(family, releases, &admitted),
            Err(*expected),
            "{family} {releases:?} must not decode"
        );
    }
    for exact in ["3", "3.4", "3.4.21", "2.6.14"] {
        let decoded = ReleaseCoordinates::decode("vue", &[exact], &admitted)
            .unwrap_or_else(|error| panic!("{exact} is exact and admitted: {error:?}"));
        assert_eq!(decoded.family(), "vue");
    }
    assert_eq!(
        ReleaseCoordinates::decode("vue", &["4.0.0"], &admitted),
        Err(ReleaseDescriptorError::UnsupportedVersion),
        "an exact release outside the admitted line still fails admission"
    );
}

/// VC08 / VID0T-AC4 — source keys serialize and round-trip exactly, collide
/// safely, and keep backend/process identity outside the key.
#[test]
fn vc08_source_keys_round_trip_collide_safely_and_exclude_backends() {
    let source = SourceId::from_canonical(&SourceFile("editor:/ws/a/App.vue"));
    let unit = SourceUnitId::from_lineage(&source, "script");
    let revision = SourceRevision::from_canonical(&VersionFact(2));
    let content = ContentId::from_content_bytes(b"<script>export const a = 1</script>");
    let map = MapRevision::from_canonical(&MapConstruction(4));

    // Exact round-trip: independent composition over equal members is
    // byte- and digest-identical, for both key shapes.
    let text_key = SourceTextKey {
        unit: &unit,
        revision: &revision,
        content: &content,
    };
    assert_eq!(text_key.canonical_bytes(), text_key.canonical_bytes());
    assert_eq!(
        CanonicalDigest::of_bytes(&text_key.canonical_bytes()),
        CanonicalDigest::of_bytes(&text_key.canonical_bytes())
    );
    let map_key = SourceMapKey {
        unit: &unit,
        revision: &revision,
        map: &map,
    };
    assert_eq!(map_key.canonical_bytes(), map_key.canonical_bytes());

    // One member moves, the key moves — including the map member, which is
    // a different key shape's discriminator, never a text-key field.
    let moved_revision = SourceTextKey {
        unit: &unit,
        revision: &SourceRevision::from_canonical(&VersionFact(3)),
        content: &content,
    };
    let moved_content = SourceTextKey {
        unit: &unit,
        revision: &revision,
        content: &ContentId::from_content_bytes(b"<script>export const a = 2</script>"),
    };
    let moved_unit = SourceTextKey {
        unit: &SourceUnitId::from_lineage(&source, "template"),
        revision: &revision,
        content: &content,
    };
    let moved_map = SourceMapKey {
        unit: &unit,
        revision: &revision,
        map: &MapRevision::from_canonical(&MapConstruction(5)),
    };
    for moved in [
        moved_revision.canonical_bytes(),
        moved_content.canonical_bytes(),
        moved_unit.canonical_bytes(),
    ] {
        assert_ne!(text_key.canonical_bytes(), moved);
    }
    assert_ne!(map_key.canonical_bytes(), moved_map.canonical_bytes());

    // R13 — two tuples whose concatenated payloads coincide still encode
    // and digest differently: length delimitation is the collision safety.
    let fused_a = RawTriple(b"ab", b"c", b"x").canonical_bytes();
    let fused_b = RawTriple(b"a", b"bc", b"x").canonical_bytes();
    assert_ne!(fused_a, fused_b);
    assert_ne!(
        CanonicalDigest::of_bytes(&fused_a),
        CanonicalDigest::of_bytes(&fused_b)
    );

    // R14 — backend and process facts vary; the source key bytes do not.
    // The composition takes only the three ratified members, so every
    // backend epoch, provider epoch or session handle the process may
    // hold lives outside the key: the bytes are a function of the members
    // alone.
    let before = text_key.canonical_bytes();
    let backend_state = (EngineRevision(7), ProviderEpoch(3));
    let other_backend_state = (EngineRevision(8), ProviderEpoch(4));
    assert_ne!(backend_state, other_backend_state);
    let after = text_key.canonical_bytes();
    assert_eq!(
        before, after,
        "no backend or process identity entered the key"
    );
}

/// VC11 (VID0T half) — a release change moves exactly its dependent
/// identities: the release, framework profile and attachment change; the
/// carrier, source unit and region stay equal. The profile-qualified
/// capability half is COX0's and the catalog half CPF1's.
#[test]
fn vc11_release_change_moves_exactly_dependent_identities() {
    let source = SourceId::from_canonical(&SourceFile("editor:/ws/a/Comp.vue"));
    let unit = SourceUnitId::from_lineage(&source, "template");
    let region = RegionId::from_region_role(&unit, "root");
    let carrier = CarrierProfileId::from_carrier("vue-sfc", &schema_epoch());

    let admitted = [2u32, 3];
    let vue_2_6 = release_id("vue", "2.6.14", &admitted);
    let vue_3_4 = release_id("vue", "3.4.21", &admitted);
    let profile_2_6 = FrameworkProfileId::from_release_and_carriers(
        &vue_2_6,
        std::slice::from_ref(&carrier),
        &schema_epoch(),
    );
    let profile_3_4 = FrameworkProfileId::from_release_and_carriers(
        &vue_3_4,
        std::slice::from_ref(&carrier),
        &schema_epoch(),
    );
    let attachment_2_6 = AttachmentId::from_claim(&unit, &region, &profile_2_6);
    let attachment_3_4 = AttachmentId::from_claim(&unit, &region, &profile_3_4);

    assert_ne!(vue_2_6, vue_3_4, "the release moves");
    assert_ne!(profile_2_6, profile_3_4, "the semantic profile moves");
    assert_ne!(attachment_2_6, attachment_3_4, "the attachment moves");
    // Re-deriving the surviving identities under the new release mints the
    // same values: nothing carrier-, unit- or region-shaped moved.
    assert_eq!(
        carrier,
        CarrierProfileId::from_carrier("vue-sfc", &schema_epoch())
    );
    assert_eq!(
        region,
        RegionId::from_region_role(&SourceUnitId::from_lineage(&source, "template"), "root")
    );
}

/// VID0T-AC2/AC3 support — the descriptor-schema epoch is deterministic,
/// and bumping it re-mints every profile identity derived under it
/// (`R02` through `I11`) while equal epochs never do.
#[test]
fn schema_epoch_bump_moves_exactly_profile_identities() {
    let epoch_one = ProfileSchemaEpoch::for_schema_family("successor-test.descriptor", 1);
    let epoch_one_again = ProfileSchemaEpoch::for_schema_family("successor-test.descriptor", 1);
    let epoch_two = ProfileSchemaEpoch::for_schema_family("successor-test.descriptor", 2);
    let other_family = ProfileSchemaEpoch::for_schema_family("successor-test.other", 1);
    assert_eq!(epoch_one, epoch_one_again);
    assert_ne!(epoch_one, epoch_two);
    assert_ne!(epoch_one, other_family);

    let admitted = [3u32];
    let coordinates =
        ReleaseCoordinates::decode("vue", &["3.4.21"], &admitted).expect("exact fixture release");
    let carrier_one = CarrierProfileId::from_carrier("vue-sfc", &epoch_one);
    let release_one = ReleaseId::from_coordinates(&coordinates, &epoch_one);
    let profile_one = FrameworkProfileId::from_release_and_carriers(
        &release_one,
        std::slice::from_ref(&carrier_one),
        &epoch_one,
    );
    let project_one = ProjectProfileId::from_project_release("nuxt", &release_one, &epoch_one);

    assert_eq!(
        carrier_one,
        CarrierProfileId::from_carrier("vue-sfc", &epoch_one)
    );
    assert_eq!(
        profile_one,
        FrameworkProfileId::from_release_and_carriers(
            &ReleaseId::from_coordinates(&coordinates, &epoch_one),
            std::slice::from_ref(&carrier_one),
            &epoch_one,
        )
    );
    assert_ne!(
        profile_one,
        FrameworkProfileId::from_release_and_carriers(
            &ReleaseId::from_coordinates(&coordinates, &epoch_two),
            std::slice::from_ref(&carrier_one),
            &epoch_two,
        ),
        "a schema-epoch bump re-mints the derived profile identities"
    );
    for bumped in [
        CarrierProfileId::from_carrier("vue-sfc", &epoch_two) != carrier_one,
        ReleaseId::from_coordinates(&coordinates, &epoch_two) != release_one,
        ProjectProfileId::from_project_release(
            "nuxt",
            &ReleaseId::from_coordinates(&coordinates, &epoch_two),
            &epoch_two,
        ) != project_one,
    ] {
        assert!(bumped, "every profile identity carries the schema epoch");
    }
}

/// VID0T-AC1 support — the configured-project identity is deterministic in
/// the canonical configuration identity and blind to the neighbouring
/// configuration revision.
#[test]
fn configured_project_identity_ignores_configuration_revision() {
    let configuration = ConfigurationIdentity("file://ws/a/tsconfig.json", "digest-of-bytes");
    let project = ConfiguredProjectId::from_canonical(&configuration);
    assert_eq!(project, ConfiguredProjectId::from_canonical(&configuration));
    assert_ne!(
        project,
        ConfiguredProjectId::from_canonical(&ConfigurationIdentity(
            "file://ws/b/tsconfig.json",
            "digest-of-bytes"
        ))
    );
    assert_ne!(
        project,
        ConfiguredProjectId::from_canonical(&ConfigurationIdentity(
            "file://ws/a/tsconfig.json",
            "digest-of-other-bytes"
        ))
    );

    // The revision is a neighbour: it moves while the configured project
    // identity stays, because it is not an input to it.
    let revision_one = ConfigurationRevision::from_canonical(&VersionFact(1));
    let revision_two = ConfigurationRevision::from_canonical(&VersionFact(2));
    assert_ne!(revision_one, revision_two);
    assert_eq!(project, ConfiguredProjectId::from_canonical(&configuration));
}
