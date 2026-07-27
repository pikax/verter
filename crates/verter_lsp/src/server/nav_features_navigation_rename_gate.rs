use super::*;

/// Apply the MERGED-EDIT COMPLETENESS GATE to a cross-file rename result.
///
/// - [`ChildPropRenameClass::Confirmed`]: the merged `WorkspaceEdit` MUST satisfy
///   [`workspace_edit_satisfies_child_prop_rename`] (edits BOTH the prop
///   declaration AND the parent `.vue` usage at their EXACT full ranges). If it does
///   not — including a [`ChildPropDeclarationProof::Unknown`] declaration (no
///   resolved target to prove) — the whole rename fails closed → `None`. This is
///   the fix for the usage-only-partial gap: a confirmed child-prop rename whose
///   merged edit lacks the declaration (e.g. tsgo, synthesis leg could not be
///   produced; or an unresolvable imported type) returns NO edit rather than a
///   usage-only partial. Provider-AGNOSTIC: a result whose declaration leg already
///   lands (a tsserver native leg, or a provider's imported-member edit) passes even
///   when Verter's own synthesis could not locate it (no `is_tsgo`/`is_tsserver`
///   branch).
/// - [`ChildPropRenameClass::NotChildProp`]: do NOT gate. The provider's own merged
///   result is returned untouched — not a confirmed cross-file child-prop rename, so
///   Verter must not suppress an otherwise-valid provider result.
///
/// Inspects ONLY the merged source `WorkspaceEdit`, so it is a pure function of
/// `(merged, class, new_name)` — unit-testable without a live provider.
pub(super) fn gate_cross_file_child_prop_rename(
    merged: Option<WorkspaceEdit>,
    rename_class: &ChildPropRenameClass,
    new_name: &str,
) -> Option<WorkspaceEdit> {
    let ChildPropRenameClass::Confirmed(target) = rename_class else {
        return merged;
    };
    // The resolved declaration target's URI + range — `Unknown` yields no URI/range
    // (a `None` range fails the per-leg proof, so the whole gate fails closed).
    let (expected_decl_uri, expected_decl_range) = match &target.declaration {
        ChildPropDeclarationProof::Known { uri, range, .. } => (Some(uri), *range),
        ChildPropDeclarationProof::Unknown => (None, None),
    };
    let satisfied = merged
        .as_ref()
        .zip(expected_decl_uri)
        .is_some_and(|(edit, decl_uri)| {
            workspace_edit_satisfies_child_prop_rename(
                edit,
                decl_uri,
                expected_decl_range,
                &target.usage.parent_uri,
                target.expected_parent_usage_range,
                new_name,
            )
        });
    if satisfied {
        merged
    } else {
        None
    }
}

/// The dropped provider locations NO completeness gate covers — the ones that
/// must fail the whole rename closed.
///
/// [`merge::merge_rename_locations`] reports every location it could not turn
/// into a source edit. A drop is covered by another authority in exactly ONE
/// case, and that case is decided by PROOF COMPLETENESS, never by path identity
/// alone:
///
/// * A drop on the CURRENT request's own provider companion
///   (`current_provider_path`) is an offset in the generated projection that maps
///   to no authored byte. That is what synthetic generated code looks like there:
///   Verter's IDE surface deliberately re-spells authored bindings in unmapped
///   constructs (the setup-return shim spells a binding three times:
///   `doubled: doubled as unknown as typeof doubled`), a provider correctly
///   reports every one as a rename location, and the mapper cannot tell
///   "synthetic" from "authored but mis-mapped" — unmapped is the same answer for
///   both. So the provider's list carries no completeness information about this
///   file, and refusing on any such drop would refuse essentially every real
///   rename. The drop may be delegated to [`same_file_rename_is_complete`] —
///   BUT ONLY when that gate is a strictly better oracle, which requires its
///   proof to enumerate the file's WHOLE authored occurrence set
///   (`same_file_proof.enumerates_whole_file()`). Then an authored occurrence
///   behind the drop resurfaces as a missing REQUIRED range and the transaction
///   fails there instead.
///
///   When the proof is a strict SUBSET, the delegation has no substance. A
///   `ProviderOnlyInstanceMember` proof is the cursor token alone, so the file's
///   other spelling of the same instance member (`:title="count"` beside
///   `{{ count }}`) is behind neither gate; an `Unavailable` position requires
///   nothing at all; and a carrier with no template occurrence inventory (every
///   Svelte carrier — its markup occurrences are never enumerated) hides every
///   markup occurrence. In each case the drop is unguarded and the whole rename
///   fails closed, rather than shipping a transaction that renames part of the
///   file.
/// * A drop on ANY OTHER path — a FOREIGN carrier companion, a carrier
///   PUBLIC-API surface, a real `.ts`/`.js` file — names an occurrence in a file
///   the transaction will NOT edit and that NO gate covers. The provider computed
///   those offsets from that file's own content, so the occurrence is real:
///   shipping the remainder renames the symbol here and leaves that file bound to
///   a name which no longer exists. That is the write-side dangling reference,
///   and it is what this gate closes.
///
/// Same-file identity is the shared filesystem-identity primitive, never a raw
/// `==`: the provider may spell the companion path differently (slashes,
/// drive-letter case, a `\\?\` prefix) and treating that spelling as foreign
/// would refuse a healthy rename.
pub(super) fn unguarded_rename_drops<'a>(
    dropped: &'a [merge::DroppedRenameLocation],
    current_provider_path: &str,
    same_file_proof: &SameFileProof,
) -> Vec<&'a merge::DroppedRenameLocation> {
    let same_file_gate_covers_the_file = same_file_proof.enumerates_whole_file();
    dropped
        .iter()
        .filter(|drop| {
            !(same_file_gate_covers_the_file
                && verter_span::path::fs_paths_equal(&drop.path, current_provider_path))
        })
        .collect()
}

