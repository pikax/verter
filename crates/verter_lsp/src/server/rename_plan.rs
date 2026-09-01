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
use verter_span::LspPosition;

use crate::documents::carrier_structure::project_carrier_blocks_for_document;
use crate::documents::line_index::LineIndex;
use crate::documents::position_map::PositionMapper;
use crate::features::rename::{
    classify_rename_target, MarkupOccurrenceInventory, RenameTarget, RenameTargetClass,
    SameFileEnumeration, UnenumeratedRegion,
};

use super::provider_state::CarrierMultiClaimancy;
use super::rename_prepare::multi_claimant_rename_unavailable_error;
use super::server_utils::carrier_language_for;
use super::VerterLanguageServer;

/// Whether a rename request may be served for this document at all — decided
/// before any position is classified, and identically for prepare and rename.
pub(super) enum RenameAdmission {
    /// Classify the position and serve.
    Serve {
        /// Root/provider ownership generation captured by the admission gate.
        /// `None` only for non-carrier documents.
        ownership_witness: Option<crate::configured_owner::OwnershipGenerationWitness>,
    },
    /// Another authority owns rename here (the editor's own TypeScript plugin,
    /// or a GENERATED virtual buffer whose renames are not meaningful): answer
    /// nothing, so the editor keeps its own behaviour.
    Decline,
    /// Fail closed with a user-visible reason and NO edit.
    Refuse(tower_lsp_server::jsonrpc::Error),
}
pub(crate) enum SvelteRenameScriptFactState<'a> {
    ExactSyntax(&'a verter_semantic::analysis::framework_facts::svelte::ExactSveltePropsCalls),
    SyntaxIncomplete,
    Unavailable,
    NotApplicable,
}

impl<'a> SvelteRenameScriptFactState<'a> {
    pub(crate) fn from_svelte_evidence(
        evidence: &'a verter_session::framework::script_facts::ScriptFactEvidence<
            verter_semantic::analysis::framework_facts::svelte::SvelteScriptFacts,
        >,
    ) -> Self {
        match evidence {
            verter_session::framework::script_facts::ScriptFactEvidence::Exact(exact) => {
                Self::ExactSyntax(exact.facts().syntax().props_calls())
            }
            verter_session::framework::script_facts::ScriptFactEvidence::Partial(partial) => {
                match partial.exact_syntax() {
                    Some(syntax) => Self::ExactSyntax(syntax.props_calls()),
                    None => Self::SyntaxIncomplete,
                }
            }
            verter_session::framework::script_facts::ScriptFactEvidence::Unavailable(_) => {
                Self::Unavailable
            }
            verter_session::framework::script_facts::ScriptFactEvidence::NotApplicable(_) => {
                Self::NotApplicable
            }
        }
    }
}

/// The shared rename admission gate.
///
/// A carrier owned by MULTIPLE configured projects resolves to a single tsgo
/// default owner for per-file features, but a PROVIDER rename runs only inside
/// that one project: renaming a symbol that ESCAPES the owner would leave it
/// dangling in the siblings. Proving escape needs the cross-project rename
/// fan-out, so rename (and its prepare handshake) FAILS CLOSED with a clear
/// message instead of shipping a partial edit. The same overlap cannot be
/// observed coherently during bootstrap or while a rebuild has moved provider
/// ownership ahead of the published root: both states are `NotReady`, because
/// neither can prove one generation's unique ownership. Refuse this request and
/// invite a retry rather than inferring authority from absent or stale
/// claimants.
/// Checked AFTER the editor-owned yield so an editor-plugin route still defers
/// to the editor's own rename.
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
    match server.carrier_multi_claimancy(uri) {
        CarrierMultiClaimancy::Ready => {
            RenameAdmission::Refuse(multi_claimant_rename_unavailable_error())
        }
        CarrierMultiClaimancy::NotReady => {
            RenameAdmission::Refuse(cold_ownership_rename_not_ready_error())
        }
        CarrierMultiClaimancy::NotMultiClaimant(ownership_witness) => {
            RenameAdmission::Serve { ownership_witness }
        }
    }
}

