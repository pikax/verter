//! Revisions of one carrier's projection and the bounded cache keyed by them.

use std::sync::Arc;

use indexmap::IndexMap;
use parking_lot::Mutex;
use rustc_hash::FxHashMap;
use verter_identity::encoding::{CanonicalDigest, CanonicalEncoder};
use verter_span::CanonicalPath;

use crate::sfc_projection_mapper::{MapperProjectHandle, MapperTransformResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Revision(pub [u8; 32]);

/// One revision per channel, so a consumer sees which channel changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProjectionRevisionSet {
    pub authored_text: Revision,
    pub checking_text: Revision,
    pub correspondence: Revision,
    pub public_signature: PublicSignatureRevision,
    pub dependencies: Revision,
}

/// Digest of the declaration product's public source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PublicSignatureRevision(pub [u8; 32]);

/// Everything one projection read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionReadSet {
    pub carrier: Arc<str>,
    pub external_sources: Vec<Arc<str>>,
    pub settings_identity: [u8; 32],
}

/// Bounded per-project cache of complete transform results. Its per-project
/// retention state is private.
pub struct ProjectionFragmentCache {
    capacity_per_project: usize,
    projects: Mutex<FxHashMap<MapperProjectHandle, IndexMap<CacheKey, CacheEntry>>>,
}

#[derive(PartialEq, Eq, Hash)]
struct CacheKey {
    carrier: CanonicalPath,
    inputs: CanonicalDigest,
}

impl CacheKey {
    fn new(carrier: &str, authored: Revision, settings: [u8; 32]) -> Self {
        let mut encoder = CanonicalEncoder::new("verter.session.projection.cache_inputs.v1");
        encoder.field_bytes(1, &authored.0);
        encoder.field_bytes(2, &settings);
        Self {
            carrier: CanonicalPath::new(carrier),
            inputs: encoder.digest(),
        }
    }
}

struct CacheEntry {
    external_sources: Vec<CanonicalPath>,
    result: Arc<MapperTransformResult>,
}

impl ProjectionFragmentCache {
    /// Keeps at most `capacity_per_project` entries per project.
    #[must_use]
    pub fn new(capacity_per_project: usize) -> Self {
        Self {
            capacity_per_project,
            projects: Mutex::new(FxHashMap::default()),
        }
    }
    /// A cached result whose authored text and settings match.
    pub fn get(
        &self,
        project: &MapperProjectHandle,
        carrier: &str,
        authored_text: Revision,
        settings_identity: [u8; 32],
    ) -> Option<Arc<MapperTransformResult>> {
        let key = CacheKey::new(carrier, authored_text, settings_identity);
        let mut projects = self.projects.lock();
        let entries = projects.get_mut(project)?;
        let (key, entry) = entries.shift_remove_entry(&key)?;
        let result = Arc::clone(&entry.result);
        entries.insert(key, entry);
        Some(result)
    }
    /// Store a result. Returns false and stores nothing when the result is
    /// not complete.
    pub fn admit(
        &self,
        project: &MapperProjectHandle,
        read_set: ProjectionReadSet,
        revisions: ProjectionRevisionSet,
        result: Arc<MapperTransformResult>,
    ) -> bool {
        if !result.complete || self.capacity_per_project == 0 {
            return false;
        }
        let key = CacheKey::new(
            &read_set.carrier,
            revisions.authored_text,
            read_set.settings_identity,
        );
        let entry = CacheEntry {
            external_sources: read_set
                .external_sources
                .iter()
                .map(|path| CanonicalPath::new(path))
                .collect(),
            result,
        };
        let mut projects = self.projects.lock();
        let entries = projects.entry(project.clone()).or_default();
        entries.shift_remove(&key);
        if entries.len() == self.capacity_per_project {
            entries.shift_remove_index(0);
        }
        entries.insert(key, entry);
        true
    }
    /// Drop every entry whose read set names `path`. Returns how many.
    pub fn invalidate_path(&self, path: &str) -> usize {
        let path = CanonicalPath::new(path);
        let mut removed = 0;
        self.projects.lock().retain(|_, entries| {
            let before = entries.len();
            entries.retain(|key, entry| {
                key.carrier != path && !entry.external_sources.contains(&path)
            });
            removed += before - entries.len();
            !entries.is_empty()
        });
        removed
    }
    pub fn close_project(&self, project: &MapperProjectHandle) {
        self.projects.lock().remove(project);
    }
    #[must_use]
    pub fn retained(&self) -> usize {
        self.projects.lock().values().map(IndexMap::len).sum()
    }
}

pub fn revisions_of(
    source: &str,
    result: &MapperTransformResult,
    public_declaration_source: Option<&str>,
    read_set: &ProjectionReadSet,
) -> ProjectionRevisionSet {
    let mut checking = CanonicalEncoder::new("verter.session.projection.checking_text.v1");
    let mut correspondence = CanonicalEncoder::new("verter.session.projection.correspondence.v1");
    if let Some(module) = &result.module {
        // The emission already versions both channels, including observation
        // origins that the wire's feature bitmask does not represent.
        checking.field_bytes(1, module.emission.text_revision().digest().as_bytes());
        correspondence.field_bytes(
            1,
            module
                .emission
                .correspondence_revision()
                .digest()
                .as_bytes(),
        );
    } else {
        // A withheld result has no emission from which to borrow revisions.
        checking.field_str(2, &result.text);
        correspondence.field_u64(2, result.mappings.len() as u64);
        for mapping in &result.mappings {
            for (index, value) in mapping.to_wire().into_iter().enumerate() {
                correspondence.field_u32(3 + index as u16, value);
            }
        }
    }
    let mut public = CanonicalEncoder::new("verter.session.projection.public_signature.v1");
    public.field_option(1, public_declaration_source.map(str::as_bytes));
    let mut dependencies = CanonicalEncoder::new("verter.session.projection.dependencies.v1");
    dependencies.field_str(1, CanonicalPath::new(&read_set.carrier).as_str());
    dependencies.field_sorted_set(
        2,
        read_set
            .external_sources
            .iter()
            .map(|path| CanonicalPath::new(path).into_string().into_bytes()),
    );
    dependencies.field_bytes(3, &read_set.settings_identity);
    ProjectionRevisionSet {
        authored_text: authored_revision(source),
        checking_text: Revision(*checking.digest().as_bytes()),
        correspondence: Revision(*correspondence.digest().as_bytes()),
        public_signature: PublicSignatureRevision(*public.digest().as_bytes()),
        dependencies: Revision(*dependencies.digest().as_bytes()),
    }
}

/// The same content identity used by revision production and cache lookup.
pub(crate) fn authored_revision(source: &str) -> Revision {
    let mut encoder = CanonicalEncoder::new("verter.session.projection.authored_text.v1");
    encoder.field_str(1, source);
    Revision(*encoder.digest().as_bytes())
}
