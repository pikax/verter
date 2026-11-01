//! Exact-release law (VID0 `I04`, `R04`–`R07`, `R11`): release
//! coordinates decode from one manifest row or fail with a typed error,
//! and [`ReleaseId`] mints only from decoded coordinates.

use crate::canonical::Canonical;
use crate::encoding::{CanonicalEncode, CanonicalEncoder};

use super::ProfileSchemaEpoch;

digest_identity!(
    /// Exact semantic-release identity (VID0 `I04`): one exact release,
    /// minted from admitted [`ReleaseCoordinates`] and the
    /// descriptor-schema epoch. Never a range, a versions array, a
    /// floating tag or an implied major.
    ReleaseId
);

impl ReleaseId {
    const DOMAIN_TAG: &'static str = "verter.identity.successor.release.v1";

    /// Mint from exact, already-admitted coordinates and the
    /// descriptor-schema epoch. The coordinates carry the family's
    /// admitted line, so a lock change re-mints the release identities it
    /// admits (`I04`).
    pub fn from_coordinates(coordinates: &ReleaseCoordinates, schema: &ProfileSchemaEpoch) -> Self {
        let mut encoder = CanonicalEncoder::new(Self::DOMAIN_TAG);
        encoder.field_bytes(1, coordinates.canonical_digest().as_bytes());
        encoder.field_bytes(2, schema.digest().as_bytes());
        Self(Canonical::from_encoder(&encoder))
    }
}

/// Typed rejection of an inexact release descriptor row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseDescriptorError {
    /// `R04`: the row carries two releases or a versions array — a
    /// structural failure, never two identities.
    VersionsArray,
    /// `R07`: `latest`, `next`, `canary` or another floating tag.
    FloatingTag,
    /// `R07`: a range or wildcard comparator.
    Range,
    /// `R07`: a bare family string — the row names no release.
    UntaggedFamily,
    /// `R06`: the row's release is absent or is not an exact
    /// `major[.minor[.patch]]`, so there is no exact major to mint from —
    /// an implicit default major is never coerced.
    MissingMajor,
    /// `R06`/`R11`: the resolved major is outside the family's admitted
    /// line; it is never coerced into an admitted release.
    UnsupportedVersion,
}

/// Exact, admitted release coordinates (VID0 `I04`): family key, exact
/// version, and the admitted line the family's stage-0 lock pinned when it
/// admitted them. Fields are private and coordinates arise only through
/// [`Self::decode`], so no floating, ranged, untagged or unadmitted
/// spelling can reach a [`ReleaseId`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseCoordinates {
    family: String,
    major: u32,
    minor: Option<u32>,
    patch: Option<u32>,
    admitted_majors: Vec<u32>,
}

impl ReleaseCoordinates {
    /// Decode one manifest row: the family key, the release strings the
    /// row carries, and the exact majors the family's stage-0 lock
    /// admits. One row names one release (`R04`); a floating tag, range,
    /// bare family string, absent major (`R06`, `R07`) or unadmitted
    /// major (`R11`) is a typed error, never a coerced identity.
    pub fn decode(
        family: &str,
        releases: &[&str],
        admitted_majors: &[u32],
    ) -> Result<Self, ReleaseDescriptorError> {
        let release = match releases {
            [only] => *only,
            [] => return Err(ReleaseDescriptorError::MissingMajor),
            _ => return Err(ReleaseDescriptorError::VersionsArray),
        };
        if release == family {
            return Err(ReleaseDescriptorError::UntaggedFamily);
        }
        if matches!(release, "latest" | "next" | "canary") {
            return Err(ReleaseDescriptorError::FloatingTag);
        }
        if release
            .chars()
            .any(|c| matches!(c, '^' | '~' | '>' | '<' | '=' | '*' | 'x' | 'X' | '|' | ' '))
        {
            return Err(ReleaseDescriptorError::Range);
        }
        let mut parts = release.split('.');
        let major = parse_version_part(parts.next().unwrap_or_default())?;
        let minor = parts.next().map(parse_version_part).transpose()?;
        let patch = parts.next().map(parse_version_part).transpose()?;
        if parts.next().is_some() {
            return Err(ReleaseDescriptorError::MissingMajor);
        }
        if !admitted_majors.contains(&major) {
            return Err(ReleaseDescriptorError::UnsupportedVersion);
        }
        Ok(Self {
            family: family.to_owned(),
            major,
            minor,
            patch,
            admitted_majors: admitted_majors.to_vec(),
        })
    }

    /// The framework family key the coordinates were decoded under.
    pub fn family(&self) -> &str {
        &self.family
    }

    /// The exact major — always admitted, never implied.
    pub fn major(&self) -> u32 {
        self.major
    }
}

fn parse_version_part(part: &str) -> Result<u32, ReleaseDescriptorError> {
    if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ReleaseDescriptorError::MissingMajor);
    }
    part.parse()
        .map_err(|_| ReleaseDescriptorError::MissingMajor)
}

impl CanonicalEncode for ReleaseCoordinates {
    const DOMAIN_TAG: &'static str = "verter.identity.successor.release_coordinates.v1";

    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder.field_str(1, &self.family);
        encoder.field_u32(2, self.major);
        let minor = self.minor.map(|value| value.to_le_bytes());
        encoder.field_option(3, minor.as_ref().map(|bytes| &bytes[..]));
        let patch = self.patch.map(|value| value.to_le_bytes());
        encoder.field_option(4, patch.as_ref().map(|bytes| &bytes[..]));
        // The admitted line is a set: lock spelling order is not identity.
        encoder.field_sorted_set(
            5,
            self.admitted_majors.iter().map(|major| major.to_le_bytes()),
        );
    }
}
