use crate::types::{StoreApiClassification, VueApiClassification};

/// Classify a Vue API name into a known category.
pub fn classify_vue_api(name: &str) -> VueApiClassification {
    match name {
        // Reactivity
        "ref" => VueApiClassification::Ref,
        "shallowRef" => VueApiClassification::ShallowRef,
        "reactive" => VueApiClassification::Reactive,
        "shallowReactive" => VueApiClassification::ShallowReactive,
        "computed" => VueApiClassification::Computed,
        "toRef" => VueApiClassification::ToRef,
        "toRefs" => VueApiClassification::ToRefs,
        "customRef" => VueApiClassification::CustomRef,
        "triggerRef" => VueApiClassification::TriggerRef,
        "readonly" => VueApiClassification::Readonly,
        "shallowReadonly" => VueApiClassification::ShallowReadonly,
        // Lifecycle
        "onMounted" => VueApiClassification::OnMounted,
        "onUnmounted" => VueApiClassification::OnUnmounted,
        "onBeforeMount" => VueApiClassification::OnBeforeMount,
        "onBeforeUnmount" => VueApiClassification::OnBeforeUnmount,
        "onUpdated" => VueApiClassification::OnUpdated,
        "onBeforeUpdate" => VueApiClassification::OnBeforeUpdate,
        "onActivated" => VueApiClassification::OnActivated,
        "onDeactivated" => VueApiClassification::OnDeactivated,
        "onErrorCaptured" => VueApiClassification::OnErrorCaptured,
        "onRenderTracked" => VueApiClassification::OnRenderTracked,
        "onRenderTriggered" => VueApiClassification::OnRenderTriggered,
        "onServerPrefetch" => VueApiClassification::OnServerPrefetch,
        // Watchers
        "watch" => VueApiClassification::Watch,
        "watchEffect" => VueApiClassification::WatchEffect,
        "watchPostEffect" => VueApiClassification::WatchPostEffect,
        "watchSyncEffect" => VueApiClassification::WatchSyncEffect,
        // DI
        "provide" => VueApiClassification::Provide,
        "inject" => VueApiClassification::Inject,
        // Template
        "useSlots" => VueApiClassification::UseSlots,
        "useAttrs" => VueApiClassification::UseAttrs,
        "useTemplateRef" => VueApiClassification::UseTemplateRef,
        "useId" => VueApiClassification::UseId,
        // Instance
        "getCurrentInstance" => VueApiClassification::GetCurrentInstance,
        "nextTick" => VueApiClassification::NextTick,
        // Model helper (Vue 3.4+)
        "useModel" => VueApiClassification::UseModel,
        // Watcher cleanup (Vue 3.5+)
        "onWatcherCleanup" => VueApiClassification::OnWatcherCleanup,
        // DI utility (Vue 3.3+)
        "hasInjectionContext" => VueApiClassification::HasInjectionContext,
        // Macros
        "defineProps" => VueApiClassification::DefineProps,
        "defineEmits" => VueApiClassification::DefineEmits,
        "defineModel" => VueApiClassification::DefineModel,
        "defineExpose" => VueApiClassification::DefineExpose,
        "defineOptions" => VueApiClassification::DefineOptions,
        "defineSlots" => VueApiClassification::DefineSlots,
        "withDefaults" => VueApiClassification::WithDefaults,
        // Component
        "defineComponent" => VueApiClassification::DefineComponent,
        "defineAsyncComponent" => VueApiClassification::DefineAsyncComponent,
        // Other known
        "h" => VueApiClassification::H,
        "createApp" => VueApiClassification::CreateApp,
        "createSSRApp" => VueApiClassification::CreateSSRApp,
        _ => VueApiClassification::Other,
    }
}

/// Returns true if this classification represents a reactivity primitive.
pub fn is_reactivity_api(api: VueApiClassification) -> bool {
    matches!(
        api,
        VueApiClassification::Ref
            | VueApiClassification::ShallowRef
            | VueApiClassification::Reactive
            | VueApiClassification::ShallowReactive
            | VueApiClassification::Computed
            | VueApiClassification::ToRef
            | VueApiClassification::ToRefs
            | VueApiClassification::CustomRef
            | VueApiClassification::Readonly
            | VueApiClassification::ShallowReadonly
    )
}

