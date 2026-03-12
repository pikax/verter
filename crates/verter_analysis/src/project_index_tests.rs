use super::*;
use crate::file_usage::{
    ComponentUsageOwned, EmitDeclarationOwned, FileUsageFlags, InjectUsageOwned,
    ListenedEventOwned, ProvideUsageOwned, StyleUsageInfoOwned, TemplateIdOwned,
};

fn make_file_info() -> FileUsageInfoOwned {
    FileUsageInfoOwned::default()
}

fn make_file_with_provide(key: &str) -> FileUsageInfoOwned {
    let mut info = make_file_info();
    info.provides.push(ProvideUsageOwned {
        key: Some(key.to_string()),
        is_dynamic_key: false,
        start: 0,
        end: 10,
    });
    info.flags |= FileUsageFlags::HAS_PROVIDE.bits();
    info
}

fn make_file_with_inject(key: &str) -> FileUsageInfoOwned {
    let mut info = make_file_info();
    info.injects.push(InjectUsageOwned {
        key: Some(key.to_string()),
        is_dynamic_key: false,
        has_default: false,
        binding_name: None,
        start: 0,
        end: 10,
    });
    info.flags |= FileUsageFlags::HAS_INJECT.bits();
    info
}

fn make_file_with_dynamic_inject() -> FileUsageInfoOwned {
    let mut info = make_file_info();
    info.injects.push(InjectUsageOwned {
        key: None,
        is_dynamic_key: true,
        has_default: false,
        binding_name: None,
        start: 0,
        end: 10,
    });
    info.flags |= FileUsageFlags::HAS_INJECT.bits();
    info
}

fn make_file_with_component(name: &str) -> FileUsageInfoOwned {
    let mut info = make_file_info();
    info.components.push(ComponentUsageOwned {
        name: Some(name.to_string()),
        is_dynamic: false,
        start: 0,
        end: 10,
    });
    info.flags |= FileUsageFlags::HAS_COMPONENT_USAGE.bits();
    info
}

#[test]
fn new_index_is_empty() {
    let index = ProjectIndex::new();
    assert_eq!(index.file_count(), 0);
    assert_eq!(index.all_provide_keys().count(), 0);
    assert_eq!(index.all_inject_keys().count(), 0);
}

#[test]
fn add_and_get_file() {
    let mut index = ProjectIndex::new();
    let path = PathBuf::from("src/App.vue");
    let info = make_file_info();

    index.add_file(path.clone(), info);

    assert!(index.contains_file(&path));
    assert_eq!(index.file_count(), 1);
    assert!(index.get_file(&path).is_some());
}

#[test]
fn remove_file() {
    let mut index = ProjectIndex::new();
    let path = PathBuf::from("src/App.vue");
    let info = make_file_with_provide("theme");

    index.add_file(path.clone(), info);
    assert_eq!(index.files_providing("theme").count(), 1);

    let removed = index.remove_file(&path);
    assert!(removed.is_some());
    assert!(!index.contains_file(&path));
    assert_eq!(index.files_providing("theme").count(), 0);
}

#[test]
fn provide_index() {
    let mut index = ProjectIndex::new();

    let app_path = PathBuf::from("src/App.vue");
    let child_path = PathBuf::from("src/Child.vue");

    index.add_file(app_path.clone(), make_file_with_provide("theme"));
    index.add_file(child_path.clone(), make_file_with_provide("theme"));

    let providers: Vec<_> = index.files_providing("theme").collect();
    assert_eq!(providers.len(), 2);
    assert!(providers.iter().any(|p| p.as_ref() == app_path.as_path()));
    assert!(providers.iter().any(|p| p.as_ref() == child_path.as_path()));
}

#[test]
fn inject_index() {
    let mut index = ProjectIndex::new();

    let child1_path = PathBuf::from("src/Child1.vue");
    let child2_path = PathBuf::from("src/Child2.vue");

    index.add_file(child1_path.clone(), make_file_with_inject("theme"));
    index.add_file(child2_path.clone(), make_file_with_inject("theme"));

    assert_eq!(index.files_injecting("theme").count(), 2);
}

#[test]
fn validate_inject_valid() {
    let mut index = ProjectIndex::new();

    let provider_path = PathBuf::from("src/Provider.vue");
    let consumer_path = PathBuf::from("src/Consumer.vue");

    index.add_file(provider_path.clone(), make_file_with_provide("config"));
    index.add_file(consumer_path.clone(), make_file_with_inject("config"));

    let validation = index.validate_inject(&consumer_path, "config");
    match validation {
        InjectValidation::Valid { providers } => {
            assert_eq!(providers.len(), 1);
            assert!(providers
                .iter()
                .any(|p| p.as_ref() == provider_path.as_path()));
        }
        _ => panic!("Expected Valid validation"),
    }
}

