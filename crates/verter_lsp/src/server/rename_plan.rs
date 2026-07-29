//! The single rename-plan owner: ONE admission decision and ONE classification
//! of the cursor per request, consumed by BOTH `textDocument/prepareRename`
//! (`super::rename_prepare`) and `textDocument/rename`
//! (`super::nav_features_navigation`).
//!
//! WHY one owner. Prepare and rename must agree about who owns the symbol under
//! the cursor. While prepare answered from Verter's native analysis alone it
//! vetoed every position whose sole semantic authority is the TypeScript
//! provider: VS Code aborts on a `null` prepare and never sends
//! `textDocument/rename`, so the authority was never asked at all. Failing
//! closed means emitting NO EDIT when the provider is absent, errors, answers
//! empty, or cannot be mapped safely — not never asking. The classification
//! that decides which case applies therefore lives in exactly one place
//! ([`crate::features::rename::classify_rename_target`]), read through this
//! owner.
//!
//! A resolution is PER-REQUEST and never cached across requests. `handle_rename`
//! re-resolves from the live document, re-queries the provider, and re-validates
//! the captured provider surface after its own await: prepare is not authority
//! transferable across a race, so a prepare that said yes does not license
//! rename to skip any of its own checks.

use tower_lsp_server::ls_types::{Position, Range, Uri, WorkspaceEdit};

use crate::documents::sfc_scanner::scan_sfc_blocks_for_document;
use crate::features::rename::{
    classify_rename_target, MarkupOccurrenceInventory, RenameTarget, RenameTargetClass,
    SameFileEnumeration, UnenumeratedRegion,
};

use super::rename_prepare::multi_claimant_rename_unavailable_error;
use super::server_utils::carrier_language_for;
use super::VerterLanguageServer;

/// Whether a rename request may be served for this document at all — decided
/// before any position is classified, and identically for prepare and rename.
pub(super) enum RenameAdmission {
    /// Classify the position and serve.
    Serve,
    /// Another authority owns rename here (the editor's own TypeScript plugin,
    /// or a GENERATED virtual buffer whose renames are not meaningful): answer
    /// nothing, so the editor keeps its own behaviour.
    Decline,
    /// Fail closed with a user-visible reason and NO edit.
    Refuse(tower_lsp_server::jsonrpc::Error),
}

/// The shared rename admission gate.
///
/// A carrier owned by MULTIPLE configured projects resolves to a single tsgo
/// default owner for per-file features, but a PROVIDER rename runs only inside
/// that one project: renaming a symbol that ESCAPES the owner would leave it
/// dangling in the siblings. Proving escape needs the cross-project rename
/// fan-out, so rename (and its prepare handshake) FAILS CLOSED with a clear
/// message instead of shipping a partial edit. Checked AFTER the editor-owned
/// yield so an editor-plugin route still defers to the editor's own rename.
pub(super) fn rename_request_admission(
    server: &VerterLanguageServer,
    uri: &Uri,
) -> RenameAdmission {
    if server.editor_owns_carrier_rename() {
        return RenameAdmission::Decline;
    }
    if server.documents.get_virtual_source_uri(uri).is_some() {
        return RenameAdmission::Decline;
    }
    if server.carrier_is_multi_claimant(uri) {
        return RenameAdmission::Refuse(multi_claimant_rename_unavailable_error());
    }
    RenameAdmission::Serve
}

