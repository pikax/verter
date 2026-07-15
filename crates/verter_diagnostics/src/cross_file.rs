//! Cross-file analysis snapshot and component diagnostic functions.
//!
//! Pre-computed summary of project-wide issues derived from [`ProjectIndex`].
//! Rules receive this as an immutable snapshot rather than querying the index directly,
//! keeping rule implementations simple and testable.
//!
//! Also provides pure analysis functions for cross-file component validation
//! (unknown props/v-models). These are called by the LSP with a resolver callback.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use verter_semantic::analysis::project_index::ProjectIndex;
use verter_semantic::analysis::template::{TemplateComponentUsage, TemplatePropUsage};

/// Pre-computed cross-file analysis data for lint rules.
#[derive(Debug, Default)]
pub struct CrossFileSnapshot {
    /// inject() calls with no matching provide() in the project.
    pub missing_providers: Vec<MissingProviderEntry>,
    /// provide() keys that are never injected by any file.
    pub unused_provides: Vec<UnusedProvideEntry>,
    /// Composable call chains with hidden side effects.
    pub composable_chains: Vec<ComposableChainEntry>,
    /// Multiple Vue installations detected in node_modules.
    /// Populated by the caller (host, LSP, or build tool) since it requires
    /// filesystem scanning outside the project index.
    pub duplicate_vue_versions: Vec<DuplicateVueEntry>,
    /// Store composable called outside `<script setup>` or setup function.
    pub store_outside_setup: Vec<StoreOutsideSetupEntry>,
    /// Pinia store destructured without `storeToRefs()` (reactivity loss).
    pub store_reactivity_loss: Vec<StoreReactivityLossEntry>,
    /// Circular store-to-store dependency cycles.
    pub circular_store_deps: Vec<CircularStoreDepsEntry>,
}

/// An inject() call with no matching provide() in the project.
#[derive(Debug, Clone)]
pub struct MissingProviderEntry {
    /// The injection key.
    pub key: String,
    /// File containing the inject() call.
    pub file: PathBuf,
    /// Byte span of the inject() call in the file.
    pub span: verter_span::Span,
}

/// A provide() key that is never injected by any file.
#[derive(Debug, Clone)]
pub struct UnusedProvideEntry {
    /// The injection key.
    pub key: String,
    /// File containing the provide() call.
    pub file: PathBuf,
    /// Byte span of the provide() call in the file.
    pub span: verter_span::Span,
}

/// A composable with hidden side effects (lifecycle hooks, watchers, provide/inject).
#[derive(Debug, Clone)]
pub struct ComposableChainEntry {
    /// Name of the composable function (e.g., "useMouse").
    pub composable_name: String,
    /// The call chain from the file to the composable (e.g., ["useMouse", "useEventListener"]).
    pub chain: Vec<String>,
    /// Hidden lifecycle hooks in the chain.
    pub lifecycle_hooks: Vec<String>,
    /// Hidden watchers in the chain.
    pub has_watchers: bool,
    /// Hidden provide/inject in the chain.
    pub has_provide_inject: bool,
    /// Byte span where the composable is called.
    pub span: verter_span::Span,
}

/// A Vue installation found in node_modules (possibly duplicated).
#[derive(Debug, Clone)]
pub struct DuplicateVueEntry {
    /// Filesystem path to the Vue package (e.g., "node_modules/vue" or nested).
    pub path: String,
    /// Semver version string (e.g., "3.4.21").
    pub version: String,
}

/// A store composable called outside `<script setup>` or setup function.
#[derive(Debug, Clone)]
pub struct StoreOutsideSetupEntry {
    /// The callee function name (e.g., `useUserStore`).
    pub callee: String,
    /// File containing the store call.
    pub file: PathBuf,
    /// Byte span of the call.
    pub span: verter_span::Span,
}

/// A circular dependency cycle between stores.
#[derive(Debug, Clone)]
pub struct CircularStoreDepsEntry {
    /// The cycle as a list of store IDs (e.g., ["A", "B", "C", "A"]).
    pub cycle: Vec<String>,
    /// Byte span of the first store in the cycle in the current file.
    pub span: verter_span::Span,
}

/// A Pinia store destructured without `storeToRefs()`, causing reactivity loss.
#[derive(Debug, Clone)]
pub struct StoreReactivityLossEntry {
    /// The callee function name (e.g., `useUserStore`).
    pub callee: String,
    /// Property names destructured without storeToRefs.
    pub destructured_props: Vec<String>,
    /// File containing the destructured store.
    pub file: PathBuf,
    /// Byte span of the call.
    pub span: verter_span::Span,
}