#[test]
fn validate_inject_no_provider() {
    let mut index = ProjectIndex::new();

    let consumer_path = PathBuf::from("src/Consumer.vue");
    index.add_file(consumer_path.clone(), make_file_with_inject("missing"));

    let validation = index.validate_inject(&consumer_path, "missing");
    assert_eq!(validation, InjectValidation::NoProvider);
}

#[test]
fn validate_inject_dynamic_key() {
    let mut index = ProjectIndex::new();

    let consumer_path = PathBuf::from("src/Consumer.vue");
    let info = make_file_with_dynamic_inject();
    index.add_file(consumer_path.clone(), info);

    let file_validation = index.validate_file_injects(&consumer_path);
    assert_eq!(file_validation.dynamic_keys.len(), 1);
}

#[test]
fn validate_inject_key_not_found() {
    let mut index = ProjectIndex::new();

    let consumer_path = PathBuf::from("src/Consumer.vue");
    index.add_file(consumer_path.clone(), make_file_with_inject("exists"));

    let validation = index.validate_inject(&consumer_path, "nonexistent");
    assert_eq!(validation, InjectValidation::KeyNotFound);
}

#[test]
fn validate_file_injects_mixed() {
    let mut index = ProjectIndex::new();

    let provider_path = PathBuf::from("src/Provider.vue");
    let consumer_path = PathBuf::from("src/Consumer.vue");

    index.add_file(provider_path, make_file_with_provide("provided"));

    let mut consumer_info = make_file_info();
    consumer_info.injects.push(InjectUsageOwned {
        key: Some("provided".to_string()),
        is_dynamic_key: false,
        has_default: false,
        binding_name: None,
        start: 0,
        end: 10,
    });
    consumer_info.injects.push(InjectUsageOwned {
        key: Some("missing".to_string()),
        is_dynamic_key: false,
        has_default: false,
        binding_name: None,
        start: 20,
        end: 30,
    });
    consumer_info.injects.push(InjectUsageOwned {
        key: None,
        is_dynamic_key: true,
        has_default: false,
        binding_name: None,
        start: 40,
        end: 50,
    });
    consumer_info.flags |= FileUsageFlags::HAS_INJECT.bits();
    index.add_file(consumer_path.clone(), consumer_info);

    let validation = index.validate_file_injects(&consumer_path);
    assert_eq!(validation.valid.len(), 1);
    assert_eq!(validation.valid[0].key, "provided");
    assert_eq!(validation.missing_providers.len(), 1);
    assert_eq!(validation.missing_providers[0].key, "missing");
    assert_eq!(validation.dynamic_keys.len(), 1);
}

#[test]
fn component_graph() {
    let mut index = ProjectIndex::new();

    let app_path = PathBuf::from("src/App.vue");
    let mut app_info = make_file_info();
    app_info.components.push(ComponentUsageOwned {
        name: Some("Header".to_string()),
        is_dynamic: false,
        start: 0,
        end: 10,
    });
    app_info.components.push(ComponentUsageOwned {
        name: Some("Footer".to_string()),
        is_dynamic: false,
        start: 20,
        end: 30,
    });
    index.add_file(app_path.clone(), app_info);

    let components = index.components_used_by(&app_path);
    assert_eq!(components.len(), 2);

    let files_using_header: Vec<_> = index.files_using_component("Header").collect();
    assert_eq!(files_using_header.len(), 1);
    assert!(files_using_header
        .iter()
        .any(|p| p.as_ref() == app_path.as_path()));
}

#[test]
fn provide_inject_summary() {
    let mut index = ProjectIndex::new();

    index.add_file(
        PathBuf::from("src/Provider.vue"),
        make_file_with_provide("theme"),
    );
    index.add_file(
        PathBuf::from("src/Provider2.vue"),
        make_file_with_provide("config"),
    );
    index.add_file(
        PathBuf::from("src/Consumer.vue"),
        make_file_with_inject("theme"),
    );
    index.add_file(
        PathBuf::from("src/Consumer2.vue"),
        make_file_with_inject("missing"),
    );

    let summary = index.provide_inject_summary();
    assert_eq!(summary.provide_keys.len(), 2);
    assert_eq!(summary.inject_keys.len(), 2);
    assert_eq!(summary.unused_provides.len(), 1);
    assert_eq!(summary.missing_provides.len(), 1);
}