/// Whether the file at `canonical_id` has its MARKUP occurrences enumerated for
/// the rename surface — the capability that decides whether a same-file rename
/// claim can be the file's WHOLE authored occurrence set.
///
/// A capability lookup on the file's carrier row, resolved here so the classifier
/// never learns which framework it is looking at. Two rows deviate from the
/// enumerated default, and both deviate towards FAIL-CLOSED:
///
/// * A SVELTE carrier models no markup occurrence at all. Its template snapshot
///   exists but `binding_occurrences` / `unresolved_bindings` stay empty however
///   much the markup references the binding, so its markup contributes nothing to
///   any claim and the claim can never be the whole file. Refusing is the right
///   answer there: it keeps the rename fail-closed instead of shipping a
///   transaction that renames the script and leaves an authored markup occurrence
///   like `$count` bound to the old name.
/// * A file that is not a carrier at all has no markup region this surface reads,
///   and its provider companion is its own buffer (rename is deferred for a
///   self-file projection, so no companion drop can arise) — claim nothing.
///
/// DURABLE HOME: a capability column on the framework adapter descriptor, beside
/// the other per-adapter rows. Until that column exists the polarity here is the
/// one the carrier-routing architecture guard mandates — the reference carrier is
/// the default and a deviating carrier is named — which means a NEW carrier whose
/// template analysis does not model markup occurrences would inherit
/// `Enumerated` and must declare itself here (or, better, in that column) when it
/// lands.
fn markup_occurrence_inventory(canonical_id: &str) -> MarkupOccurrenceInventory {
    match carrier_language_for(canonical_id) {
        Some(language) if language.is_svelte() => MarkupOccurrenceInventory::NotModelled,
        Some(_) => MarkupOccurrenceInventory::Enumerated,
        None => MarkupOccurrenceInventory::NotModelled,
    }
}

/// One request's rename target: the feature-layer classification of the cursor,
/// resolved from ONE document snapshot.
///
/// Resolving once is what makes the native edit and the same-file completeness
/// expectation provably the same set — they are projections of this single
/// value, not two independent re-derivations that a mid-flight edit could pull
/// apart. [`Self::resolve`] additionally proves the source, the line index and
/// the analysis that produced it describe ONE revision.
pub(super) struct RenameTargetResolution {
    target: RenameTarget,
}

impl RenameTargetResolution {
    /// Classify `position` in `uri` once. A document that is not open (or whose
    /// position does not resolve) yields [`RenameTargetClass::Unavailable`] —
    /// no anchor, no same-file range, nothing claimed.
    ///
    /// ONE DOCUMENT VERSION, or nothing. The classification measures offsets with
    /// `doc.line_index` against `doc.source` and reads spans out of the analysis;
    /// those three must describe the SAME revision, or version-A offsets get
    /// resolved against version-B spans. Two independent guards make that hold:
    ///
    /// 1. THE DOCUMENT-COMMIT FENCE. `DocumentRegistry::did_change` commits an
    ///    edit in two steps that are not atomic with each other: it drops this
    ///    canonical's validated semantic snapshot and upserts the HOST first, and
    ///    only after re-compiling the IDE surface writes the new source + line
    ///    index into `DocumentState`. In between, `get_analysis` has no snapshot
    ///    to validate and falls through to the host — already at the NEW version
    ///    while `doc.source`/`doc.line_index` still describe the OLD one. Every
    ///    registry commit (`did_open`, `did_change`, `did_close`) holds
    ///    `did_change_mutex` across BOTH steps, so taking it here makes this
    ///    capture atomic with respect to a commit. It is the same fence the
    ///    completion path takes for the same triple, and it is released before
    ///    either caller's provider await.
    /// 2. FAIL CLOSED ON DRIFT. The fence orders this capture against registry
    ///    commits; it cannot speak for a host mutation that never took it. So the
    ///    analysis is used only once the host's source for this canonical is
    ///    proven byte-identical to the `doc.source` the offsets were measured
    ///    against — validated AFTER the read, so a host that moved while the
    ///    analysis was being served is caught too. Any mismatch (or a canonical
    ///    the host no longer holds) yields [`RenameTargetClass::Unavailable`]:
    ///    NO anchor for prepare to advertise and NO range for rename to emit.
    ///    Emitting version-B spans through version-A's line index writes the new
    ///    name at byte ranges that describe neither revision.
    pub(super) async fn resolve(
        server: &VerterLanguageServer,
        uri: &Uri,
        position: &Position,
    ) -> Self {
        let _document_commit_fence = server.did_change_mutex.lock().await;
        let target = (|| {
            let doc = server.documents.get(uri)?;
            let analysis = server.documents.get_analysis(uri);
            // Post-read validation (fail closed): only an analysis proven to
            // describe THESE bytes may classify THIS cursor.
            let host_source = server.documents.host().get_source(&doc.canonical_id)?;
            if *host_source != *doc.source {
                tracing::debug!(
                    "rename: refusing a torn document/host read for {} — the host source is not \
                     the revision the cursor offsets were measured against",
                    uri.as_str()
                );
                return None;
            }
            let blocks = scan_sfc_blocks_for_document(&doc);
            let mut target = classify_rename_target(
                position,
                &doc.source,
                &blocks,
                analysis.as_ref(),
                &doc.line_index,
            );
            // THE ONE grant of the markup conjunct. The classifier leaves it
            // ungranted (fail-closed); this owner knows the file's carrier and so
            // is the only place that can assert the capability.
            if markup_occurrence_inventory(&doc.canonical_id)
                == MarkupOccurrenceInventory::Enumerated
            {
                target.grant_markup_occurrence_enumeration();
            }
            Some(target)
        })()
        .unwrap_or_else(RenameTarget::unavailable);
        Self { target }
    }

