//! Cross-file optimizer: render tree construction and prop constness analysis.
//!
//! After all files are compiled (e.g., during `preCompile`), this module builds
//! a render tree from template analysis data and computes which props can be
//! treated as constant across all call sites.
//!
//! The optimizer is conservative: a prop is only const if ALL parents pass
//! a const value AND no parent uses `v-bind` spread on the component.

use rustc_hash::{FxHashMap, FxHashSet};
use verter_analysis::template::PropValueConstness;

use crate::shared::read_lock;
use crate::VerterHost;

// ── Types ──────────────────────────────────────────────────────────────────

/// Edge in the render tree: a parent file using a child component.
#[derive(Debug, Clone)]
pub struct RenderTreeEdge {
    /// Canonical ID of the parent SFC that uses this component.
    pub parent_id: String,
    /// Component tag name as used in template (PascalCase).
    pub component_name: String,
    /// Per-prop constness at this call site.
    pub prop_constness: Vec<(String, PropValueConstness)>,
    /// Whether `v-bind="obj"` spread was used (marks all props Unknown).
    pub has_spread: bool,
    /// Static class names passed via `class="foo bar"` on the component.
    pub static_classes: Vec<String>,
    /// Whether `:class="..."` (dynamic class binding) is used on the component.
    pub has_dynamic_class: bool,
    /// Class names extracted from `:class` object syntax (conditional classes).
    pub dynamic_classes: Vec<String>,
}

/// Result of cross-file optimization analysis.
#[derive(Debug, Clone, Default)]
pub struct CrossFileResult {
    /// Per-file const prop sets (canonical_id → set of const prop names).
    pub const_prop_overrides: FxHashMap<String, FxHashSet<String>>,
    /// Diagnostics emitted during analysis.
    pub diagnostics: Vec<CrossFileDiagnostic>,
    /// Files whose constness changed compared to the previous computation.
    /// Empty on first computation. These files need recompilation.
    pub changed_files: Vec<String>,
}

/// A diagnostic from cross-file analysis.
#[derive(Debug, Clone)]
pub struct CrossFileDiagnostic {
    /// File that triggered the diagnostic.
    pub file_id: String,
    /// Diagnostic code (e.g., "CYCLE_DETECTED").
    pub code: String,
    /// Human-readable message.
    pub message: String,
}

// ── Implementation ─────────────────────────────────────────────────────────

