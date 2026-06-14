use crate::project_resolver::NativeProjectResolver;
use dashmap::DashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderPathKind {
    Ide,
    Api,
    Shadow,
}

/// Typed ownership binding for provider sync state.
///
/// Ownership is explicitly typed (`Unresolved` vs `Owned`) instead of encoded
/// as a magic owner-key string comparison.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ProviderOwnerBinding {
    /// Unresolved ownership for an open document: the file is synced into the
    /// provider but no single project owns it yet. Covers both the pre-snapshot
    /// bootstrap window AND a ready snapshot that resolves no owner (unowned or
    /// ambiguous). Open-document provider state stays in this binding until a
    /// project claims it — it is never deleted merely because ownership failed.
    #[default]
    Unresolved,
    /// Owner-aware state: file bound to a real project (tsconfig path or root).
    Owned(String),
}

impl ProviderOwnerBinding {
    /// Returns `true` if this is an unresolved (no committed owner) binding.
    pub fn is_unresolved(&self) -> bool {
        matches!(self, Self::Unresolved)
    }

    /// Returns the owner key string, or `None` when unresolved.
    pub fn owner_key(&self) -> Option<&str> {
        match self {
            Self::Unresolved => None,
            Self::Owned(key) => Some(key),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProviderSyncState {
    pub owner_binding: ProviderOwnerBinding,
    pub ide_path: Option<String>,
    pub api_path: Option<String>,
    pub shadow_path: Option<String>,
    pub ide_background_loaded: bool,
    pub api_background_loaded: bool,
    pub shadow_background_loaded: bool,
}

impl ProviderSyncState {
    pub fn active_paths(&self) -> Vec<(ProviderPathKind, String)> {
        let mut paths = Vec::new();
        for kind in [
            ProviderPathKind::Ide,
            ProviderPathKind::Api,
            ProviderPathKind::Shadow,
        ] {
            if let Some(path) = self.path_for_kind(kind) {
                paths.push((kind, path.to_string()));
            }
        }
        paths
    }

    pub fn path_for_kind(&self, kind: ProviderPathKind) -> Option<&str> {
        match kind {
            ProviderPathKind::Ide => self.ide_path.as_deref(),
            ProviderPathKind::Api => self.api_path.as_deref(),
            ProviderPathKind::Shadow => self.shadow_path.as_deref(),
        }
    }

    pub fn background_loaded_for_kind(&self, kind: ProviderPathKind) -> bool {
        match kind {
            ProviderPathKind::Ide => self.ide_background_loaded,
            ProviderPathKind::Api => self.api_background_loaded,
            ProviderPathKind::Shadow => self.shadow_background_loaded,
        }
    }

    pub fn set_background_loaded(&mut self, kind: ProviderPathKind, loaded: bool) {
        match kind {
            ProviderPathKind::Ide => self.ide_background_loaded = loaded,
            ProviderPathKind::Api => self.api_background_loaded = loaded,
            ProviderPathKind::Shadow => self.shadow_background_loaded = loaded,
        }
    }

    /// Returns `true` if this state has no committed owner (unresolved binding).
    pub fn is_unresolved(&self) -> bool {
        self.owner_binding.is_unresolved()
    }

    /// Create an unresolved (no committed owner) IDE-only sync state for a
    /// given IDE path.
    pub fn unresolved(ide_path: String) -> Self {
        Self {
            owner_binding: ProviderOwnerBinding::Unresolved,
            ide_path: Some(ide_path),
            api_path: None,
            shadow_path: None,
            ide_background_loaded: false,
            api_background_loaded: false,
            shadow_background_loaded: false,
        }
    }

    /// Create an unresolved open-document IDE state for a Vue `source_id`.
    ///
    /// The IDE path is the local `{source}.tsx` / `{source}.jsx` formula —
    /// the same artifact path the compiler emits and the provider opens for an
    /// open Vue document whose owning project is not yet resolved. This is the
    /// state the drain / foreground sync create (or preserve) so an open Vue
    /// file keeps a live TSX in the provider while ownership is unresolved.
    pub fn unresolved_vue(source_id: &str, is_jsx: bool) -> Self {
        let ext = if is_jsx { ".jsx" } else { ".tsx" };
        Self::unresolved(format!("{source_id}{ext}"))
    }

    pub fn carry_background_loaded_from(&mut self, previous: &ProviderSyncState) {
        for kind in [
            ProviderPathKind::Ide,
            ProviderPathKind::Api,
            ProviderPathKind::Shadow,
        ] {
            if self.path_for_kind(kind) == previous.path_for_kind(kind) {
                self.set_background_loaded(kind, previous.background_loaded_for_kind(kind));
            }
        }
    }

    pub fn is_background_loaded_path(&self, path: &str) -> bool {
        (self.ide_path.as_deref() == Some(path) && self.ide_background_loaded)
            || (self.api_path.as_deref() == Some(path) && self.api_background_loaded)
            || (self.shadow_path.as_deref() == Some(path) && self.shadow_background_loaded)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSyncTransition {
    pub next: ProviderSyncState,
    pub stale_paths: Vec<(ProviderPathKind, String)>,
}

pub fn vue_sync_state_for_source(
    resolver: &NativeProjectResolver,
    source_id: &str,
    is_jsx: bool,
) -> Option<ProviderSyncState> {
    let owner = resolver.owner_for_file(source_id)?;
    let owner_key = owner
        .tsconfig_path
        .clone()
        .unwrap_or_else(|| owner.root.clone());
    Some(ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Owned(owner_key),
        ide_path: resolver.provider_ide_id_for_source(source_id, is_jsx),
        api_path: resolver.provider_id_for_source(source_id),
        shadow_path: None,
        ide_background_loaded: false,
        api_background_loaded: false,
        shadow_background_loaded: false,
    })
}

/// The owner binding the CURRENT snapshot resolver would assign to `source_id`.
///
/// `Owned(key)` when a single project owns the file (the same `tsconfig_path`-
/// else-`root` key [`vue_sync_state_for_source`] uses), otherwise `Unresolved`
/// (unowned or ambiguous). This is the cheap owner-only projection — it does not
/// compile or build paths — used to decide whether an already-loaded open `.vue`
/// file's committed binding still matches the live resolution.
pub fn current_owner_binding_for_source(
    resolver: &NativeProjectResolver,
    source_id: &str,
) -> ProviderOwnerBinding {
    match resolver.owner_for_file(source_id) {
        Some(owner) => {
            let owner_key = owner
                .tsconfig_path
                .clone()
                .unwrap_or_else(|| owner.root.clone());
            ProviderOwnerBinding::Owned(owner_key)
        }
        None => ProviderOwnerBinding::Unresolved,
    }
}

/// Whether a committed sync state's owner binding still matches what the current
/// snapshot resolver would assign — i.e. the binding is up to date and a
/// fully-loaded open `.vue` file can safely SKIP re-reconciliation.
///
/// Returns `false` on any owner mismatch: an owner change (`Owned(a)` →
/// `Owned(b)`), an owner loss (`Owned(a)` → `Unresolved`), or an owner gain
/// (`Unresolved` → `Owned(b)`). A `false` here means an already-loaded open
/// `.vue` MUST still be reconciled (it would otherwise stay stranded on a dead
/// or changed owner — the `no ide_context` class).
pub fn committed_binding_matches_current(
    committed: &ProviderSyncState,
    current: &ProviderOwnerBinding,
) -> bool {
    committed.owner_binding == *current
}

/// Resolve the [`verter_session::FileLanguage`] for a non-Vue
/// provider-sync target.
///
/// `Some(language)` for plain-script rows — the target upserts and
/// syncs to the type provider as that script language (the SAME row the
/// workspace-scan and editor ingresses resolve for the path, via the
/// host's language classifier). `None` for framework-carrier rows: a
/// carrier dependency must never be upserted and synced to the provider
/// as a raw script — the Vue carrier syncs through the dedicated Vue
/// public-api path, and a carrier without a registered implementation
/// produces no provider sync state at all (its own requests surface the
/// typed unsupported-language error).
pub fn provider_script_language(
    host: &verter_session::VerterHost,
    canonical_id: &str,
) -> Option<verter_session::FileLanguage> {
    let language = host.language_classifier().classify(canonical_id);
    (!language.is_framework_carrier()).then_some(language)
}

pub fn non_vue_sync_state_for_source(
    resolver: &NativeProjectResolver,
    source_id: &str,
) -> Option<ProviderSyncState> {
    let owner = resolver.owner_for_file(source_id)?;
    let owner_key = owner
        .tsconfig_path
        .clone()
        .unwrap_or_else(|| owner.root.clone());
    Some(ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Owned(owner_key),
        ide_path: None,
        api_path: None,
        shadow_path: resolver.provider_id_for_source(source_id),
        ide_background_loaded: false,
        api_background_loaded: false,
        shadow_background_loaded: false,
    })
}

pub fn stale_paths_for_transition(
    previous: &ProviderSyncState,
    next: &ProviderSyncState,
) -> Vec<(ProviderPathKind, String)> {
    let owner_changed = previous.owner_binding != next.owner_binding;

    // Unresolved → owner-aware upgrade with unchanged IDE path: not stale.
    // The type provider already has the correct TSX content; only the owner metadata changes.
    if previous.is_unresolved() && owner_changed {
        let mut stale = Vec::new();
        for kind in [
            ProviderPathKind::Ide,
            ProviderPathKind::Api,
            ProviderPathKind::Shadow,
        ] {
            let prev_path = previous.path_for_kind(kind);
            let next_path = next.path_for_kind(kind);
            if let Some(path) = prev_path {
                // Only stale if the actual path changed (not just the owner key)
                if Some(path) != next_path {
                    stale.push((kind, path.to_string()));
                }
            }
        }
        return stale;
    }

    let mut stale = Vec::new();
    for kind in [
        ProviderPathKind::Ide,
        ProviderPathKind::Api,
        ProviderPathKind::Shadow,
    ] {
        let prev_path = previous.path_for_kind(kind);
        let next_path = next.path_for_kind(kind);
        if let Some(path) = prev_path {
            // Stale if: path changed, OR owner changed but path is the same (force rebind)
            if Some(path) != next_path || (owner_changed && Some(path) == next_path) {
                stale.push((kind, path.to_string()));
            }
        }
    }
    stale
}

pub fn prepare_sync_transition(
    states: &DashMap<String, ProviderSyncState>,
    source_id: &str,
    mut next: ProviderSyncState,
) -> ProviderSyncTransition {
    let previous = states.get(source_id).map(|entry| entry.clone());
    if let Some(previous) = previous.as_ref() {
        next.carry_background_loaded_from(previous);
    }

    ProviderSyncTransition {
        stale_paths: previous
            .as_ref()
            .map(|previous| stale_paths_for_transition(previous, &next))
            .unwrap_or_default(),
        next,
    }
}

pub fn commit_sync_transition(
    states: &DashMap<String, ProviderSyncState>,
    source_id: &str,
    next: ProviderSyncState,
) {
    states.insert(source_id.to_string(), next);
}

pub fn remove_sync_state(
    states: &DashMap<String, ProviderSyncState>,
    source_id: &str,
) -> Option<ProviderSyncState> {
    states.remove(source_id).map(|(_, state)| state)
}

/// All provider path kinds, in deterministic order.
const ALL_PATH_KINDS: [ProviderPathKind; 3] = [
    ProviderPathKind::Ide,
    ProviderPathKind::Api,
    ProviderPathKind::Shadow,
];

/// Build the provider state to commit for an OPEN Vue document that currently
/// has NO resolved owner (unowned or ambiguous).
///
/// Editor-liveness invariant: an open Vue document keeps a live TSX in the
/// provider even while its owning project is unresolved. This helper builds the
/// DESIRED Unresolved target — it FORCES the binding to
/// [`ProviderOwnerBinding::Unresolved`] and DROPS the owner-derived API path
/// (`api_path` / `api_background_loaded`) — carrying forward exactly enough prior
/// state for the caller to choose the sync mode.
///
/// An unowned open file must never advertise a stale `Owned` binding (that
/// binding would make `needs_owner_reconcile` false and strand the file on a
/// dead owner) nor a stale `{src}.vue.ts` API path (no project provides it).
///
/// **Desired-extension targeting (R3-4):** the target `ide_path` is the
/// `{src}.tsx` / `.jsx` formula derived from the CURRENT `is_jsx`. On an
/// `is_jsx` flip the prior path is NOT reused — this prevents syncing new code
/// into the wrong (old-extension) provider artifact.
///
/// **Syncability hint:** `ide_background_loaded` is set `true` ONLY when the
/// prior `ide_path` is the SAME desired-extension artifact AND was genuinely
/// live (`prev.ide_background_loaded`). The caller reads this purely to choose
/// `sync_tsx` (in-place update of an already-live path) vs `open_tsx` (first
/// open). It is NOT the committed-liveness decision: the committed `ide_path` /
/// `ide_background_loaded` are produced by [`open_unresolved_vue_commit`], which
/// routes the IDE kind through the shared per-kind revert/retain discipline
/// (`revert_unsynced_kinds` + `genuinely_stale_after_sync`) so a failed/absent
/// sync RETAINS the prior live path rather than advertising a dead/None one.
///
/// The owner-independent API path is always dropped here (it is owner-derived).
pub fn open_unresolved_vue_state(
    previous: Option<&ProviderSyncState>,
    source_id: &str,
    is_jsx: bool,
) -> ProviderSyncState {
    // Route the IDE-path naming through the shared column-backed derivation
    // (D-x) rather than re-deriving the `{src}.tsx`/`.jsx` formula locally — the
    // single naming authority, so a `.svelte` carrier projects `.tsx` exactly
    // as the column dictates.
    let desired_ide_path = verter_workspace::carrier_ide_provider_path(source_id, is_jsx);
    // Syncability hint: the desired path is already live ONLY when the prior IDE
    // path is the SAME desired-extension artifact AND was genuinely loaded. The
    // caller reads this to choose `sync_tsx` (update) vs `open_tsx` (first open).
    let prior_matches_and_live = previous
        .map(|prev| {
            prev.ide_path.as_deref() == Some(desired_ide_path.as_str())
                && prev.ide_background_loaded
        })
        .unwrap_or(false);
    ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Unresolved,
        ide_path: Some(desired_ide_path),
        api_path: None,
        shadow_path: None,
        ide_background_loaded: prior_matches_and_live,
        api_background_loaded: false,
        shadow_background_loaded: false,
    }
}

/// The stale OWNER-DERIVED API path dropped by an open-document owned→unowned
/// conversion, if any — so the caller (which holds the provider `sync` handle)
/// can CLOSE the orphaned `.vue.ts` after committing the converted state.
///
/// [`open_unresolved_vue_state`] is a pure state transform: it forces the
/// binding to `Unresolved`, preserves the owner-independent live IDE TSX, and
/// DROPS the owner-derived `api_path`. Dropping it from committed state is not
/// enough — the provider still holds the `.vue.ts` open, so it leaks as an
/// untracked artifact once no project owns the file. This helper returns the
/// path that was present on `previous` but is absent from the `converted`
/// state, paired as an [`ProviderPathKind::Api`] close target.
///
/// The API path is only OWNER-DERIVED — and therefore only invalid-once-unowned
/// — when the `previous` binding was [`ProviderOwnerBinding::Owned`]. A prior
/// `Unresolved` state's API path (if any) was not owner-derived, so it is left
/// for the existing open-document editor-liveness handling and is NOT returned
/// here. Likewise returns `None` when the previous state had no API path or the
/// converted state still carries the same API path (no drop occurred). The IDE
/// TSX is never returned — the editor-liveness invariant keeps an open
/// document's TSX live, so only the invalid owner-derived API is closed.
pub fn dropped_api_path_on_unowned_conversion(
    previous: Option<&ProviderSyncState>,
    converted: &ProviderSyncState,
) -> Option<(ProviderPathKind, String)> {
    let previous = previous?;
    // Only an `Owned` prior binding has an owner-derived API path to orphan.
    if previous.is_unresolved() {
        return None;
    }
    let prev_api = previous.api_path.as_deref()?;
    // Only stale if the converted state no longer carries this exact API path.
    if converted.api_path.as_deref() == Some(prev_api) {
        return None;
    }
    Some((ProviderPathKind::Api, prev_api.to_string()))
}

/// Per-kind partial-failure gate: for every path kind whose replacement did NOT
/// sync this pass, restore the previous live path (and its background-loaded
/// flag) onto `committed` so the committed state never advertises an unsynced
/// path. A kind that synced keeps its new path; a kind that failed and had no
/// previous path is left untouched (it was never live, so nothing is lost).
///
/// This is the state half of close-after-successful-sync: combined with
/// [`genuinely_stale_after_sync`], a failed reconciliation of one kind leaves
/// that kind's previous path both committed AND open in the provider.
pub fn revert_unsynced_kinds(
    committed: &mut ProviderSyncState,
    previous: Option<&ProviderSyncState>,
    synced_kinds: &[ProviderPathKind],
) {
    for kind in ALL_PATH_KINDS {
        if synced_kinds.contains(&kind) {
            continue;
        }
        // Kind did not sync: revert to the previous live path for this kind
        // (or clear it if there was none) and restore its loaded flag.
        let prev_path =
            previous.and_then(|prev| prev.path_for_kind(kind).map(|path| path.to_string()));
        let prev_loaded = previous
            .map(|prev| prev.background_loaded_for_kind(kind))
            .unwrap_or(false);
        match kind {
            ProviderPathKind::Ide => committed.ide_path = prev_path,
            ProviderPathKind::Api => committed.api_path = prev_path,
            ProviderPathKind::Shadow => committed.shadow_path = prev_path,
        }
        committed
            .set_background_loaded(kind, prev_loaded && committed.path_for_kind(kind).is_some());
    }
}

/// Compute the stale provider paths that are genuinely safe to close AFTER a
/// (possibly partial) sync committed `committed`.
///
/// A stale path is closed only when BOTH hold:
///   * its KIND's replacement actually synced this pass (`synced_kinds`) — a
///     kind whose new path failed to go live must keep its old path open; and
///   * the path is NOT among the committed state's active paths (a same-path
///     rebind — e.g. an owner change on the owner-independent `{src}.tsx` — must
///     not close the just-synced live artifact).
pub fn genuinely_stale_after_sync(
    stale_paths: &[(ProviderPathKind, String)],
    committed: &ProviderSyncState,
    synced_kinds: &[ProviderPathKind],
) -> Vec<(ProviderPathKind, String)> {
    let active: std::collections::HashSet<String> = committed
        .active_paths()
        .into_iter()
        .map(|(_, path)| path)
        .collect();
    stale_paths
        .iter()
        .filter(|(kind, path)| synced_kinds.contains(kind) && !active.contains(path))
        .cloned()
        .collect()
}

/// The committed provider state + close targets for an OPEN unowned Vue
/// document, computed through the SAME per-kind revert/retain discipline the
/// owner-resolved path uses ([`revert_unsynced_kinds`] + [`genuinely_stale_after_sync`]).
///
/// An unowned open Vue target participates in exactly ONE provider kind: the
/// owner-INDEPENDENT IDE TSX. The owner-derived API (`.vue.ts`) is dropped from
/// the committed state and surfaced for an UNCONDITIONAL close (R2-8); it is
/// never a participating kind of the unresolved target.
pub struct OpenUnresolvedVueCommit {
    /// The state to commit. Its `ide_path` is the path CURRENTLY LIVE in the
    /// provider: the freshly-synced desired path if it synced this pass, else
    /// the retained prior LIVE path, else `None`. Binding forced `Unresolved`,
    /// `api_path` always `None`.
    pub committed: ProviderSyncState,
    /// The owner-derived `.vue.ts` orphaned by the owned→unowned conversion, to
    /// close UNCONDITIONALLY after committing (independent of the IDE outcome).
    /// `None` when the prior binding was not `Owned` or carried no API path.
    pub dropped_api: Option<(ProviderPathKind, String)>,
    /// The prior LIVE IDE path orphaned by an `is_jsx` flip, to close ONLY after
    /// the new desired path successfully synced (close-after-success). Already
    /// gated by [`genuinely_stale_after_sync`]: it is `Some` only when the IDE
    /// kind synced this pass AND the prior path is no longer the committed path.
    pub stale_ide_after_success: Option<(ProviderPathKind, String)>,
}

/// Build the committed state + close targets for an OPEN unowned Vue document,
/// routing the IDE kind through the shared per-kind revert/retain discipline.
///
/// This is the UNIFICATION point: instead of reinventing provider-path liveness
/// (a prior `drop_unloaded_ide_path` that dropped a still-live prior path on a
/// failed/absent sync), the unresolved-preserve path now reuses exactly the
/// machinery the owner-resolved path uses:
///
///   * [`revert_unsynced_kinds`] over an IDE-only prior view — a kind that did
///     NOT sync this pass (here only the IDE kind can sync) reverts to the prior
///     LIVE path. The owner-derived API kind is intentionally absent from the
///     unresolved `target` (and from the IDE-only prior view), so it is neither
///     reverted nor re-added — it stays `None` and is closed via `dropped_api`.
///   * [`genuinely_stale_after_sync`] — the orphaned prior IDE path is closed
///     ONLY when its kind synced AND it is no longer the committed path. A
///     failed/absent IDE sync therefore retains the prior live path AND never
///     closes it (the invariant rows 7 & 9 violated).
///
/// `target` is the desired Unresolved state from [`open_unresolved_vue_state`]
/// (it carries the desired-extension IDE path and the open-vs-update syncability
/// hint via `ide_background_loaded`). `ide_synced` is whether the caller's IDE
/// sync of `target.ide_path` succeeded this pass.
///
/// INVARIANT: the committed `ide_path` is the path CURRENTLY LIVE in the
/// provider — never a path the provider has not opened, never `None` while a
/// prior live path is still open.
pub fn open_unresolved_vue_commit(
    previous: Option<&ProviderSyncState>,
    target: ProviderSyncState,
    ide_synced: bool,
) -> OpenUnresolvedVueCommit {
    debug_assert!(
        target.owner_binding.is_unresolved(),
        "open_unresolved_vue_commit target must be Unresolved-bound"
    );
    debug_assert!(
        target.api_path.is_none(),
        "open_unresolved_vue_commit target must not carry an owner-derived API path"
    );

    // The kinds that successfully went live this pass. Only the IDE kind is ever
    // synced on the unresolved-preserve path (an unowned file has no API kind).
    let synced_kinds: Vec<ProviderPathKind> = if ide_synced {
        vec![ProviderPathKind::Ide]
    } else {
        Vec::new()
    };

    // The prior view the unresolved target may carry kinds forward from: ONLY
    // the owner-independent IDE artifact, and ONLY when it was genuinely LIVE.
    // The owner-derived API/Shadow paths are NOT participating kinds of an
    // unowned target, so the IDE-only view ensures `revert_unsynced_kinds`
    // reverts a non-synced API/Shadow kind to `None` (its target value) rather
    // than re-adding the dropped owner-derived path.
    //
    // Only a prior LIVE IDE path is ever a valid retain target. A prior
    // `ide_path = Some(p)` with `ide_background_loaded == false` is a path the
    // provider never opened — it must be treated as NO live prior (`None`), or a
    // non-synced IDE kind would revert to it and commit a path the provider has
    // not opened (`ide_path = Some(p)`, `ide_background_loaded = false`),
    // violating the committed-liveness invariant.
    let prior_ide_only = previous.map(|prev| {
        let prior_live_ide_path = prev
            .ide_background_loaded
            .then(|| prev.ide_path.clone())
            .flatten();
        ProviderSyncState {
            owner_binding: ProviderOwnerBinding::Unresolved,
            ide_background_loaded: prior_live_ide_path.is_some(),
            ide_path: prior_live_ide_path,
            api_path: None,
            shadow_path: None,
            api_background_loaded: false,
            shadow_background_loaded: false,
        }
    });

    // The prior LIVE IDE path is the close CANDIDATE; the gates below decide
    // whether it is genuinely safe to close (synced kind + no longer active).
    let stale_ide_candidate: Vec<(ProviderPathKind, String)> = previous
        .and_then(|prev| {
            prev.ide_background_loaded
                .then(|| prev.ide_path.clone())
                .flatten()
        })
        .map(|path| vec![(ProviderPathKind::Ide, path)])
        .unwrap_or_default();

    let mut committed = target;
    // Mark the IDE kind live when its sync succeeded this pass (mirrors the
    // owner-resolved path's `set_background_loaded(Ide, true)` on success). The
    // `target`'s `ide_background_loaded` is only the open-vs-update syncability
    // hint, not a committed-liveness claim.
    if ide_synced {
        committed.set_background_loaded(ProviderPathKind::Ide, true);
    }
    // Per-kind retain: a kind that did NOT sync reverts to the prior LIVE path
    // (IDE) or to the target's value (API/Shadow → None via the IDE-only view).
    revert_unsynced_kinds(&mut committed, prior_ide_only.as_ref(), &synced_kinds);

    // Close the orphaned prior IDE path ONLY after a successful flip: gated on
    // the IDE kind having synced AND the prior path no longer being committed.
    let stale_ide_after_success =
        genuinely_stale_after_sync(&stale_ide_candidate, &committed, &synced_kinds)
            .into_iter()
            .next();

    // The owner-derived API path orphaned by the owned→unowned conversion, to
    // close UNCONDITIONALLY (independent of the IDE outcome — R2-8).
    let dropped_api = dropped_api_path_on_unowned_conversion(previous, &committed);

    OpenUnresolvedVueCommit {
        committed,
        dropped_api,
        stale_ide_after_success,
    }
}

#[cfg(test)]
#[path = "provider_sync_tests.rs"]
mod tests;