/// Build a [`CrossFileSnapshot`] for a specific file from the project index.
///
/// Extracts only the cross-file issues relevant to the given file path,
/// plus project-wide issues like unused provides.
pub fn build_cross_file_snapshot(project: &ProjectIndex, file_path: &Path) -> CrossFileSnapshot {
    let mut snapshot = CrossFileSnapshot::default();

    // 1. Missing providers: inject() calls in this file with no matching provide()
    let validation = project.validate_file_injects(file_path);
    for entry in validation.missing_providers {
        snapshot.missing_providers.push(MissingProviderEntry {
            key: entry.key,
            file: file_path.to_path_buf(),
            span: verter_span::Span::new(entry.start, entry.end),
        });
    }

    // 2. Unused provides: provide() keys in this file never injected anywhere
    if let Some(info) = project.get_file(file_path) {
        for provide in &info.provides {
            if let Some(key) = &provide.key {
                if project.files_injecting(key).next().is_none() {
                    snapshot.unused_provides.push(UnusedProvideEntry {
                        key: key.clone(),
                        file: file_path.to_path_buf(),
                        span: verter_span::Span::new(provide.start, provide.end),
                    });
                }
            }
        }
    }

    // 3. Composable chains with hidden side effects
    // Walk exported functions from dependency files to find hidden lifecycle/watcher usage.
    // This data comes from the analysis of imported composables.
    // (Populated when deep analysis is available via FUNC_RETURNS scope)

    // 4. Store reactivity loss: destructured without storeToRefs()
    if let Some(info) = project.get_file(file_path) {
        for usage in &info.store_usages {
            if usage.destructured_without_store_to_refs {
                snapshot
                    .store_reactivity_loss
                    .push(StoreReactivityLossEntry {
                        callee: usage.callee.clone(),
                        destructured_props: usage.destructured_props.clone(),
                        file: file_path.to_path_buf(),
                        span: verter_span::Span::new(usage.start, usage.end),
                    });
            }
        }
    }

    // 5. Circular store dependency detection
    if let Some(info) = project.get_file(file_path) {
        for def in &info.store_definitions {
            if let Some(store_id) = &def.store_id {
                if let Some(cycle) = detect_store_cycle(project, store_id) {
                    snapshot.circular_store_deps.push(CircularStoreDepsEntry {
                        cycle,
                        span: verter_span::Span::new(def.start, def.end),
                    });
                }
            }
        }
    }

    snapshot
}

/// DFS cycle detection starting from `start_id` in the store dependency graph.
fn detect_store_cycle(project: &ProjectIndex, start_id: &str) -> Option<Vec<String>> {
    let mut visited = HashSet::new();
    let mut stack = vec![(start_id.to_string(), vec![start_id.to_string()])];

    while let Some((current, path)) = stack.pop() {
        if !visited.insert(current.clone()) {
            continue;
        }
        let deps = project.store_dependencies(&current);
        for dep in deps {
            if dep == start_id {
                let mut cycle = path.clone();
                cycle.push(dep.to_string());
                return Some(cycle);
            }
            if !visited.contains(dep) {
                let mut new_path = path.clone();
                new_path.push(dep.to_string());
                stack.push((dep.to_string(), new_path));
            }
        }
    }
    None
}

// ── Component cross-file analysis ────────────────────────────────────

/// Abstracted child component info for prop/model validation.
///
/// The LSP resolves the child file, extracts this info, and passes it
/// to the analysis functions below.
#[derive(Debug, Clone)]
pub struct ChildComponentInfo {
    /// camelCase prop names from `defineProps()`.
    pub prop_names: HashSet<String>,
    /// Model names from `defineModel()`. `"modelValue"` for unnamed `defineModel()`.
    pub model_names: HashSet<String>,
    /// Whether the child suppresses prop checking
    /// (`useAttrs()` or `inheritAttrs: false`).
    pub suppresses_prop_checks: bool,
}

/// An unknown prop found on a component usage.
#[derive(Debug, Clone)]
pub struct UnknownPropEntry {
    pub component_name: String,
    pub prop_name: String,
    pub import_source: String,
    pub span_start: u32,
    pub span_end: u32,
}

/// An unknown v-model found on a component usage.
#[derive(Debug, Clone)]
pub struct UnknownModelEntry {
    pub component_name: String,
    pub model_name: String,
    pub import_source: String,
    pub span_start: u32,
    pub span_end: u32,
}