#[test]
fn project_stats() {
    let mut index = ProjectIndex::new();

    let mut info1 = make_file_with_provide("theme");
    info1.flags |= FileUsageFlags::HAS_DEFINE_PROPS.bits();
    index.add_file(PathBuf::from("src/App.vue"), info1);

    let mut info2 = make_file_with_inject("theme");
    info2.flags |= (FileUsageFlags::HAS_DEFINE_EMITS | FileUsageFlags::IS_ASYNC_SETUP).bits();
    index.add_file(PathBuf::from("src/Child.vue"), info2);

    let stats = index.stats();
    assert_eq!(stats.file_count, 2);
    assert_eq!(stats.files_with_provide, 1);
    assert_eq!(stats.files_with_inject, 1);
    assert_eq!(stats.files_with_props, 1);
    assert_eq!(stats.files_with_emits, 1);
    assert_eq!(stats.files_with_async_setup, 1);
    assert_eq!(stats.unique_provide_keys, 1);
    assert_eq!(stats.unique_inject_keys, 1);
}

#[test]
fn update_file_reindexes() {
    let mut index = ProjectIndex::new();
    let path = PathBuf::from("src/App.vue");

    index.add_file(path.clone(), make_file_with_provide("old"));
    assert_eq!(index.files_providing("old").count(), 1);
    assert_eq!(index.files_providing("new").count(), 0);

    index.add_file(path.clone(), make_file_with_provide("new"));
    assert_eq!(index.files_providing("old").count(), 0);
    assert_eq!(index.files_providing("new").count(), 1);
}

#[test]
fn clear_index() {
    let mut index = ProjectIndex::new();
    index.add_file(
        PathBuf::from("src/App.vue"),
        make_file_with_provide("theme"),
    );

    assert_eq!(index.file_count(), 1);
    index.clear();
    assert_eq!(index.file_count(), 0);
    assert_eq!(index.all_provide_keys().count(), 0);
}

#[test]
fn component_usage_summary() {
    let mut index = ProjectIndex::new();

    index.add_file(
        PathBuf::from("src/App.vue"),
        make_file_with_component("Header"),
    );
    index.add_file(
        PathBuf::from("src/Page.vue"),
        make_file_with_component("Header"),
    );
    index.add_file(
        PathBuf::from("src/Other.vue"),
        make_file_with_component("Footer"),
    );

    let summary = index.component_usage_summary();
    assert_eq!(summary.component_names.len(), 2);
    assert_eq!(summary.usage_counts.get("Header"), Some(&2));
    assert_eq!(summary.usage_counts.get("Footer"), Some(&1));
}

#[test]
fn file_paths_iterator() {
    let mut index = ProjectIndex::new();
    let path1 = PathBuf::from("src/App.vue");
    let path2 = PathBuf::from("src/Child.vue");

    index.add_file(path1.clone(), make_file_info());
    index.add_file(path2.clone(), make_file_info());

    let paths: Vec<_> = index.file_paths().collect();
    assert_eq!(paths.len(), 2);
    assert!(paths.iter().any(|p| p.as_ref() == path1.as_path()));
    assert!(paths.iter().any(|p| p.as_ref() == path2.as_path()));
}

// ==================== Style Index Tests ====================

fn make_file_with_styles(
    class_names: &[&str],
    v_binds: &[&str],
    custom_props: &[&str],
    scoped: bool,
) -> FileUsageInfoOwned {
    let mut info = make_file_info();
    info.styles.push(StyleUsageInfoOwned {
        lang: Some("css".to_string()),
        scoped,
        class_names: class_names.iter().map(|s| s.to_string()).collect(),
        v_bind_expressions: v_binds.iter().map(|s| s.to_string()).collect(),
        custom_property_names: custom_props.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    });
    let mut flags = FileUsageFlags::empty();
    if scoped {
        flags |= FileUsageFlags::HAS_SCOPED_STYLE;
    }
    if !v_binds.is_empty() {
        flags |= FileUsageFlags::HAS_V_BIND_CSS;
    }
    info.flags |= flags.bits();
    info
}

#[test]
fn test_class_index_add_remove() {
    let mut index = ProjectIndex::new();
    let path = PathBuf::from("src/App.vue");

    index.add_file(
        path.clone(),
        make_file_with_styles(&["btn", "active"], &[], &[], false),
    );

    assert_eq!(index.files_defining_class("btn").count(), 1);
    assert_eq!(index.files_defining_class("active").count(), 1);
    assert_eq!(index.files_defining_class("missing").count(), 0);

    // Remove file - indexes should be cleaned up
    index.remove_file(&path);
    assert_eq!(index.files_defining_class("btn").count(), 0);
    assert_eq!(index.files_defining_class("active").count(), 0);
}

