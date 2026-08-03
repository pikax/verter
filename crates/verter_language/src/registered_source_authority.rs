//! Live registered-source identity and validation authority.

use std::collections::HashMap;
use std::fmt;
use std::io;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::FileLanguage;

/// Canonical host file identity. The host supplies an already-normalized value;
/// this leaf authority binds and digests it without performing path routing.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CanonicalFileId(Arc<str>);

impl CanonicalFileId {
    pub fn new(value: impl Into<Arc<str>>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CanonicalFileId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Random identity of one registered-source authority lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceAuthorityNamespaceId([u8; 16]);

impl SourceAuthorityNamespaceId {
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

/// Domain-separated digest of a canonical file identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CanonicalIdentityDigest([u8; 32]);

impl CanonicalIdentityDigest {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    fn digest(canonical: &CanonicalFileId) -> Self {
        Self(sha256(&[
            b"verter.registered-source.canonical-identity.v1\0",
            canonical.as_str().as_bytes(),
        ]))
    }
}

/// Host file-incarnation identity. Equality, not ordering, defines identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileIncarnation(u64);

impl FileIncarnation {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Source generation within a file incarnation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SourceGeneration(u64);

impl SourceGeneration {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

/// SHA-256 of the exact registered UTF-8 source bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WholeSourceHash([u8; 32]);

impl WholeSourceHash {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub(crate) fn digest(source: &str) -> Self {
        Self(sha256(&[source.as_bytes()]))
    }
}

/// Sealed identity projected from a validated registered snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RegisteredSourceSnapshotId {
    authority: SourceAuthorityNamespaceId,
    canonical_digest: CanonicalIdentityDigest,
    file_incarnation: FileIncarnation,
    generation: SourceGeneration,
    content_hash: WholeSourceHash,
    resolved_file_language: FileLanguage,
}

impl RegisteredSourceSnapshotId {
    pub fn authority(&self) -> SourceAuthorityNamespaceId {
        self.authority
    }

    pub fn canonical_digest(&self) -> CanonicalIdentityDigest {
        self.canonical_digest
    }

    pub fn file_incarnation(&self) -> FileIncarnation {
        self.file_incarnation
    }

    pub fn generation(&self) -> SourceGeneration {
        self.generation
    }

    pub fn content_hash(&self) -> WholeSourceHash {
        self.content_hash
    }

    pub fn resolved_file_language(&self) -> &FileLanguage {
        &self.resolved_file_language
    }
}

/// Exact registered source state. Construction is confined to its authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredSourceSnapshot {
    id: RegisteredSourceSnapshotId,
    canonical: CanonicalFileId,
    byte_len: u32,
    source: Arc<str>,
}

impl RegisteredSourceSnapshot {
    pub fn id(&self) -> &RegisteredSourceSnapshotId {
        &self.id
    }

    pub fn snapshot_id(&self) -> &RegisteredSourceSnapshotId {
        &self.id
    }

    pub fn authority(&self) -> SourceAuthorityNamespaceId {
        self.id.authority
    }

    pub fn canonical(&self) -> &CanonicalFileId {
        &self.canonical
    }

    pub fn canonical_digest(&self) -> CanonicalIdentityDigest {
        self.id.canonical_digest
    }

    pub fn file_incarnation(&self) -> FileIncarnation {
        self.id.file_incarnation
    }

    pub fn generation(&self) -> SourceGeneration {
        self.id.generation
    }

    pub fn content_hash(&self) -> WholeSourceHash {
        self.id.content_hash
    }

    pub fn resolved_file_language(&self) -> &FileLanguage {
        &self.id.resolved_file_language
    }

    pub fn byte_len(&self) -> u32 {
        self.byte_len
    }

    pub fn bytes(&self) -> &str {
        &self.source
    }

    pub fn source_arc(&self) -> &Arc<str> {
        &self.source
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceRegistrationError {
    SourceTooLarge,
    AuthorityUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceValidationError {
    AuthorityUnavailable,
    AuthorityNamespaceMismatch,
    CanonicalDigestMismatch,
    SourceNotCurrent,
    FileIncarnationMismatch,
    SourceGenerationMismatch,
    ContentHashRehashMismatch,
    ContentHashMismatch,
    ResolvedFileLanguageMismatch,
    ByteLengthMismatch,
    SourceBytesMismatch,
}

/// Sole live mint and current-source validator for one host lifetime.
#[derive(Debug)]
pub struct RegisteredSourceAuthority {
    namespace: SourceAuthorityNamespaceId,
    current: Mutex<HashMap<(CanonicalFileId, FileIncarnation), RegisteredSourceSnapshot>>,
}

impl RegisteredSourceAuthority {
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            namespace: SourceAuthorityNamespaceId(random_bytes()?),
            current: Mutex::new(HashMap::new()),
        })
    }

    pub fn namespace(&self) -> SourceAuthorityNamespaceId {
        self.namespace
    }

    /// Rehashes exact UTF-8 bytes and atomically installs their live identity.
    pub fn register_source(
        &self,
        canonical: CanonicalFileId,
        file_incarnation: FileIncarnation,
        generation: SourceGeneration,
        resolved_file_language: FileLanguage,
        source: Arc<str>,
    ) -> Result<RegisteredSourceSnapshot, SourceRegistrationError> {
        let byte_len =
            u32::try_from(source.len()).map_err(|_| SourceRegistrationError::SourceTooLarge)?;
        let snapshot = RegisteredSourceSnapshot {
            id: RegisteredSourceSnapshotId {
                authority: self.namespace,
                canonical_digest: CanonicalIdentityDigest::digest(&canonical),
                file_incarnation,
                generation,
                content_hash: WholeSourceHash::digest(&source),
                resolved_file_language,
            },
            canonical: canonical.clone(),
            byte_len,
            source,
        };
        self.current
            .lock()
            .map_err(|_| SourceRegistrationError::AuthorityUnavailable)?
            .insert((canonical, file_incarnation), snapshot.clone());
        Ok(snapshot)
    }

    /// Validate that a sealed snapshot is still the exact current source.
    pub fn validate_current(
        &self,
        snapshot: &RegisteredSourceSnapshot,
    ) -> Result<(), SourceValidationError> {
        self.with_validated_current(snapshot, || ())
    }

    pub(crate) fn with_validated_current<R>(
        &self,
        snapshot: &RegisteredSourceSnapshot,
        operation: impl FnOnce() -> R,
    ) -> Result<R, SourceValidationError> {
        let current = self
            .current
            .lock()
            .map_err(|_| SourceValidationError::AuthorityUnavailable)?;
        self.validate_locked(&current, snapshot)?;
        Ok(operation())
    }

    fn validate_locked(
        &self,
        current: &MutexGuard<
            '_,
            HashMap<(CanonicalFileId, FileIncarnation), RegisteredSourceSnapshot>,
        >,
        snapshot: &RegisteredSourceSnapshot,
    ) -> Result<(), SourceValidationError> {
        if snapshot.id.authority != self.namespace {
            return Err(SourceValidationError::AuthorityNamespaceMismatch);
        }
        if snapshot.id.canonical_digest != CanonicalIdentityDigest::digest(&snapshot.canonical) {
            return Err(SourceValidationError::CanonicalDigestMismatch);
        }
        if snapshot.id.content_hash != WholeSourceHash::digest(&snapshot.source) {
            return Err(SourceValidationError::ContentHashRehashMismatch);
        }
        let expected_len = u32::try_from(snapshot.source.len())
            .map_err(|_| SourceValidationError::ByteLengthMismatch)?;
        if snapshot.byte_len != expected_len {
            return Err(SourceValidationError::ByteLengthMismatch);
        }

        let live = match current.get(&(snapshot.canonical.clone(), snapshot.id.file_incarnation)) {
            Some(live) => live,
            None if current
                .keys()
                .any(|(canonical, _)| canonical == &snapshot.canonical) =>
            {
                return Err(SourceValidationError::FileIncarnationMismatch);
            }
            None => return Err(SourceValidationError::SourceNotCurrent),
        };
        if snapshot.id.generation != live.id.generation {
            return Err(SourceValidationError::SourceGenerationMismatch);
        }
        if snapshot.id.content_hash != live.id.content_hash {
            return Err(SourceValidationError::ContentHashMismatch);
        }
        if snapshot.id.resolved_file_language != live.id.resolved_file_language {
            return Err(SourceValidationError::ResolvedFileLanguageMismatch);
        }
        if snapshot.byte_len != live.byte_len {
            return Err(SourceValidationError::ByteLengthMismatch);
        }
        if snapshot.source.as_ref() != live.source.as_ref() {
            return Err(SourceValidationError::SourceBytesMismatch);
        }
        Ok(())
    }
}

pub(crate) fn random_bytes<const N: usize>() -> io::Result<[u8; N]> {
    let mut bytes = [0_u8; N];
    loop {
        fill_random(&mut bytes)?;
        if bytes.iter().any(|byte| *byte != 0) {
            return Ok(bytes);
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn fill_random(bytes: &mut [u8]) -> io::Result<()> {
    unsafe extern "C" {
        fn getrandom(buffer: *mut u8, length: usize, flags: u32) -> isize;
    }

    let mut filled = 0;
    while filled < bytes.len() {
        // SAFETY: the pointer addresses the unfilled suffix of the writable
        // output slice, whose remaining length is passed exactly.
        let read = unsafe { getrandom(bytes[filled..].as_mut_ptr(), bytes.len() - filled, 0) };
        if read < 0 {
            return Err(io::Error::last_os_error());
        }
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "getrandom returned no entropy",
            ));
        }
        filled += read as usize;
    }
    Ok(())
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
))]
fn fill_random(bytes: &mut [u8]) -> io::Result<()> {
    unsafe extern "C" {
        fn getentropy(buffer: *mut u8, length: usize) -> i32;
    }

    if bytes.len() > 256 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "getentropy accepts at most 256 bytes",
        ));
    }
    // SAFETY: `bytes` is a valid writable buffer of exactly the supplied
    // length; getentropy either fills all bytes or reports failure.
    if unsafe { getentropy(bytes.as_mut_ptr(), bytes.len()) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(windows)]