    /// Whether Verter's NATIVE CSS surface owns this position. A CSS class/id has
    /// no TypeScript correlate, so a legitimately empty provider answer must not
    /// revoke the native edit's completeness.
    pub(super) fn is_css(&self) -> bool {
        matches!(self.target.class, RenameTargetClass::Css)
    }

    /// What the emitted transaction must prove about the REQUESTED file — the
    /// same-file completeness oracle.
    ///
    /// A projection of this ONE resolution, and deliberately DISTINCT from the
    /// edit set [`Self::native_workspace_edit`] emits. Every arm carries whether
    /// the required ranges are the file's WHOLE authored occurrence set, because
    /// that — not the arm, and not the class — is what licenses the
    /// current-companion drop delegation (see [`SameFileProof`]):
    ///
    /// * NATIVE / CSS — every same-file occurrence Verter's own typed analysis
    ///   proved, which is by construction the set Verter itself emits. Complete
    ///   exactly when the classifier's [`SameFileEnumeration`] witness says the
    ///   inventory covers the file.
    /// * PROVIDER-ONLY instance member — Verter owns NO occurrence of its own
    ///   (claiming one would rewrite a same-named script declaration, a different
    ///   symbol) and emits nothing, but the AUTHORED TOKEN under the cursor is
    ///   still its own fact. A transaction that does not overwrite that token
    ///   renamed something the user did not ask for, so the anchor — the exact
    ///   range `prepare_rename` requires a provider location to map back onto
    ///   before it offers anything — is required. It is NOT complete: the file's
    ///   other spellings of the same instance member are in no inventory here.
    /// * PROVIDER-ONLY instance member with NO convertible anchor — nothing can
    ///   be proven and nothing can be offered: [`SameFileProof::Unprovable`].
    /// * UNAVAILABLE — Verter proves nothing about the position. It requires
    ///   nothing (so a provider-owned result is not suppressed) and vouches for
    ///   nothing (so it cannot license a drop delegation either).
    pub(super) fn same_file_proof(&self) -> SameFileProof {
        let whole_file = matches!(
            self.target.same_file_enumeration,
            SameFileEnumeration::Complete
        );
        match (self.target.class, self.target.anchor) {
            (RenameTargetClass::Native | RenameTargetClass::Css, _) => SameFileProof::Requires {
                ranges: self.target.same_file_ranges.clone(),
                enumerates_whole_file: whole_file,
            },
            (RenameTargetClass::ProviderOnlyInstanceMember, Some(anchor)) => {
                SameFileProof::Requires {
                    ranges: vec![anchor],
                    enumerates_whole_file: false,
                }
            }
            (RenameTargetClass::ProviderOnlyInstanceMember, None) => SameFileProof::Unprovable,
            (RenameTargetClass::Unavailable, _) => SameFileProof::Requires {
                ranges: Vec::new(),
                enumerates_whole_file: false,
            },
        }
    }