#[test]
fn test_v_bind_css_index() {
    let mut index = ProjectIndex::new();

    index.add_file(
        PathBuf::from("src/A.vue"),
        make_file_with_styles(&[], &["color"], &[], false),
    );
    index.add_file(
        PathBuf::from("src/B.vue"),
        make_file_with_styles(&[], &["color", "size"], &[], false),
    );

    assert_eq!(index.files_using_v_bind_css("color").count(), 2);
    assert_eq!(index.files_using_v_bind_css("size").count(), 1);
    assert_eq!(index.files_using_v_bind_css("missing").count(), 0);
}

#[test]
fn test_custom_property_index() {
    let mut index = ProjectIndex::new();

    index.add_file(
        PathBuf::from("src/A.vue"),
        make_file_with_styles(&[], &[], &["--primary", "--spacing"], false),
    );

    assert_eq!(index.files_defining_custom_property("--primary").count(), 1);
    assert_eq!(index.files_defining_custom_property("--spacing").count(), 1);
    assert_eq!(index.files_defining_custom_property("--missing").count(), 0);

    assert_eq!(index.all_custom_properties().count(), 2);
}

#[test]
fn test_stats_with_styles() {
    let mut index = ProjectIndex::new();

    index.add_file(
        PathBuf::from("src/A.vue"),
        make_file_with_styles(&["btn"], &["color"], &["--primary"], true),
    );

    let mut module_info = make_file_info();
    module_info.styles.push(StyleUsageInfoOwned {
        is_module: true,
        class_names: vec!["card".to_string()],
        ..Default::default()
    });
    module_info.flags |= FileUsageFlags::HAS_CSS_MODULES.bits();
    index.add_file(PathBuf::from("src/B.vue"), module_info);

    let stats = index.stats();
    assert_eq!(stats.files_with_scoped_styles, 1);
    assert_eq!(stats.files_with_css_modules, 1);
    assert_eq!(stats.files_with_v_bind_css, 1);
    assert_eq!(stats.unique_css_classes, 2); // "btn" and "card"
    assert_eq!(stats.unique_custom_properties, 1);
}

#[test]
fn test_class_index_multiple_files() {
    let mut index = ProjectIndex::new();

    index.add_file(
        PathBuf::from("src/A.vue"),
        make_file_with_styles(&["btn"], &[], &[], false),
    );
    index.add_file(
        PathBuf::from("src/B.vue"),
        make_file_with_styles(&["btn"], &[], &[], false),
    );

    assert_eq!(index.files_defining_class("btn").count(), 2);
    assert_eq!(index.all_class_names().count(), 1);
}

#[test]
fn test_style_reindex_on_update() {
    let mut index = ProjectIndex::new();
    let path = PathBuf::from("src/App.vue");

    index.add_file(
        path.clone(),
        make_file_with_styles(&["old-class"], &[], &[], false),
    );
    assert_eq!(index.files_defining_class("old-class").count(), 1);

    index.add_file(
        path.clone(),
        make_file_with_styles(&["new-class"], &[], &[], false),
    );
    assert_eq!(index.files_defining_class("old-class").count(), 0);
    assert_eq!(index.files_defining_class("new-class").count(), 1);
}

