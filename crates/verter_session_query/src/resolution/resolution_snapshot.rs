//! Immutable module-resolution observations shared by every retry of one
//! top-level resolution.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::resolution::{PathProbe, ResolutionPackageManifest};

/// Paths are held as the shared text of the input keys that loaded them, so
/// recording an observation copies no spelling.
#[derive(Debug, Clone, Default)]
pub struct ResolutionObservationSnapshot {
    path_probes: FxHashMap<Arc<str>, PathProbe>,
    real_paths: FxHashMap<Arc<str>, Option<Arc<str>>>,
    manifests: FxHashMap<Arc<str>, Option<Arc<ResolutionPackageManifest>>>,
    #[cfg(any(test, feature = "test-support"))]
    stable_absent_defaults: bool,
}

impl ResolutionObservationSnapshot {
    pub fn insert_path_probe(
        &mut self,
        path: impl Into<Arc<str>>,
        probe: PathProbe,
    ) -> Option<PathProbe> {
        self.path_probes.insert(path.into(), probe)
    }

    pub fn insert_real_path(
        &mut self,
        path: impl Into<Arc<str>>,
        real_path: Option<Arc<str>>,
    ) -> Option<Option<Arc<str>>> {
        self.real_paths.insert(path.into(), real_path)
    }

    pub fn insert_package_manifest(
        &mut self,
        directory: impl Into<Arc<str>>,
        manifest: Option<Arc<ResolutionPackageManifest>>,
    ) -> Option<Option<Arc<ResolutionPackageManifest>>> {
        self.manifests.insert(directory.into(), manifest)
    }

    #[must_use]
    pub fn contains_path_probe(&self, path: &str) -> bool {
        self.path_probes.contains_key(path)
    }

    #[must_use]
    pub fn contains_real_path(&self, path: &str) -> bool {
        self.real_paths.contains_key(path)
    }

    #[must_use]
    pub fn contains_package_manifest(&self, directory: &str) -> bool {
        self.manifests.contains_key(directory)
    }

    pub fn path_probe(&self, path: &str) -> Option<PathProbe> {
        let probe = self.path_probes.get(path).copied();
        #[cfg(any(test, feature = "test-support"))]
        if probe.is_none() && self.stable_absent_defaults {
            return Some(PathProbe::Absent);
        }
        probe
    }

    pub fn real_path(&self, path: &str) -> Option<Option<Arc<str>>> {
        let real_path = self.real_paths.get(path).cloned();
        #[cfg(any(test, feature = "test-support"))]
        if real_path.is_none() && self.stable_absent_defaults {
            return Some(None);
        }
        real_path
    }

    pub fn package_manifest(
        &self,
        directory: &str,
    ) -> Option<Option<Arc<ResolutionPackageManifest>>> {
        let manifest = self.manifests.get(directory).cloned();
        #[cfg(any(test, feature = "test-support"))]
        if manifest.is_none() && self.stable_absent_defaults {
            return Some(None);
        }
        manifest
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn with_stable_absent_defaults_for_test() -> Self {
        Self {
            stable_absent_defaults: true,
            ..Self::default()
        }
    }
}
