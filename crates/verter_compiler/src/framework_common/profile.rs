//! Framework and carrier profile identities of the catalog.
//!
//! A [`FrameworkProfileManifest`] is the identity half of a semantic-profile
//! row: the exact release one framework profile is the meaning of, and the
//! carriers it claims. The catalog keys its rows by the profiles minted here,
//! and the session's adapter registry mints its advertised profiles from the
//! same manifests through the same function, so the two can never disagree.

use verter_identity::successor::{
    CarrierProfileId, FrameworkProfileId, ProfileSchemaEpoch, ReleaseCoordinates,
    ReleaseDescriptorError, ReleaseId,
};
use verter_language::LanguageId;

/// The descriptor-schema family of every built-in semantic-profile
/// manifest: the epoch behind each minted [`FrameworkProfileId`]. Bumping
/// [`FRAMEWORK_PROFILE_DESCRIPTOR_SCHEMA_EPOCH`] re-mints every advertised
/// profile and nothing else. It is a descriptor epoch, not the TypeInfo wire
/// schema version.
pub const FRAMEWORK_PROFILE_DESCRIPTOR_SCHEMA_FAMILY: &str = "verter.framework-profile.descriptor";

/// The current epoch of [`FRAMEWORK_PROFILE_DESCRIPTOR_SCHEMA_FAMILY`].
pub const FRAMEWORK_PROFILE_DESCRIPTOR_SCHEMA_EPOCH: u32 = 1;

/// The syntax-schema family of every claimed carrier profile. Its epoch is
/// the registered parser-grammar version the catalog stamps on each carrier,
/// so a grammar bump re-mints the carrier profiles (and through them the
/// semantic profiles that claim them); a release change never does.
pub const CARRIER_SYNTAX_SCHEMA_FAMILY: &str = "verter.carrier.syntax";

/// The registered parser-grammar version of every built-in carrier: the
/// epoch of [`CARRIER_SYNTAX_SCHEMA_FAMILY`] the built-in carrier and
/// framework profiles are minted under.
pub const CARRIER_PARSER_GRAMMAR_VERSION: u32 = 1;

/// One semantic profile's manifest: the inputs, and only the inputs, a
/// [`FrameworkProfileId`] is minted from.
///
/// The manifest declares exactly one EXACT release of one family, the majors
/// the family's admitted line allows, and the carrier families the profile
/// claims. Nothing here is derived from the adapter id, a display tag, a file
/// extension or an implementation version: those are presentation or
/// implementation metadata, never a release. A declaration the release law
/// rejects — a range, a floating tag, a versions array, a bare family, an
/// unadmitted major — fails registration with its typed
/// [`ReleaseDescriptorError`]; it is never coerced into a profile and never
/// reaches a request.
///
/// The declared release means exactly those coordinates: `"3"`, `"3.0"` and
/// `"3.0.0"` mint three different profiles, and an admitted major does not
/// make every `3.x.y` package an instance of `"3"`. Reading the installed
/// package release and per-package activation feed this declaration; they
/// are not a second one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameworkProfileManifest {
    /// The semantic family key the release belongs to (`"vue"`, `"svelte"`).
    pub family: &'static str,
    /// The one exact release this profile is the meaning of —
    /// `major[.minor[.patch]]`, never a range or a floating tag.
    pub release: &'static str,
    /// The exact majors the family's admitted line allows. A release whose
    /// major is outside it is `unsupported-version` at construction.
    pub admitted_majors: &'static [u32],
    /// The carrier families this profile claims (its component carriers),
    /// each minted as a [`CarrierProfileId`] under the carrier syntax
    /// epoch. Order and repetition are not identity.
    pub claimed_carriers: &'static [&'static str],
}

impl FrameworkProfileManifest {
    /// Mint the profile this manifest declares: decode the exact release
    /// under the admitted line, mint its [`ReleaseId`] and the claimed
    /// [`CarrierProfileId`]s under `carrier_syntax_schema`, and compose the
    /// [`FrameworkProfileId`] under the current descriptor epoch. The only
    /// minting site — a profile without its release cannot exist, and a
    /// rejected declaration is a typed error, never a profile.
    pub fn mint(
        &self,
        carrier_syntax_schema: &ProfileSchemaEpoch,
    ) -> Result<FrameworkProfileId, ReleaseDescriptorError> {
        let schema = framework_profile_descriptor_schema();
        let coordinates =
            ReleaseCoordinates::decode(self.family, &[self.release], self.admitted_majors)?;
        let release = ReleaseId::from_coordinates(&coordinates, &schema);
        let carriers: Vec<CarrierProfileId> = self
            .claimed_carriers
            .iter()
            .map(|carrier| CarrierProfileId::from_carrier(carrier, carrier_syntax_schema))
            .collect();
        Ok(FrameworkProfileId::from_release_and_carriers(
            &release, &carriers, &schema,
        ))
    }
}

/// The current semantic descriptor-schema epoch every built-in profile is
/// minted under.
#[must_use]
pub fn framework_profile_descriptor_schema() -> ProfileSchemaEpoch {
    ProfileSchemaEpoch::for_schema_family(
        FRAMEWORK_PROFILE_DESCRIPTOR_SCHEMA_FAMILY,
        FRAMEWORK_PROFILE_DESCRIPTOR_SCHEMA_EPOCH,
    )
}

/// The carrier syntax-schema epoch for a registered parser-grammar version.
#[must_use]
pub fn carrier_syntax_schema(parser_grammar_version: u32) -> ProfileSchemaEpoch {
    ProfileSchemaEpoch::for_schema_family(CARRIER_SYNTAX_SCHEMA_FAMILY, parser_grammar_version)
}

/// The carrier profile of a built-in carrier language, under the built-in
/// parser-grammar version.
#[must_use]
pub fn carrier_profile(carrier_language_id: &LanguageId) -> CarrierProfileId {
    CarrierProfileId::from_carrier(
        carrier_language_id.as_str(),
        &carrier_syntax_schema(CARRIER_PARSER_GRAMMAR_VERSION),
    )
}

/// The framework profile a manifest declares, under the built-in
/// parser-grammar version.
pub fn framework_profile(
    manifest: &FrameworkProfileManifest,
) -> Result<FrameworkProfileId, ReleaseDescriptorError> {
    manifest.mint(&carrier_syntax_schema(CARRIER_PARSER_GRAMMAR_VERSION))
}