#[test]
fn stress_test_200_files() {
    let mut index = ProjectIndex::with_capacity(200);

    // Add 200 files with varied provide/inject/component usage
    for i in 0..200 {
        let path = PathBuf::from(format!("src/components/Comp{i}.vue"));
        let mut info = make_file_info();

        // Every 3rd file provides a key
        if i % 3 == 0 {
            info.provides.push(ProvideUsageOwned {
                key: Some(format!("key-{}", i % 15)),
                is_dynamic_key: false,
                start: 0,
                end: 10,
            });
            info.flags |= FileUsageFlags::HAS_PROVIDE.bits();
        }

        // Every 4th file injects a key
        if i % 4 == 0 {
            info.injects.push(InjectUsageOwned {
                key: Some(format!("key-{}", (i + 3) % 15)),
                is_dynamic_key: false,
                has_default: false,
                binding_name: None,
                start: 0,
                end: 10,
            });
            info.flags |= FileUsageFlags::HAS_INJECT.bits();
        }

        // Every 2nd file uses a component
        if i % 2 == 0 {
            info.components.push(ComponentUsageOwned {
                name: Some(format!("Widget{}", i % 10)),
                is_dynamic: false,
                start: 0,
                end: 10,
            });
            info.flags |= FileUsageFlags::HAS_COMPONENT_USAGE.bits();
        }

        // Some files have styles
        if i % 5 == 0 {
            info.styles.push(StyleUsageInfoOwned {
                class_names: vec![format!("cls-{}", i % 20)],
                custom_property_names: vec![format!("--var-{}", i % 10)],
                scoped: i % 10 == 0,
                ..Default::default()
            });
            if i % 10 == 0 {
                info.flags |= FileUsageFlags::HAS_SCOPED_STYLE.bits();
            }
        }

        index.add_file(path, info);
    }

    assert_eq!(index.file_count(), 200);

    let stats_before = index.stats();
    assert_eq!(stats_before.file_count, 200);
    assert!(stats_before.files_with_provide > 0);
    assert!(stats_before.files_with_inject > 0);
    assert!(stats_before.unique_provide_keys > 0);

    let summary = index.provide_inject_summary();
    assert!(!summary.provide_keys.is_empty());
    assert!(!summary.inject_keys.is_empty());

    // Update 20 files (change their provide keys)
    for i in (0..200).step_by(10) {
        let path = PathBuf::from(format!("src/components/Comp{i}.vue"));
        let mut info = make_file_info();
        info.provides.push(ProvideUsageOwned {
            key: Some(format!("updated-key-{}", i % 5)),
            is_dynamic_key: false,
            start: 0,
            end: 10,
        });
        info.flags |= FileUsageFlags::HAS_PROVIDE.bits();
        index.add_file(path, info);
    }

    assert_eq!(
        index.file_count(),
        200,
        "file count should stay the same after updates"
    );

    // Remove 50 files
    for i in 0..50 {
        let path = PathBuf::from(format!("src/components/Comp{i}.vue"));
        index.remove_file(&path);
    }

    assert_eq!(index.file_count(), 150);

    let stats_after = index.stats();
    assert_eq!(stats_after.file_count, 150);

    // Summary should still be consistent
    let summary_after = index.provide_inject_summary();
    // Provide keys in index should match what files actually provide
    for key in &summary_after.provide_keys {
        assert!(
            index.has_providers(key),
            "provide key '{key}' should have at least one provider"
        );
    }
    for key in &summary_after.inject_keys {
        assert!(
            index.has_injectors(key),
            "inject key '{key}' should have at least one injector"
        );
    }
}

#[test]
fn validate_inject_unindexed_file() {
    let index = ProjectIndex::new();
    let path = PathBuf::from("src/Unknown.vue");
    let result = index.validate_inject(&path, "anything");
    assert_eq!(
        result,
        InjectValidation::KeyNotFound,
        "validate_inject on unindexed file should return KeyNotFound"
    );
}

#[test]
fn validate_file_injects_unindexed_file() {
    let index = ProjectIndex::new();
    let path = PathBuf::from("src/Unknown.vue");
    let result = index.validate_file_injects(&path);
    assert!(result.valid.is_empty());
    assert!(result.missing_providers.is_empty());
    assert!(result.dynamic_keys.is_empty());
}

// --- CSS Variable Flow Tests ---

#[test]
fn var_reference_index_tracks_usages() {
    let mut index = ProjectIndex::new();
    let info = FileUsageInfoOwned {
        styles: vec![StyleUsageInfoOwned {
            var_reference_names: vec!["--primary".to_string(), "--spacing".to_string()],
            ..Default::default()
        }],
        ..Default::default()
    };
    index.add_file(PathBuf::from("src/App.vue"), info);

    let refs: Vec<_> = index
        .files_referencing_custom_property("--primary")
        .collect();
    assert_eq!(refs.len(), 1);
    assert!(refs[0].ends_with("App.vue"));

    // Non-existent reference
    assert_eq!(
        index
            .files_referencing_custom_property("--nonexistent")
            .count(),
        0
    );
}

#[test]
fn template_css_var_index_tracks_style_bindings() {
    let mut index = ProjectIndex::new();
    let info = FileUsageInfoOwned {
        template_css_var_names: vec!["--color".to_string()],
        ..Default::default()
    };
    index.add_file(PathBuf::from("src/Comp.vue"), info);

    let files: Vec<_> = index.files_setting_css_var_in_template("--color").collect();
    assert_eq!(files.len(), 1);
}