fn fill_random(bytes: &mut [u8]) -> io::Result<()> {
    use std::ffi::c_void;
    use std::ptr;

    #[link(name = "bcrypt")]
    unsafe extern "system" {
        fn BCryptGenRandom(
            algorithm: *mut c_void,
            buffer: *mut u8,
            buffer_len: u32,
            flags: u32,
        ) -> i32;
    }

    const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 0x0000_0002;
    let len = u32::try_from(bytes.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "random buffer too large"))?;
    // SAFETY: the system-preferred RNG accepts a null algorithm handle, and
    // `bytes` supplies a valid writable buffer of exactly `len` bytes.
    let status = unsafe {
        BCryptGenRandom(
            ptr::null_mut(),
            bytes.as_mut_ptr(),
            len,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status >= 0 {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "BCryptGenRandom failed with NTSTATUS {status:#x}"
        )))
    }
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn fill_random(bytes: &mut [u8]) -> io::Result<()> {
    getrandom::getrandom(bytes).map_err(|error| io::Error::other(error.to_string()))
}

#[cfg(not(any(
    windows,
    all(target_arch = "wasm32", target_os = "unknown"),
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
)))]
fn fill_random(_bytes: &mut [u8]) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "no operating-system entropy source on this target",
    ))
}

pub(crate) fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finish()
}

