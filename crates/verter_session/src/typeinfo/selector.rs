#![deny(missing_docs)]
//! Typed request subjects, decoded once at the typeinfo adapter.
//!
//! A client names a file by its path, which is the wire form of a
//! `FileName` selector. The host decodes that path before any resolution:
//! a source the loader proves absent (none loaded, and its load answers
//! not found) is the typed `UnknownSource` refusal, never a miss. An executor receives a
//! [`FileNameSelector`] the host vouched for: its fields are private and
//! [`VerterHost::decode_file_name`] is its only constructor, so no executor
//! can be handed an undecoded path. A component subject decodes its owner
//! file through the same [`VerterHost::decode_source`].

use std::sync::Arc;

use verter_protocol::typeinfo::graph::{self as wire, TypeInfoRequestError};
use verter_protocol::verter::v1::type_info_request_error;

use crate::VerterHost;

/// A source the host knows: loaded, or loadable through its workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KnownSource {
    canonical: Arc<str>,
}

impl KnownSource {
    /// The canonical path the host vouched for.
    #[must_use]
    pub(crate) fn canonical(&self) -> &str {
        &self.canonical
    }

    /// The canonical as a shared string.
    #[must_use]
    pub(crate) fn canonical_arc(&self) -> Arc<str> {
        Arc::clone(&self.canonical)
    }
}

/// A declaration named in a file the host knows: the decoded subject of a
/// resolve-symbol request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileNameSelector {
    source: KnownSource,
    name: Arc<str>,
}

impl FileNameSelector {
    /// The canonical path of that file.
    #[must_use]
    pub(crate) fn canonical(&self) -> &str {
        self.source.canonical()
    }

    /// The declaration name.
    #[must_use]
    pub(crate) fn name(&self) -> &str {
        &self.name
    }
}

impl VerterHost {
    /// Decode a request's file path: the source the host knows by that
    /// path, or `UnknownSource` when loading it proves no such file exists.
    ///
    /// Only the loader's own not-found answer is absence. A load that ends
    /// any other way (cancelled, superseded, failed in a stage) proves
    /// nothing about the file, so the request continues and answers as it
    /// would without this decode.
    ///
    /// # Errors
    ///
    /// Returns the typed `UnknownSource` request error, echoing `path`.
    pub(crate) fn decode_source(&self, path: &str) -> Result<KnownSource, TypeInfoRequestError> {
        let absent = self.scheduler.try_get_source(path).is_none()
            && matches!(
                self.load_outcome(path),
                Some(Err(crate::HostLoadError::Failed(error))) if self.proves_absent(&error, path)
            );
        if absent {
            Err(TypeInfoRequestError {
                kind: Some(type_info_request_error::Kind::UnknownSource(
                    wire::wire_error_unknown_source(path),
                )),
            })
        } else {
            Ok(KnownSource {
                canonical: Arc::from(path),
            })
        }
    }

    /// Whether a scheduler failure is the loader's not-found answer for
    /// `path` itself, directly or as the cause of the stage that waited on
    /// it.
    fn proves_absent(&self, error: &verter_scheduler::job::SchedulerError, path: &str) -> bool {
        match error {
            verter_scheduler::job::SchedulerError::FileNotFound { file_id } => {
                self.normalized_analysis_canonical(file_id)
                    == self.normalized_analysis_canonical(path)
            }
            verter_scheduler::job::SchedulerError::DependencyFailed { cause, .. } => {
                self.proves_absent(cause, path)
            }
            _ => false,
        }
    }

    /// Decode a resolve-symbol subject: `name` in the file at `path`.
    ///
    /// # Errors
    ///
    /// Returns `UnknownSource` when the host does not know the file. A
    /// known file without the name decodes; its answer is the miss.
    pub(crate) fn decode_file_name(
        &self,
        path: &str,
        name: &str,
    ) -> Result<FileNameSelector, TypeInfoRequestError> {
        Ok(FileNameSelector {
            source: self.decode_source(path)?,
            name: Arc::from(name),
        })
    }
}
