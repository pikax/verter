//! FFI ↔ host conversions for the typeinfo substrate: the stringly-tagged
//! projection mode. NAPI / WASM adapters use these helpers to keep
//! boundary parsing in one place.

use verter_protocol::typeinfo::{
    MODE_EXPANDED, MODE_IDENTITY, MODE_NAVIGATE, MODE_SHALLOW, MODE_SKELETON,
};
use verter_type_engine::semantic_query::ProjectionMode;

use super::error::FfiConversionError;

// ---------------------------------------------------------------------------
// FFI ↔ host: ProjectionMode
// ---------------------------------------------------------------------------

/// Lower a stringly-tagged FFI projection-mode value to its host enum.
///
/// Accepts the canonical lower-case tags `"identity" | "navigate" |
/// "shallow" | "expanded" | "skeleton"`. Unknown tags surface as
/// [`FfiConversionError::InvalidProjectionMode`].
pub fn parse_projection_mode(tag: &str) -> Result<ProjectionMode, FfiConversionError> {
    match tag {
        MODE_IDENTITY => Ok(ProjectionMode::Identity),
        MODE_NAVIGATE => Ok(ProjectionMode::Navigate),
        MODE_SHALLOW => Ok(ProjectionMode::Shallow),
        MODE_EXPANDED => Ok(ProjectionMode::Expanded),
        MODE_SKELETON => Ok(ProjectionMode::Skeleton),
        other => Err(FfiConversionError::InvalidProjectionMode(other.to_string())),
    }
}

/// Inverse of [`parse_projection_mode`] — render a host
/// `ProjectionMode` to its canonical FFI tag.
pub fn projection_mode_tag(mode: ProjectionMode) -> &'static str {
    match mode {
        ProjectionMode::Identity => MODE_IDENTITY,
        ProjectionMode::Navigate => MODE_NAVIGATE,
        ProjectionMode::Shallow => MODE_SHALLOW,
        ProjectionMode::Expanded => MODE_EXPANDED,
        ProjectionMode::Skeleton => MODE_SKELETON,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_mode_round_trips() {
        for mode in [
            ProjectionMode::Identity,
            ProjectionMode::Navigate,
            ProjectionMode::Shallow,
            ProjectionMode::Expanded,
            ProjectionMode::Skeleton,
        ] {
            let tag = projection_mode_tag(mode);
            assert_eq!(parse_projection_mode(tag).unwrap(), mode);
        }
    }

    #[test]
    fn parse_projection_mode_rejects_unknown() {
        let err = parse_projection_mode("WhatIsThis").unwrap_err();
        assert!(matches!(err, FfiConversionError::InvalidProjectionMode(s) if s == "WhatIsThis"));
    }
}