/// Returns true if this classification represents a lifecycle hook.
pub fn is_lifecycle_api(api: VueApiClassification) -> bool {
    matches!(
        api,
        VueApiClassification::OnMounted
            | VueApiClassification::OnUnmounted
            | VueApiClassification::OnBeforeMount
            | VueApiClassification::OnBeforeUnmount
            | VueApiClassification::OnUpdated
            | VueApiClassification::OnBeforeUpdate
            | VueApiClassification::OnActivated
            | VueApiClassification::OnDeactivated
            | VueApiClassification::OnErrorCaptured
            | VueApiClassification::OnRenderTracked
            | VueApiClassification::OnRenderTriggered
            | VueApiClassification::OnServerPrefetch
    )
}

/// Returns true if this classification represents a watcher API.
pub fn is_watcher_api(api: VueApiClassification) -> bool {
    matches!(
        api,
        VueApiClassification::Watch
            | VueApiClassification::WatchEffect
            | VueApiClassification::WatchPostEffect
            | VueApiClassification::WatchSyncEffect
            | VueApiClassification::OnWatcherCleanup
    )
}

/// Classify a store/state management API based on the function name and import source.
///
/// Returns `Some(StoreApiClassification)` for known Pinia/Vuex APIs.
/// Returns `None` for non-store APIs.
pub fn classify_store_api(name: &str, import_source: &str) -> Option<StoreApiClassification> {
    match import_source {
        "pinia" => match name {
            "defineStore" => Some(StoreApiClassification::PiniaDefineStore),
            "storeToRefs" => Some(StoreApiClassification::PiniaStoreToRefs),
            "mapState" => Some(StoreApiClassification::PiniaMapState),
            "mapGetters" => Some(StoreApiClassification::PiniaMapGetters),
            "mapActions" => Some(StoreApiClassification::PiniaMapActions),
            "mapWritableState" => Some(StoreApiClassification::PiniaMapWritableState),
            "createPinia" => Some(StoreApiClassification::PiniaCreatePinia),
            _ => None,
        },
        "vuex" => match name {
            "createStore" => Some(StoreApiClassification::VuexCreateStore),
            "useStore" => Some(StoreApiClassification::VuexUseStore),
            "mapState" => Some(StoreApiClassification::VuexMapState),
            "mapGetters" => Some(StoreApiClassification::VuexMapGetters),
            "mapMutations" => Some(StoreApiClassification::VuexMapMutations),
            "mapActions" => Some(StoreApiClassification::VuexMapActions),
            _ => None,
        },
        _ => None,
    }
}

