use crate::project_resolver::NativeProjectResolver;
use dashmap::DashMap;

#[derive(Debug, Clone)]
pub struct ResolverSnapshot {
    pub generation: u64,
    pub resolver: NativeProjectResolver,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderPathKind {
    Ide,
    Api,
    Shadow,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProviderSyncState {
    pub owner_tsconfig_path: Option<String>,
    pub provider_root: Option<String>,
    pub project_config_path: Option<String>,
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

pub fn project_config_path_for_provider_root(provider_root: &str) -> String {
    format!("{provider_root}/tsconfig.json")
}

pub fn vue_sync_state_for_source(
    resolver: &NativeProjectResolver,
    source_id: &str,
    is_jsx: bool,
) -> Option<ProviderSyncState> {
    let owner = resolver.owner_for_file(source_id)?;
    Some(ProviderSyncState {
        owner_tsconfig_path: owner.tsconfig_path.clone(),
        provider_root: Some(owner.provider_root.clone()),
        project_config_path: Some(project_config_path_for_provider_root(&owner.provider_root)),
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
    Some(ProviderSyncState {
        owner_tsconfig_path: owner.tsconfig_path.clone(),
        provider_root: Some(owner.provider_root.clone()),
        project_config_path: Some(project_config_path_for_provider_root(&owner.provider_root)),
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
    let mut stale = Vec::new();
    for kind in [
        ProviderPathKind::Ide,
        ProviderPathKind::Api,
        ProviderPathKind::Shadow,
    ] {
        let previous_path = previous.path_for_kind(kind);
        let next_path = next.path_for_kind(kind);
        if let Some(path) = previous_path.filter(|path| Some(*path) != next_path) {
            stale.push((kind, path.to_string()));
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
    fn vue_sync_state_tracks_owner_specific_provider_root() {
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
            state.owner_tsconfig_path.as_deref(),
            Some("/workspace/pkg-a/tsconfig.json")
        );
        assert!(
            state
                .provider_root
                .as_deref()
                .unwrap()
                .contains("/workspace/pkg-a/.verter/ide/"),
            "provider root should be owner-specific"
        );
        assert!(
            state.ide_path.as_deref().unwrap().ends_with(".tsx"),
            "provider IDE path must remain TSX/JSX-backed"
        );
        assert!(
            state.api_path.as_deref().unwrap().ends_with(".vue.ts"),
            "Vue imports must keep resolving through .vue.ts"
        );
    }

    #[test]
    fn stale_paths_only_include_paths_that_change() {
        let previous = ProviderSyncState {
            ide_path: Some("/workspace/.verter/ide/a/src/App.vue.tsx".to_string()),
            api_path: Some("/workspace/.verter/ide/a/src/App.vue.ts".to_string()),
            ..Default::default()
        };
        let next = ProviderSyncState {
            ide_path: Some("/workspace/.verter/ide/b/src/App.vue.tsx".to_string()),
            api_path: Some("/workspace/.verter/ide/a/src/App.vue.ts".to_string()),
            ..Default::default()
        };

        assert_eq!(
            stale_paths_for_transition(&previous, &next),
            vec![(
                ProviderPathKind::Ide,
                "/workspace/.verter/ide/a/src/App.vue.tsx".to_string()
            )]
        );
    }

    #[test]
    fn prepare_sync_transition_preserves_background_flags_for_unchanged_paths() {
        let states = DashMap::new();
        states.insert(
            "/workspace/src/App.vue".to_string(),
            ProviderSyncState {
                ide_path: Some("/workspace/.verter/ide/a/src/App.vue.tsx".to_string()),
                api_path: Some("/workspace/.verter/ide/a/src/App.vue.ts".to_string()),
                ide_background_loaded: true,
                api_background_loaded: true,
                ..Default::default()
            },
        );

        let transition = prepare_sync_transition(
            &states,
            "/workspace/src/App.vue",
            ProviderSyncState {
                ide_path: Some("/workspace/.verter/ide/a/src/App.vue.tsx".to_string()),
                api_path: Some("/workspace/.verter/ide/a/src/App.vue.ts".to_string()),
                ..Default::default()
            },
        );

        assert!(transition.stale_paths.is_empty());
        assert!(transition.next.ide_background_loaded);
        assert!(transition.next.api_background_loaded);
    }

    #[test]
    fn remove_sync_state_returns_all_active_paths() {
        let states = DashMap::new();
        states.insert(
            "/workspace/src/util.ts".to_string(),
            ProviderSyncState {
                shadow_path: Some("/workspace/.verter/ide/a/src/util.__verter__.ts".to_string()),
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
                "/workspace/.verter/ide/a/src/util.__verter__.ts".to_string()
            )]
        );
        assert!(states.is_empty());
    }
}