struct Sha256 {
    state: [u32; 8],
    buffer: [u8; 64],
    buffer_len: usize,
    byte_len: u64,
}

impl Sha256 {
    const fn new() -> Self {
        Self {
            state: [
                0x6a09_e667,
                0xbb67_ae85,
                0x3c6e_f372,
                0xa54f_f53a,
                0x510e_527f,
                0x9b05_688c,
                0x1f83_d9ab,
                0x5be0_cd19,
            ],
            buffer: [0; 64],
            buffer_len: 0,
            byte_len: 0,
        }
    }

    fn update(&mut self, mut input: &[u8]) {
        self.byte_len = self.byte_len.wrapping_add(input.len() as u64);
        if self.buffer_len != 0 {
            let take = (64 - self.buffer_len).min(input.len());
            self.buffer[self.buffer_len..self.buffer_len + take].copy_from_slice(&input[..take]);
            self.buffer_len += take;
            input = &input[take..];
            if self.buffer_len < 64 {
                return;
            }
            let block = self.buffer;
            self.compress(&block);
            self.buffer_len = 0;
        }
        while input.len() >= 64 {
            let block: &[u8; 64] = input[..64].try_into().expect("64-byte SHA-256 block");
            self.compress(block);
            input = &input[64..];
        }
        self.buffer[..input.len()].copy_from_slice(input);
        self.buffer_len = input.len();
    }