fn cold_ownership_rename_not_ready_error() -> tower_lsp_server::jsonrpc::Error {
    tower_lsp_server::jsonrpc::Error {
        code: tower_lsp_server::jsonrpc::ErrorCode::ServerError(-32803),
        message: "verter: rename requires a coherent authoritative project ownership snapshot, \
                  but none is available for this request. Retry after workspace initialization \
                  completes; if ownership remains unavailable, reload the workspace or inspect \
                  Verter's logs."
            .into(),
        data: None,
    }
}

pub(super) fn ownership_changed_during_rename_error() -> tower_lsp_server::jsonrpc::Error {
    tower_lsp_server::jsonrpc::Error {
        code: tower_lsp_server::jsonrpc::ErrorCode::ServerError(-32803),
        message: "verter: project ownership changed while rename was running. Retry after the \
                  workspace rebuild completes; no rename edit was produced."
            .into(),
        data: None,
    }
}

/// Whether the file at `canonical_id` has its MARKUP occurrences enumerated for
/// the rename surface — the capability that decides whether a same-file rename
/// claim can be the file's WHOLE authored occurrence set.
///
/// A capability lookup on the file's carrier row, resolved here so the classifier
/// never learns which framework it is looking at. Two rows deviate from the
/// enumerated default, and both deviate towards FAIL-CLOSED:
///
/// * A SVELTE carrier models no semantic markup occurrence inventory. Its
///   template snapshot exists but `binding_occurrences` / `unresolved_bindings`
///   stay empty however much the markup references the binding. The conservative
///   ASCII-token proof built by [`conservative_svelte_authored_occurrences`] can
///   still enumerate a local name across the whole authored file; without that
///   proof this capability remains `NotModelled` and rename refuses.
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