#[test]
fn script_css_var_index_tracks_dom_manipulations() {
    let mut index = ProjectIndex::new();
    let info = FileUsageInfoOwned {
        script_css_var_names: vec!["--theme-bg".to_string()],
        ..Default::default()
    };
    index.add_file(PathBuf::from("src/Theme.vue"), info);

    let files: Vec<_> = index
        .files_manipulating_css_var_in_script("--theme-bg")
        .collect();
    assert_eq!(files.len(), 1);
}

#[test]
fn css_var_flow_cross_component() {
    let mut index = ProjectIndex::new();

    // Component A defines --theme-color in style
    index.add_file(
        PathBuf::from("src/A.vue"),
        FileUsageInfoOwned {
            styles: vec![StyleUsageInfoOwned {
                custom_property_names: vec!["--theme-color".to_string()],
                ..Default::default()
            }],
            ..Default::default()
        },
    );

    // Component B uses var(--theme-color) in style and sets it in template
    index.add_file(
        PathBuf::from("src/B.vue"),
        FileUsageInfoOwned {
            styles: vec![StyleUsageInfoOwned {
                var_reference_names: vec!["--theme-color".to_string()],
                ..Default::default()
            }],
            template_css_var_names: vec!["--theme-color".to_string()],
            ..Default::default()
        },
    );

    // Component C manipulates it via script
    index.add_file(
        PathBuf::from("src/C.vue"),
        FileUsageInfoOwned {
            script_css_var_names: vec!["--theme-color".to_string()],
            ..Default::default()
        },
    );

    let flow = index.css_var_flow("--theme-color");
    assert_eq!(flow.name, "--theme-color");
    assert_eq!(flow.style_definitions.len(), 1);
    assert!(flow.style_definitions[0].ends_with("A.vue"));
    assert_eq!(flow.style_var_usages.len(), 1);
    assert!(flow.style_var_usages[0].ends_with("B.vue"));
    assert_eq!(flow.template_definitions.len(), 1);
    assert!(flow.template_definitions[0].ends_with("B.vue"));
    assert_eq!(flow.script_manipulations.len(), 1);
    assert!(flow.script_manipulations[0].ends_with("C.vue"));
}

#[test]
fn css_var_flow_empty_for_unknown() {
    let index = ProjectIndex::new();
    let flow = index.css_var_flow("--nonexistent");
    assert_eq!(flow.name, "--nonexistent");
    assert!(flow.style_definitions.is_empty());
    assert!(flow.style_var_usages.is_empty());
    assert!(flow.template_definitions.is_empty());
    assert!(flow.script_manipulations.is_empty());
}

#[test]
fn stats_include_var_reference_counts() {
    let mut index = ProjectIndex::new();

    // File defines --a and --b, references --a and --c
    index.add_file(
        PathBuf::from("src/A.vue"),
        FileUsageInfoOwned {
            styles: vec![StyleUsageInfoOwned {
                custom_property_names: vec!["--a".to_string(), "--b".to_string()],
                var_reference_names: vec!["--a".to_string(), "--c".to_string()],
                ..Default::default()
            }],
            ..Default::default()
        },
    );

    let stats = index.stats();
    assert_eq!(stats.unique_custom_properties, 2); // --a, --b
    assert_eq!(stats.unique_var_references, 2); // --a, --c
    assert_eq!(stats.unreferenced_custom_properties, 1); // --b (defined but not referenced)
    assert_eq!(stats.unresolved_var_references, 1); // --c (referenced but not defined)
}

#[test]
fn remove_file_cleans_up_var_indexes() {
    let mut index = ProjectIndex::new();
    index.add_file(
        PathBuf::from("src/A.vue"),
        FileUsageInfoOwned {
            styles: vec![StyleUsageInfoOwned {
                var_reference_names: vec!["--color".to_string()],
                ..Default::default()
            }],
            template_css_var_names: vec!["--size".to_string()],
            script_css_var_names: vec!["--offset".to_string()],
            ..Default::default()
        },
    );

    assert_eq!(
        index.files_referencing_custom_property("--color").count(),
        1
    );
    assert_eq!(index.files_setting_css_var_in_template("--size").count(), 1);
    assert_eq!(
        index
            .files_manipulating_css_var_in_script("--offset")
            .count(),
        1
    );

    index.remove_file(Path::new("src/A.vue"));

    assert_eq!(
        index.files_referencing_custom_property("--color").count(),
        0
    );
    assert_eq!(index.files_setting_css_var_in_template("--size").count(), 0);
    assert_eq!(
        index
            .files_manipulating_css_var_in_script("--offset")
            .count(),
        0
    );
}

// =============================================================================
// Event Flow Tests
// =============================================================================

