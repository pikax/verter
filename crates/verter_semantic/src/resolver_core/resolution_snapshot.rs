//! Immutable module-resolution observations shared by every retry of one
//! top-level resolution.

use std::collections::HashMap;
use std::sync::Arc;

use crate::resolver_core::{PathProbe, ResolutionPackageManifest};

#[derive(Debug, Clone, Default)]
pub struct ResolutionObservationSnapshot {
    path_probes: HashMap<String, PathProbe>,
    real_paths: HashMap<String, Option<Arc<str>>>,
    manifests: HashMap<String, Option<Arc<ResolutionPackageManifest>>>,
    #[cfg(test)]
    stable_absent_defaults: bool,
}

impl ResolutionObservationSnapshot {
    pub fn insert_path_probe(&mut self, path: String, probe: PathProbe) -> Option<PathProbe> {
        self.path_probes.insert(path, probe)
    }

    pub fn insert_real_path(
        &mut self,
        path: String,
        real_path: Option<Arc<str>>,
    ) -> Option<Option<Arc<str>>> {
        self.real_paths.insert(path, real_path)
    }

    pub fn insert_package_manifest(
        &mut self,
        directory: String,
        manifest: Option<Arc<ResolutionPackageManifest>>,
    ) -> Option<Option<Arc<ResolutionPackageManifest>>> {
        self.manifests.insert(directory, manifest)
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
        #[cfg(test)]
        if probe.is_none() && self.stable_absent_defaults {
            return Some(PathProbe::Absent);
        }
        probe
    }

    pub fn real_path(&self, path: &str) -> Option<Option<Arc<str>>> {
        let real_path = self.real_paths.get(path).cloned();
        #[cfg(test)]
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
        #[cfg(test)]
        if manifest.is_none() && self.stable_absent_defaults {
            return Some(None);
        }
        manifest
    }

    #[cfg(test)]
    pub(crate) fn with_stable_absent_defaults_for_test() -> Self {
        Self {
            stable_absent_defaults: true,
            ..Self::default()
        }
    }
}