/// Returns true if a function call looks like a convention-based store composable.
///
/// Convention: callee matches `use*Store` and the import source contains `/store` or `/stores`.
pub fn is_store_composable_call(callee: &str, import_source: &str) -> bool {
    callee.starts_with("use")
        && callee.ends_with("Store")
        && callee.len() > "useStore".len()
        && (import_source.contains("/store")
            || import_source.contains("/stores")
            || import_source.contains("\\store")
            || import_source.contains("\\stores"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// @ai-generated
    #[test]
    fn classify_known_apis() {
        assert_eq!(classify_vue_api("ref"), VueApiClassification::Ref);
        assert_eq!(classify_vue_api("computed"), VueApiClassification::Computed);
        assert_eq!(
            classify_vue_api("onMounted"),
            VueApiClassification::OnMounted
        );
        assert_eq!(classify_vue_api("watch"), VueApiClassification::Watch);
        assert_eq!(
            classify_vue_api("defineProps"),
            VueApiClassification::DefineProps
        );
    }

    /// @ai-generated
    #[test]
    fn classify_unknown_api() {
        assert_eq!(classify_vue_api("unknownApi"), VueApiClassification::Other);
    }

    /// @ai-generated
    #[test]
    fn reactivity_checks() {
        assert!(is_reactivity_api(VueApiClassification::Ref));
        assert!(is_reactivity_api(VueApiClassification::Computed));
        assert!(!is_reactivity_api(VueApiClassification::OnMounted));
    }

    /// @ai-generated - Vue 3.3+ / 3.4+ / 3.5+ APIs
    #[test]
    fn classify_newer_apis() {
        assert_eq!(classify_vue_api("useId"), VueApiClassification::UseId);
        assert_eq!(classify_vue_api("useModel"), VueApiClassification::UseModel);
        assert_eq!(
            classify_vue_api("onWatcherCleanup"),
            VueApiClassification::OnWatcherCleanup
        );
        assert_eq!(
            classify_vue_api("hasInjectionContext"),
            VueApiClassification::HasInjectionContext
        );
    }

    /// @ai-generated - onWatcherCleanup is classified as a watcher API
    #[test]
    fn on_watcher_cleanup_is_watcher_api() {
        assert!(is_watcher_api(VueApiClassification::OnWatcherCleanup));
    }

    /// @ai-generated - Classification is case-sensitive
    #[test]
    fn classify_is_case_sensitive() {
        assert_eq!(classify_vue_api("Ref"), VueApiClassification::Other);
        assert_eq!(classify_vue_api("REF"), VueApiClassification::Other);
        assert_eq!(classify_vue_api("Computed"), VueApiClassification::Other);
        assert_eq!(classify_vue_api("OnMounted"), VueApiClassification::Other);
    }

    /// @ai-generated - Whitespace and near-matches are Other
    #[test]
    fn classify_near_misses() {
        assert_eq!(classify_vue_api("ref "), VueApiClassification::Other);
        assert_eq!(classify_vue_api(" ref"), VueApiClassification::Other);
        assert_eq!(classify_vue_api("refs"), VueApiClassification::Other);
        assert_eq!(classify_vue_api(""), VueApiClassification::Other);
    }

    // ── Store classifier tests ──

    /// @ai-generated - Pinia APIs are classified correctly
    #[test]
    fn classify_pinia_apis() {
        assert_eq!(
            classify_store_api("defineStore", "pinia"),
            Some(StoreApiClassification::PiniaDefineStore)
        );
        assert_eq!(
            classify_store_api("storeToRefs", "pinia"),
            Some(StoreApiClassification::PiniaStoreToRefs)
        );
        assert_eq!(
            classify_store_api("mapState", "pinia"),
            Some(StoreApiClassification::PiniaMapState)
        );
        assert_eq!(
            classify_store_api("mapGetters", "pinia"),
            Some(StoreApiClassification::PiniaMapGetters)
        );
        assert_eq!(
            classify_store_api("mapActions", "pinia"),
            Some(StoreApiClassification::PiniaMapActions)
        );
        assert_eq!(
            classify_store_api("mapWritableState", "pinia"),
            Some(StoreApiClassification::PiniaMapWritableState)
        );
        assert_eq!(
            classify_store_api("createPinia", "pinia"),
            Some(StoreApiClassification::PiniaCreatePinia)
        );
    }

    /// @ai-generated - Vuex APIs are classified correctly
    #[test]
    fn classify_vuex_apis() {
        assert_eq!(
            classify_store_api("createStore", "vuex"),
            Some(StoreApiClassification::VuexCreateStore)
        );
        assert_eq!(
            classify_store_api("useStore", "vuex"),
            Some(StoreApiClassification::VuexUseStore)
        );
        assert_eq!(
            classify_store_api("mapState", "vuex"),
            Some(StoreApiClassification::VuexMapState)
        );
        assert_eq!(
            classify_store_api("mapGetters", "vuex"),
            Some(StoreApiClassification::VuexMapGetters)
        );
        assert_eq!(
            classify_store_api("mapMutations", "vuex"),
            Some(StoreApiClassification::VuexMapMutations)
        );
        assert_eq!(
            classify_store_api("mapActions", "vuex"),
            Some(StoreApiClassification::VuexMapActions)
        );
    }

    /// @ai-generated - Unknown APIs return None
    #[test]
    fn classify_store_api_unknown() {
        assert_eq!(classify_store_api("unknownFn", "pinia"), None);
        assert_eq!(classify_store_api("unknownFn", "vuex"), None);
        assert_eq!(classify_store_api("defineStore", "vue"), None);
        assert_eq!(classify_store_api("ref", "pinia"), None);
    }

    /// @ai-generated - Convention-based store composable detection
    #[test]
    fn convention_based_store_composable() {
        assert!(is_store_composable_call("useUserStore", "@/stores/user"));
        assert!(is_store_composable_call("useAuthStore", "../stores/auth"));
        assert!(is_store_composable_call("useCartStore", "~/store/cart"));
        assert!(is_store_composable_call(
            "useSettingsStore",
            "@/stores/settings.ts"
        ));
    }

    /// @ai-generated - Convention-based detection rejects non-store composables
    #[test]
    fn convention_based_store_composable_negative() {
        // Wrong naming pattern
        assert!(!is_store_composable_call("useRouter", "@/stores/router"));
        assert!(!is_store_composable_call("useStore", "@/stores/main")); // too short, just "useStore"
                                                                         // Wrong import source
        assert!(!is_store_composable_call("useUserStore", "@/utils/user"));
        assert!(!is_store_composable_call("useAuthStore", "vue-router"));
        // Not a composable at all
        assert!(!is_store_composable_call("createStore", "@/stores/main"));
    }
}
