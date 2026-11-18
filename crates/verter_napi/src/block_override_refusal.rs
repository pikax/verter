//! The error `applyBlockOverrides` throws when the host refuses supplied
//! block content: its `code` names the refusal, and the TypeScript union of
//! those codes is rendered from [`BlockOverrideRefusalCode`] into the
//! published declaration, never written by hand.

use napi::Error;
use verter_ffi::convert::host_error_to_string;
use verter_session as host;

use crate::host_error_status;

/// The `code` of the error `applyBlockOverrides` throws when the host
/// refuses supplied block content: one per refusal, except that a result
/// made for an owner revision a newer one has replaced is as stale as one
/// the host marked stale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ts_rs::TS)]
#[ts(rename = "HostBlockOverrideRefusalCode")]
// The variant names are the published codes, so they keep their shared prefix.
#[allow(clippy::enum_variant_names)]
pub enum BlockOverrideRefusalCode {
    BlockContentUnavailable,
    BlockContentOwnerMissing,
    BlockContentStale,
    BlockContentSourcesConflict,
    BlockContentArtifactMismatch,
    BlockContentBasisMismatch,
    BlockContentSourceSpaceMismatch,
    BlockContentCodeHashMismatch,
    BlockContentSourceMapHashMismatch,
    BlockContentInvalidSourceMap,
    BlockContentCorrelationMismatch,
    BlockContentCorrelationTerminal,
    BlockContentCorrelationClosed,
    BlockContentCorrelationCancelled,
    BlockContentMalformedToken,
    BlockContentPayloadTooLarge,
    BlockContentDuplicateBlock,
}

impl BlockOverrideRefusalCode {
    /// The code of a refusal.
    pub fn of(refusal: &host::BlockContentRefusal) -> Self {
        use host::BlockContentRefusal as Refusal;
        match refusal {
            Refusal::Unavailable { .. } => Self::BlockContentUnavailable,
            Refusal::Missing => Self::BlockContentOwnerMissing,
            Refusal::Stale | Refusal::CorrelationSuperseded => Self::BlockContentStale,
            Refusal::Conflict => Self::BlockContentSourcesConflict,
            Refusal::ArtifactMismatch => Self::BlockContentArtifactMismatch,
            Refusal::BasisMismatch => Self::BlockContentBasisMismatch,
            Refusal::SourceSpaceMismatch => Self::BlockContentSourceSpaceMismatch,
            Refusal::CodeHashMismatch => Self::BlockContentCodeHashMismatch,
            Refusal::SourceMapHashMismatch => Self::BlockContentSourceMapHashMismatch,
            Refusal::InvalidSourceMap => Self::BlockContentInvalidSourceMap,
            Refusal::CorrelationMismatch => Self::BlockContentCorrelationMismatch,
            Refusal::CorrelationTerminal => Self::BlockContentCorrelationTerminal,
            Refusal::CorrelationClosed => Self::BlockContentCorrelationClosed,
            Refusal::CorrelationCancelled => Self::BlockContentCorrelationCancelled,
            Refusal::MalformedToken => Self::BlockContentMalformedToken,
            Refusal::PayloadTooLarge => Self::BlockContentPayloadTooLarge,
            Refusal::DuplicateBlock => Self::BlockContentDuplicateBlock,
        }
    }

    /// The code as the error carries it: the variant's name, which is the
    /// member the published TypeScript union declares for it.
    pub fn code(self) -> String {
        format!("{self:?}")
    }
}

/// The same error with its status carried as text, the form a typed code
/// needs.
pub(crate) fn error_with_text_code(err: Error) -> Error<String> {
    Error::new(err.status.to_string(), err.reason)
}

/// The error of `applyBlockOverrides`: the host error's text, with the
/// refusal's code when the host refused the supplied content.
pub(crate) fn block_override_error(err: host::HostError) -> Error<String> {
    let code = match &err {
        host::HostError::BlockContentRefused(refusal) => {
            BlockOverrideRefusalCode::of(refusal).code()
        }
        other => host_error_status(other).to_string(),
    };
    Error::new(code, host_error_to_string(&err))
}

#[cfg(test)]
mod tests {
    use napi::Status;

    use super::*;

    #[test]
    fn every_refusal_carries_its_code_and_the_host_message() {
        use host::BlockContentRefusal::*;
        let unavailable = host::BlockContentRefusal::Unavailable {
            block_token: "block".to_string(),
            availability: host::BlockContentAvailability::Missing,
        };
        let cases = [
            (unavailable, "BlockContentUnavailable"),
            (Missing, "BlockContentOwnerMissing"),
            (Stale, "BlockContentStale"),
            (CorrelationSuperseded, "BlockContentStale"),
            (Conflict, "BlockContentSourcesConflict"),
            (ArtifactMismatch, "BlockContentArtifactMismatch"),
            (BasisMismatch, "BlockContentBasisMismatch"),
            (SourceSpaceMismatch, "BlockContentSourceSpaceMismatch"),
            (CodeHashMismatch, "BlockContentCodeHashMismatch"),
            (SourceMapHashMismatch, "BlockContentSourceMapHashMismatch"),
            (InvalidSourceMap, "BlockContentInvalidSourceMap"),
            (CorrelationMismatch, "BlockContentCorrelationMismatch"),
            (CorrelationTerminal, "BlockContentCorrelationTerminal"),
            (CorrelationClosed, "BlockContentCorrelationClosed"),
            (CorrelationCancelled, "BlockContentCorrelationCancelled"),
            (MalformedToken, "BlockContentMalformedToken"),
            (PayloadTooLarge, "BlockContentPayloadTooLarge"),
            (DuplicateBlock, "BlockContentDuplicateBlock"),
        ];
        for (refusal, code) in cases {
            let error = host::HostError::BlockContentRefused(refusal);
            let message = host_error_to_string(&error);
            let thrown = block_override_error(error);
            assert_eq!(thrown.status, code);
            assert_eq!(thrown.reason, message, "the message text is unchanged");
        }
    }

    #[test]
    fn other_errors_keep_their_status_code() {
        let invalid = block_override_error(host::HostError::InvalidQuery);
        assert_eq!(invalid.status, Status::InvalidArg.to_string());
    }
}