    fn finish(mut self) -> [u8; 32] {
        let bit_len = self.byte_len.wrapping_mul(8);
        self.buffer[self.buffer_len] = 0x80;
        self.buffer_len += 1;
        if self.buffer_len > 56 {
            self.buffer[self.buffer_len..].fill(0);
            let block = self.buffer;
            self.compress(&block);
            self.buffer = [0; 64];
        } else {
            self.buffer[self.buffer_len..56].fill(0);
        }
        self.buffer[56..64].copy_from_slice(&bit_len.to_be_bytes());
        let block = self.buffer;
        self.compress(&block);

        let mut output = [0_u8; 32];
        for (chunk, word) in output.chunks_exact_mut(4).zip(self.state) {
            chunk.copy_from_slice(&word.to_be_bytes());
        }
        output
    }

    fn compress(&mut self, block: &[u8; 64]) {
        const K: [u32; 64] = [
            0x428a_2f98,
            0x7137_4491,
            0xb5c0_fbcf,
            0xe9b5_dba5,
            0x3956_c25b,
            0x59f1_11f1,
            0x923f_82a4,
            0xab1c_5ed5,
            0xd807_aa98,
            0x1283_5b01,
            0x2431_85be,
            0x550c_7dc3,
            0x72be_5d74,
            0x80de_b1fe,
            0x9bdc_06a7,
            0xc19b_f174,
            0xe49b_69c1,
            0xefbe_4786,
            0x0fc1_9dc6,
            0x240c_a1cc,
            0x2de9_2c6f,
            0x4a74_84aa,
            0x5cb0_a9dc,
            0x76f9_88da,
            0x983e_5152,
            0xa831_c66d,
            0xb003_27c8,
            0xbf59_7fc7,
            0xc6e0_0bf3,
            0xd5a7_9147,
            0x06ca_6351,
            0x1429_2967,
            0x27b7_0a85,
            0x2e1b_2138,
            0x4d2c_6dfc,
            0x5338_0d13,
            0x650a_7354,
            0x766a_0abb,
            0x81c2_c92e,
            0x9272_2c85,
            0xa2bf_e8a1,
            0xa81a_664b,
            0xc24b_8b70,
            0xc76c_51a3,
            0xd192_e819,
            0xd699_0624,
            0xf40e_3585,
            0x106a_a070,
            0x19a4_c116,
            0x1e37_6c08,
            0x2748_774c,
            0x34b0_bcb5,
            0x391c_0cb3,
            0x4ed8_aa4a,
            0x5b9c_ca4f,
            0x682e_6ff3,
            0x748f_82ee,
            0x78a5_636f,
            0x84c8_7814,
            0x8cc7_0208,
            0x90be_fffa,
            0xa450_6ceb,
            0xbef9_a3f7,
            0xc671_78f2,
        ];
        let mut schedule = [0_u32; 64];
        for (slot, bytes) in schedule[..16].iter_mut().zip(block.chunks_exact(4)) {
            *slot = u32::from_be_bytes(bytes.try_into().expect("four-byte SHA-256 word"));
        }
        for index in 16..64 {
            let s0 = schedule[index - 15].rotate_right(7)
                ^ schedule[index - 15].rotate_right(18)
                ^ (schedule[index - 15] >> 3);
            let s1 = schedule[index - 2].rotate_right(17)
                ^ schedule[index - 2].rotate_right(19)
                ^ (schedule[index - 2] >> 10);
            schedule[index] = schedule[index - 16]
                .wrapping_add(s0)
                .wrapping_add(schedule[index - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for index in 0..64 {
            let sum1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choose = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(sum1)
                .wrapping_add(choose)
                .wrapping_add(K[index])
                .wrapping_add(schedule[index]);
            let sum0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = sum0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
        self.state[5] = self.state[5].wrapping_add(f);
        self.state[6] = self.state[6].wrapping_add(g);
        self.state[7] = self.state[7].wrapping_add(h);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn minted() -> (RegisteredSourceAuthority, RegisteredSourceSnapshot) {
        let authority = RegisteredSourceAuthority::new().expect("source authority");
        let snapshot = authority
            .register_source(
                CanonicalFileId::new("file:///workspace/App.vue"),
                FileIncarnation::new(7),
                SourceGeneration::new(11),
                crate::FileLanguage::vue(),
                Arc::from("<template>hello</template>"),
            )
            .expect("registered snapshot");
        (authority, snapshot)
    }

    #[test]
    fn sha256_matches_the_standard_abc_vector() {
        assert_eq!(
            sha256(&[b"abc"]),
            [
                0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
                0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
                0xf2, 0x00, 0x15, 0xad,
            ]
        );
    }

    #[test]
    fn sha256_multipart_matches_the_standard_abc_vector() {
        assert_eq!(
            sha256(&[b"a", b"bc"]),
            [
                0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
                0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
                0xf2, 0x00, 0x15, 0xad,
            ]
        );
    }

    #[test]
    fn short_canonical_ids_have_distinct_digests_and_snapshot_ids() {
        let authority = RegisteredSourceAuthority::new().expect("source authority");
        let register = |canonical| {
            authority
                .register_source(
                    CanonicalFileId::new(canonical),
                    FileIncarnation::new(7),
                    SourceGeneration::new(11),
                    crate::FileLanguage::vue(),
                    Arc::from("<template>same</template>"),
                )
                .expect("registered snapshot")
        };
        let a = register("file:///a.vue");
        let b = register("file:///b.vue");

        let canonical_digests_differ = a.canonical_digest() != b.canonical_digest();
        let snapshot_ids_differ = a.snapshot_id() != b.snapshot_id();
        assert!(
            canonical_digests_differ && snapshot_ids_differ,
            "canonical digest distinct: {canonical_digests_differ}; snapshot ID distinct: {snapshot_ids_differ}"
        );
    }

    #[test]
    fn valid_snapshot_round_trips_all_jointly_validated_fields() {
        let (authority, snapshot) = minted();
        authority
            .validate_current(&snapshot)
            .expect("minted snapshot is current");
        assert_eq!(snapshot.canonical().as_str(), "file:///workspace/App.vue");
        assert_eq!(snapshot.file_incarnation(), FileIncarnation::new(7));
        assert_eq!(snapshot.generation(), SourceGeneration::new(11));
        assert_eq!(
            snapshot.resolved_file_language(),
            &crate::FileLanguage::vue()
        );
        assert_eq!(snapshot.byte_len(), 26);
        assert_eq!(snapshot.bytes(), "<template>hello</template>");
        assert_eq!(
            snapshot.content_hash(),
            WholeSourceHash::digest(snapshot.bytes())
        );
        assert_eq!(snapshot.snapshot_id(), snapshot.id());
    }

    #[test]
    fn tampered_authority_namespace_is_rejected() {
        let (authority, snapshot) = minted();
        let mut tampered = snapshot.clone();
        tampered.id.authority = SourceAuthorityNamespaceId([0xA5; 16]);
        assert_eq!(
            authority.validate_current(&tampered),
            Err(SourceValidationError::AuthorityNamespaceMismatch)
        );
    }

    #[test]
    fn tampered_canonical_digest_is_rejected() {
        let (authority, snapshot) = minted();
        let mut tampered = snapshot.clone();
        tampered.id.canonical_digest = CanonicalIdentityDigest([0xB6; 32]);
        assert_eq!(
            authority.validate_current(&tampered),
            Err(SourceValidationError::CanonicalDigestMismatch)
        );
    }

    #[test]
    fn tampered_file_incarnation_is_rejected() {
        let (authority, snapshot) = minted();
        let mut tampered = snapshot.clone();
        tampered.id.file_incarnation = FileIncarnation::new(8);
        assert_eq!(
            authority.validate_current(&tampered),
            Err(SourceValidationError::FileIncarnationMismatch)
        );
    }

    #[test]
    fn tampered_source_generation_is_rejected() {
        let (authority, snapshot) = minted();
        let mut tampered = snapshot.clone();
        tampered.id.generation = SourceGeneration::new(12);
        assert_eq!(
            authority.validate_current(&tampered),
            Err(SourceValidationError::SourceGenerationMismatch)
        );
    }

    #[test]
    fn tampered_content_hash_is_rejected() {
        let (authority, snapshot) = minted();
        let mut tampered = snapshot.clone();
        tampered.id.content_hash = WholeSourceHash([0xC7; 32]);
        assert_eq!(
            authority.validate_current(&tampered),
            Err(SourceValidationError::ContentHashRehashMismatch)
        );
    }

    #[test]
    fn tampered_source_bytes_are_rehashed_and_rejected() {
        let (authority, snapshot) = minted();
        let mut tampered = snapshot.clone();
        tampered.source = Arc::from("<template>other</template>");
        assert_eq!(
            authority.validate_current(&tampered),
            Err(SourceValidationError::ContentHashRehashMismatch)
        );
    }

    #[test]
    fn tampered_resolved_live_language_is_rejected() {
        let (authority, snapshot) = minted();
        let mut tampered = snapshot.clone();
        tampered.id.resolved_file_language = crate::FileLanguage::svelte();
        assert_eq!(
            authority.validate_current(&tampered),
            Err(SourceValidationError::ResolvedFileLanguageMismatch)
        );
    }

    #[test]
    fn tampered_byte_length_is_rejected() {
        let (authority, snapshot) = minted();
        let mut tampered = snapshot.clone();
        tampered.byte_len += 1;
        authority
            .current
            .lock()
            .expect("source registry")
            .get_mut(&(snapshot.canonical().clone(), snapshot.file_incarnation()))
            .expect("current source")
            .byte_len = tampered.byte_len;
        assert_eq!(
            authority.validate_current(&tampered),
            Err(SourceValidationError::ByteLengthMismatch)
        );
    }

    #[test]
    fn missing_current_registration_is_rejected() {
        let (authority, snapshot) = minted();
        authority
            .current
            .lock()
            .expect("source registry")
            .remove(&(snapshot.canonical().clone(), snapshot.file_incarnation()));
        assert_eq!(
            authority.validate_current(&snapshot),
            Err(SourceValidationError::SourceNotCurrent)
        );
    }

    #[test]
    fn current_content_hash_must_match_the_rehashed_snapshot() {
        let (authority, snapshot) = minted();
        authority
            .current
            .lock()
            .expect("source registry")
            .get_mut(&(snapshot.canonical().clone(), snapshot.file_incarnation()))
            .expect("current source")
            .id
            .content_hash = WholeSourceHash([0xDA; 32]);
        assert_eq!(
            authority.validate_current(&snapshot),
            Err(SourceValidationError::ContentHashMismatch)
        );
    }

    #[test]
    fn current_byte_length_must_match_the_rehashed_snapshot() {
        let (authority, snapshot) = minted();
        authority
            .current
            .lock()
            .expect("source registry")
            .get_mut(&(snapshot.canonical().clone(), snapshot.file_incarnation()))
            .expect("current source")
            .byte_len += 1;
        assert_eq!(
            authority.validate_current(&snapshot),
            Err(SourceValidationError::ByteLengthMismatch)
        );
    }

    #[test]
    fn exact_bytes_are_compared_after_hash_and_length_validation() {
        let (authority, snapshot) = minted();
        authority
            .current
            .lock()
            .expect("source registry")
            .get_mut(&(snapshot.canonical().clone(), snapshot.file_incarnation()))
            .expect("current source")
            .source = Arc::from("<template>jello</template>");
        assert_eq!(
            authority.validate_current(&snapshot),
            Err(SourceValidationError::SourceBytesMismatch)
        );
    }

    #[test]
    fn previous_generation_is_not_current_after_registration_advances() {
        let (authority, snapshot) = minted();
        authority
            .register_source(
                snapshot.canonical().clone(),
                snapshot.file_incarnation(),
                SourceGeneration::new(12),
                crate::FileLanguage::vue(),
                Arc::from("<template>new</template>"),
            )
            .expect("next source generation");
        assert_eq!(
            authority.validate_current(&snapshot),
            Err(SourceValidationError::SourceGenerationMismatch)
        );
    }

    #[test]
    fn distinct_live_file_incarnations_are_validated_independently() {
        let authority = RegisteredSourceAuthority::new().expect("source authority");
        let canonical = CanonicalFileId::new("file:///workspace/App.vue");
        let base = authority
            .register_source(
                canonical.clone(),
                FileIncarnation::new(1),
                SourceGeneration::new(1),
                crate::FileLanguage::vue(),
                Arc::from("<template>base</template>"),
            )
            .expect("base registration");
        let overlay = authority
            .register_source(
                canonical,
                FileIncarnation::new(2),
                SourceGeneration::new(1),
                crate::FileLanguage::vue(),
                Arc::from("<template>overlay</template>"),
            )
            .expect("overlay registration");

        authority
            .validate_current(&base)
            .expect("base remains live");
        authority
            .validate_current(&overlay)
            .expect("overlay remains live");
    }
}
