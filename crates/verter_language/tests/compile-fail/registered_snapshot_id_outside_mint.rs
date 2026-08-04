use verter_language::registered_source_authority::{
    CanonicalIdentityDigest, FileIncarnation, RegisteredSourceSnapshotId, SourceAuthorityNamespaceId,
    SourceGeneration, WholeSourceHash,
};
use verter_language::FileLanguage;

fn mint_registered_snapshot_id(
    authority: SourceAuthorityNamespaceId,
    canonical_digest: CanonicalIdentityDigest,
    file_incarnation: FileIncarnation,
    generation: SourceGeneration,
    content_hash: WholeSourceHash,
    resolved_file_language: FileLanguage,
) -> RegisteredSourceSnapshotId {
    RegisteredSourceSnapshotId {
        authority,
        canonical_digest,
        file_incarnation,
        generation,
        content_hash,
        resolved_file_language,
    }
}

fn main() {}