/// Attributes that are always valid on any component (Vue fallthrough attrs).
const BUILTIN_ATTRS: &[&str] = &["class", "style"];

/// Convert kebab-case to camelCase for prop name comparison.
pub fn kebab_to_camel(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut upper_next = false;
    for ch in name.chars() {
        if ch == '-' {
            upper_next = true;
            continue;
        }
        if upper_next {
            for uc in ch.to_uppercase() {
                out.push(uc);
            }
            upper_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// Check if a single prop is unknown (not defined by the child).
fn is_unknown_prop(prop: &TemplatePropUsage, defined_props: &HashSet<String>) -> bool {
    if prop.from_spread {
        return false;
    }
    if BUILTIN_ATTRS.contains(&prop.name.as_str()) {
        return false;
    }
    let camel_name = kebab_to_camel(&prop.name);
    !defined_props.contains(&camel_name)
}

/// Find unknown props across all component usages.
///
/// `resolve_child` maps import source to child component info.
pub fn find_unknown_props(
    components: &[TemplateComponentUsage],
    resolve_child: &dyn Fn(&str) -> Option<ChildComponentInfo>,
) -> Vec<UnknownPropEntry> {
    let mut results = Vec::new();

    for comp in components {
        if comp.is_dynamic || comp.has_spread {
            continue;
        }

        let import_source = match &comp.import_source {
            Some(s) => s.as_str(),
            None => continue,
        };

        let child = match resolve_child(import_source) {
            Some(c) => c,
            None => continue,
        };

        if child.suppresses_prop_checks {
            continue;
        }

        for prop in &comp.props {
            if is_unknown_prop(prop, &child.prop_names) {
                results.push(UnknownPropEntry {
                    component_name: comp.name.clone(),
                    prop_name: prop.name.clone(),
                    import_source: import_source.to_string(),
                    span_start: prop.span.start,
                    span_end: prop.span.end,
                });
            }
        }
    }

    results
}

/// Find unknown v-models across all component usages.
///
/// `resolve_child` maps import source to child component info.
pub fn find_unknown_models(
    components: &[TemplateComponentUsage],
    resolve_child: &dyn Fn(&str) -> Option<ChildComponentInfo>,
) -> Vec<UnknownModelEntry> {
    let mut results = Vec::new();

    for comp in components {
        if comp.is_dynamic || comp.v_models.is_empty() {
            continue;
        }

        let import_source = match &comp.import_source {
            Some(s) => s.as_str(),
            None => continue,
        };

        let child = match resolve_child(import_source) {
            Some(c) => c,
            None => continue,
        };

        for vmodel in &comp.v_models {
            if !child.model_names.contains(&vmodel.binding_name) {
                results.push(UnknownModelEntry {
                    component_name: comp.name.clone(),
                    model_name: vmodel.binding_name.clone(),
                    import_source: import_source.to_string(),
                    span_start: vmodel.span.start,
                    span_end: vmodel.span.end,
                });
            }
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_semantic::analysis::file_usage::{
        FileUsageFlags, FileUsageInfoOwned, InjectUsageOwned, ProvideUsageOwned,
    };

    fn make_project_with_provide_inject() -> (ProjectIndex, PathBuf, PathBuf) {
        let mut index = ProjectIndex::default();
        let provider = PathBuf::from("/src/Provider.vue");
        let consumer = PathBuf::from("/src/Consumer.vue");

        // Provider provides "theme"
        let mut provider_info = FileUsageInfoOwned::default();
        provider_info.provides.push(ProvideUsageOwned {
            key: Some("theme".to_string()),
            is_dynamic_key: false,
            start: 10,
            end: 30,
        });
        provider_info.set_flags(FileUsageFlags::HAS_PROVIDE | FileUsageFlags::IS_SETUP_SCRIPT);
        index.add_file(provider.clone(), provider_info);

        // Consumer injects "theme" and "config" (no provider for config)
        let mut consumer_info = FileUsageInfoOwned::default();
        consumer_info.injects.push(InjectUsageOwned {
            key: Some("theme".to_string()),
            is_dynamic_key: false,
            has_default: false,
            binding_name: None,
            start: 10,
            end: 30,
        });
        consumer_info.injects.push(InjectUsageOwned {
            key: Some("config".to_string()),
            is_dynamic_key: false,
            has_default: false,
            binding_name: None,
            start: 40,
            end: 60,
        });
        consumer_info.set_flags(FileUsageFlags::HAS_INJECT | FileUsageFlags::IS_SETUP_SCRIPT);
        index.add_file(consumer.clone(), consumer_info);

        (index, provider, consumer)
    }

    #[test]
    fn missing_provider_detected() {
        let (index, _provider, consumer) = make_project_with_provide_inject();
        let snapshot = build_cross_file_snapshot(&index, &consumer);

        assert_eq!(snapshot.missing_providers.len(), 1);
        assert_eq!(snapshot.missing_providers[0].key, "config");
        assert_eq!(snapshot.missing_providers[0].span.start, 40);
    }

    #[test]
    fn unused_provide_detected() {
        let (index, provider, _consumer) = make_project_with_provide_inject();
        let snapshot = build_cross_file_snapshot(&index, &provider);

        // "theme" IS injected by consumer, so it should not be unused
        assert_eq!(snapshot.unused_provides.len(), 0);
    }

    #[test]
    fn unused_provide_when_no_consumer() {
        let mut index = ProjectIndex::default();
        let provider = PathBuf::from("/src/Provider.vue");

        let mut info = FileUsageInfoOwned::default();
        info.provides.push(ProvideUsageOwned {
            key: Some("orphan-key".to_string()),
            is_dynamic_key: false,
            start: 5,
            end: 25,
        });
        info.set_flags(FileUsageFlags::HAS_PROVIDE);
        index.add_file(provider.clone(), info);

        let snapshot = build_cross_file_snapshot(&index, &provider);
        assert_eq!(snapshot.unused_provides.len(), 1);
        assert_eq!(snapshot.unused_provides[0].key, "orphan-key");
    }

    #[test]
    fn empty_project_no_issues() {
        let index = ProjectIndex::default();
        let file = PathBuf::from("/src/App.vue");
        let snapshot = build_cross_file_snapshot(&index, &file);

        assert!(snapshot.missing_providers.is_empty());
        assert!(snapshot.unused_provides.is_empty());
        assert!(snapshot.composable_chains.is_empty());
    }

    // ── Component cross-file analysis tests ─────────────────────────

    use verter_semantic::analysis::template::{
        PropValueConstness, TemplateComponentUsage, TemplateComponentVModel, TemplatePropUsage,
    };

    fn make_child(props: &[&str], models: &[&str], suppress: bool) -> ChildComponentInfo {
        ChildComponentInfo {
            prop_names: props.iter().map(|s| s.to_string()).collect(),
            model_names: models.iter().map(|s| s.to_string()).collect(),
            suppresses_prop_checks: suppress,
        }
    }

    fn make_prop(name: &str) -> TemplatePropUsage {
        TemplatePropUsage {
            name: name.to_string(),
            is_bound: true,
            expression: None,
            constness: PropValueConstness::Dynamic,
            referenced_bindings: vec![],
            from_spread: false,
            span: verter_span::Span::new(10, 20),
            name_span: verter_span::Span::new(0, 0),
            is_shorthand: false,
        }
    }

    fn make_comp(
        name: &str,
        import_source: &str,
        props: Vec<TemplatePropUsage>,
    ) -> TemplateComponentUsage {
        TemplateComponentUsage {
            name: name.to_string(),
            import_source: Some(import_source.to_string()),
            is_dynamic: false,
            props,
            has_spread: false,
            slots_used: vec![],
            static_classes: vec![],
            has_dynamic_class: false,
            dynamic_classes: vec![],
            v_models: vec![],
            bindings: vec![],
            events: vec![],
            span: verter_span::Span::new(0, 50),
        }
    }

    #[test]
    fn unknown_prop_detected() {
        let comps = vec![make_comp("Child", "./Child.vue", vec![make_prop("foo")])];
        let child = make_child(&["msg"], &[], false);

        let unknowns = find_unknown_props(&comps, &|_| Some(child.clone()));
        assert_eq!(unknowns.len(), 1);
        assert_eq!(unknowns[0].prop_name, "foo");
        assert_eq!(unknowns[0].component_name, "Child");
        assert!(
            !unknowns.iter().any(|u| u.prop_name == "msg"),
            "msg should not be flagged"
        );
    }

    #[test]
    fn known_prop_passes() {
        let comps = vec![make_comp("Child", "./Child.vue", vec![make_prop("msg")])];
        let child = make_child(&["msg"], &[], false);

        let unknowns = find_unknown_props(&comps, &|_| Some(child.clone()));
        assert!(unknowns.is_empty());
    }

    #[test]
    fn builtin_attrs_not_flagged() {
        let comps = vec![make_comp(
            "Child",
            "./Child.vue",
            vec![make_prop("class"), make_prop("style")],
        )];
        let child = make_child(&[], &[], false);

        let unknowns = find_unknown_props(&comps, &|_| Some(child.clone()));
        assert!(unknowns.is_empty(), "builtin attrs should not be flagged");
    }

    #[test]
    fn kebab_case_matches_camel() {
        let comps = vec![make_comp(
            "Child",
            "./Child.vue",
            vec![make_prop("some-prop")],
        )];
        let child = make_child(&["someProp"], &[], false);

        let unknowns = find_unknown_props(&comps, &|_| Some(child.clone()));
        assert!(unknowns.is_empty(), "kebab-case should match camelCase");
    }

    #[test]
    fn suppresses_prop_checks() {
        let comps = vec![make_comp(
            "Child",
            "./Child.vue",
            vec![make_prop("unknown")],
        )];
        let child = make_child(&[], &[], true);

        let unknowns = find_unknown_props(&comps, &|_| Some(child.clone()));
        assert!(unknowns.is_empty(), "suppression should skip all checks");
    }

    #[test]
    fn dynamic_component_skipped() {
        let comps = vec![TemplateComponentUsage {
            name: "component".to_string(),
            import_source: None,
            is_dynamic: true,
            props: vec![make_prop("foo")],
            has_spread: false,
            slots_used: vec![],
            static_classes: vec![],
            has_dynamic_class: false,
            dynamic_classes: vec![],
            v_models: vec![],
            bindings: vec![],
            events: vec![],
            span: verter_span::Span::new(0, 50),
        }];

        let unknowns = find_unknown_props(&comps, &|_| None);
        assert!(unknowns.is_empty());
    }

    #[test]
    fn spread_skips_component() {
        let comps = vec![TemplateComponentUsage {
            name: "Child".to_string(),
            import_source: Some("./Child.vue".to_string()),
            is_dynamic: false,
            props: vec![make_prop("extra")],
            has_spread: true,
            slots_used: vec![],
            static_classes: vec![],
            has_dynamic_class: false,
            dynamic_classes: vec![],
            v_models: vec![],
            bindings: vec![],
            events: vec![],
            span: verter_span::Span::new(0, 50),
        }];
        let child = make_child(&[], &[], false);

        let unknowns = find_unknown_props(&comps, &|_| Some(child.clone()));
        assert!(unknowns.is_empty(), "spread should skip component");
    }

    #[test]
    fn unknown_vmodel_detected() {
        let comps = vec![TemplateComponentUsage {
            name: "Child".to_string(),
            import_source: Some("./Child.vue".to_string()),
            is_dynamic: false,
            props: vec![],
            has_spread: false,
            slots_used: vec![],
            static_classes: vec![],
            has_dynamic_class: false,
            dynamic_classes: vec![],
            v_models: vec![TemplateComponentVModel {
                binding_name: "title".to_string(),
                span: verter_span::Span::new(10, 30),
            }],
            bindings: vec![],
            events: vec![],
            span: verter_span::Span::new(0, 50),
        }];
        let child = make_child(&[], &[], false);

        let unknowns = find_unknown_models(&comps, &|_| Some(child.clone()));
        assert_eq!(unknowns.len(), 1);
        assert_eq!(unknowns[0].model_name, "title");
    }

    #[test]
    fn known_vmodel_passes() {
        let comps = vec![TemplateComponentUsage {
            name: "Child".to_string(),
            import_source: Some("./Child.vue".to_string()),
            is_dynamic: false,
            props: vec![],
            has_spread: false,
            slots_used: vec![],
            static_classes: vec![],
            has_dynamic_class: false,
            dynamic_classes: vec![],
            v_models: vec![TemplateComponentVModel {
                binding_name: "modelValue".to_string(),
                span: verter_span::Span::new(10, 30),
            }],
            bindings: vec![],
            events: vec![],
            span: verter_span::Span::new(0, 50),
        }];
        let child = make_child(&[], &["modelValue"], false);

        let unknowns = find_unknown_models(&comps, &|_| Some(child.clone()));
        assert!(unknowns.is_empty());
    }

    #[test]
    fn kebab_to_camel_converts_correctly() {
        assert_eq!(kebab_to_camel("foo"), "foo");
        assert_eq!(kebab_to_camel("some-prop"), "someProp");
        assert_eq!(kebab_to_camel("my-long-prop-name"), "myLongPropName");
    }
}