fn make_file_with_emits(events: &[&str]) -> FileUsageInfoOwned {
    let mut info = make_file_info();
    for event in events {
        info.emit_declarations.push(EmitDeclarationOwned {
            event_name: event.to_string(),
        });
    }
    info.flags |= FileUsageFlags::HAS_DEFINE_EMITS.bits();
    info
}

fn make_file_with_listeners(events: &[(&str, Option<&str>)]) -> FileUsageInfoOwned {
    let mut info = make_file_info();
    for (event, comp) in events {
        info.listened_events.push(ListenedEventOwned {
            event_name: event.to_string(),
            component_name: comp.map(|s| s.to_string()),
        });
    }
    info
}

#[test]
fn event_flow_emit_and_listen() {
    let mut index = ProjectIndex::new();

    // ChildButton emits "click" and "close"
    index.add_file(
        PathBuf::from("/src/ChildButton.vue"),
        make_file_with_emits(&["click", "close"]),
    );

    // Parent listens for "click" on ChildButton
    index.add_file(
        PathBuf::from("/src/Parent.vue"),
        make_file_with_listeners(&[("click", Some("ChildButton"))]),
    );

    // Trace "click" flow
    let flow = index.event_flow("click");
    assert_eq!(flow.event_name, "click");
    assert_eq!(flow.emitters.len(), 1, "one file emits 'click'");
    assert_eq!(flow.listeners.len(), 1, "one file listens for 'click'");

    // Trace "close" flow — emitted but not listened
    let flow = index.event_flow("close");
    assert_eq!(flow.emitters.len(), 1);
    assert_eq!(flow.listeners.len(), 0, "no listeners for 'close'");

    // Trace unknown event
    let flow = index.event_flow("unknown");
    assert_eq!(flow.emitters.len(), 0);
    assert_eq!(flow.listeners.len(), 0);
}

#[test]
fn event_flow_all_names() {
    let mut index = ProjectIndex::new();
    index.add_file(
        PathBuf::from("/src/A.vue"),
        make_file_with_emits(&["save", "cancel"]),
    );
    index.add_file(
        PathBuf::from("/src/B.vue"),
        make_file_with_listeners(&[("save", None), ("delete", None)]),
    );

    let emit_names: Vec<&String> = index.all_emit_names().collect();
    assert!(emit_names.contains(&&"save".to_string()));
    assert!(emit_names.contains(&&"cancel".to_string()));

    let listen_names: Vec<&String> = index.all_listened_event_names().collect();
    assert!(listen_names.contains(&&"save".to_string()));
    assert!(listen_names.contains(&&"delete".to_string()));
}

#[test]
fn event_flow_remove_file_cleans_indexes() {
    let mut index = ProjectIndex::new();
    index.add_file(
        PathBuf::from("/src/A.vue"),
        make_file_with_emits(&["submit"]),
    );

    assert_eq!(index.files_emitting("submit").count(), 1);

    index.remove_file(Path::new("/src/A.vue"));
    assert_eq!(index.files_emitting("submit").count(), 0);
}

// =============================================================================
// Template ID Tests
// =============================================================================

fn make_file_with_template_ids(ids: &[&str]) -> FileUsageInfoOwned {
    let mut info = make_file_info();
    for id in ids {
        info.template_ids
            .push(TemplateIdOwned { id: id.to_string() });
    }
    info
}

#[test]
fn template_id_index_lookup() {
    let mut index = ProjectIndex::new();
    index.add_file(
        PathBuf::from("/src/App.vue"),
        make_file_with_template_ids(&["app", "modal-root"]),
    );

    assert!(index.has_template_id("app"));
    assert!(index.has_template_id("modal-root"));
    assert!(!index.has_template_id("nonexistent"));

    assert_eq!(index.files_with_template_id("app").count(), 1);
}

#[test]
fn template_id_remove_file_cleans_index() {
    let mut index = ProjectIndex::new();
    index.add_file(
        PathBuf::from("/src/App.vue"),
        make_file_with_template_ids(&["teleport-target"]),
    );

    assert!(index.has_template_id("teleport-target"));

    index.remove_file(Path::new("/src/App.vue"));
    assert!(!index.has_template_id("teleport-target"));
}

// =============================================================================
// Store Index Tests
// =============================================================================

use crate::file_usage::{StoreDefinitionOwned, StoreUsageOwned};
use crate::types::StoreApiClassification;

