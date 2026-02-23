use crate::types::VueApiClassification;

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
}
