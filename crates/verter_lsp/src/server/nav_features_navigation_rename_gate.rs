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
    expected_same_file_ranges: Option<&[Range]>,
) -> Option<WorkspaceEdit> {
    let emitted =
        gate_cross_file_child_prop_rename(result, rename_class, new_name).map(|mut edit| {
            merge::dedupe_rename_workspace_edit_with_preferred(&mut edit, Some(uri));
            edit
        });
    if !same_file_rename_is_complete(
        emitted.as_ref(),
        uri,
        expected_same_file_ranges,
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

/// The same-file ranges Verter's own typed analysis proves are occurrences of the
/// identifier a rename at `position` targets — the oracle
/// [`same_file_rename_is_complete`] proves the emitted transaction against.
pub(super) fn same_file_rename_expectation(
    server: &VerterLanguageServer,
    uri: &Uri,
    position: &Position,
) -> Option<Vec<Range>> {
    let doc = server.documents.get(uri)?;
    let analysis = server.documents.get_analysis(uri);
    let blocks = scan_sfc_blocks(&doc.source);
    crate::features::rename::same_file_rename_ranges(
        position,
        &doc.source,
        &blocks,
        analysis.as_ref(),
        &doc.line_index,
    )
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
/// transaction against [`same_file_rename_expectation`] — the exact set of
/// same-file ranges Verter's own typed analysis proves are occurrences of this
/// identifier, which is by construction the set Verter itself emits. Every one
/// must be present for THIS file at its exact range with the new name; a missing
/// range means something downstream (the provider merge, the URI-spelling
/// coalesce, the usage synthesis) dropped an edit, which is always a bug.
/// Cross-file additions are unconstrained.
///
/// A CONFIRMED cross-file child-prop rename is exempt: it re-anchors the
/// initiating usage edit itself and is already held to the stricter
/// declaration+usage proof of [`gate_cross_file_child_prop_rename`].
pub(super) fn same_file_rename_is_complete(
    emitted: Option<&WorkspaceEdit>,
    uri: &Uri,
    expected: Option<&[Range]>,
    new_name: &str,
    rename_class: &ChildPropRenameClass,
) -> bool {
    if !matches!(rename_class, ChildPropRenameClass::NotChildProp) {
        return true;
    }
    let Some(expected) = expected else {
        return true;
    };
    workspace_edit_covers_same_file_ranges(emitted, uri, expected, new_name)
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