/// Canonicalize and prove the final rename transaction, or fail closed.
///
/// 1. MERGED-EDIT COMPLETENESS GATE: for a CONFIRMED cross-file child-prop
///    rename the emitted `WorkspaceEdit` must edit BOTH the prop declaration AND
///    the parent `.vue` usage at their EXACT full ranges, or the whole rename
///    returns no edit — never a usage-only / decl-only partial. A `NotChildProp`
///    result passes through untouched. See [`gate_cross_file_child_prop_rename`].
/// 2. Dedupe, folding every URI spelling of the request's file onto `uri`.
/// 3. SAME-FILE COMPLETENESS GATE: see [`same_file_rename_is_complete`].
pub(super) fn finalize_rename_transaction(
    result: Option<WorkspaceEdit>,
    rename_class: &ChildPropRenameClass,
    new_name: &str,
    uri: &Uri,
    same_file_proof: &SameFileProof,
) -> Option<WorkspaceEdit> {
    let emitted =
        gate_cross_file_child_prop_rename(result, rename_class, new_name).map(|mut edit| {
            merge::dedupe_rename_workspace_edit_with_preferred(&mut edit, Some(uri));
            edit
        });
    if !same_file_rename_is_complete(
        emitted.as_ref(),
        uri,
        same_file_proof,
        new_name,
        rename_class,
    ) {
        tracing::warn!(
            "rename: refusing an incomplete edit set — the emitted transaction does not cover \
             every authored occurrence in {}",
            uri.as_str()
        );
        return None;
    }
    emitted
}

/// The SAME-FILE completeness gate — the write-side answer to "the provider
/// returned SOMETHING, so the non-emptiness check passed". Non-emptiness cannot
/// see a partial: a 2-of-4 edit set renames the declaration and leaves the
/// template bound to a name that no longer exists, and the editor applies it as
/// a success.
///
/// What is compared is NOT rename-vs-references counts. `references`
/// legitimately reports spans a rename must never overwrite (the whole
/// `defineProps(…)` call span of a macro bound to the name, and a provider's
/// comment/string matches that `findRenameLocations` correctly excludes), so a
/// count comparison would refuse valid renames. This compares the EMITTED
/// transaction against
/// [`RenameTargetResolution::same_file_proof`](super::rename_plan::RenameTargetResolution::same_file_proof)
/// — for a natively-owned symbol the exact set of same-file ranges Verter's own
/// typed analysis proves are occurrences of this identifier, which is by
/// construction the set Verter itself emits, and for a PROVIDER-ONLY instance
/// member the authored token under the cursor. Every proved range must be present
/// for THIS file at its exact range with the new name; a missing range means
/// something downstream (the provider merge, the URI-spelling coalesce, the usage
/// synthesis) dropped an edit, or the provider answered for a different
/// occurrence — both of which ship a rename of something the caller did not ask
/// for. This gate constrains the REQUESTED file only; the files a provider
/// location names and the transaction does not edit are proven separately by
/// [`unguarded_rename_drops`], which is the authority for every path but this
/// one.
///
/// [`SameFileProof::Unprovable`] fails closed. A [`SameFileProof::Requires`] set
/// is empty ONLY for a position Verter cannot classify at all, where it asserts
/// nothing rather than suppressing a provider-owned result — and where it
/// therefore also vouches for no dropped companion leg
/// ([`unguarded_rename_drops`]).
///
/// A CONFIRMED cross-file child-prop rename is exempt: it re-anchors the
/// initiating usage edit itself and is already held to the stricter
/// declaration+usage proof of [`gate_cross_file_child_prop_rename`].
pub(super) fn same_file_rename_is_complete(
    emitted: Option<&WorkspaceEdit>,
    uri: &Uri,
    proof: &SameFileProof,
    new_name: &str,
    rename_class: &ChildPropRenameClass,
) -> bool {
    if !matches!(rename_class, ChildPropRenameClass::NotChildProp) {
        return true;
    }
    match proof {
        SameFileProof::Unprovable => false,
        SameFileProof::Requires { ranges, .. } => {
            workspace_edit_covers_same_file_ranges(emitted, uri, ranges, new_name)
        }
    }
}

/// Whether `edit` overwrites EVERY `expected` range of `uri` with `new_name`.
///
/// Matches the file by canonical path equality, not by URI spelling — the merge
/// keys its mapped edits by its own (lowercased) provider path form, which the
/// final coalesce folds onto the request's spelling.
pub(super) fn workspace_edit_covers_same_file_ranges(
    edit: Option<&WorkspaceEdit>,
    uri: &Uri,
    expected: &[Range],
    new_name: &str,
) -> bool {
    if expected.is_empty() {
        return true;
    }
    let Some(changes) = edit.and_then(|edit| edit.changes.as_ref()) else {
        return false;
    };
    let expected_path = uri_to_canonical_id(uri);
    let edits: Vec<&TextEdit> = changes
        .iter()
        .filter(|(key, _)| {
            verter_span::path::fs_paths_equal(&uri_to_canonical_id(key), &expected_path)
        })
        .flat_map(|(_, edits)| edits.iter())
        .collect();
    expected.iter().all(|range| {
        edits
            .iter()
            .any(|candidate| candidate.range == *range && candidate.new_text == new_name)
    })
}