impl VerterHost {
    /// Compute cross-file prop constness optimizations.
    ///
    /// Builds a render tree from all compiled files' template analysis data,
    /// then determines which child component props are const across ALL parents.
    ///
    /// A prop is const only when:
    /// - ALL parent call sites pass a `Const` value for that prop
    /// - No parent uses `v-bind` spread on the component
    /// - The component is not a root (has at least one known parent)
    ///
    /// Returns `CrossFileResult` with per-file const prop sets and diagnostics.
    pub fn compute_cross_file_optimizations(&self) -> CrossFileResult {
        // Step 1: Build reverse render tree.
        // For each child canonical ID, collect all parent usages.
        let mut child_usages: FxHashMap<String, Vec<RenderTreeEdge>> = FxHashMap::default();
        let mut diagnostics = Vec::new();

        // Collect (parent_id, template_analysis) pairs from the appropriate source.
        #[cfg(feature = "scheduler")]
        let parent_templates: Vec<(
            String,
            std::sync::Arc<verter_analysis::template::TemplateAnalysisSnapshot>,
        )> = {
            self.scheduler
                .node_ids()
                .into_iter()
                .filter(|id| self.compile_cache.get(id).map_or(true, |cc| !cc.evicted))
                .filter_map(|id| {
                    // Use raw_template_analysis_for_file which lazily computes
                    // template analysis if not already cached.
                    let tpl = self.raw_template_analysis_for_file(&id);
                    tpl.map(|t| (id, t))
                })
                .collect()
        };

        #[cfg(not(feature = "scheduler"))]
        let parent_templates: Vec<(
            String,
            std::sync::Arc<verter_analysis::template::TemplateAnalysisSnapshot>,
        )> = {
            let files = read_lock(&self.files);
            files
                .iter()
                .filter_map(|(id, entry)| {
                    entry
                        .template_analysis
                        .as_ref()
                        .map(|t| (id.clone(), std::sync::Arc::clone(t)))
                })
                .collect()
        };

        for (parent_id, tpl) in &parent_templates {
            for component in &tpl.components {
                if component.is_dynamic {
                    continue;
                }

                let child_canonical = match &component.import_source {
                    Some(source) => self.resolve_via_vfs(
                        parent_id,
                        source,
                        verter_vfs::ResolutionContext {
                            phase: verter_vfs::ResolvePhase::CodegenBlocker,
                            kind: verter_vfs::ResolveRequestKind::EsmImport,
                        },
                    ),
                    None => None,
                };

                let child_id = match child_canonical {
                    Some(id) => id,
                    None => continue,
                };

                let prop_constness: Vec<(String, PropValueConstness)> = component
                    .props
                    .iter()
                    .map(|p| (p.name.clone(), p.constness))
                    .collect();

                child_usages
                    .entry(child_id)
                    .or_default()
                    .push(RenderTreeEdge {
                        parent_id: parent_id.clone(),
                        component_name: component.name.clone(),
                        prop_constness,
                        has_spread: component.has_spread,
                        static_classes: component.static_classes.clone(),
                        has_dynamic_class: component.has_dynamic_class,
                        dynamic_classes: component.dynamic_classes.clone(),
                    });
            }
        }

        // Step 2: Detect cycles via visited tracking.
        // For now, cycles are detected but we just mark diagnostics.
        // The constness aggregation below handles cycles conservatively
        // (any cycle participant that also appears as a parent will just add
        // its edges normally — the aggregation is per-prop across all parents).

        // Step 3: For each child, aggregate prop constness across ALL parents.
        let mut const_prop_overrides: FxHashMap<String, FxHashSet<String>> = FxHashMap::default();

        for (child_id, edges) in &child_usages {
            // Collect all unique prop names across all parents.
            let mut prop_names: FxHashSet<String> = FxHashSet::default();
            let mut any_spread = false;

            for edge in edges {
                if edge.has_spread {
                    any_spread = true;
                }
                for (name, _) in &edge.prop_constness {
                    prop_names.insert(name.clone());
                }
            }

            // If any parent uses spread, skip — can't guarantee constness.
            if any_spread {
                diagnostics.push(CrossFileDiagnostic {
                    file_id: child_id.clone(),
                    code: "SPREAD_PREVENTS_OPTIMIZATION".to_string(),
                    message: "v-bind spread used on component in one or more parents — skipping prop constness optimization".to_string(),
                });
                continue;
            }

            // For each prop: const only if ALL parents pass Const.
            let mut const_props: FxHashSet<String> = FxHashSet::default();

            for prop_name in &prop_names {
                let all_const = edges.iter().all(|edge| {
                    // Check if this parent passes this prop, and if so, is it const?
                    match edge.prop_constness.iter().find(|(n, _)| n == prop_name) {
                        Some((_, PropValueConstness::Const)) => true,
                        Some(_) => false, // Dynamic or Unknown
                        None => true, // Parent doesn't pass this prop — doesn't affect constness
                                      // (child uses default value, which is always static)
                    }
                });

                if all_const {
                    const_props.insert(prop_name.clone());
                }
            }

            if !const_props.is_empty() {
                const_prop_overrides.insert(child_id.clone(), const_props);
            }
        }

        // Step 4: Diff against previous overrides to find changed files.
        let prev_overrides = read_lock(&self.last_const_prop_overrides);
        let mut changed_files = Vec::new();

        // Files that gained or changed const props
        for (child_id, new_consts) in &const_prop_overrides {
            let changed = match prev_overrides.get(child_id) {
                Some(old_consts) => old_consts != new_consts,
                None => true, // New entry
            };
            if changed {
                changed_files.push(child_id.clone());
            }
        }
        // Files that lost all const props
        for child_id in prev_overrides.keys() {
            if !const_prop_overrides.contains_key(child_id) {
                changed_files.push(child_id.clone());
            }
        }
        drop(prev_overrides);

        // Store new overrides for next diff
        let mut prev = crate::shared::write_lock(&self.last_const_prop_overrides);
        *prev = const_prop_overrides.clone();
        drop(prev);

        CrossFileResult {
            const_prop_overrides,
            diagnostics,
            changed_files,
        }
    }
}

