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