fn make_file_with_store_usage(callee: &str, import_source: &str) -> FileUsageInfoOwned {
    let mut info = make_file_info();
    info.store_usages.push(StoreUsageOwned {
        binding_name: "store".to_string(),
        callee: callee.to_string(),
        import_source: import_source.to_string(),
        store_api: StoreApiClassification::StoreComposable,
        has_store_to_refs: false,
        destructured_without_store_to_refs: false,
        destructured_props: Vec::new(),
        start: 0,
        end: 10,
    });
    info.flags |= FileUsageFlags::HAS_STORE_USAGE.bits();
    info
}

fn make_file_with_store_definition(store_id: &str, export_name: &str) -> FileUsageInfoOwned {
    let mut info = make_file_info();
    info.store_definitions.push(StoreDefinitionOwned {
        store_id: Some(store_id.to_string()),
        export_name: export_name.to_string(),
        store_api: StoreApiClassification::PiniaDefineStore,
        state_properties: vec!["count".to_string()],
        getters: vec!["doubleCount".to_string()],
        actions: vec!["increment".to_string()],
        start: 0,
        end: 50,
    });
    info.flags |= FileUsageFlags::HAS_STORE_DEFINITION.bits();
    info
}

/// @ai-generated - Store usage is indexed and queryable
#[test]
fn store_usage_indexed() {
    let mut index = ProjectIndex::new();
    index.add_file(
        PathBuf::from("/src/App.vue"),
        make_file_with_store_usage("useUserStore", "@/stores/user"),
    );

    let users: Vec<_> = index.files_using_store("useUserStore").collect();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].as_ref(), Path::new("/src/App.vue"));

    // Negative: non-existent store
    let users: Vec<_> = index.files_using_store("useCartStore").collect();
    assert!(users.is_empty(), "should not find non-existent store");
}

/// @ai-generated - Store definition is indexed and queryable
#[test]
fn store_definition_indexed() {
    let mut index = ProjectIndex::new();
    index.add_file(
        PathBuf::from("/src/stores/user.ts"),
        make_file_with_store_definition("user", "useUserStore"),
    );

    let def_file = index.store_defined_in("user");
    assert!(def_file.is_some());
    assert_eq!(def_file.unwrap().as_ref(), Path::new("/src/stores/user.ts"));

    // Negative: non-existent store
    assert!(
        index.store_defined_in("cart").is_none(),
        "should not find non-existent store"
    );
}

/// @ai-generated - all_store_ids returns all defined store IDs
#[test]
fn all_store_ids_query() {
    let mut index = ProjectIndex::new();
    index.add_file(
        PathBuf::from("/src/stores/user.ts"),
        make_file_with_store_definition("user", "useUserStore"),
    );
    index.add_file(
        PathBuf::from("/src/stores/cart.ts"),
        make_file_with_store_definition("cart", "useCartStore"),
    );

    let ids: Vec<&String> = index.all_store_ids().collect();
    assert_eq!(ids.len(), 2);
    assert!(ids.iter().any(|id| *id == "user"));
    assert!(ids.iter().any(|id| *id == "cart"));
}

/// @ai-generated - Removing a file cleans up store indexes
#[test]
fn store_remove_file_cleans_indexes() {
    let mut index = ProjectIndex::new();
    index.add_file(
        PathBuf::from("/src/stores/user.ts"),
        make_file_with_store_definition("user", "useUserStore"),
    );
    index.add_file(
        PathBuf::from("/src/App.vue"),
        make_file_with_store_usage("useUserStore", "@/stores/user"),
    );

    assert!(index.store_defined_in("user").is_some());
    assert_eq!(index.files_using_store("useUserStore").count(), 1);

    // Remove definition file
    index.remove_file(Path::new("/src/stores/user.ts"));
    assert!(
        index.store_defined_in("user").is_none(),
        "definition should be removed"
    );

    // Remove usage file
    index.remove_file(Path::new("/src/App.vue"));
    assert_eq!(
        index.files_using_store("useUserStore").count(),
        0,
        "usage should be removed"
    );
}

/// @ai-generated - store_flow traces definition + consumers
#[test]
fn store_flow_traces_correctly() {
    let mut index = ProjectIndex::new();
    index.add_file(
        PathBuf::from("/src/stores/user.ts"),
        make_file_with_store_definition("user", "useUserStore"),
    );
    index.add_file(
        PathBuf::from("/src/App.vue"),
        make_file_with_store_usage("useUserStore", "@/stores/user"),
    );

    let flow = index.store_flow("user");
    assert_eq!(flow.store_id, "user");
    assert!(flow.definition_file.is_some());
    assert!(!flow.consumer_files.is_empty());
}
