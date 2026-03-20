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
/// Replaces the `"__provisional__"` magic string sentinel. Bootstrap state
/// is now explicitly typed instead of encoded in a string comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderOwnerBinding {
    /// Pre-snapshot provisional state: file synced before ownership is known.
    Provisional,
    /// Owner-aware state: file bound to a real project (tsconfig path or root).
    Owned(String),
}

impl Default for ProviderOwnerBinding {
    fn default() -> Self {
        Self::Provisional
    }
}

impl ProviderOwnerBinding {
    /// Returns `true` if this is a provisional (pre-snapshot) binding.
    pub fn is_provisional(&self) -> bool {
        matches!(self, Self::Provisional)
    }

    /// Returns the owner key string, or `None` for provisional.
    pub fn owner_key(&self) -> Option<&str> {
        match self {
            Self::Provisional => None,
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

    /// Returns `true` if this state was created by provisional (pre-snapshot) sync.
    pub fn is_provisional(&self) -> bool {
        self.owner_binding.is_provisional()
    }

    /// Create a provisional sync state (no resolver, IDE-only).
    pub fn provisional(ide_path: String) -> Self {
        Self {
            owner_binding: ProviderOwnerBinding::Provisional,
            ide_path: Some(ide_path),
            api_path: None,
            shadow_path: None,
            ide_background_loaded: false,
            api_background_loaded: false,
            shadow_background_loaded: false,
        }
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

    // Provisional → owner-aware upgrade with unchanged IDE path: not stale.
    // The type provider already has the correct TSX content; only the owner metadata changes.
    if previous.is_provisional() && owner_changed {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vue_sync_state_uses_owner_key_from_tsconfig() {
        let resolver = NativeProjectResolver::new(vec![
            crate::project_resolver::IdeProjectConfig::new(
                "/workspace/pkg-a".to_string(),
                "/workspace".to_string(),
                Some("/workspace/pkg-a/tsconfig.json".to_string()),
            ),
            crate::project_resolver::IdeProjectConfig::new(
                "/workspace".to_string(),
                "/workspace".to_string(),
                None,
            ),
        ]);

        let state = vue_sync_state_for_source(&resolver, "/workspace/pkg-a/src/App.vue", false)
            .expect("matched Vue source should materialize provider state");

        assert_eq!(
            state.owner_binding,
            ProviderOwnerBinding::Owned("/workspace/pkg-a/tsconfig.json".to_string()),
            "owner_binding should be Owned with tsconfig path when available"
        );
        assert_eq!(
            state.ide_path.as_deref(),
            Some("/workspace/pkg-a/src/App.vue.tsx"),
            "provider IDE path should be canonical_id.tsx"
        );
        assert_eq!(
            state.api_path.as_deref(),
            Some("/workspace/pkg-a/src/App.vue.ts"),
            "Vue public API output should still be tracked alongside the IDE artifact"
        );
    }

    #[test]
    fn stale_paths_only_include_paths_that_change() {
        let previous = ProviderSyncState {
            owner_binding: ProviderOwnerBinding::Owned("/workspace/tsconfig.json".to_string()),
            ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
            api_path: Some("/workspace/src/App.vue.ts".to_string()),
            ..Default::default()
        };
        let next = ProviderSyncState {
            owner_binding: ProviderOwnerBinding::Owned("/workspace/tsconfig.json".to_string()),
            ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
            api_path: Some("/workspace/src/App.vue.ts".to_string()),
            ..Default::default()
        };

        assert!(
            stale_paths_for_transition(&previous, &next).is_empty(),
            "same owner + same paths = no stale"
        );
    }

    #[test]
    fn owner_change_forces_stale_even_when_paths_unchanged() {
        let previous = ProviderSyncState {
            owner_binding: ProviderOwnerBinding::Owned("/workspace/tsconfig.old.json".to_string()),
            ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
            api_path: Some("/workspace/src/App.vue.ts".to_string()),
            ..Default::default()
        };
        let next = ProviderSyncState {
            owner_binding: ProviderOwnerBinding::Owned("/workspace/tsconfig.new.json".to_string()),
            ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
            api_path: Some("/workspace/src/App.vue.ts".to_string()),
            ..Default::default()
        };

        let stale = stale_paths_for_transition(&previous, &next);
        assert_eq!(
            stale.len(),
            2,
            "both active paths should be stale on owner change"
        );
        assert!(stale.contains(&(
            ProviderPathKind::Ide,
            "/workspace/src/App.vue.tsx".to_string()
        )));
        assert!(stale.contains(&(
            ProviderPathKind::Api,
            "/workspace/src/App.vue.ts".to_string()
        )));
    }

    #[test]
    fn fallback_to_fallback_owner_change_detected() {
        let previous = ProviderSyncState {
            owner_binding: ProviderOwnerBinding::Owned("/workspace/old-root".to_string()),
            shadow_path: Some("/workspace/src/utils.ts".to_string()),
            ..Default::default()
        };
        let next = ProviderSyncState {
            owner_binding: ProviderOwnerBinding::Owned("/workspace/new-root".to_string()),
            shadow_path: Some("/workspace/src/utils.ts".to_string()),
            ..Default::default()
        };

        let stale = stale_paths_for_transition(&previous, &next);
        assert_eq!(
            stale.len(),
            1,
            "fallback→fallback with different root = stale"
        );
        assert_eq!(stale[0].1, "/workspace/src/utils.ts");
    }

    #[test]
    fn prepare_sync_transition_preserves_background_flags_for_unchanged_paths() {
        let states = DashMap::new();
        states.insert(
            "/workspace/src/App.vue".to_string(),
            ProviderSyncState {
                owner_binding: ProviderOwnerBinding::Owned("/workspace/tsconfig.json".to_string()),
                ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
                api_path: Some("/workspace/src/App.vue.ts".to_string()),
                ide_background_loaded: true,
                api_background_loaded: true,
                ..Default::default()
            },
        );

        let transition = prepare_sync_transition(
            &states,
            "/workspace/src/App.vue",
            ProviderSyncState {
                owner_binding: ProviderOwnerBinding::Owned("/workspace/tsconfig.json".to_string()),
                ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
                api_path: Some("/workspace/src/App.vue.ts".to_string()),
                ..Default::default()
            },
        );

        assert!(transition.stale_paths.is_empty());
        assert!(transition.next.ide_background_loaded);
        assert!(transition.next.api_background_loaded);
    }

    #[test]
    fn provisional_state_is_detected() {
        let state = ProviderSyncState::provisional("/workspace/src/App.vue.tsx".to_string());
        assert!(
            state.is_provisional(),
            "provisional state should be detected"
        );
        assert_eq!(state.owner_binding, ProviderOwnerBinding::Provisional);
        assert_eq!(
            state.ide_path.as_deref(),
            Some("/workspace/src/App.vue.tsx")
        );
        assert!(state.api_path.is_none(), "provisional has no API path");
    }

    #[test]
    fn provisional_to_owner_aware_same_ide_path_not_stale() {
        let provisional = ProviderSyncState::provisional("/workspace/src/App.vue.tsx".to_string());
        let owner_aware = ProviderSyncState {
            owner_binding: ProviderOwnerBinding::Owned("/workspace/tsconfig.json".to_string()),
            ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
            api_path: Some("/workspace/src/App.vue.ts".to_string()),
            ..Default::default()
        };

        let stale = stale_paths_for_transition(&provisional, &owner_aware);
        assert!(
            stale.is_empty(),
            "provisional → owner-aware with same IDE path should not be stale, got: {:?}",
            stale
        );
    }

    #[test]
    fn provisional_to_owner_aware_different_ide_path_is_stale() {
        let provisional = ProviderSyncState::provisional("/workspace/src/App.vue.tsx".to_string());
        let owner_aware = ProviderSyncState {
            owner_binding: ProviderOwnerBinding::Owned("/workspace/tsconfig.json".to_string()),
            ide_path: Some("/workspace/src/App.vue.jsx".to_string()),
            api_path: Some("/workspace/src/App.vue.ts".to_string()),
            ..Default::default()
        };

        let stale = stale_paths_for_transition(&provisional, &owner_aware);
        assert_eq!(stale.len(), 1, "different IDE path should be stale");
        assert_eq!(stale[0].1, "/workspace/src/App.vue.tsx");
    }

    #[test]
    fn remove_sync_state_returns_all_active_paths() {
        let states = DashMap::new();
        states.insert(
            "/workspace/src/util.ts".to_string(),
            ProviderSyncState {
                owner_binding: ProviderOwnerBinding::Owned("/workspace".to_string()),
                shadow_path: Some("/workspace/src/util.ts".to_string()),
                shadow_background_loaded: true,
                ..Default::default()
            },
        );

        let removed = remove_sync_state(&states, "/workspace/src/util.ts")
            .expect("source-keyed sync state should be removable");

        assert_eq!(
            removed.active_paths(),
            vec![(
                ProviderPathKind::Shadow,
                "/workspace/src/util.ts".to_string()
            )]
        );
        assert!(states.is_empty());
    }
}