/// Conservatively enumerate every authored spelling of the ASCII identifier at
/// `anchor` in a Svelte source file.
///
/// This is a COMPLETENESS proof, not an edit producer. It deliberately includes
/// same-spelled text in comments, strings, and unrelated scopes; those false
/// positives can only make rename refuse when the provider does not cover them.
/// They can never make a partial transaction pass. The `$name` Svelte store
/// spelling is included when renaming `name`, because `$` is a framework prefix
/// there rather than evidence that the textual `name` occurrence is unrelated.
/// Unicode and otherwise non-identifier anchors decline the proof and retain the
/// existing fail-closed behavior.
fn conservative_svelte_authored_occurrences(
    source: &str,
    line_index: &LineIndex,
    anchor: Option<Range>,
) -> Option<Vec<Range>> {
    let anchor = anchor?;
    let anchor_start = line_index.position_to_offset(&anchor.start)? as usize;
    let anchor_end = line_index.position_to_offset(&anchor.end)? as usize;
    let name = source.get(anchor_start..anchor_end)?;
    let mut name_bytes = name.bytes();
    let first = name_bytes.next()?;
    let is_start = |byte: u8| byte.is_ascii_alphabetic() || matches!(byte, b'_' | b'$');
    let is_continue = |byte: u8| is_start(byte) || byte.is_ascii_digit();
    if !is_start(first) || !name_bytes.all(is_continue) {
        return None;
    }

    let source_bytes = source.as_bytes();
    let mut ranges = Vec::new();
    for (start, _) in source.match_indices(name) {
        let end = start + name.len();
        let preceded_by_identifier = start.checked_sub(1).is_some_and(|before| {
            let byte = source_bytes[before];
            is_continue(byte) && !(byte == b'$' && first != b'$')
        });
        let followed_by_identifier = source_bytes.get(end).is_some_and(|byte| is_continue(*byte));
        if preceded_by_identifier || followed_by_identifier {
            continue;
        }
        ranges.push(Range::new(
            line_index.offset_to_position(start as u32)?,
            line_index.offset_to_position(end as u32)?,
        ));
    }
    ranges
        .iter()
        .any(|range| range == &anchor)
        .then_some(ranges)
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
    conservative_svelte_authored_ranges: Option<Vec<Range>>,
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
        let (target, conservative_svelte_authored_ranges) = (|| {
            let doc = server.documents.get(uri)?;
            let analysis = server.documents.get_analysis(uri);
            let is_svelte = carrier_language_for(&doc.canonical_id)
                .is_some_and(|language| language.is_svelte());
            let svelte_script_facts = is_svelte.then(|| {
                server
                    .documents
                    .host()
                    .resolve_svelte_script_facts(&doc.canonical_id)
            });
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
            let blocks = project_carrier_blocks_for_document(&doc);
            let mut target = classify_rename_target(
                position,
                &doc.source,
                &blocks,
                analysis.as_ref(),
                &doc.line_index,
            );
            let svelte_script_facts_refuse = match svelte_script_facts
                .as_ref()
                .map(SvelteRenameScriptFactState::from_svelte_evidence)
            {
                Some(SvelteRenameScriptFactState::ExactSyntax(props_calls)) => {
                    doc.line_index
                        .position_to_offset(position)
                        .is_some_and(|offset| {
                            svelte_props_call_requires_public_refusal_at(props_calls, offset)
                        })
                        || svelte_native_edit_touches_public_key(
                            &target,
                            props_calls,
                            &doc.line_index,
                        )
                }
                Some(
                    SvelteRenameScriptFactState::SyntaxIncomplete
                    | SvelteRenameScriptFactState::Unavailable
                    | SvelteRenameScriptFactState::NotApplicable,
                ) => true,
                None => false,
            };
            // Typed public members and legacy `export let` declarations are
            // covered by the public-API projection. Untyped `$props()` roles
            // come from their exact producer-owned key/binding inventories. A
            // native same-name occurrence set that reaches any exact public-key
            // span is unsafe as a whole even when the cursor names another
            // symbol, so it takes the same refusal path.
            if is_svelte
                && (svelte_script_facts_refuse
                    || svelte_public_prop_at(
                        server,
                        &doc.canonical_id,
                        &doc.source,
                        &doc.line_index,
                        position,
                    ))
            {
                target.class = RenameTargetClass::PublicComponentProp;
                target.same_file_ranges.clear();
                target.same_file_enumeration =
                    SameFileEnumeration::Partial(UnenumeratedRegion::NoOccurrenceInventory);
            }
            // THE ONE grant of the markup conjunct. The classifier leaves it
            // ungranted (fail-closed); this owner knows the file's carrier and so
            // is the only place that can assert the capability.
            if markup_occurrence_inventory(&doc.canonical_id)
                == MarkupOccurrenceInventory::Enumerated
            {
                target.grant_markup_occurrence_enumeration();
            }
            let conservative_svelte_authored_ranges = (is_svelte
                && target.class == RenameTargetClass::Native)
                .then(|| {
                    conservative_svelte_authored_occurrences(
                        &doc.source,
                        &doc.line_index,
                        target.anchor,
                    )
                })
                .flatten();
            Some((target, conservative_svelte_authored_ranges))
        })()
        .unwrap_or_else(|| (RenameTarget::unavailable(), None));
        Self {
            target,
            conservative_svelte_authored_ranges,
        }
    }

    /// Whether Verter's NATIVE CSS surface owns this position. A CSS class/id has
    /// no TypeScript correlate, so a legitimately empty provider answer must not
    /// revoke the native edit's completeness.
    pub(super) fn is_css(&self) -> bool {
        matches!(self.target.class, RenameTargetClass::Css)
    }

    /// Whether this cursor is positively classified as a public component prop.
    pub(super) fn is_public_component_prop(&self) -> bool {
        matches!(self.target.class, RenameTargetClass::PublicComponentProp)
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
        if let Some(ranges) = &self.conservative_svelte_authored_ranges {
            return SameFileProof::Requires {
                ranges: ranges.clone(),
                enumerates_whole_file: true,
            };
        }
        let whole_file = matches!(
            self.target.same_file_enumeration,
            SameFileEnumeration::Complete
        );
        match (self.target.class, self.target.anchor) {
            (RenameTargetClass::PublicComponentProp, _) => SameFileProof::Unprovable,
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
            // No offer: the editor must never begin a public-prop rename the
            // direct request is required to refuse.
            (RenameTargetClass::PublicComponentProp, _) => RenamePlan::Decline,
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

/// Whether an exact `$props()` key/binding span requires public-prop refusal.
///
/// A public key always refuses, including a shorthand span that also appears
/// as a local binding. A local-only binding refuses only when its call's key
/// set is open. Positions in neither inventory are unchanged.
fn svelte_props_call_requires_public_refusal_at<E>(facts: &E, offset: u32) -> bool
where
    E: verter_semantic::analysis::framework_facts::NegativeEvidence<
        Observation = verter_semantic::analysis::framework_facts::svelte::SveltePropsCall,
    >,
{
    facts.observations().iter().any(|props_call| {
        props_call
            .public_keys
            .iter()
            .any(|key| key.span.contains_offset(offset))
            || (props_call.has_rest
                && props_call
                    .local_bindings
                    .iter()
                    .any(|binding| binding.span.contains_offset(offset)))
    })
}

/// Whether Verter's proposed native same-file edit set intersects an exact
/// `$props()` public-key span.
///
/// Native occurrence collection is spelling-based within the file. An
/// unrelated symbol with the same spelling can therefore propose an edit at a
/// public key. The producer-owned public-key spans are the safety boundary: any
/// intersection refuses the whole rename rather than changing component API.
fn svelte_native_edit_touches_public_key<E>(
    target: &RenameTarget,
    facts: &E,
    line_index: &LineIndex,
) -> bool
where
    E: verter_semantic::analysis::framework_facts::NegativeEvidence<
        Observation = verter_semantic::analysis::framework_facts::svelte::SveltePropsCall,
    >,
{
    if target.class != RenameTargetClass::Native {
        return false;
    }

    target.same_file_ranges.iter().any(|range| {
        let Some(start) = line_index.position_to_offset(&range.start) else {
            return false;
        };
        let Some(end) = line_index.position_to_offset(&range.end) else {
            return false;
        };

        facts.observations().iter().any(|props_call| {
            props_call
                .public_keys
                .iter()
                .any(|key| start < key.span.end && key.span.start < end)
        })
    })
}

/// Whether the existing Svelte public-API projection maps the authored cursor
/// as a local public prop name.
///
/// The projector emits source-map runs only for byte-verified prop-name
/// anchors. Mapping succeeds only inside one such exact run. Convert the
/// negotiated editor position through the document's byte offset into UTF-16,
/// which is the source-map coordinate convention.
fn svelte_public_prop_at(
    server: &VerterLanguageServer,
    canonical_id: &str,
    source: &str,
    line_index: &LineIndex,
    position: &Position,
) -> bool {
    let Some(offset) = line_index.position_to_offset(position) else {
        return false;
    };
    let Some(utf16_position) = LineIndex::new_utf16(source).offset_to_position(offset) else {
        return false;
    };
    let Some(source_map) = server
        .documents
        .host()
        .get_public_api_projection(canonical_id)
        .ok()
        .flatten()
        .and_then(|projection| projection.response.source_map)
    else {
        return false;
    };
    let Ok(mapper) = PositionMapper::from_json(&source_map) else {
        return false;
    };
    mapper
        .carrier_to_tsx(LspPosition::new(
            utf16_position.line,
            utf16_position.character,
        ))
        .is_some()
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