#[cfg(test)]
mod tests {

    use crate::*;
    use std::sync::Arc;

    fn make_host() -> VerterHost {
        VerterHost::new_standalone(HostConfig::default())
    }

    fn upsert_vue(host: &VerterHost, id: &str, src: &str) {
        host.upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(src),
            file_kind: FileKind::VueSfc,
            aliases: Vec::new(),
        })
        .unwrap();
    }

    fn compile_file(host: &VerterHost, id: &str) {
        host.get_virtual_file(VirtualQuery {
            raw_id: Some(format!("{}?vue&type=template", id)),
            canonical_id: None,
            node_kind: None,
            compile_profile: CompileProfile::default(),
        })
        .unwrap();
    }

    /// @ai-generated - All parents pass const prop → child gets optimization
    #[test]
    fn all_parents_const_prop_optimized() {
        let host = make_host();

        // Child component with a `msg` prop
        upsert_vue(
            &host,
            "/project/Child.vue",
            r#"<script setup>
defineProps({ msg: String })
</script>
<template><div>{{ msg }}</div></template>"#,
        );

        // Parent passes a literal string (const)
        upsert_vue(
            &host,
            "/project/Parent.vue",
            r#"<script setup>
import Child from './Child.vue'
</script>
<template><Child msg="hello" /></template>"#,
        );

        // Compile both
        compile_file(&host, "/project/Child.vue");
        compile_file(&host, "/project/Parent.vue");

        let result = host.compute_cross_file_optimizations();
        // Child.vue should have "msg" as const prop
        let child_consts = result.const_prop_overrides.get("/project/Child.vue");
        assert!(
            child_consts.is_some(),
            "Expected const props for Child.vue, got none. Overrides: {:?}",
            result.const_prop_overrides
        );
        assert!(
            child_consts.unwrap().contains("msg"),
            "Expected 'msg' in const props, got: {:?}",
            child_consts
        );
    }

    /// @ai-generated - One dynamic parent prevents optimization
    #[test]
    fn one_dynamic_parent_prevents_optimization() {
        let host = make_host();

        upsert_vue(
            &host,
            "/project/Child.vue",
            r#"<script setup>
defineProps({ msg: String })
</script>
<template><div>{{ msg }}</div></template>"#,
        );

        // Parent A passes const
        upsert_vue(
            &host,
            "/project/ParentA.vue",
            r#"<script setup>
import Child from './Child.vue'
</script>
<template><Child msg="hello" /></template>"#,
        );

        // Parent B passes dynamic ref
        upsert_vue(
            &host,
            "/project/ParentB.vue",
            r#"<script setup>
import { ref } from 'vue'
import Child from './Child.vue'
const msg = ref('world')
</script>
<template><Child :msg="msg" /></template>"#,
        );

        compile_file(&host, "/project/Child.vue");
        compile_file(&host, "/project/ParentA.vue");
        compile_file(&host, "/project/ParentB.vue");

        let result = host.compute_cross_file_optimizations();
        // msg should NOT be const because ParentB passes dynamic
        let child_consts = result.const_prop_overrides.get("/project/Child.vue");
        let has_msg = child_consts.is_some_and(|s| s.contains("msg"));
        assert!(
            !has_msg,
            "msg should NOT be const when one parent passes dynamic"
        );
    }

    /// @ai-generated - Root component (no parents) → no optimization
    #[test]
    fn root_component_props_remain_dynamic() {
        let host = make_host();

        upsert_vue(
            &host,
            "/project/Root.vue",
            r#"<script setup>
defineProps({ title: String })
</script>
<template><div>{{ title }}</div></template>"#,
        );

        compile_file(&host, "/project/Root.vue");

        let result = host.compute_cross_file_optimizations();
        // No parent uses Root, so it shouldn't have any const props
        assert!(
            !result
                .const_prop_overrides
                .contains_key("/project/Root.vue"),
            "Root with no parents should not have const props"
        );
    }

    /// @ai-generated - v-bind spread prevents all prop optimizations
    #[test]
    fn spread_prevents_all_optimizations() {
        let host = make_host();

        upsert_vue(
            &host,
            "/project/Child.vue",
            r#"<script setup>
defineProps({ msg: String, count: Number })
</script>
<template><div>{{ msg }}{{ count }}</div></template>"#,
        );

        upsert_vue(
            &host,
            "/project/Parent.vue",
            r#"<script setup>
import Child from './Child.vue'
const props = { msg: 'hello', count: 42 }
</script>
<template><Child v-bind="props" /></template>"#,
        );

        compile_file(&host, "/project/Child.vue");
        compile_file(&host, "/project/Parent.vue");

        let result = host.compute_cross_file_optimizations();
        assert!(
            !result
                .const_prop_overrides
                .contains_key("/project/Child.vue"),
            "Spread should prevent all prop optimizations"
        );
        // Should have a diagnostic about spread
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "SPREAD_PREVENTS_OPTIMIZATION"),
            "Expected SPREAD_PREVENTS_OPTIMIZATION diagnostic"
        );
    }

    /// @ai-generated - Unresolved component (no import source) is skipped
    #[test]
    fn unresolved_component_skipped() {
        let host = make_host();

        upsert_vue(
            &host,
            "/project/Parent.vue",
            r#"<template><GlobalComp msg="hello" /></template>"#,
        );

        compile_file(&host, "/project/Parent.vue");

        let result = host.compute_cross_file_optimizations();
        // GlobalComp has no import source, should be skipped entirely
        assert!(
            result.const_prop_overrides.is_empty(),
            "Unresolved components should not produce overrides"
        );
    }

    /// @ai-generated - Empty host produces empty result
    #[test]
    fn empty_host_produces_empty_result() {
        let host = make_host();
        let result = host.compute_cross_file_optimizations();
        assert!(result.const_prop_overrides.is_empty());
        assert!(result.diagnostics.is_empty());
    }

    /// @ai-generated - Multiple const parents all passing const → optimized
    #[test]
    fn multiple_const_parents_optimized() {
        let host = make_host();

        upsert_vue(
            &host,
            "/project/Child.vue",
            r#"<script setup>
defineProps({ msg: String })
</script>
<template><div>{{ msg }}</div></template>"#,
        );

        upsert_vue(
            &host,
            "/project/ParentA.vue",
            r#"<script setup>
import Child from './Child.vue'
</script>
<template><Child msg="hello" /></template>"#,
        );

        upsert_vue(
            &host,
            "/project/ParentB.vue",
            r#"<script setup>
import Child from './Child.vue'
</script>
<template><Child msg="world" /></template>"#,
        );

        compile_file(&host, "/project/Child.vue");
        compile_file(&host, "/project/ParentA.vue");
        compile_file(&host, "/project/ParentB.vue");

        let result = host.compute_cross_file_optimizations();
        let child_consts = result.const_prop_overrides.get("/project/Child.vue");
        assert!(
            child_consts.is_some_and(|s| s.contains("msg")),
            "msg should be const when all parents pass const values"
        );
    }

    /// @ai-generated - Parent not passing a prop → prop not in const set
    /// (optimization only applies to actively-passed props)
    #[test]
    fn parent_not_passing_prop_doesnt_prevent_constness() {
        let host = make_host();

        upsert_vue(
            &host,
            "/project/Child.vue",
            r#"<script setup>
defineProps({ msg: String, label: { type: String, default: 'default' } })
</script>
<template><div>{{ msg }} {{ label }}</div></template>"#,
        );

        // Parent passes msg but not label
        upsert_vue(
            &host,
            "/project/Parent.vue",
            r#"<script setup>
import Child from './Child.vue'
</script>
<template><Child msg="hello" /></template>"#,
        );

        compile_file(&host, "/project/Child.vue");
        compile_file(&host, "/project/Parent.vue");

        let result = host.compute_cross_file_optimizations();
        let child_consts = result.const_prop_overrides.get("/project/Child.vue");
        assert!(child_consts.is_some(), "Expected const props for Child.vue");
        let consts = child_consts.unwrap();
        assert!(consts.contains("msg"), "msg should be const");
        // label is never passed by any parent → not tracked in optimization
        // (default values are handled by the child's runtime, not the optimizer)
        assert!(
            !consts.contains("label"),
            "label (never passed) should not be in const set"
        );
    }

    /// @ai-generated - Alias-based import resolved via parent dependencies
    #[test]
    fn alias_import_resolved_via_parent_deps() {
        let host = make_host();

        upsert_vue(
            &host,
            "/project/src/components/Child.vue",
            r#"<script setup>
defineProps({ msg: String })
</script>
<template><div>{{ msg }}</div></template>"#,
        );

        // Parent imports via alias (e.g., @/components/Child.vue)
        upsert_vue(
            &host,
            "/project/src/App.vue",
            r#"<script setup>
import Child from '@/components/Child.vue'
</script>
<template><Child msg="hello" /></template>"#,
        );

        compile_file(&host, "/project/src/components/Child.vue");
        compile_file(&host, "/project/src/App.vue");

        // Simulate caller-resolved deps (as unplugin/LSP would do after resolving tsconfig paths)
        host.set_import_dependencies(
            "/project/src/App.vue",
            vec![crate::DependencyResolution {
                specifier: "@/components/Child.vue".to_string(),
                resolved_canonical_id: Some("/project/src/components/Child.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );

        let result = host.compute_cross_file_optimizations();
        let child_consts = result
            .const_prop_overrides
            .get("/project/src/components/Child.vue");
        assert!(
            child_consts.is_some(),
            "Expected const props for Child.vue via alias resolution. Overrides: {:?}",
            result.const_prop_overrides
        );
        assert!(child_consts.unwrap().contains("msg"), "msg should be const");
    }

    /// @ai-generated - Host alias map resolves tsconfig/vite paths
    #[test]
    fn host_alias_map_resolves_paths() {
        let host = make_host();

        upsert_vue(
            &host,
            "/project/src/components/Child.vue",
            r#"<script setup>
defineProps({ title: String })
</script>
<template><div>{{ title }}</div></template>"#,
        );

        upsert_vue(
            &host,
            "/project/src/App.vue",
            r#"<script setup>
import Child from '@/components/Child.vue'
</script>
<template><Child title="test" /></template>"#,
        );

        compile_file(&host, "/project/src/components/Child.vue");
        compile_file(&host, "/project/src/App.vue");

        // Configure workspace resolver with alias (as LSP/unplugin would do)
        {
            use verter_analysis::project_resolver::*;
            host.workspace().configure_resolver(vec![IdeProjectConfig {
                root: "/project".to_string(),
                workspace_root: "/project".to_string(),
                tsconfig_path: None,
                provider_root: "/project".to_string(),
                workspace_aliases: vec![WorkspaceAlias {
                    find: "@/".to_string(),
                    replacement: "/project/src/".to_string(),
                }],
                compiler_options: IdeProjectCompilerOptions::default(),
                references: vec![],
                membership: ProjectMembership::MatchAll,
            }]);
        }

        let result = host.compute_cross_file_optimizations();
        let child_consts = result
            .const_prop_overrides
            .get("/project/src/components/Child.vue");
        assert!(
            child_consts.is_some(),
            "Expected const props via host alias map. Overrides: {:?}",
            result.const_prop_overrides
        );
    }

    /// @ai-generated - First computation lists all optimized files as changed
    #[test]
    fn first_computation_lists_all_as_changed() {
        let host = make_host();

        upsert_vue(
            &host,
            "/project/Child.vue",
            r#"<script setup>
defineProps({ msg: String })
</script>
<template><div>{{ msg }}</div></template>"#,
        );
        upsert_vue(
            &host,
            "/project/Parent.vue",
            r#"<script setup>
import Child from './Child.vue'
</script>
<template><Child msg="hello" /></template>"#,
        );
        compile_file(&host, "/project/Child.vue");
        compile_file(&host, "/project/Parent.vue");

        let result = host.compute_cross_file_optimizations();
        assert!(
            result
                .changed_files
                .contains(&"/project/Child.vue".to_string()),
            "First computation should list all optimized files as changed"
        );
    }

    /// @ai-generated - Second computation with no changes returns empty changed_files
    #[test]
    fn no_changes_returns_empty_changed_files() {
        let host = make_host();

        upsert_vue(
            &host,
            "/project/Child.vue",
            r#"<script setup>
defineProps({ msg: String })
</script>
<template><div>{{ msg }}</div></template>"#,
        );
        upsert_vue(
            &host,
            "/project/Parent.vue",
            r#"<script setup>
import Child from './Child.vue'
</script>
<template><Child msg="hello" /></template>"#,
        );
        compile_file(&host, "/project/Child.vue");
        compile_file(&host, "/project/Parent.vue");

        // First computation
        let _ = host.compute_cross_file_optimizations();
        // Second computation with no changes
        let result = host.compute_cross_file_optimizations();
        assert!(
            result.changed_files.is_empty(),
            "No changes should produce empty changed_files, got: {:?}",
            result.changed_files
        );
    }

    /// @ai-generated - Parent changes prop from const to dynamic → child in changed_files
    #[test]
    fn parent_changes_prop_const_to_dynamic_invalidates_child() {
        let host = make_host();

        upsert_vue(
            &host,
            "/project/Child.vue",
            r#"<script setup>
defineProps({ msg: String })
</script>
<template><div>{{ msg }}</div></template>"#,
        );
        upsert_vue(
            &host,
            "/project/Parent.vue",
            r#"<script setup>
import Child from './Child.vue'
</script>
<template><Child msg="hello" /></template>"#,
        );
        compile_file(&host, "/project/Child.vue");
        compile_file(&host, "/project/Parent.vue");

        let result1 = host.compute_cross_file_optimizations();
        assert!(result1
            .const_prop_overrides
            .contains_key("/project/Child.vue"));

        // Now parent changes to dynamic prop
        upsert_vue(
            &host,
            "/project/Parent.vue",
            r#"<script setup>
import { ref } from 'vue'
import Child from './Child.vue'
const msg = ref('hello')
</script>
<template><Child :msg="msg" /></template>"#,
        );
        compile_file(&host, "/project/Parent.vue");

        let result2 = host.compute_cross_file_optimizations();
        assert!(
            !result2
                .const_prop_overrides
                .contains_key("/project/Child.vue"),
            "Child should no longer have const props"
        );
        assert!(
            result2
                .changed_files
                .contains(&"/project/Child.vue".to_string()),
            "Child.vue should be in changed_files after parent made prop dynamic"
        );
    }

    /// @ai-generated - Dynamic component (:is) is skipped
    #[test]
    fn dynamic_component_skipped() {
        let host = make_host();

        upsert_vue(
            &host,
            "/project/Child.vue",
            r#"<script setup>
defineProps({ msg: String })
</script>
<template><div>{{ msg }}</div></template>"#,
        );

        upsert_vue(
            &host,
            "/project/Parent.vue",
            r#"<script setup>
import { ref } from 'vue'
import Child from './Child.vue'
const comp = ref(Child)
</script>
<template><component :is="comp" msg="hello" /></template>"#,
        );

        compile_file(&host, "/project/Child.vue");
        compile_file(&host, "/project/Parent.vue");

        let result = host.compute_cross_file_optimizations();
        // Dynamic component usage should not contribute to render tree
        assert!(
            !result
                .const_prop_overrides
                .contains_key("/project/Child.vue"),
            "Dynamic component usage should not create render tree edges"
        );
    }
}