    /// Which region of the file this resolution's same-file inventory does NOT
    /// account for — `None` when the inventory covers the whole file.
    ///
    /// A projection of the same one resolution as [`Self::same_file_proof`], and
    /// strictly narrower: the proof answers WHETHER the inventory covers the
    /// file, this answers WHICH region it misses. That is the whole point of
    /// [`UnenumeratedRegion`] — it is named "so the resulting refusal can say
    /// what it could not prove" — and it is REPORTING only. No gate reads it and
    /// no outcome depends on it, so a refusal that has no region to report stays
    /// exactly the refusal it already was.
    pub(super) fn unenumerated_region(&self) -> Option<UnenumeratedRegion> {
        match self.target.same_file_enumeration {
            SameFileEnumeration::Complete => None,
            SameFileEnumeration::Partial(region) => Some(region),
        }
    }

    /// Verter's own half of the rename transaction, keyed by the request's URI.
    pub(super) fn native_workspace_edit(&self, uri: &Uri, new_name: &str) -> Option<WorkspaceEdit> {
        self.target.same_file_workspace_edit(uri, new_name)
    }

    /// The prepare projection of this resolution — see [`RenamePlan`].
    pub(super) fn prepare_plan(&self) -> RenamePlan {
        match (self.target.class, self.target.anchor) {
            // Verter's own analysis is the authority and its range is exact.
            (RenameTargetClass::Native | RenameTargetClass::Css, Some(range)) => {
                RenamePlan::Offer(range)
            }
            // The provider is the SOLE authority here: ask it before answering.
            (RenameTargetClass::ProviderOnlyInstanceMember, Some(anchor)) => {
                RenamePlan::ProbeProvider { anchor }
            }
            // Nothing renameable, or no authored range to offer (fail closed —
            // an editor must never be handed a guessed range).
            _ => RenamePlan::Decline,
        }
    }
}

/// What the emitted rename transaction must prove about the requested file — see
/// [`RenameTargetResolution::same_file_proof`].
pub(super) enum SameFileProof {
    /// The transaction must overwrite EVERY one of `ranges` with the new name.
    ///
    /// `enumerates_whole_file` is a SEPARATE, stronger claim: that `ranges` is the
    /// file's complete authored occurrence set, so a transaction satisfying this
    /// proof has demonstrably left nothing in this file behind. Only that claim
    /// licenses delegating a dropped provider location on the request's own
    /// generated companion to this gate; a required-but-incomplete set (an empty
    /// one included) proves the ranges it names and vouches for nothing else.
    Requires {
        ranges: Vec<Range>,
        enumerates_whole_file: bool,
    },
    /// Nothing about this position can be proven, so no transaction can be proven
    /// complete: fail closed.
    Unprovable,
}

impl SameFileProof {
    /// Whether this proof asserts its required ranges are the file's COMPLETE
    /// authored occurrence set. `false` for [`Self::Unprovable`].
    pub(super) fn enumerates_whole_file(&self) -> bool {
        matches!(
            self,
            Self::Requires {
                enumerates_whole_file: true,
                ..
            }
        )
    }
}

/// What `textDocument/prepareRename` must do for a resolved target.
pub(super) enum RenamePlan {
    /// Offer this authored range immediately.
    Offer(Range),
    /// Consult the TypeScript provider — the sole authority at this position —
    /// and offer `anchor` ONLY if it proves a safely mappable rename target.
    /// Provider absent, error, timeout, empty answer, superseded surface, or an
    /// unmappable location set all mean no offer.
    ProbeProvider { anchor: Range },
    /// No authority, or no exact authored range: answer nothing.
    Decline,
}
